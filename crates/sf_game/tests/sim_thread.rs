//! The simulation thread (WP1.15): that it computes exactly what the headless
//! runner computes, that a slow sim does not slow the frames, that catching up
//! is bounded, and that it shuts down.
//!
//! No window, no GPU: the "renderer" of the frame-loop test is a loop that
//! does what `Runner::render` does with the sim (take the newest snapshot, ask
//! the interpolation factor, walk the entities) and measures how long that
//! takes.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use sf_game::sim_snapshot::SimSnapshot;
use sf_game::sim_thread::{MAX_CATCH_UP_TICKS, SimConfig, SimHandle, interpolation_alpha};
use sf_headless::run::{RunOptions, run};
use sf_headless::scenario::Scenario;
use sf_math::Vec2;
use sf_math::render_bridge::vec2_to_f32;
use sf_sim::{Command, EntityView, Order, PlayerId, Tick, World, WorldConfig};

const WAIT: Duration = Duration::from_secs(120);

/// The tests here measure time (frames, catching up, joins) and each starts
/// threads of its own; run side by side on a small CI machine they would
/// disturb each other's clocks. They take this lock, one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn one_at_a_time() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner)
}

fn scenario(name: &str) -> Scenario {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/scenarios")
        .join(format!("{name}.ron"));
    Scenario::load(&path).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The scenario's world with nothing scheduled: no spawns, no commands.
fn bare_world(s: &Scenario) -> World {
    Scenario {
        spawns: Vec::new(),
        commands: Vec::new(),
        ..s.clone()
    }
    .build_world()
    .expect("the scenario without its commands is still valid")
}

/// Every command the scenario schedules, in execution order.
fn scenario_commands(s: &Scenario) -> Vec<(Tick, PlayerId, Command)> {
    s.build_world()
        .unwrap()
        .pending_commands()
        .iter()
        .map(|(tick, player, command)| (tick, player, command.clone()))
        .collect()
}

fn views_of(world: &World) -> Vec<EntityView> {
    world.snapshot().iter().collect()
}

/// Runs `name` for `ticks` ticks through the sim thread, one tick at a time,
/// with its commands sent over the channel, and through `sf_headless`; every
/// tick's checksum, and every entity of the snapshot, must be the same.
fn threaded_equals_headless(name: &str, ticks: u64) {
    let s = scenario(name);
    // The headless runner, with a checksum recorded after every tick.
    let headless = run(&s, &RunOptions { ticks, every: 1 }).unwrap();
    assert_eq!(headless.samples.len() as u64, ticks);

    // A second single-threaded world to compare what the snapshots show.
    let mut reference = s.build_world().unwrap();

    let mut sim = SimHandle::spawn(bare_world(&s), SimConfig::manual()).unwrap();
    for (tick, player, command) in scenario_commands(&s) {
        sim.send_at(tick, player, command);
    }
    for t in 1..=ticks {
        sim.advance(1);
        let snap = sim
            .wait_for_tick(Tick(t), WAIT)
            .unwrap_or_else(|| panic!("{name}: the sim did not reach tick {t}"));
        assert_eq!(snap.tick(), Tick(t), "{name}");
        let expected = headless.samples[(t - 1) as usize];
        assert_eq!(expected.ticks, t);
        assert_eq!(
            snap.checksum(),
            Some(expected.checksum),
            "{name}: checksum after tick {t}: threaded sim and sf_headless differ"
        );
        // What the picture is drawn from is the same world, entity by entity.
        let step = reference.step();
        assert_eq!(
            step.checksum, expected.checksum,
            "{name}: reference, tick {t}"
        );
        assert_eq!(
            snap.views(),
            views_of(&reference).as_slice(),
            "{name}: tick {t}"
        );
        assert_eq!(snap.len(), reference.len());
    }
    let counters = sim.counters();
    assert_eq!(counters.ticks, ticks);
    assert_eq!(
        counters.late_commands, 0,
        "{name}: every command was in time"
    );
    let exit = sim.stop();
    assert!(!exit.panicked);
    assert_eq!(exit.ticks, ticks);
    assert_eq!(
        exit.checksums.last(),
        Some(&headless.final_checksum),
        "{name}: final checksum"
    );
    assert_eq!(exit.checksums, reference.checksum_history(), "{name}");
}

#[test]
fn threaded_sim_matches_headless_on_the_orders_scenario() {
    let _serial = one_at_a_time();
    // Structures, a unit table, movement classes, every order type, a despawn.
    threaded_equals_headless("orders", 1_600);
}

#[test]
fn threaded_sim_matches_headless_on_the_command_mix() {
    let _serial = one_at_a_time();
    // The legacy commands and 1,000 wanderers.
    threaded_equals_headless("command_mix", 400);
}

#[test]
fn threaded_sim_matches_headless_on_spawned_units_and_terrain() {
    let _serial = one_at_a_time();
    threaded_equals_headless("units_spawn", 400);
    threaded_equals_headless("terrain_256", 200);
}

#[test]
fn threaded_sim_matches_headless_with_paths() {
    let _serial = one_at_a_time();
    threaded_equals_headless("pathing_256", 120);
}

#[test]
fn threaded_sim_matches_headless_with_crowd_collisions() {
    let _serial = one_at_a_time();
    threaded_equals_headless("crowd_jam", 120);
}

/// The same, with the sim on its own clock and the commands sent while it
/// runs, from what the snapshots show: nothing about *when* a command lands is
/// fixed, but the log says on which tick it did, and a single-threaded world
/// that is given the log makes the same world.
#[test]
fn a_running_sim_with_live_commands_replays_to_the_same_checksums() {
    let _serial = one_at_a_time();
    let s = scenario("orders");
    let mut sim = SimHandle::spawn(
        bare_world(&s),
        SimConfig {
            period: Duration::from_millis(3),
            start_paused: true,
            record_commands: true,
            ..SimConfig::realtime()
        },
    )
    .unwrap();
    for (tick, player, command) in scenario_commands(&s) {
        sim.send_at(tick, player, command);
    }
    sim.start();
    let mut next_order = 20;
    let mut sent = 0;
    loop {
        let snap = sim.wait_for_tick(Tick(next_order), WAIT).unwrap();
        // Orders from what the player sees: every third own unit of the
        // snapshot walks somewhere that depends on the tick.
        let ids: Vec<_> = snap
            .views()
            .iter()
            .filter(|v| v.has_stats && v.owner.0 == 0)
            .map(|v| v.id)
            .enumerate()
            .filter(|(i, _)| (*i as u64 + next_order / 20).is_multiple_of(3))
            .map(|(_, id)| id)
            .collect();
        let at = Vec2::from_ints(8 + (next_order % 47) as i32, 8 + (next_order % 31) as i32);
        sim.send(
            PlayerId(0),
            Command::Order {
                ids,
                order: Order::Move(at),
                queued: next_order % 40 == 0,
            },
        );
        sent += 1;
        next_order += 20;
        if next_order > 400 {
            break;
        }
    }
    let exit = sim.stop().clone();
    assert!(!exit.panicked);
    assert!(exit.ticks >= 380, "ran {} ticks", exit.ticks);
    assert!(exit.commands.len() >= sent, "the log holds every command");

    // The world's checksum includes the commands that wait for their tick, so
    // each one goes in when it went in on the sim thread.
    let mut replay = bare_world(&s);
    for _ in 0..exit.ticks {
        let now = replay.tick();
        for logged in exit.commands.iter().filter(|c| c.arrived == now) {
            assert!(logged.effective >= logged.requested);
            replay
                .schedule(logged.effective, logged.player, logged.command.clone())
                .unwrap();
        }
        replay.step();
    }
    assert_eq!(
        replay.checksum_history(),
        exit.checksums.as_slice(),
        "a single-threaded replay of the sim thread's command log"
    );
}

/// A world with a few wanderers on a small map: ticks cost nothing.
fn light_world() -> World {
    let mut world = World::new(WorldConfig::new(5).with_map_size(32, 32)).unwrap();
    for i in 0..40 {
        world
            .schedule(
                Tick(0),
                PlayerId(0),
                Command::Spawn {
                    kind: sf_sim::Kind(1 + (i % 3) as u16),
                    pos: Vec2::from_ints(2 + i % 28, 2 + i / 2 % 28),
                    owner: PlayerId(0),
                },
            )
            .unwrap();
    }
    world
}

/// The frames of the game while the sim is slow. A sim tick that takes three
/// times its budget must not show in how long the frame loop takes: the loop
/// only ever reads the newest snapshot.
#[test]
fn a_slow_sim_does_not_delay_the_frames() {
    let _serial = one_at_a_time();
    let tick_delay = Duration::from_millis(150);
    let sim = SimHandle::spawn(
        scenario("orders").build_world().unwrap(),
        SimConfig {
            extra_tick_time: tick_delay,
            ..SimConfig::realtime()
        },
    )
    .unwrap();

    let frame_gap = Duration::from_millis(5);
    let (mut frames, mut slowest_work, mut longest_interval) =
        (0_u32, Duration::ZERO, Duration::ZERO);
    let mut last_start: Option<Instant> = None;
    let mut seen_behind = false;
    let mut distinct_ticks = std::collections::BTreeSet::new();
    let mut sum = 0.0_f64;
    let begun = Instant::now();
    while begun.elapsed() < Duration::from_millis(1500) {
        let start = Instant::now();
        if let Some(previous) = last_start {
            longest_interval = longest_interval.max(start.duration_since(previous));
        }
        last_start = Some(start);

        // What Runner::render does with the sim, every frame:
        let snap = sim.latest();
        let alpha = interpolation_alpha(start, snap.pace(), sim.period());
        for v in snap.views() {
            let (now, before) = (vec2_to_f32(v.pos), vec2_to_f32(v.prev_pos));
            sum += f64::from(alpha) * f64::from(now[0] - before[0]);
        }
        seen_behind |= !snap.pace().lag.is_zero();
        distinct_ticks.insert(snap.tick());

        slowest_work = slowest_work.max(start.elapsed());
        frames += 1;
        thread::sleep(frame_gap);
    }
    assert!(sum.is_finite());
    // The sim really was slow: far fewer ticks than the wall clock allows, and
    // it knew it was behind.
    let counters = sim.counters();
    assert!(
        counters.ticks <= 1500 / 150 + 2,
        "{} ticks in 1.5 s of 150 ms ticks",
        counters.ticks
    );
    assert!(counters.ticks >= 3, "the sim ran: {}", counters.ticks);
    assert!(seen_behind, "the snapshots said the sim was behind");
    assert!(distinct_ticks.len() >= 3);
    // And the frames did not notice. A tick is 150 ms; the loop never waited
    // for one.
    assert!(frames >= 40, "only {frames} frames in 1.5 s");
    assert!(
        slowest_work < Duration::from_millis(40),
        "a frame's work took {slowest_work:?} while the sim was busy"
    );
    assert!(
        longest_interval < Duration::from_millis(75),
        "frames were {longest_interval:?} apart while the sim ticks took {tick_delay:?}"
    );
}

/// Runs `f` on a thread and fails if it does not finish in `limit`: a hang in
/// a shutdown must fail the test, not the CI job's timeout.
fn finishes_within(label: &str, limit: Duration, f: impl FnOnce() + Send + 'static) {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        f();
        let _ = tx.send(());
    });
    rx.recv_timeout(limit)
        .unwrap_or_else(|_| panic!("{label}: did not finish in {limit:?}"));
}

#[test]
fn the_sim_thread_joins_cleanly_however_it_is_running() {
    let _serial = one_at_a_time();
    let limit = Duration::from_secs(5);

    finishes_within("a running sim, dropped", limit, || {
        let sim = SimHandle::spawn(light_world(), SimConfig::realtime()).unwrap();
        thread::sleep(Duration::from_millis(120));
        assert!(sim.is_running());
        drop(sim);
    });

    finishes_within("a sim that was never started", limit, || {
        let sim = SimHandle::spawn(
            light_world(),
            SimConfig {
                start_paused: true,
                ..SimConfig::realtime()
            },
        )
        .unwrap();
        assert_eq!(sim.latest().tick(), Tick(0));
        drop(sim);
    });

    finishes_within("a stepped sim", limit, || {
        let sim = SimHandle::spawn(light_world(), SimConfig::manual()).unwrap();
        sim.advance(3);
        drop(sim);
    });

    // In the middle of a tick that takes 400 ms, the window is closed: the
    // join waits for that tick and no longer.
    finishes_within("a sim in the middle of a long tick", limit, || {
        let mut sim = SimHandle::spawn(
            light_world(),
            SimConfig {
                extra_tick_time: Duration::from_millis(400),
                ..SimConfig::realtime()
            },
        )
        .unwrap();
        thread::sleep(Duration::from_millis(100));
        let began = Instant::now();
        sim.stop();
        let waited = began.elapsed();
        assert!(
            waited < Duration::from_millis(1500),
            "joined after {waited:?}"
        );
    });

    finishes_within("stopping twice, using a stopped sim", limit, || {
        let mut sim = SimHandle::spawn(light_world(), SimConfig::realtime()).unwrap();
        thread::sleep(Duration::from_millis(60));
        let exit = sim.stop().clone();
        assert!(!exit.panicked && exit.ticks >= 1);
        assert_eq!(sim.stop().ticks, exit.ticks, "the second stop is a no-op");
        assert!(!sim.is_running());
        // The last snapshot stays readable, commands go nowhere and nothing
        // panics.
        let last = sim.latest();
        assert_eq!(last.tick().0, exit.ticks);
        sim.send(PlayerId(0), Command::Stop { ids: Vec::new() });
        sim.advance(1);
    });
}

/// After a hitch the sim catches up, but in bursts of at most
/// `1 + MAX_CATCH_UP_TICKS` ticks, one burst per interval.
#[test]
fn catching_up_after_a_hitch_is_bounded() {
    let _serial = one_at_a_time();
    let period = Duration::from_millis(20);
    let sim = SimHandle::spawn(
        light_world(),
        SimConfig {
            period,
            // Tick 10 takes 15 periods: 14 ticks to catch up, which is under
            // the limit at which the clock is restarted instead.
            hitch: Some((10, Duration::from_millis(300))),
            ..SimConfig::realtime()
        },
    )
    .unwrap();
    // Watch the published ticks: no two reads within one interval differ by
    // more than two bursts.
    let mut reads: Vec<(Instant, u64)> = Vec::new();
    let begun = Instant::now();
    while begun.elapsed() < Duration::from_millis(1400) {
        reads.push((Instant::now(), sim.latest().tick().0));
        thread::sleep(Duration::from_millis(1));
    }
    let counters = sim.counters();
    assert!(
        (2..=1 + u64::from(MAX_CATCH_UP_TICKS)).contains(&counters.max_pass_ticks),
        "the sim caught up (more than one tick in a pass) in bursts of at most {}: {}",
        1 + MAX_CATCH_UP_TICKS,
        counters.max_pass_ticks
    );
    assert_eq!(counters.clock_resets, 0, "14 ticks of lag are caught up");
    assert!(counters.behind_passes >= 1);
    for (i, &(t0, n0)) in reads.iter().enumerate() {
        for &(t1, n1) in &reads[i + 1..] {
            if t1.duration_since(t0) > period {
                break;
            }
            assert!(
                n1 - n0 <= 2 * (1 + u64::from(MAX_CATCH_UP_TICKS)),
                "{} ticks within {:?}",
                n1 - n0,
                t1.duration_since(t0)
            );
        }
    }
    // It did catch up: back on its clock, ticks match the wall time again.
    let snap = sim.latest();
    assert!(
        snap.pace().lag < period,
        "still {:?} behind",
        snap.pace().lag
    );
    let expected = 1400 / 20;
    assert!(
        snap.tick().0 + 6 >= expected && snap.tick().0 <= expected + 2,
        "tick {} after 1.4 s of 20 ms ticks",
        snap.tick().0
    );
}

/// A stall too long to catch up (here 75 ticks against a limit of 20) restarts
/// the clock: the game runs behind the wall clock from then on, no tick is
/// skipped, and there is no burst.
#[test]
fn a_stall_beyond_the_limit_restarts_the_clock_and_skips_nothing() {
    let _serial = one_at_a_time();
    let period = Duration::from_millis(20);
    let mut sim = SimHandle::spawn(
        light_world(),
        SimConfig {
            period,
            hitch: Some((5, Duration::from_millis(1500))),
            record_commands: true,
            ..SimConfig::realtime()
        },
    )
    .unwrap();
    let begun = Instant::now();
    thread::sleep(Duration::from_millis(2300));
    let wall = begun.elapsed();
    let counters = sim.counters();
    let exit = sim.stop().clone();
    assert_eq!(counters.clock_resets, 1);
    assert!(counters.max_pass_ticks <= 1 + u64::from(MAX_CATCH_UP_TICKS));
    // The wall clock would have allowed this many; the stall's 75 ticks are
    // not made up for.
    let allowed = (wall.as_millis() / period.as_millis()) as u64;
    assert!(
        exit.ticks + 40 < allowed,
        "{} ticks in {wall:?}, a catching-up sim would have {allowed}",
        exit.ticks
    );
    // Every tick that ran has its checksum: none was skipped.
    assert_eq!(exit.checksums.len() as u64, exit.ticks);
}

/// A command for a tick that has run is not lost and not silent: it runs in
/// the next tick and is counted.
#[test]
fn a_command_for_a_past_tick_runs_next_and_is_counted() {
    let _serial = one_at_a_time();
    let mut sim = SimHandle::spawn(
        light_world(),
        SimConfig {
            record_commands: true,
            ..SimConfig::manual()
        },
    )
    .unwrap();
    sim.advance(5);
    let snap = sim.wait_for_tick(Tick(5), WAIT).unwrap();
    let id = snap.views()[0].id;
    sim.send_at(
        Tick(2),
        PlayerId(0),
        Command::Order {
            ids: vec![id],
            order: Order::Move(Vec2::from_ints(1, 1)),
            queued: false,
        },
    );
    sim.advance(1);
    let after = sim.wait_for_tick(Tick(6), WAIT).unwrap();
    assert_eq!(after.order_of(id), Some(Order::Move(Vec2::from_ints(1, 1))));
    assert_eq!(sim.counters().late_commands, 1);
    let exit = sim.stop();
    assert_eq!(exit.commands.len(), 1);
    assert_eq!(exit.commands[0].requested, Tick(2));
    assert_eq!(exit.commands[0].effective, Tick(5));
}

/// The pieces of the snapshot the cursor reads agree with the world they were
/// taken from.
#[test]
fn a_snapshot_answers_like_the_world() {
    let _serial = one_at_a_time();
    let s = scenario("orders");
    let mut world = s.build_world().unwrap();
    for _ in 0..50 {
        world.step();
    }
    let snap = SimSnapshot::capture(&world);
    assert_eq!(snap.tick(), world.tick());
    assert_eq!(snap.checksum(), world.checksum_history().last().copied());
    assert_eq!(snap.len(), world.len());
    for v in world.snapshot().iter() {
        assert_eq!(snap.get(v.id), Some(v));
        assert_eq!(snap.stats_of(v.id), world.stats_of(v.id));
        assert_eq!(snap.order_of(v.id), world.order_of(v.id));
        assert!(snap.is_alive(v.id));
    }
    let (w, h) = world.map_size();
    for class in 0..64_u16 {
        for y in 0..h {
            for x in 0..w {
                let cell = sf_sim::Cell::new(x, y);
                assert_eq!(
                    snap.passable(class, cell),
                    world.passable(class, cell),
                    "class {class} at {cell}"
                );
            }
        }
    }
}
