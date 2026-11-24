use std::collections::HashMap;

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

    /// Picks `count` representative colors from `img` with weighted k-means in Oklab.
    ///
    /// Pixels are first grouped into a histogram of similar colors, so large flat areas are one
    /// heavy point instead of millions of samples. `variety` runs from 0 to 1: at 0 every
    /// group counts by its pixel count, and at 1 every distinct group counts equally, which
    /// favours small accents over large areas.
    pub fn extract(img: &RgbaImage, count: usize, variety: f32) -> Self {
        let count = count.clamp(1, MAX_PALETTE_COLORS);
        let points = color_histogram(img, 1.0 - variety.clamp(0.0, 1.0));
        if points.is_empty() {
            return Self::black_white();
        }

        let mut colors: Vec<[u8; 3]> = if points.len() <= count {
            points
                .iter()
                .map(|p| p.rgb.map(|c| c.round() as u8))
                .collect()
        } else {
            kmeans(&points, count)
        };
        colors.sort_by_key(|c| c[0] as u32 * 299 + c[1] as u32 * 587 + c[2] as u32 * 114);
        colors.dedup();
        Self { colors }
    }
}

/// A group of similar pixel colors.
struct ColorPoint {
    /// Mean sRGB color of the group, 0..=255 per channel.
    rgb: [f32; 3],
    lab: [f32; 3],
    weight: f32,
}

/// Groups the opaque pixels of `img` into 5-bit-per-channel bins. Each bin weighs
/// `pixel count ^ exponent`.
fn color_histogram(img: &RgbaImage, exponent: f32) -> Vec<ColorPoint> {
    const SAMPLE_LIMIT: usize = 250_000;
    let step = (img.width() as usize * img.height() as usize / SAMPLE_LIMIT).max(1);

    let mut bins: HashMap<u16, ([u64; 3], u32)> = HashMap::new();
    for p in img.pixels().step_by(step).filter(|p| p[3] > 0) {
        let key = ((p[0] >> 3) as u16) << 10 | ((p[1] >> 3) as u16) << 5 | (p[2] >> 3) as u16;
        let bin = bins.entry(key).or_default();
        for (sum, &channel) in bin.0.iter_mut().zip(&p.0) {
            *sum += channel as u64;
        }
        bin.1 += 1;
    }

    // Hash maps iterate in random order; sorting keeps the result reproducible.
    let mut bins: Vec<_> = bins.into_iter().collect();
    bins.sort_unstable_by_key(|(key, _)| *key);
    bins.into_iter()
        .map(|(_, (sum, n))| {
            let rgb = sum.map(|s| s as f32 / n as f32);
            ColorPoint {
                lab: linear_to_oklab(rgb.map(|c| srgb_to_linear(c / 255.0))),
                rgb,
                weight: (n as f32).powf(exponent),
            }
        })
        .collect()
}

/// Splitmix64, for reproducible seeding without a dependency.
struct Rng(u64);

impl Rng {
    fn next_unit(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((z ^ (z >> 31)) >> 40) as f32 / (1u32 << 24) as f32
    }
}

fn distance_squared(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

/// Picks an index with probability proportional to `scores`.
fn pick_weighted(scores: &[f32], rng: &mut Rng) -> usize {
    let total: f32 = scores.iter().sum();
    let mut target = rng.next_unit() * total;
    for (i, &s) in scores.iter().enumerate() {
        if target < s {
            return i;
        }
        target -= s;
    }
    scores.len() - 1
}

/// Adds `value * weight` to `sum`.
fn add_scaled(sum: &mut [f32; 3], value: &[f32; 3], weight: f32) {
    for (s, v) in sum.iter_mut().zip(value) {
        *s += v * weight;
    }
}

fn kmeans(points: &[ColorPoint], k: usize) -> Vec<[u8; 3]> {
    const MAX_ITERATIONS: usize = 40;
    let mut rng = Rng(0x5eed);

    // k-means++ seeding: each new center is likely to be far from the existing ones.
    let weights: Vec<f32> = points.iter().map(|p| p.weight).collect();
    let mut centers = vec![points[pick_weighted(&weights, &mut rng)].lab];
    let mut nearest: Vec<f32> = points
        .iter()
        .map(|p| distance_squared(&p.lab, &centers[0]))
        .collect();
    while centers.len() < k {
        let scores: Vec<f32> = points
            .iter()
            .zip(&nearest)
            .map(|(p, d)| p.weight * d)
            .collect();
        if scores.iter().sum::<f32>() <= 0.0 {
            break;
        }
        let center = points[pick_weighted(&scores, &mut rng)].lab;
        for (p, d) in points.iter().zip(nearest.iter_mut()) {
            *d = d.min(distance_squared(&p.lab, &center));
        }
        centers.push(center);
    }

    let mut assignment = vec![usize::MAX; points.len()];
    for _ in 0..MAX_ITERATIONS {
        let mut changed = false;
        let mut cost = vec![0.0f32; points.len()];
        for (i, p) in points.iter().enumerate() {
            let (best, dist) = centers
                .iter()
                .map(|c| distance_squared(&p.lab, c))
                .enumerate()
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap();
            cost[i] = p.weight * dist;
            if assignment[i] != best {
                assignment[i] = best;
                changed = true;
            }
        }
        if !changed {
            break;
        }

        let mut sums = vec![[0.0f32; 3]; centers.len()];
        let mut totals = vec![0.0f32; centers.len()];
        for (p, &a) in points.iter().zip(&assignment) {
            add_scaled(&mut sums[a], &p.lab, p.weight);
            totals[a] += p.weight;
        }
        for (i, center) in centers.iter_mut().enumerate() {
            if totals[i] > 0.0 {
                *center = sums[i].map(|s| s / totals[i]);
            } else {
                // An empty cluster restarts at the worst-served point.
                let worst = (0..points.len())
                    .max_by(|&a, &b| cost[a].total_cmp(&cost[b]))
                    .unwrap();
                *center = points[worst].lab;
                cost[worst] = 0.0;
            }
        }
    }

    let mut sums = vec![[0.0f32; 3]; centers.len()];
    let mut totals = vec![0.0f32; centers.len()];
    for (p, &a) in points.iter().zip(&assignment) {
        add_scaled(&mut sums[a], &p.rgb, p.weight);
        totals[a] += p.weight;
    }
    sums.iter()
        .zip(&totals)
        .filter(|(_, t)| **t > 0.0)
        .map(|(s, t)| s.map(|v| (v / t).round().clamp(0.0, 255.0) as u8))
        .collect()
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
        let p = Palette::extract(&img, 2, 0.5);
        assert_eq!(p.colors.len(), 2);
        assert!(p.colors.contains(&[200, 30, 30]));
        assert!(p.colors.contains(&[20, 20, 200]));
    }

    /// About 90% shades of one blue, plus small red, green and yellow patches.
    fn mostly_blue_image() -> RgbaImage {
        RgbaImage::from_fn(100, 100, |x, y| {
            let i = x + y * 100;
            if i < 9_000 {
                let (r, g, b) = (20 + i % 7, 60 + i % 11, 200 + i % 40);
                Rgba([r as u8, g as u8, b as u8, 255])
            } else if i < 9_330 {
                Rgba([200, 30, 30, 255])
            } else if i < 9_660 {
                Rgba([30, 200, 30, 255])
            } else {
                Rgba([230, 220, 40, 255])
            }
        })
    }

    #[test]
    fn extract_does_not_waste_colors_on_shades_of_a_dominant_color() {
        let colors = Palette::extract(&mostly_blue_image(), 4, 0.5).colors;
        let count = |f: fn(&[u8; 3]) -> bool| colors.iter().filter(|c| f(c)).count();
        assert_eq!(colors.len(), 4, "{colors:?}");
        assert_eq!(count(|c| c[2] > 150 && c[0] < 80), 1, "blue in {colors:?}");
        assert_eq!(
            count(|c| c[0] > 150 && c[1] < 100 && c[2] < 100),
            1,
            "red in {colors:?}"
        );
        assert_eq!(
            count(|c| c[1] > 150 && c[0] < 100 && c[2] < 100),
            1,
            "green in {colors:?}"
        );
        assert_eq!(
            count(|c| c[0] > 180 && c[1] > 180 && c[2] < 120),
            1,
            "yellow in {colors:?}"
        );
    }

    #[test]
    fn extract_is_reproducible() {
        let img = mostly_blue_image();
        assert_eq!(
            Palette::extract(&img, 6, 0.3),
            Palette::extract(&img, 6, 0.3)
        );
    }

    #[test]
    fn extract_returns_the_requested_count_at_any_variety() {
        let img = RgbaImage::from_fn(64, 64, |x, y| {
            Rgba([(x * 4) as u8, (y * 4) as u8, ((x ^ y) * 4) as u8, 255])
        });
        for variety in [0.0, 0.5, 1.0] {
            assert_eq!(
                Palette::extract(&img, 12, variety).colors.len(),
                12,
                "variety {variety}"
            );
        }
    }

    #[test]
    fn extract_ignores_transparent_pixels() {
        let mut img = RgbaImage::from_pixel(8, 8, Rgba([255, 0, 0, 0]));
        img.put_pixel(0, 0, Rgba([0, 0, 255, 255]));
        assert_eq!(Palette::extract(&img, 3, 0.5).colors, vec![[0, 0, 255]]);
    }

    #[test]
    fn extract_respects_count_and_unique_colors() {
        let img = RgbaImage::from_fn(32, 32, |x, y| Rgba([(x * 8) as u8, (y * 8) as u8, 0, 255]));
        assert!(Palette::extract(&img, 8, 0.5).colors.len() <= 8);
        let flat = RgbaImage::from_pixel(4, 4, Rgba([9, 9, 9, 255]));
        assert_eq!(Palette::extract(&flat, 5, 0.5).colors, vec![[9, 9, 9]]);
    }
}
