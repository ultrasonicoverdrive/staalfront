//! `assets/config/audio.ron`: every tunable of the audio (WP11.3).
//!
//! - `ra_audio`: "Use original Red Alert audio" (WP11.7).
//! - `spatial`: the distance, zoom and pan model of [`crate::spatial`].
//! - `mix`: volumes.
//! - `positional`: which in-game sounds play, how many, how often
//!   ([`crate::positional`]).
//!
//! The shipped file is also compiled in ([`AudioConfig::embedded`]) and is
//! the fallback when the file on disk is missing or broken, so there are no
//! tuning numbers in Rust. [`AudioConfig::validate`] refuses nonsense; the
//! loader then keeps the shipped numbers and returns a warning.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::ra::{CONFIG_FILE, RaAudioConfig};
use crate::spatial::SpatialParams;

/// The shipped `assets/config/audio.ron`, compiled in.
pub const EMBEDDED_CONFIG: &str = include_str!("../../../assets/config/audio.ron");

/// `assets/config/audio.ron`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioConfig {
    #[serde(default)]
    pub ra_audio: RaAudioConfig,
    pub spatial: SpatialConfig,
    pub mix: MixConfig,
    pub positional: PositionalConfig,
}

/// The numbers of [`SpatialParams`], in metres.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpatialConfig {
    /// Full volume within this horizontal distance from the focus.
    pub ref_radius_m: f32,
    /// Silent beyond this horizontal distance from the focus.
    pub max_radius_m: f32,
    /// Camera distance up to which no zoom attenuation applies.
    pub near_distance_m: f32,
    /// Lowest zoom gain (reached when zoomed far out).
    pub min_zoom_gain: f32,
    /// Half-width of the stereo image at zero camera distance.
    pub pan_width_m: f32,
    /// Growth of that half-width per metre of camera distance.
    pub pan_growth: f32,
    /// Largest `|pan|`.
    pub max_pan: f32,
    /// Gain below which a sound is not worth a voice.
    pub audible_gain: f32,
}

impl SpatialConfig {
    pub fn params(&self) -> SpatialParams {
        SpatialParams {
            ref_radius: self.ref_radius_m,
            max_radius: self.max_radius_m,
            near_distance: self.near_distance_m,
            min_zoom_gain: self.min_zoom_gain,
            pan_width: self.pan_width_m,
            pan_growth: self.pan_growth,
            max_pan: self.max_pan,
            audible_gain: self.audible_gain,
        }
    }
}

/// Volumes (linear, `1.0` = the bank's own).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MixConfig {
    pub sfx_volume: f32,
}

/// How one sound event is thinned out (see [`crate::positional`]).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventRule {
    /// Higher goes first when the frame's budget runs out (then the louder).
    pub priority: u8,
    /// Playbacks of this event in one frame, at most.
    pub max_per_batch: u32,
    /// Ticks between two playbacks of this event when it is at full volume.
    pub min_spacing_ticks: u32,
    /// The same when the sound is at the edge of being audible; in between
    /// the spacing grows as the gain falls ("farther away = less often").
    pub far_spacing_ticks: u32,
    /// Events of one tick with this sound within this many metres of a
    /// louder one merge into it.
    pub merge_radius_m: f32,
}

/// Which in-game sounds play and how many.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PositionalConfig {
    /// Playbacks started per frame over all events, at most.
    pub max_cues_per_frame: u32,
    /// The rule of an event that no entry of `events` covers.
    pub fallback: EventRule,
    /// Rules by event name. A rule for `weapon.fire` covers `weapon.fire.bullet`
    /// too; the longest matching name wins.
    pub events: BTreeMap<String, EventRule>,
}

impl PositionalConfig {
    /// The rule of `event`: its own entry, else the nearest enclosing dotted
    /// prefix, else the fallback.
    pub fn rule(&self, event: &str) -> &EventRule {
        let mut name = event;
        loop {
            if let Some(r) = self.events.get(name) {
                return r;
            }
            match name.rfind('.') {
                Some(i) => name = &name[..i],
                None => return &self.fallback,
            }
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        AudioConfig::embedded()
    }
}

impl AudioConfig {
    /// The shipped file. Pinned by tests, so it cannot be broken unnoticed.
    pub fn embedded() -> AudioConfig {
        let c: AudioConfig = ron::from_str(EMBEDDED_CONFIG)
            .unwrap_or_else(|e| panic!("the shipped audio.ron parses: {e}"));
        if let Err(e) = c.validate() {
            panic!("the shipped audio.ron is valid: {e}");
        }
        c
    }

    /// Read `<asset_root>/config/audio.ron`. A missing file gives the
    /// shipped numbers; a file that does not parse or fails
    /// [`validate`](Self::validate) gives them with a warning.
    pub fn load(asset_root: &Path) -> (AudioConfig, Option<String>) {
        let path = asset_root.join(CONFIG_FILE);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return (AudioConfig::embedded(), None);
        };
        match Self::parse(&text) {
            Ok(c) => (c, None),
            Err(e) => (
                AudioConfig::embedded(),
                Some(format!(
                    "cannot use {}: {e}; using the shipped defaults",
                    path.display()
                )),
            ),
        }
    }

    /// Parse and validate RON text.
    pub fn parse(text: &str) -> Result<AudioConfig, String> {
        let c: AudioConfig = ron::from_str(text).map_err(|e| e.to_string())?;
        c.validate()?;
        Ok(c)
    }

    /// Range checks. The first problem is returned.
    pub fn validate(&self) -> Result<(), String> {
        let s = &self.spatial;
        for (n, v) in [
            ("ref_radius_m", s.ref_radius_m),
            ("max_radius_m", s.max_radius_m),
            ("near_distance_m", s.near_distance_m),
            ("min_zoom_gain", s.min_zoom_gain),
            ("pan_width_m", s.pan_width_m),
            ("pan_growth", s.pan_growth),
            ("max_pan", s.max_pan),
            ("audible_gain", s.audible_gain),
        ] {
            if !v.is_finite() {
                return Err(format!("spatial.{n} is not a finite number"));
            }
        }
        if s.ref_radius_m < 0.0 {
            return Err("spatial.ref_radius_m must not be negative".into());
        }
        if s.max_radius_m <= s.ref_radius_m {
            return Err("spatial.max_radius_m must be larger than ref_radius_m".into());
        }
        if s.near_distance_m <= 0.0 {
            return Err("spatial.near_distance_m must be positive".into());
        }
        if !(s.min_zoom_gain > 0.0 && s.min_zoom_gain <= 1.0) {
            return Err("spatial.min_zoom_gain must be in (0, 1]".into());
        }
        if s.pan_width_m <= 0.0 || s.pan_growth < 0.0 {
            return Err("spatial.pan_width_m must be positive and pan_growth not negative".into());
        }
        if !(0.0..=1.0).contains(&s.max_pan) {
            return Err("spatial.max_pan must be in [0, 1]".into());
        }
        if !(s.audible_gain > 0.0 && s.audible_gain < 1.0) {
            return Err("spatial.audible_gain must be in (0, 1)".into());
        }
        let v = self.mix.sfx_volume;
        if !(v.is_finite() && (0.0..=2.0).contains(&v)) {
            return Err("mix.sfx_volume must be in [0, 2]".into());
        }
        let p = &self.positional;
        if p.max_cues_per_frame == 0 {
            return Err("positional.max_cues_per_frame must be at least 1".into());
        }
        check_rule("positional.fallback", &p.fallback)?;
        for (name, rule) in &p.events {
            let ok = !name.is_empty()
                && name
                    .split('.')
                    .all(|part| !part.is_empty() && part.bytes().all(is_name_byte));
            if !ok {
                return Err(format!(
                    "positional.events: \"{name}\" is not a dotted lower-case event name"
                ));
            }
            check_rule(&format!("positional.events.\"{name}\""), rule)?;
        }
        Ok(())
    }
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'
}

fn check_rule(what: &str, r: &EventRule) -> Result<(), String> {
    if r.max_per_batch == 0 {
        return Err(format!("{what}: max_per_batch must be at least 1"));
    }
    if r.far_spacing_ticks < r.min_spacing_ticks {
        return Err(format!(
            "{what}: far_spacing_ticks must not be below min_spacing_ticks"
        ));
    }
    if !(r.merge_radius_m.is_finite() && r.merge_radius_m >= 0.0) {
        return Err(format!("{what}: merge_radius_m must be 0 or more"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(p: u8, max: u32, min_sp: u32, far_sp: u32, merge: f32) -> EventRule {
        EventRule {
            priority: p,
            max_per_batch: max,
            min_spacing_ticks: min_sp,
            far_spacing_ticks: far_sp,
            merge_radius_m: merge,
        }
    }

    /// The shipped file against literals: a changed number fails here.
    #[test]
    fn the_shipped_audio_ron_is_these_numbers() {
        let c = AudioConfig::embedded();
        assert!(!c.ra_audio.enabled && c.ra_audio.path.is_empty());
        assert_eq!(
            c.spatial,
            SpatialConfig {
                ref_radius_m: 25.0,
                max_radius_m: 125.0,
                near_distance_m: 40.0,
                min_zoom_gain: 0.2,
                pan_width_m: 10.0,
                pan_growth: 1.0,
                max_pan: 0.9,
                audible_gain: 0.003,
            }
        );
        assert_eq!(c.mix, MixConfig { sfx_volume: 1.0 });
        let p = &c.positional;
        assert_eq!(p.max_cues_per_frame, 16);
        assert_eq!(p.fallback, rule(1, 3, 1, 6, 6.0));
        let want: Vec<(&str, EventRule)> = vec![
            ("explosion", rule(4, 3, 1, 6, 10.0)),
            ("explosion.large", rule(5, 2, 2, 8, 14.0)),
            ("impact", rule(0, 3, 1, 8, 6.0)),
            ("impact.cannon", rule(1, 3, 1, 8, 6.0)),
            ("impact.demolition", rule(2, 2, 2, 10, 8.0)),
            ("impact.miss", rule(0, 2, 2, 10, 6.0)),
            ("impact.rocket", rule(1, 3, 1, 8, 6.0)),
            ("impact.shell", rule(2, 2, 2, 10, 8.0)),
            ("unit.destroyed", rule(4, 3, 1, 6, 8.0)),
            ("weapon.fire", rule(1, 3, 1, 8, 8.0)),
            ("weapon.fire.cannon", rule(2, 3, 1, 8, 8.0)),
            ("weapon.fire.demolition", rule(3, 2, 2, 10, 10.0)),
            ("weapon.fire.howitzer", rule(3, 2, 2, 10, 10.0)),
            ("weapon.fire.precision", rule(2, 2, 2, 10, 8.0)),
            ("weapon.fire.rocket", rule(2, 3, 1, 8, 8.0)),
            ("weapon.fire.shell", rule(3, 2, 2, 10, 10.0)),
        ];
        let got: Vec<(&str, EventRule)> = p.events.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn the_params_carry_every_field_across() {
        let p = AudioConfig::embedded().spatial.params();
        assert_eq!(p.ref_radius, 25.0);
        assert_eq!(p.max_radius, 125.0);
        assert_eq!(p.near_distance, 40.0);
        assert_eq!(p.min_zoom_gain, 0.2);
        assert_eq!(p.pan_width, 10.0);
        assert_eq!(p.pan_growth, 1.0);
        assert_eq!(p.max_pan, 0.9);
        assert_eq!(p.audible_gain, 0.003);
        assert_eq!(p, SpatialParams::default());
    }

    #[test]
    fn a_rule_is_found_by_name_then_by_prefix_then_the_fallback() {
        let p = AudioConfig::embedded().positional;
        assert_eq!(p.rule("weapon.fire.rocket"), &rule(2, 3, 1, 8, 8.0));
        // no entry of its own: the one for "weapon.fire"
        assert_eq!(p.rule("weapon.fire.bullet"), &rule(1, 3, 1, 8, 8.0));
        assert_eq!(p.rule("explosion.small"), &rule(4, 3, 1, 6, 10.0));
        assert_eq!(p.rule("explosion.large"), &rule(5, 2, 2, 8, 14.0));
        // nothing covers it
        assert_eq!(p.rule("ui.click"), &p.fallback);
        assert_eq!(p.rule("weapon"), &p.fallback);
        // a prefix must end at a dot: "impactor" is not under "impact"
        assert_eq!(p.rule("impactor.x"), &p.fallback);
    }

    fn with(edit: &dyn Fn(&mut AudioConfig)) -> Result<(), String> {
        let mut c = AudioConfig::embedded();
        edit(&mut c);
        c.validate()
    }

    #[test]
    fn nonsense_is_refused() {
        type Edit = Box<dyn Fn(&mut AudioConfig)>;
        let bad: Vec<(&str, Edit)> = vec![
            ("ref", Box::new(|c| c.spatial.ref_radius_m = -1.0)),
            ("max", Box::new(|c| c.spatial.max_radius_m = 25.0)),
            ("near", Box::new(|c| c.spatial.near_distance_m = 0.0)),
            ("floor0", Box::new(|c| c.spatial.min_zoom_gain = 0.0)),
            ("floor2", Box::new(|c| c.spatial.min_zoom_gain = 2.0)),
            ("panw", Box::new(|c| c.spatial.pan_width_m = 0.0)),
            ("maxpan", Box::new(|c| c.spatial.max_pan = 1.5)),
            ("audible", Box::new(|c| c.spatial.audible_gain = 0.0)),
            ("nan", Box::new(|c| c.spatial.pan_growth = f32::NAN)),
            ("vol", Box::new(|c| c.mix.sfx_volume = -0.1)),
            ("cap", Box::new(|c| c.positional.max_cues_per_frame = 0)),
            (
                "max0",
                Box::new(|c| c.positional.fallback.max_per_batch = 0),
            ),
            (
                "far<min",
                Box::new(|c| c.positional.fallback.far_spacing_ticks = 0),
            ),
            (
                "merge",
                Box::new(|c| c.positional.fallback.merge_radius_m = -1.0),
            ),
            (
                "name",
                Box::new(|c| {
                    let r = c.positional.fallback;
                    c.positional.events.insert("Weapon..Fire".into(), r);
                }),
            ),
        ];
        for (what, edit) in &bad {
            assert!(with(edit).is_err(), "{what} should be refused");
        }
        assert!(with(&|_| {}).is_ok());
    }

    #[test]
    fn a_broken_file_gives_the_shipped_numbers_and_a_warning() {
        let dir = std::env::temp_dir().join(format!("sf_audio_cfg_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(dir.join("config/audio.ron"), "(spatial: (").unwrap();
        let (c, w) = AudioConfig::load(&dir);
        assert_eq!(c, AudioConfig::embedded());
        assert!(w.unwrap().contains("shipped defaults"));
        // unknown fields are refused too
        let text = EMBEDDED_CONFIG.replace("sfx_volume: 1.0", "sfx_volume: 1.0, loudness: 3");
        assert!(AudioConfig::parse(&text).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_file_on_disk_is_the_embedded_one() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let (c, w) = AudioConfig::load(&root);
        assert!(w.is_none());
        assert_eq!(c, AudioConfig::embedded());
    }
}
