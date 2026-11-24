use std::f32::consts::PI;

use image::RgbaImage;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use super::Cancel;
use super::palette::Quantizer;

pub const MAX_MATRIX_SIZE: usize = 16;

/// A threshold matrix that is tiled across the image. Any width and height work.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Matrix {
    pub width: usize,
    pub height: usize,
    pub values: Vec<u32>,
}

impl Matrix {
    pub fn new(width: usize, height: usize, values: Vec<u32>) -> Self {
        assert_eq!(values.len(), width * height);
        Self {
            width,
            height,
            values,
        }
    }

    pub fn from_fn(width: usize, height: usize, f: impl Fn(usize, usize) -> u32) -> Self {
        let values = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .map(|(x, y)| f(x, y))
            .collect();
        Self::new(width, height, values)
    }

    /// Whether the matrix is non-empty and its values match its dimensions.
    pub fn is_well_formed(&self) -> bool {
        self.width > 0 && self.height > 0 && self.values.len() == self.width * self.height
    }

    /// Checks a user-supplied matrix against the sizes the editor supports.
    pub fn validate(&self) -> Result<(), String> {
        let ok_size = |n: usize| (1..=MAX_MATRIX_SIZE).contains(&n);
        if !ok_size(self.width) || !ok_size(self.height) {
            return Err(format!("matrix size must be 1 to {MAX_MATRIX_SIZE}"));
        }
        if !self.is_well_formed() {
            return Err("matrix value count does not match its size".into());
        }
        Ok(())
    }

    /// Resizes the matrix, keeping the values of cells that stay inside it.
    pub fn resize(&mut self, width: usize, height: usize) {
        let width = width.clamp(1, MAX_MATRIX_SIZE);
        let height = height.clamp(1, MAX_MATRIX_SIZE);
        let mut values = vec![0; width * height];
        for y in 0..self.height.min(height) {
            for x in 0..self.width.min(width) {
                values[y * width + x] = self.values[y * self.width + x];
            }
        }
        self.width = width;
        self.height = height;
        self.values = values;
    }

    /// Replaces the values with their ranks `0..n`, ties broken by position.
    pub fn rank_normalize(&mut self) {
        let mut order: Vec<usize> = (0..self.values.len()).collect();
        order.sort_by_key(|&i| (self.values[i], i));
        for (rank, &i) in order.iter().enumerate() {
            self.values[i] = rank as u32;
        }
    }

    /// Per-cell thresholds in `-0.5..0.5`, scaled by the largest value in the matrix.
    pub fn thresholds(&self) -> Vec<f32> {
        let max = self.values.iter().copied().max().unwrap_or(0) as f32;
        self.values
            .iter()
            .map(|&v| (v as f32 + 0.5) / (max + 1.0) - 0.5)
            .collect()
    }

    pub fn bayer(size: usize) -> Self {
        assert!(size.is_power_of_two());
        let mut values = vec![0u32];
        let mut n = 1;
        while n < size {
            let mut next = vec![0u32; (n * 2) * (n * 2)];
            for y in 0..n {
                for x in 0..n {
                    let v = values[y * n + x] * 4;
                    next[y * 2 * n + x] = v;
                    next[y * 2 * n + x + n] = v + 2;
                    next[(y + n) * 2 * n + x] = v + 3;
                    next[(y + n) * 2 * n + x + n] = v + 1;
                }
            }
            values = next;
            n *= 2;
        }
        Self::new(size, size, values)
    }

    /// Dark dots that grow from the center of each tile.
    pub fn clustered_dot(size: usize) -> Self {
        let center = (size as f32 - 1.0) / 2.0;
        let key = |i: usize| {
            let (x, y) = ((i % size) as f32 - center, (i / size) as f32 - center);
            (x * x + y * y, y.atan2(x) + PI)
        };
        let mut order: Vec<usize> = (0..size * size).collect();
        order.sort_by(|&a, &b| key(a).partial_cmp(&key(b)).unwrap());
        let mut values = vec![0; size * size];
        for (i, &cell) in order.iter().enumerate() {
            values[cell] = (size * size - 1 - i) as u32;
        }
        Self::new(size, size, values)
    }

    pub fn horizontal_lines(size: usize) -> Self {
        Self::from_fn(size, size, |x, y| (y * size + stagger(x, size)) as u32)
    }

    pub fn diagonal_lines(size: usize) -> Self {
        Self::from_fn(size, size, |x, y| {
            (((x + y) % size) * size + stagger(x, size)) as u32
        })
    }
}

/// Visits `0..n` in a spread-out order so that partial coverage does not clump left to right.
fn stagger(i: usize, n: usize) -> usize {
    let bits = n.next_power_of_two().trailing_zeros();
    if bits == 0 {
        return 0;
    }
    (i.reverse_bits() >> (usize::BITS - bits)) % n
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrderedPreset {
    Bayer2,
    Bayer4,
    Bayer8,
    Bayer16,
    Dot4,
    Dot8,
    HorizontalLines,
    DiagonalLines,
}

impl OrderedPreset {
    pub const ALL: [OrderedPreset; 8] = [
        Self::Bayer2,
        Self::Bayer4,
        Self::Bayer8,
        Self::Bayer16,
        Self::Dot4,
        Self::Dot8,
        Self::HorizontalLines,
        Self::DiagonalLines,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Bayer2 => "Bayer 2×2",
            Self::Bayer4 => "Bayer 4×4",
            Self::Bayer8 => "Bayer 8×8",
            Self::Bayer16 => "Bayer 16×16",
            Self::Dot4 => "Clustered dot 4×4",
            Self::Dot8 => "Clustered dot 8×8",
            Self::HorizontalLines => "Horizontal lines",
            Self::DiagonalLines => "Diagonal lines",
        }
    }

    pub fn matrix(self) -> Matrix {
        match self {
            Self::Bayer2 => Matrix::bayer(2),
            Self::Bayer4 => Matrix::bayer(4),
            Self::Bayer8 => Matrix::bayer(8),
            Self::Bayer16 => Matrix::bayer(16),
            Self::Dot4 => Matrix::clustered_dot(4),
            Self::Dot8 => Matrix::clustered_dot(8),
            Self::HorizontalLines => Matrix::horizontal_lines(4),
            Self::DiagonalLines => Matrix::diagonal_lines(4),
        }
    }
}

pub struct OrderedParams<'a> {
    pub matrix: &'a Matrix,
    pub quantizer: &'a Quantizer,
    /// Multiplier on the estimated distance between palette steps.
    pub spread: f32,
}

/// Runs ordered dithering. Returns `None` if `cancelled` reports true.
pub fn dither(src: &RgbaImage, params: &OrderedParams, cancelled: Cancel) -> Option<RgbaImage> {
    if !params.matrix.is_well_formed() {
        return Some(src.clone());
    }
    let thresholds = params.matrix.thresholds();
    let (mw, mh) = (params.matrix.width, params.matrix.height);
    dither_with(src, params.quantizer, params.spread, cancelled, |x, y| {
        thresholds[(y % mh) * mw + x % mw]
    })
}

/// Shared dithering core. `threshold(x, y)` returns a value in `-0.5..0.5` for each pixel.
pub fn dither_with(
    src: &RgbaImage,
    q: &Quantizer,
    spread: f32,
    cancelled: Cancel,
    threshold: impl Fn(usize, usize) -> f32 + Sync,
) -> Option<RgbaImage> {
    let (w, h) = (src.width() as usize, src.height() as usize);
    if q.is_empty() || w == 0 || h == 0 {
        return Some(src.clone());
    }
    let amplitude = spread * q.step_estimate();

    let mut buf = src.as_raw().clone();
    buf.par_chunks_mut(w * 4)
        .enumerate()
        .try_for_each(|(y, row)| {
            if cancelled() {
                return Err(());
            }
            for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                // A low threshold lets a pixel turn light first, so it is subtracted.
                let offset = threshold(x, y) * amplitude;
                let working = q.to_working([px[0], px[1], px[2]]);
                let value = working.map(|v| v - offset);
                let c = q.colors[q.nearest(value)];
                px[..3].copy_from_slice(&c);
            }
            Ok(())
        })
        .ok()?;

    RgbaImage::from_raw(w as u32, h as u32, buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::palette::{ColorSettings, Palette};
    use image::Rgba;

    fn gray_image(w: u32, h: u32, v: u8) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([v, v, v, 255]))
    }

    fn run(img: &RgbaImage, matrix: &Matrix, palette: &Palette) -> RgbaImage {
        let quantizer = Quantizer::new(palette, ColorSettings::default());
        let params = OrderedParams {
            matrix,
            quantizer: &quantizer,
            spread: 1.0,
        };
        dither(img, &params, &|| false).unwrap()
    }

    fn is_permutation(m: &Matrix) -> bool {
        let mut v = m.values.clone();
        v.sort();
        v.iter().enumerate().all(|(i, &x)| x == i as u32)
    }

    #[test]
    fn bayer_2_matches_reference() {
        assert_eq!(Matrix::bayer(2).values, vec![0, 2, 3, 1]);
    }

    #[test]
    fn bayer_4_matches_reference() {
        assert_eq!(
            Matrix::bayer(4).values,
            vec![0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5]
        );
    }

    #[test]
    fn presets_are_valid_permutations() {
        for preset in OrderedPreset::ALL {
            let m = preset.matrix();
            assert!(m.validate().is_ok(), "{}", preset.name());
            assert!(is_permutation(&m), "{}", preset.name());
        }
    }

    #[test]
    fn mid_gray_is_half_white_with_bayer() {
        let out = run(
            &gray_image(64, 64, 128),
            &Matrix::bayer(8),
            &Palette::black_white(),
        );
        let white = out.pixels().filter(|p| p[0] == 255).count();
        assert_eq!(white, 64 * 64 / 2);
    }

    #[test]
    fn output_is_periodic_for_flat_input() {
        let m = Matrix::bayer(4);
        let out = run(&gray_image(16, 16, 90), &m, &Palette::black_white());
        for y in 0..12 {
            for x in 0..12 {
                assert_eq!(out.get_pixel(x, y), out.get_pixel(x + 4, y + 4));
            }
        }
    }

    #[test]
    fn works_with_non_square_matrices() {
        let m = Matrix::new(3, 2, vec![0, 2, 4, 5, 3, 1]);
        let out = run(&gray_image(10, 7, 100), &m, &Palette::black_white());
        assert!(out.pixels().all(|p| p[0] == 0 || p[0] == 255));
    }

    #[test]
    fn rank_normalize_produces_permutation() {
        let mut m = Matrix::new(2, 2, vec![50, 7, 7, 900]);
        m.rank_normalize();
        assert_eq!(m.values, vec![2, 0, 1, 3]);
    }

    #[test]
    fn resize_keeps_overlap() {
        let mut m = Matrix::bayer(4);
        m.resize(2, 3);
        assert_eq!((m.width, m.height), (2, 3));
        assert_eq!(m.values, vec![0, 8, 12, 4, 3, 11]);
    }

    #[test]
    fn cancellation_returns_none() {
        let m = Matrix::bayer(2);
        let q = Quantizer::new(&Palette::black_white(), ColorSettings::default());
        let params = OrderedParams {
            matrix: &m,
            quantizer: &q,
            spread: 1.0,
        };
        assert!(dither(&gray_image(4, 4, 10), &params, &|| true).is_none());
    }
}
