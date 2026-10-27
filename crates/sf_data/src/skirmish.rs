//! The armed skirmish of the combat benchmark (`assets/config/skirmish.ron`, WP5.6b): where the waves of units
//! appear, how often, who is in a wave and how the two sides face each other.
//!
//! The file is the scene of `sf_demo --bench --fight` (M0c, `docs/bench/README.md`). It is data and not code
//! (PLAN §5 rule 6): a number that changes what the benchmark shows, or the checksums of its world, lives here, is
//! checked when it is loaded ([`Skirmish::from_ron`]) and against the unit definitions
//! ([`Skirmish::check_against`]), and is pinned by a test. `sf_game::skirmish` places it with commands.

use std::collections::BTreeSet;
use std::fmt;

use serde::Deserialize;

use crate::units::UnitDefs;

/// The shipped file, compiled in.
pub const EMBEDDED_SKIRMISH: &str = include_str!("../../../assets/config/skirmish.ron");

/// Why a skirmish file was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkirmishError(pub String);

impl fmt::Display for SkirmishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "skirmish.ron: {}", self.0)
    }
}

impl std::error::Error for SkirmishError {}

fn err<T>(message: impl Into<String>) -> Result<T, SkirmishError> {
    Err(SkirmishError(message.into()))
}

/// One side: the owner of its units and which way from the middle of a site they stand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Side {
    /// The player number.
    pub owner: u8,
    /// `-1` west of the middle of the site, `1` east of it.
    pub direction: i32,
}

/// One place of the skirmish, in cells on the map of 256 x 256 cells the benchmark path is written for.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Site {
    /// What the site is for.
    pub name: String,
    /// The middle between the two sides.
    pub at: (i32, i32),
}

/// One unit of a wave.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    /// The id of an armed role in `unit_defs.ron`.
    pub role: String,
    /// Tenths of a cell behind the side's front line.
    pub along: i32,
    /// Tenths of a cell sideways.
    pub across: i32,
}

/// The file as written.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skirmish {
    /// Ticks between two waves at a site.
    pub wave_every_ticks: u64,
    /// The tick of the last wave.
    pub last_wave_tick: u64,
    /// Tenths of a cell between the two front lines.
    pub gap_tenths: i32,
    /// The two sides.
    pub sides: Vec<Side>,
    /// The sites.
    pub sites: Vec<Site>,
    /// The wave of one side.
    pub wave: Vec<Slot>,
}

impl Skirmish {
    /// The shipped file.
    ///
    /// # Panics
    ///
    /// If the compiled-in file is invalid, which a test rules out.
    pub fn embedded() -> Skirmish {
        Skirmish::from_ron(EMBEDDED_SKIRMISH)
            .unwrap_or_else(|e| panic!("the embedded skirmish.ron is valid: {e}"))
    }

    /// Parses and checks the text of a `skirmish.ron` (everything that does not need the unit definitions).
    pub fn from_ron(text: &str) -> Result<Skirmish, SkirmishError> {
        let s: Skirmish = ron::from_str(text).map_err(|e| SkirmishError(e.to_string()))?;
        s.check()?;
        Ok(s)
    }

    fn check(&self) -> Result<(), SkirmishError> {
        if !(1..=1000).contains(&self.wave_every_ticks) {
            return err("wave_every_ticks must be 1 to 1000");
        }
        if self.last_wave_tick > 100_000 {
            return err("last_wave_tick must be at most 100000");
        }
        // A wave per site per `wave_every_ticks`, up to the last: bounded, so a typo cannot schedule millions.
        if self.waves() > 1000 {
            return err("more than 1000 waves");
        }
        if !(1..=200).contains(&self.gap_tenths) {
            return err("gap_tenths must be 1 to 200 (0.1 to 20 cells)");
        }
        if self.sides.len() != 2 {
            return err("two sides are needed");
        }
        if self.sides[0].owner == self.sides[1].owner {
            return err("the two sides need different owners");
        }
        if self.sides.iter().any(|s| s.direction.abs() != 1) {
            return err("a side's direction is -1 or 1");
        }
        if self.sides[0].direction == self.sides[1].direction {
            return err("the two sides stand on opposite sides of the site");
        }
        if self.sites.is_empty() || self.sites.len() > 8 {
            return err("1 to 8 sites are needed");
        }
        let mut names = BTreeSet::new();
        for site in &self.sites {
            if !names.insert(site.name.as_str()) {
                return err(format!("two sites are called {}", site.name));
            }
            if !(8..=248).contains(&site.at.0) || !(8..=248).contains(&site.at.1) {
                return err(format!(
                    "site {} must be 8 to 248 cells from the map's origin (it is placed on a map of 256 cells)",
                    site.name
                ));
            }
        }
        if self.wave.is_empty() || self.wave.len() > 64 {
            return err("a wave has 1 to 64 units");
        }
        for slot in &self.wave {
            if !(-100..=200).contains(&slot.along) || !(-100..=100).contains(&slot.across) {
                return err(format!(
                    "{}: along must be -100 to 200 and across -100 to 100 tenths of a cell",
                    slot.role
                ));
            }
        }
        Ok(())
    }

    /// How many waves a site gets: at tick 0 and every `wave_every_ticks` up to `last_wave_tick`.
    pub fn waves(&self) -> u64 {
        self.last_wave_tick / self.wave_every_ticks + 1
    }

    /// How many units the whole skirmish makes: waves, sites, sides and units of a wave.
    pub fn units(&self) -> u64 {
        self.waves() * self.sites.len() as u64 * self.sides.len() as u64 * self.wave.len() as u64
    }

    /// Checks the roles against the unit definitions: every one exists and is armed (a unit with no weapon would
    /// stand in a fight and do nothing).
    pub fn check_against(&self, defs: &UnitDefs) -> Result<(), SkirmishError> {
        for slot in &self.wave {
            let Some(def) = defs.unit(&slot.role) else {
                return err(format!("{} is not a unit of unit_defs.ron", slot.role));
            };
            if def.weapons.is_empty() {
                return err(format!("{} has no weapon", slot.role));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shipped() -> Skirmish {
        Skirmish::embedded()
    }

    /// Every number of the shipped file, as literals.
    #[test]
    fn every_shipped_number_is_this_number() {
        let s = shipped();
        assert_eq!(
            (s.wave_every_ticks, s.last_wave_tick, s.gap_tenths),
            (100, 1400, 36)
        );
        assert_eq!(
            s.sides,
            [
                Side {
                    owner: 0,
                    direction: -1
                },
                Side {
                    owner: 1,
                    direction: 1
                }
            ]
        );
        let sites: Vec<(&str, (i32, i32))> =
            s.sites.iter().map(|x| (x.name.as_str(), x.at)).collect();
        assert_eq!(
            sites,
            [
                ("centre", (128, 128)),
                ("mid_hold", (174, 97)),
                ("low_pass", (140, 171))
            ]
        );
        let wave: Vec<(&str, i32, i32)> = s
            .wave
            .iter()
            .map(|x| (x.role.as_str(), x.along, x.across))
            .collect();
        assert_eq!(
            wave,
            [
                ("rifleman", 0, -12),
                ("rifleman", 0, 0),
                ("rifleman", 0, 12),
                ("rocket_trooper", 0, -24),
                ("rocket_trooper", 0, 24),
                ("heavy_tank", 16, 0),
                ("light_tank", 16, -28),
                ("light_tank", 16, 28),
            ]
        );
        assert_eq!((s.waves(), s.units()), (15, 720));
    }

    #[test]
    fn the_shipped_roles_exist_and_are_armed() {
        shipped()
            .check_against(&UnitDefs::embedded())
            .expect("the shipped skirmish matches the shipped definitions");
    }

    #[test]
    fn a_role_that_does_not_exist_or_is_not_armed_is_refused() {
        let defs = UnitDefs::embedded();
        let mut s = shipped();
        s.wave[0].role = "no_such_role".into();
        assert!(
            s.check_against(&defs)
                .unwrap_err()
                .0
                .contains("no_such_role")
        );
        // A harvester has no weapon.
        s.wave[0].role = "harvester".into();
        assert!(s.check_against(&defs).unwrap_err().0.contains("no weapon"));
    }

    #[test]
    fn bad_files_are_refused_with_the_field_named() {
        let text = EMBEDDED_SKIRMISH;
        let bad = |from: &str, to: &str| {
            assert!(text.contains(from), "{from}");
            Skirmish::from_ron(&text.replacen(from, to, 1))
                .unwrap_err()
                .0
        };
        assert!(bad("wave_every_ticks: 100", "wave_every_ticks: 0").contains("wave_every_ticks"));
        assert!(bad("last_wave_tick: 1400", "last_wave_tick: 999999").contains("last_wave_tick"));
        assert!(bad("wave_every_ticks: 100", "wave_every_ticks: 1").contains("1000 waves"));
        assert!(bad("gap_tenths: 36", "gap_tenths: 0").contains("gap_tenths"));
        assert!(bad("(owner: 1, direction: 1)", "(owner: 0, direction: 1)").contains("owners"));
        assert!(bad("(owner: 1, direction: 1)", "(owner: 1, direction: 0)").contains("direction"));
        assert!(bad("(owner: 1, direction: 1)", "(owner: 1, direction: -1)").contains("opposite"));
        assert!(bad("\"mid_hold\"", "\"centre\"").contains("two sites"));
        assert!(bad("(128, 128)", "(2, 128)").contains("centre"));
        assert!(bad("across: -12", "across: -300").contains("rifleman"));
        // An unknown field is refused, not ignored.
        assert!(bad("gap_tenths: 36,", "gap_tenths: 36, colour: 1,").contains("colour"));
    }
}
