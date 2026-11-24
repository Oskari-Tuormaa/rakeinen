use std::f32::consts::FRAC_PI_4;

use image::RgbaImage;
use rayon::prelude::*;

use super::Region;

#[derive(Clone, PartialEq, Debug)]
pub struct PreprocessSettings {
    /// Shrinks the image by this integer factor before dithering, giving chunky pixels.
    pub pixel_size: u32,
    pub grayscale: bool,
    pub invert: bool,
    /// `-1.0..=1.0`, added to every channel.
    pub brightness: f32,
    /// `-1.0..=1.0`, where 0 leaves the image unchanged.
    pub contrast: f32,
    /// Values above 1 brighten the midtones.
    pub gamma: f32,
}

impl Default for PreprocessSettings {
    fn default() -> Self {
        Self {
            pixel_size: 1,
            grayscale: false,
            invert: false,
            brightness: 0.0,
            contrast: 0.0,
            gamma: 1.0,
        }
    }
}

impl PreprocessSettings {
    fn changes_tones(&self) -> bool {
        self.grayscale
            || self.invert
            || self.brightness != 0.0
            || self.contrast != 0.0
            || self.gamma != 1.0
    }
}

/// Size of the preprocessed image: every `pixel_size` block of the source becomes one pixel.
pub fn output_size(width: u32, height: u32, pixel_size: u32) -> (u32, u32) {
    let ps = pixel_size.max(1);
    ((width / ps).max(1), (height / ps).max(1))
}

/// Computes one rectangle of the preprocessed image. Every output pixel depends only on its
/// own `pixel_size` block of the source, so a region is identical to the same area of the
/// full result. `region` must lie inside [`output_size`].
pub fn preprocess_region(
    src: &RgbaImage,
    settings: &PreprocessSettings,
    region: Region,
) -> RgbaImage {
    let ps = settings.pixel_size.max(1) as usize;
    let (sw, sh) = (src.width() as usize, src.height() as usize);
    let (rw, rh) = (region.w as usize, region.h as usize);
    let raw = src.as_raw();
    let curve = settings.changes_tones().then(|| tone_curve(settings));

    let mut out = vec![0u8; rw * rh * 4];
    out.par_chunks_mut(rw * 4)
        .enumerate()
        .for_each(|(ry, row)| {
            let y0 = (region.y as usize + ry) * ps;
            let y1 = (y0 + ps).min(sh);
            for (rx, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let x0 = (region.x as usize + rx) * ps;
                let x1 = (x0 + ps).min(sw);
                *px = if ps == 1 {
                    let i = (y0 * sw + x0) * 4;
                    [raw[i], raw[i + 1], raw[i + 2], raw[i + 3]]
                } else {
                    let mut sum = [0u32; 4];
                    for y in y0..y1 {
                        for x in x0..x1 {
                            let i = (y * sw + x) * 4;
                            for c in 0..4 {
                                sum[c] += raw[i + c] as u32;
                            }
                        }
                    }
                    let n = (((x1 - x0) * (y1 - y0)) as u32).max(1);
                    sum.map(|s| ((s + n / 2) / n) as u8)
                };
                if let Some(curve) = &curve {
                    apply_tone(px, curve, settings.grayscale);
                }
            }
        });
    RgbaImage::from_raw(region.w, region.h, out).expect("buffer matches region size")
}

fn apply_tone(px: &mut [u8; 4], curve: &[u8; 256], grayscale: bool) {
    let [r, g, b] = if grayscale {
        let luma =
            (0.2126 * px[0] as f32 + 0.7152 * px[1] as f32 + 0.0722 * px[2] as f32).round() as u8;
        [luma; 3]
    } else {
        [px[0], px[1], px[2]]
    };
    px[0] = curve[r as usize];
    px[1] = curve[g as usize];
    px[2] = curve[b as usize];
}

fn tone_curve(s: &PreprocessSettings) -> [u8; 256] {
    // Maps contrast -1..1 onto a slope of 0..infinity, with 0 giving exactly 1.
    let slope = ((s.contrast.clamp(-0.999, 0.999) + 1.0) * FRAC_PI_4).tan();
    let inverse_gamma = 1.0 / s.gamma.max(0.01);
    let mut curve = [0u8; 256];
    for (i, out) in curve.iter_mut().enumerate() {
        let mut v = (i as f32 / 255.0).powf(inverse_gamma);
        v = (v - 0.5) * slope + 0.5;
        v += s.brightness;
        if s.invert {
            v = 1.0 - v;
        }
        *out = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    curve
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, imageops};

    fn sample() -> RgbaImage {
        RgbaImage::from_fn(8, 6, |x, y| Rgba([(x * 30) as u8, (y * 40) as u8, 90, 200]))
    }

    fn preprocess(src: &RgbaImage, settings: &PreprocessSettings) -> RgbaImage {
        let (w, h) = output_size(src.width(), src.height(), settings.pixel_size);
        preprocess_region(src, settings, Region::full(w, h))
    }

    #[test]
    fn defaults_leave_image_unchanged() {
        let img = sample();
        assert_eq!(preprocess(&img, &PreprocessSettings::default()), img);
    }

    #[test]
    fn neutral_tone_curve_is_identity() {
        let curve = tone_curve(&PreprocessSettings::default());
        assert!(curve.iter().enumerate().all(|(i, &v)| v as usize == i));
    }

    #[test]
    fn invert_flips_colors_and_keeps_alpha() {
        let img = sample();
        let out = preprocess(
            &img,
            &PreprocessSettings {
                invert: true,
                ..Default::default()
            },
        );
        assert_eq!(out.get_pixel(0, 0), &Rgba([255, 255, 165, 200]));
    }

    #[test]
    fn grayscale_equalizes_channels() {
        let out = preprocess(
            &sample(),
            &PreprocessSettings {
                grayscale: true,
                ..Default::default()
            },
        );
        assert!(out.pixels().all(|p| p[0] == p[1] && p[1] == p[2]));
    }

    #[test]
    fn brightness_extremes_saturate() {
        let settings = |brightness| PreprocessSettings {
            brightness,
            ..Default::default()
        };
        assert!(
            preprocess(&sample(), &settings(1.0))
                .pixels()
                .all(|p| p[0] == 255)
        );
        assert!(
            preprocess(&sample(), &settings(-1.0))
                .pixels()
                .all(|p| p[0] == 0)
        );
    }

    #[test]
    fn contrast_pushes_values_away_from_the_middle() {
        let curve = tone_curve(&PreprocessSettings {
            contrast: 0.5,
            ..Default::default()
        });
        assert!(curve[64] < 64);
        assert!(curve[192] > 192);
        assert!((curve[128] as i32 - 128).abs() <= 1);
    }

    #[test]
    fn gamma_above_one_brightens_midtones() {
        let curve = tone_curve(&PreprocessSettings {
            gamma: 2.0,
            ..Default::default()
        });
        assert!(curve[128] > 128);
        assert_eq!((curve[0], curve[255]), (0, 255));
    }

    #[test]
    fn pixel_size_shrinks_the_image() {
        let img = RgbaImage::from_pixel(40, 25, Rgba([10, 20, 30, 255]));
        let out = preprocess(
            &img,
            &PreprocessSettings {
                pixel_size: 4,
                ..Default::default()
            },
        );
        assert_eq!((out.width(), out.height()), (10, 6));
        assert_eq!(out.get_pixel(3, 3), &Rgba([10, 20, 30, 255]));
    }

    #[test]
    fn pixel_size_averages_each_block() {
        let mut img = RgbaImage::new(2, 2);
        for (i, v) in [0u8, 100, 200, 41].into_iter().enumerate() {
            img.put_pixel(i as u32 % 2, i as u32 / 2, Rgba([v, v, v, 255]));
        }
        let out = preprocess(
            &img,
            &PreprocessSettings {
                pixel_size: 2,
                ..Default::default()
            },
        );
        assert_eq!(out.get_pixel(0, 0), &Rgba([85, 85, 85, 255]));
    }

    #[test]
    fn pixel_size_never_collapses_below_one_pixel() {
        let out = preprocess(
            &sample(),
            &PreprocessSettings {
                pixel_size: 32,
                ..Default::default()
            },
        );
        assert_eq!((out.width(), out.height()), (1, 1));
    }

    #[test]
    fn region_matches_the_same_area_of_the_full_result() {
        let img = RgbaImage::from_fn(37, 29, |x, y| {
            Rgba([
                (x * 7 % 256) as u8,
                (y * 11 % 256) as u8,
                ((x * y) % 256) as u8,
                255,
            ])
        });
        for pixel_size in [1, 3] {
            let settings = PreprocessSettings {
                pixel_size,
                contrast: 0.3,
                gamma: 1.4,
                brightness: -0.1,
                ..Default::default()
            };
            let full = preprocess(&img, &settings);
            let region = Region {
                x: 2,
                y: 1,
                w: 5,
                h: 4,
            };
            let part = preprocess_region(&img, &settings, region);
            let expected = imageops::crop_imm(&full, 2, 1, 5, 4).to_image();
            assert_eq!(part, expected, "pixel size {pixel_size}");
        }
    }
}
