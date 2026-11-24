use image::{Rgba, RgbaImage};

use super::Cancel;
use super::kernels::{Kernel, Tap};
use super::palette::Quantizer;

pub struct DiffusionParams<'a> {
    pub kernel: &'a Kernel,
    pub quantizer: &'a Quantizer,
    pub serpentine: bool,
    pub strength: f32,
}

/// Runs error diffusion. Returns `None` when `cancelled` reports true between scanlines.
pub fn diffuse(src: &RgbaImage, params: &DiffusionParams, cancelled: Cancel) -> Option<RgbaImage> {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let mut out = src.clone();
    let q = params.quantizer;
    if q.is_empty() || w == 0 || h == 0 {
        return Some(out);
    }

    let forward = params.kernel.taps();
    let mirrored: Vec<Tap> = forward.iter().map(|t| Tap { dx: -t.dx, ..*t }).collect();

    let mut buf: Vec<[f32; 3]> = src
        .pixels()
        .map(|p| q.to_working([p[0], p[1], p[2]]))
        .collect();

    for y in 0..h {
        if cancelled() {
            return None;
        }
        let reversed = params.serpentine && y % 2 == 1;
        let taps = if reversed { &mirrored } else { &forward };

        for i in 0..w {
            let x = if reversed { w - 1 - i } else { i };
            let idx = y * w + x;

            let old = buf[idx].map(|v| v.clamp(0.0, 255.0));
            let chosen = q.nearest(old);
            let color = q.colors[chosen];
            let target = q.working(chosen);
            let alpha = src.get_pixel(x as u32, y as u32)[3];
            out.put_pixel(
                x as u32,
                y as u32,
                Rgba([color[0], color[1], color[2], alpha]),
            );

            let err = [
                (old[0] - target[0]) * params.strength,
                (old[1] - target[1]) * params.strength,
                (old[2] - target[2]) * params.strength,
            ];
            for tap in taps {
                let nx = x as i32 + tap.dx;
                let ny = y as i32 + tap.dy;
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                let n = &mut buf[ny as usize * w + nx as usize];
                n[0] += err[0] * tap.weight;
                n[1] += err[1] * tap.weight;
                n[2] += err[2] * tap.weight;
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::kernels::BuiltinKernel;
    use crate::engine::palette::{ColorSettings, Palette};

    fn gray_image(w: u32, h: u32, v: u8) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([v, v, v, 255]))
    }

    fn run_with(
        img: &RgbaImage,
        palette: &Palette,
        serpentine: bool,
        color: ColorSettings,
    ) -> RgbaImage {
        let kernel = BuiltinKernel::FloydSteinberg.kernel();
        let quantizer = Quantizer::new(palette, color);
        let params = DiffusionParams {
            kernel: &kernel,
            quantizer: &quantizer,
            serpentine,
            strength: 1.0,
        };
        diffuse(img, &params, &|| false).unwrap()
    }

    fn run(img: &RgbaImage, palette: &Palette, serpentine: bool) -> RgbaImage {
        run_with(img, palette, serpentine, ColorSettings::default())
    }

    fn white_ratio(img: &RgbaImage) -> f32 {
        img.pixels().filter(|p| p[0] == 255).count() as f32 / (img.width() * img.height()) as f32
    }

    #[test]
    fn output_only_contains_palette_colors() {
        let palette = Palette::black_white();
        let out = run(&gray_image(32, 32, 100), &palette, false);
        for p in out.pixels() {
            assert!(p[0] == 0 || p[0] == 255);
            assert_eq!(p[3], 255);
        }
    }

    #[test]
    fn mid_gray_is_roughly_half_white() {
        let palette = Palette::black_white();
        for serpentine in [false, true] {
            let out = run(&gray_image(64, 64, 128), &palette, serpentine);
            let ratio = white_ratio(&out);
            assert!((ratio - 0.5).abs() < 0.02, "ratio {ratio}");
        }
    }

    #[test]
    fn linear_light_preserves_physical_brightness() {
        let color = ColorSettings {
            linear_light: true,
            ..ColorSettings::default()
        };
        let out = run_with(
            &gray_image(64, 64, 128),
            &Palette::black_white(),
            true,
            color,
        );
        // sRGB 128 is about 21.6% linear light.
        let ratio = white_ratio(&out);
        assert!((ratio - 0.216).abs() < 0.02, "ratio {ratio}");
    }

    #[test]
    fn pure_colors_are_preserved() {
        let palette = Palette::black_white();
        assert!(
            run(&gray_image(8, 8, 0), &palette, false)
                .pixels()
                .all(|p| p[0] == 0)
        );
        assert!(
            run(&gray_image(8, 8, 255), &palette, false)
                .pixels()
                .all(|p| p[0] == 255)
        );
    }

    #[test]
    fn is_deterministic() {
        let palette = Palette::grayscale(4);
        let img = gray_image(40, 40, 77);
        assert_eq!(run(&img, &palette, true), run(&img, &palette, true));
    }

    #[test]
    fn cancellation_returns_none() {
        let kernel = BuiltinKernel::FloydSteinberg.kernel();
        let quantizer = Quantizer::new(&Palette::black_white(), ColorSettings::default());
        let params = DiffusionParams {
            kernel: &kernel,
            quantizer: &quantizer,
            serpentine: false,
            strength: 1.0,
        };
        assert!(diffuse(&gray_image(4, 4, 10), &params, &|| true).is_none());
    }
}
