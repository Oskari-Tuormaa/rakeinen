use std::sync::OnceLock;

use image::RgbaImage;

use super::Cancel;
use super::ordered::{Matrix, dither_with};
use super::palette::Palette;

const BLUE_NOISE_SIZE: usize = 64;
const BLUE_NOISE_SIGMA: f32 = 1.5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoiseKind {
    White,
    Blue,
}

impl NoiseKind {
    pub const ALL: [NoiseKind; 2] = [Self::White, Self::Blue];

    pub fn name(self) -> &'static str {
        match self {
            Self::White => "White noise",
            Self::Blue => "Blue noise",
        }
    }
}

pub struct NoiseParams<'a> {
    pub kind: NoiseKind,
    pub palette: &'a Palette,
    pub spread: f32,
    /// Only used by white noise.
    pub seed: u32,
}

pub fn dither(src: &RgbaImage, params: &NoiseParams, cancelled: Cancel) -> Option<RgbaImage> {
    match params.kind {
        NoiseKind::White => {
            let seed = params.seed;
            dither_with(src, params.palette, params.spread, cancelled, |x, y| {
                white_threshold(x as u32, y as u32, seed)
            })
        }
        NoiseKind::Blue => {
            let matrix = blue_noise_matrix();
            let thresholds = matrix.thresholds();
            let size = matrix.width;
            dither_with(src, params.palette, params.spread, cancelled, |x, y| {
                thresholds[(y % size) * size + x % size]
            })
        }
    }
}

fn hash(mut v: u32) -> u32 {
    v ^= v >> 16;
    v = v.wrapping_mul(0x7feb_352d);
    v ^= v >> 15;
    v = v.wrapping_mul(0x846c_a68b);
    v ^ (v >> 16)
}

fn white_threshold(x: u32, y: u32, seed: u32) -> f32 {
    let h = hash(hash(x ^ hash(seed)).wrapping_add(y));
    (h >> 8) as f32 / (1u32 << 24) as f32 - 0.5
}

/// A tileable blue noise threshold matrix, generated once on first use.
pub fn blue_noise_matrix() -> &'static Matrix {
    static MATRIX: OnceLock<Matrix> = OnceLock::new();
    MATRIX.get_or_init(|| generate_blue_noise(BLUE_NOISE_SIZE))
}

/// Void-and-cluster blue noise (Ulichney, 1993). Values are a permutation of `0..size²`.
fn generate_blue_noise(size: usize) -> Matrix {
    let n = size * size;
    let radius = (BLUE_NOISE_SIGMA * 3.0).ceil() as i32;
    let mut offsets = Vec::new();
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let d2 = (dx * dx + dy * dy) as f32;
            offsets.push((
                dx,
                dy,
                (-d2 / (2.0 * BLUE_NOISE_SIGMA * BLUE_NOISE_SIGMA)).exp(),
            ));
        }
    }

    let splat = |energy: &mut [f32], idx: usize, sign: f32| {
        let (cx, cy) = ((idx % size) as i32, (idx / size) as i32);
        for &(dx, dy, w) in &offsets {
            let x = (cx + dx).rem_euclid(size as i32) as usize;
            let y = (cy + dy).rem_euclid(size as i32) as usize;
            energy[y * size + x] += sign * w;
        }
    };
    // Index of the cell with the highest (or lowest) energy among cells equal to `want`.
    let extreme = |pattern: &[bool], energy: &[f32], want: bool, highest: bool| {
        let mut best = usize::MAX;
        let mut best_energy = if highest { f32::MIN } else { f32::MAX };
        for i in 0..n {
            if pattern[i] != want {
                continue;
            }
            let better = if highest {
                energy[i] > best_energy
            } else {
                energy[i] < best_energy
            };
            if better {
                best = i;
                best_energy = energy[i];
            }
        }
        best
    };

    let mut pattern = vec![false; n];
    let mut state = 0x1234_5678u32;
    let mut placed = 0;
    while placed < n / 10 {
        state = hash(state.wrapping_add(0x9e37_79b9));
        let i = state as usize % n;
        if !pattern[i] {
            pattern[i] = true;
            placed += 1;
        }
    }
    let mut energy = vec![0.0f32; n];
    for i in (0..n).filter(|&i| pattern[i]) {
        splat(&mut energy, i, 1.0);
    }

    // Phase 1: relax the initial pattern until the tightest cluster is the largest void.
    loop {
        let cluster = extreme(&pattern, &energy, true, true);
        pattern[cluster] = false;
        splat(&mut energy, cluster, -1.0);
        let void = extreme(&pattern, &energy, false, false);
        pattern[void] = true;
        splat(&mut energy, void, 1.0);
        if void == cluster {
            break;
        }
    }

    let ones = pattern.iter().filter(|&&p| p).count();
    let mut ranks = vec![0u32; n];

    // Phase 2: rank the initial ones by repeatedly removing the tightest cluster.
    let mut p2 = pattern.clone();
    let mut e2 = energy.clone();
    for rank in (0..ones).rev() {
        let cluster = extreme(&p2, &e2, true, true);
        p2[cluster] = false;
        splat(&mut e2, cluster, -1.0);
        ranks[cluster] = rank as u32;
    }

    // Phase 3: rank the remaining cells by repeatedly filling the largest void.
    for rank in ones..n {
        let void = extreme(&pattern, &energy, false, false);
        pattern[void] = true;
        splat(&mut energy, void, 1.0);
        ranks[void] = rank as u32;
    }

    Matrix::new(size, size, ranks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn gray_image(w: u32, h: u32, v: u8) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([v, v, v, 255]))
    }

    fn run(kind: NoiseKind, v: u8, seed: u32) -> RgbaImage {
        let palette = Palette::black_white();
        let params = NoiseParams {
            kind,
            palette: &palette,
            spread: 1.0,
            seed,
        };
        dither(&gray_image(128, 128, v), &params, &|| false).unwrap()
    }

    fn white_ratio(img: &RgbaImage) -> f32 {
        img.pixels().filter(|p| p[0] == 255).count() as f32 / (img.width() * img.height()) as f32
    }

    /// Fraction of horizontally adjacent pixel pairs that are both white.
    fn adjacent_white(img: &RgbaImage) -> f32 {
        let mut pairs = 0;
        let mut both = 0;
        for y in 0..img.height() {
            for x in 0..img.width() - 1 {
                pairs += 1;
                if img.get_pixel(x, y)[0] == 255 && img.get_pixel(x + 1, y)[0] == 255 {
                    both += 1;
                }
            }
        }
        both as f32 / pairs as f32
    }

    #[test]
    fn blue_noise_is_a_permutation() {
        let m = blue_noise_matrix();
        assert!(m.is_well_formed());
        let mut v = m.values.clone();
        v.sort();
        assert!(v.iter().enumerate().all(|(i, &x)| x == i as u32));
    }

    #[test]
    fn blue_noise_hits_requested_density() {
        let ratio = white_ratio(&run(NoiseKind::Blue, 64, 0));
        assert!((ratio - 0.25).abs() < 0.01, "ratio {ratio}");
    }

    #[test]
    fn blue_noise_clumps_less_than_white_noise() {
        let blue = adjacent_white(&run(NoiseKind::Blue, 64, 0));
        let white = adjacent_white(&run(NoiseKind::White, 64, 0));
        assert!(blue < white * 0.6, "blue {blue}, white {white}");
    }

    #[test]
    fn white_noise_density_is_roughly_right() {
        let ratio = white_ratio(&run(NoiseKind::White, 128, 7));
        assert!((ratio - 0.5).abs() < 0.03, "ratio {ratio}");
    }

    #[test]
    fn white_noise_depends_on_seed_but_is_deterministic() {
        assert_eq!(run(NoiseKind::White, 100, 1), run(NoiseKind::White, 100, 1));
        assert_ne!(run(NoiseKind::White, 100, 1), run(NoiseKind::White, 100, 2));
    }
}
