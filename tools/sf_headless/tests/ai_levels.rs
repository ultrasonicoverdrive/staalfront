//! WP9.4a, the difficulty levels `easy` and `hard` (`docs/rules/ai.md`, "Difficulty levels (WP9.4a)"): the two
//! shipped profile files (`assets/ai/easy.ron`, `assets/ai/hard.ron`) on the scenarios `ai_vs_passive_easy` and
//! `ai_vs_passive_hard` (the AI against the static opponent of m1, as `ai_vs_passive`), with the win tick, the checksum
//! and the AI digest pinned, and the check that the scenarios play the shipped files. The numbers of the files
//! themselves are pinned in `sf_data` and `sf_ai`; the `--profiles` flag is tested in `tournament.rs`.

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::run::{Finished, RunOptions, run_full, run_observed};
use sf_headless::scenario::Scenario;
use sf_sim::{Command, PlayerId, Verdict};

const P0: PlayerId = PlayerId(0);

const EASY_EXPECTED: &str = include_str!("../../../assets/scenarios/ai_vs_passive_easy.expected");
const HARD_EXPECTED: &str = include_str!("../../../assets/scenarios/ai_vs_passive_hard.expected");
/// `Ai::digest` of player 0 at the end of each scenario.
const EASY_DIGEST: u64 = 0xbb6d_2465_77c1_6583;
const HARD_DIGEST: u64 = 0x9c65_1299_32d6_0bda;
/// The tick the match names the AI the winner.
const EASY_WON: u64 = 13_256;
const HARD_WON: u64 = 9_093;
/// The length of the scenarios: the AI must have won before it.
const EASY_TICKS: u64 = 20_000;
const HARD_TICKS: u64 = 16_000;

fn load(name: &str) -> Scenario {
    Scenario::load(&scenario_path(&format!("{name}.ron"))).unwrap()
}

fn go(s: &Scenario) -> Finished {
    run_full(
        s,
        &RunOptions {
            ticks: s.ticks,
            every: 100,
        },
    )
    .unwrap()
}

/// The tick the match named player 0 the winner.
fn won_at(s: &Scenario) -> Option<u64> {
    let mut won = None;
    run_observed(
        s,
        &RunOptions {
            ticks: s.ticks,
            every: 100,
        },
        |w, _| {
            if won.is_none()
                && let Some(r) = w.match_result()
                && r.verdict == Verdict::Won(P0)
            {
                won = Some(r.tick.0);
            }
        },
    )
    .unwrap();
    won
}

/// The kinds of the structures the AI ordered, in order (`BuildStructure` commands).
fn builds(f: &Finished) -> Vec<u16> {
    f.ai_commands
        .iter()
        .filter_map(|c| match c.2 {
            Command::BuildStructure { kind, .. } => Some(kind.0),
            _ => None,
        })
        .collect()
}

#[test]
fn the_scenarios_play_the_shipped_profile_files() {
    // The scenarios keep a copy of the profile next to their definitions; it is the shipped file today, so a change of
    // `assets/ai/easy.ron` or `hard.ron` fails here until the scenario is re-pinned on purpose.
    for name in ["easy", "hard"] {
        let shipped = std::fs::read_to_string(scenario_path(&format!("../ai/{name}.ron"))).unwrap();
        let copy = std::fs::read_to_string(scenario_path(&format!(
            "ai_vs_passive_{name}_defs/{name}.ron"
        )))
        .unwrap();
        assert_eq!(shipped, copy, "{name}");
        let s = load(&format!("ai_vs_passive_{name}"));
        assert_eq!(s.ais[0].state().profile.name, name);
    }
}

#[test]
fn the_checksums_and_ai_digests_of_the_two_level_scenarios_are_pinned() {
    for (name, expected, digest, ticks) in [
        ("ai_vs_passive_easy", EASY_EXPECTED, EASY_DIGEST, EASY_TICKS),
        ("ai_vs_passive_hard", HARD_EXPECTED, HARD_DIGEST, HARD_TICKS),
    ] {
        let s = load(name);
        assert_eq!(s.ticks, ticks, "{name}");
        let f = go(&s);
        println!(
            "{name}: {:#018x} digest {:#018x}",
            f.outcome.final_checksum,
            f.ais[0].digest()
        );
        assert_eq!(
            f.outcome.final_checksum,
            parse_checksum(expected).unwrap(),
            "{name}"
        );
        assert_eq!(f.outcome.ai_digests, [(0, digest)], "{name}");
    }
}

#[test]
fn easy_wins_against_the_static_opponent_before_the_end_and_builds_its_whole_list() {
    let s = load("ai_vs_passive_easy");
    let won = won_at(&s);
    println!("easy won at {won:?}");
    assert_eq!(won, Some(EASY_WON));
    const { assert!(EASY_WON < EASY_TICKS) };
    // The goal list of easy (Meridian): power plant, refinery, vehicle factory, barracks, a second power plant, and no
    // second refinery; the kinds are those of unit_defs.ron (1, 2, 4, 3).
    let f = go(&s);
    let b = builds(&f);
    println!("easy builds {b:?}");
    assert_eq!(b, [1, 2, 4, 1, 3]);
    let mut sorted = b.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, [1, 1, 2, 3, 4]);
}

#[test]
fn hard_wins_against_the_static_opponent_before_the_end_and_builds_the_whole_shipped_list() {
    let s = load("ai_vs_passive_hard");
    let won = won_at(&s);
    println!("hard won at {won:?}");
    assert_eq!(won, Some(HARD_WON));
    const { assert!(HARD_WON < HARD_TICKS) };
    let f = go(&s);
    let b = builds(&f);
    println!("hard builds {b:?}");
    // The shipped list (two power plants, two refineries, vehicle factory, barracks); with two builds at once the
    // order is not the list's.
    assert_eq!(b, [1, 1, 2, 3, 2, 4]);
    let mut sorted = b.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, [1, 1, 2, 2, 3, 4]);
}
