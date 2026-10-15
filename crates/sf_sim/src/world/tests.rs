//! Unit tests for the storage rules of [`World`]: ids, spawn, despawn, slot
//! reuse, configuration. Behaviour, commands and determinism are tested
//! through the public API in `tests/`.

use super::*;

fn world() -> World {
    World::new(WorldConfig::new(1)).unwrap()
}

fn spawn(w: &mut World, n: i32) -> EntityId {
    w.spawn(Kind(0), Vec2::from_ints(n, n), PlayerId(0))
        .unwrap()
}

fn slots(w: &World) -> Vec<u32> {
    w.snapshot().iter().map(|v| v.id.index()).collect()
}

#[test]
fn config_is_validated() {
    let ok = WorldConfig::new(5);
    assert!(World::new(ok).is_ok());
    assert_eq!(ok.width, 256);
    assert_eq!(ok.height, 256);
    let bad = |c: WorldConfig| World::new(c).unwrap_err();
    assert_eq!(bad(ok.with_map_size(0, 10)), ConfigError::EmptyMap);
    assert_eq!(bad(ok.with_map_size(10, 0)), ConfigError::EmptyMap);
    assert_eq!(bad(ok.with_map_size(2000, 10)), ConfigError::MapTooLarge);
    assert!(World::new(ok.with_map_size(MAX_MAP_SIDE, 1)).is_ok());
    let slow = WorldConfig {
        speed: Fx::ZERO,
        ..ok
    };
    assert_eq!(bad(slow), ConfigError::SpeedTooLow);
    let tiny = WorldConfig {
        speed: Fx::from_raw(5), // rounds to nothing per tick
        ..ok
    };
    assert_eq!(bad(tiny), ConfigError::SpeedTooLow);
    let stuck = WorldConfig {
        turn_per_tick: 0,
        ..ok
    };
    assert_eq!(bad(stuck), ConfigError::NoTurning);
    assert!(!ConfigError::EmptyMap.to_string().is_empty());
}

#[test]
fn a_new_world_is_empty_at_tick_zero() {
    let w = world();
    assert_eq!(w.tick(), Tick::ZERO);
    assert!(w.is_empty());
    assert_eq!(w.len(), 0);
    assert_eq!(w.slot_count(), 0);
    assert!(w.checksum_history().is_empty());
    assert_eq!(w.map_size(), (256, 256));
    assert!(w.snapshot().is_empty());
}

#[test]
fn ids_are_assigned_in_order_starting_at_slot_zero_generation_zero() {
    let mut w = world();
    let ids: Vec<EntityId> = (0..4).map(|n| spawn(&mut w, n)).collect();
    for (n, id) in ids.iter().enumerate() {
        assert_eq!(*id, EntityId::new(n as u32, 0));
    }
    assert_eq!(w.len(), 4);
    assert_eq!(w.slot_count(), 4);
}

#[test]
fn despawn_bumps_the_generation_and_reuses_slots_last_freed_first() {
    let mut w = world();
    let a = spawn(&mut w, 1);
    let b = spawn(&mut w, 2);
    let c = spawn(&mut w, 3);
    assert!(w.despawn(a));
    assert!(w.despawn(b));
    assert_eq!(w.free, vec![0, 1]);
    assert_eq!(w.len(), 1);
    assert_eq!(w.slot_count(), 3); // tombstones keep their place

    // The stale ids are gone for every lookup.
    assert!(!w.is_alive(a) && !w.is_alive(b));
    assert_eq!(w.get(a), None);
    assert!(w.is_alive(c));

    // Last freed (slot 1) first, with the bumped generation; then slot 0;
    // then a brand new slot at the end.
    let d = spawn(&mut w, 4);
    let e = spawn(&mut w, 5);
    let f = spawn(&mut w, 6);
    assert_eq!(d, EntityId::new(1, 1));
    assert_eq!(e, EntityId::new(0, 1));
    assert_eq!(f, EntityId::new(3, 0));
    // The old ids still do not match the entities that took their slots.
    assert!(!w.is_alive(a) && !w.is_alive(b));
    assert!(w.is_alive(d) && w.is_alive(e) && w.is_alive(f));
    assert!(w.free.is_empty());
}

#[test]
fn double_despawn_and_unknown_ids_change_nothing() {
    let mut w = world();
    let a = spawn(&mut w, 1);
    assert!(w.despawn(a));
    let before = w.clone();
    assert!(!w.despawn(a));
    assert!(!w.despawn(EntityId::new(99, 0)));
    assert!(!w.despawn(EntityId::new(0, 7)));
    assert!(w == before);
}

#[test]
fn iteration_is_by_slot_index_and_a_despawn_moves_nobody() {
    let mut w = world();
    let ids: Vec<EntityId> = (0..6).map(|n| spawn(&mut w, n)).collect();
    assert_eq!(slots(&w), vec![0, 1, 2, 3, 4, 5]);
    let before: Vec<_> = w.snapshot().iter().collect();

    w.despawn(ids[1]);
    w.despawn(ids[4]);
    // The others keep their slots, positions and relative order.
    assert_eq!(slots(&w), vec![0, 2, 3, 5]);
    let after: Vec<_> = w.snapshot().iter().collect();
    let expected: Vec<_> = before
        .iter()
        .copied()
        .filter(|v| v.id != ids[1] && v.id != ids[4])
        .collect();
    assert_eq!(after, expected);

    // New entities fill the freed slots, last freed first, so iteration is
    // by slot and not by age: the newest entity (slot 4) comes before the
    // older one in the last slot.
    spawn(&mut w, 7);
    assert_eq!(slots(&w), vec![0, 2, 3, 4, 5]);
    spawn(&mut w, 8);
    assert_eq!(slots(&w), vec![0, 1, 2, 3, 4, 5]);
}

#[test]
fn a_freed_slot_holds_nothing_of_the_old_entity() {
    let mut w = world();
    let a = spawn(&mut w, 9);
    w.entities.target[0] = Some(Vec2::from_ints(3, 3));
    w.despawn(a);
    let e = &w.entities;
    assert!(!e.alive[0]);
    assert_eq!(e.pos[0], Vec2::ZERO);
    assert_eq!(e.prev_pos[0], Vec2::ZERO);
    assert_eq!(e.target[0], None);
    assert_eq!(e.behavior[0], Behavior::Idle);
    assert_eq!(e.generation[0], 1);
}

#[test]
fn a_slot_at_the_last_generation_is_retired_not_reused() {
    let mut w = world();
    let a = spawn(&mut w, 1);
    w.entities.generation[0] = u32::MAX;
    let a = EntityId::new(a.index(), u32::MAX);
    assert!(w.is_alive(a));
    assert!(w.despawn(a));
    assert!(w.free.is_empty(), "a retired slot must not be reusable");
    assert!(!w.is_alive(a));
    assert_eq!(w.entities.generation[0], u32::MAX);
    // The next spawn gets a fresh slot, not the retired one.
    let b = spawn(&mut w, 2);
    assert_eq!(b, EntityId::new(1, 0));
    assert_eq!(w.len(), 1);
}

#[test]
fn spawn_stops_at_the_entity_limit_without_side_effects() {
    let mut w = world();
    for _ in 0..MAX_ENTITIES {
        w.entities.push_free_slot();
    }
    let before = w.clone();
    assert_eq!(w.spawn(Kind(0), Vec2::ZERO, PlayerId(0)), None);
    assert!(
        w == before,
        "a refused spawn must leave the world untouched"
    );
    // A free slot below the limit still works.
    w.free.push(17);
    assert_eq!(
        w.spawn(Kind(0), Vec2::ZERO, PlayerId(0)),
        Some(EntityId::new(17, 0))
    );
}

#[test]
fn spawn_clamps_the_position_into_the_map() {
    let mut w = World::new(WorldConfig::new(1).with_map_size(40, 30)).unwrap();
    let mut at = |x: i32, y: i32| {
        let id = w
            .spawn(Kind(0), Vec2::from_ints(x, y), PlayerId(0))
            .unwrap();
        w.get(id).unwrap().pos
    };
    let edge = Vec2::new(
        Fx::from_int(40) - Fx::EPSILON,
        Fx::from_int(30) - Fx::EPSILON,
    );
    assert_eq!(at(-5, -5), Vec2::ZERO);
    assert_eq!(at(1000, 1000), edge);
    assert_eq!(at(40, 30), edge); // the far edge itself is outside
    assert_eq!(at(12, 7), Vec2::from_ints(12, 7));
}

#[test]
fn a_new_entity_starts_without_a_previous_state_to_interpolate_from() {
    let mut w = world();
    let id = spawn(&mut w, 50);
    let v = w.get(id).unwrap();
    assert_eq!(v.prev_pos, v.pos);
    assert_eq!(v.prev_facing, v.facing);
}

#[test]
fn each_entity_has_its_own_stream_that_ignores_slots_and_neighbours() {
    let mut a = entity_rng(7, 0);
    let mut b = entity_rng(7, 1);
    let mut c = entity_rng(8, 0);
    let first = |r: &mut Rng| (r.next_u32(), r.next_u32(), r.next_u32());
    let (fa, fb, fc) = (first(&mut a), first(&mut b), first(&mut c));
    assert_ne!(fa, fb);
    assert_ne!(fa, fc);
    assert_ne!(fb, fc);

    // The stream of the second spawn is the same whatever slot it lands in
    // and whatever happened to the first entity.
    let mut w1 = world();
    let mut w2 = world();
    spawn(&mut w1, 1);
    let x = spawn(&mut w1, 2);
    let y1 = spawn(&mut w2, 1);
    w2.despawn(y1); // the second spawn of w2 reuses slot 0
    let z = spawn(&mut w2, 2);
    assert_ne!(x.index(), z.index());
    assert_eq!(
        w1.entities.rng[x.index() as usize],
        w2.entities.rng[z.index() as usize]
    );
}

#[test]
fn step_runs_commands_then_behaviour_then_records_the_checksum() {
    let mut w = world();
    w.schedule(
        Tick(0),
        PlayerId(0),
        Command::Spawn {
            kind: Kind(2),
            pos: Vec2::from_ints(5, 6),
            owner: PlayerId(3),
        },
    )
    .unwrap();
    assert_eq!(w.pending_commands().len(), 1);
    let r = w.step();
    assert_eq!(r.tick, Tick(0));
    assert_eq!(r.commands_executed, 1);
    assert_eq!(r.commands_ignored, 0);
    assert_eq!(r.checksum, w.checksum());
    assert_eq!(w.tick(), Tick(1));
    assert!(w.pending_commands().is_empty());
    let v = w.snapshot().iter().next().unwrap();
    assert_eq!((v.kind, v.owner), (Kind(2), PlayerId(3)));
    // The spawn ran before the behaviour: prev is the spawn position.
    assert_eq!(v.prev_pos, Vec2::from_ints(5, 6));
}

#[test]
fn debug_output_is_a_short_summary() {
    let mut w = world();
    for n in 0..50 {
        spawn(&mut w, n);
    }
    let text = format!("{w:?}");
    assert!(text.len() < 400, "{text}");
    assert!(text.contains("entities: 50"));
}
