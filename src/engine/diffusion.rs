use image::{Rgba, RgbaImage};

use super::Cancel;
use super::kernels::{Kernel, Tap};
use super::palette::Palette;

pub struct DiffusionParams<'a> {
    pub kernel: &'a Kernel,
    pub palette: &'a Palette,
    pub serpentine: bool,
    pub strength: f32,
}

/// Runs error diffusion. Returns `None` when `cancelled` reports true between scanlines.
pub fn diffuse(src: &RgbaImage, params: &DiffusionParams, cancelled: Cancel) -> Option<RgbaImage> {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let mut out = src.clone();
    if params.palette.is_empty() || w == 0 || h == 0 {
        return Some(out);
    }

    let forward = params.kernel.taps();
    let mirrored: Vec<Tap> = forward.iter().map(|t| Tap { dx: -t.dx, ..*t }).collect();

    let mut buf: Vec<[f32; 3]> = src
        .pixels()
        .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
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
            let chosen = params.palette.colors[params.palette.nearest(old)];
            let alpha = src.get_pixel(x as u32, y as u32)[3];
            out.put_pixel(
                x as u32,
                y as u32,
                Rgba([chosen[0], chosen[1], chosen[2], alpha]),
            );

            let err = [
                (old[0] - chosen[0] as f32) * params.strength,
                (old[1] - chosen[1] as f32) * params.strength,
                (old[2] - chosen[2] as f32) * params.strength,
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

    fn gray_image(w: u32, h: u32, v: u8) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([v, v, v, 255]))
    }

    fn run(img: &RgbaImage, palette: &Palette, serpentine: bool) -> RgbaImage {
        let kernel = BuiltinKernel::FloydSteinberg.kernel();
        let params = DiffusionParams {
            kernel: &kernel,
            palette,
            serpentine,
            strength: 1.0,
        };
        diffuse(img, &params, &|| false).unwrap()
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
            let white = out.pixels().filter(|p| p[0] == 255).count() as f32;
            let ratio = white / (64.0 * 64.0);
            assert!((ratio - 0.5).abs() < 0.02, "ratio {ratio}");
        }
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
        let palette = Palette::black_white();
        let params = DiffusionParams {
            kernel: &kernel,
            palette: &palette,
            serpentine: false,
            strength: 1.0,
        };
        assert!(diffuse(&gray_image(4, 4, 10), &params, &|| true).is_none());
    }
}
