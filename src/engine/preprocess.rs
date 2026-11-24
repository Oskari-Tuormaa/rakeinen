use std::f32::consts::FRAC_PI_4;

use image::RgbaImage;
use image::imageops::{self, FilterType};

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

/// Resizes and tone-adjusts `src`. The result is `pixel_size` times smaller in each dimension.
pub fn preprocess(src: &RgbaImage, settings: &PreprocessSettings) -> RgbaImage {
    let mut img = if settings.pixel_size > 1 {
        let w = (src.width() / settings.pixel_size).max(1);
        let h = (src.height() / settings.pixel_size).max(1);
        imageops::resize(src, w, h, FilterType::Triangle)
    } else {
        src.clone()
    };
    if !settings.changes_tones() {
        return img;
    }

    let curve = tone_curve(settings);
    for p in img.pixels_mut() {
        let [r, g, b] = if settings.grayscale {
            let luma =
                (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32).round() as u8;
            [luma; 3]
        } else {
            [p[0], p[1], p[2]]
        };
        p[0] = curve[r as usize];
        p[1] = curve[g as usize];
        p[2] = curve[b as usize];
    }
    img
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
    use image::Rgba;

    fn sample() -> RgbaImage {
        RgbaImage::from_fn(8, 6, |x, y| Rgba([(x * 30) as u8, (y * 40) as u8, 90, 200]))
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
}
