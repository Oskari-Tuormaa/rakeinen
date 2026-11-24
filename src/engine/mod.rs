pub mod diffusion;
pub mod kernels;
pub mod noise;
pub mod ordered;
pub mod palette;
pub mod preprocess;

use image::{RgbaImage, imageops};

use diffusion::{DiffusionParams, diffuse};
use kernels::{BuiltinKernel, Kernel};
use noise::{NoiseKind, NoiseParams};
use ordered::{Matrix, OrderedParams, OrderedPreset};
use palette::{ColorSettings, Palette, Quantizer};
use preprocess::{PreprocessSettings, output_size, preprocess_region};

/// Reports whether the running job has been superseded and should stop early.
pub type Cancel<'a> = &'a (dyn Fn() -> bool + Sync);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MethodKind {
    Diffusion,
    Ordered,
    Noise,
}

impl MethodKind {
    pub const ALL: [MethodKind; 3] = [Self::Diffusion, Self::Ordered, Self::Noise];

    pub fn name(self) -> &'static str {
        match self {
            Self::Diffusion => "Error diffusion",
            Self::Ordered => "Ordered",
            Self::Noise => "Noise",
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct DiffusionSettings {
    pub kernel: Kernel,
    pub serpentine: bool,
    pub strength: f32,
}

#[derive(Clone, PartialEq, Debug)]
pub struct OrderedSettings {
    pub matrix: Matrix,
    pub spread: f32,
}

#[derive(Clone, PartialEq, Debug)]
pub struct NoiseSettings {
    pub kind: NoiseKind,
    pub spread: f32,
    pub seed: u32,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Settings {
    pub preprocess: PreprocessSettings,
    pub method: MethodKind,
    pub diffusion: DiffusionSettings,
    pub ordered: OrderedSettings,
    pub noise: NoiseSettings,
    pub palette: Palette,
    pub color: ColorSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            preprocess: PreprocessSettings::default(),
            method: MethodKind::Diffusion,
            diffusion: DiffusionSettings {
                kernel: BuiltinKernel::FloydSteinberg.kernel(),
                serpentine: true,
                strength: 1.0,
            },
            ordered: OrderedSettings {
                matrix: OrderedPreset::Bayer8.matrix(),
                spread: 1.0,
            },
            noise: NoiseSettings {
                kind: NoiseKind::Blue,
                spread: 1.0,
                seed: 0,
            },
            palette: Palette::black_white(),
            color: ColorSettings::default(),
        }
    }
}

/// A rectangle of the processed image, in processed pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Region {
    pub fn full(w: u32, h: u32) -> Self {
        Self { x: 0, y: 0, w, h }
    }

    /// Moves and shrinks the region to fit inside a `width` × `height` image, keeping at least
    /// one pixel. Both dimensions must be non-zero.
    pub fn clamped(self, width: u32, height: u32) -> Self {
        let x = self.x.min(width - 1);
        let y = self.y.min(height - 1);
        Self {
            x,
            y,
            w: self.w.clamp(1, width - x),
            h: self.h.clamp(1, height - y),
        }
    }

    pub fn covers(self, width: u32, height: u32) -> bool {
        self.x == 0 && self.y == 0 && self.w >= width && self.h >= height
    }

    pub fn area(self) -> u64 {
        self.w as u64 * self.h as u64
    }
}

/// How far around a region error diffusion starts, so that the error carried into the region
/// has settled instead of starting from nothing at its edge.
const DIFFUSION_WARMUP: u32 = 32;

/// Size of the processed image: `pixel_size` times smaller than `src` in each dimension.
pub fn processed_size(src: &RgbaImage, settings: &Settings) -> (u32, u32) {
    output_size(src.width(), src.height(), settings.preprocess.pixel_size)
}

/// `src` after preprocessing but before dithering, which is what the dithering actually sees.
pub fn preprocessed(src: &RgbaImage, settings: &Settings) -> RgbaImage {
    let (w, h) = processed_size(src, settings);
    preprocess_region(src, &settings.preprocess, Region::full(w, h))
}

/// Preprocesses and dithers all of `src`. Returns `None` if `cancelled` becomes true mid-run.
// Not called by the app yet; exporting the result will use it.
#[allow(dead_code)]
pub fn process(src: &RgbaImage, settings: &Settings, cancelled: Cancel) -> Option<RgbaImage> {
    let (w, h) = processed_size(src, settings);
    process_region(src, settings, Region::full(w, h), cancelled)
}

/// Preprocesses and dithers one rectangle of the processed image.
///
/// Ordered and noise dithering are exactly equal to the same area of a full run. Error
/// diffusion carries state from the pixels before it, so it starts a margin above, left and
/// right of the region and discards that margin. That gives the same look but a different
/// pattern, except when the region covers the full width and starts at the top.
pub fn process_region(
    src: &RgbaImage,
    settings: &Settings,
    region: Region,
    cancelled: Cancel,
) -> Option<RgbaImage> {
    let (w, h) = processed_size(src, settings);
    let region = region.clamped(w, h);
    let quantizer = Quantizer::new(&settings.palette, settings.color);
    let origin = (region.x as usize, region.y as usize);
    match settings.method {
        MethodKind::Diffusion => {
            let x0 = region.x.saturating_sub(DIFFUSION_WARMUP);
            let x1 = (region.x + region.w + DIFFUSION_WARMUP).min(w);
            // An even first row keeps the serpentine direction of every row the same as in a
            // full run.
            let y0 = region.y.saturating_sub(DIFFUSION_WARMUP) & !1;
            let work = Region {
                x: x0,
                y: y0,
                w: x1 - x0,
                h: region.y + region.h - y0,
            };
            let prepared = preprocess_region(src, &settings.preprocess, work);
            let dithered = diffuse(
                &prepared,
                &DiffusionParams {
                    kernel: &settings.diffusion.kernel,
                    quantizer: &quantizer,
                    serpentine: settings.diffusion.serpentine,
                    strength: settings.diffusion.strength,
                },
                cancelled,
            )?;
            if work == region {
                return Some(dithered);
            }
            Some(
                imageops::crop_imm(&dithered, region.x - x0, region.y - y0, region.w, region.h)
                    .to_image(),
            )
        }
        MethodKind::Ordered => {
            let prepared = preprocess_region(src, &settings.preprocess, region);
            ordered::dither(
                &prepared,
                &OrderedParams {
                    matrix: &settings.ordered.matrix,
                    quantizer: &quantizer,
                    spread: settings.ordered.spread,
                },
                origin,
                cancelled,
            )
        }
        MethodKind::Noise => {
            let prepared = preprocess_region(src, &settings.preprocess, region);
            noise::dither(
                &prepared,
                &NoiseParams {
                    kind: settings.noise.kind,
                    quantizer: &quantizer,
                    spread: settings.noise.spread,
                    seed: settings.noise.seed,
                },
                origin,
                cancelled,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn gradient(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            let v = ((x + y) * 255 / (w + h - 2)) as u8;
            Rgba([v, 255 - v, (x * 3 % 256) as u8, 255])
        })
    }

    fn crop(img: &RgbaImage, r: Region) -> RgbaImage {
        imageops::crop_imm(img, r.x, r.y, r.w, r.h).to_image()
    }

    fn white_ratio(img: &RgbaImage) -> f32 {
        let white = img.pixels().filter(|p| p[0] == 255).count();
        white as f32 / (img.width() * img.height()) as f32
    }

    const REGION: Region = Region {
        x: 41,
        y: 37,
        w: 53,
        h: 49,
    };

    fn full(src: &RgbaImage, s: &Settings) -> RgbaImage {
        process(src, s, &|| false).unwrap()
    }

    fn region(src: &RgbaImage, s: &Settings, r: Region) -> RgbaImage {
        process_region(src, s, r, &|| false).unwrap()
    }

    #[test]
    fn region_clamping_keeps_a_valid_rectangle() {
        let r = Region {
            x: 90,
            y: 5,
            w: 50,
            h: 0,
        }
        .clamped(100, 20);
        assert_eq!(
            r,
            Region {
                x: 90,
                y: 5,
                w: 10,
                h: 1
            }
        );
        let outside = Region {
            x: 500,
            y: 500,
            w: 5,
            h: 5,
        }
        .clamped(100, 20);
        assert_eq!((outside.x, outside.y), (99, 19));
    }

    #[test]
    fn ordered_region_equals_the_same_area_of_a_full_run() {
        let src = gradient(160, 140);
        for pixel_size in [1, 2] {
            let mut s = Settings {
                method: MethodKind::Ordered,
                ..Settings::default()
            };
            s.preprocess.pixel_size = pixel_size;
            s.preprocess.contrast = 0.2;
            let whole = full(&src, &s);
            assert_eq!(
                region(&src, &s, REGION),
                crop(&whole, REGION),
                "ps {pixel_size}"
            );
        }
    }

    #[test]
    fn noise_regions_equal_the_same_area_of_a_full_run() {
        let src = gradient(160, 140);
        for kind in noise::NoiseKind::ALL {
            let mut s = Settings {
                method: MethodKind::Noise,
                ..Settings::default()
            };
            s.noise.kind = kind;
            s.noise.seed = 9;
            let whole = full(&src, &s);
            assert_eq!(
                region(&src, &s, REGION),
                crop(&whole, REGION),
                "{}",
                kind.name()
            );
        }
    }

    #[test]
    fn diffusion_region_has_requested_size_and_matching_tone() {
        let src = gradient(200, 200);
        let s = Settings::default();
        let whole = full(&src, &s);
        let part = region(&src, &s, REGION);
        assert_eq!((part.width(), part.height()), (REGION.w, REGION.h));
        let expected = white_ratio(&crop(&whole, REGION));
        let actual = white_ratio(&part);
        assert!((expected - actual).abs() < 0.03, "{expected} vs {actual}");
    }

    #[test]
    fn diffusion_region_from_the_top_with_full_width_is_exact() {
        let src = gradient(120, 90);
        for serpentine in [false, true] {
            let mut s = Settings::default();
            s.diffusion.serpentine = serpentine;
            let whole = full(&src, &s);
            let r = Region {
                x: 0,
                y: 0,
                w: 120,
                h: 41,
            };
            assert_eq!(
                region(&src, &s, r),
                crop(&whole, r),
                "serpentine {serpentine}"
            );
        }
    }

    #[test]
    fn full_region_equals_process() {
        let src = gradient(64, 48);
        let s = Settings::default();
        assert_eq!(region(&src, &s, Region::full(64, 48)), full(&src, &s));
    }
}
