//! A performance check of the crowd pass (WP4.4): 2,000 units with bodies on a 256 by
//! 256 cell map, the shipped unit definitions, two halves walking through each other.
//!
//! Prints how many ticks a second the simulation runs at (`cargo test --release -p
//! sf_headless --test crowd_perf -- --nocapture`) and asserts that the number is not
//! absurd, with floors far below what a release build does so that a slow runner does
//! not fail the build: the point is that the cost of a tick is bounded (the index is a
//! counting sort, a pass looks at nine buckets round each unit) and that the pass does
//! not make a tick take seconds. The ticks a second of the headline benchmark, the
//! wanderers of `sf_demo`, do not collide and are not touched.

use std::time::Instant;

use sf_bridge::{movement_table, unit_table};
use sf_data::movement::MovementCosts;
use sf_data::terrain::TerrainTypes;
use sf_data::units::UnitDefs;
use sf_math::{Fx, Vec2};
use sf_sim::{Command, EntityId, Kind, Order, PlayerId, World, WorldConfig};

/// The roles that are spawned: the rifleman (kind 12) and the light tank (kind 19).
const RIFLEMAN: Kind = Kind(12);
const LIGHT_TANK: Kind = Kind(19);

const UNITS: u32 = 2000;
const TICKS: u64 = 300;

fn world() -> World {
    let defs = UnitDefs::embedded();
    let costs = MovementCosts::from_defs(&defs, &TerrainTypes::embedded()).unwrap();
    World::new(WorldConfig::new(0x7E44_0045).with_map_size(256, 256))
        .unwrap()
        .with_units(unit_table(&defs).unwrap())
        .unwrap()
        .with_movement(movement_table(&costs).unwrap())
        .unwrap()
}

fn cell(x: u32, y: u32) -> Vec2 {
    Vec2::new(
        Fx::from_int(x as i32) + Fx::HALF,
        Fx::from_int(y as i32) + Fx::HALF,
    )
}

/// 1,000 units in a block of the west and 1,000 in a block of the east, mixed
/// riflemen and tanks, each half ordered to the other block's side.
fn crowd() -> (World, Vec<EntityId>) {
    let mut w = world();
    let mut ids = Vec::new();
    for k in 0..UNITS {
        let west = k % 2 == 0;
        let n = k / 2;
        // A block 40 cells wide and 25 high on each side, two cells a unit.
        let x = if west { 20 } else { 196 } + (n % 20) * 2;
        let y = 100 + (n / 20) * 2;
        let kind = if n % 3 == 0 { LIGHT_TANK } else { RIFLEMAN };
        w.schedule(
            w.tick(),
            PlayerId(0),
            Command::SpawnUnit {
                kind,
                pos: cell(x, y),
                owner: PlayerId(0),
            },
        )
        .unwrap();
        ids.push(EntityId::new(k, 0));
    }
    w.step();
    assert_eq!(w.len(), UNITS as usize);
    for (k, &id) in ids.iter().enumerate() {
        let west = k % 2 == 0;
        let n = k as u32 / 2;
        let goal = if west {
            cell(200 + (n % 20) * 2, 110)
        } else {
            cell(24 + (n % 20) * 2, 110)
        };
        w.schedule(
            w.tick(),
            PlayerId(0),
            Command::Order {
                ids: vec![id],
                order: Order::Move(goal),
                queued: false,
            },
        )
        .unwrap();
    }
    (w, ids)
}

#[test]
fn two_thousand_units_with_bodies_run_at_a_bounded_cost() {
    let (mut w, _) = crowd();
    let start = Instant::now();
    let mut slowest = std::time::Duration::ZERO;
    for _ in 0..TICKS {
        let t = Instant::now();
        w.step();
        slowest = slowest.max(t.elapsed());
    }
    let elapsed = start.elapsed();
    let per_second = TICKS as f64 / elapsed.as_secs_f64();
    let stats = w.crowd_stats();
    eprintln!(
        "crowd_perf: {UNITS} units with bodies on 256 x 256, {TICKS} ticks in {:.2?}: \
         {per_second:.0} ticks a second ({:.2?} a tick, the slowest {:.2?}); {} ticks with a \
         push, {} idle units shoved, {} sidesteps, {} requests for a new route, at most {} bodies",
        elapsed,
        elapsed / TICKS as u32,
        slowest,
        stats.busy_ticks,
        stats.shoves,
        stats.sidesteps,
        stats.replans,
        stats.peak_bodies,
    );
    assert_eq!(stats.peak_bodies, UNITS);
    assert!(stats.busy_ticks > 0, "the halves did meet");
    // The game runs at 20 ticks a second: even in a debug build on a slow runner the
    // crowd must not need seconds for a tick.
    let floor = if cfg!(debug_assertions) { 1.0 } else { 20.0 };
    assert!(
        per_second >= floor,
        "{per_second:.1} ticks a second, the floor is {floor}"
    );
}

#[test]
fn the_crowd_of_two_thousand_is_reproducible() {
    let (mut a, _) = crowd();
    let (mut b, _) = crowd();
    for _ in 0..40 {
        a.step();
        b.step();
    }
    assert_eq!(a.checksum_history(), b.checksum_history());
}
