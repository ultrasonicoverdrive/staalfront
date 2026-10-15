//! Coverage tests for the checksum: every piece of state it claims to cover
//! must change the value, and the pieces it claims to leave out must not.
//!
//! These tests reach into the private columns, which is why they live inside
//! the crate. They are the guard against someone adding a column to the store
//! and forgetting to hash it: the list of mutations below has to grow with
//! the columns.

use std::collections::BTreeSet;

use sf_math::{Facing, Fx, Rng, RngState, Vec2};

use super::*;
use crate::behavior::Behavior;
use crate::ids::{EntityId, Kind, PlayerId, Tick};
use crate::world::WorldConfig;

/// A world with live entities in every behaviour, targets set and unset, a
/// non-trivial free list and used random streams, after 25 ticks.
fn base() -> World {
    let mut w = World::new(WorldConfig::new(7).with_map_size(64, 48)).unwrap();
    let ids: Vec<EntityId> = (0..6)
        .map(|n| {
            w.spawn(
                Kind(n as u16),
                Vec2::from_ints(5 + n * 7, 6 + n * 5),
                PlayerId(n as u8 % 3),
            )
            .unwrap()
        })
        .collect();
    // One idle, one on its way somewhere, the rest wandering.
    w.entities.behavior[1] = Behavior::Idle;
    w.entities.behavior[2] = Behavior::MoveTo;
    w.entities.target[2] = Some(Vec2::from_ints(60, 40));
    for _ in 0..25 {
        w.step();
    }
    // Free two slots so the free list has an order that matters.
    w.despawn(ids[4]);
    w.despawn(ids[3]);
    assert_eq!(w.free, vec![4, 3]);
    w
}

fn slot_with_target(w: &World, wanted: bool) -> usize {
    (0..w.entities.len())
        .find(|&i| w.entities.alive[i] && w.entities.target[i].is_some() == wanted)
        .expect("the base world has both kinds")
}

type Mutation = (&'static str, fn(&mut World));

fn bump_fx(v: Fx) -> Fx {
    Fx::from_raw(v.raw() + 1)
}

/// Every covered piece of state, changed by the smallest amount possible.
fn covered_mutations() -> Vec<Mutation> {
    vec![
        ("config.seed", |w| w.config.seed ^= 1),
        ("config.width", |w| w.config.width += 1),
        ("config.height", |w| w.config.height += 1),
        ("config.speed", |w| w.config.speed = bump_fx(w.config.speed)),
        ("config.turn_per_tick", |w| w.config.turn_per_tick += 1),
        ("tick", |w| w.tick = Tick(w.tick.0 + 1)),
        ("spawn_serial", |w| w.spawn_serial += 1),
        ("generation of a live slot", |w| {
            w.entities.generation[0] += 1
        }),
        ("generation of a free slot", |w| {
            w.entities.generation[3] += 1
        }),
        ("live flag", |w| w.entities.alive[5] = false),
        ("kind", |w| {
            w.entities.kind[0] = Kind(w.entities.kind[0].0 + 1)
        }),
        ("owner", |w| {
            w.entities.owner[0] = PlayerId(w.entities.owner[0].0 + 1)
        }),
        ("pos.x", |w| {
            w.entities.pos[0].x = bump_fx(w.entities.pos[0].x)
        }),
        ("pos.y", |w| {
            w.entities.pos[0].y = bump_fx(w.entities.pos[0].y)
        }),
        ("facing", |w| {
            w.entities.facing[0] = w.entities.facing[0].turn(1)
        }),
        ("behavior to idle", |w| {
            w.entities.behavior[0] = Behavior::Idle
        }),
        ("behavior to move", |w| {
            w.entities.behavior[0] = Behavior::MoveTo
        }),
        ("behavior idle to wander", |w| {
            w.entities.behavior[1] = Behavior::Wander
        }),
        ("target removed", |w| {
            let i = slot_with_target(w, true);
            w.entities.target[i] = None;
        }),
        ("target added", |w| {
            let i = slot_with_target(w, false);
            w.entities.target[i] = Some(Vec2::ZERO);
        }),
        ("target.x", |w| {
            let i = slot_with_target(w, true);
            let t = w.entities.target[i].unwrap();
            w.entities.target[i] = Some(Vec2::new(bump_fx(t.x), t.y));
        }),
        ("target.y", |w| {
            let i = slot_with_target(w, true);
            let t = w.entities.target[i].unwrap();
            w.entities.target[i] = Some(Vec2::new(t.x, bump_fx(t.y)));
        }),
        ("area.min.x", |w| {
            w.entities.area[0].min.x = bump_fx(w.entities.area[0].min.x)
        }),
        ("area.min.y", |w| {
            w.entities.area[0].min.y = bump_fx(w.entities.area[0].min.y)
        }),
        ("area.max.x", |w| {
            w.entities.area[0].max.x = bump_fx(w.entities.area[0].max.x)
        }),
        ("area.max.y", |w| {
            w.entities.area[0].max.y = bump_fx(w.entities.area[0].max.y)
        }),
        ("rng state", |w| {
            w.entities.rng[0].next_u32();
        }),
        ("rng stream", |w| {
            let s = w.entities.rng[0].state();
            w.entities.rng[0] = Rng::from_state(RngState {
                state: s.state,
                inc: s.inc + 2,
            });
        }),
        ("free list order", |w| w.free.reverse()),
        ("free list longer", |w| w.free.push(0)),
        ("free list shorter", |w| {
            w.free.pop();
        }),
    ]
}

/// State the checksum deliberately does not look at.
fn uncovered_mutations() -> Vec<Mutation> {
    vec![
        ("prev_pos of a live slot", |w| {
            w.entities.prev_pos[0].x = bump_fx(w.entities.prev_pos[0].x)
        }),
        ("prev_facing of a live slot", |w| {
            w.entities.prev_facing[0] = w.entities.prev_facing[0].turn(5)
        }),
        ("position of a free slot", |w| {
            w.entities.pos[3] = Vec2::from_ints(9, 9)
        }),
        ("everything else of a free slot", |w| {
            w.entities.kind[3] = Kind(99);
            w.entities.owner[3] = PlayerId(9);
            w.entities.facing[3] = Facing::new(77);
            w.entities.behavior[3] = Behavior::Wander;
            w.entities.target[3] = Some(Vec2::ONE);
            w.entities.rng[3] = Rng::new(123);
        }),
        ("the checksum history", |w| w.history.push(0)),
        ("the queue's insertion counter", |w| {
            w.queue
                .schedule(
                    Tick(0),
                    Tick(1_000),
                    PlayerId(0),
                    Command::Despawn {
                        id: EntityId::new(0, 0),
                    },
                )
                .unwrap();
            w.queue.pop_due(Tick(1_000));
        }),
    ]
}

#[test]
fn every_covered_piece_of_state_changes_the_checksum() {
    let w = base();
    let reference = w.checksum();
    let mut seen = BTreeSet::new();
    seen.insert(reference);
    for (name, mutate) in covered_mutations() {
        let mut m = w.clone();
        mutate(&mut m);
        let sum = m.checksum();
        assert_ne!(
            sum, reference,
            "changing {name} did not change the checksum"
        );
        assert!(
            seen.insert(sum),
            "changing {name} collided with another change"
        );
    }
}

#[test]
fn state_the_checksum_leaves_out_does_not_change_it() {
    let w = base();
    let reference = w.checksum();
    for (name, mutate) in uncovered_mutations() {
        let mut m = w.clone();
        mutate(&mut m);
        assert_eq!(
            m.checksum(),
            reference,
            "{name} should not be part of the checksum"
        );
    }
}

#[test]
fn the_checksum_is_stable_for_equal_state() {
    let a = base();
    let b = base();
    assert!(a == b);
    assert_eq!(a.checksum(), b.checksum());
    assert_eq!(a.checksum(), a.clone().checksum());
}

fn id(n: u32) -> EntityId {
    EntityId::new(n, 0)
}

/// One pending command each, differing from its neighbours in exactly one
/// field, including the tick and the player it is scheduled for.
fn queue_variants() -> Vec<(&'static str, Tick, PlayerId, Command)> {
    let spawn = |kind, x, y, owner| Command::Spawn {
        kind: Kind(kind),
        pos: Vec2::from_ints(x, y),
        owner: PlayerId(owner),
    };
    let area = |a, b, c, d| crate::behavior::Area::from_cells(a, b, c, d);
    let t = Tick(40);
    let p = PlayerId(1);
    vec![
        ("spawn", t, p, spawn(1, 2, 3, 4)),
        ("spawn tick", Tick(41), p, spawn(1, 2, 3, 4)),
        ("spawn player", t, PlayerId(2), spawn(1, 2, 3, 4)),
        ("spawn kind", t, p, spawn(2, 2, 3, 4)),
        ("spawn x", t, p, spawn(1, 5, 3, 4)),
        ("spawn y", t, p, spawn(1, 2, 5, 4)),
        ("spawn owner", t, p, spawn(1, 2, 3, 5)),
        ("despawn", t, p, Command::Despawn { id: id(1) }),
        ("despawn index", t, p, Command::Despawn { id: id(2) }),
        (
            "despawn generation",
            t,
            p,
            Command::Despawn {
                id: EntityId::new(1, 1),
            },
        ),
        (
            "wander area",
            t,
            p,
            Command::SetWanderArea {
                ids: vec![id(1), id(2)],
                area: area(1, 2, 3, 4),
            },
        ),
        (
            "wander ids order",
            t,
            p,
            Command::SetWanderArea {
                ids: vec![id(2), id(1)],
                area: area(1, 2, 3, 4),
            },
        ),
        (
            "wander more ids",
            t,
            p,
            Command::SetWanderArea {
                ids: vec![id(1), id(2), id(3)],
                area: area(1, 2, 3, 4),
            },
        ),
        (
            "wander fewer ids",
            t,
            p,
            Command::SetWanderArea {
                ids: vec![id(1)],
                area: area(1, 2, 3, 4),
            },
        ),
        (
            "wander min.x",
            t,
            p,
            Command::SetWanderArea {
                ids: vec![id(1), id(2)],
                area: area(0, 2, 3, 4),
            },
        ),
        (
            "wander min.y",
            t,
            p,
            Command::SetWanderArea {
                ids: vec![id(1), id(2)],
                area: area(1, 1, 3, 4),
            },
        ),
        (
            "wander max.x",
            t,
            p,
            Command::SetWanderArea {
                ids: vec![id(1), id(2)],
                area: area(1, 2, 9, 4),
            },
        ),
        (
            "wander max.y",
            t,
            p,
            Command::SetWanderArea {
                ids: vec![id(1), id(2)],
                area: area(1, 2, 3, 9),
            },
        ),
        (
            "move",
            t,
            p,
            Command::MoveTo {
                ids: vec![id(1), id(2)],
                target: Vec2::from_ints(3, 4),
            },
        ),
        (
            "move ids order",
            t,
            p,
            Command::MoveTo {
                ids: vec![id(2), id(1)],
                target: Vec2::from_ints(3, 4),
            },
        ),
        (
            "move fewer ids",
            t,
            p,
            Command::MoveTo {
                ids: vec![id(1)],
                target: Vec2::from_ints(3, 4),
            },
        ),
        (
            "move x",
            t,
            p,
            Command::MoveTo {
                ids: vec![id(1), id(2)],
                target: Vec2::from_ints(5, 4),
            },
        ),
        (
            "move y",
            t,
            p,
            Command::MoveTo {
                ids: vec![id(1), id(2)],
                target: Vec2::from_ints(3, 5),
            },
        ),
        (
            "empty wander",
            t,
            p,
            Command::SetWanderArea {
                ids: vec![],
                area: area(1, 2, 3, 4),
            },
        ),
        (
            "empty move",
            t,
            p,
            Command::MoveTo {
                ids: vec![],
                target: Vec2::from_ints(3, 4),
            },
        ),
    ]
}

#[test]
fn every_field_of_a_pending_command_changes_the_checksum() {
    let w = base();
    let reference = w.checksum();
    let mut seen = BTreeSet::new();
    seen.insert(reference);
    for (name, tick, player, command) in queue_variants() {
        let mut m = w.clone();
        m.schedule(tick, player, command).unwrap();
        let sum = m.checksum();
        assert!(seen.insert(sum), "pending command variant {name} collided");
    }
}

#[test]
fn the_execution_order_of_pending_commands_is_part_of_the_checksum() {
    let first = Command::Despawn { id: id(1) };
    let second = Command::Despawn { id: id(2) };
    let schedule = |order: [&Command; 2]| {
        let mut w = base();
        for c in order {
            w.schedule(Tick(30), PlayerId(0), c.clone()).unwrap();
        }
        w.checksum()
    };
    assert_ne!(schedule([&first, &second]), schedule([&second, &first]));

    // Different players run in player order whatever the arrival order, so
    // that arrival order is not part of the state.
    let by_player = |players: [u8; 2]| {
        let mut w = base();
        for p in players {
            w.schedule(
                Tick(30),
                PlayerId(p),
                Command::Despawn {
                    id: id(u32::from(p)),
                },
            )
            .unwrap();
        }
        w.checksum()
    };
    assert_eq!(by_player([0, 1]), by_player([1, 0]));
}

#[test]
fn the_format_tag_and_section_markers_are_distinct() {
    let markers = [SECTION_CONFIG, SECTION_SLOTS, SECTION_FREE, SECTION_QUEUE];
    let unique: BTreeSet<u8> = markers.iter().copied().collect();
    assert_eq!(unique.len(), markers.len());
    assert!(!FORMAT_TAG.is_empty());
}
