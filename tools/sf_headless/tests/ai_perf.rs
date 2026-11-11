//! What one AI think costs (WP9.1a): the mean per AI per tick and the worst single think, in `ai_fog` and in
//! the same scenario with 5,000 wanderers of player 1 spread over the map (most of them in shroud, every one
//! a cell test for the observation). Budget of the design memo: mean at most 20 microseconds per AI per tick,
//! worst think at most 1 millisecond, on the CI container. The assertions are loose (ten times the budget,
//! release builds only: a debug build is far slower); the numbers are printed, and the PR reports them.
//!
//! Run: `cargo test --release -p sf_headless --test ai_perf -- --nocapture`.
//!
//! The clock is read here, around `Ai::think`, and nowhere in `sf_ai`.

mod common;

use std::time::{Duration, Instant};

use common::scenario_path;
use sf_headless::run::{RunOptions, run_full};
use sf_headless::scenario::{Rect, Scenario, SpawnGroup};

struct Cost {
    ticks: u64,
    thinks: u64,
    total: Duration,
    worst: Duration,
}

impl Cost {
    fn mean_per_tick_micros(&self) -> f64 {
        self.total.as_secs_f64() * 1e6 / self.ticks as f64
    }

    fn mean_think_micros(&self) -> f64 {
        self.total.as_secs_f64() * 1e6 / self.thinks.max(1) as f64
    }
}

/// Runs the scenario's world for `ticks` ticks like the runner does and times only the AIs' thinks.
fn measure(s: &Scenario, ticks: u64) -> Cost {
    let mut world = s.build_world().unwrap();
    let mut ais = s.ais.clone();
    let mut out = Vec::new();
    let (mut thinks, mut total, mut worst) = (0, Duration::ZERO, Duration::ZERO);
    for _ in 0..ticks {
        for ai in &mut ais {
            if ai.due(world.tick()) {
                let started = Instant::now();
                out.clear();
                ai.think(&world, &mut out);
                let spent = started.elapsed();
                thinks += 1;
                total += spent;
                worst = worst.max(spent);
            }
        }
        world.step();
    }
    Cost {
        ticks: ticks * ais.len() as u64,
        thinks,
        total,
        worst,
    }
}

fn crowded(wanderers: u32) -> Scenario {
    let mut s = Scenario::load(&scenario_path("ai_fog.ron")).unwrap();
    s.spawns.push(SpawnGroup {
        tick: 0,
        player: 1,
        count: wanderers,
        kinds: vec![0],
        owners: vec![1],
        area: Rect {
            min: (0, 0),
            max: (96, 96),
        },
    });
    s
}

fn report(label: &str, c: &Cost) {
    println!(
        "ai think cost, {label}: {} thinks in {} AI ticks, mean {:.2} us per AI per tick, {:.1} us per think, worst think {:.1} us",
        c.thinks,
        c.ticks,
        c.mean_per_tick_micros(),
        c.mean_think_micros(),
        c.worst.as_secs_f64() * 1e6
    );
}

#[test]
fn the_think_cost_is_measured_in_ai_fog_and_in_a_crowd() {
    let ticks = if cfg!(debug_assertions) { 200 } else { 800 };
    let small = measure(
        &Scenario::load(&scenario_path("ai_fog.ron")).unwrap(),
        ticks,
    );
    report("ai_fog (9 entities)", &small);
    let big = measure(&crowded(5000), ticks);
    report("ai_fog + 5000 wanderers", &big);
    assert!(small.thinks > 0 && big.thinks > 0);
    if !cfg!(debug_assertions) {
        // Ten times the memo's budget: the container is noisy and shared.
        for c in [&small, &big] {
            assert!(
                c.mean_per_tick_micros() <= 200.0,
                "mean {:.1} us per AI per tick",
                c.mean_per_tick_micros()
            );
            assert!(
                c.worst <= Duration::from_millis(10),
                "worst think {:?}",
                c.worst
            );
        }
    }
}

/// The wall time of a whole run (`run_full`, the runner's own loop) of `ticks` ticks.
fn timed_run(s: &Scenario, ticks: u64) -> Duration {
    let started = Instant::now();
    let finished = run_full(s, &RunOptions { ticks, every: 100 }).unwrap();
    let spent = started.elapsed();
    assert_eq!(finished.outcome.ticks, ticks);
    spent
}

#[test]
fn a_whole_run_with_the_ai_against_the_same_run_without_it() {
    let ticks = if cfg!(debug_assertions) { 200 } else { 800 };
    let with = crowded(5000);
    let mut without = crowded(5000);
    without.ais.clear();
    // Alternating, three pairs; the median of each.
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for _ in 0..3 {
        a.push(timed_run(&with, ticks));
        b.push(timed_run(&without, ticks));
    }
    a.sort();
    b.sort();
    let per_second = |d: Duration| ticks as f64 / d.as_secs_f64();
    println!(
        "whole run, ai_fog + 5000 wanderers, {ticks} ticks: with the AI {:.0} ticks/s, without {:.0} ticks/s (medians of 3 alternating runs)",
        per_second(a[1]),
        per_second(b[1])
    );
    if !cfg!(debug_assertions) {
        assert!(
            a[1] <= b[1] + b[1] / 2,
            "with the AI {:?}, without {:?}",
            a[1],
            b[1]
        );
    }
}
