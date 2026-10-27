//! The armed skirmish of `sf_demo --bench --fight` (WP5.6b), the combat scene of the M0c benchmark.
//!
//! The benchmark flies a fixed camera path ([`crate::bench`]) over 5,000 wandering units. For M0c the same
//! crowd and the same path are used, and a fight is added where the path looks: shots, hits, deaths, wrecks
//! and smoke are what the frames then have to draw on top of the crowd. The fight is **fixed**: the sites, the
//! roles, the places and the ticks are the data of `assets/config/skirmish.ron` (`sf_data::skirmish`), so every
//! run (and every machine) plays the same skirmish and the checksums of the world are comparable.
//! `tests/bench_scene.rs` pins them.
//!
//! # What is placed
//!
//! Sites under the phases of the flight that are to show a fight (the map centre for `close_up`, then the
//! place of `mid_hold` and the middle of the `low_pass`). At each site, every `wave_every_ticks` from tick 0 to
//! `last_wave_tick`, a **wave** of units of each side appears face to face at `gap_tenths`/10 cells, inside the
//! reach of the weapons (cannons 5 cells, rifles 4). Nobody is given an order: the units pick their own targets
//! and fire back (WP5.4), which is what the game does, so the waves keep the fight going for the whole flight
//! (a wave that has died is replaced) and make the shots, hits, deaths and wrecks the effects are to draw.
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

use sf_data::skirmish::Skirmish;
use sf_data::units::UnitDefs;
use sf_math::{Fx, Vec2};
use sf_sim::{Command, Kind, PlayerId, Tick, World};

/// The map the sites of the file are written for, cells a side (the camera path of the benchmark is too).
pub const PATH_MAP_CELLS: i32 = 256;

/// Schedules every wave of every site of the shipped `skirmish.ron` on `world`, which must have the unit table and
/// the combat of `defs` (`with_units` and [`crate::playfield::with_combat`]) and must not have started. `cells` is
/// the map's size: the sites scale with it as the camera path does. Returns how many units were scheduled
/// (`Skirmish::units`; a role the definitions lack is left out and not counted).
///
/// # Panics
///
/// If the shipped file does not match `defs`, which a test rules out for the shipped definitions.
pub fn schedule(world: &mut World, defs: &UnitDefs, cells: (u32, u32)) -> u32 {
    let skirmish = Skirmish::embedded();
    skirmish
        .check_against(defs)
        .unwrap_or_else(|e| panic!("the skirmish matches the unit definitions: {e}"));
    let mut scheduled = 0;
    for site in &skirmish.sites {
        // The camera path scales with the map; so does the fight.
        let cx = site.at.0 * cells.0 as i32 / PATH_MAP_CELLS;
        let cy = site.at.1 * cells.1 as i32 / PATH_MAP_CELLS;
        for wave in 0..skirmish.waves() {
            let tick = Tick(wave * skirmish.wave_every_ticks);
            // A little sideways shift per wave, so that a survivor of the last wave is not stood on.
            let shift = (wave % 3) as i32 * 4 - 4;
            for side in &skirmish.sides {
                for slot in &skirmish.wave {
                    let Some(kind) = defs.kind_of(&slot.role) else {
                        continue;
                    };
                    let front = skirmish.gap_tenths / 2 + slot.along;
                    let x = cx * 10 + side.direction * front;
                    let y = cy * 10 + slot.across + shift;
                    let tenth = |v: i32| Fx::from_ratio(i64::from(v), 10);
                    let command = Command::SpawnUnit {
                        kind: Kind(kind),
                        pos: Vec2::new(tenth(x), tenth(y)),
                        owner: PlayerId(side.owner),
                    };
                    world
                        .schedule(tick, PlayerId(side.owner), command)
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

    /// The scheduling counts what the file says, and a smaller map scales the sites (the first wave at tick 0
    /// is there after one step).
    #[test]
    fn the_shipped_skirmish_schedules_its_units_and_scales_with_the_map() {
        let defs = UnitDefs::embedded();
        let skirmish = Skirmish::embedded();
        for cells in [256_u32, 128] {
            let world =
                World::new(sf_sim::WorldConfig::new(1).with_map_size(cells, cells)).unwrap();
            let mut world = crate::bench_scene::armed(world, &defs);
            let scheduled = schedule(&mut world, &defs, (cells, cells));
            assert_eq!(u64::from(scheduled), skirmish.units(), "{cells} cells");
            world.step();
            let first_wave = skirmish.sites.len() * skirmish.sides.len() * skirmish.wave.len();
            assert_eq!(world.len(), first_wave, "{cells} cells");
            // Every unit of the first wave is within 6 cells of its (scaled) site.
            let scaled: Vec<(f32, f32)> = skirmish
                .sites
                .iter()
                .map(|s| {
                    let f = cells as f32 / PATH_MAP_CELLS as f32;
                    (s.at.0 as f32 * f, s.at.1 as f32 * f)
                })
                .collect();
            for v in world.snapshot().iter() {
                let [x, y] = sf_math::render_bridge::vec2_to_f32(v.pos);
                assert!(
                    scaled
                        .iter()
                        .any(|s| (x - s.0).abs() < 6.0 && (y - s.1).abs() < 6.0),
                    "a unit at ({x}, {y}) on {cells} cells"
                );
            }
        }
    }
}
