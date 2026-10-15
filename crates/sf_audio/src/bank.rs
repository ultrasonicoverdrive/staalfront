//! The sound bank: named sounds from `assets/audio/bank.ron`.
//!
//! ```ron
//! (
//!     sounds: {
//!         "test_click": (
//!             file: "audio/sfx/test_click.wav", // relative to the asset root
//!             volume: 0.8,                      // linear, default 1.0
//!             pitch_variation: 0.05,            // +-5 % playback rate, default 0
//!             max_instances: 4,                 // simultaneous voices, default 8
//!             category: Sfx,                    // Sfx | Ui | Music, default Sfx
//!             looped: false,                    // default false
//!         ),
//!     },
//! )
//! ```
//!
//! Parsing is strict about the syntax (an error keeps the previous bank);
//! [`Bank::validate`] is lenient about values: it fixes or drops a bad entry
//! and reports a warning instead of failing.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Component, Path};

use serde::Deserialize;

/// Volume group, for later per-category volume settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
pub enum Category {
    Sfx,
    Ui,
    Music,
}

impl Category {
    pub const ALL: [Category; 3] = [Category::Sfx, Category::Ui, Category::Music];

    pub(crate) fn index(self) -> usize {
        match self {
            Category::Sfx => 0,
            Category::Ui => 1,
            Category::Music => 2,
        }
    }
}

/// Default for [`SoundDef::max_instances`].
pub const DEFAULT_MAX_INSTANCES: u32 = 8;
/// Largest accepted `pitch_variation` (+-50 %).
pub const MAX_PITCH_VARIATION: f32 = 0.5;

fn one() -> f32 {
    1.0
}

fn default_max_instances() -> u32 {
    DEFAULT_MAX_INSTANCES
}

fn default_category() -> Category {
    Category::Sfx
}

/// One named sound.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoundDef {
    /// Path relative to the asset root, forward slashes.
    pub file: String,
    /// Base volume, linear (`1.0` = file volume).
    #[serde(default = "one")]
    pub volume: f32,
    /// Random playback-rate spread as a fraction (`0.05` = +-5 %).
    #[serde(default)]
    pub pitch_variation: f32,
    /// How many instances may play at once; further plays are dropped.
    #[serde(default = "default_max_instances")]
    pub max_instances: u32,
    #[serde(default = "default_category")]
    pub category: Category,
    #[serde(default)]
    pub looped: bool,
}

/// All sounds, by name (sorted, so iteration and warnings are stable).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bank {
    #[serde(default)]
    pub sounds: BTreeMap<String, SoundDef>,
}

/// The bank file could not be read or parsed.
#[derive(Debug)]
pub enum BankError {
    Io(std::io::Error),
    Parse(String),
}

impl fmt::Display for BankError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BankError::Io(e) => write!(f, "cannot read sound bank: {e}"),
            BankError::Parse(e) => write!(f, "cannot parse sound bank: {e}"),
        }
    }
}

impl std::error::Error for BankError {}

impl Bank {
    /// Parse RON text (no validation of values).
    pub fn parse(text: &str) -> Result<Bank, BankError> {
        ron::from_str(text).map_err(|e| BankError::Parse(e.to_string()))
    }

    /// Read and parse a bank file.
    pub fn load(path: &Path) -> Result<Bank, BankError> {
        let text = std::fs::read_to_string(path).map_err(BankError::Io)?;
        Bank::parse(&text)
    }

    pub fn get(&self, name: &str) -> Option<&SoundDef> {
        self.sounds.get(name)
    }

    /// Check values and files; fix or drop bad entries and return one
    /// warning per problem. `file_exists` gets the `file` field as written.
    ///
    /// - path escaping the asset root (absolute, `..`): entry dropped
    /// - missing file: warning only, the entry stays (a hot-reload may add
    ///   the file later; playing it is skipped until then)
    /// - negative / non-finite volume, out-of-range pitch variation,
    ///   `max_instances` of 0: clamped to a sane value
    pub fn validate(&mut self, file_exists: impl Fn(&str) -> bool) -> Vec<String> {
        let mut warnings = Vec::new();
        let mut dropped = Vec::new();
        for (name, def) in &mut self.sounds {
            if !path_is_inside_root(&def.file) {
                warnings.push(format!(
                    "sound '{name}': file '{}' must be a relative path without '..'; sound removed",
                    def.file
                ));
                dropped.push(name.clone());
                continue;
            }
            if !file_exists(&def.file) {
                warnings.push(format!(
                    "sound '{name}': file '{}' not found; it will be skipped when played",
                    def.file
                ));
            }
            if !def.volume.is_finite() || def.volume < 0.0 {
                warnings.push(format!(
                    "sound '{name}': volume {} is invalid; using 1.0",
                    def.volume
                ));
                def.volume = 1.0;
            }
            if !def.pitch_variation.is_finite()
                || def.pitch_variation < 0.0
                || def.pitch_variation > MAX_PITCH_VARIATION
            {
                let fixed = if def.pitch_variation.is_finite() {
                    def.pitch_variation.clamp(0.0, MAX_PITCH_VARIATION)
                } else {
                    0.0
                };
                warnings.push(format!(
                    "sound '{name}': pitch_variation {} is outside 0..={MAX_PITCH_VARIATION}; using {fixed}",
                    def.pitch_variation
                ));
                def.pitch_variation = fixed;
            }
            if def.max_instances == 0 {
                warnings.push(format!(
                    "sound '{name}': max_instances 0 would never play; using 1"
                ));
                def.max_instances = 1;
            }
        }
        for name in dropped {
            self.sounds.remove(&name);
        }
        warnings
    }
}

fn path_is_inside_root(file: &str) -> bool {
    let p = Path::new(file);
    !file.is_empty()
        && !p.is_absolute()
        && !file.starts_with(['/', '\\'])
        && p.components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
        && !file.contains(':')
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"(
        sounds: {
            "click": (file: "audio/sfx/click.wav", volume: 0.8, pitch_variation: 0.05,
                      max_instances: 4, category: Ui),
            "theme": (file: "audio/music/theme.ogg", category: Music, looped: true),
        },
    )"#;

    #[test]
    fn parses_a_bank_with_defaults() {
        let bank = Bank::parse(GOOD).unwrap();
        assert_eq!(bank.sounds.len(), 2);
        let click = bank.get("click").unwrap();
        assert_eq!(click.file, "audio/sfx/click.wav");
        assert_eq!(click.volume, 0.8);
        assert_eq!(click.pitch_variation, 0.05);
        assert_eq!(click.max_instances, 4);
        assert_eq!(click.category, Category::Ui);
        assert!(!click.looped);
        let theme = bank.get("theme").unwrap();
        assert_eq!(theme.volume, 1.0);
        assert_eq!(theme.pitch_variation, 0.0);
        assert_eq!(theme.max_instances, DEFAULT_MAX_INSTANCES);
        assert_eq!(theme.category, Category::Music);
        assert!(theme.looped);
        assert!(bank.get("nope").is_none());
    }

    #[test]
    fn empty_bank_is_valid() {
        assert!(Bank::parse("()").unwrap().sounds.is_empty());
    }

    #[test]
    fn syntax_and_unknown_fields_are_errors_not_panics() {
        assert!(matches!(
            Bank::parse("(sounds: {"),
            Err(BankError::Parse(_))
        ));
        assert!(Bank::parse(r#"(sounds: {"a": (file: "x.wav", colume: 1.0)})"#).is_err());
        assert!(Bank::parse(r#"(sounds: {"a": (volume: 1.0)})"#).is_err()); // file missing
        assert!(Bank::parse(r#"(sounds: {"a": (file: "x.wav", category: Bass)})"#).is_err());
        assert!(matches!(
            Bank::load(Path::new("/definitely/not/here/bank.ron")),
            Err(BankError::Io(_))
        ));
    }

    #[test]
    fn unknown_file_is_a_warning_and_the_entry_stays() {
        let mut bank = Bank::parse(GOOD).unwrap();
        let w = bank.validate(|f| f == "audio/sfx/click.wav");
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("theme") && w[0].contains("not found"));
        assert_eq!(bank.sounds.len(), 2);
    }

    #[test]
    fn bad_values_are_clamped_with_a_warning() {
        let mut bank = Bank::parse(
            r#"(sounds: {"a": (file: "a.wav", volume: -1.0, pitch_variation: 2.0, max_instances: 0)})"#,
        )
        .unwrap();
        let w = bank.validate(|_| true);
        assert_eq!(w.len(), 3, "{w:?}");
        let a = bank.get("a").unwrap();
        assert_eq!(a.volume, 1.0);
        assert_eq!(a.pitch_variation, MAX_PITCH_VARIATION);
        assert_eq!(a.max_instances, 1);
    }

    #[test]
    fn non_finite_values_are_fixed() {
        let mut bank = Bank {
            sounds: BTreeMap::from([(
                "a".to_string(),
                SoundDef {
                    file: "a.wav".into(),
                    volume: f32::NAN,
                    pitch_variation: f32::NAN,
                    max_instances: 1,
                    category: Category::Sfx,
                    looped: false,
                },
            )]),
        };
        assert_eq!(bank.validate(|_| true).len(), 2);
        let a = bank.get("a").unwrap();
        assert!(a.volume.is_finite() && a.pitch_variation == 0.0);
    }

    #[test]
    fn paths_outside_the_asset_root_are_dropped() {
        for bad in [
            "../secret.wav",
            "/etc/passwd",
            "a/../../b.wav",
            "C:/x.wav",
            "\\\\server\\x.wav",
            "",
        ] {
            let mut bank = Bank {
                sounds: BTreeMap::from([(
                    "a".to_string(),
                    SoundDef {
                        file: bad.to_string(),
                        volume: 1.0,
                        pitch_variation: 0.0,
                        max_instances: 1,
                        category: Category::Sfx,
                        looped: false,
                    },
                )]),
            };
            let w = bank.validate(|_| true);
            assert_eq!(w.len(), 1, "{bad}: {w:?}");
            assert!(bank.sounds.is_empty(), "{bad}");
        }
    }

    #[test]
    fn the_shipped_bank_is_valid_and_its_files_exist() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let mut bank = Bank::load(&assets.join("audio/bank.ron")).unwrap();
        let w = bank.validate(|f| assets.join(f).is_file());
        assert!(w.is_empty(), "{w:?}");
        assert!(bank.get("test_click").is_some());
    }
}
