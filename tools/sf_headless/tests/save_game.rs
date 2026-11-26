//! WP13.1e-1: the save file (`sf_headless::savefile`, `docs/rules/save.md`, "The save file") and the headless
//! round trip of world and AIs together.
//!
//! * Every CI scenario (the list of the headless job in `ci.yml`) is run with a save file written every 97 ticks
//!   at the save point, loaded onto a fresh world and fresh AIs of the scenario, and the run goes on with the
//!   loaded world, AIs and log. At each save the loaded world has the live checksum, the loaded AIs equal the live
//!   ones, the log is the live log, the AIs are in the save where the scenario has them, and the file written again
//!   from what was loaded is the file. The run ends on the committed pin; for a scenario with AIs the AI digests
//!   and the AIs' commands are those of the run without saves.
//! * `ai_raid` saved and loaded at every tick (release, ignored) and its first 200 ticks in every build.
//! * A recorded game (`game` block, `recorded` commands) through save files, and resumed from one with
//!   `run::resume`, which passes over the arrivals already in the save (`Arrivals::skip_to`).
//! * The command log replays from the start world to the checksum the save holds.
//! * One pinned file hash, every truncation and bit flip refused, and every refusal with its own message.

mod common;

use common::saves::expected;
use common::{TempDir, scenario_path};
use sf_ai::Ai;
use sf_headless::run::{Finished, RunOptions, SavePoint, replay_log, resume, run_full, run_saving};
use sf_headless::savefile::{self, Header, Identity, LoggedCommand, SaveFileError, read_header};
use sf_headless::scenario::{GameSpec, RecordedCommand, Scenario, render_recording};
use sf_math::hash::Fnv1a64;
use sf_sim::save::SaveError;
use sf_sim::{PlayerId, Tick, World};

/// The save interval, a prime (as in `common/saves.rs`).
const EVERY: u64 = 97;

/// The scenario list of the CI headless job (`.github/workflows/ci.yml`, the `--expect` run).
fn ci_scenarios() -> Vec<String> {
    let ci = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/ci.yml"),
    )
    .unwrap();
    let line = ci
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("for name in ") && l.ends_with("; do"))
        .expect("the scenario loop of the headless job");
    let list = &line["for name in ".len()..line.len() - "; do".len()];
    list.split_whitespace().map(str::to_owned).collect()
}

fn load_scenario(name: &str) -> Scenario {
    Scenario::load(&scenario_path(&format!("{name}.ron"))).unwrap()
}

/// What a run through save files did.
struct Through {
    finished: Finished,
    /// Save files written and loaded.
    saves: u64,
    /// Of those, the ones that held at least one AI.
    with_ai: u64,
}

/// Writes the save file at a save point, loads it onto a fresh world and fresh AIs of the scenario, checks what
/// came back against the live state, and puts the loaded world, AIs and log in place of the live ones.
fn save_and_load(
    name: &str,
    scenario: &Scenario,
    fresh: &World,
    identity: &Identity,
    p: SavePoint<'_>,
) -> (Vec<u8>, bool) {
    let t = p.world.tick().0;
    let bytes = savefile::write(identity, p.world, p.ais, p.log)
        .unwrap_or_else(|e| panic!("{name}: write at tick {t}: {e}"));
    let header = read_header(&bytes).unwrap_or_else(|e| panic!("{name}: header at {t}: {e}"));
    assert_eq!(header.tick, Tick(t), "{name}");
    assert_eq!(header.checksum, p.world.checksum(), "{name}: tick {t}");
    assert_eq!(&header.identity, identity, "{name}: tick {t}");
    let loaded = savefile::load(&bytes, identity, fresh.clone(), scenario.ais.clone())
        .unwrap_or_else(|e| panic!("{name}: load at tick {t}: {e}"));
    assert_eq!(loaded.world.tick(), Tick(t), "{name}");
    assert_eq!(
        loaded.world.checksum(),
        p.world.checksum(),
        "{name}: tick {t}"
    );
    assert_eq!(&loaded.ais, p.ais, "{name}: the AIs at tick {t}");
    assert_eq!(&loaded.log, p.log, "{name}: the log at tick {t}");
    assert!(loaded.host.is_empty());
    // The save holds the AIs where the scenario has them.
    assert_eq!(loaded.ais.len(), scenario.ais.len(), "{name}: tick {t}");
    let again = savefile::write(identity, &loaded.world, &loaded.ais, &loaded.log).unwrap();
    assert!(
        again == bytes,
        "{name}: the file written again at tick {t} is not the file"
    );
    let with_ai = !loaded.ais.is_empty();
    **p.world = loaded.world;
    *p.ais = loaded.ais;
    *p.log = loaded.log;
    (bytes, with_ai)
}

/// Runs `scenario` to its end with a save file written and loaded at every save point where `due(tick)` holds,
/// going on with what was loaded.
fn through_saves(name: &str, scenario: &Scenario, due: impl Fn(u64) -> bool) -> Through {
    let options = RunOptions::for_scenario(scenario);
    let fresh = scenario.build_fresh_world().unwrap();
    let identity = Identity::of_scenario(scenario, &fresh);
    let (mut saves, mut with_ai) = (0u64, 0u64);
    let finished = run_saving(
        scenario,
        &options,
        |p| {
            let t = p.world.tick().0;
            if t == 0 || !due(t) {
                return;
            }
            let (_, ai) = save_and_load(name, scenario, &fresh, &identity, p);
            saves += 1;
            with_ai += u64::from(ai);
        },
        |_, _| {},
    )
    .unwrap();
    Through {
        finished,
        saves,
        with_ai,
    }
}

/// [`through_saves`] every [`EVERY`] ticks, ending on the committed pin; with AIs, the AI digests and commands
/// are those of the run without saves.
fn check_scenario(name: &str) {
    let scenario = load_scenario(name);
    let through = through_saves(name, &scenario, |t| t % EVERY == 0);
    let o = &through.finished.outcome;
    assert_eq!(
        o.final_checksum,
        expected(name),
        "{name}: final checksum through save files"
    );
    assert!(through.saves >= 1, "{name}: no save");
    if scenario.ais.is_empty() {
        assert_eq!(through.with_ai, 0);
    } else {
        assert_eq!(
            through.with_ai, through.saves,
            "{name}: a save without its AIs"
        );
        let plain = run_full(&scenario, &RunOptions::for_scenario(&scenario)).unwrap();
        assert_eq!(o.ai_digests, plain.outcome.ai_digests, "{name}: AI digests");
        assert!(
            through.finished.ai_commands == plain.ai_commands,
            "{name}: the AIs' commands"
        );
        assert!(through.finished.log == plain.log, "{name}: the log");
    }
    eprintln!(
        "{name}: {} save files, {} with AIs",
        through.saves, through.with_ai
    );
}

macro_rules! ci_scenario_tests {
    ($($test:ident => $name:literal),* $(,)?) => {
        /// The scenarios the tests below cover, in the order of `ci.yml`.
        const COVERED: &[&str] = &[$($name),*];
        $(
            #[test]
            fn $test() {
                check_scenario($name);
            }
        )*
    };
}

ci_scenario_tests! {
    wander_5000_through_save_files => "wander_5000",
    command_mix_through_save_files => "command_mix",
    terrain_256_through_save_files => "terrain_256",
    units_spawn_through_save_files => "units_spawn",
    orders_through_save_files => "orders",
    terrain_costs_through_save_files => "terrain_costs",
    pathing_through_save_files => "pathing",
    pathing_256_through_save_files => "pathing_256",
    bridge_200_through_save_files => "bridge_200",
    crowd_jam_through_save_files => "crowd_jam",
    turning_through_save_files => "turning",
    replan_through_save_files => "replan",
    formation_move_through_save_files => "formation_move",
    weapons_reload_through_save_files => "weapons_reload",
    projectiles_through_save_files => "projectiles",
    armour_splash_through_save_files => "armour_splash",
    tanks_vs_infantry_through_save_files => "tanks_vs_infantry",
    destruction_through_save_files => "destruction",
    placement_through_save_files => "placement",
    construction_through_save_files => "construction",
    ore_256_through_save_files => "ore_256",
    harvest_through_save_files => "harvest",
    storage_through_save_files => "storage",
    money_through_save_files => "money",
    production_through_save_files => "production",
    production_repeat_through_save_files => "production_repeat",
    match_through_save_files => "match",
    power_through_save_files => "power",
    defences_through_save_files => "defences",
    repair_sell_through_save_files => "repair_sell",
    vision_through_save_files => "vision",
    vision_targeting_through_save_files => "vision_targeting",
    silo_through_save_files => "silo",
    ai_fog_through_save_files => "ai_fog",
    ai_base_through_save_files => "ai_base",
    ai_noore_through_save_files => "ai_noore",
    ai_boxed_through_save_files => "ai_boxed",
    ai_economy_through_save_files => "ai_economy",
    ai_rebuild_through_save_files => "ai_rebuild",
    ai_deadlock_through_save_files => "ai_deadlock",
    ai_scout_through_save_files => "ai_scout",
    ai_vs_passive_through_save_files => "ai_vs_passive",
    ai_raid_through_save_files => "ai_raid",
    ai_raid_guard_through_save_files => "ai_raid_guard",
    ai_bounce_through_save_files => "ai_bounce",
    ai_harvest_raid_through_save_files => "ai_harvest_raid",
    ai_vs_passive_easy_through_save_files => "ai_vs_passive_easy",
    ai_vs_passive_hard_through_save_files => "ai_vs_passive_hard",
}

/// The tests above are the CI list: a scenario added to the job and not here fails.
#[test]
fn every_ci_scenario_has_its_save_file_test() {
    let names = ci_scenarios();
    assert!(names.len() > 40, "{names:?}");
    assert_eq!(names, COVERED, "the CI scenarios and the save file tests");
}

/// `ai_raid` (two AIs that raid each other) saved and loaded at every save point of its first 200 ticks, then
/// every 97, to its pin: every build.
#[test]
fn the_first_ticks_of_ai_raid_load_after_every_tick() {
    let scenario = load_scenario("ai_raid");
    let through = through_saves("ai_raid", &scenario, |t| t <= 200 || t % EVERY == 0);
    assert_eq!(through.finished.outcome.final_checksum, expected("ai_raid"));
    assert!(through.saves >= 200 + 50);
    assert_eq!(through.with_ai, through.saves);
}

/// `ai_raid` saved and loaded at every save point to its pin (a save the runner writes must always load).
/// `cargo test --release -p sf_headless --test save_game -- --ignored every_tick`.
#[test]
#[ignore = "a save file at every tick: run in release"]
fn every_tick_ai_raid() {
    let start = std::time::Instant::now();
    let scenario = load_scenario("ai_raid");
    let through = through_saves("ai_raid", &scenario, |_| true);
    assert_eq!(through.finished.outcome.final_checksum, expected("ai_raid"));
    assert_eq!(through.saves, scenario.ticks - 1);
    eprintln!(
        "ai_raid: {} save files in {:?}",
        through.saves,
        start.elapsed()
    );
}

/// The save file of `name` at the save point of tick `at`, and the scenario.
fn save_at(name: &str, at: u64) -> (Scenario, Vec<u8>) {
    let scenario = load_scenario(name);
    let bytes = save_of(&scenario, at);
    (scenario, bytes)
}

fn save_of(scenario: &Scenario, at: u64) -> Vec<u8> {
    let fresh = scenario.build_fresh_world().unwrap();
    let identity = Identity::of_scenario(scenario, &fresh);
    let mut bytes = None;
    let options = RunOptions {
        ticks: at + 1,
        every: 1000,
    };
    run_saving(
        scenario,
        &options,
        |p| {
            if p.world.tick().0 == at {
                bytes = Some(savefile::write(&identity, p.world, p.ais, p.log).unwrap());
            }
        },
        |_, _| {},
    )
    .unwrap();
    bytes.unwrap()
}

fn load_of(scenario: &Scenario, bytes: &[u8]) -> Result<savefile::Loaded, SaveFileError> {
    let fresh = scenario.build_fresh_world().unwrap();
    let identity = Identity::of_scenario(scenario, &fresh);
    savefile::load(bytes, &identity, fresh, scenario.ais.clone())
}

/// The pinned save: `ai_raid` at tick 970. Its length and trailer hash are the same on Linux and Windows (CI runs
/// this test on both). Moves with every world or AI format change and every new AI state field: re-pinning is part
/// of that WP.
#[test]
fn the_save_file_of_ai_raid_at_tick_970_is_pinned() {
    let (_, bytes) = save_at("ai_raid", 970);
    let mut trailer = [0u8; 8];
    trailer.copy_from_slice(&bytes[bytes.len() - 8..]);
    let mut h = Fnv1a64::new();
    h.write_bytes(&bytes);
    eprintln!(
        "ai_raid@970: {} bytes, trailer {:#018x}, hash {:#018x}",
        bytes.len(),
        u64::from_le_bytes(trailer),
        h.finish()
    );
    assert_eq!(&bytes[..4], b"SFSG");
    assert_eq!((bytes.len(), h.finish()), PINNED);
}

/// The length and the FNV-1a 64 hash of the whole file of
/// [`the_save_file_of_ai_raid_at_tick_970_is_pinned`].
const PINNED: (usize, u64) = (55_296, 0xfac0_fae0_a97b_7248);

/// Resuming from a save file (`run::resume`) ends where the run without a save ends, and the loaded game goes on
/// with the same AI digests.
#[test]
fn ai_raid_resumed_from_a_save_file_ends_on_its_pin() {
    let (scenario, bytes) = save_at("ai_raid", 970);
    let loaded = load_of(&scenario, &bytes).unwrap();
    let options = RunOptions::for_scenario(&scenario);
    let resumed = resume(&scenario, &options, loaded, |_| {}, |_, _| {}).unwrap();
    assert_eq!(resumed.outcome.final_checksum, expected("ai_raid"));
    let plain = run_full(&scenario, &options).unwrap();
    assert_eq!(resumed.outcome.ai_digests, plain.outcome.ai_digests);
    assert!(resumed.log == plain.log, "the log after resuming");
}

/// The command log of a save replays from the scenario's start world to the checksum the save holds.
#[test]
fn the_log_of_an_ai_scenario_replays_to_the_saved_checksum() {
    for (name, at) in [("ai_raid", 970), ("ai_vs_passive", 1455)] {
        let (scenario, bytes) = save_at(name, at);
        let loaded = load_of(&scenario, &bytes).unwrap();
        assert!(!loaded.log.is_empty(), "{name}: the AIs sent commands");
        let replayed = replay_log(&scenario, &loaded.log, loaded.header.tick).unwrap();
        assert_eq!(replayed.tick(), Tick(at));
        assert_eq!(replayed.checksum(), loaded.header.checksum, "{name}");
        // Without the log the replay is another world.
        let bare = replay_log(&scenario, &[], loaded.header.tick).unwrap();
        assert_ne!(bare.checksum(), loaded.header.checksum, "{name}");
    }
}

// --- A recorded game -----------------------------------------------------------------------------------------

/// The shipped assets, absolute.
fn assets() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets")
        .canonicalize()
        .unwrap()
}

/// Ticks of the recorded game.
const RECORDED_TICKS: u64 = 1500;

/// A recorded game of `m1_duel` with both seats played by the shipped AIs: the record is made here by running the
/// game's world with the game's AIs and logging what they send, every fifth command for two ticks later than it
/// was sent, plus an empty `Stop` of player 1 two ticks before every 97th tick that runs three ticks later (so
/// recorded commands are pending across every save point). Returns the scenario (loaded from the written file) and
/// the final checksum of the run that made it.
fn recorded_game(dir: &TempDir) -> (Scenario, u64) {
    let assets = assets();
    let game = GameSpec {
        assets: assets.to_string_lossy().into_owned(),
        match_path: "matches/m1_duel.ron".into(),
        speed_raw: 98_304,
    };
    let seed = 0x5EED_13E1;
    let cells = (96, 96);
    let path = dir.join("recorded.ron");
    let write = |commands: &[RecordedCommand]| {
        let text =
            render_recording("recorded", seed, cells, &game, RECORDED_TICKS, commands).unwrap();
        std::fs::write(&path, text).unwrap();
        Scenario::load(&path).unwrap()
    };
    let empty = write(&[]);
    let text = std::fs::read_to_string(assets.join("matches/m1_duel.ron")).unwrap();
    let def = sf_data::match_def::MatchDef::from_ron(&text).unwrap();
    let defs = sf_data::units::UnitDefs::load(&assets.join("config"), &assets).unwrap();
    let mut ais = sf_ai::match_ais(&def, &defs, 7, &|name| {
        std::fs::read_to_string(assets.join("ai").join(format!("{name}.ron")))
            .map_err(|e| e.to_string())
    })
    .unwrap();
    assert_eq!(ais.len(), 2);
    let mut world = empty.build_world().unwrap();
    let mut commands = Vec::new();
    let mut wanted = Vec::new();
    while world.tick().0 < RECORDED_TICKS {
        let now = world.tick();
        if now.0 % EVERY == EVERY - 2 {
            let stop = sf_sim::Command::Stop { ids: Vec::new() };
            let at = Tick(now.0 + 3);
            commands.push(RecordedCommand::new(now, at, PlayerId(1), &stop));
            world.schedule(at, PlayerId(1), stop).unwrap();
        }
        for ai in &mut ais {
            if ai.due(world.tick()) {
                wanted.clear();
                ai.think(&world, &mut wanted);
                for command in wanted.drain(..) {
                    let now = world.tick();
                    let at = if commands.len() % 5 == 4 {
                        Tick(now.0 + 2)
                    } else {
                        now
                    };
                    commands.push(RecordedCommand::new(now, at, ai.player(), &command));
                    world.schedule(at, ai.player(), command).unwrap();
                }
            }
        }
        world.step();
    }
    assert!(commands.len() > 20, "{} commands", commands.len());
    let recorded = write(&commands);
    (recorded, world.checksum())
}

/// A recorded game through save files every 97 ticks ends on the checksum of the run that recorded it, its log
/// replays to the saved checksum, and resuming from a save passes over the arrivals the save holds.
#[test]
fn a_recorded_game_round_trips_through_its_saves() {
    let dir = TempDir::new("save_game_recorded");
    let (scenario, end) = recorded_game(&dir);
    let plain = run_full(&scenario, &RunOptions::for_scenario(&scenario)).unwrap();
    assert_eq!(plain.outcome.final_checksum, end, "the record replays");

    let through = through_saves("recorded", &scenario, |t| t % EVERY == 0);
    assert_eq!(through.finished.outcome.final_checksum, end);
    assert!(through.saves >= 14);
    assert_eq!(through.with_ai, 0);

    // Tick 970: the `Stop` that arrived at 968 runs at 971, so the save holds a pending recorded command.
    let bytes = save_of(&scenario, 970);
    let loaded = load_of(&scenario, &bytes).unwrap();
    assert!(
        loaded
            .log
            .iter()
            .any(|c| c.arrived == Tick(968) && c.effective == Tick(971)),
        "the delayed command in the log"
    );
    let n = loaded.log.len();
    assert_eq!(
        n,
        scenario
            .recorded
            .iter()
            .filter(|c| c.arrived <= 970)
            .count()
    );
    let replayed = replay_log(&scenario, &loaded.log, Tick(970)).unwrap();
    assert_eq!(replayed.checksum(), loaded.header.checksum);
    let options = RunOptions::for_scenario(&scenario);
    let resumed = resume(&scenario, &options, loaded, |_| {}, |_, _| {}).unwrap();
    assert_eq!(resumed.outcome.final_checksum, end);
    assert!(resumed.log == plain.log);
}

/// The fresh world of a recorded game is the game's world before its starting objects: tick 0, empty, and a save of
/// the game restores onto it.
#[test]
fn a_recorded_game_has_a_fresh_world() {
    let dir = TempDir::new("save_game_fresh");
    let (scenario, _) = recorded_game(&dir);
    let fresh = scenario.build_fresh_world().unwrap();
    assert_eq!(fresh.tick(), Tick(0));
    assert_eq!(fresh.len(), 0);
    assert!(fresh.pending_commands().is_empty());
    assert_eq!(
        fresh.rules_digest(),
        scenario.build_world().unwrap().rules_digest()
    );
}

// --- Refusals --------------------------------------------------------------------------------------------------

/// `ai_raid` with a second AI, for player 1 (every CI scenario has one AI): the profile of the scenario's
/// directory, resolved for player 1's faction, seed 2.
fn two_ais() -> Scenario {
    let mut s = load_scenario("ai_raid");
    let dir = scenario_path("ai_raid_defs");
    let defs = sf_data::units::UnitDefs::load(&dir, &scenario_path("..")).unwrap();
    let text = std::fs::read_to_string(dir.join("normal.ron")).unwrap();
    let sites = s.ais[0].state().start_sites.clone();
    let faction = sf_data::units::Faction::Veyran;
    s.ais
        .push(sf_ai::make_ai(PlayerId(1), faction, &text, &defs, 2, sites).unwrap());
    s.ai.push(sf_headless::scenario::AiSpec {
        player: 1,
        profile: "normal.ron".into(),
        seed: 2,
    });
    s.name = "ai_raid_two".into();
    s
}

/// Two AIs through save files every 97 ticks: the run ends where the run without saves ends, with the same AI
/// digests, commands and log.
#[test]
fn two_ais_round_trip_through_their_saves() {
    let s = two_ais();
    let plain = run_full(&s, &RunOptions::for_scenario(&s)).unwrap();
    let through = through_saves("ai_raid_two", &s, |t| t % EVERY == 0);
    assert_eq!(through.with_ai, through.saves);
    let o = &through.finished.outcome;
    assert_eq!(o.final_checksum, plain.outcome.final_checksum);
    assert_eq!(o.ai_digests.len(), 2);
    assert_eq!(o.ai_digests, plain.outcome.ai_digests);
    assert!(through.finished.log == plain.log);
    assert!(
        plain.ai_commands.iter().any(|c| c.1 == 1),
        "player 1's AI sends commands"
    );
}

/// A small save with two AIs for the byte-level tests: [`two_ais`] at tick 291.
fn small() -> (Scenario, Vec<u8>) {
    let s = two_ais();
    let bytes = save_of(&s, 291);
    (s, bytes)
}

/// The sections of a save file: `(tag, body)`, in order.
fn sections(bytes: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let data = &bytes[8..bytes.len() - 8];
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let tag = data[i];
        let n = u32::from_le_bytes(data[i + 1..i + 5].try_into().unwrap()) as usize;
        out.push((tag, data[i + 5..i + 5 + n].to_vec()));
        i += 5 + n;
    }
    out
}

/// A save file of these parts with a right hash: a hand-made file.
fn assemble(head: &[u8], sections: &[(u8, Vec<u8>)], extra: &[u8]) -> Vec<u8> {
    let mut out = head.to_vec();
    for (tag, body) in sections {
        out.push(*tag);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
    }
    out.extend_from_slice(extra);
    rehash(out)
}

fn rehash(mut body: Vec<u8>) -> Vec<u8> {
    let mut h = Fnv1a64::new();
    h.write_bytes(&body);
    body.extend_from_slice(&h.finish().to_le_bytes());
    body
}

/// `bytes` with section `tag` changed by `change`, hashed again.
fn edited(bytes: &[u8], tag: u8, change: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut s = sections(bytes);
    change(&mut s.iter_mut().find(|(t, _)| *t == tag).unwrap().1);
    assemble(&bytes[..8], &s, &[])
}

#[test]
fn the_layout_is_magic_version_five_sections_and_a_hash() {
    let (_, bytes) = small();
    assert_eq!(&bytes[..4], b"SFSG");
    assert_eq!(&bytes[4..8], &1u32.to_le_bytes());
    let tags: Vec<u8> = sections(&bytes).iter().map(|s| s.0).collect();
    assert_eq!(tags, b"IWALH");
    assert_eq!(assemble(&bytes[..8], &sections(&bytes), &[]), bytes);
    let s = sections(&bytes);
    // The world section is `World::save`, whole.
    assert_eq!(&s[1].1[..4], b"SFSV");
    // Two AIs (ascending), host section version 1 and empty.
    assert_eq!(s[2].1[0], 2);
    assert_eq!(s[4].1, [1, 0, 0, 0, 0, 0, 0, 0]);
}

/// Every cut of the file is refused, without a panic.
#[test]
fn every_truncation_is_refused() {
    let (scenario, bytes) = small();
    for n in 0..bytes.len() {
        let cut = &bytes[..n];
        let e = read_header(cut).unwrap_err();
        assert!(
            matches!(e, SaveFileError::Truncated | SaveFileError::Damaged),
            "{n}: {e}"
        );
        if n % 97 == 0 {
            assert!(load_of(&scenario, cut).is_err());
        }
    }
    eprintln!("{} truncations refused", bytes.len());
}

/// Every single-bit flip is refused (by the hash), without a panic; and a flip in sections `I`, `A`, `L` or `H`
/// of a hand-made file with a right hash loads or is refused, never panics.
#[test]
fn every_bit_flip_is_refused() {
    let (scenario, bytes) = small();
    for i in 0..bytes.len() {
        for bit in 0..8 {
            let mut b = bytes.clone();
            b[i] ^= 1 << bit;
            assert_eq!(
                read_header(&b),
                Err(SaveFileError::Damaged),
                "byte {i} bit {bit}"
            );
        }
    }
    let (mut refused, mut passed) = (0, 0);
    for tag in *b"IALH" {
        let len = sections(&bytes)
            .iter()
            .find(|s| s.0 == tag)
            .unwrap()
            .1
            .len();
        // Every byte of the small sections, every 7th of the AI section.
        let step = if tag == b'A' { 7 } else { 1 };
        for i in (0..len).step_by(step) {
            let b = edited(&bytes, tag, |body| body[i] ^= 1 << (i % 8));
            match load_of(&scenario, &b) {
                Ok(_) => passed += 1,
                Err(_) => refused += 1,
            }
        }
    }
    eprintln!("hand-made flips: {refused} refused, {passed} loaded");
    assert!(refused > 0);
}

type IdentityChange = fn(&mut Identity);

fn refused(scenario: &Scenario, bytes: &[u8]) -> SaveFileError {
    load_of(scenario, bytes).unwrap_err()
}

#[test]
fn a_save_of_another_game_is_refused_with_its_message() {
    let (scenario, bytes) = small();
    load_of(&scenario, &bytes).unwrap();
    // Another scenario.
    let other = load_scenario("ai_raid");
    let e = refused(&other, &bytes);
    assert_eq!(e, SaveFileError::OtherMatch("match"));
    assert_eq!(e.to_string(), "the save is of another game: other match");
    // The same scenario with another AI level: `ai_vs_passive_easy` under the name of `ai_vs_passive`.
    let mut easy = load_scenario("ai_vs_passive_easy");
    easy.name = scenario.name.clone();
    assert_eq!(
        refused(&easy, &bytes),
        SaveFileError::OtherMatch("AI level")
    );
    // Another seed.
    let mut seeded = scenario.clone();
    seeded.seed ^= 1;
    assert_eq!(refused(&seeded, &bytes), SaveFileError::OtherMatch("seed"));
    // Another identity field each.
    let fresh = scenario.build_fresh_world().unwrap();
    let base = Identity::of_scenario(&scenario, &fresh);
    let cases: [(IdentityChange, &str); 5] = [
        (|i| i.kind = savefile::SaveKind::Match, "kind of game"),
        (|i| i.cells.0 += 1, "map size"),
        (|i| i.speed_raw += 1, "speed"),
        (|i| i.local_player = 1, "local player"),
        (|i| i.ai_level = None, "AI level"),
    ];
    for (change, what) in cases {
        let mut id = base.clone();
        change(&mut id);
        let e = savefile::load(&bytes, &id, fresh.clone(), scenario.ais.clone()).unwrap_err();
        assert_eq!(e, SaveFileError::OtherMatch(what));
    }
}

#[test]
fn other_assets_are_refused_with_their_message() {
    let (scenario, bytes) = small();
    let fresh = scenario.build_fresh_world().unwrap();
    let identity = Identity::of_scenario(&scenario, &fresh);
    // Other rules: a fresh world with another path budget has another rules digest.
    let other = fresh
        .clone()
        .with_path_budget(sf_sim::MIN_NODE_BUDGET + 7)
        .unwrap();
    assert_ne!(other.rules_digest(), fresh.rules_digest());
    let e = savefile::load(&bytes, &identity, other, scenario.ais.clone()).unwrap_err();
    assert_eq!(e, SaveFileError::OtherMatch("rules or assets"));
    assert_eq!(
        e.to_string(),
        "the save is of another game: other rules or assets"
    );
    // A hand-made file whose copy of the digest matches the host but whose world does not: the world refuses.
    let digest = fresh.rules_digest();
    let other_digest = {
        let w = other_world_bytes(&scenario);
        u64::from_le_bytes(w[8..16].try_into().unwrap())
    };
    assert_ne!(other_digest, digest);
    let b = edited(&bytes, b'W', |w| {
        w[8..16].copy_from_slice(&other_digest.to_le_bytes())
    });
    assert_eq!(
        refused(&scenario, &b),
        SaveFileError::World(SaveError::Rules)
    );
}

/// The `World::save` bytes of a world with other rules (another path budget).
fn other_world_bytes(scenario: &Scenario) -> Vec<u8> {
    scenario
        .build_fresh_world()
        .unwrap()
        .with_path_budget(sf_sim::MIN_NODE_BUDGET + 7)
        .unwrap()
        .save()
        .unwrap()
}

#[test]
fn another_number_of_ais_is_refused() {
    let (scenario, bytes) = small();
    let fresh = scenario.build_fresh_world().unwrap();
    let identity = Identity::of_scenario(&scenario, &fresh);
    let mut fewer = scenario.ais.clone();
    fewer.pop();
    let e = savefile::load(&bytes, &identity, fresh.clone(), fewer).unwrap_err();
    assert_eq!(e, SaveFileError::OtherMatch("number of AIs"));
    assert_eq!(
        e.to_string(),
        "the save is of another game: other number of AIs"
    );
    // As many AIs, another player.
    let mut moved = scenario.ais.clone();
    let last = moved.pop().unwrap();
    let s = last.state();
    moved.push(Ai::new(
        PlayerId(7),
        s.profile.clone(),
        0,
        s.start_sites.clone(),
    ));
    let e = savefile::load(&bytes, &identity, fresh, moved).unwrap_err();
    assert_eq!(e, SaveFileError::OtherMatch("AI players"));
}

#[test]
fn trailing_bytes_are_refused() {
    let (scenario, bytes) = small();
    let b = assemble(&bytes[..8], &sections(&bytes), &[0]);
    let e = refused(&scenario, &b);
    assert_eq!(e, SaveFileError::TrailingBytes);
    assert_eq!(e.to_string(), "the save file has bytes after its sections");
    // A section with a byte too many.
    let b = edited(&bytes, b'H', |h| h.push(0));
    assert!(matches!(
        refused(&scenario, &b),
        SaveFileError::Malformed(_)
    ));
}

#[test]
fn magic_and_versions_are_refused_with_their_messages() {
    let (scenario, bytes) = small();
    let mut b = bytes[..bytes.len() - 8].to_vec();
    b[3] = b'V';
    assert_eq!(read_header(&rehash(b)), Err(SaveFileError::BadMagic));
    let mut b = bytes[..bytes.len() - 8].to_vec();
    b[4] = 2;
    let e = read_header(&rehash(b)).unwrap_err();
    assert_eq!(e.to_string(), "save file format 2, this build reads 1");
    // The world and AI format copies in `I`: the 8 bytes before digest, tick and checksum.
    let b = edited(&bytes, b'I', |i| {
        let n = i.len();
        i[n - 32] ^= 0x40;
    });
    let e = read_header(&b).unwrap_err();
    assert!(
        matches!(e, SaveFileError::Version { what: "world", .. }),
        "{e}"
    );
    let b = edited(&bytes, b'I', |i| {
        let n = i.len();
        i[n - 28] ^= 0x40;
    });
    let e = read_header(&b).unwrap_err();
    assert!(
        matches!(e, SaveFileError::Version { what: "AI", .. }),
        "{e}"
    );
    let b = edited(&bytes, b'H', |h| h[0] = 9);
    let e = refused(&scenario, &b);
    assert_eq!(e.to_string(), "host format 9, this build reads 1");
}

#[test]
fn a_header_that_contradicts_the_world_is_refused() {
    let (scenario, bytes) = small();
    let b = edited(&bytes, b'I', |i| {
        let n = i.len();
        i[n - 16] ^= 1;
    });
    assert_eq!(read_header(&b).unwrap().tick, Tick(290));
    assert_eq!(
        refused(&scenario, &b),
        SaveFileError::Inconsistent("the tick")
    );
    let b = edited(&bytes, b'I', |i| {
        let n = i.len();
        i[n - 1] ^= 1;
    });
    assert_eq!(
        refused(&scenario, &b),
        SaveFileError::Inconsistent("the checksum")
    );
}

#[test]
fn a_broken_world_or_ai_section_is_refused_with_its_part() {
    let (scenario, bytes) = small();
    let b = edited(&bytes, b'W', |w| w.push(0));
    assert!(matches!(refused(&scenario, &b), SaveFileError::World(_)));
    // The second AI's bytes with one more byte: player 1 refuses (its length is the 4 bytes after its player byte).
    let b = edited(&bytes, b'A', |a| {
        let n0 = u32::from_le_bytes(a[2..6].try_into().unwrap()) as usize;
        let at = 1 + 1 + 4 + n0; // the second AI's player byte
        let n1 = u32::from_le_bytes(a[at + 1..at + 5].try_into().unwrap());
        a[at + 1..at + 5].copy_from_slice(&(n1 + 1).to_le_bytes());
        a.push(0);
    });
    let e = refused(&scenario, &b);
    let SaveFileError::Ai(p, _) = e else {
        panic!("{e}")
    };
    let second = scenario.ais[1].player();
    assert_eq!(p, second);
    assert!(
        e.to_string()
            .starts_with(&format!("the AI of player {} in the save", second.0))
    );
}

#[test]
fn an_impossible_log_is_refused_with_its_message() {
    let (scenario, bytes) = small();
    let header: Header = read_header(&bytes).unwrap();
    let loaded = load_of(&scenario, &bytes).unwrap();
    let fresh = scenario.build_fresh_world().unwrap();
    let identity = Identity::of_scenario(&scenario, &fresh);
    let cmd = loaded.log[0].command.clone();
    let c = |arrived: u64, effective: u64| LoggedCommand {
        arrived: Tick(arrived),
        effective: Tick(effective),
        player: PlayerId(0),
        command: cmd.clone(),
    };
    let t = header.tick.0;
    let cases = [
        (vec![c(5, 4)], "a command runs before it arrived"),
        (vec![c(t + 1, t + 1)], "a command arrived after the save"),
        (
            vec![c(6, 6), c(5, 9)],
            "the commands are not in the order they arrived",
        ),
    ];
    for (log, what) in cases {
        let b = savefile::write(&identity, &loaded.world, &loaded.ais, &log).unwrap();
        let e = refused(&scenario, &b);
        assert_eq!(e, SaveFileError::Log(what));
        assert_eq!(
            e.to_string(),
            format!("the command log in the save: {what}")
        );
    }
    // Equal ticks are allowed: arrived = effective = the save tick.
    let b = savefile::write(&identity, &loaded.world, &loaded.ais, &[c(t, t), c(t, t)]).unwrap();
    load_of(&scenario, &b).unwrap();
    // A command tag that does not exist.
    let b = edited(&bytes, b'L', |l| l[4 + 17] = 0xEE);
    assert_eq!(
        refused(&scenario, &b),
        SaveFileError::Log("a command is malformed")
    );
}

#[test]
fn the_ais_must_be_ascending_to_be_written() {
    let (scenario, bytes) = small();
    let loaded = load_of(&scenario, &bytes).unwrap();
    let mut ais = loaded.ais.clone();
    ais.reverse();
    let identity = Identity::of_scenario(&scenario, &loaded.world);
    assert_eq!(
        savefile::write(&identity, &loaded.world, &ais, &loaded.log),
        Err(SaveFileError::Malformed(
            "the AIs are not ascending by player"
        ))
    );
}
