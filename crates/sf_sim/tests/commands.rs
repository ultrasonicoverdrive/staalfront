//! Command scheduling and execution through the public API: order, timing,
//! rejection of past ticks, and what happens to ids that went stale.

use sf_math::Vec2;
use sf_sim::{
    Area, Command, EntityId, EntityView, Kind, PlayerId, ScheduleError, Tick, World, WorldConfig,
};

fn world() -> World {
    World::new(WorldConfig::new(3)).unwrap()
}

fn spawn(kind: u16, x: i32, y: i32) -> Command {
    Command::Spawn {
        kind: Kind(kind),
        pos: Vec2::from_ints(x, y),
        owner: PlayerId(0),
    }
}

fn at(w: &mut World, tick: u64, player: u8, command: Command) {
    w.schedule(Tick(tick), PlayerId(player), command).unwrap();
}

fn views(w: &World) -> Vec<EntityView> {
    w.snapshot().iter().collect()
}

fn kinds(w: &World) -> Vec<u16> {
    views(w).iter().map(|v| v.kind.0).collect()
}

#[test]
fn commands_run_in_player_order_whatever_order_they_were_scheduled_in() {
    let mut w = world();
    at(&mut w, 0, 2, spawn(2, 20, 20));
    at(&mut w, 0, 0, spawn(0, 10, 10));
    at(&mut w, 0, 1, spawn(1, 15, 15));
    let r = w.step();
    assert_eq!(r.commands_executed, 3);
    // Slots are assigned in execution order: player 0, then 1, then 2.
    assert_eq!(kinds(&w), vec![0, 1, 2]);
    assert_eq!(views(&w)[2].id, EntityId::new(2, 0));
}

#[test]
fn one_player_keeps_the_order_it_scheduled_in() {
    let mut w = world();
    for kind in [5, 6, 7, 8] {
        at(&mut w, 0, 1, spawn(kind, 10, 10));
    }
    w.step();
    assert_eq!(kinds(&w), vec![5, 6, 7, 8]);
}

#[test]
fn a_command_runs_at_the_start_of_its_own_tick_and_not_before() {
    let mut w = world();
    at(&mut w, 3, 0, spawn(1, 50, 50));
    for tick in 0..3 {
        let r = w.step();
        assert_eq!(r.tick, Tick(tick));
        assert_eq!(r.commands_executed, 0);
        assert!(w.is_empty(), "spawned before its tick {tick}");
        assert_eq!(w.pending_commands().len(), 1);
    }
    let r = w.step();
    assert_eq!(r.tick, Tick(3));
    assert_eq!(r.commands_executed, 1);
    assert_eq!(w.len(), 1);
    assert!(w.pending_commands().is_empty());
    // It ran before the behaviours of the same tick: the entity has already
    // had its first behaviour step, starting from the spawn position.
    assert_eq!(views(&w)[0].prev_pos, Vec2::from_ints(50, 50));
}

#[test]
fn past_ticks_are_rejected_and_change_nothing() {
    let mut w = world();
    for _ in 0..5 {
        w.step();
    }
    assert_eq!(w.tick(), Tick(5));
    let before = w.clone();

    let err = w
        .schedule(Tick(4), PlayerId(0), spawn(1, 1, 1))
        .unwrap_err();
    assert_eq!(
        err,
        ScheduleError::PastTick {
            scheduled: Tick(4),
            current: Tick(5)
        }
    );
    assert!(err.to_string().contains("tick 4"));
    assert!(w.schedule(Tick(0), PlayerId(0), spawn(1, 1, 1)).is_err());
    assert!(w == before, "a rejected command must leave no trace");

    // The tick that is about to run, and any later one, are accepted.
    w.schedule(Tick(5), PlayerId(0), spawn(1, 1, 1)).unwrap();
    w.schedule(Tick(900), PlayerId(0), spawn(1, 1, 1)).unwrap();
    assert_eq!(w.pending_commands().len(), 2);
}

#[test]
fn a_tick_that_just_ran_can_no_longer_be_scheduled() {
    let mut w = world();
    at(&mut w, 0, 0, spawn(1, 1, 1));
    w.step();
    assert!(matches!(
        w.schedule(Tick(0), PlayerId(0), spawn(2, 2, 2)),
        Err(ScheduleError::PastTick { .. })
    ));
    assert!(w.schedule(Tick(1), PlayerId(0), spawn(2, 2, 2)).is_ok());
}

#[test]
fn stale_ids_are_skipped_and_a_command_with_no_effect_is_counted_as_ignored() {
    let mut w = world();
    for kind in 0..3 {
        at(&mut w, 0, 0, spawn(kind, 30, 30));
    }
    w.step();
    let ids: Vec<EntityId> = views(&w).iter().map(|v| v.id).collect();

    at(&mut w, 1, 0, Command::Despawn { id: ids[1] });
    let r = w.step();
    assert_eq!((r.commands_executed, r.commands_ignored), (1, 0));
    assert_eq!(w.len(), 2);

    at(&mut w, 2, 0, Command::Despawn { id: ids[1] }); // already gone
    at(
        &mut w,
        2,
        0,
        Command::MoveTo {
            ids: vec![ids[1]],
            target: Vec2::from_ints(5, 5),
        },
    ); // only stale ids
    at(
        &mut w,
        2,
        0,
        Command::MoveTo {
            ids: vec![ids[1], ids[0]],
            target: Vec2::from_ints(5, 5),
        },
    ); // one stale, one live: runs for the live one
    at(
        &mut w,
        2,
        0,
        Command::SetWanderArea {
            ids: vec![],
            area: Area::from_cells(0, 0, 4, 4),
        },
    ); // nobody to command
    let r = w.step();
    assert_eq!((r.commands_executed, r.commands_ignored), (1, 3));
    assert_eq!(w.len(), 2);
}

#[test]
fn a_reused_slot_does_not_obey_orders_meant_for_its_predecessor() {
    let mut w = world();
    at(&mut w, 0, 0, spawn(1, 100, 100));
    w.step();
    let old = views(&w)[0].id;

    // The old entity goes and a new one takes over its slot, in one tick.
    at(&mut w, 1, 0, Command::Despawn { id: old });
    at(&mut w, 1, 0, spawn(2, 40, 40));
    w.step();
    let new = views(&w)[0].id;
    assert_eq!(new.index(), old.index());
    assert_ne!(new.generation(), old.generation());
    assert!(!w.is_alive(old));
    assert_eq!(w.get(new).unwrap().kind, Kind(2));

    // An order addressed to the old id finds nobody.
    at(
        &mut w,
        2,
        0,
        Command::MoveTo {
            ids: vec![old],
            target: Vec2::from_ints(1, 1),
        },
    );
    let r = w.step();
    assert_eq!(r.commands_ignored, 1);
    // An order addressed to the new id works.
    at(
        &mut w,
        3,
        0,
        Command::MoveTo {
            ids: vec![new],
            target: Vec2::from_ints(1, 1),
        },
    );
    assert_eq!(w.step().commands_executed, 1);
}

#[test]
fn a_despawned_entity_does_not_take_part_in_the_tick_it_is_removed_in() {
    let mut w = world();
    at(&mut w, 0, 0, spawn(1, 100, 100));
    for _ in 0..30 {
        w.step();
    }
    let id = views(&w)[0].id;
    at(&mut w, 30, 0, Command::Despawn { id });
    w.step();
    assert!(w.is_empty());
    assert!(w.snapshot().iter().next().is_none());
    assert_eq!(w.get(id), None);
}

#[test]
fn naming_an_entity_twice_in_one_command_is_harmless() {
    let mut w = world();
    at(&mut w, 0, 0, spawn(1, 100, 100));
    w.step();
    let id = views(&w)[0].id;
    at(
        &mut w,
        1,
        0,
        Command::MoveTo {
            ids: vec![id, id, id],
            target: Vec2::from_ints(7, 7),
        },
    );
    assert_eq!(w.step().commands_executed, 1);
    assert_eq!(w.len(), 1);
}
