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
    /// The manager whose turn the worst think was (WP9.1b-2: so a spike can be put on its owner).
    worst_manager: Option<sf_ai::Manager>,
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
    let mut worst_manager = None;
    for _ in 0..ticks {
        for ai in &mut ais {
            if ai.due(world.tick()) {
                let asked = ai.state().previews;
                let manager = sf_ai::Manager::of_think(ai.state().thinks);
                let started = Instant::now();
                out.clear();
                ai.think(&world, &mut out);
                let spent = started.elapsed();
                thinks += 1;
                total += spent;
                if spent > worst {
                    worst = spent;
                    worst_manager = Some(manager);
                }
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
        worst_manager,
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
        "ai think cost, {label}: {} thinks in {} AI ticks, mean {:.2} us per AI per tick, {:.1} us per think, worst think {:.1} us (manager {:?}), {} placement questions ({:.3} per think, at most {} in one)",
        c.thinks,
        c.ticks,
        c.mean_per_tick_micros(),
        c.mean_think_micros(),
        c.worst.as_secs_f64() * 1e6,
        c.worst_manager,
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
    // WP9.1b-1: the AI builds. Long enough for the whole goal list of `ai_base` (it stands at tick 8078 since
    // WP9.1b-2, when the economy buys two harvesters).
    let long = if cfg!(debug_assertions) { 400 } else { 9000 };
    let mut costs = vec![small, big];
    // WP9.1b-2: and the economy turn at work, with the harvesters, the silo and a repair.
    for name in [
        "ai_base.ron",
        "ai_boxed.ron",
        "ai_economy.ron",
        "ai_rebuild.ron",
        // WP9.1c-1: production, rally points, the scout (long enough for the scout's legs; the army exists
        // only from tick 15,020, `what_one_production_turn_and_one_army_turn_cost_on_their_own` has it).
        "ai_scout.ron",
    ] {
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
    // question the cap allows is asked and refused, on `ai_base` with 5,000 wanderers on the map. The barracks
    // (`Kind(3)`) needs a power plant, which does not stand at tick 1, so the simulation refuses every place
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

#[test]
fn what_one_economy_turn_costs_on_its_own() {
    // WP9.1b-2: the economy turn alone (harvesters, silo, repair), on the observation of `ai_rebuild` at tick
    // 8200 with 5,000 wanderers on the map (the scenario's own script does not play out in that crowd: its
    // ids are the wanderers'), with the power plant made damaged in the observation so that the repair path
    // runs. The observation is built once (it is the same for every manager), the turn is called 20,000 times
    // on a copy of the state.
    let s = crowded_in("ai_rebuild.ron", 5000);
    let mut world = s.build_world().unwrap();
    let mut ais = s.ais.clone();
    let mut out = Vec::new();
    while world.tick().0 < 8200 {
        for ai in &mut ais {
            if ai.due(world.tick()) {
                out.clear();
                ai.think(&world, &mut out);
                for command in out.drain(..) {
                    world.schedule(world.tick(), ai.player(), command).unwrap();
                }
            }
        }
        world.step();
    }
    let player = sf_sim::PlayerId(0);
    let state = ais[0].state().clone();
    let mut obs = sf_ai::Observation::build(&world, player, &state.known_structures);
    for o in obs.own.iter_mut().filter(|o| o.kind == sf_sim::Kind(1)) {
        o.hp = o.max_hp / 5;
    }
    let mut worst = Duration::ZERO;
    let mut total = Duration::ZERO;
    let runs = 20_000u32;
    let mut sent = 0;
    for _ in 0..runs {
        let mut st = state.clone();
        let mut lens = sf_ai::Lens::new(&world, player);
        out.clear();
        let started = Instant::now();
        sf_ai::economy::turn(&mut st, &obs, &mut lens, &mut out);
        let spent = started.elapsed();
        total += spent;
        worst = worst.max(spent);
        sent += out.len();
    }
    println!(
        "one economy turn (observation of {} own entities, {} entities in the world, {sent} commands in {runs} turns): mean {:.2} us, worst {:.1} us (the state is cloned outside the clock)",
        obs.own.len(),
        world.len(),
        total.as_secs_f64() * 1e6 / f64::from(runs),
        worst.as_secs_f64() * 1e6
    );
    if !cfg!(debug_assertions) {
        assert!(total / runs <= Duration::from_micros(100));
    }
}

#[test]
fn what_one_production_turn_and_one_army_turn_cost_on_their_own() {
    // WP9.1c-1: the production turn and the army turn alone, on the observation of `ai_scout` late in the run
    // (the army exists, the factories have queues) with 200 more soldiers idle far from the gather cell added to
    // it, so that the army turn sends two gather orders of 19 and keeps the bookkeeping of 200 units. The
    // observation is built once; each turn is called 20,000 times on a copy of the state.
    let s = Scenario::load(&scenario_path("ai_scout.ron")).unwrap();
    let mut world = s.build_world().unwrap();
    let mut ais = s.ais.clone();
    let mut out = Vec::new();
    let until = if cfg!(debug_assertions) { 2500 } else { 17_000 };
    while world.tick().0 < until {
        for ai in &mut ais {
            if ai.due(world.tick()) {
                out.clear();
                ai.think(&world, &mut out);
                for command in out.drain(..) {
                    world.schedule(world.tick(), ai.player(), command).unwrap();
                }
            }
        }
        world.step();
    }
    let player = sf_sim::PlayerId(0);
    let state = ais[0].state().clone();
    let mut obs = sf_ai::Observation::build(&world, player, &state.known_structures);
    // Rich, so that the production turn runs to the end of its loop (one order per factory).
    obs.credits = 100_000;
    let first = obs.own.iter().map(|o| o.id.index()).max().unwrap_or(0) + 1000;
    for i in 0..200u32 {
        obs.own.push(sf_ai::Own {
            id: sf_sim::EntityId::new(first + i, 0),
            kind: sf_sim::Kind(12),
            cell: sf_sim::Cell::new(60 + i % 10, 60 + i / 10 % 10),
            footprint: None,
            hp: 50,
            order: sf_sim::Order::Idle,
            standing: true,
            max_hp: 50,
            building: None,
            repairing: false,
            factory: None,
            selling: false,
            armed: true,
            mobile: true,
        });
    }
    let runs = 20_000u32;
    for (name, army) in [("production", false), ("army", true)] {
        let mut worst = Duration::ZERO;
        let mut total = Duration::ZERO;
        let mut sent = 0;
        for _ in 0..runs {
            let mut st = state.clone();
            let mut lens = sf_ai::Lens::new(&world, player);
            out.clear();
            let started = Instant::now();
            if army {
                sf_ai::army::turn(&mut st, &obs, &mut lens, &mut out);
            } else {
                sf_ai::production::turn(&mut st, &obs, &mut lens, &mut out);
            }
            let spent = started.elapsed();
            total += spent;
            worst = worst.max(spent);
            sent += out.len();
        }
        println!(
            "one {name} turn (observation of {} own entities, {} entities in the world, {sent} commands in {runs} turns): mean {:.2} us, worst {:.1} us (the state is cloned outside the clock)",
            obs.own.len(),
            world.len(),
            total.as_secs_f64() * 1e6 / f64::from(runs),
            worst.as_secs_f64() * 1e6
        );
        if !cfg!(debug_assertions) {
            assert!(total / runs <= Duration::from_micros(100), "{name}");
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

#[test]
fn what_the_army_turn_costs_with_50_and_200_units_at_the_launch_and_on_the_march() {
    // WP9.1c-2: the army turn alone on the observation of `ai_vs_passive` at tick 4000 (the base and, since WP9.1c-3, no unit yet: the first is queued at 4,460 and exists at 4,621)
    // with N riflemen added idle at the gather cell, so that the turn launches an attack (the trigger, the target,
    // one `AttackMove` with N ids), and the turn after it with the N attackers idle half-way (the march: one
    // `AttackMove` again). 20,000 turns each on a copy of the state; the clock is outside the clone.
    let s = Scenario::load(&scenario_path("ai_vs_passive.ron")).unwrap();
    let mut world = s.build_world().unwrap();
    let mut ais = s.ais.clone();
    let mut out = Vec::new();
    while world.tick().0 < 4000 {
        for ai in &mut ais {
            if ai.due(world.tick()) {
                out.clear();
                ai.think(&world, &mut out);
                for command in out.drain(..) {
                    world.schedule(world.tick(), ai.player(), command).unwrap();
                }
            }
        }
        world.step();
    }
    let player = sf_sim::PlayerId(0);
    let mut state = ais[0].state().clone();
    state.profile.army.attack_value = 1;
    let gather = state.gather.expect("a gather cell");
    let runs = 20_000u32;
    for n in [50u32, 200] {
        let mut obs = sf_ai::Observation::build(&world, player, &state.known_structures);
        obs.tick = sf_sim::Tick(6000);
        let first = obs.own.iter().map(|o| o.id.index()).max().unwrap_or(0) + 1000;
        let soldier = |i: u32, cell: sf_sim::Cell| sf_ai::Own {
            id: sf_sim::EntityId::new(first + i, 0),
            kind: sf_sim::Kind(12),
            cell,
            footprint: None,
            hp: 50,
            order: sf_sim::Order::Idle,
            standing: true,
            max_hp: 50,
            building: None,
            repairing: false,
            factory: None,
            selling: false,
            armed: true,
            mobile: true,
        };
        for i in 0..n {
            obs.own.push(soldier(
                i,
                sf_sim::Cell::new(gather.x - 3 + i % 7, gather.y - 3 + i / 7 % 7),
            ));
        }
        let mut launched = state.clone();
        sf_ai::army::turn(
            &mut launched,
            &obs,
            &mut sf_ai::Lens::new(&world, player),
            &mut out,
        );
        assert!(
            launched.attackers.len() as u32 >= n,
            "the turn launches with the {n} added"
        );
        let mut march = obs.clone();
        march.tick = sf_sim::Tick(6200);
        for o in march.own.iter_mut().filter(|o| o.id.index() >= first) {
            o.cell = sf_sim::Cell::new(48, 30);
        }
        for (label, st0, o) in [("launch", &state, &obs), ("march", &launched, &march)] {
            let (mut total, mut worst, mut sent) = (Duration::ZERO, Duration::ZERO, 0);
            for _ in 0..runs {
                let mut st = st0.clone();
                let mut lens = sf_ai::Lens::new(&world, player);
                out.clear();
                let started = Instant::now();
                sf_ai::army::turn(&mut st, o, &mut lens, &mut out);
                let spent = started.elapsed();
                total += spent;
                worst = worst.max(spent);
                sent += out.len();
            }
            println!(
                "one army turn, {label}, {n} riflemen added ({} own entities, {sent} commands in {runs} turns): mean {:.2} us, worst {:.1} us",
                o.own.len(),
                total.as_secs_f64() * 1e6 / f64::from(runs),
                worst.as_secs_f64() * 1e6
            );
            assert!(
                sent >= runs as usize,
                "{label}: every turn orders the attack"
            );
            if !cfg!(debug_assertions) {
                assert!(total / runs <= Duration::from_micros(100), "{label} {n}");
            }
        }
    }
}

#[test]
fn ai_vs_passive_with_and_without_the_ai_and_the_tick_of_the_launch() {
    // WP9.1c-2: `ai_vs_passive` to the win (14,659; 13,839 since WP9.1c-3, run 14,700 ticks), the AI's thinks timed (mean, worst), and the step time of
    // the tick in which each attack is launched (the simulation plans the `AttackMove` for the whole wave then)
    // against the mean step. Then the whole run with and without the AI, alternating, medians of three (without
    // the AI nothing happens on player 0's side, so this is an upper bound of what the AI and what it makes cost).
    let s = Scenario::load(&scenario_path("ai_vs_passive.ron")).unwrap();
    let ticks = if cfg!(debug_assertions) { 2000 } else { 14_700 };
    let cost = measure(&s, ticks);
    report("ai_vs_passive", &cost);
    let mut world = s.build_world().unwrap();
    let mut ais = s.ais.clone();
    let mut out = Vec::new();
    let (mut steps, mut launches) = (Duration::ZERO, Vec::new());
    for _ in 0..ticks {
        let mut launch = false;
        for ai in &mut ais {
            if ai.due(world.tick()) {
                let before = ai.state().attacks_launched;
                out.clear();
                ai.think(&world, &mut out);
                launch |= ai.state().attacks_launched > before;
                for command in out.drain(..) {
                    world.schedule(world.tick(), ai.player(), command).unwrap();
                }
            }
        }
        let started = Instant::now();
        world.step();
        let spent = started.elapsed();
        steps += spent;
        if launch {
            launches.push((world.tick().0 - 1, spent));
        }
    }
    let mean = steps.as_secs_f64() * 1e6 / ticks as f64;
    for (t, d) in &launches {
        println!(
            "ai_vs_passive: the step of tick {t} (an attack launched) took {:.1} us; mean step {mean:.1} us",
            d.as_secs_f64() * 1e6
        );
    }
    if !cfg!(debug_assertions) {
        assert!(!launches.is_empty());
        assert!(cost.worst <= Duration::from_millis(10), "{:?}", cost.worst);
    }
    let mut without = s.clone();
    without.ais.clear();
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for _ in 0..3 {
        a.push(timed_run(&s, ticks));
        b.push(timed_run(&without, ticks));
    }
    a.sort();
    b.sort();
    let per_second = |d: Duration| ticks as f64 / d.as_secs_f64();
    println!(
        "whole run, ai_vs_passive, {ticks} ticks: with the AI {:.0} ticks/s, without {:.0} ticks/s (medians of 3 alternating runs)",
        per_second(a[1]),
        per_second(b[1])
    );
}
