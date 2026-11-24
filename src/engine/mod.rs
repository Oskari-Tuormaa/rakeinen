pub mod diffusion;
pub mod kernels;
pub mod noise;
pub mod ordered;
pub mod palette;
pub mod preprocess;

use image::RgbaImage;

use diffusion::{DiffusionParams, diffuse};
use kernels::{BuiltinKernel, Kernel};
use noise::{NoiseKind, NoiseParams};
use ordered::{Matrix, OrderedParams, OrderedPreset};
use palette::{ColorSettings, Palette, Quantizer};
use preprocess::{PreprocessSettings, preprocess};

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

/// Preprocesses and dithers `src` with `settings`. The result is `pixel_size` times smaller than
/// `src` in each dimension. Returns `None` if `cancelled` becomes true mid-run.
pub fn process(src: &RgbaImage, settings: &Settings, cancelled: Cancel) -> Option<RgbaImage> {
    let prepared = preprocess(src, &settings.preprocess);
    let src = &prepared;
    let quantizer = Quantizer::new(&settings.palette, settings.color);
    match settings.method {
        MethodKind::Diffusion => diffuse(
            src,
            &DiffusionParams {
                kernel: &settings.diffusion.kernel,
                quantizer: &quantizer,
                serpentine: settings.diffusion.serpentine,
                strength: settings.diffusion.strength,
            },
            cancelled,
        ),
        MethodKind::Ordered => ordered::dither(
            src,
            &OrderedParams {
                matrix: &settings.ordered.matrix,
                quantizer: &quantizer,
                spread: settings.ordered.spread,
            },
            cancelled,
        ),
        MethodKind::Noise => noise::dither(
            src,
            &NoiseParams {
                kind: settings.noise.kind,
                quantizer: &quantizer,
                spread: settings.noise.spread,
                seed: settings.noise.seed,
            },
            cancelled,
        ),
    }
}
