use super::*;

fn crowd(extra: &str) -> Scenario {
    Scenario::from_ron_str(&format!(
        "Scenario(name: \"unit\", seed: 11, map: (width: 64, height: 64), ticks: 30, \
         spawns: [(count: 40, kinds: [0, 1], owners: [0, 1, 2], \
         area: (min: (0, 0), max: (64, 64)))]{extra})"
    ))
    .unwrap()
}

fn opts(ticks: u64, every: u64) -> RunOptions {
    RunOptions { ticks, every }
}

fn sample_ticks(o: &Outcome) -> Vec<u64> {
    o.samples.iter().map(|s| s.ticks).collect()
}

#[test]
fn checksums_are_recorded_every_k_ticks_and_at_the_end() {
    let s = crowd("");
    assert_eq!(sample_ticks(&run(&s, &opts(10, 3)).unwrap()), [3, 6, 9, 10]);
    // A multiple of the interval is not recorded twice.
    assert_eq!(sample_ticks(&run(&s, &opts(9, 3)).unwrap()), [3, 6, 9]);
    // An interval longer than the run still records the end.
    assert_eq!(sample_ticks(&run(&s, &opts(5, 100)).unwrap()), [5]);
    assert_eq!(sample_ticks(&run(&s, &opts(4, 1)).unwrap()), [1, 2, 3, 4]);
}

#[test]
fn the_samples_are_the_checksum_history_of_the_world() {
    let s = crowd("");
    let outcome = run(&s, &opts(25, 5)).unwrap();
    let mut world = s.build_world().unwrap();
    for _ in 0..25 {
        world.step();
    }
    for sample in &outcome.samples {
        assert_eq!(
            Some(sample.checksum),
            world.checksum_after(sf_sim::Tick(sample.ticks - 1)),
            "after {} ticks",
            sample.ticks
        );
    }
    assert_eq!(outcome.final_checksum, world.checksum());
    assert_eq!(
        outcome.samples.last().unwrap().checksum,
        outcome.final_checksum
    );
    assert_eq!(outcome.entities, 40);
}

#[test]
fn the_initial_checksum_is_the_world_before_any_tick() {
    let s = crowd("");
    let outcome = run(&s, &opts(3, 1)).unwrap();
    assert_eq!(
        outcome.initial_checksum,
        s.build_world().unwrap().checksum()
    );
    assert_ne!(outcome.initial_checksum, outcome.final_checksum);
}

#[test]
fn a_run_is_repeatable() {
    let s = crowd("");
    assert_eq!(
        run(&s, &opts(30, 10)).unwrap(),
        run(&s, &opts(30, 10)).unwrap()
    );
}

#[test]
fn the_interval_does_not_change_the_simulation() {
    let s = crowd("");
    let a = run(&s, &opts(30, 1)).unwrap();
    let b = run(&s, &opts(30, 7)).unwrap();
    assert_eq!(a.final_checksum, b.final_checksum);
    assert_eq!(a.initial_checksum, b.initial_checksum);
    // The samples both runs have are the same.
    for sb in &b.samples {
        assert!(a.samples.contains(sb));
    }
}

#[test]
fn a_different_seed_gives_a_different_world() {
    let a = crowd("");
    let mut b = crowd("");
    b.seed += 1;
    assert_ne!(
        run(&a, &opts(5, 5)).unwrap().final_checksum,
        run(&b, &opts(5, 5)).unwrap().final_checksum
    );
}

#[test]
fn counts_commands() {
    let s = crowd(
        ", commands: [(tick: 3, command: MoveTo(ids: [Range(0, 5)], target: (10, 10))), \
         (tick: 4, command: Despawn(ids: [Id(900, 0)])), \
         (tick: 6, command: Despawn(ids: [Id(0, 0), Id(1, 0)]))]",
    );
    let o = run(&s, &opts(30, 30)).unwrap();
    // 40 spawns, a MoveTo, one Despawn of slot 900 and a Despawn of two ids
    // (which is one sf_sim command per id).
    assert_eq!(o.commands_scheduled, 44);
    // The Despawn of slot 900 hits nothing and counts as ignored.
    assert_eq!(o.commands_executed, 43);
    assert_eq!(o.commands_ignored, 1);
    assert_eq!(o.entities, 38);
}

#[test]
fn a_timed_command_changes_the_checksum() {
    let plain = crowd("");
    let commanded =
        crowd(", commands: [(tick: 10, command: MoveTo(ids: [Range(0, 20)], target: (5, 5)))]");
    let a = run(&plain, &opts(30, 10)).unwrap();
    let b = run(&commanded, &opts(30, 10)).unwrap();
    assert_ne!(a.final_checksum, b.final_checksum);
    // The pending command is part of the state from the start ...
    assert_ne!(a.initial_checksum, b.initial_checksum);
    // ... and the difference stays: the entities really walked elsewhere.
    for (x, y) in a.samples.iter().zip(&b.samples) {
        assert_ne!(x.checksum, y.checksum, "after {} ticks", x.ticks);
    }
}

#[test]
fn a_command_after_a_shortened_run_stays_pending() {
    let s = crowd(", commands: [(tick: 20, command: Despawn(ids: [Range(0, 10)]))]");
    let short = run(&s, &opts(10, 10)).unwrap();
    assert_eq!(short.entities, 40);
    assert_eq!(short.commands_executed, 40);
    assert_eq!(short.commands_scheduled, 50);
    let full = run(&s, &opts(30, 10)).unwrap();
    assert_eq!(full.entities, 30);
    assert_eq!(full.commands_executed, 50);
}

#[test]
fn a_longer_run_than_the_file_is_fine() {
    let s = crowd("");
    let o = run(&s, &opts(45, 20)).unwrap();
    assert_eq!(o.ticks, 45);
    assert_eq!(sample_ticks(&o), [20, 40, 45]);
}

#[test]
fn bad_options_are_refused() {
    let s = crowd("");
    for bad in [opts(0, 10), opts(10, 0)] {
        match run(&s, &bad) {
            Err(ScenarioError::Invalid { problems, .. }) => assert_eq!(problems.len(), 1),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
    assert!(matches!(
        run(&s, &opts(0, 0)),
        Err(ScenarioError::Invalid { problems, .. }) if problems.len() == 2
    ));
}

#[test]
fn default_options_follow_the_scenario() {
    let s = crowd("");
    assert_eq!(RunOptions::for_scenario(&s), opts(30, DEFAULT_EVERY));
}
