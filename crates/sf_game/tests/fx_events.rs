//! The effects event queue (WP5.6a, follow-up 31): every shot, hit and death of every tick reaches the
//! picture exactly once and in order, whatever the sim thread did meanwhile.
//!
//! The sim thread runs a real fight (the `destruction` scenario: tanks, riflemen, rocket troopers, structures
//! that fall) on its own clock, made slow enough that it falls behind and catches up (`max_catch_up` is the
//! game's 3, so a pass steps up to four ticks and publishes one snapshot), while the main thread reads
//! the way a frame does. What comes out of `SimHandle::drain_fx` is compared with the events of the same
//! scenario run single-threaded, tick by tick, and with the world's own counts of shots, impacts and
//! deaths, which are the independent truth.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use sf_game::fx_events::{FX_QUEUE_CAP, FxEvent, FxKind, record};
use sf_game::sim_thread::{MAX_CATCH_UP_TICKS, SimConfig, SimHandle};
use sf_headless::scenario::Scenario;
use sf_sim::{Command, Mount, PlayerId, ProjectileKind, Tick, World};

fn scenario() -> Scenario {
    let path: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenarios/destruction.ron");
    Scenario::load(&path).expect("the destruction scenario loads")
}

fn bare_world(s: &Scenario) -> World {
    Scenario {
        spawns: Vec::new(),
        commands: Vec::new(),
        ..s.clone()
    }
    .build_world()
    .expect("the scenario without its commands is still valid")
}

fn scenario_commands(s: &Scenario) -> Vec<(Tick, PlayerId, Command)> {
    s.build_world()
        .unwrap()
        .pending_commands()
        .iter()
        .map(|(tick, player, command)| (tick, player, command.clone()))
        .collect()
}

/// What the single-threaded run found: the events, and the world's own counts.
struct Reference {
    events: Vec<FxEvent>,
    unresolved: u64,
    /// Shots, instant shots, impacts and deaths the world reported, summed over the ticks.
    shots: u64,
    instant: u64,
    impacts: u64,
    deaths: u64,
}

fn reference(s: &Scenario, ticks: u64) -> Reference {
    let mut world = s.build_world().unwrap();
    let mut r = Reference {
        events: Vec::new(),
        unresolved: 0,
        shots: 0,
        instant: 0,
        impacts: 0,
        deaths: 0,
    };
    for _ in 0..ticks {
        world.step();
        r.unresolved += record(&world, &mut r.events);
        r.shots += world.shots().len() as u64;
        r.instant += world
            .shots()
            .iter()
            .filter(|s| {
                world
                    .weapon_table()
                    .weapon(s.weapon)
                    .is_some_and(|w| w.projectile == ProjectileKind::Instant)
            })
            .count() as u64;
        r.impacts += world.impacts().len() as u64;
        r.deaths += world.deaths().len() as u64;
    }
    r
}

/// Runs the scenario on the sim thread with `config`, reading the way a frame does (every few milliseconds,
/// the newest snapshot, the events), until `ticks` ticks have run; returns what came out, in order, the
/// ticks run, the counters and how many deaths the snapshots showed.
fn threaded(
    s: &Scenario,
    config: SimConfig,
    ticks: u64,
    read_every: Option<Duration>,
) -> (Vec<FxEvent>, u64, sf_game::sim_thread::SimCounters, u64) {
    let mut sim = SimHandle::spawn(bare_world(s), config).unwrap();
    for (tick, player, command) in scenario_commands(s) {
        sim.send_at(tick, player, command);
    }
    sim.start();
    let mut got = Vec::new();
    let mut seen_deaths = 0;
    let mut last_tick = Tick::ZERO;
    while sim.counters().ticks < ticks {
        match read_every {
            Some(every) => {
                thread::sleep(every);
                // A frame: the newest snapshot (what it shows of deaths is the lossy list) and the queue.
                let snap = sim.latest();
                if snap.tick() != last_tick {
                    seen_deaths += snap.deaths().len() as u64;
                    last_tick = snap.tick();
                }
                sim.drain_fx(&mut got);
            }
            None => thread::sleep(Duration::from_millis(20)),
        }
    }
    let exit = sim.stop().clone();
    assert!(!exit.panicked);
    sim.drain_fx(&mut got);
    (got, exit.ticks, sim.counters(), seen_deaths)
}

fn kinds(events: &[FxEvent]) -> (usize, usize, usize) {
    let fires = events
        .iter()
        .filter(|e| matches!(e.kind, FxKind::Fire { .. }))
        .count();
    let impacts = events
        .iter()
        .filter(|e| matches!(e.kind, FxKind::Impact { .. }))
        .count();
    let deaths = events
        .iter()
        .filter(|e| matches!(e.kind, FxKind::Death { .. }))
        .count();
    (fires, impacts, deaths)
}

fn check(got: &[FxEvent], ran: u64, counters: sf_game::sim_thread::SimCounters, s: &Scenario) {
    let r = reference(s, ran);
    let (fires, impacts, deaths) = kinds(&r.events);
    // The scenario is a real fight: the comparison is not about nothing.
    assert!(fires >= 50, "{fires} shots in {ran} ticks");
    assert!(impacts >= 50, "{impacts} impacts");
    assert!(deaths >= 2, "{deaths} deaths");
    assert!(r.impacts >= 5, "projectiles went off: {}", r.impacts);
    // The reference is the world's own report, counted independently of `record`.
    assert_eq!(
        r.events.len() as u64 + r.unresolved,
        r.shots + r.instant + r.impacts + r.deaths,
        "events plus unresolved = shots + instant hits + impacts + deaths"
    );
    assert_eq!(deaths as u64, r.deaths, "every death");
    // What the thread delivered is exactly that, in order.
    assert_eq!(counters.fx_dropped, 0, "the queue was never full");
    assert_eq!(counters.fx_unresolved, r.unresolved);
    assert_eq!(got.len(), r.events.len(), "no event lost or doubled");
    assert_eq!(
        got,
        r.events.as_slice(),
        "the same events in the same order"
    );
    assert!(
        got.windows(2).all(|w| w[0].tick <= w[1].tick),
        "in tick order"
    );
}

/// A sim that is slower than its clock steps up to four ticks per pass and publishes once: still every
/// event of every tick comes out.
#[test]
fn nothing_is_lost_when_the_sim_falls_behind_and_catches_up() {
    let s = scenario();
    let config = SimConfig {
        period: Duration::from_millis(4),
        extra_tick_time: Duration::from_millis(6),
        start_paused: true,
        ..SimConfig::realtime()
    };
    assert_eq!(config.max_catch_up, MAX_CATCH_UP_TICKS);
    assert!(config.max_catch_up > 0);
    let (got, ran, counters, seen_deaths) =
        threaded(&s, config, 900, Some(Duration::from_millis(7)));
    assert!(ran >= 900);
    assert!(
        counters.max_pass_ticks > 1,
        "the sim caught up in passes of several ticks: {}",
        counters.max_pass_ticks
    );
    check(&got, ran, counters, &s);
    let (_, _, deaths) = kinds(&got);
    println!(
        "ran {ran} ticks, longest pass {} ticks, {} events; the snapshots showed {seen_deaths} of {deaths} deaths",
        counters.max_pass_ticks,
        got.len()
    );
}

/// One long hitch makes a catch-up pass of the most ticks the sim takes at once, and the main thread reads
/// nothing at all while it runs: the events wait in the queue and come out whole.
#[test]
fn a_hitch_and_a_main_thread_that_reads_nothing_lose_nothing() {
    let s = scenario();
    // The hitch is put at the first tick that has an event, so the pass that catches up has events in it.
    let first = reference(&s, 400).events[0].tick;
    let config = SimConfig {
        period: Duration::from_millis(4),
        hitch: Some((first, Duration::from_millis(80))),
        start_paused: true,
        ..SimConfig::realtime()
    };
    let (got, ran, counters, _) = threaded(&s, config, 700, None);
    assert!(
        counters.max_pass_ticks > 1,
        "the hitch forced a pass of several ticks: {}",
        counters.max_pass_ticks
    );
    assert!(counters.max_pass_ticks <= 1 + u64::from(MAX_CATCH_UP_TICKS));
    // Nothing was read during the run, so everything came out of the queue in one go.
    check(&got, ran, counters, &s);
}

/// The queue's cap is the one place events can go missing, and it says so: the counter of the sim thread.
#[test]
fn the_cap_is_a_real_number_and_the_counters_start_at_zero() {
    assert_eq!(FX_QUEUE_CAP, 4096);
    let world = bare_world(&scenario());
    let sim = SimHandle::spawn(world, SimConfig::manual()).unwrap();
    let c = sim.counters();
    assert_eq!((c.fx_recorded, c.fx_dropped, c.fx_unresolved), (0, 0, 0));
    let mut out = Vec::new();
    sim.drain_fx(&mut out);
    assert!(out.is_empty());
}

/// What recording costs the sim thread: not a test that can fail, a measurement (`--ignored --nocapture`,
/// release build). The same scenario is stepped twice per round, alternating: with `record` after every
/// step and without. Relative numbers on a busy machine; the sim tick budget is 50 ms.
#[test]
#[ignore = "a measurement, run by hand: cargo test --release -p sf_game --test fx_events -- --ignored --nocapture"]
fn what_recording_costs_the_sim_thread() {
    use std::time::Instant;
    let s = scenario();
    let mut with = Vec::new();
    let mut without = Vec::new();
    let mut events = 0;
    for _ in 0..5 {
        for record_too in [false, true] {
            let mut world = s.build_world().unwrap();
            let mut out = Vec::with_capacity(256);
            let mut step_time = Duration::ZERO;
            let mut record_time = Duration::ZERO;
            for _ in 0..1500 {
                let began = Instant::now();
                world.step();
                step_time += began.elapsed();
                if record_too {
                    let began = Instant::now();
                    out.clear();
                    record(&world, &mut out);
                    record_time += began.elapsed();
                    events += out.len();
                }
            }
            if record_too {
                with.push((step_time, record_time));
            } else {
                without.push(step_time);
            }
        }
    }
    let per_tick = |d: Duration| d.as_secs_f64() * 1e6 / 1500.0;
    let step_only: Vec<f64> = without.iter().map(|d| per_tick(*d)).collect();
    let step_rec: Vec<f64> = with.iter().map(|(s, _)| per_tick(*s)).collect();
    let rec: Vec<f64> = with.iter().map(|(_, r)| per_tick(*r)).collect();
    let med = |v: &[f64]| {
        let mut v = v.to_vec();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    println!(
        "destruction scenario, 1500 ticks, medians of 5 alternating rounds: step {:.1} us/tick alone, \
         {:.1} us/tick in the rounds that also record; record {:.2} us/tick ({} events in all rounds)",
        med(&step_only),
        med(&step_rec),
        med(&rec),
        events
    );
}

/// A shot's flash points along the weapon's mount (review of #77): a turret weapon along the turret's facing,
/// even when the turret is turned away from its hull, and a hull weapon (or an infantry rifle) along the hull,
/// with the shooter's body length as its size. Checked on a real fight through `record`, shot by shot against
/// the world's own shooter, and the fight must contain both cases, or the test proves nothing.
#[test]
fn a_shot_points_along_its_mount_turret_or_hull() {
    let s = scenario();
    let mut world = s.build_world().unwrap();
    let (mut turned_turret, mut on_hull) = (0, 0);
    for _ in 0..900 {
        world.step();
        let mut events = Vec::new();
        record(&world, &mut events);
        let fires: Vec<&FxEvent> = events
            .iter()
            .filter(|e| matches!(e.kind, FxKind::Fire { .. }))
            .collect();
        let shooters: Vec<_> = world
            .shots()
            .iter()
            .filter_map(|shot| Some((shot, world.get(shot.attacker)?)))
            .collect();
        assert_eq!(
            fires.len(),
            shooters.len(),
            "every living shooter fires once"
        );
        for (ev, (shot, v)) in fires.iter().zip(&shooters) {
            let weapon = world.weapon_table().weapon(shot.weapon).unwrap();
            match (weapon.mount, v.turret) {
                (Mount::Turret, Some(t)) => {
                    assert_eq!(ev.facing, t.facing.raw(), "a turret shot uses the turret");
                    if t.facing != v.facing {
                        turned_turret += 1;
                    }
                }
                _ => {
                    assert_eq!(ev.facing, v.facing.raw(), "a hull shot uses the hull");
                    on_hull += 1;
                }
            }
            assert!(ev.size > 0.0);
        }
    }
    println!("turret shots off the hull: {turned_turret}, hull or infantry shots: {on_hull}");
    assert!(
        turned_turret > 0,
        "no turret shot with the turret off its hull"
    );
    assert!(on_hull > 0, "no hull or infantry shot");
}
