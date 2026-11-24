use image::RgbaImage;

pub const MAX_PALETTE_COLORS: usize = 256;

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

    /// One `RRGGBB` color per line, as used by Lospec `.hex` files.
    pub fn to_hex(&self) -> String {
        self.colors
            .iter()
            .map(|c| format!("{:02x}{:02x}{:02x}\n", c[0], c[1], c[2]))
            .collect()
    }

    pub fn from_hex(text: &str) -> Result<Self, String> {
        let mut colors = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with("//") {
                continue;
            }
            let hex = line.strip_prefix('#').unwrap_or(line);
            let valid = hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit());
            if !valid {
                return Err(format!("line {}: expected a color like RRGGBB", n + 1));
            }
            let channel = |i: usize| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
            colors.push([channel(0), channel(1), channel(2)]);
        }
        Self::checked(colors)
    }

    pub fn to_gpl(&self, name: &str) -> String {
        let mut out = format!("GIMP Palette\nName: {name}\nColumns: 8\n#\n");
        for (i, c) in self.colors.iter().enumerate() {
            out.push_str(&format!(
                "{:3} {:3} {:3}\tColor {}\n",
                c[0],
                c[1],
                c[2],
                i + 1
            ));
        }
        out
    }

    pub fn from_gpl(text: &str) -> Result<Self, String> {
        let mut lines = text
            .lines()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty());
        match lines.next() {
            Some((_, header)) if header.trim() == "GIMP Palette" => {}
            _ => return Err("missing 'GIMP Palette' header".into()),
        }
        let mut colors = Vec::new();
        for (n, line) in lines {
            let line = line.trim();
            if line.starts_with('#') || line.starts_with(|c: char| c.is_alphabetic()) {
                continue;
            }
            let mut parts = line.split_whitespace().map(|p| p.parse::<u8>());
            match (parts.next(), parts.next(), parts.next()) {
                (Some(Ok(r)), Some(Ok(g)), Some(Ok(b))) => colors.push([r, g, b]),
                _ => return Err(format!("line {}: expected 'R G B'", n + 1)),
            }
        }
        Self::checked(colors)
    }

    /// Parses `.gpl` files as GIMP palettes and everything else as a list of hex colors.
    pub fn parse(extension: &str, text: &str) -> Result<Self, String> {
        if extension.eq_ignore_ascii_case("gpl") {
            Self::from_gpl(text)
        } else {
            Self::from_hex(text)
        }
    }

    fn checked(mut colors: Vec<[u8; 3]>) -> Result<Self, String> {
        if colors.is_empty() {
            return Err("no colors found".into());
        }
        colors.truncate(MAX_PALETTE_COLORS);
        Ok(Self { colors })
    }

    /// Picks up to `count` representative colors from `img` using median cut.
    pub fn extract(img: &RgbaImage, count: usize) -> Self {
        let count = count.clamp(1, MAX_PALETTE_COLORS);
        let step = (img.width() as usize * img.height() as usize / 65_536).max(1);
        let pixels: Vec<[u8; 3]> = img
            .pixels()
            .step_by(step)
            .filter(|p| p[3] > 0)
            .map(|p| [p[0], p[1], p[2]])
            .collect();
        if pixels.is_empty() {
            return Self::black_white();
        }

        let channel_range = |b: &[[u8; 3]], ch: usize| {
            let min = b.iter().map(|p| p[ch]).min().unwrap_or(0);
            let max = b.iter().map(|p| p[ch]).max().unwrap_or(0);
            max - min
        };
        let widest = |b: &[[u8; 3]]| (0..3).max_by_key(|&ch| channel_range(b, ch)).unwrap();

        let mut boxes = vec![pixels];
        while boxes.len() < count {
            let candidate = boxes
                .iter()
                .enumerate()
                .filter(|(_, b)| b.len() > 1)
                .map(|(i, b)| (i, channel_range(b, widest(b))))
                .filter(|&(_, range)| range > 0)
                .max_by_key(|&(_, range)| range);
            let Some((index, _)) = candidate else { break };

            let mut b = boxes.swap_remove(index);
            let ch = widest(&b);
            b.sort_unstable_by_key(|p| p[ch]);
            let upper = b.split_off(b.len() / 2);
            boxes.push(b);
            boxes.push(upper);
        }

        let mut colors: Vec<[u8; 3]> = boxes
            .iter()
            .map(|b| {
                let mean = |ch: usize| {
                    (b.iter().map(|p| p[ch] as u64).sum::<u64>() / b.len() as u64) as u8
                };
                [mean(0), mean(1), mean(2)]
            })
            .collect();
        colors.sort_by_key(|c| c[0] as u32 * 299 + c[1] as u32 * 587 + c[2] as u32 * 114);
        colors.dedup();
        Self { colors }
    }
}

pub fn presets() -> Vec<(&'static str, Palette)> {
    let hex = |list: &[u32]| {
        Palette::new(
            list.iter()
                .map(|c| [(c >> 16) as u8, (c >> 8) as u8, *c as u8])
                .collect(),
        )
    };
    vec![
        ("Black & white", Palette::black_white()),
        ("Grayscale 4", Palette::grayscale(4)),
        ("Grayscale 16", Palette::grayscale(16)),
        ("Game Boy", hex(&[0x0f380f, 0x306230, 0x8bac0f, 0x9bbc0f])),
        (
            "CGA (mode 4, palette 1)",
            hex(&[0x000000, 0x55ffff, 0xff55ff, 0xffffff]),
        ),
        (
            "EGA",
            hex(&[
                0x000000, 0x0000aa, 0x00aa00, 0x00aaaa, 0xaa0000, 0xaa00aa, 0xaa5500, 0xaaaaaa,
                0x555555, 0x5555ff, 0x55ff55, 0x55ffff, 0xff5555, 0xff55ff, 0xffff55, 0xffffff,
            ]),
        ),
        (
            "PICO-8",
            hex(&[
                0x000000, 0x1d2b53, 0x7e2553, 0x008751, 0xab5236, 0x5f574f, 0xc2c3c7, 0xfff1e8,
                0xff004d, 0xffa300, 0xffec27, 0x00e436, 0x29adff, 0x83769c, 0xff77a8, 0xffccaa,
            ]),
        ),
    ]
}

/// How "closest palette color" is measured.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Metric {
    /// Euclidean distance on the working-space channel values.
    Rgb,
    /// Euclidean distance in Oklab, which tracks perceived color difference better.
    Oklab,
}

impl Metric {
    pub const ALL: [Metric; 2] = [Self::Rgb, Self::Oklab];

    pub fn name(self) -> &'static str {
        match self {
            Self::Rgb => "RGB",
            Self::Oklab => "Oklab (perceptual)",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ColorSettings {
    /// Do the dithering arithmetic on linear light instead of gamma-encoded sRGB values.
    pub linear_light: bool,
    pub metric: Metric,
}

impl Default for ColorSettings {
    fn default() -> Self {
        Self {
            linear_light: false,
            metric: Metric::Rgb,
        }
    }
}

/// A palette prepared for fast matching. "Working" values are 0..=255 per channel, either
/// the plain sRGB values or linear light scaled to the same range.
pub struct Quantizer {
    pub colors: Vec<[u8; 3]>,
    working: Vec<[f32; 3]>,
    lab: Vec<[f32; 3]>,
    metric: Metric,
    linear_light: bool,
    decode: [f32; 256],
}

impl Quantizer {
    pub fn new(palette: &Palette, settings: ColorSettings) -> Self {
        let mut decode = [0.0; 256];
        for (i, d) in decode.iter_mut().enumerate() {
            *d = if settings.linear_light {
                srgb_to_linear(i as f32 / 255.0) * 255.0
            } else {
                i as f32
            };
        }
        let mut q = Self {
            colors: palette.colors.clone(),
            working: Vec::new(),
            lab: Vec::new(),
            metric: settings.metric,
            linear_light: settings.linear_light,
            decode,
        };
        q.working = q.colors.iter().map(|&c| q.to_working(c)).collect();
        q.lab = q.working.iter().map(|&w| q.oklab(w)).collect();
        q
    }

    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }

    pub fn to_working(&self, px: [u8; 3]) -> [f32; 3] {
        px.map(|c| self.decode[c as usize])
    }

    pub fn working(&self, index: usize) -> [f32; 3] {
        self.working[index]
    }

    fn oklab(&self, working: [f32; 3]) -> [f32; 3] {
        let linear = working.map(|v| {
            let v = v / 255.0;
            if self.linear_light {
                v
            } else {
                srgb_to_linear(v)
            }
        });
        linear_to_oklab(linear)
    }

    /// Index of the palette color closest to `working`. The palette must not be empty.
    pub fn nearest(&self, working: [f32; 3]) -> usize {
        let value = working.map(|v| v.clamp(0.0, 255.0));
        let (target, candidates) = match self.metric {
            Metric::Rgb => (value, &self.working),
            Metric::Oklab => (self.oklab(value), &self.lab),
        };
        let mut best = 0;
        let mut best_dist = f32::INFINITY;
        for (i, c) in candidates.iter().enumerate() {
            let d = [target[0] - c[0], target[1] - c[1], target[2] - c[2]];
            let dist = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            if dist < best_dist {
                best_dist = dist;
                best = i;
            }
        }
        best
    }

    /// Typical per-channel distance between neighbouring palette colors (255 for black and white).
    pub fn step_estimate(&self) -> f32 {
        if self.working.len() < 2 {
            return 0.0;
        }
        let dist = |a: &[f32; 3], b: &[f32; 3]| {
            let d = |i: usize| a[i] - b[i];
            (d(0).powi(2) + d(1).powi(2) + d(2).powi(2)).sqrt()
        };
        let total: f32 = self
            .working
            .iter()
            .enumerate()
            .map(|(i, a)| {
                self.working
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .map(|(_, b)| dist(a, b))
                    .fold(f32::INFINITY, f32::min)
            })
            .sum();
        total / self.working.len() as f32 / 3f32.sqrt()
    }
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_oklab([r, g, b]: [f32; 3]) -> [f32; 3] {
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn rgb_quantizer(palette: &Palette) -> Quantizer {
        Quantizer::new(palette, ColorSettings::default())
    }

    #[test]
    fn nearest_picks_closest_color() {
        let q = rgb_quantizer(&Palette::black_white());
        assert_eq!(q.nearest([10.0, 10.0, 10.0]), 0);
        assert_eq!(q.nearest([200.0, 200.0, 200.0]), 1);
    }

    #[test]
    fn nearest_clamps_out_of_range_values() {
        let q = Quantizer::new(
            &Palette::black_white(),
            ColorSettings {
                linear_light: false,
                metric: Metric::Oklab,
            },
        );
        assert_eq!(q.nearest([-500.0, -500.0, -500.0]), 0);
        assert_eq!(q.nearest([900.0, 900.0, 900.0]), 1);
    }

    #[test]
    fn oklab_of_white_and_black() {
        let white = linear_to_oklab([1.0, 1.0, 1.0]);
        assert!((white[0] - 1.0).abs() < 1e-3);
        assert!(white[1].abs() < 1e-3 && white[2].abs() < 1e-3);
        assert_eq!(linear_to_oklab([0.0, 0.0, 0.0]), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn oklab_matching_prefers_perceptually_closer_color() {
        let palette = Palette::new(vec![[0, 0, 0], [255, 255, 255], [255, 0, 0]]);
        let q = Quantizer::new(
            &palette,
            ColorSettings {
                linear_light: false,
                metric: Metric::Oklab,
            },
        );
        assert_eq!(q.nearest([230.0, 20.0, 20.0]), 2);
        assert_eq!(q.nearest([30.0, 30.0, 30.0]), 0);
    }

    #[test]
    fn linear_light_decodes_mid_gray_darker() {
        let q = Quantizer::new(
            &Palette::black_white(),
            ColorSettings {
                linear_light: true,
                metric: Metric::Rgb,
            },
        );
        let w = q.to_working([128, 128, 128]);
        assert!((w[0] - 55.0).abs() < 1.0, "{}", w[0]);
        assert_eq!(q.to_working([255, 255, 255]), [255.0; 3]);
    }

    #[test]
    fn step_estimate_matches_gray_steps() {
        assert!((rgb_quantizer(&Palette::black_white()).step_estimate() - 255.0).abs() < 0.01);
        assert!((rgb_quantizer(&Palette::grayscale(4)).step_estimate() - 85.0).abs() < 0.01);
        assert_eq!(
            rgb_quantizer(&Palette::new(vec![[1, 2, 3]])).step_estimate(),
            0.0
        );
    }

    #[test]
    fn grayscale_spans_full_range() {
        let p = Palette::grayscale(4);
        assert_eq!(p.colors.first(), Some(&[0, 0, 0]));
        assert_eq!(p.colors.last(), Some(&[255, 255, 255]));
        assert_eq!(p.colors.len(), 4);
    }

    #[test]
    fn hex_round_trips() {
        let p = Palette::new(vec![[0, 0, 0], [255, 128, 1], [18, 52, 86]]);
        assert_eq!(Palette::from_hex(&p.to_hex()).unwrap(), p);
    }

    #[test]
    fn hex_accepts_hash_comments_and_blank_lines() {
        let p = Palette::from_hex("; my palette\n\n#FF0000\n00ff00\n").unwrap();
        assert_eq!(p.colors, vec![[255, 0, 0], [0, 255, 0]]);
    }

    #[test]
    fn hex_rejects_garbage() {
        assert!(Palette::from_hex("hello").is_err());
        assert!(Palette::from_hex("12345").is_err());
        assert!(Palette::from_hex("").is_err());
    }

    #[test]
    fn gpl_round_trips() {
        let p = Palette::new(vec![[0, 0, 0], [255, 128, 1], [18, 52, 86]]);
        assert_eq!(Palette::from_gpl(&p.to_gpl("test")).unwrap(), p);
    }

    #[test]
    fn gpl_parses_gimp_file() {
        let text = "GIMP Palette\nName: Sample\nColumns: 4\n# comment\n 10  20  30\tDark blue\n255 255 255 White\n";
        let p = Palette::from_gpl(text).unwrap();
        assert_eq!(p.colors, vec![[10, 20, 30], [255, 255, 255]]);
        assert!(Palette::from_gpl("255 0 0\n").is_err());
    }

    #[test]
    fn parse_picks_format_by_extension() {
        assert!(Palette::parse("gpl", "ff0000\n").is_err());
        assert!(Palette::parse("hex", "ff0000\n").is_ok());
    }

    #[test]
    fn extract_finds_dominant_colors() {
        let mut img = RgbaImage::from_pixel(8, 8, Rgba([200, 30, 30, 255]));
        for x in 0..8 {
            for y in 0..4 {
                img.put_pixel(x, y, Rgba([20, 20, 200, 255]));
            }
        }
        let p = Palette::extract(&img, 2);
        assert_eq!(p.colors.len(), 2);
        assert!(p.colors.contains(&[200, 30, 30]));
        assert!(p.colors.contains(&[20, 20, 200]));
    }

    #[test]
    fn extract_respects_count_and_unique_colors() {
        let img = RgbaImage::from_fn(32, 32, |x, y| Rgba([(x * 8) as u8, (y * 8) as u8, 0, 255]));
        assert!(Palette::extract(&img, 8).colors.len() <= 8);
        let flat = RgbaImage::from_pixel(4, 4, Rgba([9, 9, 9, 255]));
        assert_eq!(Palette::extract(&flat, 5).colors, vec![[9, 9, 9]]);
    }
}
