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
    fn grayscale_spans_full_range() {
        let p = Palette::grayscale(4);
        assert_eq!(p.colors.first(), Some(&[0, 0, 0]));
        assert_eq!(p.colors.last(), Some(&[255, 255, 255]));
        assert_eq!(p.colors.len(), 4);
    }
}
