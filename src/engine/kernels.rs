use serde::{Deserialize, Serialize};

/// An error diffusion kernel stored as a grid, as shown in the kernel editor.
///
/// Row 0 is the current scanline and the current pixel sits at column `cols / 2`.
/// Only cells that come after the current pixel in scan order carry meaning: the cells to
/// the right of the center in row 0, and every cell in the rows below.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
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

    /// Resizes the grid keeping every weight at the same offset from the center pixel.
    /// `cols` is rounded up to an odd number so the center column stays well defined.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        let rows = rows.max(1);
        let cols = (cols.max(1)) | 1;
        let mut weights = vec![0.0; rows * cols];
        let new_center = cols / 2;
        for y in 0..self.rows.min(rows) {
            for x in 0..self.cols {
                let nx = x as i32 - self.center() as i32 + new_center as i32;
                if nx >= 0 && (nx as usize) < cols {
                    weights[y * cols + nx as usize] = self.weights[y * self.cols + x];
                }
            }
        }
        self.rows = rows;
        self.cols = cols;
        self.weights = weights;
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.rows == 0 || self.cols == 0 || self.cols.is_multiple_of(2) {
            return Err("kernel needs at least one row and an odd number of columns".into());
        }
        if self.weights.len() != self.rows * self.cols {
            return Err("kernel weight count does not match its size".into());
        }
        if !self.divisor.is_finite() || self.divisor == 0.0 {
            return Err("kernel divisor must be a non-zero number".into());
        }
        Ok(())
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
    fn resize_keeps_weights_relative_to_center() {
        let mut k = BuiltinKernel::FloydSteinberg.kernel();
        let before = k.taps();
        k.resize(3, 7);
        assert_eq!((k.rows, k.cols), (3, 7));
        let mut after = k.taps();
        let mut before = before;
        let key = |t: &Tap| (t.dy, t.dx);
        before.sort_by_key(key);
        after.sort_by_key(key);
        assert_eq!(before.len(), after.len());
        for (a, b) in before.iter().zip(&after) {
            assert_eq!(key(a), key(b));
            assert!((a.weight - b.weight).abs() < 1e-6);
        }
    }

    #[test]
    fn resize_forces_odd_columns() {
        let mut k = BuiltinKernel::Burkes.kernel();
        k.resize(2, 4);
        assert_eq!(k.cols, 5);
    }

    #[test]
    fn validate_rejects_bad_kernels() {
        let mut k = BuiltinKernel::Atkinson.kernel();
        assert!(k.validate().is_ok());
        k.divisor = 0.0;
        assert!(k.validate().is_err());
        k.divisor = 8.0;
        k.weights.pop();
        assert!(k.validate().is_err());
    }

    #[test]
    fn non_forward_cells_are_ignored() {
        let k = Kernel::from_grid(1.0, &[&[9., 9., 0.], &[1., 0., 0.]]);
        let taps = k.taps();
        assert_eq!(taps.len(), 1);
        assert_eq!((taps[0].dx, taps[0].dy), (-1, 1));
    }
}
