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
    // The human sent none, and the AI sent only what its managers may send (`sf_ai`'s whitelist since
    // WP9.1c-2): builds, units to queue, rally points, repairs, (once per match, in a deadlock) a sale and unit
    // orders that are not queued: `Move` (the scout, the gathering army, survivors home), `AttackMove` (the
    // attack, the defence) and `Attack` (the finishing rule, on a structure it sees).
    assert!(p.exit.commands.iter().all(|c| c.player == AI));
    assert!(ai.iter().all(|c| matches!(
        c.command,
        sf_sim::Command::BuildStructure { .. }
            | sf_sim::Command::QueueUnit { .. }
            | sf_sim::Command::SetRally { .. }
            | sf_sim::Command::Repair { .. }
            | sf_sim::Command::Sell { .. }
    ) || matches!(
        &c.command,
        sf_sim::Command::Order {
            order: sf_sim::Order::Move(_) | sf_sim::Order::AttackMove(_) | sf_sim::Order::Attack(_),
            queued: false,
            ids,
        } if !ids.is_empty()
    )));
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
    // Think 0 (tick 7) is the economy manager's turn (no refinery yet, nothing to send); think 1 (tick 17) the
    // construction manager's.
    assert_eq!(
        builds[0].0, 17,
        "the first build of the goal list: {builds:?}"
    );
}

/// The AI builds its base in the game: by tick 9000 the goal list has got past its first entries
/// (power plant, refinery, vehicle factory, second power plant, barracks, ...) on 4000 credits and the
/// refinery's harvester. (9000, was 4000 until WP9.1c-3: the scout, the first unit the barracks makes, is
/// queued at 7,147 now, after the vehicle factory and the second harvester; the barracks stands at 3,017.)
#[test]
fn the_ai_builds_its_base_in_the_game() {
    let p = play(&ai_file(), 9_000);
    // Both the construction and the economy manager act in the game: structures are built and harvesters queued.
    let sent = |f: fn(&sf_sim::Command) -> bool| {
        p.exit
            .commands
            .iter()
            .filter(|c| c.player == AI && f(&c.command))
            .count()
    };
    let builds_sent = sent(|c| matches!(c, sf_sim::Command::BuildStructure { .. }));
    let queued = sent(|c| matches!(c, sf_sim::Command::QueueUnit { .. }));
    // The commands the whitelist above allows are also really sent: the barracks gets its rally point and the
    // scout is ordered to its first leg (a plain `Move`, at 7,317) within these 9000 ticks.
    let rallies = sent(|c| matches!(c, sf_sim::Command::SetRally { .. }));
    let moves = sent(|c| {
        matches!(
            c,
            sf_sim::Command::Order {
                order: sf_sim::Order::Move(_),
                ..
            }
        )
    });
    println!(
        "tick 9000: {builds_sent} builds, {queued} units queued, {rallies} rally points, {moves} moves"
    );
    assert!(
        rallies >= 1 && moves >= 1,
        "{rallies} rallies, {moves} moves"
    );
    assert!(
        builds_sent > 0 && queued > 0,
        "{builds_sent} builds, {queued} queues"
    );
    let builds = builds_of(&p.exit, AI);
    // What the rule promises: the first three goals of the list (power plant, refinery, vehicle factory: 3000 of the
    // 4000 credits) are ordered whatever the harvesters of the economy manager cost afterwards. Since WP9.1b-2
    // the economy spends on harvesters too, so by tick 9000 the later goals are not promised (measured at 9000
    // in WP9.1c-3: 6 kinds, 7 structures).
    let kinds: std::collections::BTreeSet<u16> = builds.iter().map(|b| b.1).collect();
    assert!(
        kinds.len() >= 3,
        "at least three kinds of structure were ordered by tick 9000: {builds:?}"
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
            // Every tick is slow on purpose, so every tick is in the hitch log and a leak always shows.
            extra_tick_time: Duration::from_millis(21),
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
    // Every other tick is slow too (`extra_tick_time`) and carries the think of its own tick and nothing
    // of this one's: a tick without a think has 0, a thinking tick (7, 17, 37) never the number of tick 27.
    let tick_ms = sim.tick_ms_since(0);
    assert_eq!(slow.len(), 39, "every tick is slow: {slow:?}");
    for s in slow.iter().filter(|s| s.0 != 27) {
        assert!(s.2 <= s.1, "a think is part of its tick: {s:?}");
        if s.0 % 10 != 7 {
            assert_eq!(s.2, 0.0, "a tick without a think has no think time: {s:?}");
        }
        assert!(s.2 != think_ms, "the think of tick 27 leaked into {s:?}");
        assert_eq!(s.1, tick_ms[s.0 as usize - 1], "the time of tick {}", s.0);
    }
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

/// The army timing in the real game seat: `m1_ai`, the AI as player 1 (north-east, 4,000 credits) against the
/// idle human, driven the way the sim thread drives it (think before the step). `ai_vs_passive` of
/// `sf_headless` has the AI as player 0 on the human's site against m1's static opponent; this is the seat the
/// game uses. Measured, and pinned (WP9.1c-2, re-measured with the army floor and the attack): the last
/// structure of the goal list before the match ends, the first army unit, the army at the gather cell at ticks
/// 4000, 8000 and 12000, the first attack, and the tick the AI wins (the idle human has only its construction
/// centre, which the attack finds on the human's start site). Asserted beyond the pins, as the rules promise:
/// the first army unit comes long before the 17,867 of WP9.1c-1 (the army floor), the first attack not before
/// `first_attack_tick` and with at least `min_units`, and after the win the AI sends nothing (decision 5).
#[test]
fn the_army_timing_in_the_game_seat() {
    let (mut world, _, mut ais) = game(&ai_file());
    let mut last_build = 0;
    let mut first_army = None;
    let mut first_attack = None;
    let mut after_end = 0;
    let (mut rallies, mut moves) = (0, 0);
    let mut at = Vec::new();
    while world.tick().0 < 20_000 {
        for ai in &mut ais {
            if ai.due(world.tick()) {
                let mut out = Vec::new();
                ai.think(&world, &mut out);
                if world.match_result().is_some() {
                    after_end += out.len();
                }
                for c in out {
                    match &c {
                        sf_sim::Command::BuildStructure { .. } => last_build = world.tick().0,
                        sf_sim::Command::QueueUnit { kind, .. }
                            if [12, 13, 19].contains(&kind.0) && first_army.is_none() =>
                        {
                            first_army = Some(world.tick().0);
                        }
                        sf_sim::Command::SetRally { .. } => rallies += 1,
                        sf_sim::Command::Order {
                            order: sf_sim::Order::Move(_),
                            ..
                        } => moves += 1,
                        sf_sim::Command::Order {
                            order: sf_sim::Order::AttackMove(_),
                            ids,
                            ..
                        } if first_attack.is_none() => {
                            first_attack = Some((world.tick().0, ids.len()));
                        }
                        _ => {}
                    }
                    world.schedule(world.tick(), ai.player(), c).unwrap();
                }
            }
        }
        world.step();
        let t = world.tick().0;
        if [4000, 8000, 12000].contains(&t) {
            let ai = &ais[0];
            let g = ai.state().gather.expect("a gather cell");
            let near = world
                .snapshot()
                .iter()
                .filter(|v| v.owner == AI && world.footprint_of(v.id).is_none())
                .filter(|v| ![11, 15].contains(&v.kind.0))
                .filter_map(|v| world.cell_at(v.pos))
                .filter(|c| c.x.abs_diff(g.x).max(c.y.abs_diff(g.y)) <= 8)
                .count();
            at.push(near);
        }
    }
    let won = world.match_result();
    println!(
        "m1_ai: last structure ordered at {last_build}, first army unit queued at {first_army:?}, army at the gather cell at 4000/8000/12000: {at:?}; first attack {first_attack:?}; result {won:?}; {rallies} rally points, {moves} moves"
    );
    assert!(
        rallies >= 1 && moves >= 1,
        "{rallies} rallies, {moves} moves"
    );
    let profile = &ais[0].state().profile;
    let (attack_tick, size) = first_attack.expect("an attack goes out");
    assert!(attack_tick >= u64::from(profile.army.first_attack_tick));
    assert!(size >= usize::from(profile.army.min_units));
    // The first army unit is queued after the second harvester (1,887 in this seat, `the_opening_in_the_game_seat`).
    assert!(first_army.unwrap() > 1_887);
    assert_eq!(after_end, 0, "nothing sent once the match is decided");
    let won = won.expect("the AI wins against the idle human");
    assert_eq!(won.verdict, sf_sim::Verdict::Won(AI));
    assert_eq!(
        (last_build, first_army, at, attack_tick, won.tick.0),
        (
            M1_AI_LAST_BUILD,
            Some(M1_AI_FIRST_ARMY),
            M1_AI_ARMY.to_vec(),
            M1_AI_FIRST_ATTACK,
            M1_AI_WON
        )
    );
}

/// Measured (WP9.1c-2): the last build before the win, the first army unit, the army at the gather cell at
/// 4000, 8000 and 12000, the first attack, the win. (WP9.1c-1, without the floor and the attack: the last of
/// the eleven builds at 17,817, the first army unit at 17,867, the army 0, 0, 0, 0, 2 at 4000 to 20000.)
/// WP9.1c-3 (the vehicle factory second in the list, the second refinery before the first tower, `attack_value` 2,000,
/// `max_wait` 12,000): last build 3,017 (the barracks; the list then waits for the second refinery while the floor takes
/// the money), first army unit 4,587, army at the gather cell 0, 4, 0 (the wave of 6, value 2,000, leaves at 9,717
/// on `attack_value`), win 11,664. (WP9.1c-2, vehicle factory fifth, `max_wait` 6,000: 1,097, 1,147, [10, 0, 0], 6,037, 8,396.)
// WP9.2a re-pin: 3,017 -> 12,097 (the AI rebuilds a structure while the strike is staging and marching); measured: with the staging, reinforcement and
// partial-defence numbers of the shipped profile set to 0 the old values come back (3,017, win 11,664).
const M1_AI_LAST_BUILD: u64 = 11_657;
const M1_AI_FIRST_ARMY: u64 = 4_547;
const M1_AI_ARMY: [usize; 3] = [0, 4, 0];
const M1_AI_FIRST_ATTACK: u64 = 10_037;
const M1_AI_WON: u64 = 12_673;

/// Drives `ais` on `world` to `until` the way the sim thread does, calling `look` after every step.
fn drive(
    world: &mut World,
    ais: &mut [sf_ai::Ai],
    until: u64,
    mut look: impl FnMut(&World, &[sf_ai::Ai], &[sf_sim::Command]),
) {
    while world.tick().0 < until {
        let mut sent = Vec::new();
        for ai in ais.iter_mut() {
            if ai.due(world.tick()) {
                let mut out = Vec::new();
                ai.think(world, &mut out);
                for c in out {
                    sent.push(c.clone());
                    world.schedule(world.tick(), ai.player(), c).unwrap();
                }
            }
        }
        world.step();
        look(world, ais, &sent);
    }
}

/// Fix round 1 (item 4): the money of the AI in the game seat between ticks 9,000 and 11,800 (WP9.1c-3: the first
/// attack at 9,717, and the win at 11,664; was 5,500 to 8,500 around 6,037 and 8,396). Printed every 250 ticks:
/// credits, free (`budget::free`), what production holds back (`reserve` + the next goal's saving), the next goal,
/// the items queued, the harvesters and the ore delivered so far. A measurement trace, not a pin: its only assert
/// is that rows were printed; the four values `docs/rules/ai.md` quotes are read from its output (fix round 2).
#[test]
fn the_money_of_the_ai_in_the_game_seat_around_the_first_attack() {
    let (mut world, _, mut ais) = game(&ai_file());
    let mut rows = Vec::new();
    drive(&mut world, &mut ais, 12_400, |w, ais, _| {
        let t = w.tick().0;
        if t >= 9_000 && t.is_multiple_of(250) {
            let st = ais[0].state();
            let obs = sf_ai::Observation::build(w, AI, &st.known_structures);
            let lens = sf_ai::Lens::new(w, AI);
            let next = sf_ai::base::next_build(st, &obs, &lens);
            let saving = next.map_or(0, |k| sf_ai::base::saving_for(st, &obs, &lens, k));
            let queued: Vec<u16> = obs
                .own
                .iter()
                .filter_map(|o| o.factory.as_ref())
                .flat_map(|f| f.queue.iter().map(|q| q.0.0))
                .collect();
            let harvesters = obs.own.iter().filter(|o| o.kind.0 == 11).count();
            rows.push((
                t,
                obs.credits,
                sf_ai::budget::free(&obs, &lens),
                u64::from(st.profile.economy.reserve) + saving,
                next.map(|k| k.0),
                queued,
                harvesters,
                w.ore_delivered(AI),
            ));
        }
    });
    for r in &rows {
        println!(
            "tick {}: credits {}, free {}, holdback {} (next goal {:?}), queued {:?}, {} harvesters, ore delivered {}",
            r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7
        );
    }
    assert!(!rows.is_empty());
}

/// Fix round 1 (item 4): `m1_ai` to 30,000 ticks with the attack switched off (`attack_value` and `max_wait`
/// beyond reach: no attack, so no win and the floor stays on while the army is below it), to see when the goal
/// list completes with the floor. Printed: the last build, the army every 4,000 ticks.
#[test]
fn the_goal_list_in_the_game_seat_with_the_floor_and_no_attack() {
    let (mut world, _, mut ais) = game(&ai_file());
    for ai in &mut ais {
        let st = ai.state().clone();
        let mut p = st.profile.clone();
        p.army.attack_value = 1_000_000;
        p.army.max_wait = 72_000;
        *ai = sf_ai::Ai::new(st.player, p, 458758, st.start_sites.clone());
    }
    let (mut last_build, mut last_kind, mut army) = (0, 0, Vec::new());
    drive(&mut world, &mut ais, 30_000, |w, _, sent| {
        for c in sent {
            if let sf_sim::Command::BuildStructure { kind, .. } = c {
                last_build = w.tick().0 - 1;
                last_kind = kind.0;
            }
        }
        if w.tick().0.is_multiple_of(4000) {
            let n = w
                .snapshot()
                .iter()
                .filter(|v| v.owner == AI && [12, 13, 19].contains(&v.kind.0))
                .count();
            army.push((w.tick().0, n));
        }
    });
    println!(
        "m1_ai, no attack: last build at {last_build}, army {army:?}, result {:?}",
        world.match_result()
    );
    assert_eq!(world.match_result(), None);
    // Measured (WP9.1c-3): the list's last build at 19,777 (was 29,777 and not complete: the vehicle factory
    // waited for the 4,000 credits the floor spent on twelve units). The target of 20,000 was met in this seat
    // (not in `ai_vs_passive`, 22,730); the ore of the map is the limit, see docs/rules/ai.md "The opening".
    // WP9.2a-2: 20,297. The new gather cell moves where the later structures are placed (the cell and its ring are
    // kept free of them), so the list's timing shifts: gap rule alone 20,297, gathering of every unit alone 20,537,
    // both 20,297, both off 19,777 (docs/rules/ai.md "The start-site advantage"). The target of 20,000 is no longer
    // met in this seat; that is written down there as a known cost, not hidden by this pin.
    assert_eq!(last_build, 20_297);
    // The last structure ordered is the list's last entry, the advanced defences (kind 7).
    assert_eq!(last_kind, 7);
    assert!(last_build <= 21_000, "{last_build}");
}

/// What `the_opening_in_the_game_seat` reads off a run.
#[derive(Debug, Default)]
struct Opening {
    vehicle_factory_ordered: Option<u64>,
    barracks_ordered: Option<u64>,
    harvester_queued: Option<u64>,
    second_harvester: Option<u64>,
    first_unit_queued: Option<u64>,
    floor_reached: Option<u64>,
    first_wave: Option<(u64, usize, u64)>,
    won: Option<u64>,
    credits: Vec<(u64, u64)>,
    builds: Vec<(u64, u16)>,
}

/// `m1_ai` driven to `ticks` (the attack switched off when `attack` is false: beyond reach, so the floor stays
/// on until it is reached), what the opening did and when.
fn opening(attack: bool, ticks: u64) -> Opening {
    use sf_sim::{Command, Kind, Verdict};
    let (mut world, _, mut ais) = game(&ai_file());
    if !attack {
        for ai in &mut ais {
            let st = ai.state().clone();
            let mut p = st.profile.clone();
            p.army.attack_value = 1_000_000;
            p.army.max_wait = 72_000;
            *ai = sf_ai::Ai::new(st.player, p, 458758, st.start_sites.clone());
        }
    }
    let mut o = Opening::default();
    let mut launched = 0;
    drive(&mut world, &mut ais, ticks, |w, ais, sent| {
        let t = w.tick().0;
        for c in sent {
            match c {
                Command::BuildStructure { kind, .. } => {
                    o.builds.push((t - 1, kind.0));
                    if kind.0 == 4 {
                        o.vehicle_factory_ordered.get_or_insert(t - 1);
                    }
                    if kind.0 == 3 {
                        o.barracks_ordered.get_or_insert(t - 1);
                    }
                }
                Command::QueueUnit { kind, .. } => {
                    if kind.0 == 11 {
                        o.harvester_queued.get_or_insert(t - 1);
                    }
                    if [12, 13, 19].contains(&kind.0) {
                        o.first_unit_queued.get_or_insert(t - 1);
                    }
                }
                _ => {}
            }
        }
        if t.is_multiple_of(500) && t <= 12_000 {
            o.credits.push((t, w.credits(AI)));
        }
        let cost = |k: Kind| {
            u64::from(
                w.production_table()
                    .and_then(|t| t.get(k))
                    .map_or(0, |u| u.cost),
            )
        };
        let snap = w.snapshot();
        if snap
            .iter()
            .filter(|v| v.owner == AI && v.kind.0 == 11)
            .count()
            >= 2
        {
            o.second_harvester.get_or_insert(t);
        }
        let value: u64 = snap
            .iter()
            .filter(|v| v.owner == AI && [12, 13, 19].contains(&v.kind.0))
            .map(|v| cost(v.kind))
            .sum();
        let st = ais[0].state();
        if value >= u64::from(st.profile.army.army_floor_value) {
            o.floor_reached.get_or_insert(t);
        }
        if st.attacks_launched > launched {
            launched = st.attacks_launched;
            let v = snap
                .iter()
                .filter(|v| st.attackers().contains(&v.id))
                .map(|v| cost(v.kind))
                .sum();
            o.first_wave.get_or_insert((t - 1, st.attackers().len(), v));
        }
        if o.won.is_none()
            && let Some(r) = w.match_result()
            && r.verdict == Verdict::Won(AI)
        {
            o.won = Some(r.tick.0);
        }
    });
    o
}

/// WP9.1c-3, the opening in the game seat (`m1_ai`, shipped profile; 4,000 credits, one free harvester):
/// the vehicle factory is the third build, the bought harvester is queued as soon as it stands and the second
/// harvester exists at 1,887 (well before 4,000), the barracks comes after it, and the first unit of the mix is
/// queued at 4,547 (4,587 before WP9.2a-2). The army floor (2,000) is reached at 9,654 and the first wave leaves at
/// 10,037 on `attack_value` (2,000, equal to the floor) with 6 units worth 2,000; the match is won at 12,673. Credits every 500 ticks are
/// pinned (they sit near the reserve while the floor spends everything above it).
#[test]
fn the_opening_in_the_game_seat() {
    let o = opening(true, 12_800);
    println!("{o:#?}");
    assert_eq!(o.vehicle_factory_ordered, Some(817));
    assert_eq!(o.harvester_queued, Some(1_487));
    assert_eq!(o.second_harvester, Some(1_887));
    assert_eq!(o.barracks_ordered, Some(2_977));
    assert_eq!(o.first_unit_queued, Some(4_547));
    assert_eq!(o.floor_reached, Some(9_654));
    assert_eq!(o.first_wave, Some((10_037, 6, 2_000)));
    assert_eq!(o.won, Some(12_673)); // WP9.2a: 11,664 before the staging (the strike waits at its cell); WP9.2a-2: 12,312 before the gather rules
    assert_eq!(
        o.builds[..5],
        [(17, 1), (257, 2), (817, 4), (2_137, 1), (2_977, 3)]
    );
    // What the rules promise: two harvesters by 4,000 and before the first unit of the mix.
    assert!(o.second_harvester.unwrap() < 4_000);
    assert!(o.second_harvester < o.first_unit_queued);
    // The first wave leaves on `attack_value`, long before `max_wait` ends the floor.
    let profile = game(&ai_file()).2[0].state().profile.clone();
    assert_eq!(profile.army.attack_value, profile.army.army_floor_value);
    assert!(o.first_wave.unwrap().0 + 1_000 < u64::from(profile.army.max_wait));
    assert_eq!(
        o.credits[..24],
        [
            (500, 3_157),
            (1_000, 2_091),
            (1_500, 1_165),
            (2_000, 100),
            (2_500, 0),
            (3_000, 368),
            (3_500, 10),
            (4_000, 400),
            (4_500, 780),
            (5_000, 300),
            (5_500, 400),
            (6_000, 470),
            (6_500, 800),
            (7_000, 966),
            (7_500, 400),
            (8_000, 600),
            (8_500, 800),
            (9_000, 800),
            (9_500, 300),
            (10_000, 400),
            (10_500, 610),
            (11_000, 800),
            (11_500, 1_000),
            (12_000, 432),
        ]
    );
}
