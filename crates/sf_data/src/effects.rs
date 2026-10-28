//! Visual effects (`assets/config/effects.ron`, WP5.6a): which particles a shot, a hit, a death and a
//! projectile in flight make.
//!
//! Presentation data: nothing here reaches the simulation, so numbers are plain `f32`s (the `Decimal` of the
//! unit definitions exists for the sim's fixed point). The file is checked when it is parsed
//! ([`Effects::from_ron`]) and once more against the unit definitions ([`Effects::check_against`]): every
//! effect named exists, every damage type has an entry, every `by_weapon` key is a weapon id, unknown fields are
//! refused, and the numbers are in the ranges below. `docs/rules/effects.md` says what each field does.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;

use crate::units::UnitDefs;

/// The shipped file, compiled in.
pub const EMBEDDED_EFFECTS: &str = include_str!("../../../assets/config/effects.ron");

/// Most particles one burst may make.
pub const MAX_COUNT: u32 = 64;
/// Most instances (pool particles and tracer dots together) one frame may draw: the size of the GPU buffer
/// of `sf_render::Renderer` (a test there keeps the two equal).
pub const MAX_INSTANCES: u32 = 8192;

/// Why an effects file was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectsError(pub String);

impl fmt::Display for EffectsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "effects.ron: {}", self.0)
    }
}

impl std::error::Error for EffectsError {}

fn err<T>(message: impl Into<String>) -> Result<T, EffectsError> {
    Err(EffectsError(message.into()))
}

/// The budgets and the zoom rules.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Size of the particle pool.
    pub max_particles: u32,
    /// Most tracer dots drawn at once (the buffer holds both).
    pub max_tracer_dots: u32,
    /// Smallest size on the screen, physical pixels.
    pub min_px: f32,
    /// Largest size on the screen, physical pixels.
    pub max_px: f32,
    /// Below this apparent size of the reference unit the zoom counts as middle.
    pub middle_below_px: f32,
    /// The share of a burst made at middle zoom.
    pub middle_factor: f32,
    /// Smoke is refused above this share (percent) of the pool.
    pub smoke_refuse_percent: u32,
    /// The alpha a death flash keeps at strategic zoom.
    pub strategic_flash_alpha: f32,
    /// Events older than this many ticks are dropped when the picture takes them.
    pub max_event_age_ticks: u32,
    /// A particle rises at `rise_mps` times a factor spread evenly over `1 ± rise_jitter / 2`.
    pub rise_jitter: f32,
    /// Smoke's alpha comes in over the first `1 / smoke_fade_in` of its life.
    pub smoke_fade_in: f32,
    /// A death flash holds its alpha over this share of its life, then fades out.
    pub flash_hold: f32,
    /// The last tracer dot is `1 - tracer_taper` of the head's size and alpha.
    pub tracer_taper: f32,
}

/// Where a burst starts and how wide it is, without the model.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    /// Height above the ground of a hit, a death and wreck smoke, metres.
    pub hit_height_m: f32,
    /// A death bursts this much higher than a hit, metres (the blast of a body starts above its wreck).
    pub death_lift_m: f32,
    /// Height above the ground a projectile that flies on the ground is drawn at, metres.
    pub projectile_lift_m: f32,
    /// Hit burst radius as a fraction of the size the event carries.
    pub impact_radius_fraction: f32,
    /// Death burst radius as a fraction of the body length.
    pub death_radius_fraction: f32,
    /// Least burst radius of a hit or a death, metres.
    pub min_burst_radius_m: f32,
}

/// Where a flash starts without a muzzle anchor in the model.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Muzzle {
    /// Fraction of the body length out along the mount's facing.
    pub reach_fraction: f32,
    /// Fraction of the body length above the ground.
    pub height_fraction: f32,
    /// Least reach in metres.
    pub min_reach_m: f32,
}

/// One kind of particle burst.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectDef {
    /// Particles in the burst.
    pub count: u32,
    /// How long a particle lives, seconds.
    pub life_s: f32,
    /// Size at birth, metres across.
    pub size_m: f32,
    /// Growth of the size, metres per second.
    pub growth_mps: f32,
    /// sRGB colour.
    pub colour: (u8, u8, u8),
    /// Peak alpha.
    pub alpha: f32,
    /// Additive light (true) or smoke class (false).
    pub glow: bool,
    /// Upward speed, metres per second.
    pub rise_mps: f32,
    /// Sideways speed at birth, up to this, metres per second.
    pub spread_mps: f32,
    /// The audio event name; empty = silent.
    pub sound: String,
}

/// The effects of a damage type.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DamageEffects {
    /// At the muzzle.
    pub fire: Vec<String>,
    /// Where it lands.
    pub impact: Vec<String>,
}

/// A weapon that differs from its damage type; an empty list keeps the damage type's.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponEffects {
    /// At the muzzle.
    #[serde(default)]
    pub fire: Vec<String>,
    /// Where it lands.
    #[serde(default)]
    pub impact: Vec<String>,
}

/// What a destroyed unit or structure makes.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeathEffects {
    /// A mobile body smaller than this (cells) is infantry.
    pub infantry_below_cells: f32,
    /// A mobile body smaller than this (cells) is a small vehicle.
    pub small_below_cells: f32,
    /// Infantry.
    pub infantry: Vec<String>,
    /// Small vehicles.
    pub small: Vec<String>,
    /// Vehicles.
    pub vehicle: Vec<String>,
    /// Structures.
    pub structure: Vec<String>,
}

/// What a wreck gives off.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WreckSmoke {
    /// The puff.
    pub effect: String,
    /// Puffs per second.
    pub per_second: f32,
    /// The last share of the wreck's life over which it stops.
    pub fade_fraction: f32,
    /// The puffs start in a disc of this fraction of the wreck's length.
    pub radius_fraction: f32,
}

/// How one kind of projectile is drawn.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tracer {
    /// Glowing dots behind the head.
    pub dots: u32,
    /// Distance between dots, metres.
    pub spacing_m: f32,
    /// Size of the head, metres across (the dots taper).
    pub size_m: f32,
    /// sRGB colour.
    pub colour: (u8, u8, u8),
    /// Peak alpha.
    pub alpha: f32,
    /// Puffs left behind.
    pub trail: Vec<String>,
    /// Puffs per second.
    pub trail_per_s: f32,
}

/// The four tracers.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tracers {
    /// A straight flight.
    pub straight: Tracer,
    /// An arc.
    pub arc: Tracer,
    /// A guided flight.
    pub guided: Tracer,
    /// A falling shell.
    pub falling: Tracer,
}

/// The file as written.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Effects {
    /// Budgets and zoom rules.
    pub limits: Limits,
    /// The muzzle fallback.
    pub muzzle: Muzzle,
    /// Where hits, deaths and projectiles are drawn.
    pub placement: Placement,
    /// The named bursts.
    pub effects: BTreeMap<String, EffectDef>,
    /// By damage type name.
    pub by_damage_type: BTreeMap<String, DamageEffects>,
    /// By weapon id.
    #[serde(default)]
    pub by_weapon: BTreeMap<String, WeaponEffects>,
    /// Where nothing was hit.
    pub miss: Vec<String>,
    /// Deaths.
    pub death: DeathEffects,
    /// Wrecks.
    pub wreck_smoke: WreckSmoke,
    /// Projectiles in flight.
    pub tracers: Tracers,
}

/// Whether `name` is a dotted event name: two or more parts of `[a-z0-9_]`, separated by dots.
pub fn is_event_name(name: &str) -> bool {
    let mut parts = 0;
    for part in name.split('.') {
        parts += 1;
        if part.is_empty()
            || !part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return false;
        }
    }
    parts >= 2
}

fn in_range(what: &str, v: f32, lo: f32, hi: f32) -> Result<(), EffectsError> {
    if v.is_finite() && v >= lo && v <= hi {
        Ok(())
    } else {
        err(format!("{what} is {v}, must be between {lo} and {hi}"))
    }
}

impl Effects {
    /// The shipped file.
    ///
    /// # Panics
    ///
    /// If the compiled-in file is invalid, which a test rules out.
    pub fn embedded() -> Effects {
        Effects::from_ron(EMBEDDED_EFFECTS)
            .unwrap_or_else(|e| panic!("the embedded effects.ron is valid: {e}"))
    }

    /// Parses and checks the text of an `effects.ron` (everything that does not need the unit definitions).
    pub fn from_ron(text: &str) -> Result<Effects, EffectsError> {
        let effects: Effects = ron::from_str(text).map_err(|e| EffectsError(e.to_string()))?;
        effects.check()?;
        Ok(effects)
    }

    fn check(&self) -> Result<(), EffectsError> {
        let l = &self.limits;
        if l.max_particles < 1 || l.max_particles.saturating_add(l.max_tracer_dots) > MAX_INSTANCES
        {
            return err(format!(
                "limits.max_particles (at least 1) and limits.max_tracer_dots together must be at most \
                 {MAX_INSTANCES}, the size of the GPU buffer"
            ));
        }
        in_range("limits.min_px", l.min_px, 0.5, 64.0)?;
        in_range("limits.max_px", l.max_px, 1.0, 512.0)?;
        if l.min_px >= l.max_px {
            return err("limits.min_px must be below limits.max_px");
        }
        in_range("limits.middle_below_px", l.middle_below_px, 1.0, 1000.0)?;
        in_range("limits.middle_factor", l.middle_factor, 0.05, 1.0)?;
        if !(1..=100).contains(&l.smoke_refuse_percent) {
            return err("limits.smoke_refuse_percent must be 1 to 100");
        }
        in_range(
            "limits.strategic_flash_alpha",
            l.strategic_flash_alpha,
            0.5,
            1.0,
        )?;
        if !(1..=1000).contains(&l.max_event_age_ticks) {
            return err("limits.max_event_age_ticks must be 1 to 1000");
        }
        in_range("limits.rise_jitter", l.rise_jitter, 0.0, 2.0)?;
        in_range("limits.smoke_fade_in", l.smoke_fade_in, 1.0, 100.0)?;
        in_range("limits.flash_hold", l.flash_hold, 0.0, 0.95)?;
        in_range("limits.tracer_taper", l.tracer_taper, 0.0, 1.0)?;
        let p = &self.placement;
        in_range("placement.hit_height_m", p.hit_height_m, 0.0, 10.0)?;
        in_range("placement.death_lift_m", p.death_lift_m, 0.0, 10.0)?;
        in_range(
            "placement.projectile_lift_m",
            p.projectile_lift_m,
            0.0,
            10.0,
        )?;
        in_range(
            "placement.impact_radius_fraction",
            p.impact_radius_fraction,
            0.0,
            2.0,
        )?;
        in_range(
            "placement.death_radius_fraction",
            p.death_radius_fraction,
            0.0,
            2.0,
        )?;
        in_range(
            "placement.min_burst_radius_m",
            p.min_burst_radius_m,
            0.0,
            10.0,
        )?;
        let m = &self.muzzle;
        in_range("muzzle.reach_fraction", m.reach_fraction, 0.0, 2.0)?;
        in_range("muzzle.height_fraction", m.height_fraction, 0.0, 2.0)?;
        in_range("muzzle.min_reach_m", m.min_reach_m, 0.0, 10.0)?;

        let mut sounds: BTreeMap<&str, &str> = BTreeMap::new();
        for (name, e) in &self.effects {
            let at = |field: &str| format!("effects.{name}.{field}");
            if !(1..=MAX_COUNT).contains(&e.count) {
                return err(format!(
                    "{} is {}, must be 1 to {MAX_COUNT}",
                    at("count"),
                    e.count
                ));
            }
            in_range(&at("life_s"), e.life_s, 0.01, 10.0)?;
            in_range(&at("size_m"), e.size_m, 0.05, 50.0)?;
            in_range(&at("growth_mps"), e.growth_mps, -50.0, 50.0)?;
            in_range(&at("alpha"), e.alpha, 0.01, 1.0)?;
            in_range(&at("rise_mps"), e.rise_mps, -50.0, 50.0)?;
            in_range(&at("spread_mps"), e.spread_mps, 0.0, 50.0)?;
            if !e.sound.is_empty() {
                if !is_event_name(&e.sound) {
                    return err(format!(
                        "{} is \"{}\", not a dotted lower-case event name",
                        at("sound"),
                        e.sound
                    ));
                }
                if let Some(other) = sounds.insert(&e.sound, name) {
                    return err(format!(
                        "effects \"{other}\" and \"{name}\" share the sound \"{}\"",
                        e.sound
                    ));
                }
            }
        }
        let known = |list: &[String], what: &str, required: bool| -> Result<(), EffectsError> {
            if required && list.is_empty() {
                return err(format!("{what} names no effect"));
            }
            for name in list {
                if !self.effects.contains_key(name) {
                    return err(format!("{what} names the unknown effect \"{name}\""));
                }
            }
            Ok(())
        };
        for (damage, d) in &self.by_damage_type {
            known(&d.fire, &format!("by_damage_type.{damage}.fire"), true)?;
            known(&d.impact, &format!("by_damage_type.{damage}.impact"), true)?;
        }
        for (weapon, w) in &self.by_weapon {
            known(&w.fire, &format!("by_weapon.{weapon}.fire"), false)?;
            known(&w.impact, &format!("by_weapon.{weapon}.impact"), false)?;
            if w.fire.is_empty() && w.impact.is_empty() {
                return err(format!("by_weapon.{weapon} overrides nothing"));
            }
        }
        known(&self.miss, "miss", true)?;
        let d = &self.death;
        in_range(
            "death.infantry_below_cells",
            d.infantry_below_cells,
            0.1,
            10.0,
        )?;
        in_range("death.small_below_cells", d.small_below_cells, 0.1, 10.0)?;
        if d.infantry_below_cells > d.small_below_cells {
            return err("death.infantry_below_cells must not exceed death.small_below_cells");
        }
        known(&d.infantry, "death.infantry", true)?;
        known(&d.small, "death.small", true)?;
        known(&d.vehicle, "death.vehicle", true)?;
        known(&d.structure, "death.structure", true)?;
        known(
            std::slice::from_ref(&self.wreck_smoke.effect),
            "wreck_smoke.effect",
            true,
        )?;
        in_range(
            "wreck_smoke.per_second",
            self.wreck_smoke.per_second,
            0.0,
            60.0,
        )?;
        in_range(
            "wreck_smoke.radius_fraction",
            self.wreck_smoke.radius_fraction,
            0.0,
            2.0,
        )?;
        in_range(
            "wreck_smoke.fade_fraction",
            self.wreck_smoke.fade_fraction,
            0.0,
            1.0,
        )?;
        for (name, t) in self.tracers.all() {
            let at = |field: &str| format!("tracers.{name}.{field}");
            if !(1..=16).contains(&t.dots) {
                return err(format!("{} must be 1 to 16", at("dots")));
            }
            in_range(&at("spacing_m"), t.spacing_m, 0.0, 20.0)?;
            in_range(&at("size_m"), t.size_m, 0.05, 20.0)?;
            in_range(&at("alpha"), t.alpha, 0.01, 1.0)?;
            known(&t.trail, &at("trail"), false)?;
            in_range(&at("trail_per_s"), t.trail_per_s, 0.0, 120.0)?;
            if t.trail.is_empty() != (t.trail_per_s == 0.0) {
                return err(format!(
                    "{} and {} go together",
                    at("trail"),
                    at("trail_per_s")
                ));
            }
        }
        Ok(())
    }

    /// Checks the file against the unit definitions: every damage type has an entry (and every entry is
    /// a damage type), every `by_weapon` key is a weapon id.
    pub fn check_against(&self, defs: &UnitDefs) -> Result<(), EffectsError> {
        for t in &defs.classes().damage_types {
            if !self.by_damage_type.contains_key(&t.name) {
                return err(format!(
                    "damage type \"{}\" has no by_damage_type entry",
                    t.name
                ));
            }
        }
        for name in self.by_damage_type.keys() {
            if defs.classes().damage_kind(name).is_none() {
                return err(format!("by_damage_type has \"{name}\", not a damage type"));
            }
        }
        for id in self.by_weapon.keys() {
            if defs.weapon(id).is_none() {
                return err(format!("by_weapon has \"{id}\", not a weapon id"));
            }
        }
        Ok(())
    }

    /// The fire effects of a weapon: its own if `by_weapon` has any, else its damage type's.
    pub fn fire_of(&self, weapon_id: &str, damage_type: &str) -> &[String] {
        match self.by_weapon.get(weapon_id) {
            Some(w) if !w.fire.is_empty() => &w.fire,
            _ => self
                .by_damage_type
                .get(damage_type)
                .map_or(&[], |d| &d.fire),
        }
    }

    /// The impact effects of a weapon: its own if `by_weapon` has any, else its damage type's.
    pub fn impact_of(&self, weapon_id: &str, damage_type: &str) -> &[String] {
        match self.by_weapon.get(weapon_id) {
            Some(w) if !w.impact.is_empty() => &w.impact,
            _ => self
                .by_damage_type
                .get(damage_type)
                .map_or(&[], |d| &d.impact),
        }
    }
}

impl Tracers {
    /// The four tracers by name.
    pub fn all(&self) -> [(&'static str, &Tracer); 4] {
        [
            ("straight", &self.straight),
            ("arc", &self.arc),
            ("guided", &self.guided),
            ("falling", &self.falling),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shipped() -> Effects {
        Effects::embedded()
    }

    /// The shipped limits, number by number: a change to the file must change this test too.
    #[test]
    fn the_shipped_limits_are_these_numbers() {
        let l = shipped().limits;
        assert_eq!(l.max_particles, 4096);
        assert_eq!(l.max_tracer_dots, 2048);
        assert_eq!(l.min_px, 2.0);
        assert_eq!(l.max_px, 64.0);
        assert_eq!(l.middle_below_px, 60.0);
        assert_eq!(l.middle_factor, 0.5);
        assert_eq!(l.smoke_refuse_percent, 75);
        assert_eq!(l.strategic_flash_alpha, 0.6);
        assert_eq!(l.max_event_age_ticks, 40);
        assert_eq!(
            (l.rise_jitter, l.smoke_fade_in, l.flash_hold, l.tracer_taper),
            (0.4, 8.0, 0.5, 0.6)
        );
        let p = shipped().placement;
        assert_eq!(
            (
                p.hit_height_m,
                p.death_lift_m,
                p.projectile_lift_m,
                p.impact_radius_fraction,
                p.death_radius_fraction,
                p.min_burst_radius_m
            ),
            (0.6, 0.5, 1.0, 0.5, 0.4, 0.3)
        );
        assert_eq!(shipped().wreck_smoke.radius_fraction, 0.25);
        let m = shipped().muzzle;
        assert_eq!(
            (m.reach_fraction, m.height_fraction, m.min_reach_m),
            (0.5, 0.22, 0.8)
        );
    }

    /// Every effect, every number: `(name, count, life_s, size_m, growth, colour, alpha, glow, rise, spread, sound)`.
    #[test]
    #[allow(clippy::type_complexity)]
    fn every_shipped_effect_is_these_numbers() {
        #[rustfmt::skip]
        let want: &[(&str, u32, f32, f32, f32, (u8, u8, u8), f32, bool, f32, f32, &str)] = &[
            ("blast_cannon",     4,  0.32, 2.0, 7.0,  (255, 170, 80),  1.0,  true,  1.0, 4.0, "impact.cannon"),
            ("blast_demolition", 10, 0.60, 5.0, 10.0, (255, 150, 60),  1.0,  true,  1.0, 8.0, "impact.demolition"),
            ("blast_rocket",     5,  0.40, 2.4, 8.0,  (255, 150, 70),  1.0,  true,  1.0, 4.0, "impact.rocket"),
            ("blast_shell",      8,  0.50, 3.6, 9.0,  (255, 160, 70),  1.0,  true,  1.0, 6.0, "impact.shell"),
            ("damage_fire",      1,  0.70, 2.0, 0.6,  (255, 120, 30),  1.0,  true,  2.0, 0.5, ""),
            ("damage_smoke",     1,  3.00, 2.6, 1.6,  (50, 48, 46),    0.55, false, 2.6, 0.5, ""),
            ("dust_puff",        3,  0.80, 1.4, 2.5,  (165, 140, 105), 0.45, false, 0.8, 2.5, "impact.miss"),
            ("explosion_large",  14, 0.90, 6.0, 10.0, (255, 140, 50),  1.0,  true,  2.0, 9.0, "explosion.large"),
            ("explosion_medium", 9,  0.60, 3.0, 8.0,  (255, 150, 60),  1.0,  true,  1.5, 5.0, "explosion.medium"),
            ("explosion_small",  6,  0.45, 2.2, 7.0,  (255, 160, 70),  1.0,  true,  1.5, 4.0, "explosion.small"),
            ("flash_bullet",     1,  0.08, 0.9, 3.0,  (255, 226, 150), 1.0,  true,  0.0, 0.0, "weapon.fire.bullet"),
            ("flash_cannon",     3,  0.14, 1.8, 6.0,  (255, 200, 110), 1.0,  true,  0.0, 2.0, "weapon.fire.cannon"),
            ("flash_demolition", 1,  0.10, 1.0, 3.0,  (255, 240, 200), 1.0,  true,  0.0, 0.0, "weapon.fire.demolition"),
            ("flash_howitzer",   7,  0.35, 4.0, 10.0, (255, 210, 130), 1.0,  true,  0.0, 5.0, "weapon.fire.howitzer"),
            ("flash_precision",  1,  0.06, 0.7, 2.0,  (255, 250, 220), 1.0,  true,  0.0, 0.0, "weapon.fire.precision"),
            ("flash_rocket",     3,  0.22, 1.4, 5.0,  (255, 170, 90),  1.0,  true,  0.0, 3.0, "weapon.fire.rocket"),
            ("flash_shell",      5,  0.28, 2.8, 8.0,  (255, 190, 100), 1.0,  true,  0.0, 4.0, "weapon.fire.shell"),
            ("pop_infantry",     3,  0.35, 1.0, 3.0,  (255, 200, 120), 1.0,  true,  1.0, 3.0, "unit.destroyed.infantry"),
            ("smoke_blast",      4,  1.20, 2.2, 3.0,  (110, 105, 100), 0.50, false, 2.0, 2.0, ""),
            ("smoke_death",      5,  1.80, 3.0, 3.5,  (80, 78, 75),    0.55, false, 2.5, 3.0, ""),
            ("smoke_muzzle",     2,  0.70, 1.2, 2.0,  (150, 150, 150), 0.40, false, 1.0, 1.0, ""),
            ("smoke_wreck",      1,  2.00, 2.0, 2.0,  (70, 68, 66),    0.35, false, 2.0, 0.5, ""),
            ("spark_bullet",     3,  0.16, 0.5, 2.0,  (255, 220, 150), 1.0,  true,  1.5, 4.0, "impact.bullet"),
            ("spark_precision",  3,  0.18, 0.5, 2.0,  (255, 245, 210), 1.0,  true,  1.5, 3.0, "impact.precision"),
            ("trail_smoke",      1,  0.80, 0.6, 1.5,  (190, 190, 190), 0.35, false, 0.3, 0.3, ""),
        ];
        let e = shipped();
        assert_eq!(
            e.effects.len(),
            want.len(),
            "an effect was added or removed"
        );
        for (row, (name, def)) in want.iter().zip(&e.effects) {
            assert_eq!(row.0, name, "the effects are sorted by name");
            let got = (
                def.count,
                def.life_s,
                def.size_m,
                def.growth_mps,
                def.colour,
                def.alpha,
                def.glow,
                def.rise_mps,
                def.spread_mps,
            );
            let exp = (
                row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9,
            );
            assert_eq!(got, exp, "{name}");
            assert_eq!(def.sound, row.10, "{name} sound");
        }
    }

    /// Which effects each damage type, the one weapon override, the miss, the deaths, the wreck smoke and the
    /// tracers use.
    #[test]
    fn the_links_and_the_rest_are_these_numbers() {
        let e = shipped();
        let pairs: Vec<(&str, Vec<&str>, Vec<&str>)> = e
            .by_damage_type
            .iter()
            .map(|(k, d)| {
                (
                    k.as_str(),
                    d.fire.iter().map(String::as_str).collect(),
                    d.impact.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("bullet", vec!["flash_bullet"], vec!["spark_bullet"]),
                (
                    "cannon",
                    vec!["flash_cannon", "smoke_muzzle"],
                    vec!["blast_cannon"]
                ),
                (
                    "demolition",
                    vec!["flash_demolition"],
                    vec!["blast_demolition", "smoke_blast"]
                ),
                (
                    "precision",
                    vec!["flash_precision"],
                    vec!["spark_precision"]
                ),
                (
                    "rocket",
                    vec!["flash_rocket", "smoke_muzzle"],
                    vec!["blast_rocket", "smoke_blast"]
                ),
                (
                    "shell",
                    vec!["flash_shell", "smoke_muzzle"],
                    vec!["blast_shell", "smoke_blast"]
                ),
            ]
        );
        assert_eq!(e.by_weapon.len(), 1);
        assert_eq!(
            e.by_weapon["howitzer"].fire,
            ["flash_howitzer", "smoke_muzzle"]
        );
        assert!(e.by_weapon["howitzer"].impact.is_empty());
        assert_eq!(e.miss, ["dust_puff"]);
        let d = &e.death;
        assert_eq!((d.infantry_below_cells, d.small_below_cells), (1.0, 1.3));
        assert_eq!(d.infantry, ["pop_infantry"]);
        assert_eq!(d.small, ["explosion_small", "smoke_death"]);
        assert_eq!(d.vehicle, ["explosion_medium", "smoke_death"]);
        assert_eq!(d.structure, ["explosion_large", "smoke_death"]);
        let w = &e.wreck_smoke;
        assert_eq!(
            (w.effect.as_str(), w.per_second, w.fade_fraction),
            ("smoke_wreck", 3.0, 0.2)
        );
        let t: Vec<_> = e
            .tracers
            .all()
            .iter()
            .map(|(n, t)| {
                (
                    *n,
                    t.dots,
                    t.spacing_m,
                    t.size_m,
                    t.colour,
                    t.alpha,
                    t.trail.len(),
                    t.trail_per_s,
                )
            })
            .collect();
        assert_eq!(
            t,
            vec![
                ("straight", 4, 0.9, 0.45, (255, 235, 160), 1.0, 0, 0.0),
                ("arc", 2, 0.7, 0.9, (255, 200, 120), 1.0, 0, 0.0),
                ("guided", 5, 0.8, 0.55, (255, 190, 120), 1.0, 1, 30.0),
                ("falling", 3, 1.2, 0.8, (255, 230, 170), 1.0, 0, 0.0),
            ]
        );
        assert_eq!(e.tracers.guided.trail, ["trail_smoke"]);
    }

    /// Every weapon of the shipped definitions resolves to a fire and an impact effect.
    #[test]
    fn every_shipped_weapon_resolves_to_a_fire_and_an_impact_effect() {
        let defs = UnitDefs::embedded();
        let e = shipped();
        e.check_against(&defs)
            .expect("the shipped file matches the definitions");
        assert!(!defs.weapons().is_empty());
        for w in defs.weapons() {
            let fire = e.fire_of(&w.id, &w.damage_type);
            let impact = e.impact_of(&w.id, &w.damage_type);
            assert!(!fire.is_empty(), "{} has a fire effect", w.id);
            assert!(!impact.is_empty(), "{} has an impact effect", w.id);
        }
        // The one override wins; the others take their damage type's.
        assert_eq!(
            e.fire_of("howitzer", "shell"),
            ["flash_howitzer", "smoke_muzzle"]
        );
        assert_eq!(
            e.impact_of("howitzer", "shell"),
            ["blast_shell", "smoke_blast"]
        );
        assert_eq!(
            e.fire_of("cannon", "cannon"),
            ["flash_cannon", "smoke_muzzle"]
        );
    }

    /// The event names are well formed and no two effects share one.
    #[test]
    fn the_sound_names_are_well_formed_and_unique() {
        let e = shipped();
        let mut seen = std::collections::BTreeSet::new();
        for (name, d) in &e.effects {
            if d.sound.is_empty() {
                continue;
            }
            assert!(is_event_name(&d.sound), "{name}: {}", d.sound);
            assert!(seen.insert(d.sound.clone()), "{name}: {} twice", d.sound);
        }
        for good in ["a.b", "weapon.fire.bullet", "explosion.small", "x_1.y2"] {
            assert!(is_event_name(good), "{good}");
        }
        for bad in ["", "a", "a.", ".a", "a..b", "A.b", "a b.c", "a-b.c"] {
            assert!(!is_event_name(bad), "{bad:?}");
        }
    }

    /// Edits the shipped text in one place and says what the loader answers.
    fn refused(find: &str, replace: &str) -> String {
        assert!(EMBEDDED_EFFECTS.contains(find), "{find} is in the file");
        Effects::from_ron(&EMBEDDED_EFFECTS.replacen(find, replace, 1))
            .expect_err("the loader refuses it")
            .0
    }

    #[test]
    fn the_loader_refuses_what_it_should() {
        assert!(refused("max_particles: 4096", "max_particles: 0").contains("max_particles"));
        assert!(refused("min_px: 2.0", "min_px: 80.0").contains("min_px"));
        assert!(refused("middle_factor: 0.5", "middle_factor: 0.0").contains("middle_factor"));
        assert!(
            refused("strategic_flash_alpha: 0.6", "strategic_flash_alpha: 0.2")
                .contains("strategic_flash_alpha")
        );
        assert!(
            refused("fire: [\"flash_bullet\"]", "fire: [\"flash_nope\"]")
                .contains("unknown effect \"flash_nope\"")
        );
        assert!(refused("miss: [\"dust_puff\"]", "miss: []").contains("miss"));
        assert!(
            refused("sound: \"impact.bullet\"", "sound: \"Impact Bullet\"").contains("event name")
        );
        assert!(
            refused("sound: \"impact.bullet\"", "sound: \"impact.cannon\"")
                .contains("share the sound")
        );
        assert!(refused("count: 14,", "count: 0,").contains("count"));
        assert!(refused("life_s: 0.08", "life_s: -1.0").contains("life_s"));
        assert!(refused("trail_per_s: 30.0", "trail_per_s: 0.0").contains("go together"));
        assert!(refused("limits: (", "extra: 1, limits: (").contains("extra"));
        assert!(
            refused(
                "size_m: 0.9, growth_mps: 3.0,",
                "size_m: 0.9, growth_mps: 3.0, wobble: 1,"
            )
            .contains("wobble")
        );
        assert!(
            refused("small_below_cells: 1.3", "small_below_cells: 0.5").contains("must not exceed")
        );
    }

    #[test]
    fn a_file_that_does_not_match_the_definitions_is_refused() {
        let defs = UnitDefs::embedded();
        let drop_bullet = EMBEDDED_EFFECTS.replacen(
            "\"bullet\":     (fire: [\"flash_bullet\"],                       impact: [\"spark_bullet\"]),",
            "",
            1,
        );
        let e = Effects::from_ron(&drop_bullet).expect("valid on its own");
        let msg = e.check_against(&defs).expect_err("bullet has no entry").0;
        assert!(msg.contains("\"bullet\""), "{msg}");
        let wrong = EMBEDDED_EFFECTS.replacen("\"howitzer\": (fire", "\"mortar\": (fire", 1);
        let e = Effects::from_ron(&wrong).expect("valid on its own");
        let msg = e.check_against(&defs).expect_err("mortar is no weapon").0;
        assert!(msg.contains("\"mortar\""), "{msg}");
    }
}
