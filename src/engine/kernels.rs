/// An error diffusion kernel stored as a grid, as shown in the kernel editor.
///
/// Row 0 is the current scanline and the current pixel sits at column `cols / 2`.
/// Only cells that come after the current pixel in scan order carry meaning: the cells to
/// the right of the center in row 0, and every cell in the rows below.
#[derive(Clone, PartialEq, Debug)]
pub struct Kernel {
    pub cols: usize,
    pub rows: usize,
    pub weights: Vec<f32>,
    pub divisor: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Tap {
    pub dx: i32,
    pub dy: i32,
    pub weight: f32,
}

impl Kernel {
    pub fn from_grid(divisor: f32, grid: &[&[f32]]) -> Self {
        let rows = grid.len();
        let cols = grid.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut weights = vec![0.0; rows * cols];
        for (y, row) in grid.iter().enumerate() {
            for (x, w) in row.iter().enumerate() {
                weights[y * cols + x] = *w;
            }
        }
        Self {
            cols,
            rows,
            weights,
            divisor,
        }
    }

    pub fn center(&self) -> usize {
        self.cols / 2
    }

    pub fn is_forward(&self, x: usize, y: usize) -> bool {
        y > 0 || x > self.center()
    }

    /// The non-zero forward cells as offsets with weights already divided by the divisor.
    pub fn taps(&self) -> Vec<Tap> {
        let divisor = if self.divisor == 0.0 {
            1.0
        } else {
            self.divisor
        };
        let mut taps = Vec::new();
        for y in 0..self.rows {
            for x in 0..self.cols {
                let w = self.weights[y * self.cols + x];
                if w != 0.0 && self.is_forward(x, y) {
                    taps.push(Tap {
                        dx: x as i32 - self.center() as i32,
                        dy: y as i32,
                        weight: w / divisor,
                    });
                }
            }
        }
        taps
    }

    pub fn forward_sum(&self) -> f32 {
        self.taps().iter().map(|t| t.weight).sum()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuiltinKernel {
    FloydSteinberg,
    JarvisJudiceNinke,
    Stucki,
    Atkinson,
    Burkes,
    Sierra,
    SierraTwoRow,
    SierraLite,
}

impl BuiltinKernel {
    pub const ALL: [BuiltinKernel; 8] = [
        Self::FloydSteinberg,
        Self::JarvisJudiceNinke,
        Self::Stucki,
        Self::Atkinson,
        Self::Burkes,
        Self::Sierra,
        Self::SierraTwoRow,
        Self::SierraLite,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::FloydSteinberg => "Floyd–Steinberg",
            Self::JarvisJudiceNinke => "Jarvis–Judice–Ninke",
            Self::Stucki => "Stucki",
            Self::Atkinson => "Atkinson",
            Self::Burkes => "Burkes",
            Self::Sierra => "Sierra",
            Self::SierraTwoRow => "Sierra Two-Row",
            Self::SierraLite => "Sierra Lite",
        }
    }

    pub fn kernel(self) -> Kernel {
        match self {
            Self::FloydSteinberg => Kernel::from_grid(16.0, &[&[0., 0., 7.], &[3., 5., 1.]]),
            Self::JarvisJudiceNinke => Kernel::from_grid(
                48.0,
                &[
                    &[0., 0., 0., 7., 5.],
                    &[3., 5., 7., 5., 3.],
                    &[1., 3., 5., 3., 1.],
                ],
            ),
            Self::Stucki => Kernel::from_grid(
                42.0,
                &[
                    &[0., 0., 0., 8., 4.],
                    &[2., 4., 8., 4., 2.],
                    &[1., 2., 4., 2., 1.],
                ],
            ),
            Self::Atkinson => Kernel::from_grid(
                8.0,
                &[
                    &[0., 0., 0., 1., 1.],
                    &[0., 1., 1., 1., 0.],
                    &[0., 0., 1., 0., 0.],
                ],
            ),
            Self::Burkes => {
                Kernel::from_grid(32.0, &[&[0., 0., 0., 8., 4.], &[2., 4., 8., 4., 2.]])
            }
            Self::Sierra => Kernel::from_grid(
                32.0,
                &[
                    &[0., 0., 0., 5., 3.],
                    &[2., 4., 5., 4., 2.],
                    &[0., 2., 3., 2., 0.],
                ],
            ),
            Self::SierraTwoRow => {
                Kernel::from_grid(16.0, &[&[0., 0., 0., 4., 3.], &[1., 2., 3., 2., 1.]])
            }
            Self::SierraLite => Kernel::from_grid(4.0, &[&[0., 0., 2.], &[1., 1., 0.]]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_diffusion_kernels_sum_to_one() {
        for k in BuiltinKernel::ALL {
            let sum = k.kernel().forward_sum();
            if k == BuiltinKernel::Atkinson {
                assert!((sum - 0.75).abs() < 1e-6, "{}", k.name());
            } else {
                assert!((sum - 1.0).abs() < 1e-6, "{}: {sum}", k.name());
            }
        }
    }

    #[test]
    fn floyd_steinberg_taps() {
        let taps = BuiltinKernel::FloydSteinberg.kernel().taps();
        assert_eq!(taps.len(), 4);
        assert!(taps.iter().any(|t| t.dx == 1 && t.dy == 0));
        assert!(taps.iter().any(|t| t.dx == -1 && t.dy == 1));
    }

    #[test]
    fn non_forward_cells_are_ignored() {
        let k = Kernel::from_grid(1.0, &[&[9., 9., 0.], &[1., 0., 0.]]);
        let taps = k.taps();
        assert_eq!(taps.len(), 1);
        assert_eq!((taps[0].dx, taps[0].dy), (-1, 1));
    }
}
