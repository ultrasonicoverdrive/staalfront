//! Saving and loading on the game's sim thread (WP13.1e-2, `docs/rules/save.md` "The save point in the game").
//!
//! Every test goes through the real `SimHandle` with manual pacing (or the realtime clock where the test is
//! about it), on worlds built with the production builders of `runner::match_scene` (`playfield::make_world`,
//! `matchplay::configure`, `matchplay::ais`, `matchplay::schedule`, one step). A save is taken with
//! `SimHandle::save`, made into a file with `savefile::write_bytes`, loaded with `savefile::load` onto the fresh
//! world and AIs a load would build, and played on with `SimHandle::spawn_restored`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sf_data::map::Map;
use sf_data::units::UnitDefs;
use sf_game::sim_snapshot::SimSnapshot;
use sf_game::sim_thread::{SaveParts, SimConfig, SimExit, SimHandle, SimSaveError};
use sf_game::{bench_scene, matchplay, playfield, record};
use sf_headless::run::{RunOptions, run};
use sf_headless::savefile::{self, Identity, Loaded, SaveKind};
use sf_headless::scenario::Scenario;
use sf_sim::{Cell, Command, PlayerId, Tick, World};

const WAIT: Duration = Duration::from_secs(120);

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn match_file(name: &str) -> PathBuf {
    matchplay::file_path(Some(Path::new(name)), &assets())
}

/// The world of `file` before its starting objects and its first tick, and its fresh AIs: what a load builds.
fn fresh(file: &Path) -> (World, Vec<sf_ai::Ai>, (u32, u32)) {
    let assets = assets();
    let def = matchplay::load(file).unwrap();
    let map = Map::load(&matchplay::map_dir(&def, &assets)).unwrap();
    let cells = map.size_in_cells();
    let defs = UnitDefs::load(&assets.join("config"), &assets).unwrap();
    let world = playfield::make_world(bench_scene::world_config(cells), &map).unwrap();
    let world = matchplay::configure(world, &def, &defs).unwrap();
    let ais = matchplay::ais(&def, &defs, &assets).unwrap();
    (world, ais, cells)
}

/// What `runner::match_scene` hands the sim thread: the fresh world with the starting objects and one tick run.
fn game(file: &Path) -> (World, Vec<sf_ai::Ai>, (u32, u32)) {
    let assets = assets();
    let def = matchplay::load(file).unwrap();
    let defs = UnitDefs::load(&assets.join("config"), &assets).unwrap();
    let (mut world, ais, cells) = fresh(file);
    matchplay::schedule(&mut world, &def, &defs).unwrap();
    world.step();
    (world, ais, cells)
}

/// A match that can be saved, stepped by hand.
fn config() -> SimConfig {
    SimConfig {
        saves: true,
        ..SimConfig::manual()
    }
}

fn spawn(file: &Path) -> SimHandle {
    let (world, ais, _) = game(file);
    SimHandle::spawn_with_ais(world, config(), ais).unwrap()
}

/// Steps until the world has run `tick` ticks.
fn step_to(sim: &SimHandle, tick: u64) {
    let now = sim.latest().tick().0;
    assert!(now <= tick, "at {now}, asked for {tick}");
    if now < tick {
        sim.advance(u32::try_from(tick - now).unwrap());
        sim.wait_for_tick(Tick(tick), WAIT).expect("the ticks ran");
    }
}

fn identity(file: &Path, world: &World) -> Identity {
    let name = record::match_path_text(file, &assets());
    Identity::of(SaveKind::Match, &name, None, 0, world)
}

/// The save file of `parts` for the match `file`.
fn file_of(file: &Path, parts: &SaveParts) -> Vec<u8> {
    let (world, ..) = fresh(file);
    savefile::write_bytes(
        &identity(file, &world),
        parts.rules_digest,
        parts.tick,
        parts.checksum,
        &parts.world,
        &parts.ais,
        &parts.log,
    )
    .unwrap()
}

/// The save file loaded as `sf_game` will load it (13.1e-3): onto the fresh world and AIs of the match.
fn load(file: &Path, bytes: &[u8]) -> Loaded {
    let (world, ais, _) = fresh(file);
    let expected = identity(file, &world);
    savefile::load(bytes, &expected, world, ais).unwrap()
}

/// A new sim thread on the save `parts` of the match `file`, through the save file.
fn restored(file: &Path, parts: &SaveParts) -> SimHandle {
    let loaded = load(file, &file_of(file, parts));
    SimHandle::spawn_restored(loaded.world, config(), loaded.ais, loaded.log).unwrap()
}

/// The log as a save holds it.
fn saved_log(exit: &SimExit) -> Vec<savefile::LoggedCommand> {
    exit.commands.iter().map(|c| c.to_saved()).collect()
}

/// The ticks in `from..to` (ticks run) after which the checksums of `a` and `b` differ, or are missing.
fn differing(a: &SimExit, b: &SimExit, from: u64, to: u64) -> Vec<u64> {
    (from..to)
        .filter(|&t| {
            let (x, y) = (a.checksum_after(Tick(t)), b.checksum_after(Tick(t)));
            x.is_none() || x != y
        })
        .collect()
}

// --- Save, load, play on -------------------------------------------------------------------------------------

/// `m1_duel`, the computer against the computer: saved at three ticks on the running thread, each save loaded
/// into a new thread that plays on to the end. Every restored thread has the uninterrupted checksum after
/// every tick, the same log (since tick 0) and the same AI state at the end.
#[test]
fn an_ai_match_saved_and_loaded_plays_on_tick_for_tick() {
    let file = match_file("m1_duel");
    let end = 3_000;
    let saves = [97, 600, 1_111, 2_222];
    let mut sim = spawn(&file);
    let mut parts = Vec::new();
    for &at in &saves {
        step_to(&sim, at);
        parts.push(sim.save(WAIT).unwrap());
    }
    step_to(&sim, end);
    let last_save = sim.save(WAIT).unwrap();
    let straight = sim.stop().clone();
    assert!(!straight.panicked);
    assert_eq!(straight.checksums_start, Tick(0));
    let ai_commands = straight
        .commands
        .iter()
        .filter(|c| c.arrived.0 < end)
        .count();
    assert!(ai_commands > 10, "{ai_commands} AI commands");

    for p in &parts {
        assert_eq!(p.ais.len(), 2, "both AIs are saved");
        assert_eq!(
            straight.checksum_after(Tick(p.tick.0 - 1)),
            Some(p.checksum)
        );
        let mut back = restored(&file, p);
        assert_eq!(back.latest().tick(), p.tick);
        // The snapshot's checksum is the last entry of the history, which a restored world begins empty: one of
        // the reports lost until the first step (`LOST_UNTIL_THE_FIRST_STEP`).
        assert_eq!(back.latest().checksum(), None);
        step_to(&back, end);
        let after = back.save(WAIT).unwrap();
        let exit = back.stop().clone();
        assert_eq!(
            exit.checksums_start, p.tick,
            "the history starts at the save"
        );
        assert_eq!(exit.checksums.len() as u64, end - p.tick.0);
        let wrong = differing(&exit, &straight, p.tick.0, end);
        assert!(
            wrong.is_empty(),
            "save at {}: {} ticks differ, the first {:?}",
            p.tick,
            wrong.len(),
            wrong.first()
        );
        assert_eq!(saved_log(&exit), saved_log(&straight), "the full log");
        assert_eq!(
            after,
            SaveParts {
                took: after.took,
                ..last_save.clone()
            }
        );
    }
}

/// The human's commands around a save, on `m1_ai` (the human against the computer): a command sent before the
/// save is in the save's log and in its world's queue, one sent after it (for the same tick of arrival) is in
/// neither; after the load the second is sent again, and the restored game equals the one that went on.
#[test]
fn commands_before_the_save_are_in_it_commands_after_it_are_not() {
    let file = match_file("m1_ai");
    let (at, end) = (500, 900);
    // The tick before the save is slow, so the save and the command after it are both waiting in the channel
    // while it runs: the order of the messages alone puts the command after the save.
    let (world, ais, _) = game(&file);
    let slow = SimConfig {
        hitch: Some((at - 1, Duration::from_millis(300))),
        ..config()
    };
    let mut sim = SimHandle::spawn_with_ais(world, slow, ais).unwrap();
    step_to(&sim, at - 1);
    let before = Command::Stop { ids: Vec::new() };
    let after = Command::Stop { ids: Vec::new() };
    sim.send_at(Tick(at + 2), PlayerId(0), before.clone());
    sim.advance(1);
    let request = sim.request_save();
    sim.send_at(Tick(at + 3), PlayerId(0), after.clone());
    let parts = request.wait(WAIT).unwrap();
    assert_eq!(parts.tick, Tick(at));
    let human: Vec<_> = parts
        .log
        .iter()
        .filter(|c| c.player == PlayerId(0))
        .map(|c| (c.arrived.0, c.effective.0))
        .collect();
    assert_eq!(
        human,
        [(at - 1, at + 2)],
        "the command before the save, only"
    );
    let loaded = load(&file, &file_of(&file, &parts));
    let pending: Vec<_> = loaded
        .world
        .pending_commands()
        .iter()
        .filter(|(_, p, _)| *p == PlayerId(0))
        .map(|(t, ..)| t.0)
        .collect();
    assert_eq!(
        pending,
        [at + 2],
        "the queue holds the command before the save, only"
    );
    step_to(&sim, end);
    let straight = sim.stop().clone();

    let mut back =
        SimHandle::spawn_restored(loaded.world, config(), loaded.ais, loaded.log).unwrap();
    back.send_at(Tick(at + 3), PlayerId(0), after);
    step_to(&back, end);
    let exit = back.stop().clone();
    assert!(differing(&exit, &straight, at, end).is_empty());
    assert_eq!(saved_log(&exit), saved_log(&straight));
    let human: Vec<_> = exit
        .commands
        .iter()
        .filter(|c| c.player == PlayerId(0))
        .map(|c| (c.arrived.0, c.effective.0))
        .collect();
    assert_eq!(human, [(at - 1, at + 2), (at, at + 3)]);
}

/// The full command log is kept by a match that can be saved, without `--record`: the same log as with it.
#[test]
fn a_match_keeps_the_full_log_without_record() {
    let file = match_file("m1_duel");
    let play = |config: SimConfig| {
        let (world, ais, _) = game(&file);
        let mut sim = SimHandle::spawn_with_ais(world, config, ais).unwrap();
        step_to(&sim, 2_000);
        sim.stop().clone()
    };
    let saving = play(config());
    let recording = play(SimConfig {
        record_commands: true,
        ..SimConfig::manual()
    });
    let neither = play(SimConfig::manual());
    assert!(recording.commands.len() > 5);
    assert_eq!(saving.commands, recording.commands);
    assert!(
        neither.commands.is_empty(),
        "the sandbox's log is off as before"
    );
    assert_eq!(
        saving.checksums, neither.checksums,
        "the log changes no checksum"
    );
}

// --- Where and when a save is answered --------------------------------------------------------------------

/// A save while held answers at once, at the tick where the hold began, and twice gives the same parts; a
/// thread with its own clock answers between two ticks, and the save loads to its tick and checksum. Covers the
/// four places a message is handled: by hand, held, the clock's wait, the drain before a tick.
#[test]
fn a_save_is_answered_held_and_on_the_clock() {
    let file = match_file("m1_ai");
    // Stepped by hand, held.
    let sim = spawn(&file);
    step_to(&sim, 40);
    sim.set_paused(true);
    sim.advance(5);
    let a = sim.save(WAIT).unwrap();
    let b = sim.save(WAIT).unwrap();
    assert_eq!(a.tick, Tick(40), "held: no tick ran");
    assert_eq!(a, SaveParts { took: a.took, ..b });

    // The clock, held before its start (`start_paused`) and held by the pause menu.
    let (world, ais, _) = game(&file);
    let clock = SimConfig {
        saves: true,
        start_paused: true,
        period: Duration::from_millis(2),
        ..SimConfig::realtime()
    };
    let sim = SimHandle::spawn_with_ais(world, clock, ais).unwrap();
    assert_eq!(sim.save(WAIT).unwrap().tick, Tick(1), "before the start");
    sim.start();
    sim.wait_for_tick(Tick(60), WAIT).unwrap();
    for _ in 0..20 {
        let parts = sim.save(WAIT).unwrap();
        let loaded = load(&file, &file_of(&file, &parts));
        assert_eq!(loaded.world.tick(), parts.tick);
        assert_eq!(loaded.world.checksum(), parts.checksum);
    }
    sim.set_paused(true);
    let held = sim.save(WAIT).unwrap();
    std::thread::sleep(Duration::from_millis(30));
    let again = sim.save(WAIT).unwrap();
    assert_eq!(held.tick, again.tick, "held: the world stands");
    assert_eq!(held.world, again.world);
}

/// Only a match is saved: a thread that is not one refuses with its message. A thread that died or was stopped
/// answers `Stopped`, one that is busy past the time given `Timeout`: an error, never a hang.
#[test]
fn a_save_is_refused_outside_a_match_and_never_hangs() {
    let file = match_file("m1_ai");
    let (world, ais, _) = game(&file);
    let mut sandbox = SimHandle::spawn_with_ais(world, SimConfig::manual(), ais).unwrap();
    let refused = sandbox.save(WAIT).unwrap_err();
    assert_eq!(refused, SimSaveError::NotAMatch);
    assert_eq!(refused.to_string(), "only a match can be saved");
    sandbox.stop();
    assert_eq!(sandbox.save(WAIT), Err(SimSaveError::Stopped));

    let (world, ais, _) = game(&file);
    let dies = SimConfig {
        panic_at: Some(3),
        ..config()
    };
    let mut dead = SimHandle::spawn_with_ais(world, dies, ais).unwrap();
    dead.advance(5);
    let began = std::time::Instant::now();
    while dead.is_running() {
        assert!(began.elapsed() < WAIT);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(dead.save(WAIT), Err(SimSaveError::Stopped));
    assert!(dead.stop().panicked);

    let (world, ais, _) = game(&file);
    let slow = SimConfig {
        extra_tick_time: Duration::from_millis(100),
        ..config()
    };
    let busy = SimHandle::spawn_with_ais(world, slow, ais).unwrap();
    busy.advance(10);
    let began = std::time::Instant::now();
    assert_eq!(
        busy.save(Duration::from_millis(50)),
        Err(SimSaveError::Timeout)
    );
    assert!(began.elapsed() < Duration::from_secs(5));
    // The save that timed out is answered later to nobody; the next one waits its turn.
    assert_eq!(busy.save(WAIT).unwrap().tick, Tick(11));
    assert_eq!(
        SimSaveError::Timeout.to_string(),
        "the simulation did not answer in time"
    );
    assert_eq!(
        SimSaveError::Stopped.to_string(),
        "the simulation has stopped"
    );
}

// --- The game record after a load -------------------------------------------------------------------------

/// The script of the pinned `match` scenario for player 0 (the human's opening: build, produce, rally).
fn human_script() -> Vec<(u64, Command)> {
    let s = Scenario::load(&assets().join("scenarios/match.ron")).unwrap();
    s.build_world()
        .unwrap()
        .pending_commands()
        .iter()
        .filter(|(tick, player, _)| tick.0 >= 1 && *player == PlayerId(0))
        .map(|(tick, _, command)| (tick.0, command.clone()))
        .collect()
}

/// Steps `sim` to `to`, sending each command of `script` one tick ahead, as a click is.
fn play_script(sim: &SimHandle, script: &[(u64, Command)], to: u64) {
    let mut now = sim.latest().tick().0;
    while now < to {
        for (tick, command) in script.iter().filter(|(t, _)| *t == now + 1) {
            sim.send_at(Tick(*tick), PlayerId(0), command.clone());
        }
        step_to(sim, now + 1);
        now += 1;
    }
}

/// The record (`--record`) of a game that was saved, loaded and played on replays in `sf_headless` to the
/// game's checksum at every tick: before the save the thread that was saved, after it the restored one. And a
/// record is resumed after a save by the count of the save's log (`record::commands_after_save`), not by its
/// tick: a command that arrived at the save tick after the save is in the record and not in the save, and fed
/// from the record into the restored thread it gives the game again.
#[test]
fn the_record_of_a_loaded_game_replays_and_resumes_by_count() {
    let file = match_file("m1_ai");
    let (at, end) = (970, 1_600);
    let script = human_script();
    assert!(script.iter().any(|(t, _)| *t < at) && script.iter().any(|(t, _)| *t > at));
    let mut sim = spawn(&file);
    play_script(&sim, &script, at);
    let parts = sim.save(WAIT).unwrap();
    // After the save, at the save tick: in the record of the game that went on, not in the save.
    let late = Command::Stop { ids: Vec::new() };
    sim.send_at(Tick(at + 1), PlayerId(0), late.clone());
    play_script(&sim, &script, end);
    let straight = sim.stop().clone();

    let mut back = restored(&file, &parts);
    back.send_at(Tick(at + 1), PlayerId(0), late);
    play_script(&back, &script, end);
    let exit = back.stop().clone();
    assert!(differing(&exit, &straight, at, end).is_empty());

    // The record of the restored game: the full log since tick 0.
    let scratch = std::env::temp_dir().join(format!("sf_game_save_record_{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let (_, _, cells) = fresh(&file);
    let (text, n) = record::render(&exit, cells, &assets(), &file).unwrap();
    let path = scratch.join("loaded.ron");
    std::fs::write(&path, text).unwrap();
    let scenario = Scenario::load(&path).unwrap();
    std::fs::remove_dir_all(&scratch).ok();
    assert_eq!(n, exit.commands.len());
    assert_eq!(scenario.ticks, end);
    let outcome = run(
        &scenario,
        &RunOptions {
            ticks: end,
            every: 1,
        },
    )
    .unwrap();
    let wrong: Vec<u64> = outcome
        .samples
        .iter()
        .filter(|s| {
            let t = Tick(s.ticks - 1);
            let game = exit.checksum_after(t).or(straight.checksum_after(t));
            game != Some(s.checksum)
        })
        .map(|s| s.ticks)
        .collect();
    assert!(
        wrong.is_empty(),
        "{} ticks differ, the first {:?}",
        wrong.len(),
        wrong.first()
    );

    // Resuming the record from the save: by count.
    let rest = record::commands_after_save(&scenario.recorded, &parts.log).unwrap();
    assert_eq!(rest.len(), scenario.recorded.len() - parts.log.len());
    let by_tick = scenario.recorded.iter().filter(|c| c.arrived > at).count();
    assert_eq!(
        rest.len(),
        by_tick + 1,
        "the late command arrived at the save tick"
    );
    assert_eq!((rest[0].arrived, rest[0].tick), (at, at + 1));
    // Fed as `--replay` feeds a record: without the computer opponents, whose commands the record holds.
    let loaded = load(&file, &file_of(&file, &parts));
    let mut fed =
        SimHandle::spawn_restored(loaded.world, config(), Vec::new(), loaded.log).unwrap();
    for c in rest {
        step_to(&fed, c.arrived);
        let command = c.command.to_command();
        fed.send_at(Tick(c.tick), PlayerId(c.player), command);
    }
    step_to(&fed, end);
    let fed = fed.stop().clone();
    assert!(differing(&fed, &straight, at, end).is_empty());
    // A record that does not begin with the save's log is refused.
    let mut other = scenario.recorded.clone();
    other.remove(0);
    assert!(record::commands_after_save(&other, &parts.log).is_err());
}

// --- What a new thread starts with --------------------------------------------------------------------------

/// The ore and fog of `snap`, as the renderer reads them.
/// `(ore version, ore cells, fog version, fog words)`.
type OreAndFog = (Option<u64>, Vec<(u32, u32, u16)>, Option<u64>, Vec<u64>);

fn ore_and_fog(snap: &SimSnapshot) -> OreAndFog {
    let (visible, explored) = snap.fog_words();
    let mut fog = visible.to_vec();
    fog.extend_from_slice(explored);
    (
        snap.ore_version(),
        snap.ore_cells().to_vec(),
        snap.fog_version(),
        fog,
    )
}

/// Two branches of one game that reach the same ore and fog versions with other contents (each takes the
/// same amount of ore from another cell, and each places its first structure on another cell): A runs, B's
/// save is loaded over it. The first snapshot of the loaded game has B's ore and fog, not A's, although the
/// versions are A's: the snapshot buffers of a new thread start blank (`docs/rules/save.md`, the host caches).
#[test]
fn the_first_snapshot_after_a_load_shows_the_loaded_ore_and_fog() {
    let file = match_file("m1");
    let script = human_script();
    let build = script
        .iter()
        .find(|(_, c)| matches!(c, Command::BuildStructure { .. }))
        .cloned()
        .expect("the script builds");
    let fork = 300;
    let base = spawn(&file);
    step_to(&base, fork);
    let parts = base.save(WAIT).unwrap();
    drop(base);
    let ore = base_ore(&restored(&file, &parts).latest());
    let branch = |cell: Cell, shift: u32| {
        let sim = restored(&file, &parts);
        sim.send_at(
            Tick(fork + 1),
            PlayerId(0),
            Command::TakeOre { cell, amount: 5 },
        );
        let mut moved = build.1.clone();
        if let Command::BuildStructure { cell: c, .. } = &mut moved {
            *c = Cell::new(c.x + shift, c.y);
        }
        sim.send_at(Tick(fork + 1), PlayerId(0), moved);
        sim
    };
    let a = branch(ore[0], 0);
    let b = branch(ore[1], 4);
    // Step both until the versions are equal and the contents are not.
    let mut found = None;
    for t in fork + 1..fork + 800 {
        step_to(&a, t);
        step_to(&b, t);
        let (sa, sb) = (ore_and_fog(&a.latest()), ore_and_fog(&b.latest()));
        if sa.0 == sb.0 && sa.1 != sb.1 && sa.2 == sb.2 && sa.3 != sb.3 {
            found = Some((t, sa, sb));
            break;
        }
    }
    let (t, sa, sb) = found.expect("a tick with equal versions and other contents");
    println!(
        "branches at tick {t}: ore version {:?}, fog version {:?}",
        sa.0, sa.2
    );
    assert_ne!(sa, sb);
    let save_b = b.save(WAIT).unwrap();
    let loaded = restored(&file, &save_b);
    drop(a);
    assert_eq!(ore_and_fog(&loaded.latest()), sb);
}

/// Two ore cells with ore of the snapshot.
fn base_ore(snap: &SimSnapshot) -> [Cell; 2] {
    let cells: Vec<_> = snap
        .ore_cells()
        .iter()
        .filter(|c| c.2 > 10)
        .map(|c| Cell::new(c.0, c.1))
        .collect();
    assert!(cells.len() >= 2);
    [cells[0], cells[cells.len() - 1]]
}

/// What the first snapshot (and the handle) of a loaded game does not have until its first step: the reports of
/// the last tick, which are not in a save (`docs/rules/save.md`, the "Host side, lost for one tick" paragraphs).
/// Everything else the snapshot shows is the loaded game's.
const LOST_UNTIL_THE_FIRST_STEP: [&str; 7] = [
    "checksum",
    "stalled",
    "deaths",
    "sales",
    "repairs waiting for money",
    "rejected builds",
    "effects events",
];

/// The parts of a snapshot that a load keeps, by name, as text.
fn kept(snap: &SimSnapshot) -> Vec<(&'static str, String)> {
    let players = [PlayerId(0), PlayerId(1)];
    vec![
        ("tick", format!("{:?}", snap.tick())),
        ("views", format!("{:?}", snap.views())),
        ("structure cells", format!("{:?}", snap.structure_cells())),
        ("constructions", format!("{:?}", snap.constructions())),
        ("factories", format!("{:?}", snap.factories())),
        (
            "credits",
            format!("{:?}", players.map(|p| snap.credits_of(p))),
        ),
        ("power", format!("{:?}", players.map(|p| snap.power_of(p)))),
        ("storage", format!("{:?}", snap.storage())),
        ("ore", format!("{:?}", snap.ore_cells())),
        ("fog", format!("{:?}", snap.fog_words())),
        ("wrecks", format!("{:?}", snap.wrecks())),
        ("match result", format!("{:?}", snap.match_result())),
        ("stores", format!("{:?}", snap.stores())),
    ]
}

/// The reports of the last tick, by the names of [`LOST_UNTIL_THE_FIRST_STEP`]: empty is `None`.
fn reports(sim: &SimHandle) -> Vec<(&'static str, Option<String>)> {
    let snap = sim.latest();
    let some = |empty: bool, text: String| (!empty).then_some(text);
    let waiting: Vec<_> = snap
        .repairs()
        .iter()
        .filter(|(_, r)| format!("{r:?}").contains("WaitingForMoney"))
        .collect();
    let rejected = sim.rejected_builds();
    let mut fx = Vec::new();
    sim.drain_fx(&mut fx);
    vec![
        ("checksum", snap.checksum().map(|c| c.to_string())),
        (
            "stalled",
            some(snap.stalled().is_empty(), format!("{:?}", snap.stalled())),
        ),
        (
            "deaths",
            some(snap.deaths().is_empty(), format!("{:?}", snap.deaths())),
        ),
        (
            "sales",
            some(snap.sales().is_empty(), format!("{:?}", snap.sales())),
        ),
        (
            "repairs waiting for money",
            some(waiting.is_empty(), format!("{waiting:?}")),
        ),
        (
            "rejected builds",
            some(rejected.0 == 0, format!("{rejected:?}")),
        ),
        (
            "effects events",
            some(fx.is_empty(), format!("{} events", fx.len())),
        ),
    ]
}

/// The first snapshot of a loaded game against the last one of the game that was saved: everything is the
/// same except the named list of reports of the last tick, which are empty until the first step and are back
/// after it.
#[test]
fn the_reports_of_the_last_tick_are_the_named_list_lost_on_a_load() {
    let file = match_file("m1_ai");
    let script = human_script();
    let at = 1_400;
    let sim = spawn(&file);
    play_script(&sim, &script, at);
    let parts = sim.save(WAIT).unwrap();
    let before = sim.latest();
    let before_reports = reports(&sim);
    let back = restored(&file, &parts);
    let first = back.latest();
    assert_eq!(kept(&first), kept(&before), "what a load keeps");
    let names: Vec<_> = reports(&back).iter().map(|r| r.0).collect();
    assert_eq!(names, LOST_UNTIL_THE_FIRST_STEP);
    for (name, report) in reports(&back) {
        assert_eq!(report, None, "{name} is empty after a load");
    }
    let had: Vec<_> = before_reports
        .iter()
        .filter(|r| r.1.is_some())
        .map(|r| r.0)
        .collect();
    println!("reports at the save that the load lost: {had:?}");
    assert!(had.contains(&"checksum"));
    // One step later the two games report alike again.
    step_to(&sim, at + 1);
    step_to(&back, at + 1);
    assert_eq!(kept(&back.latest()), kept(&sim.latest()));
    assert_eq!(back.latest().checksum(), sim.latest().checksum());
    assert_eq!(back.latest().stalled(), sim.latest().stalled());
    assert_eq!(back.latest().sales(), sim.latest().sales());
}

// --- What a save costs --------------------------------------------------------------------------------------

/// The cost of a save on the sim thread (`World::save` and the AI saves, `SaveParts::took`) and the size of the
/// command log, on `m1_duel` (the computer against the computer) over `SF_SAVE_COST_TICKS` ticks (default 12,000,
/// ten minutes of game time), saved every 250 ticks. Printed for the PR and `docs/rules/save.md`; release only:
/// `cargo test --release -p sf_game --test save_sim_thread -- --ignored --nocapture save_cost`.
#[test]
#[ignore = "a measurement: run in release with --nocapture"]
fn save_cost_and_log_size() {
    let ticks: u64 = std::env::var("SF_SAVE_COST_TICKS")
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or(12_000);
    let file = match_file("m1_duel");
    let mut sim = spawn(&file);
    let mut took = Vec::new();
    let mut world_bytes = Vec::new();
    let mut last = None;
    let mut at = 250;
    while at <= ticks {
        step_to(&sim, at);
        let parts = sim.save(WAIT).unwrap();
        took.push(parts.took);
        world_bytes.push(parts.world.len());
        last = Some(parts);
        at += 250;
    }
    let exit = sim.stop().clone();
    let parts = last.unwrap();
    took.sort();
    let micros = |d: Duration| d.as_micros();
    println!(
        "save on the sim thread over {} saves: median {} us, worst {} us; world {}..{} bytes",
        took.len(),
        micros(took[took.len() / 2]),
        micros(*took.last().unwrap()),
        world_bytes.iter().min().unwrap(),
        world_bytes.iter().max().unwrap()
    );
    let with_log = file_of(&file, &parts).len();
    let without = file_of(
        &file,
        &SaveParts {
            log: Vec::new(),
            ..parts.clone()
        },
    )
    .len();
    let n = exit.commands.len();
    let entry = std::mem::size_of::<sf_game::sim_thread::LoggedCommand>();
    let per_hour = |x: usize| x as u64 * 72_000 / ticks;
    println!(
        "log after {ticks} ticks: {n} commands, {} per hour; in memory {entry} bytes per entry plus the heap of its ids: \
         at least {} bytes per hour; in the file {} bytes ({} per hour)",
        per_hour(n),
        per_hour(n * entry),
        with_log - without,
        per_hour(with_log - without)
    );
}
