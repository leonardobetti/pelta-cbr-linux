//! Auto levels: one black point and one white point per page, taken from the
//! luminance histogram and applied to the page as a 256-entry lookup table.
//!
//! CLAHE works on regions and can amplify grain and draw halos around ink.
//! This is one global linear stretch, so the result is predictable: grey
//! paper becomes white and weak blacks become black.
//!
//! Colour: the points come from luminance and the same table is applied to
//! R, G and B. An equal linear map on all three channels keeps the hue of
//! every pixel that does not clip. Stretching each channel on its own would
//! change the colour balance.
//!
//! The histogram comes from a nearest-sampled proxy (long side at most
//! [`PROXY_LONG_SIDE`]). The table is applied to the full image it is given,
//! which in the pipeline is the page at display size.

use image::RgbaImage;

/// Share of the darkest pixels ignored when the black point is chosen, so a
/// few outlier pixels (dust, a stray black dot) cannot decide the stretch.
pub const LOW_PERCENTILE: f64 = 0.005;
/// The white point leaves out the same share of the brightest pixels.
pub const HIGH_PERCENTILE: f64 = 0.995;

/// Long side of the sampling proxy the histogram is taken from.
pub const PROXY_LONG_SIDE: u32 = 512;

/// A page whose black point is at most this, and whose white point is at
/// least 255 minus this, already uses the full range and is left alone.
pub const FULL_RANGE_MARGIN: u8 = 4;

/// Smallest distance between the black and white points that is stretched.
/// Flatter pages (blank paper, fog, one flat tone) would only show amplified
/// grain and banding.
pub const MIN_SPREAD: u8 = 48;

/// Pages whose white point is below this are very dark (night scenes, black
/// pages). Stretching them would blow them out, so they are left alone.
pub const MIN_WHITE_POINT: u8 = 96;

/// Pixels with less alpha than this do not count towards the histogram, as
/// in matte detection.
pub const MIN_OPAQUE_ALPHA: u8 = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Levels {
    pub black: u8,
    pub white: u8,
}

/// Rec. 709 luma of 8-bit sRGB values, with the same weights as CLAHE.
pub fn luma(r: u8, g: u8, b: u8) -> u8 {
    (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32)
        .round()
        .clamp(0.0, 255.0) as u8
}

/// Luma histogram of a nearest-sampled proxy of `img`, and its pixel count.
pub fn proxy_histogram(img: &RgbaImage) -> ([u32; 256], u32) {
    let mut hist = [0u32; 256];
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return (hist, 0);
    }
    let long = w.max(h);
    let (pw, ph) = if long <= PROXY_LONG_SIDE {
        (w, h)
    } else {
        let scale =
            |side: u32| ((side as u64 * PROXY_LONG_SIDE as u64) / long as u64).max(1) as u32;
        (scale(w), scale(h))
    };
    let mut total = 0;
    for py in 0..ph {
        let y = (py as u64 * h as u64 / ph as u64) as u32;
        for px in 0..pw {
            let x = (px as u64 * w as u64 / pw as u64) as u32;
            let p = img.get_pixel(x, y);
            if p[3] < MIN_OPAQUE_ALPHA {
                continue;
            }
            hist[luma(p[0], p[1], p[2]) as usize] += 1;
            total += 1;
        }
    }
    (hist, total)
}

/// The lowest value with more than `share` of `total` at or below it.
fn point_from_bottom(hist: impl Iterator<Item = (usize, u32)>, total: u32, share: f64) -> u8 {
    let target = share * total as f64;
    let mut seen = 0u64;
    let mut last = 0;
    for (value, count) in hist {
        seen += count as u64;
        last = value;
        if seen as f64 > target {
            break;
        }
    }
    last as u8
}

/// Black and white points at [`LOW_PERCENTILE`] and [`HIGH_PERCENTILE`], or
/// `None` for an empty histogram. The white point is counted from the top,
/// so both ends leave out the same share of pixels.
pub fn percentiles(hist: &[u32; 256], total: u32) -> Option<Levels> {
    if total == 0 {
        return None;
    }
    let black = point_from_bottom(hist.iter().copied().enumerate(), total, LOW_PERCENTILE);
    let white = point_from_bottom(
        hist.iter().copied().enumerate().rev(),
        total,
        1.0 - HIGH_PERCENTILE,
    );
    Some(Levels { black, white })
}

/// The points to stretch `img` with, or `None` when it should be left alone:
/// empty or fully transparent, very dark, flat, or already full range.
pub fn measure(img: &RgbaImage) -> Option<Levels> {
    let (hist, total) = proxy_histogram(img);
    let levels = percentiles(&hist, total)?;
    let skip = levels.white < MIN_WHITE_POINT
        || levels.white.saturating_sub(levels.black) < MIN_SPREAD
        || (levels.black <= FULL_RANGE_MARGIN && levels.white >= 255 - FULL_RANGE_MARGIN);
    (!skip).then_some(levels)
}

/// Linear map from `black..=white` to `0..=255`, clamped outside it.
pub fn lut(levels: Levels) -> [u8; 256] {
    let black = levels.black as i32;
    let span = (levels.white as i32 - black).max(1);
    let mut table = [0u8; 256];
    for (v, out) in table.iter_mut().enumerate() {
        let num = (v as i32 - black).clamp(0, span) * 255;
        *out = ((num + span / 2) / span) as u8;
    }
    table
}

/// Applies `table` to R, G and B. Alpha is unchanged.
pub fn apply_lut(img: &mut RgbaImage, table: &[u8; 256]) {
    for p in img.pixels_mut() {
        p[0] = table[p[0] as usize];
        p[1] = table[p[1] as usize];
        p[2] = table[p[2] as usize];
    }
}

/// Measures and stretches `img` in place. Returns the points used, or `None`
/// when the page was left alone.
pub fn auto_levels(img: &mut RgbaImage) -> Option<Levels> {
    let levels = measure(img)?;
    apply_lut(img, &lut(levels));
    Some(levels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_proc::fixtures;
    use image::Rgba;

    fn grey(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            let v = f(x, y);
            Rgba([v, v, v, 255])
        })
    }

    fn hist_of(values: &[(u8, u32)]) -> ([u32; 256], u32) {
        let mut hist = [0u32; 256];
        let mut total = 0;
        for &(v, n) in values {
            hist[v as usize] += n;
            total += n;
        }
        (hist, total)
    }

    /// HSV hue in degrees.
    fn hue(p: &Rgba<u8>) -> f64 {
        let [r, g, b] = [p[0] as f64, p[1] as f64, p[2] as f64];
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let c = max - min;
        let h = if max == r {
            ((g - b) / c).rem_euclid(6.0)
        } else if max == g {
            (b - r) / c + 2.0
        } else {
            (r - g) / c + 4.0
        };
        h * 60.0
    }

    #[test]
    fn percentiles_on_a_uniform_histogram() {
        let values: Vec<(u8, u32)> = (0..=255u8).map(|v| (v, 100)).collect();
        let (hist, total) = hist_of(&values);
        // 0.5% of 25600 is 128 pixels: just over one bin at each end.
        assert_eq!(
            percentiles(&hist, total),
            Some(Levels {
                black: 1,
                white: 254
            })
        );
    }

    #[test]
    fn percentiles_ignore_outliers() {
        let (hist, total) = hist_of(&[(0, 5), (60, 495), (200, 495), (255, 5)]);
        assert_eq!(
            percentiles(&hist, total),
            Some(Levels {
                black: 60,
                white: 200
            })
        );
    }

    #[test]
    fn percentiles_of_an_empty_histogram() {
        assert_eq!(percentiles(&[0; 256], 0), None);
        let (hist, total) = hist_of(&[(77, 1)]);
        assert_eq!(
            percentiles(&hist, total),
            Some(Levels {
                black: 77,
                white: 77
            })
        );
    }

    #[test]
    fn lut_maps_the_points_to_the_ends() {
        let table = lut(Levels {
            black: 50,
            white: 200,
        });
        assert_eq!(table[0], 0);
        assert_eq!(table[50], 0);
        assert_eq!(table[125], 128);
        assert_eq!(table[200], 255);
        assert_eq!(table[255], 255);
        assert!(table.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn lut_is_the_identity_for_the_full_range() {
        let table = lut(Levels {
            black: 0,
            white: 255,
        });
        assert!(table.iter().enumerate().all(|(i, &v)| i == v as usize));
    }

    #[test]
    fn the_same_table_goes_to_every_channel() {
        let mut img = RgbaImage::from_pixel(4, 4, Rgba([100, 150, 200, 77]));
        let table = lut(Levels {
            black: 50,
            white: 250,
        });
        apply_lut(&mut img, &table);
        let p = img.get_pixel(0, 0);
        assert_eq!(
            p.0,
            [table[100], table[150], table[200], 77],
            "alpha must not change"
        );
    }

    #[test]
    fn grey_scan_is_stretched() {
        let levels = measure(&fixtures::grey_scan()).expect("grey scan needs levels");
        assert!(levels.black > FULL_RANGE_MARGIN, "{levels:?}");
        assert!(levels.white < 255 - FULL_RANGE_MARGIN, "{levels:?}");

        let mut img = fixtures::grey_scan();
        auto_levels(&mut img);
        let paper = img.get_pixel(4, 4);
        assert!(paper.0[..3].iter().all(|&c| c >= 235), "paper {paper:?}");
        let ink = img.get_pixel(120, 24);
        assert!(ink.0[..3].iter().all(|&c| c <= 20), "ink {ink:?}");
    }

    #[test]
    fn full_range_pages_are_skipped() {
        let img = grey(256, 8, |x, _| x as u8);
        assert_eq!(measure(&img), None);
    }

    #[test]
    fn flat_pages_are_skipped() {
        let img = grey(64, 64, |x, y| 120 + ((x + y) % 7) as u8);
        assert_eq!(measure(&img), None);
    }

    #[test]
    fn dark_pages_are_skipped() {
        let img = grey(64, 64, |x, y| 5 + ((x * 3 + y) % 80) as u8);
        assert_eq!(measure(&img), None);
    }

    #[test]
    fn flat_image_stays_unchanged() {
        let mut img = grey(32, 32, |_, _| 128);
        let before = img.clone();
        assert_eq!(auto_levels(&mut img), None);
        assert_eq!(img, before);
    }

    #[test]
    fn transparent_pixels_do_not_count() {
        let img = RgbaImage::from_pixel(16, 16, Rgba([90, 90, 90, 0]));
        assert_eq!(measure(&img), None);
    }

    #[test]
    fn tiny_and_extreme_images_are_safe() {
        assert_eq!(measure(&RgbaImage::new(0, 0)), None);
        assert_eq!(measure(&RgbaImage::new(0, 10)), None);
        assert_eq!(measure(&grey(1, 1, |_, _| 200)), None);
        let (_, total) = proxy_histogram(&grey(5000, 1, |x, _| (x % 256) as u8));
        assert_eq!(total, PROXY_LONG_SIDE);
        let (_, total) = proxy_histogram(&grey(1, 5000, |_, y| (y % 256) as u8));
        assert_eq!(total, PROXY_LONG_SIDE);
        let mut thin = grey(1, 300, |_, y| 60 + (y % 140) as u8);
        assert!(auto_levels(&mut thin).is_some());
    }

    #[test]
    fn colour_gradient_keeps_its_hue() {
        let page = RgbaImage::from_fn(200, 120, |x, y| {
            if y < 20 {
                Rgba([190, 186, 180, 255])
            } else if y < 30 {
                Rgba([50, 48, 46, 255])
            } else {
                let t = x as f64 / 199.0;
                let r = (70.0 + 110.0 * t) as u8;
                let g = (150.0 - 60.0 * t + (y % 30) as f64) as u8;
                let b = (170.0 - 100.0 * t) as u8;
                Rgba([r, g, b, 255])
            }
        });
        let mut out = page.clone();
        assert!(auto_levels(&mut out).is_some());
        let mut checked = 0;
        for (before, after) in page.pixels().zip(out.pixels()) {
            let chroma = |p: &Rgba<u8>| {
                let c = &p.0[..3];
                c.iter().max().unwrap() - c.iter().min().unwrap()
            };
            let clipped = after.0[..3].iter().any(|&c| c == 0 || c == 255);
            if chroma(before) < 30 || clipped {
                continue;
            }
            let d = (hue(before) - hue(after)).abs();
            let d = d.min(360.0 - d);
            assert!(d <= 2.0, "hue moved {d:.2}° from {before:?} to {after:?}");
            checked += 1;
        }
        assert!(checked > 10_000, "only {checked} pixels checked");
    }

    #[test]
    fn applying_twice_is_the_same_as_once() {
        let mut once = fixtures::grey_scan();
        auto_levels(&mut once);
        let mut twice = once.clone();
        auto_levels(&mut twice);
        let max_diff = once
            .as_raw()
            .iter()
            .zip(twice.as_raw())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(max_diff <= 2, "second pass moved a channel by {max_diff}");
    }
}
