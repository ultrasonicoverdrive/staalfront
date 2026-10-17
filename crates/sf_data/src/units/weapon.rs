//! Weapon definitions (`assets/config/weapon_defs.ron`).
//!
//! A weapon is a row of numbers: how far it shoots, how often, how hard, what
//! kind of damage and what kind of projectile. Units name their weapons by id.
//! Phase 5 gives the numbers their behaviour (WP5.1 to WP5.4); WP3.1 only
//! stores and checks them. `docs/rules/units.md` lists every field.

use serde::Deserialize;

use super::classes::{ClassTables, is_valid_name};
use super::decimal::Decimal;
use super::error::{DefError, Source};

/// Longest range, minimum range and splash radius accepted, in cells.
const MAX_CELLS: i64 = 64;
/// Largest damage per shot.
const MAX_DAMAGE: u32 = 1_000_000;
/// Longest reload, in game seconds.
const MAX_RELOAD_SECONDS: i64 = 600;

/// How the projectile of a weapon travels. The flight itself is WP5.2.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum Projectile {
    /// Flies straight at the target (bullets, cannon shells).
    Straight,
    /// Lobbed over obstacles on an arc (howitzers).
    Arc,
    /// Steers toward its target in flight (rockets).
    Guided,
    /// Drops from above onto the target cell.
    Falling,
}

/// One weapon, as written in the file.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponDef {
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
}

/// The RON file.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct WeaponsFile {
    weapons: Vec<WeaponDef>,
}

/// Parses `weapon_defs.ron` and checks every weapon.
pub(crate) fn parse(src: Source<'_>, classes: &ClassTables) -> Result<Vec<WeaponDef>, DefError> {
    let file: WeaponsFile = ron::from_str(src.text).map_err(|e| src.ron_error(&e))?;
    let weapons = file.weapons;
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
        match w.reload.to_ticks() {
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
        if w.damage == 0 || w.damage > MAX_DAMAGE {
            return Err(here(format!("damage must be 1 to {MAX_DAMAGE}")));
        }
        if classes.damage_index(&w.damage_type).is_none() {
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
    Ok(weapons)
}
