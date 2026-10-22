//! Weapon definitions (`assets/config/weapon_defs.ron`).
//!
//! A weapon is a row of numbers: how far it shoots, how often, how hard, what
//! kind of damage and what kind of projectile. Units name their weapons by id.
//! Phase 5 gives the numbers their behaviour (WP5.1 to WP5.4); WP3.1 stored and
//! checked them, WP5.1 added the shape of a burst, the parameters of the
//! projectile and where the weapon is mounted. `docs/rules/weapons.md` says
//! what each field means for the work packages that follow.

use serde::Deserialize;

use super::classes::{ClassTables, is_valid_name};
use super::decimal::Decimal;
use super::error::{DefError, Source};
use super::kinds::{self, Live, Retired};

/// Largest weapon `kind`.
pub const MAX_WEAPON_KIND: u16 = 4095;

/// Longest range, minimum range and splash radius accepted, in cells.
const MAX_CELLS: i64 = 64;
/// Largest damage per shot.
const MAX_DAMAGE: u32 = 1_000_000;
/// Longest reload, in game seconds.
const MAX_RELOAD_SECONDS: i64 = 600;
/// Most shots in one burst.
pub const MAX_BURST: u8 = 16;
/// Fastest projectile, in cells per game second (800 m/s; ten cells a tick).
const MAX_PROJECTILE_SPEED: i64 = 200;
/// Highest arc or drop, in cells.
const MAX_FLIGHT_HEIGHT: i64 = 32;
/// Fastest steering of a guided projectile, in facing steps (of 256) per tick:
/// a quarter turn.
pub const MAX_GUIDE_TURN: u8 = 64;
/// The widest firing arc: half a turn to either side, which is all round.
pub const ALL_ROUND: u8 = 128;
/// The firing arc of a weapon that does not say: 8 steps of 256 to either
/// side, eleven degrees and a quarter.
pub const DEFAULT_FIRE_ARC: u8 = 8;

/// How the projectile of a weapon travels. The flight itself is WP5.2.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum Projectile {
    /// Hits the moment it is fired: no projectile exists, the damage lands in
    /// the tick of the shot (rifles, machine guns, lasers). Takes none of the
    /// projectile parameters.
    Instant,
    /// Flies straight at the target (bullets, cannon shells).
    Straight,
    /// Lobbed over obstacles on an arc (howitzers).
    Arc,
    /// Steers toward its target in flight (rockets).
    Guided,
    /// Drops from above onto the target cell.
    Falling,
}

/// What a weapon aims with.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub enum WeaponMount {
    /// Fixed to the hull: the whole unit has to face the target (infantry, a
    /// gunboat, a gun team). The default.
    #[default]
    Hull,
    /// On the turret, which turns apart from the hull (`turret_rate` of the
    /// unit; WP4.5). Only a unit with a turret may carry it.
    Turret,
}

fn default_burst() -> u8 {
    1
}

fn default_fire_arc() -> u8 {
    DEFAULT_FIRE_ARC
}

fn default_friendly_fire() -> bool {
    true
}

/// One weapon, as written in the file.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponDef {
    /// Permanent number (what the simulation uses), unique among weapons and
    /// tombstones; the position in the file does not matter.
    pub kind: u16,
    /// Unique snake_case id that units refer to.
    pub id: String,
    /// Longest distance it can hit, in cells.
    pub range: Decimal,
    /// Shortest distance it can hit, in cells; zero means none. Below `range`.
    #[serde(default)]
    pub min_range: Decimal,
    /// Time between two shots, in game seconds. A whole number of ticks
    /// (a multiple of 0.05).
    pub reload: Decimal,
    /// Damage of one hit, before armor (WP5.3).
    pub damage: u32,
    /// Name of a damage type in `unit_classes.ron`.
    pub damage_type: String,
    /// How the projectile travels.
    pub projectile: Projectile,
    /// Radius in cells in which a hit also damages, with falloff (WP5.3);
    /// zero means no splash.
    #[serde(default)]
    pub splash_radius: Decimal,
    /// Cells from the edge of a victim inside which the splash does its full
    /// damage; from there it falls in a straight line to nothing at
    /// `splash_radius` (WP5.3). Zero (left out): it starts to fall at once.
    /// Only for a weapon with a `splash_radius`, and below it.
    #[serde(default)]
    pub splash_inner: Decimal,
    /// Whether the splash also hurts the units of the shooter's own player
    /// (WP5.3). The default is true, as in a classic RTS; false spares them.
    /// The direct victim is hurt whoever it belongs to. Only for a weapon with
    /// a `splash_radius`.
    #[serde(default = "default_friendly_fire")]
    pub friendly_fire: bool,
    /// Speed of the projectile in cells per game second (WP5.2). Left out or
    /// zero: not given. Not for an `Instant` weapon.
    #[serde(default)]
    pub projectile_speed: Decimal,
    /// Height in cells: the top of the arc of an `Arc` projectile, the height
    /// a `Falling` one drops from. Left out or zero: not given. Presentation
    /// only; the flight time follows from the speed (PLAN.md 1.3).
    #[serde(default)]
    pub flight_height: Decimal,
    /// Facing steps (of 256 per turn) per tick a `Guided` projectile steers
    /// toward its target, 1 to 64. Left out or zero: not given.
    #[serde(default)]
    pub guide_turn: u8,
    /// Shots per trigger pull, 1 to 16. A burst starts when the weapon is
    /// reloaded and fires `burst` shots `burst_delay` apart.
    #[serde(default = "default_burst")]
    pub burst: u8,
    /// Time between the shots of a burst, in game seconds, a whole number of
    /// ticks. Zero for a single shot, at least one tick for a burst.
    #[serde(default)]
    pub burst_delay: Decimal,
    /// What the weapon aims with: the hull or the turret.
    #[serde(default)]
    pub mount: WeaponMount,
    /// Half the width of the firing arc in facing steps (of 256 per turn),
    /// 0 to 128 (all round): the weapon fires when the hull, or for a turret
    /// mount the turret, points within this many steps of the target.
    #[serde(default = "default_fire_arc")]
    pub fire_arc: u8,
}

/// The RON file.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct WeaponsFile {
    weapons: Vec<WeaponDef>,
    /// Tombstones of removed weapons: their numbers are never reused.
    #[serde(default)]
    retired_weapons: Vec<Retired>,
}

/// Parses `weapon_defs.ron` and checks every weapon.
pub(crate) fn parse(
    src: Source<'_>,
    classes: &ClassTables,
) -> Result<(Vec<WeaponDef>, Vec<Retired>), DefError> {
    src.check_numbers()?;
    let file: WeaponsFile = ron::from_str(src.text).map_err(|e| src.ron_error(&e))?;
    let mut weapons = file.weapons;
    for (i, w) in weapons.iter().enumerate() {
        let id = w.id.as_str();
        let here =
            |msg: String| src.error_at_record("id", id, 0, format!("weapon \"{id}\": {msg}"));
        if !is_valid_name(id) {
            return Err(here("the id is not snake_case (a-z, 0-9, _)".into()));
        }
        if weapons[..i].iter().any(|o| o.id == w.id) {
            let nth = weapons[..i].iter().filter(|o| o.id == w.id).count();
            return Err(src.error_at_record(
                "id",
                id,
                nth,
                format!("weapon id \"{id}\" appears twice"),
            ));
        }
        if w.range <= Decimal::ZERO || w.range.milli() > MAX_CELLS * 1000 {
            return Err(here(format!(
                "range {} must be above 0 and at most {MAX_CELLS} cells",
                w.range
            )));
        }
        if w.min_range.is_negative() || w.min_range >= w.range {
            return Err(here(format!(
                "min_range {} must be 0 or more and below the range {}",
                w.min_range, w.range
            )));
        }
        if w.splash_radius.is_negative() || w.splash_radius.milli() > MAX_CELLS * 1000 {
            return Err(here(format!(
                "splash_radius {} must be 0 to {MAX_CELLS} cells",
                w.splash_radius
            )));
        }
        check_splash(w, &here)?;
        match w.reload.to_ticks() {
            None if w.reload.is_negative() => {
                return Err(here(format!(
                    "reload {} s must be above 0 (at least one tick, 0.05 s)",
                    w.reload
                )));
            }
            None => {
                return Err(here(format!(
                    "reload {} s is not a whole number of ticks (a multiple of 0.05 s)",
                    w.reload
                )));
            }
            Some(0) => return Err(here("reload must be at least one tick (0.05 s)".into())),
            Some(_) if w.reload.milli() > MAX_RELOAD_SECONDS * 1000 => {
                return Err(here(format!(
                    "reload {} s is more than {MAX_RELOAD_SECONDS} s",
                    w.reload
                )));
            }
            Some(_) => {}
        }
        check_burst(w, &here)?;
        check_projectile(w, &here)?;
        if w.fire_arc > ALL_ROUND {
            return Err(here(format!(
                "fire_arc {} must be 0 to {ALL_ROUND} steps (of 256 per turn; {ALL_ROUND} is all round)",
                w.fire_arc
            )));
        }
        if w.damage == 0 || w.damage > MAX_DAMAGE {
            return Err(here(format!("damage must be 1 to {MAX_DAMAGE}")));
        }
        if classes.damage_kind(&w.damage_type).is_none() {
            return Err(src.error_in_record(
                "id",
                id,
                &w.damage_type,
                format!(
                    "weapon \"{id}\": unknown damage type \"{}\" (not in unit_classes.ron)",
                    w.damage_type
                ),
            ));
        }
    }
    if weapons.is_empty() {
        return Err(src.error("the list of weapons is empty"));
    }
    let live: Vec<Live<'_>> = weapons
        .iter()
        .map(|w| Live {
            kind: w.kind,
            name: &w.id,
        })
        .collect();
    kinds::check(
        src,
        "weapon",
        "id",
        MAX_WEAPON_KIND,
        &live,
        &file.retired_weapons,
    )?;
    weapons.sort_by_key(|w| w.kind);
    Ok((weapons, file.retired_weapons))
}

/// The splash (WP5.3): the full-damage radius lies inside the radius, and neither it nor the
/// friendly-fire flag is given to a weapon that has no splash.
fn check_splash(w: &WeaponDef, here: &dyn Fn(String) -> DefError) -> Result<(), DefError> {
    if w.splash_inner.is_negative() {
        return Err(here(format!(
            "splash_inner {} must be 0 or more",
            w.splash_inner
        )));
    }
    if w.splash_radius.is_zero() {
        if !w.splash_inner.is_zero() {
            return Err(here(format!(
                "splash_inner {} is for weapons with a splash_radius",
                w.splash_inner
            )));
        }
        if !w.friendly_fire {
            return Err(here(
                "friendly_fire: false is for weapons with a splash_radius (a direct hit has no side to spare)"
                    .into(),
            ));
        }
    } else if w.splash_inner >= w.splash_radius {
        return Err(here(format!(
            "splash_inner {} must be below the splash_radius {}",
            w.splash_inner, w.splash_radius
        )));
    }
    Ok(())
}

/// The burst: its length, the delay between the shots, and that the reload is
/// longer than the burst itself (the reload is counted from the first shot, see
/// `docs/rules/weapons.md`).
fn check_burst(w: &WeaponDef, here: &dyn Fn(String) -> DefError) -> Result<(), DefError> {
    if w.burst == 0 || w.burst > MAX_BURST {
        return Err(here(format!("burst {} must be 1 to {MAX_BURST}", w.burst)));
    }
    let Some(delay) = w.burst_delay.to_ticks() else {
        return Err(here(format!(
            "burst_delay {} s is not a whole number of ticks (a multiple of 0.05 s)",
            w.burst_delay
        )));
    };
    if w.burst == 1 && delay > 0 {
        return Err(here(format!(
            "burst_delay {} s has no use with a burst of 1 shot",
            w.burst_delay
        )));
    }
    if w.burst > 1 && delay == 0 {
        return Err(here(format!(
            "a burst of {} shots needs a burst_delay of at least one tick (0.05 s)",
            w.burst
        )));
    }
    let span = u64::from(w.burst - 1) * u64::from(delay);
    let reload = u64::from(w.reload.to_ticks().unwrap_or(0));
    if span >= reload {
        return Err(here(format!(
            "reload {} s is not longer than the burst of {} shots {} s apart ({span} ticks): \
             the next burst would start before this one has ended",
            w.reload, w.burst, w.burst_delay
        )));
    }
    Ok(())
}

/// The parameters of the projectile: which belong to which kind, and their
/// bounds. None is required (the scenario copies of the files predate them);
/// the shipped file gives every projectile a speed, a test sees to that.
fn check_projectile(w: &WeaponDef, here: &dyn Fn(String) -> DefError) -> Result<(), DefError> {
    let kind = w.projectile;
    let only = |name: &str, used_by: &str| {
        here(format!("{name} is for {used_by} projectiles, not {kind:?}"))
    };
    if w.projectile_speed.is_negative() || w.projectile_speed.milli() > MAX_PROJECTILE_SPEED * 1000
    {
        return Err(here(format!(
            "projectile_speed {} must be 0 to {MAX_PROJECTILE_SPEED} cells per second",
            w.projectile_speed
        )));
    }
    if w.flight_height.is_negative() || w.flight_height.milli() > MAX_FLIGHT_HEIGHT * 1000 {
        return Err(here(format!(
            "flight_height {} must be 0 to {MAX_FLIGHT_HEIGHT} cells",
            w.flight_height
        )));
    }
    if w.guide_turn > MAX_GUIDE_TURN {
        return Err(here(format!(
            "guide_turn {} must be 0 to {MAX_GUIDE_TURN} steps per tick",
            w.guide_turn
        )));
    }
    if kind == Projectile::Instant && !w.projectile_speed.is_zero() {
        return Err(here(
            "an Instant weapon has no projectile, so no projectile_speed".into(),
        ));
    }
    if !w.flight_height.is_zero() && !matches!(kind, Projectile::Arc | Projectile::Falling) {
        return Err(only("flight_height", "Arc and Falling"));
    }
    if w.guide_turn != 0 && kind != Projectile::Guided {
        return Err(only("guide_turn", "Guided"));
    }
    Ok(())
}
