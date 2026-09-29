//! Page matte detection: the dominant colour of a page's border, used as the
//! reader background so the page appears to sit on its own paper.
//!
//! Detection reads a band along the page edges of a nearest-sampled proxy
//! (long side ≤ [`PROXY_LONG_SIDE`]), takes the mode of a 5-bit-per-channel
//! histogram, and refines it with the per-channel median of the pixels in and
//! around the mode bin. The result is only used when enough of the band agrees
//! with it ([`MIN_CONFIDENCE`]); otherwise the reader keeps the theme
//! background.
//!
//! Transparency: pixels with alpha below [`MIN_OPAQUE_ALPHA`] never vote but
//! still count towards the band size. Transparent areas show the reader
//! background through them, so a page whose border is mostly transparent falls
//! back to the theme background instead of taking the colour of a few opaque
//! pixels. Pixels at or above the threshold vote with their colour as stored
//! (not composited), which is what the reader shows for an opaque border.

use image::RgbaImage;

/// Long side of the sampling proxy. Borders are large, flat areas, so 256 px
/// is plenty and keeps detection far below a millisecond.
pub const PROXY_LONG_SIDE: u32 = 256;

/// Width of the sampled band, as a fraction of the proxy's short side (with a
/// minimum of [`MIN_BAND_PX`]). 3% of a page is inside the paper margin of a
/// printed comic and inside the black or white matte of a digital one.
pub const BAND_FRACTION: f32 = 0.03;
pub const MIN_BAND_PX: u32 = 2;

/// Alpha below which a pixel is treated as transparent.
pub const MIN_OPAQUE_ALPHA: u8 = 128;

/// Largest per-channel difference from the matte colour that still counts as
/// "the same colour". Covers JPEG noise, paper grain and slight uneven
/// yellowing in scans without merging distinct colours.
pub const COLOUR_TOLERANCE: u8 = 16;

/// Minimum share of the band that must match the detected colour.
///
/// A uniform border (paper, black or white matte, flat colour) scores close to
/// 1: the scanned 1907 page in the tests scores 1.0. Pages without one spread
/// the band over many colours and stay well under half: 0.34 for full-bleed
/// art, 0.24 for a page cropped into its panels, 0.39 for a grainy dark
/// gradient. Art that bleeds off a single edge still leaves three clean edges,
/// about 0.75 of the band, and keeps its matte, which is the desired result.
/// 0.6 sits between those cases: it accepts one bleeding edge but rejects
/// pages that bleed on two or more, where any single colour would look
/// arbitrary.
pub const MIN_CONFIDENCE: f32 = 0.6;

const BITS: u32 = 5;
const LEVELS: usize = 1 << BITS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    /// WCAG 2 relative luminance.
    pub fn relative_luminance(self) -> f64 {
        let lin = |c: u8| {
            let v = c as f64 / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        let [r, g, b] = self.0;
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    }

    /// True when dark text has more contrast on this colour than white text.
    /// 0.179 is where the WCAG contrast ratios against black and white meet.
    pub fn prefers_dark_foreground(self) -> bool {
        self.relative_luminance() > 0.179
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Detection {
    pub colour: Rgb,
    /// Share of the sampled band within [`COLOUR_TOLERANCE`] of `colour`.
    pub confidence: f32,
}

impl Detection {
    pub fn matte(self) -> Option<Rgb> {
        (self.confidence >= MIN_CONFIDENCE).then_some(self.colour)
    }
}

/// Which edges of an image belong to the outside of the page (or spread).
#[derive(Debug, Clone, Copy)]
struct Edges {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

const ALL_EDGES: Edges = Edges {
    left: true,
    right: true,
    top: true,
    bottom: true,
};

/// Matte for a single page.
pub fn detect_page(page: &RgbaImage) -> Option<Rgb> {
    analyse_page(page)?.matte()
}

/// One matte for a two-page spread, taken from the outer edges of the combined
/// spread: the gutter edges (left page's right edge, right page's left edge)
/// are ignored.
pub fn detect_spread(left: &RgbaImage, right: &RgbaImage) -> Option<Rgb> {
    analyse_spread(left, right)?.matte()
}

pub fn analyse_page(page: &RgbaImage) -> Option<Detection> {
    let mut samples = Vec::new();
    sample_band(page, ALL_EDGES, &mut samples);
    analyse_samples(&samples)
}

pub fn analyse_spread(left: &RgbaImage, right: &RgbaImage) -> Option<Detection> {
    let mut samples = Vec::new();
    sample_band(
        left,
        Edges {
            right: false,
            ..ALL_EDGES
        },
        &mut samples,
    );
    sample_band(
        right,
        Edges {
            left: false,
            ..ALL_EDGES
        },
        &mut samples,
    );
    analyse_samples(&samples)
}

fn proxy_size(w: u32, h: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= PROXY_LONG_SIDE {
        return (w, h);
    }
    let s = PROXY_LONG_SIDE as f64 / long as f64;
    (
        ((w as f64 * s).round() as u32).max(1),
        ((h as f64 * s).round() as u32).max(1),
    )
}

/// Appends the band pixels of the nearest-sampled proxy; `None` marks a
/// transparent pixel.
fn sample_band(img: &RgbaImage, edges: Edges, out: &mut Vec<Option<[u8; 3]>>) {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    let (pw, ph) = proxy_size(w, h);
    let band = ((pw.min(ph) as f32 * BAND_FRACTION).round() as u32)
        .max(MIN_BAND_PX)
        .min(pw.min(ph).div_ceil(2));
    let src_x = |px: u32| (((px as u64 * 2 + 1) * w as u64) / (pw as u64 * 2)) as u32;
    let src_y = |py: u32| (((py as u64 * 2 + 1) * h as u64) / (ph as u64 * 2)) as u32;
    for py in 0..ph {
        let in_row_band = (edges.top && py < band) || (edges.bottom && py >= ph - band);
        for px in 0..pw {
            let in_band =
                in_row_band || (edges.left && px < band) || (edges.right && px >= pw - band);
            if !in_band {
                continue;
            }
            let p = img.get_pixel(src_x(px), src_y(py)).0;
            out.push((p[3] >= MIN_OPAQUE_ALPHA).then_some([p[0], p[1], p[2]]));
        }
    }
}

fn bin_of(c: [u8; 3]) -> [usize; 3] {
    c.map(|v| (v >> (8 - BITS)) as usize)
}

fn analyse_samples(samples: &[Option<[u8; 3]>]) -> Option<Detection> {
    if samples.is_empty() {
        return None;
    }
    let mut hist = vec![0u32; LEVELS * LEVELS * LEVELS];
    let index = |[r, g, b]: [usize; 3]| (r * LEVELS + g) * LEVELS + b;
    for c in samples.iter().flatten() {
        hist[index(bin_of(*c))] += 1;
    }
    let (mode, &mode_count) = hist.iter().enumerate().max_by_key(|&(_, n)| *n)?;
    if mode_count == 0 {
        return None;
    }
    let mode = [
        mode / (LEVELS * LEVELS),
        (mode / LEVELS) % LEVELS,
        mode % LEVELS,
    ];

    // Colours near a bin boundary split across neighbouring bins, so the median
    // includes the mode bin's immediate neighbours.
    let near_mode = |c: &[u8; 3]| {
        bin_of(*c)
            .iter()
            .zip(mode)
            .all(|(&b, m)| b.abs_diff(m) <= 1)
    };
    let mut channel_hist = [[0u32; 256]; 3];
    let mut n = 0u32;
    for c in samples.iter().flatten().filter(|c| near_mode(c)) {
        for (h, &v) in channel_hist.iter_mut().zip(c) {
            h[v as usize] += 1;
        }
        n += 1;
    }
    let median = |h: &[u32; 256]| {
        let mut seen = 0;
        for (v, &count) in h.iter().enumerate() {
            seen += count;
            if seen * 2 > n {
                return v as u8;
            }
        }
        255
    };
    let colour = [
        median(&channel_hist[0]),
        median(&channel_hist[1]),
        median(&channel_hist[2]),
    ];

    let matching = samples
        .iter()
        .flatten()
        .filter(|c| {
            c.iter()
                .zip(colour)
                .all(|(&a, b)| a.abs_diff(b) <= COLOUR_TOLERANCE)
        })
        .count();
    Some(Detection {
        colour: Rgb(colour),
        confidence: matching as f32 / samples.len() as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    use std::path::Path;
    use std::time::Instant;

    fn fixture(name: &str) -> RgbaImage {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/matte")
            .join(name);
        image::open(&path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            .to_rgba8()
    }

    /// A page with a `border`-wide margin of `matte` around busy content.
    fn framed(w: u32, h: u32, border: u32, matte: [u8; 4]) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            if x < border || y < border || x >= w - border || y >= h - border {
                Rgba(matte)
            } else {
                noise(x, y)
            }
        })
    }

    fn noise(x: u32, y: u32) -> Rgba<u8> {
        let v = x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503).rotate_left(7);
        Rgba([(v >> 3) as u8, (v >> 11) as u8, (v >> 19) as u8, 255])
    }

    fn close(a: Rgb, b: [u8; 3], tol: u8) -> bool {
        a.0.iter().zip(b).all(|(&x, y)| x.abs_diff(y) <= tol)
    }

    #[test]
    fn flat_border_is_detected_exactly() {
        for matte in [[255, 255, 255], [0, 0, 0], [243, 232, 205], [30, 60, 120]] {
            let page = framed(900, 1300, 60, [matte[0], matte[1], matte[2], 255]);
            let d = analyse_page(&page).unwrap();
            assert_eq!(d.colour, Rgb(matte));
            assert!(d.confidence > 0.99, "{matte:?}: {}", d.confidence);
        }
    }

    #[test]
    fn noisy_paper_gives_its_median_colour() {
        let base = [240i32, 228, 200];
        let page = RgbaImage::from_fn(800, 1200, |x, y| {
            let n = (noise(x, y).0[0] as i32 % 13) - 6;
            Rgba([
                (base[0] + n) as u8,
                (base[1] + n) as u8,
                (base[2] + n) as u8,
                255,
            ])
        });
        let m = detect_page(&page).unwrap();
        assert!(close(m, [240, 228, 200], 2), "{m:?}");
    }

    #[test]
    fn full_bleed_noise_has_no_matte() {
        let page = RgbaImage::from_fn(800, 1200, noise);
        let d = analyse_page(&page).unwrap();
        assert!(d.confidence < 0.1, "{}", d.confidence);
        assert_eq!(d.matte(), None);
    }

    #[test]
    fn one_bleeding_edge_keeps_the_matte_two_do_not() {
        let white = [255, 255, 255, 255];
        // White margin on the left, top and bottom; art runs off the right edge.
        let one = RgbaImage::from_fn(800, 1200, |x, y| {
            if x >= 40 && (40..1160).contains(&y) {
                noise(x, y)
            } else {
                Rgba(white)
            }
        });
        assert_eq!(detect_page(&one), Some(Rgb([255, 255, 255])));

        // Art runs off the right and bottom edges.
        let two = RgbaImage::from_fn(800, 1200, |x, y| {
            if x >= 40 && y >= 40 {
                noise(x, y)
            } else {
                Rgba(white)
            }
        });
        assert_eq!(detect_page(&two), None);
    }

    #[test]
    fn values_straddling_a_bin_edge_are_not_split() {
        // 7 and 8 fall in different 5-bit bins; the median still finds the pair.
        let page = RgbaImage::from_fn(600, 900, |x, y| {
            let v = if (x + y) % 2 == 0 { 7 } else { 8 };
            Rgba([v, v, v, 255])
        });
        let d = analyse_page(&page).unwrap();
        assert!(close(d.colour, [7, 7, 7], 1), "{:?}", d.colour);
        assert!(d.confidence > 0.99);
    }

    #[test]
    fn transparent_border_falls_back() {
        let page = framed(800, 1200, 60, [255, 0, 0, 0]);
        assert_eq!(detect_page(&page), None);
    }

    #[test]
    fn opaque_border_on_image_with_alpha_is_used() {
        let page = RgbaImage::from_fn(800, 1200, |x, y| {
            if x < 60 || y < 60 || x >= 740 || y >= 1140 {
                Rgba([20, 20, 20, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });
        assert_eq!(detect_page(&page), Some(Rgb([20, 20, 20])));
    }

    #[test]
    fn mostly_transparent_border_with_few_opaque_pixels_falls_back() {
        let page = RgbaImage::from_fn(800, 1200, |x, y| {
            let a = if (x + y) % 3 == 0 { 255 } else { 0 };
            Rgba([200, 30, 30, a])
        });
        let d = analyse_page(&page).unwrap();
        assert!(d.confidence < 0.4, "{}", d.confidence);
        assert_eq!(d.matte(), None);
    }

    #[test]
    fn spread_ignores_the_gutter() {
        let cream = [243, 232, 205, 255];
        // Each page's gutter side is busy art; the outer edges are cream.
        let left = RgbaImage::from_fn(700, 1000, |x, y| {
            if x >= 640 {
                noise(x, y)
            } else if x < 40 || y < 40 || y >= 960 {
                Rgba(cream)
            } else {
                noise(x, y)
            }
        });
        let right = RgbaImage::from_fn(700, 1000, |x, y| {
            if x < 60 {
                noise(x, y)
            } else if x >= 660 || y < 40 || y >= 960 {
                Rgba(cream)
            } else {
                noise(x, y)
            }
        });
        let d = analyse_spread(&left, &right).unwrap();
        assert_eq!(d.colour, Rgb([243, 232, 205]));
        assert!(d.confidence > 0.95, "{}", d.confidence);
    }

    #[test]
    fn spread_of_two_different_pages_takes_the_dominant_edge_colour() {
        let white = framed(700, 1000, 50, [255, 255, 255, 255]);
        let black = framed(700, 1000, 50, [0, 0, 0, 255]);
        let d = analyse_spread(&white, &black).unwrap();
        assert!(d.confidence < MIN_CONFIDENCE, "{}", d.confidence);
    }

    #[test]
    fn tiny_and_empty_images() {
        assert_eq!(analyse_page(&RgbaImage::new(0, 0)), None);
        let one = RgbaImage::from_pixel(1, 1, Rgba([9, 8, 7, 255]));
        assert_eq!(detect_page(&one), Some(Rgb([9, 8, 7])));
        let thin = RgbaImage::from_pixel(3, 500, Rgba([1, 2, 3, 255]));
        assert_eq!(detect_page(&thin), Some(Rgb([1, 2, 3])));
    }

    #[test]
    fn foreground_follows_luminance() {
        assert!(Rgb([255, 255, 255]).prefers_dark_foreground());
        assert!(Rgb([243, 232, 205]).prefers_dark_foreground());
        assert!(!Rgb([0, 0, 0]).prefers_dark_foreground());
        assert!(!Rgb([30, 60, 120]).prefers_dark_foreground());
        assert!((Rgb([255, 255, 255]).relative_luminance() - 1.0).abs() < 1e-9);
        assert!(Rgb([0, 0, 0]).relative_luminance().abs() < 1e-9);
    }

    #[test]
    fn real_page_with_paper_margin_is_cream() {
        let d = analyse_page(&fixture("nemo-1907-page.jpg")).unwrap();
        let [r, g, b] = d.colour.0;
        assert!(
            r > 220 && g > 210 && b > 170 && r > b + 15,
            "{:?}",
            d.colour
        );
        assert!(d.confidence > 0.85, "{}", d.confidence);
        assert!(d.colour.prefers_dark_foreground());
    }

    #[test]
    fn real_full_bleed_art_has_no_matte() {
        let d = analyse_page(&fixture("nemo-1907-full-bleed.jpg")).unwrap();
        assert!(d.confidence < 0.45, "{:?}", d);
        assert_eq!(d.matte(), None);
    }

    /// Grainy night sky whose edges range from very dark to mid brown: no
    /// single colour represents it, so the theme background is kept.
    #[test]
    fn real_uneven_dark_print_falls_back() {
        let d = analyse_page(&fixture("nemo-1905-dark-panel.jpg")).unwrap();
        assert!(d.colour.relative_luminance() < 0.1, "{d:?}");
        assert_eq!(d.matte(), None, "{d:?}");
    }

    /// Digital editions often letterbox the scan on black.
    #[test]
    fn real_page_on_black_matte_is_black() {
        let page = fixture("nemo-1907-page.jpg");
        let (w, h) = page.dimensions();
        let pad = w / 12;
        let mut boxed = RgbaImage::from_pixel(w + 2 * pad, h + 2 * pad, Rgba([0, 0, 0, 255]));
        image::imageops::overlay(&mut boxed, &page, pad as i64, pad as i64);
        let d = analyse_page(&boxed).unwrap();
        assert_eq!(d.matte(), Some(Rgb([0, 0, 0])), "{d:?}");
        assert!(!d.colour.prefers_dark_foreground());
    }

    #[test]
    #[ignore]
    fn print_fixture_confidences() {
        for name in [
            "nemo-1907-page.jpg",
            "nemo-1905-page.jpg",
            "nemo-1907-full-bleed.jpg",
            "nemo-1905-dark-panel.jpg",
        ] {
            println!("{name}: {:?}", analyse_page(&fixture(name)).unwrap());
        }
    }

    #[test]
    fn real_tightly_cropped_page_falls_back() {
        let d = analyse_page(&fixture("nemo-1905-page.jpg")).unwrap();
        assert_eq!(d.matte(), None, "{d:?}");
    }

    #[test]
    fn real_spread_uses_outer_edges() {
        let page = fixture("nemo-1907-page.jpg");
        let art = fixture("nemo-1907-full-bleed.jpg");
        let single = analyse_page(&page).unwrap();
        let spread = analyse_spread(&page, &page).unwrap();
        assert!(close(spread.colour, single.colour.0, 3));
        assert_eq!(detect_spread(&art, &art), None);
    }

    #[test]
    fn detection_is_fast_on_a_full_size_page() {
        let page = framed(3000, 4500, 150, [243, 232, 205, 255]);
        detect_page(&page);
        let mut runs: Vec<f64> = (0..15)
            .map(|_| {
                let t = Instant::now();
                std::hint::black_box(detect_page(&page));
                t.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        runs.sort_by(f64::total_cmp);
        let median = runs[runs.len() / 2];
        assert!(median < 5.0, "median {median:.3} ms");
    }
}
