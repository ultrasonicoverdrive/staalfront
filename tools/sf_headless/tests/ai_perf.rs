//! What one AI think costs (WP9.1a): the mean per AI per tick and the worst single think, in `ai_fog` and in
//! the same scenario with 5,000 wanderers of player 1 spread over the map (most of them in shroud, every one
//! a cell test for the observation). WP9.1b-1 adds the scenarios where the AI builds, `ai_base` and the
//! boxed-in `ai_boxed`, alone and with the same crowd, and counts the placement questions per think (each
//! walks every entity of the world twice in the simulation). Budget of the design memo: mean at most 20 microseconds per AI per tick,
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
    /// Placement questions in all, and the most in one think.
    previews: u64,
    worst_previews: u64,
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
    let mut worst_previews = 0;
    for _ in 0..ticks {
        for ai in &mut ais {
            if ai.due(world.tick()) {
                let asked = ai.state().previews;
                let started = Instant::now();
                out.clear();
                ai.think(&world, &mut out);
                let spent = started.elapsed();
                thinks += 1;
                total += spent;
                worst = worst.max(spent);
                worst_previews = worst_previews.max(ai.state().previews - asked);
                for command in out.drain(..) {
                    world.schedule(world.tick(), ai.player(), command).unwrap();
                }
            }
        }
        world.step();
    }
    Cost {
        ticks: ticks * ais.len() as u64,
        thinks,
        total,
        worst,
        previews: ais.iter().map(|a| a.state().previews).sum(),
        worst_previews,
    }
}

fn crowded(wanderers: u32) -> Scenario {
    crowded_in("ai_fog.ron", wanderers)
}

fn crowded_in(name: &str, wanderers: u32) -> Scenario {
    let mut s = Scenario::load(&scenario_path(name)).unwrap();
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
        "ai think cost, {label}: {} thinks in {} AI ticks, mean {:.2} us per AI per tick, {:.1} us per think, worst think {:.1} us, {} placement questions ({:.3} per think, at most {} in one)",
        c.thinks,
        c.ticks,
        c.mean_per_tick_micros(),
        c.mean_think_micros(),
        c.worst.as_secs_f64() * 1e6,
        c.previews,
        c.previews as f64 / c.thinks.max(1) as f64,
        c.worst_previews
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
    // WP9.1b-1: the AI builds. Long enough for the whole goal list of `ai_base` (it stands at tick 6518).
    let long = if cfg!(debug_assertions) { 400 } else { 7000 };
    let mut costs = vec![small, big];
    for name in ["ai_base.ron", "ai_boxed.ron"] {
        let alone = measure(&Scenario::load(&scenario_path(name)).unwrap(), long);
        report(name, &alone);
        let crowd = measure(&crowded_in(name, 5000), long);
        report(&format!("{name} + 5000 wanderers"), &crowd);
        assert!(alone.previews > 0 && crowd.previews > 0, "{name}");
        costs.push(alone);
        costs.push(crowd);
    }
    assert!(costs.iter().all(|c| c.thinks > 0));
    if !cfg!(debug_assertions) {
        // Ten times the memo's budget: the container is noisy and shared.
        for c in &costs {
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

#[test]
fn a_think_whose_every_placement_question_fails() {
    // The worst think of the construction manager, measured: the observation and a search in which every
    // question the cap allows is asked and refused, on `ai_base` with 5,000 wanderers on the map. The vehicle
    // factory needs a refinery, which does not stand at tick 1, so the simulation refuses every place
    // (`MissingPrerequisite`, after walking every entity like any other answer); the AI's own turn never
    // asks for it then, the search is called directly. Five runs per cap; the slowest is printed.
    let s = crowded_in("ai_base.ron", 5000);
    let mut world = s.build_world().unwrap();
    world.step();
    let player = sf_sim::PlayerId(0);
    for cap in [32u32, 64, 128, 256] {
        let mut slowest = Duration::ZERO;
        let mut asked = 0;
        for _ in 0..5 {
            let mut state = s.ais[0].state().clone();
            state.profile.economy.placement_cap = cap;
            let started = Instant::now();
            let mut obs = sf_ai::Observation::build(&world, player, &state.known_structures);
            // The wanderers would shut most places out of the mask; without them every place gets asked.
            obs.visible.clear();
            let mut lens = sf_ai::Lens::new(&world, player);
            let found = sf_ai::placement::search(&mut state, &obs, &mut lens, sf_sim::Kind(3));
            slowest = slowest.max(started.elapsed());
            assert_eq!(found, None);
            asked = lens.previews();
        }
        assert_eq!(asked, cap, "every question is asked");
        println!(
            "a think with cap {cap}: {asked} failing placement questions, {} entities: slowest of 5 {:.0} us",
            world.len(),
            slowest.as_secs_f64() * 1e6
        );
    }
}

#[test]
fn what_one_placement_question_costs_in_a_crowd() {
    // The cost a full `placement_cap` would have, measured instead of estimated: the AI's own lens asked
    // 256 times on visible ground of `ai_base` with 5,000 wanderers on the map (each question walks every
    // entity twice in the simulation). The AI's mask normally makes the first question the last.
    let s = crowded_in("ai_base.ron", 5000);
    let mut world = s.build_world().unwrap();
    world.step();
    let player = sf_sim::PlayerId(0);
    let mut lens = sf_ai::Lens::new(&world, player);
    let started = Instant::now();
    for i in 0..256u32 {
        let _ = lens.placement(sf_sim::Kind(1), sf_sim::Cell::new(8 + i % 2, 66 + i % 3));
    }
    let spent = started.elapsed();
    assert_eq!(lens.previews(), 256);
    println!(
        "256 placement questions with {} entities: {:.0} us, {:.2} us each",
        world.len(),
        spent.as_secs_f64() * 1e6,
        spent.as_secs_f64() * 1e6 / 256.0
    );
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
