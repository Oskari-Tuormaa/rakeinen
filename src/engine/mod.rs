pub mod diffusion;
pub mod kernels;
pub mod ordered;
pub mod palette;

use image::RgbaImage;

use diffusion::{DiffusionParams, diffuse};
use kernels::{BuiltinKernel, Kernel};
use ordered::{Matrix, OrderedParams, OrderedPreset};
use palette::Palette;

/// Reports whether the running job has been superseded and should stop early.
pub type Cancel<'a> = &'a (dyn Fn() -> bool + Sync);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MethodKind {
    Diffusion,
    Ordered,
}

impl MethodKind {
    pub const ALL: [MethodKind; 2] = [Self::Diffusion, Self::Ordered];

    pub fn name(self) -> &'static str {
        match self {
            Self::Diffusion => "Error diffusion",
            Self::Ordered => "Ordered",
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
pub struct Settings {
    pub method: MethodKind,
    pub diffusion: DiffusionSettings,
    pub ordered: OrderedSettings,
    pub palette: Palette,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
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
            palette: Palette::black_white(),
        }
    }
}

/// Dithers `src` with `settings`. Returns `None` if `cancelled` becomes true mid-run.
pub fn process(src: &RgbaImage, settings: &Settings, cancelled: Cancel) -> Option<RgbaImage> {
    match settings.method {
        MethodKind::Diffusion => diffuse(
            src,
            &DiffusionParams {
                kernel: &settings.diffusion.kernel,
                palette: &settings.palette,
                serpentine: settings.diffusion.serpentine,
                strength: settings.diffusion.strength,
            },
            cancelled,
        ),
        MethodKind::Ordered => ordered::dither(
            src,
            &OrderedParams {
                matrix: &settings.ordered.matrix,
                palette: &settings.palette,
                spread: settings.ordered.spread,
            },
            cancelled,
        ),
    }
}
