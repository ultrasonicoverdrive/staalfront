//! The computer opponent in the game (WP9.1d): the sim thread is the host of the `sf_ai` driver, as
//! `sf_headless::run` is for a scenario.
//!
//! The world is the one `sf_game --match assets/matches/m1_ai.ron` makes (the production builders:
//! `playfield::make_world`, `matchplay::configure` and `schedule`, one step, `matchplay::ais`, then
//! `SimHandle::spawn_with_ais` with `record_commands`, as `Runner` makes it with `--record`). What is
//! checked:
//!
//! * the AI acts through the sim thread: its commands are in the command log under its own player id, it
//!   thinks exactly when `Ai::due` says (the number of thinks is pinned) and builds the opening of its goal
//!   list;
//! * the sim thread and the headless host agree: `sf_headless::run::run_full` with the same AI gives the
//!   same checksum at every tick, so the AI thinks before the step in both (an AI that thought after the
//!   step, or never, or whose commands skipped the log, differs);
//! * a record of the match replays in `sf_headless` **without** the AI to the same checksum at every tick,
//!   because the record holds the AI's commands;
//! * what a think costs, measured on the sim thread (printed with `--nocapture`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use sf_data::map::Map;
use sf_data::units::UnitDefs;
use sf_game::sim_thread::{SimConfig, SimExit, SimHandle};
use sf_game::{bench_scene, matchplay, playfield, record};
use sf_headless::run::{RunOptions, run_full};
use sf_headless::scenario::{ExactCommand, Scenario};
use sf_sim::{PlayerId, Tick, World};

const WAIT: Duration = Duration::from_secs(120);
const AI: PlayerId = PlayerId(1);

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn ai_file() -> PathBuf {
    matchplay::file_path(Some(Path::new("m1_ai")), &assets())
}

/// What `runner::match_scene` hands the sim thread: the world after the first tick, the cells, and the AIs.
fn game(file: &Path) -> (World, (u32, u32), Vec<sf_ai::Ai>) {
    let assets = assets();
    let def = matchplay::load(file).unwrap();
    let map = Map::load(&matchplay::map_dir(&def, &assets)).unwrap();
    let cells = map.size_in_cells();
    let defs = UnitDefs::load(&assets.join("config"), &assets).unwrap();
    matchplay::check(&def, &defs, cells, PlayerId(0)).unwrap();
    let world = playfield::make_world(bench_scene::world_config(cells), &map).unwrap();
    let mut world = matchplay::configure(world, &def, &defs).unwrap();
    let ais = matchplay::ais(&def, &defs, &assets).unwrap();
    matchplay::schedule(&mut world, &def, &defs).unwrap();
    world.step();
    (world, cells, ais)
}

struct Played {
    exit: SimExit,
    cells: (u32, u32),
    thinks: u64,
    think_micros: u64,
    think_max_micros: u64,
    step_micros: u64,
    late_commands: u64,
}

/// Plays `ticks` ticks of `file` on the sim thread, one tick at a time, nobody sending a command (the human
/// is idle: everything in the log of player 1 is the AI's).
fn play(file: &Path, ticks: u64) -> Played {
    let (world, cells, ais) = game(file);
    let mut sim = SimHandle::spawn_with_ais(
        world,
        SimConfig {
            record_commands: true,
            ..SimConfig::manual()
        },
        ais,
    )
    .unwrap();
    for ran in 1..ticks {
        sim.advance(1);
        sim.wait_for_tick(Tick(ran + 1), WAIT).expect("a tick");
    }
    let c = sim.counters();
    let exit = sim.stop().clone();
    assert!(!exit.panicked, "{:?}", exit.panic_message);
    Played {
        exit,
        cells,
        thinks: c.thinks,
        think_micros: c.think_micros,
        think_max_micros: c.think_max_micros,
        step_micros: c.step_micros,
        late_commands: c.late_commands,
    }
}

/// The record of a play as a scenario `sf_headless` loads, with `recorded` as the play's log says.
fn scenario_of(exit: &SimExit, cells: (u32, u32), file: &Path, scratch: &Path) -> Scenario {
    let (text, _) = record::render(exit, cells, &assets(), file).unwrap();
    let path = scratch.join("record.ron");
    std::fs::write(&path, text).unwrap();
    Scenario::load(&path).expect("the record is a scenario sf_headless loads")
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

/// The headless checksum after world tick `t` is the sim thread's `checksums[t - 1]`; the ticks that differ.
fn mismatches(scenario: &Scenario, exit: &SimExit) -> Vec<u64> {
    let finished = run_full(
        scenario,
        &RunOptions {
            ticks: scenario.ticks,
            every: 1,
        },
    )
    .unwrap();
    let outcome = finished.outcome;
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

fn builds_of(exit: &SimExit, player: PlayerId) -> Vec<(u64, u16)> {
    exit.commands
        .iter()
        .filter(|c| c.player == player)
        .filter_map(|c| match &c.command {
            sf_sim::Command::BuildStructure { kind, .. } => Some((c.effective.0, kind.0)),
            _ => None,
        })
        .collect()
}

/// The first player-1 think is at tick 7: `(tick + 3 * player) % 10 == 0` (`assets/ai/normal.ron`:
/// `think_interval: 10`, `stagger: 3`). Of the world ticks 0 to 999 that is 7, 17 ... 997, 100 thinks.
#[test]
fn the_ai_thinks_when_it_is_due_and_its_commands_are_in_the_log_under_its_player() {
    let p = play(&ai_file(), 1_000);
    // `play` steps to tick 1000: the first step ran inside `game`, 999 more here, ticks 0..=999.
    assert_eq!(p.exit.ticks, 1_000);
    assert_eq!(
        p.thinks, 100,
        "one think per due tick of the 1000 ticks run"
    );
    assert!(p.think_micros > 0 && p.think_micros <= p.step_micros);
    assert_eq!(
        p.late_commands, 0,
        "no AI command was moved to a later tick"
    );
    assert!(p.think_max_micros <= p.think_micros);
    let ai: Vec<_> = p.exit.commands.iter().filter(|c| c.player == AI).collect();
    assert!(!ai.is_empty(), "the AI sent commands");
    // The human sent none, and the AI sent builds only (the construction manager is all there is today).
    assert!(p.exit.commands.iter().all(|c| c.player == AI));
    assert!(
        ai.iter()
            .all(|c| matches!(c.command, sf_sim::Command::BuildStructure { .. }))
    );
    // Every command was scheduled for the tick about to run, in the tick the AI was due: before the step.
    let reference = game(&ai_file()).2.remove(0);
    for c in &ai {
        assert_eq!(c.requested, c.effective);
        assert_eq!(
            c.arrived, c.effective,
            "no waiting: scheduled for the next step"
        );
        assert!(
            reference.due(c.effective),
            "tick {} is not a due tick",
            c.effective
        );
    }
    // The opening of the goal list: the power plant first, then the refinery.
    let builds = builds_of(&p.exit, AI);
    assert!(builds.len() >= 2, "{builds:?}");
    // Think 0 (tick 7) is the economy manager's turn, a stub; think 1 (tick 17) the construction manager's.
    assert_eq!(
        builds[0].0, 17,
        "the first build of the goal list: {builds:?}"
    );
}

/// The AI builds its base in the game: by tick 4000 the goal list has got past its first entries
/// (power plant, refinery, barracks, second power plant, ...) on 4000 credits and the refinery's harvester.
#[test]
fn the_ai_builds_its_base_in_the_game() {
    let p = play(&ai_file(), 4_000);
    let builds = builds_of(&p.exit, AI);
    let kinds: std::collections::BTreeSet<u16> = builds.iter().map(|b| b.1).collect();
    assert!(
        kinds.len() >= 4,
        "at least four kinds of structure were ordered by tick 4000: {builds:?}"
    );
    let scratch = Scratch::new("builds");
    let scenario = scenario_of(&p.exit, p.cells, &ai_file(), &scratch.0);
    let finished = run_full(
        &scenario,
        &RunOptions {
            ticks: scenario.ticks,
            every: 100,
        },
    )
    .unwrap();
    let structures_of_ai = finished
        .world
        .snapshot()
        .iter()
        .filter(|v| v.owner == AI && finished.world.footprint_of(v.id).is_some())
        .count();
    println!(
        "tick {}: {} builds ordered ({} kinds), {structures_of_ai} structures of the AI stand",
        p.exit.ticks,
        builds.len(),
        kinds.len()
    );
    assert!(
        structures_of_ai >= 4,
        "{structures_of_ai} structures of the AI (the construction centre included)"
    );
}

/// The same AI on the sim thread and in `sf_headless::run::run_full` gives the same world at every tick:
/// the host drives it the same way in both (think before the step, commands under the AI's player).
#[test]
fn the_sim_thread_and_the_headless_host_run_the_ai_to_the_same_checksums() {
    let ticks = 3_000;
    let p = play(&ai_file(), ticks);
    let scratch = Scratch::new("host");
    // The record's world without its recorded commands: the AI alone makes the difference.
    let mut scenario = scenario_of(
        &SimExit {
            commands: Vec::new(),
            ..p.exit.clone()
        },
        p.cells,
        &ai_file(),
        &scratch.0,
    );
    assert!(scenario.recorded.is_empty());
    scenario.ais = game(&ai_file()).2;
    let wrong = mismatches(&scenario, &p.exit);
    assert!(
        wrong.is_empty(),
        "{} of {} ticks differ, the first at tick {:?}",
        wrong.len(),
        ticks - 1,
        wrong.first()
    );
    // And the check can fail: without the AI the headless run is another match.
    scenario.ais.clear();
    assert!(!mismatches(&scenario, &p.exit).is_empty());
}

/// A record of a match with an AI replays without the AI: the record holds the AI's commands.
#[test]
fn a_recorded_match_with_an_ai_replays_without_the_ai_to_the_same_checksum_at_every_tick() {
    let ticks = 3_000;
    let p = play(&ai_file(), ticks);
    let scratch = Scratch::new("replay");
    let scenario = scenario_of(&p.exit, p.cells, &ai_file(), &scratch.0);
    assert!(scenario.ais.is_empty(), "the replay has no AI");
    assert_eq!(scenario.ticks, ticks);
    // The AI's commands are in the record, as the player-1 builds of the log.
    let recorded_builds = scenario
        .recorded
        .iter()
        .filter(|c| c.player == AI.0 && matches!(c.command, ExactCommand::BuildStructure { .. }))
        .count();
    assert_eq!(recorded_builds, builds_of(&p.exit, AI).len());
    assert!(recorded_builds >= 4);
    let wrong = mismatches(&scenario, &p.exit);
    assert!(
        wrong.is_empty(),
        "{} of {} ticks differ, the first at tick {:?}",
        wrong.len(),
        ticks - 1,
        wrong.first()
    );
    // Without the AI's commands the replay is another match.
    let mut stripped = scenario;
    stripped.recorded.clear();
    assert!(!mismatches(&stripped, &p.exit).is_empty());
}

/// The default match has no AI and nothing changes for it: no AI is made, nobody thinks, and the opponent
/// sends no command.
#[test]
fn the_default_match_has_no_ai_and_the_opponent_sends_nothing() {
    let default = matchplay::file_path(None, &assets());
    let (_, _, ais) = game(&default);
    assert!(ais.is_empty());
    let p = play(&default, 600);
    assert_eq!(p.thinks, 0);
    assert_eq!(p.think_micros, 0);
    assert!(p.exit.commands.is_empty());
}

/// What a think costs, measured on the sim thread's own counters over 20,000 ticks (16 minutes of game time)
/// of the match with the AI. Printed. The only bound asserted, a mean under 5 ms, is a loose sanity bound
/// (five times the profile's budget of 1 ms a think, `assets/ai/normal.ron`, so that a busy shared machine
/// cannot fail it); it pins nothing about the budget, which nothing enforces.
#[test]
fn the_cost_of_a_think_in_the_game_loop_is_measured() {
    let ticks = 20_000;
    let p = play(&ai_file(), ticks);
    let mean = p.think_micros as f64 / p.thinks as f64;
    println!(
        "think cost over {} ticks: {} thinks, mean {:.0} us, worst {} us; step total {} us, thinking is {:.2} % of it",
        ticks,
        p.thinks,
        mean,
        p.think_max_micros,
        p.step_micros,
        100.0 * p.think_micros as f64 / p.step_micros as f64
    );
    assert_eq!(p.thinks, ticks / 10);
    assert!(mean < 5_000.0, "mean think {mean:.0} us");
}

/// The think time reaches the counters and the hitch log (WP9.1d). The test hooks make the think of tick 27
/// take 8 ms and the step of that tick 25 ms more (`SimConfig::think_hitch`, `hitch`): the tick is slow, and
/// the third number of `slow_ticks` is the think. The think is part of the tick's time (the clock starts
/// before it), and the longest think is the longest, not the last.
#[test]
fn a_slow_think_is_in_the_counters_and_in_the_slow_tick_of_the_hitch_log() {
    let (world, _, ais) = game(&ai_file());
    let mut sim = SimHandle::spawn_with_ais(
        world,
        SimConfig {
            think_hitch: Some((27, Duration::from_millis(8))),
            hitch: Some((27, Duration::from_millis(25))),
            ..SimConfig::manual()
        },
        ais,
    )
    .unwrap();
    for ran in 1..40u64 {
        sim.advance(1);
        sim.wait_for_tick(Tick(ran + 1), WAIT).expect("a tick");
    }
    let c = sim.counters();
    assert_eq!(c.thinks, 4, "thinks at 7, 17, 27, 37");
    assert!(
        c.think_max_micros >= 8_000,
        "the longest think is the one of tick 27, not the last: {} us",
        c.think_max_micros
    );
    assert!(c.think_micros >= 8_000 && c.think_micros >= c.think_max_micros);
    let slow = sim.all_slow_ticks();
    let at_27: Vec<_> = slow.iter().filter(|s| s.0 == 27).collect();
    assert_eq!(at_27.len(), 1, "{slow:?}");
    let (_, tick_ms, think_ms) = *at_27[0];
    assert!(think_ms >= 8.0, "the think of the slow tick: {think_ms} ms");
    assert!(
        tick_ms >= 33.0 && tick_ms >= think_ms,
        "the tick took the hitch and the think: {tick_ms} ms"
    );
    // The same tick in the per-tick times (tick number 27 is the 27th, the first being tick 1).
    assert!(sim.tick_ms_since(0)[26] >= 33.0);
    // Another slow tick would carry the think of its own tick and nothing of this one's.
    assert!(slow.iter().filter(|s| s.0 != 27).all(|s| s.2 < 8.0));
    sim.stop();
}

/// Two AIs given in swapped order think in ascending player id. Both are made due on the same ticks
/// (stagger 10 with interval 10), so the order they think in is the order of their commands in the log.
#[test]
fn two_ais_think_in_ascending_player_order_whatever_order_they_are_given() {
    use sf_ai::{Ai, Profile};
    use sf_data::ai_profile::AiProfile;
    let assets = assets();
    let mut def = matchplay::load(&ai_file()).unwrap();
    // Player 0 becomes a second computer opponent (the file check is not asked: this is the host's order).
    def.players[0].controller = sf_data::match_def::Controller::Ai {
        profile: "normal".to_string(),
    };
    let map = Map::load(&matchplay::map_dir(&def, &assets)).unwrap();
    let cells = map.size_in_cells();
    let defs = UnitDefs::load(&assets.join("config"), &assets).unwrap();
    let make_ais = || -> Vec<Ai> {
        let mut profile = AiProfile::embedded();
        profile.stagger = 10;
        let sites = sf_ai::start_sites(&def, &defs);
        [1u8, 0]
            .map(|p| {
                let faction = def.players.iter().find(|q| q.id == p).unwrap().faction;
                let resolved = Profile::resolve(&profile, faction, &defs).unwrap();
                Ai::new(PlayerId(p), resolved, 1, sites.clone())
            })
            .into_iter()
            .collect()
    };
    let ais = make_ais();
    assert_eq!(ais[0].player(), PlayerId(1), "given in swapped order");
    let world = playfield::make_world(bench_scene::world_config(cells), &map).unwrap();
    let mut world = matchplay::configure(world, &def, &defs).unwrap();
    matchplay::schedule(&mut world, &def, &defs).unwrap();
    world.step();
    let mut sim = SimHandle::spawn_with_ais(
        world,
        SimConfig {
            record_commands: true,
            ..SimConfig::manual()
        },
        ais,
    )
    .unwrap();
    for ran in 1..300u64 {
        sim.advance(1);
        sim.wait_for_tick(Tick(ran + 1), WAIT).expect("a tick");
    }
    let exit = sim.stop().clone();
    // Per tick the log has player 0's commands before player 1's, and some tick has both.
    let mut both = 0;
    let mut ticks: Vec<u64> = exit.commands.iter().map(|c| c.effective.0).collect();
    ticks.dedup();
    for t in ticks {
        let players: Vec<u8> = exit
            .commands
            .iter()
            .filter(|c| c.effective.0 == t)
            .map(|c| c.player.0)
            .collect();
        assert!(
            players.windows(2).all(|w| w[0] <= w[1]),
            "tick {t}: {players:?}"
        );
        both += usize::from(players.contains(&0) && players.contains(&1));
    }
    assert!(both > 0, "the two AIs sent commands in the same tick");
}
