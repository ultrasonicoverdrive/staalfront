//! The world of the engine demo's crowd scene and of its benchmark, without a window (WP5.6b).
//!
//! `sf_demo --bench` (M0, M0b) flies a camera over 5,000 wanderers; `sf_demo --bench --fight` (M0c) flies the
//! same path over the same crowd with an armed skirmish added ([`crate::skirmish`]). The runner builds
//! those worlds with the functions of this module, and the tests build them with the same functions and step
//! them with no window and no GPU: a **headless twin** of the benchmark scene. That is what makes the
//! benchmark's checksums checkable: `tests/bench_scene.rs` pins the plain benchmark's checksum at tick 1500
//! (`0x9e196a6ced2ef024`, the one M0 recorded, `docs/bench/m0-2026-10-22.json`) and the fight benchmark's
//! checksums every 100 ticks, so M0, M0b and M0c stay comparable and a change to the scene or the simulation
//! that moves them fails a test.
//!
//! The functions here do exactly what `Runner::new` did inline before WP5.6b for the crowd scene, in the same
//! order (the plain scene's checksums did not move).

use sf_data::map::Map;
use sf_data::units::UnitDefs;
use sf_math::Fx;
use sf_sim::{World, WorldConfig};

use crate::population::Population;

/// How fast the demo's units move, cells a tick fraction (the sim's own default would be 58 km/h at 4 m a
/// cell).
pub const DEMO_SPEED: Fx = Fx::from_ratio(3, 2);

/// The world configuration of the demo on a map of `cells`: seed 1 and [`DEMO_SPEED`] (B12).
pub fn world_config(cells: (u32, u32)) -> WorldConfig {
    WorldConfig {
        speed: DEMO_SPEED,
        ..WorldConfig::new(1).with_map_size(cells.0, cells.1)
    }
}

/// The demo's world on `map`: its terrain and its ore, no entities yet.
pub fn empty_world(map: &Map) -> Result<World, String> {
    crate::playfield::make_world(world_config(map.size_in_cells()), map)
}

/// `world` with the unit table of `defs` and the combat of the game ([`crate::playfield::with_combat`]):
/// what the skirmish needs. Not applied to the plain benchmark, whose world has no unit table.
///
/// # Panics
///
/// If the shipped definitions make no valid unit table, or the world has started.
pub fn armed(world: World, defs: &UnitDefs) -> World {
    let table = sf_bridge::unit_table(defs).expect("the shipped definitions make a unit table");
    let world = world
        .with_units(table)
        .expect("a world that has not started takes its unit table");
    crate::playfield::with_combat(world, defs)
}

/// Spawns the crowd of `population` (`units` entities, the buildings of the pack in a base at the centre),
/// schedules the skirmish if `defs` are given, runs tick 0 and plants the buildings. Returns the number of
/// buildings and the number of skirmish units scheduled (0 without `defs`). This is the crowd scene of the
/// demo, unchanged since WP1.14, with the skirmish scheduled between the spawn and the first tick.
pub fn populate(
    world: &mut World,
    population: &Population,
    units: u32,
    cells: (u32, u32),
    skirmish: Option<&UnitDefs>,
) -> (u32, u32) {
    let buildings = population.spawn(world, units, cells);
    let armed_units = skirmish.map_or(0, |defs| crate::skirmish::schedule(world, defs, cells));
    // Tick 0 creates the entities; the buildings are then told to stay.
    let mut views = Vec::new();
    world.step();
    world.snapshot().collect_into(&mut views);
    population.plant_buildings(world, &views);
    (buildings, armed_units)
}
