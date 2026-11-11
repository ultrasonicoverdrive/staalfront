//! The command record (WP1.16a): a match played through the game's sim thread, written with the
//! game's own recorder, replays in `sf_headless` to the same checksum at every tick.
//!
//! The game side is built with the production functions the game uses (`playfield::make_world`,
//! `matchplay::configure` and `schedule`, one step, then `SimHandle` with `record_commands`, as
//! `Runner::new` makes it with `--record`). The commands are the script of the pinned `match`
//! scenario (build, produce, rally, attack-move across the map) and a few moves to positions that are
//! not a cell middle. Each is sent the way the game sends one (`SimHandle::send`, stamped one tick
//! ahead of the newest snapshot). The record is written by `sf_game::record::render`, read back by
//! `sf_headless::scenario::Scenario::load` and run with a checksum after every tick.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sf_data::map::Map;
use sf_data::units::UnitDefs;
use sf_game::sim_thread::{SimConfig, SimExit, SimHandle};
use sf_game::{bench_scene, matchplay, playfield, record};
use sf_headless::run::{RunOptions, run};
use sf_headless::scenario::Scenario;
use sf_math::{Fx, Vec2};
use sf_sim::{Command, EntityId, Order, PlayerId, Tick, World};

const WAIT: Duration = Duration::from_secs(60);

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

/// The world `sf_game --match` hands to its sim thread: the match file's map, the rules of the game, the
/// starting objects scheduled and the first tick run (`runner::match_scene`).
fn game_world() -> (World, (u32, u32)) {
    let assets = assets();
    let def = matchplay::load(&matchplay::file_path(None, &assets)).unwrap();
    let map = Map::load(&matchplay::map_dir(&def, &assets)).unwrap();
    let cells = map.size_in_cells();
    let defs = UnitDefs::load(&assets.join("config"), &assets).unwrap();
    let world = playfield::make_world(bench_scene::world_config(cells), &map).unwrap();
    let mut world = matchplay::configure(world, &def, &defs).unwrap();
    matchplay::schedule(&mut world, &def, &defs).unwrap();
    world.step();
    (world, cells)
}

/// The script of the pinned `match` scenario, as `(tick it runs at, player, command)`; the starting
/// objects of tick 0 are not part of it (the game schedules those itself).
fn script() -> Vec<(u64, PlayerId, Command)> {
    let path = assets().join("scenarios/match.ron");
    let s = Scenario::load(&path).unwrap();
    let mut list: Vec<_> = s
        .build_world()
        .unwrap()
        .pending_commands()
        .iter()
        .filter(|(tick, ..)| tick.0 >= 1)
        .map(|(tick, player, command)| (tick.0, player, command.clone()))
        .collect();
    // Orders to a spot a click would give: not the middle of a cell. They go to the first three tanks
    // while they wait at the rally point, before the script's attack-move of tick 2400 replaces them, so
    // the scripted attack is left intact and the match is decided by the script itself.
    let army: Vec<EntityId> = [15, 16, 17].map(|i| EntityId::new(i, 0)).to_vec();
    let odd = |x: i64, y: i64| Vec2::new(Fx::from_raw(x), Fx::from_raw(y));
    list.push((
        2_000,
        PlayerId(0),
        Command::Order {
            ids: army.clone(),
            order: Order::Move(odd(36 * 65_536 + 12_345, 58 * 65_536 + 54_321)),
            queued: false,
        },
    ));
    list.push((
        2_050,
        PlayerId(0),
        Command::Order {
            ids: army,
            order: Order::AttackMove(odd(35 * 65_536 + 1, 60 * 65_536 + 65_535)),
            queued: true,
        },
    ));
    list.sort_by_key(|(tick, ..)| *tick);
    list
}

/// Plays `ticks` ticks of the match on the sim thread, one tick at a time, sending each command of the
/// script one tick ahead; returns what the thread left.
fn play(ticks: u64) -> (SimExit, (u32, u32), Option<sf_sim::MatchResult>) {
    let (world, cells) = game_world();
    let mut sim = SimHandle::spawn(
        world,
        SimConfig {
            record_commands: true,
            ..SimConfig::manual()
        },
    )
    .unwrap();
    let script = script();
    let mut next = 0;
    for ran in 1..ticks {
        // The snapshot says `ran` ticks have run: a command for tick `ran + 1` goes in now.
        while next < script.len() && script[next].0 <= ran + 1 {
            let (_, player, command) = &script[next];
            sim.send(*player, command.clone());
            next += 1;
        }
        sim.advance(1);
        sim.wait_for_tick(Tick(ran + 1), WAIT).expect("a tick");
    }
    assert!(
        script[next..].iter().all(|(tick, ..)| *tick > ticks),
        "every command of the script that falls inside the play was sent"
    );
    let result = sim.latest().match_result();
    (sim.stop().clone(), cells, result)
}

fn write_record(exit: &SimExit, cells: (u32, u32), dir: &Path, name: &str) -> PathBuf {
    let assets = assets();
    let file = matchplay::file_path(None, &assets);
    let (text, _) = record::render(exit, cells, &assets, &file).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, &text).unwrap();
    // `SF_RECORD_KEEP=FILE` keeps the record of the full match for a profiler or `sf_game --replay`.
    if name == "play.ron"
        && let Some(out) = std::env::var_os("SF_RECORD_KEEP")
        && text.contains("ticks: 7000")
    {
        std::fs::write(out, &text).unwrap();
    }
    path
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("sf_game_{label}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// The headless checksum after world tick `t` is the sim thread's `checksums[t - 1]`.
fn mismatches(scenario: &Scenario, exit: &SimExit) -> Vec<u64> {
    let outcome = run(
        scenario,
        &RunOptions {
            ticks: scenario.ticks,
            every: 1,
        },
    )
    .unwrap();
    assert_eq!(
        outcome.initial_checksum, exit.checksums[0],
        "the world before the replay's first step is the game's world after its first tick"
    );
    outcome
        .samples
        .iter()
        .filter(|s| s.checksum != exit.checksums[(s.ticks - 1) as usize])
        .map(|s| s.ticks)
        .collect()
}

#[test]
fn a_recorded_match_replays_to_the_same_checksum_at_every_tick() {
    let ticks = 7_000;
    let (exit, cells, result) = play(ticks);
    // The match was played to its end: the script's attack took the opponent's centre and the human won.
    let result = result.expect("the match is decided within the play");
    assert_eq!(result.verdict, sf_sim::Verdict::Won(PlayerId(0)));
    println!("the match was won at tick {}", result.tick.0);
    assert_eq!(exit.ticks, ticks);
    assert!(!exit.panicked);
    let scratch = Scratch::new("record");
    let path = write_record(&exit, cells, &scratch.0, "play.ron");

    let scenario = Scenario::load(&path).expect("the record is a scenario sf_headless loads");
    assert_eq!(scenario.ticks, ticks);
    // The record holds every command that was sent (the last one arrived before the end).
    let kinds: std::collections::BTreeSet<&str> = scenario
        .recorded
        .iter()
        .map(|c| match c.command {
            sf_headless::scenario::ExactCommand::BuildStructure { .. } => "build",
            sf_headless::scenario::ExactCommand::QueueUnit { .. } => "queue",
            sf_headless::scenario::ExactCommand::SetRally { .. } => "rally",
            sf_headless::scenario::ExactCommand::Order { .. } => "order",
            _ => "other",
        })
        .collect();
    assert_eq!(
        kinds,
        ["build", "order", "queue", "rally"].into_iter().collect()
    );
    assert_eq!(scenario.recorded.len(), exit.commands.len());

    let wrong = mismatches(&scenario, &exit);
    assert!(
        wrong.is_empty(),
        "{} of {} ticks differ, the first at tick {:?}",
        wrong.len(),
        ticks - 1,
        wrong.first()
    );
}

/// What the arrival tick is for: the same commands put in before the first step instead of when they
/// arrived (what a scenario's own `commands` do) give other checksums, because the world's checksum
/// includes the commands that wait for their tick. Without `arrived` a replay could not match.
#[test]
fn the_arrival_tick_is_what_makes_the_checksums_equal() {
    let (exit, cells, _) = play(1_200);
    let scratch = Scratch::new("arrival");
    let path = write_record(&exit, cells, &scratch.0, "play.ron");
    let mut scenario = Scenario::load(&path).unwrap();
    assert!(mismatches(&scenario, &exit).is_empty());
    assert!(
        scenario.recorded.iter().any(|c| c.arrived < c.tick),
        "some command waited a tick or more"
    );
    // A command that comes in at tick 1 with its tick left as it was.
    for c in &mut scenario.recorded {
        c.arrived = 1;
    }
    let wrong = mismatches(&scenario, &exit);
    assert!(
        !wrong.is_empty(),
        "putting every command in at once must change a checksum"
    );
}

/// The values of a command survive to the last fixed-point bit: a move to a spot that is not a cell
/// middle is in the record as the raw numbers, and a record with that spot moved by one bit differs.
#[test]
fn fixed_point_positions_are_recorded_exactly() {
    let (exit, cells, _) = play(2_100);
    let scratch = Scratch::new("exact");
    let path = write_record(&exit, cells, &scratch.0, "play.ron");
    let text = std::fs::read_to_string(&path).unwrap();
    let want = format!("Move(({},{}))", 36 * 65_536 + 12_345, 58 * 65_536 + 54_321);
    assert!(text.contains(&want), "{want} is not in the record");
    let mut scenario = Scenario::load(&path).unwrap();
    assert!(mismatches(&scenario, &exit).is_empty());
    let moved = scenario
        .recorded
        .iter_mut()
        .find_map(|c| match &mut c.command {
            sf_headless::scenario::ExactCommand::Order {
                order: sf_headless::scenario::ExactOrder::Move(p),
                ..
            } => Some(p),
            _ => None,
        })
        .expect("the odd move is in the record");
    assert_eq!(*moved, (36 * 65_536 + 12_345, 58 * 65_536 + 54_321));
    moved.0 += 1;
    assert!(
        !mismatches(&scenario, &exit).is_empty(),
        "one bit of a position changes the replay"
    );
}
