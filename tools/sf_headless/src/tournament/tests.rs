//! Unit tests of the tournament's schedule, classification, aggregates and output. The games themselves
//! are played by `tests/tournament.rs` on the shipped assets.

use super::*;

const M: Faction = Faction::Meridian;
const V: Faction = Faction::Veyran;

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_string()).collect()
}

fn args(words: &[&str]) -> Args {
    match parse_args(strings(words)).unwrap() {
        Parsed::Run(a) => a,
        Parsed::Help => panic!("help"),
    }
}

// --- the schedule ----------------------------------------------------------------------------------

#[test]
fn the_seats_cycle_through_as_written_swapped_and_both_mirrors() {
    let file = [M, V];
    assert_eq!(factions_for(file, 0), [M, V]);
    assert_eq!(factions_for(file, 1), [V, M]);
    assert_eq!(factions_for(file, 2), [M, M]);
    assert_eq!(factions_for(file, 3), [V, V]);
    // The cycle repeats, and a file written the other way round starts the other way round.
    for i in 0..4 {
        assert_eq!(factions_for(file, i + 4), factions_for(file, i));
        assert_eq!(factions_for(file, i + 400), factions_for(file, i));
    }
    assert_eq!(factions_for([V, M], 0), [V, M]);
    assert_eq!(factions_for([V, M], 1), [M, V]);
    // Over a whole cycle each faction sits on each site as often as the other: no site is favoured by the
    // schedule.
    let seat0: Vec<Faction> = (0..4).map(|i| factions_for(file, i)[0]).collect();
    let seat1: Vec<Faction> = (0..4).map(|i| factions_for(file, i)[1]).collect();
    assert_eq!(seat0.iter().filter(|f| **f == M).count(), 2);
    assert_eq!(seat1.iter().filter(|f| **f == M).count(), 2);
}

#[test]
fn the_seeds_of_a_game_are_pinned_distinct_and_depend_on_the_base_seed() {
    // Pinned: a change of the mixer moves every tournament table.
    assert_eq!(
        seeds_for(1, 0),
        (5_067_554_077_270_220_563, 6_768_782_832_058_643_234)
    );
    // The mixer is SplitMix64: its first output for the state 0 is a published value.
    assert_eq!(mix(0), 0xE220_A839_7B1D_CDAF);
    let mut seen = BTreeSet::new();
    for i in 0..100 {
        let (world, ai) = seeds_for(458_758, i);
        assert_ne!(world, ai, "game {i}");
        assert!(seen.insert(world), "world seed of game {i} repeats");
        assert!(seen.insert(ai), "ai seed of game {i} repeats");
        assert_eq!(
            seeds_for(458_758, i),
            (world, ai),
            "the same on a second ask"
        );
        assert_ne!(
            seeds_for(458_759, i),
            (world, ai),
            "game {i} ignores the base seed"
        );
    }
}

#[test]
fn the_speed_of_the_tournament_world_is_the_games() {
    assert_eq!(GAME_SPEED_RAW, 98_304);
    assert_eq!(Fx::from_ratio(3, 2).raw(), GAME_SPEED_RAW);
}

// --- classification --------------------------------------------------------------------------------

#[test]
fn a_game_is_won_drawn_stalemated_or_capped() {
    let w = STALEMATE_WINDOW;
    assert_eq!(classify(Some(Some(1)), 7000, 7000, w), Ending::Won(1));
    assert_eq!(classify(Some(Some(0)), 7000, 7000, w), Ending::Won(0));
    assert_eq!(classify(Some(None), 7000, 7000, w), Ending::MutualDefeat);
    // Capped at 60,000: the last loss decides between the two labels, exactly at the window.
    assert_eq!(classify(None, 60_000, 50_000, w), Ending::Stalemate);
    assert_eq!(classify(None, 60_000, 50_001, w), Ending::Capped);
    assert_eq!(classify(None, 60_000, 59_999, w), Ending::Capped);
    assert_eq!(classify(None, 60_000, 0, w), Ending::Stalemate);
    // A game shorter than the window with no loss cannot be called a stalemate.
    assert_eq!(classify(None, 3000, 0, w), Ending::Capped);
    // The window is the caller's.
    assert_eq!(classify(None, 3000, 0, 2000), Ending::Stalemate);
    // A result beats the cap labels.
    assert_eq!(classify(Some(Some(0)), 60_000, 0, w), Ending::Won(0));
}

#[test]
fn only_a_win_or_a_mutual_defeat_is_decided() {
    assert!(Ending::Won(0).decided() && Ending::Won(1).decided());
    assert!(Ending::MutualDefeat.decided());
    assert!(!Ending::Stalemate.decided() && !Ending::Capped.decided());
    let labels = [
        Ending::Won(0).label(),
        Ending::MutualDefeat.label(),
        Ending::Stalemate.label(),
        Ending::Capped.label(),
    ];
    assert_eq!(labels, ["won", "mutual defeat", "stalemate", "capped"]);
}

// --- the numbers -----------------------------------------------------------------------------------

#[test]
fn the_median_is_the_middle_value_and_the_rounded_down_average_of_two() {
    assert_eq!(median(&[]), None);
    assert_eq!(median(&[7]), Some(7));
    assert_eq!(median(&[9, 1, 5]), Some(5));
    assert_eq!(median(&[4, 1, 3, 2]), Some(2));
    assert_eq!(median(&[10, 20, 31, 5]), Some(15));
    assert_eq!(median(&[1, 2]), Some(1));
}

#[test]
fn permille_rounds_down_and_prints_with_one_decimal() {
    assert_eq!(permille(0, 0), 0);
    assert_eq!(permille(5, 0), 0);
    assert_eq!(permille(1, 3), 333);
    assert_eq!(permille(2, 3), 666);
    assert_eq!(permille(7, 7), 1000);
    assert_eq!(permille(u32::MAX, u32::MAX), 1000);
    assert_eq!(percent_text(0), "0.0%");
    assert_eq!(percent_text(523), "52.3%");
    assert_eq!(percent_text(1000), "100.0%");
}

fn record(
    index: u32,
    factions: [Faction; 2],
    ending: Ending,
    ticks: u64,
    checksum: u64,
) -> GameRecord {
    GameRecord {
        index,
        world_seed: u64::from(index),
        ai_seed: u64::from(index) + 100,
        factions,
        ai_factions: factions,
        ending,
        defeat_tick: ending.decided().then_some(ticks),
        ticks,
        last_loss: ticks,
        final_checksum: checksum,
        ais: vec![
            AiRecord {
                player: 0,
                digest: 1,
                attacks: 2,
                commands: 0,
                profile: String::new(),
            },
            AiRecord {
                player: 1,
                digest: 3,
                attacks: 4,
                commands: 0,
                profile: String::new(),
            },
        ],
    }
}

fn sample() -> Vec<GameRecord> {
    vec![
        record(0, [M, V], Ending::Won(0), 9000, 0xA),
        record(1, [V, M], Ending::Won(0), 7000, 0xB),
        record(2, [M, M], Ending::Won(1), 12_000, 0xC),
        record(3, [V, V], Ending::Stalemate, 60_000, 0xA),
        record(4, [M, V], Ending::Capped, 60_000, 0xD),
        record(5, [V, M], Ending::MutualDefeat, 8000, 0xB),
    ]
}

#[test]
fn the_summary_counts_wins_draws_durations_and_distinct_checksums() {
    let s = summarize(&sample());
    assert_eq!(s.games, 6);
    // Seat 0 won games 0 and 1, seat 1 won game 2.
    assert_eq!(s.seat_wins, [2, 1]);
    assert_eq!((s.mutual, s.stalemates, s.capped), (1, 1, 1));
    assert_eq!(s.draws(), 3);
    // Faction rows: only the four games between different factions (0, 1, 4, 5). Game 0: M won (seat 0).
    // Game 1: seat 0 is V, and V won. Games 4 and 5 are draws.
    assert_eq!(
        s.factions,
        vec![
            FactionRow {
                faction: "Meridian".into(),
                games: 4,
                wins: 1
            },
            FactionRow {
                faction: "Veyran".into(),
                games: 4,
                wins: 1
            },
        ]
    );
    // Durations of the decided games only: 9000, 7000, 12000 and 8000.
    assert_eq!(
        s.durations,
        Some(Durations {
            min: 7000,
            median: 8500,
            max: 12_000
        })
    );
    // 0xA, 0xB, 0xC, 0xD: four values in six games.
    assert_eq!(s.distinct_checksums, 4);
    // Outcome keys (ending, ticks, last loss, attacks): all six differ here, and with digests too.
    assert_eq!((s.distinct_outcomes, s.distinct_with_digests), (6, 6));
    // Per seating, ascending by name: M v M, M v V (games 0 and 4), V v M (1 and 5), V v V.
    let names: Vec<(&str, u32, u32)> = s
        .seatings
        .iter()
        .map(|r| (r.seating.as_str(), r.games, r.distinct_checksums))
        .collect();
    assert_eq!(
        names,
        [
            ("Meridian v Meridian", 1, 1),
            ("Meridian v Veyran", 2, 2),
            ("Veyran v Meridian", 2, 1),
            ("Veyran v Veyran", 1, 1),
        ]
    );
    // The last loss (here: the tick count of each game) over the six games.
    assert_eq!(
        s.last_loss,
        Some(Durations {
            min: 7000,
            median: 10_500,
            max: 60_000
        })
    );
}

#[test]
fn games_played_alike_share_an_outcome_key_and_the_digests_tell_them_apart() {
    // Five games with the same ending, ticks, last loss and attacks, only the final checksum and the AI
    // digests differ: one outcome, as many digest keys as games.
    let mut games: Vec<GameRecord> = (0..5)
        .map(|i| record(i, [M, V], Ending::Won(0), 5000, 0x90 + u64::from(i)))
        .collect();
    for (i, g) in games.iter_mut().enumerate() {
        g.ais[0].digest = 100 + i as u64;
    }
    let s = summarize(&games);
    assert_eq!(s.distinct_checksums, 5);
    assert_eq!((s.distinct_outcomes, s.distinct_with_digests), (1, 5));
    assert_eq!(s.seatings[0].distinct_outcomes, 1);
    // A different attack count or last loss is a different outcome.
    games[1].ais[1].attacks = 9;
    games[2].last_loss = 4000;
    assert_eq!(summarize(&games).distinct_outcomes, 3);
    assert_eq!(summarize(&[]).distinct_outcomes, 0);
}

#[test]
fn a_tick_level_last_loss_shares_a_coarse_key_and_an_ending_or_attack_count_does_not() {
    // Capped games whose last loss differs by a few ticks within one 1000-tick bucket.
    let mut games: Vec<GameRecord> = (0..4)
        .map(|i| record(i, [M, V], Ending::Capped, 60_000, 0x50 + u64::from(i)))
        .collect();
    for (i, g) in games.iter_mut().enumerate() {
        g.last_loss = 41_200 + i as u64 * 7;
    }
    let s = summarize(&games);
    assert_eq!(s.distinct_outcomes, 4);
    assert_eq!((s.distinct_coarse, s.distinct_coarse_loss), (1, 1));
    assert_eq!(
        (
            s.seatings[0].distinct_coarse,
            s.seatings[0].distinct_coarse_loss
        ),
        (1, 1)
    );
    assert_eq!(games[0].coarse_loss_key().3, 41_000);
    // A last loss in another bucket splits only the rounded key.
    games[1].last_loss = 42_000;
    let s = summarize(&games);
    assert_eq!((s.distinct_coarse, s.distinct_coarse_loss), (1, 2));
    // A different ending or attack count splits both coarse keys.
    games[2].ending = Ending::Stalemate;
    games[3].ais[0].attacks = 7;
    let s = summarize(&games);
    assert_eq!((s.distinct_coarse, s.distinct_coarse_loss), (3, 4));
    assert_eq!(summarize(&[]).distinct_coarse, 0);
}

#[test]
fn distinct_checksums_count_values_not_games() {
    let same: Vec<GameRecord> = (0..5)
        .map(|i| record(i, [M, V], Ending::Won(0), 5000 + u64::from(i), 0x77))
        .collect();
    assert_eq!(summarize(&same).distinct_checksums, 1);
    let all_different: Vec<GameRecord> = (0..5)
        .map(|i| record(i, [M, V], Ending::Won(0), 5000, 0x70 + u64::from(i)))
        .collect();
    assert_eq!(summarize(&all_different).distinct_checksums, 5);
    assert_eq!(summarize(&[]).distinct_checksums, 0);
}

#[test]
fn a_summary_of_no_decided_game_has_no_durations_and_a_mirror_only_run_no_faction_rows() {
    let capped = vec![record(2, [M, M], Ending::Capped, 60_000, 1)];
    let s = summarize(&capped);
    assert_eq!(s.durations, None);
    assert!(s.factions.is_empty());
    assert_eq!(s.seat_wins, [0, 0]);
}

// --- output ----------------------------------------------------------------------------------------

fn report() -> Report {
    Report::new("m1_duel", 0xABC, 60_000, 10_000, sample())
}

#[test]
fn the_table_has_a_line_per_game_and_the_summary() {
    let text = render_table(&report());
    assert!(
        text.starts_with(
            "tournament: m1_duel, 6 games, seed 0x0000000000000abc, cap 60000 ticks\n"
        )
    );
    for r in sample() {
        assert!(
            text.contains(&log::hex(r.final_checksum)),
            "game {}",
            r.index
        );
    }
    for needle in [
        "Meridian v Veyran",
        "seat 0 won",
        "seat 1 won",
        "mutual defeat",
        "stalemate",
        "capped",
        "seat 0 wins:    2 of 6 (33.3%)",
        "seat 1 wins:    1 of 6 (16.6%)",
        "Meridian wins: 1 of 4 games against the other faction (25.0%)",
        "draws:         3 (1 mutual defeat, 1 stalemate, 1 capped)",
        "duration:      min 7000 / median 8500 / max 12000 ticks (decided games)",
        "distinct final checksums: 4 of 6 games",
        "distinct outcomes (ending, ticks, last loss, attacks): 6 of 6 games; with the AI digests: 6",
        "distinct coarse outcomes (ending, ticks, attacks): 6 of 6 games; with the last loss rounded down to 1000 ticks: 6",
        "  Meridian v Veyran: 2 games, 2 checksums, 2 outcomes, 2 with digests",
        "last loss:     min 7000 / median 10500 / max 60000 ticks",
    ] {
        assert!(text.contains(needle), "missing {needle:?} in\n{text}");
    }
    // Deterministic: the same report prints the same text.
    assert_eq!(text, render_table(&report()));
}

#[test]
fn the_json_has_the_games_the_summary_and_the_host_last() {
    let host = Host {
        os: "linux",
        arch: "x86_64",
        debug_assertions: true,
        elapsed_micros: 12_345_000,
    };
    let walls = vec![1000u128; 6];
    let text = render_json(&report(), &host, &walls);
    assert!(text.starts_with("{\n  \"format\": 1,\n  \"kind\": \"tournament\",\n"));
    assert!(text.ends_with("}\n"));
    let keys = [
        "\"match\": \"m1_duel\"",
        "\"seed\": \"0x0000000000000abc\"",
        "\"cap\": 60000",
        "\"stalemate_window\": 10000",
        "\"games\": [",
        "\"summary\": {",
        "\"seat_wins\": [2, 1]",
        "\"seat_win_permille\": [333, 166]",
        "\"mutual_defeats\": 1",
        "\"stalemates\": 1",
        "\"capped\": 1",
        "\"draws\": 3",
        "\"duration_ticks\": {\"min\": 7000, \"median\": 8500, \"max\": 12000}",
        "\"distinct_final_checksums\": 4",
        "\"distinct_outcomes\": 6",
        "\"distinct_outcomes_with_digests\": 6",
        "\"distinct_coarse\": 6",
        "\"distinct_coarse_last_loss_1000\": 6",
        "\"seatings\": [{\"seating\": \"Meridian v Meridian\", \"games\": 1,",
        "\"last_loss_ticks\": {\"min\": 7000, \"median\": 10500, \"max\": 60000}",
        "\"host\": {",
        "\"game_elapsed_ms\": [1, 1, 1, 1, 1, 1]",
    ];
    let mut at = 0;
    for key in keys {
        let found = text[at..]
            .find(key)
            .unwrap_or_else(|| panic!("{key} missing or out of order in\n{text}"));
        at += found;
    }
    // One object per game, in order, each with every field.
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.trim_start().starts_with("{\"index\""))
        .collect();
    assert_eq!(lines.len(), 6);
    for (i, line) in lines.iter().enumerate() {
        assert!(line.contains(&format!("{{\"index\": {i},")), "{line}");
        for field in [
            "\"world_seed\": \"0x",
            "\"ai_seed\": \"0x",
            "\"seats\": [\"",
            "\"ending\": \"",
            "\"winner_seat\": ",
            "\"defeat_tick\": ",
            "\"ticks\": ",
            "\"last_loss\": ",
            "\"final_checksum\": \"0x",
            "\"ais\": [{\"player\": 0, \"digest\": \"0x0000000000000001\", \"attacks\": 2}, {\"player\": 1,",
        ] {
            assert!(line.contains(field), "game {i} lacks {field}: {line}");
        }
    }
    assert!(lines[0].contains("\"winner_seat\": 0") && lines[0].contains("\"defeat_tick\": 9000"));
    assert!(lines[2].contains("\"winner_seat\": 1"));
    assert!(
        lines[3].contains("\"winner_seat\": null") && lines[3].contains("\"defeat_tick\": null")
    );
    // The deterministic part is everything above "host": no wall time in it.
    let (head, tail) = text.split_once("  \"host\": {").unwrap();
    assert!(!head.contains("elapsed") && !head.contains("12345"));
    assert!(tail.contains("\"elapsed_ms\": 12345"));
    // Braces and brackets balance.
    for (open, close) in [('{', '}'), ('[', ']')] {
        assert_eq!(
            text.matches(open).count(),
            text.matches(close).count(),
            "{open}{close}"
        );
    }
}

// --- the command line ------------------------------------------------------------------------------

#[test]
fn the_arguments_are_parsed_with_their_defaults() {
    let a = args(&["assets/matches/m1_duel.ron"]);
    assert_eq!(a.match_file, PathBuf::from("assets/matches/m1_duel.ron"));
    assert_eq!(a.games, DEFAULT_GAMES);
    assert_eq!(a.cap, DEFAULT_CAP);
    assert_eq!(a.stalemate, STALEMATE_WINDOW);
    assert_eq!((a.seed, a.json, a.assets), (None, None, None));
    assert_eq!(
        (DEFAULT_GAMES, DEFAULT_CAP, STALEMATE_WINDOW),
        (8, 60_000, 10_000)
    );

    let a = args(&[
        "x.ron",
        "--games",
        "100",
        "--cap=5000",
        "--seed",
        "0x1F_00",
        "--json",
        "out/t.json",
        "--assets",
        "assets",
        "--stalemate",
        "700",
    ]);
    assert_eq!(
        (a.games, a.cap, a.seed, a.stalemate),
        (100, 5000, Some(0x1F00), 700)
    );
    assert_eq!(a.json, Some(PathBuf::from("out/t.json")));
    assert_eq!(a.assets, Some(PathBuf::from("assets")));
    assert_eq!(args(&["x.ron", "--seed", "12_345"]).seed, Some(12_345));
    assert_eq!(parse_args(strings(&["--help"])).unwrap(), Parsed::Help);
}

#[test]
fn bad_arguments_are_refused_with_a_reason() {
    for (words, reason) in [
        (vec![], "no match file"),
        (vec!["a.ron", "b.ron"], "more than one"),
        (vec!["a.ron", "--games"], "needs a value"),
        (vec!["a.ron", "--games", "0"], "between 1 and"),
        (vec!["a.ron", "--games", "x"], "whole number"),
        (vec!["a.ron", "--games", "100001"], "between 1 and"),
        (vec!["a.ron", "--cap", "0"], "between 1 and"),
        (vec!["a.ron", "--seed", "-1"], "whole number"),
        (vec!["a.ron", "--seed", "0xg"], "whole number"),
        (vec!["a.ron", "--games", "2", "--games", "3"], "given twice"),
        (vec!["a.ron", "--ticks", "3"], "unknown option"),
        (vec!["a.ron", "-x"], "unknown option"),
    ] {
        let e = parse_args(strings(&words)).unwrap_err();
        assert!(e.contains(reason), "{words:?}: {e}");
    }
}

#[test]
fn the_asset_directory_is_two_levels_above_the_match_file_unless_given() {
    let a = args(&["assets/matches/m1_duel.ron"]);
    assert_eq!(assets_for(&a).unwrap(), PathBuf::from("assets"));
    let a = args(&["/x/assets/matches/m1_duel.ron"]);
    assert_eq!(assets_for(&a).unwrap(), PathBuf::from("/x/assets"));
    let a = args(&["matches/m1_duel.ron"]);
    assert_eq!(assets_for(&a).unwrap(), PathBuf::from("."));
    let a = args(&["m1_duel.ron", "--assets", "elsewhere"]);
    assert_eq!(assets_for(&a).unwrap(), PathBuf::from("elsewhere"));
    assert!(assets_for(&args(&["m1_duel.ron"])).is_err());
}
