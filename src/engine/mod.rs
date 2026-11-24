pub mod diffusion;
pub mod kernels;
pub mod palette;

use image::RgbaImage;

use diffusion::{DiffusionParams, diffuse};
use kernels::{BuiltinKernel, Kernel};
use palette::Palette;

#[derive(Clone, PartialEq, Debug)]
pub struct Settings {
    pub kernel: Kernel,
    pub serpentine: bool,
    pub strength: f32,
    pub palette: Palette,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            kernel: BuiltinKernel::FloydSteinberg.kernel(),
            serpentine: true,
            strength: 1.0,
            palette: Palette::black_white(),
        }
    }
}

/// Dithers `src` with `settings`. Returns `None` if `cancelled` becomes true mid-run.
pub fn process(
    src: &RgbaImage,
    settings: &Settings,
    cancelled: &dyn Fn() -> bool,
) -> Option<RgbaImage> {
    diffuse(
        src,
        &DiffusionParams {
            kernel: &settings.kernel,
            palette: &settings.palette,
            serpentine: settings.serpentine,
            strength: settings.strength,
        },
        cancelled,
    )
}
