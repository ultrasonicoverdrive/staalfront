//! How damage shows (WP6.8), in `assets/config/damage.ron`.
//!
//! The simulation knows hit points and nothing about how they look. This table says at which share of
//! its hit points an object counts as damaged and as heavily damaged ([`Damage::stage`]), which model
//! file goes with a stage ([`Damage::model_of`]), and how much smoke and fire a stage gives off and
//! where ([`Emitters`], [`Fallback`], [`DamageLimits`]). Presentation only: no simulation number lives
//! here, and no checksum depends on it (game data in RON, PLAN.md section 5 rule 6).
//!
//! ```
//! use sf_data::damage::{Damage, Stage};
//!
//! let d = Damage::embedded();
//! assert_eq!(d.stage(1000, 1000), Stage::Intact);
//! assert_eq!(d.stage(100, 1000), Stage::Heavy);
//! assert_eq!(
//!     d.model_of("set_v4/meridian_barracks.glb", Stage::Damaged).as_deref(),
//!     Some("set_v4/meridian_barracks_damaged.glb")
//! );
//! ```

use serde::Deserialize;

use crate::effects::Effects;

/// The shipped file, compiled in, like the other tables.
pub const EMBEDDED_DAMAGE: &str = include_str!("../../../assets/config/damage.ron");

/// How damaged an object looks.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// Above the damaged threshold, or an object that cannot be damaged.
    Intact,
    /// At or below `damaged_at_permille`.
    Damaged,
    /// At or below `heavy_at_permille`.
    Heavy,
}

impl Stage {
    /// Both damaged stages, in order.
    pub const DAMAGED: [Stage; 2] = [Stage::Damaged, Stage::Heavy];
}

/// The thresholds, in thousandths of the hit points left.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thresholds {
    /// At or below this: damaged.
    pub damaged_at_permille: u16,
    /// At or below this: heavily damaged.
    pub heavy_at_permille: u16,
}

/// The file name parts of the stage models.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSuffixes {
    /// Before `.glb` for a damaged model.
    pub damaged_suffix: String,
    /// Before `.glb` for a heavily damaged model.
    pub heavy_suffix: String,
}

/// What a stage gives off, puffs per second and source.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rates {
    /// Smoke puffs per second.
    pub smoke_per_s: f32,
    /// Fire puffs per second.
    pub fire_per_s: f32,
}

/// The smoke and fire.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Emitters {
    /// The effects.ron entry of a smoke puff.
    pub smoke_effect: String,
    /// The effects.ron entry of a fire puff.
    pub fire_effect: String,
    /// The effects.ron entry of a smoke puff of a structure (larger than a vehicle's).
    pub structure_smoke_effect: String,
    /// The effects.ron entry of a fire puff of a structure.
    pub structure_fire_effect: String,
    /// The anchor-name prefix of the smoke node.
    pub smoke_anchor: String,
    /// The anchor-name prefix of the fire node.
    pub fire_anchor: String,
    /// A damaged object.
    pub damaged: Rates,
    /// A heavily damaged object.
    pub heavy: Rates,
    /// A damaged structure.
    pub structure_damaged: Rates,
    /// A heavily damaged structure.
    pub structure_heavy: Rates,
    /// The disc around an anchor the puffs start in, metres.
    pub anchor_radius_m: f32,
    /// Puffs start this far above the anchor, metres (a puff on a roof's surface is cut in half).
    pub anchor_lift_m: f32,
    /// A mobile object smaller than this many cells shows nothing.
    pub min_body_cells: f32,
}

/// Where the puffs start on a model without anchors.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fallback {
    /// Height as a fraction of the body length.
    pub height_fraction: f32,
    /// At least this high, metres.
    pub min_height_m: f32,
    /// Disc radius as a fraction of the body length.
    pub radius_fraction: f32,
    /// At least this wide, metres.
    pub min_radius_m: f32,
}

/// The budgets of the damage effects.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DamageLimits {
    /// Sources that may give off smoke in one frame.
    pub max_sources: u32,
    /// A source further outside the screen than this fraction of its width or height is skipped.
    pub cull_margin: f32,
}

/// The table.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Damage {
    /// The thresholds.
    pub stages: Thresholds,
    /// The stage models.
    pub models: ModelSuffixes,
    /// Smoke and fire.
    pub emitters: Emitters,
    /// The rule for a model without anchors.
    pub fallback: Fallback,
    /// The budgets.
    pub limits: DamageLimits,
}

fn in_range(what: &str, v: f32, lo: f32, hi: f32) -> Result<(), String> {
    if v.is_finite() && v >= lo && v <= hi {
        Ok(())
    } else {
        Err(format!("{what} is {v}, must be between {lo} and {hi}"))
    }
}

fn name_part(what: &str, s: &str) -> Result<(), String> {
    if s.is_empty() || s.contains(['/', '\\', '.']) {
        Err(format!(
            "{what} \"{s}\" must be a name part without / \\ or ."
        ))
    } else {
        Ok(())
    }
}

impl Damage {
    /// Reads and checks a table. The error says what is wrong.
    pub fn parse(text: &str) -> Result<Damage, String> {
        let table: Damage = ron::from_str(text).map_err(|e| e.to_string())?;
        table.check()?;
        Ok(table)
    }

    /// The shipped table. Panics if the shipped file is wrong (a test reads it).
    pub fn embedded() -> Damage {
        Damage::parse(EMBEDDED_DAMAGE).unwrap_or_else(|e| panic!("assets/config/damage.ron: {e}"))
    }

    fn check(&self) -> Result<(), String> {
        let s = &self.stages;
        if !(1..=999).contains(&s.heavy_at_permille) || s.damaged_at_permille > 999 {
            return Err(format!(
                "stages: {} and {} must be 1 to 999",
                s.heavy_at_permille, s.damaged_at_permille
            ));
        }
        if s.heavy_at_permille >= s.damaged_at_permille {
            return Err(format!(
                "stages.heavy_at_permille {} must be below damaged_at_permille {}",
                s.heavy_at_permille, s.damaged_at_permille
            ));
        }
        name_part("models.damaged_suffix", &self.models.damaged_suffix)?;
        name_part("models.heavy_suffix", &self.models.heavy_suffix)?;
        if self.models.damaged_suffix == self.models.heavy_suffix {
            return Err("the two model suffixes must differ".into());
        }
        let e = &self.emitters;
        for (what, v) in [
            ("smoke_anchor", &e.smoke_anchor),
            ("fire_anchor", &e.fire_anchor),
        ] {
            if v.is_empty() {
                return Err(format!("emitters.{what} is empty"));
            }
        }
        for (what, v) in [
            ("structure_smoke_effect", &e.structure_smoke_effect),
            ("structure_fire_effect", &e.structure_fire_effect),
        ] {
            if v.is_empty() {
                return Err(format!("emitters.{what} is empty"));
            }
        }
        for (what, r) in [
            ("damaged", &e.damaged),
            ("heavy", &e.heavy),
            ("structure_damaged", &e.structure_damaged),
            ("structure_heavy", &e.structure_heavy),
        ] {
            in_range(&format!("{what}.smoke_per_s"), r.smoke_per_s, 0.0, 60.0)?;
            in_range(&format!("{what}.fire_per_s"), r.fire_per_s, 0.0, 60.0)?;
        }
        if e.heavy.smoke_per_s <= 0.0 && e.heavy.fire_per_s <= 0.0 {
            return Err("emitters.heavy gives off nothing".into());
        }
        if e.damaged.smoke_per_s <= 0.0 {
            return Err(
                "emitters.damaged.smoke_per_s must be above 0 (damaged shows smoke)".into(),
            );
        }
        if e.structure_heavy.smoke_per_s <= 0.0 && e.structure_heavy.fire_per_s <= 0.0 {
            return Err("emitters.structure_heavy gives off nothing".into());
        }
        if e.structure_damaged.smoke_per_s <= 0.0 {
            return Err(
                "emitters.structure_damaged.smoke_per_s must be above 0 (damaged shows smoke)"
                    .into(),
            );
        }
        in_range("emitters.anchor_radius_m", e.anchor_radius_m, 0.0, 5.0)?;
        in_range("emitters.anchor_lift_m", e.anchor_lift_m, 0.0, 10.0)?;
        in_range("emitters.min_body_cells", e.min_body_cells, 0.0, 10.0)?;
        let f = &self.fallback;
        in_range("fallback.height_fraction", f.height_fraction, 0.0, 2.0)?;
        in_range("fallback.min_height_m", f.min_height_m, 0.0, 20.0)?;
        in_range("fallback.radius_fraction", f.radius_fraction, 0.0, 2.0)?;
        in_range("fallback.min_radius_m", f.min_radius_m, 0.0, 10.0)?;
        let l = &self.limits;
        if !(1..=4096).contains(&l.max_sources) {
            return Err(format!(
                "limits.max_sources {} must be 1 to 4096",
                l.max_sources
            ));
        }
        in_range("limits.cull_margin", l.cull_margin, 0.0, 4.0)?;
        Ok(())
    }

    /// The names the table uses from effects.ron exist there, the smoke as a smoke-class effect (not
    /// glowing) and the fire as glowing light.
    pub fn check_against(&self, effects: &Effects) -> Result<(), String> {
        let find = |what: &str, name: &str| {
            effects
                .effects
                .get(name)
                .ok_or_else(|| format!("emitters.{what} \"{name}\" is not an effect"))
        };
        for (smoke_what, smoke_name, fire_what, fire_name) in [
            (
                "smoke_effect",
                &self.emitters.smoke_effect,
                "fire_effect",
                &self.emitters.fire_effect,
            ),
            (
                "structure_smoke_effect",
                &self.emitters.structure_smoke_effect,
                "structure_fire_effect",
                &self.emitters.structure_fire_effect,
            ),
        ] {
            if find(smoke_what, smoke_name)?.glow {
                return Err(format!(
                    "emitters.{smoke_what} \"{smoke_name}\" must be a smoke-class effect (glow: false)"
                ));
            }
            if !find(fire_what, fire_name)?.glow {
                return Err(format!("emitters.{fire_what} \"{fire_name}\" must glow"));
            }
        }
        Ok(())
    }

    /// The stage of an object with `hp` hit points of at most `max_hp`. A maximum of 0 is an object that
    /// cannot be damaged: intact.
    pub fn stage(&self, hp: u32, max_hp: u32) -> Stage {
        if max_hp == 0 {
            return Stage::Intact;
        }
        let permille = u64::from(hp.min(max_hp)) * 1000 / u64::from(max_hp);
        if permille <= u64::from(self.stages.heavy_at_permille) {
            Stage::Heavy
        } else if permille <= u64::from(self.stages.damaged_at_permille) {
            Stage::Damaged
        } else {
            Stage::Intact
        }
    }

    /// What `stage` gives off a mobile object; nothing for an intact object.
    pub fn rates(&self, stage: Stage) -> Option<Rates> {
        self.rates_of(stage, false)
    }

    /// What `stage` gives off, puffs per second and source: a structure's own rates when `structure`,
    /// else a mobile object's. Nothing for an intact object.
    pub fn rates_of(&self, stage: Stage, structure: bool) -> Option<Rates> {
        let e = &self.emitters;
        match (stage, structure) {
            (Stage::Intact, _) => None,
            (Stage::Damaged, false) => Some(e.damaged),
            (Stage::Heavy, false) => Some(e.heavy),
            (Stage::Damaged, true) => Some(e.structure_damaged),
            (Stage::Heavy, true) => Some(e.structure_heavy),
        }
    }

    /// The effects.ron names `(smoke, fire)` of the puffs of a structure or of a mobile object.
    pub fn puff_effects(&self, structure: bool) -> (&str, &str) {
        let e = &self.emitters;
        if structure {
            (&e.structure_smoke_effect, &e.structure_fire_effect)
        } else {
            (&e.smoke_effect, &e.fire_effect)
        }
    }

    /// The model file of `stage` for a structure whose finished model is `finished` (`dir/name.glb`):
    /// the suffix goes before the extension. `None` for an intact stage or a file without `.glb`.
    pub fn model_of(&self, finished: &str, stage: Stage) -> Option<String> {
        let suffix = match stage {
            Stage::Intact => return None,
            Stage::Damaged => &self.models.damaged_suffix,
            Stage::Heavy => &self.models.heavy_suffix,
        };
        let base = finished.strip_suffix(".glb")?;
        Some(format!("{base}{suffix}.glb"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shipped() -> Damage {
        Damage::embedded()
    }

    /// Every number of the shipped file, as literals: a change of the file fails here.
    #[test]
    fn the_shipped_file_is_these_numbers() {
        let d = shipped();
        assert_eq!(
            (d.stages.damaged_at_permille, d.stages.heavy_at_permille),
            (660, 330)
        );
        assert_eq!(
            (
                d.models.damaged_suffix.as_str(),
                d.models.heavy_suffix.as_str()
            ),
            ("_damaged", "_heavily_damaged")
        );
        let e = &d.emitters;
        assert_eq!(
            (
                e.smoke_effect.as_str(),
                e.fire_effect.as_str(),
                e.smoke_anchor.as_str(),
                e.fire_anchor.as_str()
            ),
            ("damage_smoke", "damage_fire", "fx_smoke", "fx_fire")
        );
        assert_eq!((e.damaged.smoke_per_s, e.damaged.fire_per_s), (4.0, 0.0));
        assert_eq!((e.heavy.smoke_per_s, e.heavy.fire_per_s), (8.0, 7.0));
        // A structure burns bigger (WP6.8b placeholders); the mobile numbers above are unchanged.
        assert_eq!(
            (
                e.structure_smoke_effect.as_str(),
                e.structure_fire_effect.as_str()
            ),
            ("damage_smoke_structure", "damage_fire_structure")
        );
        assert_eq!(
            (
                e.structure_damaged.smoke_per_s,
                e.structure_damaged.fire_per_s
            ),
            (5.0, 0.0)
        );
        assert_eq!(
            (e.structure_heavy.smoke_per_s, e.structure_heavy.fire_per_s),
            (10.0, 10.0)
        );
        assert_eq!(
            (e.anchor_radius_m, e.anchor_lift_m, e.min_body_cells),
            (0.4, 1.0, 1.0)
        );
        let f = &d.fallback;
        assert_eq!(
            (
                f.height_fraction,
                f.min_height_m,
                f.radius_fraction,
                f.min_radius_m
            ),
            (0.6, 1.0, 0.15, 0.3)
        );
        assert_eq!((d.limits.max_sources, d.limits.cull_margin), (256, 0.3));
    }

    #[test]
    fn the_shipped_file_matches_effects_ron() {
        shipped()
            .check_against(&Effects::embedded())
            .expect("the shipped names exist");
    }

    #[test]
    fn the_stage_follows_the_share_of_hit_points_left() {
        let d = shipped();
        // Thresholds at 660 and 330 thousandths of 1000 hit points: at or below.
        assert_eq!(d.stage(1000, 1000), Stage::Intact);
        assert_eq!(d.stage(661, 1000), Stage::Intact);
        assert_eq!(d.stage(660, 1000), Stage::Damaged);
        assert_eq!(d.stage(331, 1000), Stage::Damaged);
        assert_eq!(d.stage(330, 1000), Stage::Heavy);
        assert_eq!(d.stage(1, 1000), Stage::Heavy);
        assert_eq!(d.stage(0, 1000), Stage::Heavy);
        // More than the maximum is no worse than full; a maximum of 0 cannot be damaged.
        assert_eq!(d.stage(2000, 1000), Stage::Intact);
        assert_eq!(d.stage(0, 0), Stage::Intact);
        // The share, not the number: 3 of 5 is 600 thousandths.
        assert_eq!(d.stage(3, 5), Stage::Damaged);
        assert_eq!(d.stage(4, 5), Stage::Intact);
        assert_eq!(d.stage(u32::MAX, u32::MAX), Stage::Intact);
    }

    #[test]
    fn a_stage_has_a_model_file_and_an_intact_one_has_none() {
        let d = shipped();
        assert_eq!(
            d.model_of("set_v4/veyran_refinery.glb", Stage::Heavy)
                .as_deref(),
            Some("set_v4/veyran_refinery_heavily_damaged.glb")
        );
        assert_eq!(
            d.model_of("set_v4/veyran_refinery.glb", Stage::Intact),
            None
        );
        assert_eq!(d.model_of("not_a_glb", Stage::Damaged), None);
    }

    #[test]
    fn rates_are_only_for_damaged_objects() {
        let d = shipped();
        assert_eq!(d.rates(Stage::Intact), None);
        assert_eq!(d.rates_of(Stage::Intact, true), None);
        assert_eq!(d.rates(Stage::Damaged).unwrap().fire_per_s, 0.0);
        assert!(d.rates(Stage::Heavy).unwrap().fire_per_s > 0.0);
    }

    /// A structure has its own, bigger effects and higher rates than a mobile object.
    #[test]
    fn a_structure_has_its_own_effects_and_rates() {
        let d = shipped();
        assert_eq!(d.puff_effects(false), ("damage_smoke", "damage_fire"));
        assert_eq!(
            d.puff_effects(true),
            ("damage_smoke_structure", "damage_fire_structure")
        );
        let (mobile, structure) = (
            d.rates_of(Stage::Heavy, false).unwrap(),
            d.rates_of(Stage::Heavy, true).unwrap(),
        );
        assert!(structure.fire_per_s > mobile.fire_per_s);
        assert!(structure.smoke_per_s > mobile.smoke_per_s);
        assert_eq!(d.rates(Stage::Heavy), Some(mobile));
        // The flame is about twice the size, the smoke column larger (effects.ron).
        let fx = Effects::embedded();
        let size = |n: &str| fx.effects[n].size_m;
        assert!(size("damage_fire_structure") >= 1.8 * size("damage_fire"));
        assert!(size("damage_smoke_structure") > size("damage_smoke"));
    }

    fn with(from: &str, to: &str) -> Result<Damage, String> {
        assert!(EMBEDDED_DAMAGE.contains(from), "{from} is in the file");
        Damage::parse(&EMBEDDED_DAMAGE.replace(from, to))
    }

    #[test]
    fn a_bad_table_is_refused_with_what_is_wrong() {
        let e = with("heavy_at_permille: 330", "heavy_at_permille: 700").unwrap_err();
        assert!(e.contains("below damaged_at_permille"), "{e}");
        let e = with("damaged_suffix: \"_damaged\"", "damaged_suffix: \"a/b\"").unwrap_err();
        assert!(e.contains("damaged_suffix"), "{e}");
        let e = with(
            "heavy_suffix: \"_heavily_damaged\"",
            "heavy_suffix: \"_damaged\"",
        )
        .unwrap_err();
        assert!(e.contains("differ"), "{e}");
        let e = with("max_sources: 256", "max_sources: 0").unwrap_err();
        assert!(e.contains("max_sources"), "{e}");
        let e = with("min_height_m: 1.0", "min_height_m: -1.0").unwrap_err();
        assert!(e.contains("min_height_m"), "{e}");
        let e = with("damaged: (smoke_per_s: 4.0", "damaged: (smoke_per_s: 0.0").unwrap_err();
        assert!(e.contains("damaged.smoke_per_s"), "{e}");
        let e = with(
            "structure_heavy: (smoke_per_s: 10.0, fire_per_s: 10.0)",
            "structure_heavy: (smoke_per_s: 0.0, fire_per_s: 0.0)",
        )
        .unwrap_err();
        assert!(e.contains("structure_heavy"), "{e}");
        let e = with(
            "structure_damaged: (smoke_per_s: 5.0",
            "structure_damaged: (smoke_per_s: 0.0",
        )
        .unwrap_err();
        assert!(e.contains("structure_damaged.smoke_per_s"), "{e}");
        let e = with("fire_per_s: 10.0)", "fire_per_s: 99.0)").unwrap_err();
        assert!(e.contains("structure_heavy.fire_per_s"), "{e}");
        let e = with("limits: (", "limits: (surprise: 1, ").unwrap_err();
        assert!(e.contains("surprise"), "{e}");
    }

    #[test]
    fn names_that_effects_ron_lacks_or_with_the_wrong_class_are_refused() {
        let effects = Effects::embedded();
        let e = with("\"damage_smoke\"", "\"nope\"")
            .unwrap()
            .check_against(&effects)
            .unwrap_err();
        assert!(e.contains("smoke_effect"), "{e}");
        let e = with("\"damage_fire_structure\"", "\"nope\"")
            .unwrap()
            .check_against(&effects)
            .unwrap_err();
        assert!(e.contains("structure_fire_effect"), "{e}");
        let e = with(
            "structure_fire_effect: \"damage_fire_structure\"",
            "structure_fire_effect: \"smoke_wreck\"",
        )
        .unwrap()
        .check_against(&effects)
        .unwrap_err();
        assert!(
            e.contains("structure_fire_effect") && e.contains("must glow"),
            "{e}"
        );
        let e = with(
            "structure_smoke_effect: \"damage_smoke_structure\"",
            "structure_smoke_effect: \"flash_bullet\"",
        )
        .unwrap()
        .check_against(&effects)
        .unwrap_err();
        assert!(
            e.contains("structure_smoke_effect") && e.contains("smoke-class"),
            "{e}"
        );
        // The fire must glow and the smoke must not.
        let e = with(
            "fire_effect: \"damage_fire\"",
            "fire_effect: \"smoke_wreck\"",
        )
        .unwrap()
        .check_against(&effects)
        .unwrap_err();
        assert!(e.contains("must glow"), "{e}");
        let e = with(
            "smoke_effect: \"damage_smoke\"",
            "smoke_effect: \"flash_bullet\"",
        )
        .unwrap()
        .check_against(&effects)
        .unwrap_err();
        assert!(e.contains("smoke-class"), "{e}");
    }
}
