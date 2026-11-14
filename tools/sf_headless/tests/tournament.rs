//! The AI tournament (WP9.5a) on the shipped assets: `assets/matches/m1_duel.ron` played through
//! `sf_headless::tournament`, the same code the command line runs. The pure parts (schedule, classification,
//! aggregates, output) are unit-tested in `src/tournament/tests.rs`; this file plays real games.
//!
//! The games are short (a cap of [`SMOKE_CAP`] ticks, far below the default 60,000) so that a debug build
//! finishes the file in about a minute; the release numbers of the full length are in `docs/rules/ai.md`.

mod common;

use std::path::{Path, PathBuf};

use common::TempDir;
use sf_data::match_def::{Controller, MatchDef};
use sf_data::units::Faction;
use sf_headless::cli::{EXIT_INVALID, EXIT_OK, run_cli};
use sf_headless::tournament::{
    Ending, GAME_SPEED_RAW, GameRecord, Report, STALEMATE_WINDOW, factions_for, load_setup,
    play_game, play_seeded, prepare, prepare_sited, render_table, run_games, seeds_for,
    sites_swapped,
};

/// The cap of the smoke games, in ticks. The AIs have built by then, but since WP9.1c-3's opening no attack has
/// been launched yet (0 attacks per AI in all four games, measured with `--games 4 --cap 9000` on the merged tree);
/// the smoke games do not cover an attack, `tournament_decided.rs` does. The cap is left as it is (test time).
const SMOKE_CAP: u64 = 9_000;

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn duel_path() -> PathBuf {
    assets().join("matches/m1_duel.ron")
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|w| (*w).to_string()).collect()
}

fn duel_args(extra: &[&str]) -> Vec<String> {
    let path = duel_path().to_string_lossy().into_owned();
    let mut all = vec!["tournament".to_string(), path];
    all.extend(words(extra));
    all
}

fn tournament(extra: &[&str]) -> (u8, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_cli(duel_args(extra), &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

// --- the match file --------------------------------------------------------------------------------

#[test]
fn m1_duel_is_m1_ai_with_both_seats_under_the_computer() {
    let duel = MatchDef::from_ron(&std::fs::read_to_string(duel_path()).unwrap()).unwrap();
    let ai = MatchDef::embedded_ai();
    assert_eq!(
        (duel.name.as_str(), duel.map.as_str(), duel.seed),
        ("m1_duel", "m1", 458_758)
    );
    assert_eq!(duel.map, ai.map);
    assert_eq!(duel.players.len(), 2);
    for (d, a) in duel.players.iter().zip(&ai.players) {
        // The same seats, credits and starting objects as m1_ai: 4,000 credits and a construction centre.
        assert_eq!((d.id, d.faction, d.credits), (a.id, a.faction, a.credits));
        assert_eq!(d.credits, 4000);
        assert_eq!(d.objects, a.objects);
        assert_eq!(
            d.controller,
            Controller::Ai {
                profile: "normal".to_string()
            }
        );
    }
    assert_eq!(
        (duel.players[0].faction, duel.players[1].faction),
        (Faction::Meridian, Faction::Veyran)
    );
    // The tournament takes it as a setup and knows the factions of the file.
    let setup = load_setup(&assets(), &duel_path()).unwrap();
    assert_eq!(setup.name(), "m1_duel");
    assert_eq!(setup.file_factions(), [Faction::Meridian, Faction::Veyran]);
}

#[test]
fn a_match_with_a_human_seat_or_a_missing_profile_is_refused_with_a_reason() {
    // m1_ai has a standard seat: a tournament has no human.
    let e = load_setup(&assets(), &assets().join("matches/m1_ai.ron"))
        .err()
        .unwrap();
    assert!(
        e.contains("player 0") && e.contains("not a computer opponent"),
        "{e}"
    );
    let e = load_setup(&assets(), &assets().join("matches/m1.ron"))
        .err()
        .unwrap();
    assert!(e.contains("not a computer opponent"), "{e}");
    // A profile that is not under assets/ai is named.
    let dir = TempDir::new("tournament_profile");
    let text = std::fs::read_to_string(duel_path())
        .unwrap()
        .replace("profile: \"normal\"", "profile: \"nonesuch\"");
    std::fs::write(dir.join("x.ron"), text).unwrap();
    let e = load_setup(&assets(), &dir.join("x.ron")).err().unwrap();
    assert!(
        e.contains("nonesuch.ron") && e.contains("cannot read"),
        "{e}"
    );
    // A match file that is not there.
    let e = load_setup(&assets(), &assets().join("matches/nothing.ron"))
        .err()
        .unwrap();
    assert!(e.contains("cannot read"), "{e}");
}

// --- games -----------------------------------------------------------------------------------------

/// The seats of games 0 to 3 are what the schedule says, read back from the world that was played.
#[test]
fn the_seat_swap_reaches_the_world_the_game_is_played_in() {
    let setup = load_setup(&assets(), &duel_path()).unwrap();
    let file = setup.file_factions();
    let mut seen = Vec::new();
    for index in 0..4 {
        // A short game: the seats are decided before the first tick, so 200 ticks show them.
        let r = play_game(&setup, 1, index, 200, STALEMATE_WINDOW).unwrap();
        assert_eq!(r.factions, factions_for(file, index), "game {index}");
        seen.push(r.factions);
    }
    // As written, swapped, and both mirrors: four different seatings, the first as the file has it.
    assert_eq!(
        seen,
        vec![
            [Faction::Meridian, Faction::Veyran],
            [Faction::Veyran, Faction::Meridian],
            [Faction::Meridian, Faction::Meridian],
            [Faction::Veyran, Faction::Veyran],
        ]
    );
    // The seeds of the record are the schedule's.
    let r = play_game(&setup, 1, 3, 200, STALEMATE_WINDOW).unwrap();
    assert_eq!((r.world_seed, r.ai_seed), seeds_for(1, 3));
}

/// The seats of a game are what the **world** and the **AIs** have, not what the schedule says: read from
/// `World::faction_of` and from each AI's profile, both checked against the schedule, and the world's speed.
#[test]
fn the_world_and_the_ais_have_the_factions_of_the_schedule_and_the_games_speed() {
    let setup = load_setup(&assets(), &duel_path()).unwrap();
    let file = setup.file_factions();
    for index in 0..4 {
        let want = factions_for(file, index);
        let p = prepare(&setup, want, 1, 2).unwrap();
        assert_eq!(p.world_factions, want, "world, game {index}");
        assert_eq!(p.ai_factions, want, "AIs, game {index}");
        assert_eq!(p.world.config().speed.raw(), GAME_SPEED_RAW, "game {index}");
        assert_eq!(p.ais.len(), 2);
        // And the record of a played game says the same.
        let r = play_seeded(&setup, index, want, (1, 2), 50, STALEMATE_WINDOW).unwrap();
        assert_eq!((r.factions, r.ai_factions), (want, want));
    }
}

/// The AI's random stream is not drawn by any rule yet: a game with only the AI seed changed is the same
/// world to the last checksum (the AIs' digests, which hash the stream, differ).
#[test]
fn the_ai_seed_alone_does_not_change_a_game() {
    let setup = load_setup(&assets(), &duel_path()).unwrap();
    let file = setup.file_factions();
    let play =
        |ai_seed: u64| play_seeded(&setup, 0, file, (77, ai_seed), 3000, STALEMATE_WINDOW).unwrap();
    let (a, b) = (play(1), play(2));
    assert_eq!(a.final_checksum, b.final_checksum);
    assert_eq!(a.outcome_key(), b.outcome_key());
    assert_ne!(
        a.ais.iter().map(|x| x.digest).collect::<Vec<_>>(),
        b.ais.iter().map(|x| x.digest).collect::<Vec<_>>(),
        "the digest hashes the stream, so it follows the seed"
    );
    // The world seed alone does change the game.
    let c = play_seeded(&setup, 0, file, (78, 1), 3000, STALEMATE_WINDOW).unwrap();
    assert_ne!(a.final_checksum, c.final_checksum);
}

/// A game that never ends still stops, at the cap, as a draw; and the label follows the last loss.
#[test]
fn the_cap_stops_a_game_and_a_quiet_one_is_a_stalemate() {
    let setup = load_setup(&assets(), &duel_path()).unwrap();
    // Nothing fights in the first 1,500 ticks (the AIs are building), so with a window of 1,000 ticks the
    // capped game is a stalemate.
    let r = play_game(&setup, 1, 0, 1500, 1000).unwrap();
    assert_eq!(r.ticks, 1500, "the cap is the end of the game");
    assert_eq!(r.ending, Ending::Stalemate);
    assert_eq!((r.defeat_tick, r.last_loss), (None, 0));
    // The same game with a window longer than the game: capped, not a stalemate.
    let r = play_game(&setup, 1, 0, 1500, 2000).unwrap();
    assert_eq!((r.ticks, r.ending), (1500, Ending::Capped));
    // And the cap is exact for another cap.
    assert_eq!(play_game(&setup, 1, 0, 777, 1000).unwrap().ticks, 777);
}

/// Every game of a run is decided or labelled, consistently with its numbers.
fn check_consistent(r: &GameRecord, cap: u64, window: u64) {
    assert!(r.ticks <= cap, "game {}", r.index);
    assert_eq!(r.ais.len(), 2, "game {}", r.index);
    assert_eq!(
        r.ais.iter().map(|a| a.player).collect::<Vec<_>>(),
        [0, 1],
        "game {}",
        r.index
    );
    match r.ending {
        Ending::Won(seat) => {
            assert!(seat < 2);
            assert_eq!(
                r.defeat_tick,
                Some(r.ticks),
                "game {}: the game ends at the defeat",
                r.index
            );
        }
        Ending::MutualDefeat => assert_eq!(r.defeat_tick, Some(r.ticks)),
        Ending::Stalemate => {
            assert_eq!((r.ticks, r.defeat_tick), (cap, None));
            assert!(r.ticks - r.last_loss >= window, "game {}", r.index);
        }
        Ending::Capped => {
            assert_eq!((r.ticks, r.defeat_tick), (cap, None));
            assert!(r.ticks - r.last_loss < window, "game {}", r.index);
        }
    }
}

#[test]
fn four_games_are_decided_or_labelled_and_the_table_is_the_same_on_a_second_run() {
    let cap = SMOKE_CAP.to_string();
    let dir = TempDir::new("tournament_smoke");
    let json = dir.join("t.json");
    let json_arg = json.to_string_lossy().into_owned();
    let run = [
        "--games",
        "4",
        "--cap",
        &cap,
        "--stalemate",
        "3000",
        "--seed",
        "7",
    ];
    let mut first_args = run.to_vec();
    first_args.extend(["--json", &json_arg]);
    let (code, table, progress) = tournament(&first_args);
    assert_eq!(code, EXIT_OK, "{progress}");
    // Progress and wall time are on stderr, the table only has numbers that are the same every time.
    assert_eq!(
        progress.lines().filter(|l| l.starts_with("game ")).count(),
        4,
        "{progress}"
    );
    assert!(progress.contains("wall time:"), "{progress}");
    assert!(
        !table.contains(" ms"),
        "no wall time in the table:\n{table}"
    );

    let (code2, table2, _) = tournament(&run);
    assert_eq!(code2, EXIT_OK);
    assert_eq!(table, table2, "the same arguments give the same table");

    // The table: a line for each of the four games and the summary.
    assert!(
        table
            .starts_with("tournament: m1_duel, 4 games, seed 0x0000000000000007, cap 9000 ticks\n")
    );
    for needle in [
        "Meridian v Veyran",
        "Veyran v Meridian",
        "Meridian v Meridian",
        "Veyran v Veyran",
        "seat 0 wins:",
        "seat 1 wins:",
        "draws:",
        "duration:",
        "distinct final checksums: ",
    ] {
        assert!(table.contains(needle), "missing {needle:?}:\n{table}");
    }

    // The games themselves, through the library: every one decided or labelled, nothing in between.
    let setup = load_setup(&assets(), &duel_path()).unwrap();
    let args = match sf_headless::tournament::parse_args(words(&[
        "x.ron",
        "--games",
        "4",
        "--cap",
        &cap,
        "--stalemate",
        "3000",
        "--seed",
        "7",
    ]))
    .unwrap()
    {
        sf_headless::tournament::Parsed::Run(a) => a,
        sf_headless::tournament::Parsed::Help => unreachable!(),
    };
    let (records, _) = run_games(&setup, &args, |_, _| {}).unwrap();
    assert_eq!(records.len(), 4);
    for r in &records {
        check_consistent(r, SMOKE_CAP, 3000);
    }
    let report = Report::new("m1_duel", 7, SMOKE_CAP, 3000, records.clone());
    assert_eq!(
        render_table(&report),
        table,
        "the tool prints the table of its own games"
    );
    // Four seatings, four world seeds, and games that end in different states.
    let checksums: std::collections::BTreeSet<u64> =
        records.iter().map(|r| r.final_checksum).collect();
    assert_eq!(report.summary.distinct_checksums as usize, checksums.len());
    assert_eq!(
        checksums.len(),
        4,
        "four seatings and four seeds make four different games"
    );

    // The JSON: written, one line per game, the summary agrees with the table.
    let text = std::fs::read_to_string(&json).unwrap();
    assert!(text.starts_with("{\n  \"format\": 1,\n  \"kind\": \"tournament\",\n"));
    assert!(text.contains("\"match\": \"m1_duel\"") && text.contains("\"cap\": 9000"));
    assert_eq!(
        text.lines()
            .filter(|l| l.trim_start().starts_with("{\"index\""))
            .count(),
        4
    );
    assert!(text.contains("\"distinct_final_checksums\": 4"));
    assert!(text.find("\"summary\"").unwrap() < text.find("\"host\"").unwrap());
    for r in &records {
        assert!(text.contains(&format!(
            "\"final_checksum\": \"{:#018x}\"",
            r.final_checksum
        )));
        assert!(text.contains(&format!("\"digest\": \"{:#018x}\"", r.ais[0].digest)));
    }
}

#[test]
fn a_bad_command_line_or_match_exits_two() {
    let (code, out, err) = tournament(&["--games", "0"]);
    assert_eq!(code, EXIT_INVALID);
    assert!(
        out.is_empty() && err.contains("between 1 and") && err.contains("Usage:"),
        "{err}"
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_cli(
        words(&["tournament", "/nonexistent/matches/m.ron"]),
        &mut out,
        &mut err,
    );
    assert_eq!(code, EXIT_INVALID);
    assert!(String::from_utf8(err).unwrap().contains("cannot read"));
    // Help goes to stdout.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        run_cli(words(&["tournament", "--help"]), &mut out, &mut err),
        EXIT_OK
    );
    assert!(
        String::from_utf8(out)
            .unwrap()
            .starts_with("Usage: sf_headless tournament")
    );
    assert!(err.is_empty());
    // The main help names the subcommand.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    run_cli(words(&["--help"]), &mut out, &mut err);
    assert!(
        String::from_utf8(out)
            .unwrap()
            .contains("sf_headless tournament <match.ron>")
    );
}

/// The site swap reaches the world: whoever owns the construction centre on the south-west site (12, 68) in
/// game `k` does not own it in game `k + 4`, and the faction on that site changes with it (WP9.2a fix round,
/// the 8-cycle). Pins the cycle on the shipped duel.
#[test]
fn the_start_sites_swap_in_the_world_every_four_games() {
    let setup = load_setup(&assets(), &duel_path()).unwrap();
    let file = setup.file_factions();
    let owner_sw = |index: u32| {
        let p = prepare_sited(
            &setup,
            factions_for(file, index),
            sites_swapped(index),
            1,
            2,
        )
        .unwrap();
        let id = p.world.occupant_at(sf_sim::Cell::new(12, 68)).unwrap();
        p.world.get(id).unwrap().owner
    };
    for k in 0..4 {
        let (a, b) = (owner_sw(k), owner_sw(k + 4));
        assert_eq!(a.0, 0, "game {k}: seat 0 starts on the south-west site");
        assert_ne!(a, b, "game {k} and {}: the site changes hands", k + 4);
        assert_eq!(owner_sw(k + 8), a, "game {}", k + 8);
    }
    // A played game records the swap.
    let r = play_game(&setup, 1, 5, 50, STALEMATE_WINDOW).unwrap();
    assert!(r.sites_swapped);
    assert!(
        !play_game(&setup, 1, 1, 50, STALEMATE_WINDOW)
            .unwrap()
            .sites_swapped
    );
}
