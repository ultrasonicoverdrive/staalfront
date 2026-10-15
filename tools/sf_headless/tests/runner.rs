//! Running the shipped scenario files through the library.

mod common;

use common::{deterministic_part, scenario_path};
use sf_headless::log::{self, Host};
use sf_headless::run::{Outcome, RunOptions, run};
use sf_headless::scenario::Scenario;
use std::time::Duration;

fn load(name: &str) -> Scenario {
    Scenario::load(&scenario_path(name)).unwrap()
}

fn log_of(outcome: &Outcome, elapsed_ms: u64) -> String {
    log::render(outcome, &Host::current(Duration::from_millis(elapsed_ms)))
}

#[test]
fn running_a_scenario_twice_gives_identical_checksums() {
    let scenario = load("command_mix.ron");
    let options = RunOptions {
        ticks: scenario.ticks,
        every: 50,
    };
    let first = run(&scenario, &options).unwrap();
    let second = run(&scenario, &options).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.samples.len(), 30);

    // The logs differ only where wall time and machine are recorded.
    let (a, b) = (log_of(&first, 12), log_of(&second, 345));
    assert_ne!(a, b);
    assert_eq!(deterministic_part(&a), deterministic_part(&b));
}

#[test]
fn a_fresh_load_of_the_file_runs_the_same() {
    let options = RunOptions {
        ticks: 300,
        every: 100,
    };
    let a = run(&load("command_mix.ron"), &options).unwrap();
    let b = run(&load("command_mix.ron"), &options).unwrap();
    assert_eq!(a, b);
}

#[test]
fn a_scheduled_command_changes_the_checksum() {
    let with = load("command_mix.ron");
    let mut without = with.clone();
    without.commands.clear();
    assert!(!with.commands.is_empty());

    let options = RunOptions::for_scenario(&with);
    let a = run(&with, &options).unwrap();
    let b = run(&without, &options).unwrap();
    assert_ne!(a.final_checksum, b.final_checksum);
    assert_ne!(a.initial_checksum, b.initial_checksum);
    assert!(a.commands_executed > b.commands_executed);

    // A single command is enough: keep only the first, which runs at tick 75.
    let mut one = without.clone();
    one.commands.push(with.commands[0].clone());
    let c = run(&one, &options).unwrap();
    assert_ne!(c.final_checksum, b.final_checksum);
    // Every checksum from the start differs from the run without the command:
    // the pending command is part of the checksummed state, and after tick 75
    // the entities have walked elsewhere.
    for (x, y) in c.samples.iter().zip(&b.samples) {
        assert_ne!(x.checksum, y.checksum, "after {} ticks", x.ticks);
    }
}

#[test]
fn the_command_mix_exercises_the_queue() {
    let o = run(
        &load("command_mix.ron"),
        &RunOptions {
            ticks: 1_500,
            every: 1_500,
        },
    )
    .unwrap();
    // Spawns, four kinds of commands, a reused slot and one command whose ids
    // are all stale.
    assert_eq!(o.commands_scheduled, 429);
    assert_eq!(o.commands_ignored, 1);
    assert_eq!(o.commands_executed, 428);
    assert_eq!(o.entities, 400 + 12 - 11 - 1);
}

#[test]
fn the_ticks_override_stops_earlier_and_keeps_later_commands_pending() {
    let scenario = load("command_mix.ron");
    let short = run(
        &scenario,
        &RunOptions {
            ticks: 100,
            every: 100,
        },
    )
    .unwrap();
    assert_eq!(short.ticks, 100);
    assert_eq!(short.samples.len(), 1);
    // Only the first group and the tick-75 command have run.
    assert_eq!(short.commands_executed, 400 + 1);
    assert_eq!(short.entities, 400);
    // The checksum after 100 ticks is the same as the one recorded at tick 100
    // of the full run.
    let full = run(
        &scenario,
        &RunOptions {
            ticks: 1_500,
            every: 100,
        },
    )
    .unwrap();
    assert_eq!(full.samples[0], short.samples[0]);
}
