use serde::{Deserialize, Serialize};

use crate::engine::Settings;

/// Marks a JSON file as a Rakeinen preset, so other JSON files are rejected with a clear error.
const FORMAT: &str = "rakeinen-preset";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct PresetFile {
    format: String,
    version: u32,
    settings: Settings,
}

pub fn to_json(settings: &Settings) -> Result<String, String> {
    let file = PresetFile {
        format: FORMAT.to_owned(),
        version: VERSION,
        settings: settings.clone(),
    };
    serde_json::to_string_pretty(&file).map_err(|e| e.to_string())
}

/// Parses a preset. Settings missing from the file keep their defaults, so presets stay
/// loadable as settings are added.
pub fn from_json(text: &str) -> Result<Settings, String> {
    let file: PresetFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if file.format != FORMAT {
        return Err("this is not a Rakeinen preset".into());
    }
    if file.version > VERSION {
        return Err(format!(
            "this preset was made by a newer version of Rakeinen (format {})",
            file.version
        ));
    }
    file.settings.sanitized()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::MethodKind;
    use crate::engine::kernels::BuiltinKernel;
    use crate::engine::noise::NoiseKind;
    use crate::engine::ordered::OrderedPreset;
    use crate::engine::palette::{Metric, Palette};

    fn customised() -> Settings {
        let mut s = Settings {
            method: MethodKind::Noise,
            palette: Palette::new(vec![[1, 2, 3], [250, 128, 7], [0, 255, 99]]),
            ..Settings::default()
        };
        s.preprocess.pixel_size = 4;
        s.preprocess.grayscale = true;
        s.preprocess.invert = true;
        s.preprocess.brightness = -0.25;
        s.preprocess.contrast = 0.4;
        s.preprocess.gamma = 1.7;
        s.diffusion.kernel = BuiltinKernel::Atkinson.kernel();
        s.diffusion.kernel.weights[3] = 2.5;
        s.diffusion.serpentine = false;
        s.diffusion.strength = 0.8;
        s.ordered.matrix = OrderedPreset::Dot8.matrix();
        s.ordered.spread = 1.25;
        s.noise.kind = NoiseKind::White;
        s.noise.spread = 0.5;
        s.noise.seed = 4_000_000_007;
        s.color.linear_light = true;
        s.color.metric = Metric::Oklab;
        s
    }

    #[test]
    fn default_settings_round_trip() {
        let s = Settings::default();
        assert_eq!(from_json(&to_json(&s).unwrap()).unwrap(), s);
    }

    #[test]
    fn customised_settings_round_trip_exactly() {
        let s = customised();
        assert_eq!(from_json(&to_json(&s).unwrap()).unwrap(), s);
    }

    #[test]
    fn every_method_round_trips() {
        for method in MethodKind::ALL {
            let s = Settings {
                method,
                ..customised()
            };
            assert_eq!(from_json(&to_json(&s).unwrap()).unwrap().method, method);
        }
    }

    #[test]
    fn files_that_are_not_presets_are_rejected() {
        let kernel = serde_json::to_string(&BuiltinKernel::Stucki.kernel()).unwrap();
        assert!(from_json(&kernel).is_err());
        assert!(from_json("not json").is_err());
        let wrong = r#"{"format":"something-else","version":1,"settings":{}}"#;
        assert_eq!(
            from_json(wrong).unwrap_err(),
            "this is not a Rakeinen preset"
        );
    }

    #[test]
    fn presets_from_a_newer_version_are_rejected() {
        let text = r#"{"format":"rakeinen-preset","version":99,"settings":{}}"#;
        assert!(from_json(text).unwrap_err().contains("newer version"));
    }

    #[test]
    fn missing_settings_keep_their_defaults() {
        let text = r#"{"format":"rakeinen-preset","version":1,"settings":{"method":"Ordered"}}"#;
        let s = from_json(text).unwrap();
        assert_eq!(s.method, MethodKind::Ordered);
        assert_eq!(
            Settings {
                method: MethodKind::Diffusion,
                ..s
            },
            Settings::default()
        );
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let text =
            r#"{"format":"rakeinen-preset","version":1,"extra":5,"settings":{"future":true}}"#;
        assert_eq!(from_json(text).unwrap(), Settings::default());
    }

    #[test]
    fn out_of_range_numbers_are_clamped() {
        let mut s = Settings::default();
        s.preprocess.pixel_size = 500;
        s.preprocess.brightness = 9.0;
        s.preprocess.contrast = -9.0;
        s.preprocess.gamma = 0.0;
        s.diffusion.strength = 40.0;
        s.ordered.spread = -1.0;
        s.noise.spread = 7.0;
        let loaded = from_json(&to_json(&s).unwrap()).unwrap();
        assert_eq!(loaded.preprocess.pixel_size, 32);
        assert_eq!(loaded.preprocess.brightness, 1.0);
        assert_eq!(loaded.preprocess.contrast, -1.0);
        assert_eq!(loaded.preprocess.gamma, 0.2);
        assert_eq!(loaded.diffusion.strength, 1.5);
        assert_eq!(loaded.ordered.spread, 0.0);
        assert_eq!(loaded.noise.spread, 2.0);
    }

    #[test]
    fn invalid_contents_are_rejected() {
        let mut s = Settings::default();
        s.palette.colors.clear();
        assert!(
            from_json(&to_json(&s).unwrap())
                .unwrap_err()
                .contains("palette")
        );

        let mut s = Settings::default();
        s.diffusion.kernel.divisor = 0.0;
        assert!(
            from_json(&to_json(&s).unwrap())
                .unwrap_err()
                .contains("kernel")
        );

        let mut s = Settings::default();
        s.ordered.matrix.values.pop();
        assert!(
            from_json(&to_json(&s).unwrap())
                .unwrap_err()
                .contains("matrix")
        );
    }
}
