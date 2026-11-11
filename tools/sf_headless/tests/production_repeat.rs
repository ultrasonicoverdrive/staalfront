//! Repeat (WP6.10) on the playfield: the `production_repeat` scenario with its checksum pinned. See
//! `assets/scenarios/production_repeat.ron` and `docs/rules/production.md`.
//!
//! Slot 1 is the barracks (rifleman: 100 credits over 100 ticks), repeat is on from tick 1 to tick
//! 1500 with two riflemen in the queue, and player 0 starts with 150 credits, so the factory runs
//! out of money at once and goes on as the harvesters unload.

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::run::{RunOptions, run};
use sf_headless::scenario::Scenario;
use sf_sim::{EntityId, FactoryStatus};

const EXPECTED: &str = include_str!("../../../assets/scenarios/production_repeat.expected");

const BARRACKS: EntityId = EntityId::new(1, 0);
const OFF_AT: u64 = 1500;

/// Per tick, after the step: the queue length, the status, whether it repeats and the riflemen out
/// so far.
struct Row {
    queue: usize,
    status: FactoryStatus,
    repeat: bool,
    out: usize,
}

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("production_repeat.ron")).unwrap()
}

fn rows() -> Vec<Row> {
    let s = scenario();
    let mut w = s.build_world().unwrap();
    let mut out = 0;
    let mut rows = Vec::new();
    for _ in 0..s.ticks {
        w.step();
        out += w.produced().len();
        let v = w.production_of(BARRACKS);
        rows.push(Row {
            queue: v.as_ref().map_or(0, |v| v.queue.len()),
            status: v.as_ref().map_or(FactoryStatus::Idle, |v| v.status),
            repeat: v.as_ref().is_some_and(|v| v.repeat),
            out,
        });
    }
    rows
}

/// The row after the step that ran tick `t - 1`, i.e. the world at tick `t`.
fn at(rows: &[Row], tick: u64) -> &Row {
    &rows[(tick - 1) as usize]
}

#[test]
fn the_final_checksum_is_the_pinned_value() {
    let expected = parse_checksum(EXPECTED).unwrap();
    let s = scenario();
    let outcome = run(&s, &RunOptions::for_scenario(&s)).unwrap();
    assert_eq!(outcome.final_checksum, expected);
    // Every command of the file was taken: nothing was refused.
    assert_eq!(outcome.commands_ignored, 0);
}

#[test]
fn while_repeat_is_on_the_queue_stays_at_two_and_the_factory_waits_for_money_and_goes_on() {
    let rows = rows();
    assert!(
        !at(&rows, 1).repeat,
        "the command of tick 1 runs in that step"
    );
    for t in 3..OFF_AT {
        let r = at(&rows, t);
        assert!(r.repeat, "tick {t}");
        assert_eq!(r.queue, 2, "tick {t}");
    }
    let waiting = (3..OFF_AT)
        .filter(|&t| at(&rows, t).status == FactoryStatus::WaitingForMoney)
        .count();
    assert!(waiting > 0, "it ran out of money at some time");
    // The riflemen: the 150 credits are one and a half, so the rest comes from the ore.
    assert_eq!(at(&rows, OFF_AT - 1).out, 5);
    // The harvesters feed it: more came out in the second half than the start money buys.
    assert!(at(&rows, OFF_AT - 1).out > at(&rows, 750).out);
}

#[test]
fn after_repeat_is_off_the_queue_runs_out_and_nothing_more_comes() {
    let rows = rows();
    assert!(!at(&rows, OFF_AT + 1).repeat);
    let at_off = at(&rows, OFF_AT).out;
    let last = rows.last().unwrap();
    assert_eq!(last.queue, 0);
    assert_eq!(last.status, FactoryStatus::Idle);
    // The two that were in the queue came out, and no third.
    assert_eq!(last.out, at_off + 2);
}
