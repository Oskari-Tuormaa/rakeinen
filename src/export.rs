use std::path::Path;

use image::RgbaImage;
use rayon::prelude::*;

use crate::engine::{self, Settings};

pub const MAX_SCALE: u32 = 32;
/// Refuse to build images larger than this many pixels, to avoid running out of memory.
pub const MAX_OUTPUT_PIXELS: u64 = 200_000_000;

pub fn output_size(width: u32, height: u32, scale: u32) -> (u32, u32) {
    (width * scale.max(1), height * scale.max(1))
}

/// Repeats every pixel `scale` times in both directions, keeping hard edges.
pub fn upscale_nearest(img: &RgbaImage, scale: u32) -> RgbaImage {
    if scale <= 1 {
        return img.clone();
    }
    let s = scale as usize;
    let (w, h) = (img.width() as usize, img.height() as usize);
    let raw = img.as_raw();

    let mut out = vec![0u8; w * s * h * s * 4];
    out.par_chunks_mut(w * s * 4)
        .enumerate()
        .for_each(|(y, row)| {
            let src_row = &raw[(y / s) * w * 4..][..w * 4];
            for (x, dst) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let i = (x / s) * 4;
                *dst = [src_row[i], src_row[i + 1], src_row[i + 2], src_row[i + 3]];
            }
        });
    RgbaImage::from_raw((w * s) as u32, (h * s) as u32, out).expect("buffer matches image size")
}

/// Dithers the whole image with `settings` and upscales it by `scale`.
pub fn render(src: &RgbaImage, settings: &Settings, scale: u32) -> Option<RgbaImage> {
    let dithered = engine::process(src, settings, &|| false)?;
    Some(upscale_nearest(&dithered, scale))
}

pub fn save_png(img: &RgbaImage, path: &Path) -> Result<(), String> {
    img.save_with_format(path, image::ImageFormat::Png)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::MethodKind;
    use image::Rgba;

    fn sample() -> RgbaImage {
        RgbaImage::from_fn(5, 4, |x, y| {
            Rgba([(x * 50) as u8, (y * 60) as u8, 7, (x + y) as u8])
        })
    }

    #[test]
    fn upscale_repeats_each_pixel() {
        let img = sample();
        let big = upscale_nearest(&img, 3);
        assert_eq!((big.width(), big.height()), (15, 12));
        for y in 0..12 {
            for x in 0..15 {
                assert_eq!(big.get_pixel(x, y), img.get_pixel(x / 3, y / 3));
            }
        }
    }

    #[test]
    fn scale_of_one_changes_nothing() {
        let img = sample();
        assert_eq!(upscale_nearest(&img, 1), img);
        assert_eq!(upscale_nearest(&img, 0), img);
    }

    #[test]
    fn output_size_multiplies_both_sides() {
        assert_eq!(output_size(40, 30, 4), (160, 120));
        assert_eq!(output_size(40, 30, 0), (40, 30));
    }

    #[test]
    fn render_dithers_then_upscales() {
        let src = RgbaImage::from_pixel(20, 10, Rgba([120, 120, 120, 255]));
        let settings = Settings {
            method: MethodKind::Ordered,
            ..Settings::default()
        };
        let plain = render(&src, &settings, 1).unwrap();
        let big = render(&src, &settings, 2).unwrap();
        assert_eq!((plain.width(), plain.height()), (20, 10));
        assert_eq!(big, upscale_nearest(&plain, 2));
    }

    #[test]
    fn render_with_pixel_size_and_matching_scale_restores_the_source_size() {
        let src = RgbaImage::from_pixel(40, 24, Rgba([50, 90, 200, 255]));
        let mut settings = Settings::default();
        settings.preprocess.pixel_size = 4;
        let out = render(&src, &settings, 4).unwrap();
        assert_eq!((out.width(), out.height()), (40, 24));
    }

    #[test]
    fn png_round_trips_with_alpha() {
        let img = sample();
        let path = std::env::temp_dir().join(format!("rakeinen-test-{}.png", std::process::id()));
        save_png(&img, &path).unwrap();
        let loaded = image::open(&path).unwrap().to_rgba8();
        std::fs::remove_file(&path).ok();
        assert_eq!(loaded, img);
    }
}
