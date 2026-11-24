#[derive(Clone, PartialEq, Debug)]
pub struct Palette {
    pub colors: Vec<[u8; 3]>,
}

impl Palette {
    pub fn new(colors: Vec<[u8; 3]>) -> Self {
        Self { colors }
    }

    pub fn black_white() -> Self {
        Self::grayscale(2)
    }

    pub fn grayscale(levels: usize) -> Self {
        let levels = levels.max(2);
        let colors = (0..levels)
            .map(|i| {
                let v = (i as f32 / (levels - 1) as f32 * 255.0).round() as u8;
                [v, v, v]
            })
            .collect();
        Self { colors }
    }

    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }

    /// Typical per-channel distance between neighbouring palette colors (255 for black and white).
    pub fn step_estimate(&self) -> f32 {
        if self.colors.len() < 2 {
            return 0.0;
        }
        let dist = |a: &[u8; 3], b: &[u8; 3]| {
            let d = |i: usize| a[i] as f32 - b[i] as f32;
            (d(0).powi(2) + d(1).powi(2) + d(2).powi(2)).sqrt()
        };
        let total: f32 = self
            .colors
            .iter()
            .enumerate()
            .map(|(i, a)| {
                self.colors
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .map(|(_, b)| dist(a, b))
                    .fold(f32::INFINITY, f32::min)
            })
            .sum();
        total / self.colors.len() as f32 / 3f32.sqrt()
    }

    /// Index of the palette color closest to `rgb` (0..=255 per channel). The palette must not be empty.
    pub fn nearest(&self, rgb: [f32; 3]) -> usize {
        let mut best = 0;
        let mut best_dist = f32::INFINITY;
        for (i, c) in self.colors.iter().enumerate() {
            let dr = rgb[0] - c[0] as f32;
            let dg = rgb[1] - c[1] as f32;
            let db = rgb[2] - c[2] as f32;
            let dist = dr * dr + dg * dg + db * db;
            if dist < best_dist {
                best_dist = dist;
                best = i;
            }
        }
        best
    }
}

pub fn presets() -> Vec<(&'static str, Palette)> {
    vec![
        ("Black & white", Palette::black_white()),
        ("Grayscale 4", Palette::grayscale(4)),
        ("Grayscale 16", Palette::grayscale(16)),
        (
            "Game Boy",
            Palette::new(vec![
                [0x0f, 0x38, 0x0f],
                [0x30, 0x62, 0x30],
                [0x8b, 0xac, 0x0f],
                [0x9b, 0xbc, 0x0f],
            ]),
        ),
        (
            "CGA (mode 4, palette 1)",
            Palette::new(vec![
                [0x00, 0x00, 0x00],
                [0x55, 0xff, 0xff],
                [0xff, 0x55, 0xff],
                [0xff, 0xff, 0xff],
            ]),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_picks_closest_color() {
        let p = Palette::black_white();
        assert_eq!(p.nearest([10.0, 10.0, 10.0]), 0);
        assert_eq!(p.nearest([200.0, 200.0, 200.0]), 1);
    }

    #[test]
    fn step_estimate_matches_gray_steps() {
        assert!((Palette::black_white().step_estimate() - 255.0).abs() < 0.01);
        assert!((Palette::grayscale(4).step_estimate() - 85.0).abs() < 0.01);
        assert_eq!(Palette::new(vec![[1, 2, 3]]).step_estimate(), 0.0);
    }

    #[test]
    fn grayscale_spans_full_range() {
        let p = Palette::grayscale(4);
        assert_eq!(p.colors.first(), Some(&[0, 0, 0]));
        assert_eq!(p.colors.last(), Some(&[255, 255, 255]));
        assert_eq!(p.colors.len(), 4);
    }
}
