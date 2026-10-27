//! The armed skirmish of `sf_demo --bench --fight` (WP5.6b), the combat scene of the M0c benchmark.
//!
//! The benchmark flies a fixed camera path ([`crate::bench`]) over 5,000 wandering units. For M0c the same
//! crowd and the same path are used, and a fight is added where the path looks: shots, hits, deaths, wrecks
//! and smoke are what the frames then have to draw on top of the crowd. The fight is **fixed**: the sites, the
//! roles, the places and the ticks are constants of this file, so every run (and every machine) plays the
//! same skirmish and the checksums of the world are comparable. `tests/bench_scene.rs` and the tests of
//! `bench_scene` pin them.
//!
//! # What is placed
//!
//! Three sites, one under each phase of the flight that is to show a fight: the map centre (the `close_up`
//! phase looks there), the place of the `mid_hold` phase and the middle of the `low_pass`. At each site, every
//! [`WAVE_EVERY`] ticks from tick 0 to [`LAST_WAVE`], a **wave** of [`WAVE`] units of each side appears face to face
//! at [`GAP_TENTHS`]/10 cells, inside the reach of the weapons (cannons 5 cells, rifles 4). Nobody is given an
//! order: the units pick their own targets and fire back (WP5.4), which is what the game does, so the waves
//! keep the fight going for the whole flight (a wave that has died is replaced) and make the shots, hits,
//! deaths and wrecks the effects are to draw.
//!
//! The units are made by `SpawnUnit` commands scheduled on the world before it starts, from the unit
//! definitions (`unit_defs.ron`) and the combat of the world ([`crate::playfield::with_combat`]); nothing writes
//! to the world directly.
//!
//! # Where the crowd is
//!
//! The wanderers are the demo's own, spawned exactly as in the plain benchmark (the same seed, kinds and
//! owners). They are not armed (they have no unit definition), so the skirmish does not touch them and they
//! do not take part in it.

use sf_data::units::UnitDefs;
use sf_math::{Fx, Vec2};
use sf_sim::{Command, Kind, PlayerId, Tick, World};

use crate::sandbox::{MERIDIAN, VEYRAN};

/// Ticks between two waves at a site.
pub const WAVE_EVERY: u64 = 100;

/// The tick of the last wave (the benchmark's flight is about 1,500 ticks long).
pub const LAST_WAVE: u64 = 1400;

/// Tenths of a cell between the two front lines of a wave.
pub const GAP_TENTHS: i32 = 36;

/// One place of the skirmish, in cells on the map of 256 x 256 cells the benchmark path is written for
/// (a smaller map scales it, as the camera path does).
#[derive(Clone, Copy, Debug)]
pub struct Site {
    /// What the site is for.
    pub name: &'static str,
    /// The middle between the two sides, cells on the 256-cell map.
    pub at: (i32, i32),
}

/// The three sites. The `close_up` phase of the flight looks at (500..530 m, 500..520 m) = cells (125..132,
/// 125..130); the `mid_hold` at (700..690 m, 380..400 m) = cells (175..172, 95..100); the `low_pass` flies from
/// (300 m, 650 m) to (820 m, 720 m), so cells (140, 171) are about its middle.
pub const SITES: [Site; 3] = [
    Site {
        name: "centre",
        at: (128, 128),
    },
    Site {
        name: "mid_hold",
        at: (174, 97),
    },
    Site {
        name: "low_pass",
        at: (140, 171),
    },
];

/// One unit of a wave: its role, and where it stands in tenths of a cell: `along` is the distance behind the
/// side's front line, `across` the offset sideways.
struct Slot {
    role: &'static str,
    along: i32,
    across: i32,
}

/// The wave of one side: infantry in front (they reach the other side's front line), the tanks behind them.
/// Eight units: three riflemen and two rocket troopers, a heavy tank (two muzzles) and two light tanks (one
/// muzzle each).
const WAVE_SLOTS: [Slot; 8] = [
    Slot {
        role: "rifleman",
        along: 0,
        across: -12,
    },
    Slot {
        role: "rifleman",
        along: 0,
        across: 0,
    },
    Slot {
        role: "rifleman",
        along: 0,
        across: 12,
    },
    Slot {
        role: "rocket_trooper",
        along: 0,
        across: -24,
    },
    Slot {
        role: "rocket_trooper",
        along: 0,
        across: 24,
    },
    Slot {
        role: "heavy_tank",
        along: 16,
        across: 0,
    },
    Slot {
        role: "light_tank",
        along: 16,
        across: -28,
    },
    Slot {
        role: "light_tank",
        along: 16,
        across: 28,
    },
];

/// Units in a wave of one side.
pub const WAVE: usize = WAVE_SLOTS.len();

/// How many waves a site gets.
pub const fn waves() -> u64 {
    LAST_WAVE / WAVE_EVERY + 1
}

/// The shipped definitions' roles of the skirmish, by kind number.
fn kind_of(defs: &UnitDefs, role: &str) -> Option<Kind> {
    defs.kind_of(role).map(Kind)
}

/// Schedules every wave of every site on `world`, which must have the unit table and the combat of `defs`
/// (`with_units` and [`crate::playfield::with_combat`]) and must not have started. Returns how many units were
/// scheduled (`3 * waves * 2 * WAVE`). A role the definitions lack is left out and not counted.
pub fn schedule(world: &mut World, defs: &UnitDefs, cells: (u32, u32)) -> u32 {
    let mut scheduled = 0;
    for site in &SITES {
        // The camera path scales with the map; so does the fight.
        let cx = site.at.0 * cells.0 as i32 / 256;
        let cy = site.at.1 * cells.1 as i32 / 256;
        for wave in 0..waves() {
            let tick = Tick(wave * WAVE_EVERY);
            // A little sideways shift per wave, so that a survivor of the last wave is not stood on.
            let shift = (wave % 3) as i32 * 4 - 4;
            for (side, owner, dir) in [("meridian", MERIDIAN, -1), ("veyran", VEYRAN, 1)] {
                let _ = side;
                for slot in &WAVE_SLOTS {
                    let Some(kind) = kind_of(defs, slot.role) else {
                        continue;
                    };
                    let front = GAP_TENTHS / 2 + slot.along;
                    let x = cx * 10 + dir * front;
                    let y = cy * 10 + slot.across + shift;
                    let pos = Vec2::new(
                        Fx::from_ratio(i64::from(x), 10),
                        Fx::from_ratio(i64::from(y), 10),
                    );
                    let command = Command::SpawnUnit {
                        kind,
                        pos,
                        owner: PlayerId(owner),
                    };
                    world
                        .schedule(tick, PlayerId(owner), command)
                        .expect("a world that has not started takes commands for any tick");
                    scheduled += 1;
                }
            }
        }
    }
    scheduled
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wave_has_eight_units_and_there_are_fifteen_waves() {
        assert_eq!(WAVE, 8);
        assert_eq!(waves(), 15);
        assert_eq!(
            SITES.map(|s| s.at),
            [(128, 128), (174, 97), (140, 171)],
            "the sites under the three phases of the flight"
        );
    }

    #[test]
    fn every_role_of_a_wave_is_in_the_shipped_definitions_and_armed() {
        let defs = UnitDefs::embedded();
        for slot in &WAVE_SLOTS {
            assert!(kind_of(&defs, slot.role).is_some(), "{}", slot.role);
            let def = defs
                .unit(slot.role)
                .unwrap_or_else(|| panic!("{}", slot.role));
            assert!(!def.weapons.is_empty(), "{} is armed", slot.role);
        }
    }
}
