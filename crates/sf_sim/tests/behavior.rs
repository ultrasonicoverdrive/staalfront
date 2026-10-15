//! The placeholder behaviours through the public API: wander stays in bounds,
//! respects speed and turn rate, and `MoveTo` / `SetWanderArea` do what they
//! say.

use std::collections::BTreeSet;

use sf_math::{Facing, Fx, Rng, Vec2, hash::Fnv1a64};
use sf_sim::{
    Area, Command, DEFAULT_SPEED, DEFAULT_TURN_PER_TICK, EntityId, EntityView, Kind, PlayerId,
    Tick, World, WorldConfig, per_tick,
};

/// A world of the given size with `n` entities spawned on tick 0 at pseudo
/// random positions that include points outside the map (they get clamped).
fn crowd(seed: u64, width: u32, height: u32, n: usize) -> World {
    let mut w = World::new(WorldConfig::new(seed).with_map_size(width, height)).unwrap();
    let mut script = Rng::with_stream(seed, 11);
    for i in 0..n {
        let mut span = |side: u32| {
            // From 10 cells outside the map on either side.
            script.fx_range(Fx::from_int(-10), Fx::from_int(side as i32 + 10))
        };
        let pos = Vec2::new(span(width), span(height));
        w.schedule(
            Tick(0),
            PlayerId(0),
            Command::Spawn {
                kind: Kind((i % 3) as u16),
                pos,
                owner: PlayerId((i % 4) as u8),
            },
        )
        .unwrap();
    }
    w
}

fn views(w: &World) -> Vec<EntityView> {
    w.snapshot().iter().collect()
}

fn ids(w: &World) -> Vec<EntityId> {
    views(w).iter().map(|v| v.id).collect()
}

fn step_len() -> Fx {
    per_tick(DEFAULT_SPEED)
}

#[test]
fn wanderers_stay_inside_the_map() {
    // A small map, so the entities reach the walls and arrive over and over.
    let (width, height) = (24u32, 16u32);
    let mut w = crowd(5, width, height, 200);
    let max = Vec2::new(
        Fx::from_int(width as i32) - Fx::EPSILON,
        Fx::from_int(height as i32) - Fx::EPSILON,
    );
    let inside = |p: Vec2| p.x >= Fx::ZERO && p.y >= Fx::ZERO && p.x <= max.x && p.y <= max.y;
    for tick in 0..4_000 {
        w.step();
        for v in w.snapshot().iter() {
            assert!(
                inside(v.pos),
                "tick {tick}: {} left the map at {:?}",
                v.id,
                v.pos
            );
            assert!(inside(v.prev_pos), "tick {tick}: previous position outside");
        }
    }
    assert_eq!(w.len(), 200);
}

#[test]
fn entities_move_at_the_configured_speed_and_turn_at_the_configured_rate() {
    let mut w = crowd(6, 120, 120, 100);
    let step = step_len().raw();
    let (mut full_steps, mut moves, mut longest) = (0u32, 0u32, 0i64);
    for _ in 0..1_500 {
        w.step();
        for v in w.snapshot().iter() {
            let moved = v.pos.distance(v.prev_pos).raw();
            // Never faster than one step (the table vector is a hair off one).
            assert!(moved <= step + 2, "moved {moved} raw units, step is {step}");
            longest = longest.max(moved);
            if moved > 0 {
                moves += 1;
                if moved >= step - 2 {
                    full_steps += 1;
                }
            }
            assert!(
                v.facing.abs_diff(v.prev_facing) <= DEFAULT_TURN_PER_TICK,
                "turned {} steps in one tick",
                v.facing.abs_diff(v.prev_facing)
            );
        }
    }
    assert!(longest >= step - 2, "nobody ever covered a full step");
    // Most moves are full steps; the rest are arrivals and wall contacts.
    assert!(full_steps * 10 >= moves * 9, "{full_steps} of {moves}");
}

#[test]
fn wanderers_keep_exploring_the_map() {
    let side = 60i32;
    let mut w = crowd(8, side as u32, side as u32, 80);
    let mut travelled = vec![0i64; 80];
    let mut sectors: Vec<BTreeSet<(i32, i32)>> = vec![BTreeSet::new(); 80];
    for _ in 0..3_000 {
        w.step();
        for (k, v) in w.snapshot().iter().enumerate() {
            travelled[k] += v.pos.distance(v.prev_pos).raw();
            let (cx, cy) = v.pos.cell();
            sectors[k].insert((cx / 10, cy / 10));
        }
    }
    let one_cell = Fx::ONE.raw();
    for k in 0..80 {
        // A full-speed walk would be 600 cells; turning on the spot costs some.
        assert!(
            travelled[k] > 150 * one_cell,
            "entity {k} travelled only {}",
            travelled[k]
        );
        assert!(
            sectors[k].len() >= 6,
            "entity {k} visited {} sectors",
            sectors[k].len()
        );
    }
}

#[test]
fn moving_entities_head_the_way_they_face() {
    // Away from the walls an entity moves exactly along its facing vector.
    let mut w = crowd(9, 200, 200, 60);
    let step = step_len().raw();
    let mut checked = 0;
    for _ in 0..800 {
        w.step();
        for v in w.snapshot().iter() {
            let d = v.pos - v.prev_pos;
            let clear = |c: Fx| c > Fx::from_int(2) && c < Fx::from_int(198);
            if d.length().raw() >= step - 2 && clear(v.pos.x) && clear(v.pos.y) {
                assert_eq!(Facing::from_vec2(d), Some(v.facing));
                checked += 1;
            }
        }
    }
    assert!(checked > 1_000, "only {checked} moves were checked");
}

#[test]
fn the_snapshot_always_shows_the_last_two_ticks() {
    let mut w = crowd(10, 80, 80, 50);
    w.step();
    let mut last = views(&w);
    for _ in 0..300 {
        w.step();
        let now = views(&w);
        assert_eq!(now.len(), last.len());
        for (a, b) in last.iter().zip(&now) {
            assert_eq!(a.id, b.id);
            assert_eq!(b.prev_pos, a.pos, "prev_pos must be last tick's pos");
            assert_eq!(
                b.prev_facing, a.facing,
                "prev_facing must be last tick's facing"
            );
        }
        last = now;
    }
}

fn one_entity(width: u32, height: u32, x: i32, y: i32) -> (World, EntityId) {
    let mut w = World::new(WorldConfig::new(21).with_map_size(width, height)).unwrap();
    w.schedule(
        Tick(0),
        PlayerId(0),
        Command::Spawn {
            kind: Kind(0),
            pos: Vec2::from_ints(x, y),
            owner: PlayerId(0),
        },
    )
    .unwrap();
    w.step();
    let id = ids(&w)[0];
    (w, id)
}

#[test]
fn move_to_walks_a_straight_line_arrives_exactly_and_stands_still() {
    let (mut w, id) = one_entity(256, 256, 10, 10);
    let start = w.get(id).unwrap().pos;
    let target = Vec2::new(Fx::from_ratio(221, 2), Fx::from_ratio(241, 4)); // (110.5, 60.25)
    w.schedule(
        Tick(1),
        PlayerId(0),
        Command::MoveTo {
            ids: vec![id],
            target,
        },
    )
    .unwrap();

    let line = target - start;
    let line_len = line.length();
    let mut arrived_at = None;
    for tick in 1..1_000u64 {
        w.step();
        let v = w.get(id).unwrap();
        // Never strays far from the straight line start to target.
        let off_line = (v.pos - start).cross(line).abs().mul_div(Fx::ONE, line_len);
        assert!(
            off_line < Fx::HALF,
            "tick {tick}: {off_line} cells off the line"
        );
        if arrived_at.is_none() && v.pos == target {
            arrived_at = Some(tick);
        }
        if arrived_at.is_some() {
            break;
        }
    }
    let arrived_at = arrived_at.expect("the entity never arrived");
    // About 110 cells at a fifth of a cell per tick, plus at most 16 ticks
    // of turning on the spot (half a turn at 8 steps per tick).
    let walking = (line_len.raw() / step_len().raw()) as u64;
    assert!(
        arrived_at >= walking,
        "arrived after {arrived_at}, faster than {walking}"
    );
    assert!(
        arrived_at <= walking + 20,
        "arrived after {arrived_at}, expected about {walking}"
    );

    // Standing still: position, previous position and checksum-relevant
    // motion are fixed from now on.
    for _ in 0..60 {
        w.step();
        let v = w.get(id).unwrap();
        assert_eq!(v.pos, target);
        assert_eq!(v.prev_pos, target);
    }
}

#[test]
fn move_to_outside_the_map_goes_to_the_nearest_valid_point() {
    let (mut w, id) = one_entity(40, 30, 20, 15);
    w.schedule(
        Tick(1),
        PlayerId(0),
        Command::MoveTo {
            ids: vec![id],
            target: Vec2::from_ints(1_000, -50),
        },
    )
    .unwrap();
    for _ in 0..400 {
        w.step();
    }
    let edge = Vec2::new(Fx::from_int(40) - Fx::EPSILON, Fx::ZERO);
    assert_eq!(w.get(id).unwrap().pos, edge);
}

#[test]
fn a_wander_area_keeps_the_wanderers_in_it() {
    let mut w = crowd(12, 100, 100, 40);
    w.step(); // the spawns run
    let group = ids(&w);
    assert_eq!(group.len(), 40);
    // Send the whole crowd to a 12 by 12 corner.
    let area = Area::from_cells(0, 0, 12, 12);
    w.schedule(
        Tick(1),
        PlayerId(1),
        Command::SetWanderArea {
            ids: group.clone(),
            area,
        },
    )
    .unwrap();
    // The farthest corner is 140 cells away: 700 ticks of walking plus turning.
    for _ in 0..1_000 {
        w.step();
    }
    // From now on every position stays in the area; targets are inside it and
    // the path to a target inside may stray by well under a cell.
    let lo = -Fx::ONE;
    let hi = Fx::from_int(13);
    let mut visited = BTreeSet::new();
    for _ in 0..1_500 {
        w.step();
        for id in &group {
            let p = w.get(*id).unwrap().pos;
            assert!(
                p.x >= lo && p.y >= lo && p.x <= hi && p.y <= hi,
                "{id} at {p:?}"
            );
            let (cx, cy) = p.cell();
            visited.insert((cx, cy));
        }
    }
    // And they really use the area, not just a corner of it.
    assert!(visited.len() > 60, "only {} cells visited", visited.len());
}

#[test]
fn a_wander_area_outside_the_map_collapses_to_its_nearest_corner() {
    let (mut w, id) = one_entity(50, 40, 25, 20);
    w.schedule(
        Tick(1),
        PlayerId(0),
        Command::SetWanderArea {
            ids: vec![id],
            area: Area::from_cells(500, 500, 600, 600),
        },
    )
    .unwrap();
    for _ in 0..600 {
        w.step();
    }
    let corner = Vec2::new(
        Fx::from_int(50) - Fx::EPSILON,
        Fx::from_int(40) - Fx::EPSILON,
    );
    assert_eq!(w.get(id).unwrap().pos, corner);
}

#[test]
fn wander_is_a_pure_function_of_seed_and_commands() {
    // Two identical crowds stay identical position by position, and a third
    // with another seed does not.
    let hash = |seed: u64| {
        let mut w = crowd(seed, 64, 64, 120);
        let mut h = Fnv1a64::new();
        for _ in 0..500 {
            w.step();
            for v in w.snapshot().iter() {
                h.write_vec2(v.pos);
                h.write_facing(v.facing);
            }
        }
        h.finish()
    };
    assert_eq!(hash(1), hash(1));
    assert_ne!(hash(1), hash(2));
}
