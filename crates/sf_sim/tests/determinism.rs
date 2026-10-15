//! Determinism tests: the same seed and the same commands must give the same
//! state on every tick, and any change to either must show in the checksum.
//!
//! The full-size run (5000 wanderers, 10000 ticks) takes minutes in a debug
//! build, so debug builds run the small scenario of `common` and CI also runs
//! `cargo test --release -p sf_sim`, which runs the full one on Linux and
//! Windows.

mod common;

use common::{Scenario, run, script, world_with, world_with_commands};
use sf_math::Vec2;
use sf_sim::{Command, Kind, PlayerId, Tick, World, WorldConfig};

#[test]
fn two_worlds_agree_on_every_tick() {
    let s = Scenario::for_profile();
    let mut a = world_with(&s);
    let mut b = world_with(&s);
    for tick in 0..s.ticks {
        let ra = a.step();
        let rb = b.step();
        assert_eq!(ra, rb, "step reports differ at tick {tick}");
    }
    assert_eq!(a.checksum_history(), b.checksum_history());
    assert_eq!(a.checksum_history().len() as u64, s.ticks);
    // Not only the checksum: the whole state, including the interpolation
    // columns the checksum leaves out.
    assert!(a == b, "worlds differ although every checksum matched");
    // The scenario really ran: the crowd is still there and the queue empty.
    assert!(a.len() >= s.entities * 9 / 10);
    assert!(a.pending_commands().is_empty());
}

#[test]
fn checksum_history_matches_the_reports_and_the_live_checksum() {
    let s = Scenario::small();
    let mut w = world_with(&s);
    let mut seen = Vec::new();
    for _ in 0..200 {
        let r = w.step();
        assert_eq!(r.checksum, w.checksum());
        seen.push(r.checksum);
    }
    assert_eq!(w.checksum_history(), seen.as_slice());
    assert_eq!(w.checksum_after(Tick(0)), Some(seen[0]));
    assert_eq!(w.checksum_after(Tick(199)), Some(seen[199]));
    assert_eq!(w.checksum_after(Tick(200)), None);
    assert_eq!(w.tick(), Tick(200));
}

#[test]
fn a_different_seed_changes_the_checksum_from_the_first_tick() {
    let s = Scenario::small();
    // The same script on both worlds: only the world seed differs.
    let cmds = script(&s);
    let mut a = world_with_commands(&s, &cmds);
    let mut b = World::new(WorldConfig::new(s.seed ^ 1)).unwrap();
    for (tick, player, cmd) in &cmds {
        b.schedule(*tick, *player, cmd.clone()).unwrap();
    }
    let ha = run(&mut a, 300);
    let hb = run(&mut b, 300);
    assert_ne!(ha[0], hb[0]);
    for (k, (x, y)) in ha.iter().zip(&hb).enumerate() {
        assert_ne!(x, y, "tick {k} agrees although the seeds differ");
    }
}

#[test]
fn one_extra_command_changes_the_checksum_from_that_tick_on() {
    let s = Scenario::small();
    let mut cmds = script(&s);
    let mut a = world_with_commands(&s, &cmds);
    // One more spawn, at tick 40.
    cmds.push((
        Tick(40),
        PlayerId(2),
        Command::Spawn {
            kind: Kind(1),
            pos: Vec2::from_ints(3, 4),
            owner: PlayerId(1),
        },
    ));
    let mut b = world_with_commands(&s, &cmds);
    let ha = run(&mut a, 120);
    let hb = run(&mut b, 120);
    // Identical until the command's tick; the pending command already shows
    // in the checksum, so the histories part even before it runs.
    for k in 0..120usize {
        assert_ne!(
            ha[k], hb[k],
            "tick {k}: an extra pending or executed command was not seen"
        );
    }
}

#[test]
fn a_command_that_changes_nothing_still_shows_while_pending_and_not_after() {
    // A despawn of an id that does not exist is ignored when it runs; the
    // worlds re-converge only if it left no trace, and until then the pending
    // command is part of the state.
    let s = Scenario::small();
    let cmds = script(&s);
    let mut a = world_with_commands(&s, &cmds);
    let mut with_noop = cmds.clone();
    with_noop.push((
        Tick(30),
        PlayerId(3),
        Command::Despawn {
            id: sf_sim::EntityId::new(100_000, 7),
        },
    ));
    let mut b = world_with_commands(&s, &with_noop);
    let ha = run(&mut a, 60);
    let hb = run(&mut b, 60);
    for k in 0..30 {
        assert_ne!(ha[k], hb[k], "pending no-op invisible at tick {k}");
    }
    for k in 30..60 {
        assert_eq!(ha[k], hb[k], "ignored command left a trace at tick {k}");
    }
}

#[test]
fn the_order_commands_arrive_in_does_not_matter_between_players() {
    // Lockstep peers receive each other's commands in different orders. What
    // is fixed is each player's own order, so scheduling the same commands
    // grouped by player (descending) instead of by script order must give the
    // same state.
    let s = Scenario::small();
    let cmds = script(&s);
    let mut regrouped = cmds.clone();
    regrouped.sort_by_key(|(_, player, _)| std::cmp::Reverse(*player));
    assert_ne!(
        cmds.iter().map(|c| c.1).collect::<Vec<_>>(),
        regrouped.iter().map(|c| c.1).collect::<Vec<_>>(),
        "the regrouping must actually reorder something"
    );
    let mut a = world_with_commands(&s, &cmds);
    let mut b = world_with_commands(&s, &regrouped);
    assert_eq!(run(&mut a, 400), run(&mut b, 400));
    assert!(a == b);
}

#[test]
fn a_clone_mid_run_continues_identically() {
    let s = Scenario::small();
    let mut a = world_with(&s);
    run(&mut a, 500);
    let mut b = a.clone();
    assert!(a == b);
    let ha = run(&mut a, 500);
    let hb = run(&mut b, 500);
    assert_eq!(ha, hb);
    assert!(a == b);
}

#[test]
fn worlds_that_differ_only_in_when_they_were_stepped_agree() {
    // No hidden clock: stepping in bursts or one by one is the same thing.
    let s = Scenario::small();
    let mut a = world_with(&s);
    let mut b = world_with(&s);
    for _ in 0..10 {
        for _ in 0..50 {
            a.step();
        }
    }
    run(&mut b, 500);
    assert_eq!(a.checksum_history(), b.checksum_history());
}
