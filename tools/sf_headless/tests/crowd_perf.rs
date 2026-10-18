//! A performance check of the crowd pass (WP4.4): 2,000 units with bodies on a 256 by
//! 256 cell map, the shipped unit definitions and terrain, two dense blocks that are
//! sent to the same point and so run into each other.
//!
//! The west block (1,000 units, mixed riflemen and light tanks, one cell apiece) and
//! the east block start a cell apart, and each gets one group order (flowfields,
//! arrival slots) to the point between them. That is the worst case the crowd pass
//! has: everybody is within a cell or two of several others for the whole run. The
//! test asserts that the blocks really touched (idle units shoved aside, many pairs
//! of bodies from the two blocks overlapping at once, sidesteps) so the number it
//! prints is the cost of a crowd that collides, not of one that walks past.
//!
//! Prints how many ticks a second the simulation runs at (`cargo test --release -p
//! sf_headless --test crowd_perf -- --nocapture`) and asserts bounds far below what a
//! release build does so that a slow runner does not fail the build: the point is that
//! the cost of a tick is bounded (the index is a counting sort, a pass looks at nine
//! buckets round each unit), that the pass does not make a tick take seconds and that
//! no single tick is much slower than the rest. The ticks a second of the headline
//! benchmark, the wanderers of `sf_demo`, do not collide and are not touched.

use std::time::{Duration, Instant};

use sf_bridge::{movement_table, terrain_defs, unit_table};
use sf_data::movement::MovementCosts;
use sf_data::terrain::TerrainTypes;
use sf_data::units::UnitDefs;
use sf_math::{Fx, Vec2};
use sf_sim::{Command, EntityId, Kind, Order, PlayerId, Terrain, World, WorldConfig};

/// The roles that are spawned: the rifleman (kind 12) and the light tank (kind 19).
const RIFLEMAN: Kind = Kind(12);
const LIGHT_TANK: Kind = Kind(19);

const UNITS: u32 = 2000;
const TICKS: u64 = 400;
const MAP: u32 = 256;

/// The west block's first column, the east block's first column (a cell between
/// the last of the west and the first of the east), and the first row of both.
/// A block is 40 units wide and 25 high, a cell to a unit.
const WEST_X: u32 = 108;
const EAST_X: u32 = 149;
const TOP: u32 = 100;
const WIDE: u32 = 40;

/// Where both blocks are sent: the gap between them.
const GOAL: (u32, u32) = (148, 112);

fn world() -> World {
    let defs = UnitDefs::embedded();
    let types = TerrainTypes::embedded();
    let costs = MovementCosts::from_defs(&defs, &types).unwrap();
    // Grass everywhere (terrain id 0 of the shipped table).
    let layer = vec![0_u8; (MAP * MAP) as usize];
    let terrain = Terrain::from_layer(MAP, MAP, &layer, &terrain_defs(&types, &costs)).unwrap();
    World::with_terrain(
        WorldConfig::new(0x7E44_0045).with_map_size(MAP, MAP),
        terrain,
    )
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

/// Even ids are the west block, odd ids the east block.
fn is_west(id: EntityId) -> bool {
    id.index().is_multiple_of(2)
}

/// Both blocks, spawned, and one group order for each to the same point.
fn crowd() -> (World, Vec<EntityId>) {
    let mut w = world();
    let mut ids = Vec::new();
    for k in 0..UNITS {
        let west = k.is_multiple_of(2);
        let n = k / 2;
        let x = if west { WEST_X } else { EAST_X } + n % WIDE;
        let y = TOP + n / WIDE;
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
    for west in [true, false] {
        let group: Vec<EntityId> = ids
            .iter()
            .copied()
            .filter(|&id| is_west(id) == west)
            .collect();
        w.schedule(
            w.tick(),
            PlayerId(0),
            Command::Order {
                ids: group,
                order: Order::Move(cell(GOAL.0, GOAL.1)),
                queued: false,
            },
        )
        .unwrap();
    }
    (w, ids)
}

/// How many pairs of bodies, one from each block, overlap right now.
fn cross_contacts(w: &World, ids: &[EntityId]) -> usize {
    let bodies: Vec<(bool, Vec2, Fx)> = ids
        .iter()
        .map(|&id| {
            (
                is_west(id),
                w.get(id).unwrap().pos,
                w.collision_radius_of(id).unwrap(),
            )
        })
        .collect();
    let mut pairs = 0;
    for (a, &(wa, pa, ra)) in bodies.iter().enumerate() {
        for &(wb, pb, rb) in &bodies[a + 1..] {
            if wa == wb || (pa.x - pb.x).abs() > Fx::from_int(2) {
                continue;
            }
            if pa.distance(pb) < ra + rb {
                pairs += 1;
            }
        }
    }
    pairs
}

#[test]
fn two_thousand_colliding_units_run_at_a_bounded_cost() {
    let (mut w, ids) = crowd();
    let mut spent = Duration::ZERO;
    let mut slowest = Duration::ZERO;
    let mut slowest_at = 0;
    let mut most_contacts = 0;
    let mut met_at = None;
    for t in 1..=TICKS {
        let began = Instant::now();
        w.step();
        let took = began.elapsed();
        spent += took;
        if took > slowest {
            slowest = took;
            slowest_at = t;
        }
        // Looked at outside the clock of the tick.
        if t.is_multiple_of(20) {
            let pairs = cross_contacts(&w, &ids);
            if pairs > most_contacts {
                most_contacts = pairs;
            }
            if met_at.is_none() && pairs > 0 {
                met_at = Some(t);
            }
        }
    }
    // The time of the ticks alone, without the looking.
    let stats = w.crowd_stats();
    let per_second = TICKS as f64 / spent.as_secs_f64();
    eprintln!(
        "crowd_perf: {UNITS} colliding units on {MAP} x {MAP}, {TICKS} ticks in {spent:.2?}: \
         {per_second:.0} ticks a second ({:.2?} a tick, the slowest {slowest:.2?} on tick \
         {slowest_at}); the blocks first touched by tick {met_at:?}, \
         at most {most_contacts} overlapping pairs from the two blocks at once; {} ticks with a \
         push, {} idle units shoved, {} sidesteps, {} requests for a new route, at most {} bodies",
        spent / TICKS as u32,
        stats.busy_ticks,
        stats.shoves,
        stats.sidesteps,
        stats.replans,
        stats.peak_bodies,
    );
    assert_eq!(stats.peak_bodies, UNITS);
    // The blocks really met: units of one block stood in the other's way, idle ones
    // were pushed aside, and many bodies from the two overlapped at one time.
    assert!(stats.busy_ticks > 0);
    assert!(stats.shoves > 0, "no idle unit was shoved: {stats:?}");
    assert!(
        most_contacts >= 100,
        "only {most_contacts} overlapping pairs from the two blocks: they did not meet"
    );
    // The game runs at 20 ticks a second: even in a debug build on a slow runner the
    // crowd must not need seconds for a tick, and no tick may stand out.
    let floor = if cfg!(debug_assertions) { 1.0 } else { 20.0 };
    assert!(
        per_second >= floor,
        "{per_second:.1} ticks a second, the floor is {floor}"
    );
    // In a release build no tick may take longer than the 20 Hz budget, 50 ms
    // (the game steps the sim on its own thread since WP1.15, so a tick over
    // budget no longer stalls a frame, but it makes the sim fall behind its
    // clock). The review of WP4.4 measured 12.7 ms on average and 22 ms at worst.
    let longest = if cfg!(debug_assertions) {
        Duration::from_secs(5)
    } else {
        Duration::from_millis(50)
    };
    assert!(
        slowest <= longest,
        "the slowest tick ({slowest_at}) took {slowest:.2?}, the bound is {longest:.2?}"
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
