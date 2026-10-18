//! Tests of the parts of the crowd pass below the world: the settings, the
//! shapes, the distance two bodies keep, the spatial index against a plain scan
//! of every pair, and the state a unit remembers. The behaviour (pushing, passing,
//! holding, stuck) is in `tests/collision.rs` and the headless scenarios
//! `bridge_200` and `crowd_jam`.

use sf_math::hash::Fnv1a64;
use sf_math::{Facing, Fx, Rng, Vec2};

use super::*;
use crate::behavior::Area;
use crate::entities::NewEntity;
use crate::ids::{Kind, PlayerId};
use crate::movement::{ClassCosts, Cost};
use crate::terrain::TerrainId;
use crate::units::UnitStats;

fn stats(kind: u16, class: Option<u16>, size_tenths: i64) -> UnitStats {
    UnitStats {
        kind: Kind(kind),
        hp: 10,
        movement_class: class,
        speed_per_tick: if class.is_some() {
            Fx::from_ratio(1, 20)
        } else {
            Fx::ZERO
        },
        turn_per_tick: if class.is_some() { 8 } else { 0 },
        size: Fx::from_ratio(size_tenths, 10),
        footprint: None,
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    }
}

fn movement() -> MovementTable {
    let class = |class, list: [Cost; 2]| ClassCosts {
        class,
        costs: [TerrainId(0), TerrainId(1)].into_iter().zip(list).collect(),
    };
    MovementTable::new(vec![
        class(0, [Cost::Speed(1000), Cost::Impassable]),
        class(5, [Cost::Speed(1000), Cost::Speed(1000)]),
    ])
    .unwrap()
}

#[test]
fn the_default_settings_are_valid_and_the_limits_are_the_documented_ones() {
    let ok = CollisionConfig::DEFAULT;
    assert!(ok.is_valid());
    assert_eq!(ok, CollisionConfig::default());
    let with = |wait, stuck, attempts| CollisionConfig {
        enabled: true,
        wait_ticks: wait,
        stuck_ticks: stuck,
        stuck_attempts: attempts,
    };
    assert!(with(1, 2, 1).is_valid());
    assert!(!with(0, 40, 3).is_valid(), "at least a tick to wait");
    assert!(!with(40, 40, 3).is_valid(), "stuck after waiting");
    assert!(!with(6, MAX_STUCK_TICKS + 1, 3).is_valid());
    assert!(with(6, MAX_STUCK_TICKS, 3).is_valid());
    assert!(!with(6, 40, 0).is_valid(), "at least one attempt");
}

#[test]
fn a_shape_is_made_of_the_size_and_the_movement_table() {
    let units = UnitTable::new(vec![
        stats(1, Some(0), 4),  // a rifleman
        stats(2, Some(0), 12), // a tank
        stats(3, Some(5), 10), // a plane: its class enters every terrain type
        stats(4, None, 5),     // an object
        stats(5, Some(0), 30), // a ferry
        stats(6, Some(9), 4),  // a class the movement table does not know
    ])
    .unwrap();
    let shapes = shapes_of(&units, &movement());
    let at = |kind: u16| shapes[units.index_of(Kind(kind)).unwrap()];
    assert!(at(1).collides && at(1).small);
    assert_eq!(at(1).radius, Fx::from_ratio(1, 5));
    assert_eq!(at(1).mass, 1);
    assert!(at(2).collides && !at(2).small);
    assert_eq!(at(2).radius, Fx::from_ratio(3, 5));
    assert_eq!(at(2).mass, 2, "a cell and a fifth wide");
    assert!(!at(3).collides, "aircraft pass over everything");
    assert!(!at(4).collides, "an object has no class");
    assert_eq!(at(5).mass, 3);
    // A class that is not understood is never blocked by terrain, but it is a body.
    assert!(at(6).collides);
    // Without a movement table every class is a body.
    let open = shapes_of(&units, &MovementTable::empty());
    assert!(open[units.index_of(Kind(3)).unwrap()].collides);
}

#[test]
fn two_small_bodies_keep_three_quarters_of_the_radii_and_the_rest_the_whole_sum() {
    let body = |r: i64, small| Body {
        radius: Fx::from_ratio(r, 10),
        small,
        ..Body::NONE
    };
    // Tenths of a cell cannot all be written exactly in sixteen bits of fraction.
    let near =
        |a: Fx, tenths: i64| (a - Fx::from_ratio(tenths, 10)).abs() < Fx::from_ratio(1, 1000);
    assert!(near(min_distance(&body(2, true), &body(2, true)), 3));
    assert!(near(min_distance(&body(6, false), &body(6, false)), 12));
    assert!(near(min_distance(&body(2, true), &body(6, false)), 8));
    // Symmetric.
    assert_eq!(
        min_distance(&body(2, true), &body(6, false)),
        min_distance(&body(6, false), &body(2, true))
    );
}

fn entities(points: &[(i32, i32)]) -> Entities {
    let mut e = Entities::default();
    for (n, &(x, y)) in points.iter().enumerate() {
        let i = e.push_free_slot();
        e.fill(
            i,
            NewEntity {
                kind: Kind(1),
                owner: PlayerId(0),
                // A thirtieth of a cell off the middle, so that nothing is on an edge.
                pos: Vec2::new(
                    Fx::from_ratio(i64::from(x) * 100 + 3, 100),
                    Fx::from_ratio(i64::from(y) * 100 + 7, 100),
                ),
                facing: Facing::default(),
                behavior: crate::behavior::Behavior::Idle,
                area: Area::new(Vec2::ZERO, Vec2::ZERO),
                rng: Rng::new(n as u64),
                def: 0,
                hp: 10,
            },
        );
    }
    e
}

fn body(radius_tenths: i64) -> Body {
    Body {
        role: Role::Idle,
        radius: Fx::from_ratio(radius_tenths, 10),
        small: false,
        mass: 1,
        class: 0,
        owner: 0,
    }
}

#[test]
fn the_index_finds_every_pair_a_plain_scan_finds_in_slot_order() {
    // A spread of points over a 24 by 24 map, three of the slots not bodies at all.
    let mut rng = Rng::new(99);
    let points: Vec<(i32, i32)> = (0..300)
        .map(|_| {
            (
                (rng.next_u32() % 2400) as i32,
                (rng.next_u32() % 2400) as i32,
            )
        })
        .collect();
    let e = entities(&points);
    let mut bodies: Vec<Body> = (0..points.len()).map(|_| body(6)).collect();
    for dead in [4, 77, 250] {
        bodies[dead] = Body::NONE;
    }
    let mut index = Index::default();
    // Buckets two cells wide: the biggest pair distance is a body and a half.
    index.configure(2, 24, 24);
    index.build(&e, &bodies);
    assert_eq!(index.items.len(), points.len() - 3);
    for u in 0..points.len() {
        if bodies[u].role == Role::None {
            continue;
        }
        let near: Vec<u32> = index.near(e.pos[u]).collect();
        // Every body within the biggest pair distance (a little over one cell) is found, each once.
        let mut sorted = near.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), near.len(), "no body twice");
        for (v, other) in bodies.iter().enumerate() {
            if other.role == Role::None || v == u {
                continue;
            }
            let d = (e.pos[u] - e.pos[v]).length();
            if d <= Fx::from_ratio(12, 10) {
                assert!(near.contains(&(v as u32)), "{u} does not find {v} at {d:?}");
            }
        }
        assert!(near.contains(&(u as u32)), "a body finds itself");
    }
    // Inside a bucket the slots ascend.
    for b in 0..index.start.len() - 1 {
        let items = &index.items[index.start[b] as usize..index.start[b + 1] as usize];
        assert!(items.windows(2).all(|w| w[0] < w[1]));
    }
    // Rebuilding gives the same index.
    let before = index.items.clone();
    index.build(&e, &bodies);
    assert_eq!(index.items, before);
}

#[test]
fn a_unit_at_rest_leaves_the_checksum_alone_and_one_that_remembers_does_not() {
    let rest = CrowdState::default();
    assert!(rest.at_rest());
    // The scratch of a tick is not memory.
    let walking = CrowdState {
        walking: true,
        refused: true,
        stranded: true,
        ..CrowdState::default()
    };
    assert!(walking.at_rest());
    for remembers in [
        CrowdState {
            quiet: 1,
            ..CrowdState::default()
        },
        CrowdState {
            attempts: 1,
            ..CrowdState::default()
        },
        CrowdState {
            side: Some((Vec2::ONE, 3)),
            ..CrowdState::default()
        },
        CrowdState {
            shoved: Fx::EPSILON,
            ..CrowdState::default()
        },
        CrowdState {
            replan: true,
            ..CrowdState::default()
        },
        CrowdState {
            idle_hit: true,
            ..CrowdState::default()
        },
        // WP4.4b: held up by arrived units, and a settled unit that closes up.
        CrowdState {
            pressed: 1,
            ..CrowdState::default()
        },
        CrowdState {
            home: Some(Vec2::ONE),
            ..CrowdState::default()
        },
    ] {
        assert!(!remembers.at_rest(), "{remembers:?}");
        let (mut a, mut b) = (Fnv1a64::new(), Fnv1a64::new());
        rest.write_hash(&mut a);
        remembers.write_hash(&mut b);
        assert_ne!(a.finish(), b.finish(), "{remembers:?} is in the hash");
    }
    // A new order forgets the progress but not how far the unit was shoved.
    let mut c = CrowdState {
        quiet: 9,
        attempts: 2,
        pressed: 70,
        replan: true,
        shoved: Fx::ONE,
        side: Some((Vec2::ONE, 3)),
        ..CrowdState::default()
    };
    c.reset_progress();
    assert_eq!((c.quiet, c.attempts, c.replan, c.side), (0, 0, false, None));
    assert_eq!(c.pressed, 0);
    assert_eq!(c.shoved, Fx::ONE);
}

#[test]
fn a_slot_taken_is_in_the_hash_but_does_not_keep_the_crowd_pass_awake() {
    let rest = CrowdState::default();
    let hash = |c: &CrowdState| {
        let mut h = Fnv1a64::new();
        c.write_hash(&mut h);
        h.finish()
    };
    let held = |serial, slot| CrowdState {
        claim: Claim { serial, slot },
        ..CrowdState::default()
    };
    // A unit standing on its slot is not "remembering a crowd": the pass sleeps when
    // all that is left is the settled.
    assert!(held(3, 7).at_rest());
    // But which slot it holds is state that later arrivals depend on.
    assert_ne!(hash(&rest), hash(&held(3, 7)));
    assert_ne!(hash(&held(3, 7)), hash(&held(3, 8)));
    assert_ne!(hash(&held(3, 7)), hash(&held(4, 7)));
    assert_ne!(hash(&held(3, 7)), hash(&held(3, Claim::SHARED)));
    // A unit that took none hashes as it did before WP4.4b.
    assert_eq!(hash(&rest), hash(&CrowdState::default()));
}
