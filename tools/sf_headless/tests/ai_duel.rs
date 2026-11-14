//! WP9.2a: the duel of the shipped profile against its own past (`docs/rules/ai.md`, "Groups").
//!
//! `normal_old` is a frozen copy of the shipped profile as it was before the groups
//! (`tests/fixtures/normal_old.ron`, a test fixture and no shipped profile). A tournament reads the profile of
//! each seat from the match file (`controller: Ai(profile: ..)`), so a profile per seat needs no flag of its own:
//! a scratch asset directory with both profiles and a match file that names them. These tests pin that, the
//! fixture itself, and the rate of the commands in one duel game; the 100-game duels are run by
//! `sf_headless tournament` on such a directory (the recipe is in the documentation).

mod common;

use std::path::{Path, PathBuf};

use common::TempDir;
use sf_data::ai_profile::AiProfile;
use sf_data::units::Faction;
use sf_headless::tournament::{Ending, STALEMATE_WINDOW, load_setup, play_game, play_seeded};

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/normal_old.ron")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A scratch asset directory with the shipped definitions and map, the shipped profile `normal` and the frozen
/// `normal_old`, and the match file of `m1_duel` with seat `new_seat` (0 or 1) on `normal` and the other on
/// `normal_old`. Returns the directory and the match file.
fn duel_dir(dir: &TempDir, new_seat: usize) -> (PathBuf, PathBuf) {
    let root = dir.join(&format!("assets{new_seat}"));
    copy_dir(&assets().join("config"), &root.join("config"));
    copy_dir(&assets().join("maps/m1"), &root.join("maps/m1"));
    std::fs::create_dir_all(root.join("ai")).unwrap();
    std::fs::copy(assets().join("ai/normal.ron"), root.join("ai/normal.ron")).unwrap();
    std::fs::copy(fixture(), root.join("ai/normal_old.ron")).unwrap();
    let text = std::fs::read_to_string(assets().join("matches/m1_duel.ron")).unwrap();
    // The profile of each seat is named on a line `controller: Ai(profile: "normal"),` (the comments that
    // mention it are left alone); seat 0 comes first in the file.
    let mut seats = ["normal", "normal"];
    seats[1 - new_seat] = "normal_old";
    let mut next = 0;
    let mut out = String::new();
    for line in text.lines() {
        if line
            .trim_start()
            .starts_with("controller: Ai(profile: \"normal\")")
        {
            out.push_str(&line.replace("\"normal\"", &format!("\"{}\"", seats[next])));
            next += 1;
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    assert_eq!(next, 2, "m1_duel.ron has two computer seats");
    let matches = root.join("matches");
    std::fs::create_dir_all(&matches).unwrap();
    let path = matches.join("duel.ron");
    std::fs::write(&path, out).unwrap();
    (root, path)
}

#[test]
fn the_frozen_profile_is_the_shipped_one_with_the_numbers_of_wp9_2a_off() {
    // The fixture parsed equals the shipped profile with its name changed and the six numbers of the groups put
    // back to what a profile that does not write them has: so "normal-old" is exactly "normal without WP9.2a".
    let old = AiProfile::from_ron(&std::fs::read_to_string(fixture()).unwrap()).unwrap();
    let mut new = AiProfile::embedded();
    assert!(
        new.army.groups_on(),
        "the shipped profile turns the groups on"
    );
    assert!(!old.army.groups_on());
    new.name = "normal_old".to_string();
    let a = &mut new.army;
    a.home_guard_permille = 0;
    a.stage_distance = 0;
    a.reinforce_value = 0;
    a.defend_ratio_permille = 0;
    a.timing_jitter_permille = 0;
    a.gather_refinery_gap = 0;
    a.gather_everyone = false;
    a.stage_permille = old.army.stage_permille;
    a.stage_patience = old.army.stage_patience;
    assert_eq!(old, new);
}

#[test]
fn a_profile_per_seat_is_played_on_its_own_seat_in_both_orders() {
    let dir = TempDir::new("ai_duel_seats");
    for new_seat in [0usize, 1] {
        let (root, path) = duel_dir(&dir, new_seat);
        let setup = load_setup(&root, &path).unwrap();
        let r = play_game(&setup, 1, 0, 300, STALEMATE_WINDOW).unwrap();
        let names: Vec<&str> = r.ais.iter().map(|a| a.profile.as_str()).collect();
        let mut want = ["normal", "normal"];
        want[1 - new_seat] = "normal_old";
        assert_eq!(names, want, "new profile on seat {new_seat}");
        // The faction swap does not move the profile: game 1 puts the other faction on each seat.
        let r = play_seeded(
            &setup,
            1,
            [Faction::Veyran, Faction::Meridian],
            (1, 2),
            300,
            STALEMATE_WINDOW,
        )
        .unwrap();
        let names: Vec<&str> = r.ais.iter().map(|a| a.profile.as_str()).collect();
        assert_eq!(
            names, want,
            "swapped factions, new profile on seat {new_seat}"
        );
    }
}

/// The commands per 1,000 ticks of an AI that sent `commands` in `ticks` ticks, in hundredths.
fn per_thousand(commands: u64, ticks: u64) -> u64 {
    commands * 100_000 / ticks
}

#[test]
fn the_commands_of_a_duel_game_stay_at_a_bounded_rate() {
    // One duel game, new against old, 40,000 ticks (the armies are out in it: the old one launches at about
    // 9,700, the new one a little later). A spam of army orders (an order each army turn for every unit, or a
    // march command with no chunk bound sent again and again) would multiply the rate; the measured numbers are in
    // docs/rules/ai.md "Groups", and the bound is 1.3 times the higher of them.
    let dir = TempDir::new("ai_duel_rate");
    let (root, path) = duel_dir(&dir, 0);
    let setup = load_setup(&root, &path).unwrap();
    let r = play_game(&setup, 0, 0, 40_000, STALEMATE_WINDOW).unwrap();
    let rates: Vec<u64> = r
        .ais
        .iter()
        .map(|a| per_thousand(a.commands, r.ticks))
        .collect();
    println!(
        "commands {:?} in {} ticks, per 1,000 ticks (hundredths) {rates:?}, ending {:?}",
        r.ais.iter().map(|a| a.commands).collect::<Vec<_>>(),
        r.ticks,
        r.ending
    );
    assert!(r.ticks >= 30_000 || matches!(r.ending, Ending::Won(_)));
    for (seat, rate) in rates.iter().enumerate() {
        assert!(*rate > 0, "seat {seat} sent commands");
        assert!(
            *rate <= RATE_BOUND,
            "seat {seat}: {rate} hundredths of a command per 1,000 ticks is over the bound {RATE_BOUND}"
        );
    }
}

/// The most commands per 1,000 ticks (in hundredths) an AI may send in the duel game above: the higher measured
/// rate times 1.3, rounded up to one decimal (WP9.2a-2, shipped numbers: 93 and 58 commands in 40,000 ticks, 2.32 and
/// 1.45 per 1,000; 2.32 x 1.3 = 3.02, so 3.1). It was 2.3 (fix round 1 of WP9.2a: 65 and 52 commands in 37,931
/// ticks, 1.71 and 1.37). Cause of the move, measured: with `gather_everyone` off the game is the old one (65 and 52 in
/// 37,931 ticks, won by seat 0); with it on, the game is not decided in 40,000 ticks and seat 0 sends 33 single-unit
/// `Move` orders to the gather cell instead of 9 (the units made beside the cell are sent too): the extra 28 commands
/// are those, not a spam. A doubling of the orders still fails it.
const RATE_BOUND: u64 = 310;
