//! Opt-in page image processing: linear-light Lanczos3 / Mitchell, or CLAHE.
//!
//! Exactly one mode is active in the UI (Nothing / Lanczos3 / Mitchell /
//! Auto contrast). When enabled, work runs at display size only, off the UI
//! thread, with a small bounded cache (no unbounded buffers).

use std::collections::HashMap;

use fast_image_resize::images::Image as FirImage;
use fast_image_resize::{
    create_srgb_mapper, FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer,
};
use image::imageops::FilterType as ImageFilter;
use image::{DynamicImage, RgbaImage};

use crate::auto_levels::auto_levels;
use crate::settings::{ImageProcessingSettings, ScalingMode};

/// Hard cap on cached processed pages (visible + neighbors + headroom).
const CACHE_CAP: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub page: usize,
    pub width: u32,
    pub height: u32,
    pub scaling: ScalingMode,
    pub auto_contrast: bool,
    pub auto_levels: bool,
}

#[derive(Debug, Clone)]
pub struct ProcessedPage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Bounded LRU-ish cache: insert evicts oldest when over capacity.
#[derive(Default)]
pub struct ProcessCache {
    order: Vec<CacheKey>,
    map: HashMap<CacheKey, ProcessedPage>,
}

impl ProcessCache {
    pub fn get(&mut self, key: &CacheKey) -> Option<ProcessedPage> {
        if let Some(page) = self.map.get(key) {
            // Refresh LRU order.
            if let Some(i) = self.order.iter().position(|k| k == key) {
                let k = self.order.remove(i);
                self.order.push(k);
            }
            return Some(page.clone());
        }
        None
    }

    pub fn insert(&mut self, key: CacheKey, page: ProcessedPage) {
        if self.map.contains_key(&key) {
            self.map.insert(key, page);
            return;
        }
        while self.map.len() >= CACHE_CAP {
            if let Some(old) = self.order.first().copied() {
                self.order.remove(0);
                self.map.remove(&old);
            } else {
                break;
            }
        }
        self.order.push(key);
        self.map.insert(key, page);
    }

    pub fn clear(&mut self) {
        self.order.clear();
        self.map.clear();
    }
}

/// Fit `src` into `max` preserving aspect (may upscale for display-size processing).
pub fn fit_contain(src_w: u32, src_h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if src_w == 0 || src_h == 0 || max_w == 0 || max_h == 0 {
        return (src_w.max(1), src_h.max(1));
    }
    let scale = (max_w as f64 / src_w as f64).min(max_h as f64 / src_h as f64);
    let w = ((src_w as f64) * scale).round().max(1.0) as u32;
    let h = ((src_h as f64) * scale).round().max(1.0) as u32;
    (w, h)
}

/// Decode encoded page bytes → RGBA8.
pub fn decode_rgba(bytes: &[u8]) -> Result<RgbaImage, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    Ok(img.to_rgba8())
}

/// Full pipeline: decode → resize → optional Auto levels → optional CLAHE →
/// RGBA bytes at display size.
pub fn process_page(
    encoded: &[u8],
    display_w: u32,
    display_h: u32,
    settings: ImageProcessingSettings,
) -> Result<ProcessedPage, String> {
    let src = decode_rgba(encoded)?;
    let (tw, th) = fit_contain(src.width(), src.height(), display_w, display_h);

    let mut rgba = match settings.scaling {
        ScalingMode::Nothing => {
            // Still resize when auto-contrast needs a display-size buffer.
            if (tw, th) != (src.width(), src.height()) {
                resize_image_crate(&src, tw, th, ImageFilter::Triangle)
            } else {
                src
            }
        }
        ScalingMode::Lanczos3 => resize_convolution_linear(&src, tw, th, FilterType::Lanczos3)?,
        ScalingMode::MitchellNetravali => {
            resize_convolution_linear(&src, tw, th, FilterType::Mitchell)?
        }
    };

    if settings.auto_levels {
        auto_levels(&mut rgba);
    }

    if settings.auto_contrast {
        clahe_luminance(&mut rgba, 8, 8, 2.0);
    }

    let width = rgba.width();
    let height = rgba.height();
    Ok(ProcessedPage {
        width,
        height,
        rgba: rgba.into_raw(),
    })
}

fn resize_image_crate(src: &RgbaImage, tw: u32, th: u32, filter: ImageFilter) -> RgbaImage {
    DynamicImage::ImageRgba8(src.clone())
        .resize_exact(tw, th, filter)
        .to_rgba8()
}

/// Convolution resize in linear light via `create_srgb_mapper`.
///
/// Used for Lanczos3 and Mitchell–Netravali. Alpha is cast (not gamma-mapped);
/// premultiply/divide stays on by default for x2/x4 pixel types.
fn resize_convolution_linear(
    src: &RgbaImage,
    tw: u32,
    th: u32,
    filter: FilterType,
) -> Result<RgbaImage, String> {
    let sw = src.width();
    let sh = src.height();
    if sw == tw && sh == th {
        return Ok(src.clone());
    }

    let src_fir = FirImage::from_vec_u8(sw, sh, src.as_raw().to_vec(), PixelType::U8x4)
        .map_err(|e| e.to_string())?;

    let mapper = create_srgb_mapper();
    let mut src_linear = FirImage::new(sw, sh, PixelType::U16x4);
    mapper
        .forward_map(&src_fir, &mut src_linear)
        .map_err(|e| e.to_string())?;

    let mut dst_linear = FirImage::new(tw, th, PixelType::U16x4);
    let mut resizer = Resizer::new();
    let opts = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(filter));
    resizer
        .resize(&src_linear, &mut dst_linear, &opts)
        .map_err(|e| e.to_string())?;

    let mut dst_srgb = FirImage::new(tw, th, PixelType::U8x4);
    mapper
        .backward_map(&dst_linear, &mut dst_srgb)
        .map_err(|e| e.to_string())?;

    RgbaImage::from_raw(tw, th, dst_srgb.into_vec())
        .ok_or_else(|| "RGBA buffer size mismatch".to_string())
}

/// Tile-based CLAHE on Rec.709 luminance; chromaticity preserved. ~100 lines, no dep.
///
/// `tiles_x` × `tiles_y` local histograms, clip limit as multiple of average bin count.
fn clahe_luminance(img: &mut RgbaImage, tiles_x: u32, tiles_y: u32, clip_limit: f32) {
    let w = img.width() as usize;
    let h = img.height() as usize;
    if w == 0 || h == 0 || tiles_x == 0 || tiles_y == 0 {
        return;
    }

    let mut lum = vec![0u8; w * h];
    for (i, px) in img.pixels().enumerate() {
        let r = px[0] as f32;
        let g = px[1] as f32;
        let b = px[2] as f32;
        lum[i] = (0.2126 * r + 0.7152 * g + 0.0722 * b).round().clamp(0.0, 255.0) as u8;
    }

    let tx = tiles_x as usize;
    let ty = tiles_y as usize;
    let tile_w = (w + tx - 1) / tx;
    let tile_h = (h + ty - 1) / ty;

    // Per-tile CDF lookup (256 bins each).
    let mut cdfs = vec![[0u8; 256]; tx * ty];
    for ty_i in 0..ty {
        for tx_i in 0..tx {
            let x0 = tx_i * tile_w;
            let y0 = ty_i * tile_h;
            let x1 = (x0 + tile_w).min(w);
            let y1 = (y0 + tile_h).min(h);
            let mut hist = [0u32; 256];
            let mut count = 0u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    hist[lum[y * w + x] as usize] += 1;
                    count += 1;
                }
            }
            if count == 0 {
                continue;
            }
            let avg = count as f32 / 256.0;
            let limit = (clip_limit * avg).max(1.0);
            let mut excess = 0f32;
            for bin in hist.iter_mut() {
                let v = *bin as f32;
                if v > limit {
                    excess += v - limit;
                    *bin = limit as u32;
                }
            }
            let bonus = (excess / 256.0) as u32;
            for bin in hist.iter_mut() {
                *bin += bonus;
            }
            let mut cdf = [0u32; 256];
            cdf[0] = hist[0];
            for i in 1..256 {
                cdf[i] = cdf[i - 1] + hist[i];
            }
            let cdf_min = cdf.iter().copied().find(|&v| v > 0).unwrap_or(0);
            let denom = (count - cdf_min).max(1);
            let mut lut = [0u8; 256];
            for i in 0..256 {
                lut[i] = (((cdf[i].saturating_sub(cdf_min)) as f32 / denom as f32) * 255.0)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
            cdfs[ty_i * tx + tx_i] = lut;
        }
    }

    // Bilinear blend of neighbouring tile LUTs.
    for y in 0..h {
        for x in 0..w {
            let gx = (x as f32 + 0.5) / tile_w as f32 - 0.5;
            let gy = (y as f32 + 0.5) / tile_h as f32 - 0.5;
            let x0 = gx.floor() as i32;
            let y0 = gy.floor() as i32;
            let fx = gx - x0 as f32;
            let fy = gy - y0 as f32;

            let sample = |tx_i: i32, ty_i: i32, v: u8| -> f32 {
                let tx_i = tx_i.clamp(0, tx as i32 - 1) as usize;
                let ty_i = ty_i.clamp(0, ty as i32 - 1) as usize;
                cdfs[ty_i * tx + tx_i][v as usize] as f32
            };

            let v = lum[y * w + x];
            let mapped = sample(x0, y0, v) * (1.0 - fx) * (1.0 - fy)
                + sample(x0 + 1, y0, v) * fx * (1.0 - fy)
                + sample(x0, y0 + 1, v) * (1.0 - fx) * fy
                + sample(x0 + 1, y0 + 1, v) * fx * fy;
            let new_l = mapped.round().clamp(0.0, 255.0);
            let old_l = lum[y * w + x] as f32;
            let px = img.get_pixel_mut(x as u32, y as u32);
            if old_l > 1e-3 {
                let scale = new_l / old_l;
                px[0] = (px[0] as f32 * scale).round().clamp(0.0, 255.0) as u8;
                px[1] = (px[1] as f32 * scale).round().clamp(0.0, 255.0) as u8;
                px[2] = (px[2] as f32 * scale).round().clamp(0.0, 255.0) as u8;
            } else {
                let g = new_l as u8;
                px[0] = g;
                px[1] = g;
                px[2] = g;
            }
        }
    }
}

/// A synthetic grey-paper "scan" shared by the pipeline and matte tests.
#[cfg(test)]
pub(crate) mod fixtures {
    use image::{DynamicImage, Rgba, RgbaImage};

    pub const PAGE_W: u32 = 240;
    pub const PAGE_H: u32 = 320;

    /// Warm grey paper with grain, rows of dark ink and a colour gradient.
    /// Nothing reaches pure black or white, as in a weak scan.
    pub fn grey_scan() -> RgbaImage {
        RgbaImage::from_fn(PAGE_W, PAGE_H, |x, y| {
            let grain = ((x.wrapping_mul(73) ^ y.wrapping_mul(151)) % 9) as i32 - 4;
            let paper = |c: i32| (c + grain).clamp(0, 255) as u8;
            let inside = (24..PAGE_W - 24).contains(&x) && (24..PAGE_H - 24).contains(&y);
            if inside && (200..280).contains(&y) && (40..200).contains(&x) {
                Rgba([(60 + x / 2) as u8, 120, (200 - x / 2) as u8, 255])
            } else if inside && y % 12 < 3 {
                Rgba([paper(46), paper(43), paper(40), 255])
            } else {
                Rgba([paper(206), paper(200), paper(190), 255])
            }
        })
    }

    pub fn png(img: &RgbaImage) -> Vec<u8> {
        let mut buf = Vec::new();
        DynamicImage::ImageRgba8(img.clone())
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .expect("encode png");
        buf
    }

    /// FNV-1a, enough to pin down byte-identical output in a test.
    pub fn fnv1a(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
            (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    fn process_grey_scan(scaling: ScalingMode, auto_contrast: bool, auto_levels: bool) -> Vec<u8> {
        let bytes = fixtures::png(&fixtures::grey_scan());
        let settings = ImageProcessingSettings {
            scaling,
            auto_contrast,
            auto_levels,
        };
        let out = process_page(&bytes, 180, 240, settings).unwrap();
        assert_eq!((out.width, out.height), (180, 240));
        out.rgba
    }

    /// Output hashes recorded from 0.1.8 before Auto levels existed (debug and
    /// release builds agreed). Auto levels off must reproduce them exactly.
    #[test]
    fn auto_levels_off_is_byte_identical_to_before() {
        let mut golden = vec![
            (ScalingMode::Nothing, false, 0xe7ad_c5f5_462a_28e8_u64),
            (ScalingMode::Nothing, true, 0xd077_81fe_ab38_ebea),
        ];
        // fast_image_resize picks SIMD code per CPU, so its exact bytes were
        // only recorded on x86_64.
        if cfg!(target_arch = "x86_64") {
            golden.extend([
                (ScalingMode::Lanczos3, false, 0xbd23_58f8_b91e_ce97),
                (ScalingMode::Lanczos3, true, 0xbd87_bab2_b450_eb9e),
                (ScalingMode::MitchellNetravali, false, 0x0d64_f704_b967_40fa),
                (ScalingMode::MitchellNetravali, true, 0x53a6_1a54_2053_248f),
            ]);
        }
        for (scaling, contrast, hash) in golden {
            let out = process_grey_scan(scaling, contrast, false);
            assert_eq!(
                fixtures::fnv1a(&out),
                hash,
                "{scaling:?}, auto contrast {contrast}"
            );
        }
    }

    #[test]
    fn auto_levels_runs_after_the_resize_and_before_clahe() {
        let src = fixtures::grey_scan();
        let mut expected = resize_image_crate(&src, 180, 240, ImageFilter::Triangle);
        assert!(auto_levels(&mut expected).is_some());
        clahe_luminance(&mut expected, 8, 8, 2.0);
        assert_eq!(
            process_grey_scan(ScalingMode::Nothing, true, true),
            expected.into_raw()
        );
    }

    #[test]
    fn auto_levels_on_changes_every_scaling_mode() {
        for scaling in [
            ScalingMode::Nothing,
            ScalingMode::Lanczos3,
            ScalingMode::MitchellNetravali,
        ] {
            assert_ne!(
                process_grey_scan(scaling, false, true),
                process_grey_scan(scaling, false, false),
                "{scaling:?}"
            );
        }
    }

    #[test]
    fn fit_contain_shrinks() {
        assert_eq!(fit_contain(2000, 1000, 1000, 800), (1000, 500));
    }

    #[test]
    fn clahe_runs_on_small_image() {
        let mut img: RgbaImage = ImageBuffer::from_fn(64, 64, |x, y| {
            let v = ((x + y) % 256) as u8;
            Rgba([v, v, v, 255])
        });
        clahe_luminance(&mut img, 4, 4, 2.0);
        assert_eq!(img.width(), 64);
    }

    #[test]
    fn cache_evicts_at_cap() {
        let mut cache = ProcessCache::default();
        for i in 0..6 {
            let key = CacheKey {
                page: i,
                width: 10,
                height: 10,
                scaling: ScalingMode::Nothing,
                auto_contrast: false,
                auto_levels: false,
            };
            cache.insert(
                key,
                ProcessedPage {
                    width: 10,
                    height: 10,
                    rgba: vec![0; 10 * 10 * 4],
                },
            );
        }
        assert!(cache.map.len() <= CACHE_CAP);
    }

    /// Set `PELTA_WRITE_CROPS=1` and `PELTA_CROP_SRC=/path/to.jpg` to dump before/after PNGs.
    #[test]
    fn write_pr_crops_when_env_set() {
        if std::env::var_os("PELTA_WRITE_CROPS").is_none() {
            return;
        }
        let src = std::env::var("PELTA_CROP_SRC").expect("PELTA_CROP_SRC");
        let out_dir = std::env::var("PELTA_CROP_OUT").unwrap_or_else(|_| "/tmp/pelta-crops".into());
        let bytes = std::fs::read(&src).expect("read src");
        let before = process_page(
            &bytes,
            900,
            1200,
            ImageProcessingSettings {
                scaling: ScalingMode::Nothing,
                auto_contrast: false,
                auto_levels: false,
            },
        )
        .expect("before");
        // "Before" still resizes with Triangle when display size differs — use raw decode crop instead.
        let raw = decode_rgba(&bytes).expect("decode");
        let after = process_page(
            &bytes,
            900,
            1200,
            ImageProcessingSettings {
                scaling: ScalingMode::Lanczos3,
                auto_contrast: false,
                auto_levels: false,
            },
        )
        .expect("after");

        let after_mitchell = process_page(
            &bytes,
            900,
            1200,
            ImageProcessingSettings {
                scaling: ScalingMode::MitchellNetravali,
                auto_contrast: false,
                auto_levels: false,
            },
        )
        .expect("after mitchell");

        let after_clahe = process_page(
            &bytes,
            900,
            1200,
            ImageProcessingSettings {
                scaling: ScalingMode::Nothing,
                auto_contrast: true,
                auto_levels: false,
            },
        )
        .expect("after clahe");

        fn crop_center(rgba: &[u8], w: u32, h: u32, cw: u32, ch: u32) -> image::RgbaImage {
            let x0 = w.saturating_sub(cw) / 2;
            let y0 = h.saturating_sub(ch) / 2;
            let mut out = image::RgbaImage::new(cw, ch);
            for y in 0..ch {
                for x in 0..cw {
                    let sx = x0 + x;
                    let sy = y0 + y;
                    let i = ((sy * w + sx) * 4) as usize;
                    out.put_pixel(
                        x,
                        y,
                        image::Rgba([rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]),
                    );
                }
            }
            out
        }

        // Match after size for fair text-edge compare: scale raw with GTK-like triangle.
        let before_full =
            super::resize_image_crate(&raw, after.width, after.height, ImageFilter::Triangle);
        let crop_w = 320u32;
        let crop_h = 200u32;
        let before_crop = crop_center(
            before_full.as_raw(),
            before_full.width(),
            before_full.height(),
            crop_w,
            crop_h,
        );
        let after_crop = crop_center(&after.rgba, after.width, after.height, crop_w, crop_h);
        let mitchell_crop = crop_center(
            &after_mitchell.rgba,
            after_mitchell.width,
            after_mitchell.height,
            crop_w,
            crop_h,
        );
        let clahe_crop = crop_center(
            &after_clahe.rgba,
            after_clahe.width,
            after_clahe.height,
            crop_w,
            crop_h,
        );
        let _ = before; // unused intentionally (display path)
        std::fs::create_dir_all(&out_dir).ok();
        before_crop
            .save(format!("{out_dir}/before-gtk-ish-crop.png"))
            .expect("save before");
        after_crop
            .save(format!("{out_dir}/after-lanczos3-crop.png"))
            .expect("save after");
        mitchell_crop
            .save(format!("{out_dir}/after-mitchell-crop.png"))
            .expect("save mitchell");
        clahe_crop
            .save(format!("{out_dir}/after-clahe-crop.png"))
            .expect("save clahe");
        eprintln!("Wrote crops to {out_dir}");
    }

    #[test]
    fn mitchell_resize_runs() {
        let img: RgbaImage = ImageBuffer::from_fn(64, 48, |x, y| {
            let v = ((x * 3 + y * 5) % 256) as u8;
            Rgba([v, v.wrapping_add(20), v.wrapping_add(40), 255])
        });
        let mut buf = Vec::new();
        {
            let dyn_img = DynamicImage::ImageRgba8(img);
            dyn_img
                .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
                .expect("encode png");
        }
        let out = process_page(
            &buf,
            32,
            24,
            ImageProcessingSettings {
                scaling: ScalingMode::MitchellNetravali,
                auto_contrast: false,
                auto_levels: false,
            },
        )
        .expect("mitchell");
        assert_eq!((out.width, out.height), (32, 24));
        assert_eq!(out.rgba.len(), 32 * 24 * 4);
    }
}
