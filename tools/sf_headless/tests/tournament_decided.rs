//! A tournament game that is decided, on a real world (WP9.5a fix round 1).
//!
//! The smoke games of `tests/tournament.rs` are capped before anything is decided (the normal AI needs about
//! 48,000 ticks to take a base, `docs/rules/ai.md`), so the `Won` and `MutualDefeat` branches, the defeat tick
//! and the winner-to-seat mapping would never run in CI. These games are decided in a few hundred ticks: a
//! match file in a scratch directory in which one seat starts with a construction centre and nothing else
//! and the other with its own centre and twelve light tanks standing beside that centre. Both are computer
//! controlled, with the shipped profile, on the shipped map and definitions.

mod common;

use std::path::{Path, PathBuf};

use common::TempDir;
use sf_data::units::Faction;
use sf_headless::tournament::{Ending, STALEMATE_WINDOW, load_setup, play_game, play_seeded};

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

/// The match file: `strong` (0 or 1) is the seat with the twelve tanks beside its own centre.
fn fixture(strong: usize) -> String {
    // Seat 0's centre is at (12, 68), seat 1's at (72, 14); a centre is 4 by 4 cells.
    let tanks_at = |cells: &[(u32, u32)]| {
        cells
            .iter()
            .map(|(x, y)| format!("(role: \"light_tank\", cell: ({x}, {y})),"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let near_seat_1 = tanks_at(&grid(67, 15));
    let near_seat_0 = tanks_at(&grid(17, 69));
    let (army0, army1, credits0, credits1) = if strong == 0 {
        (near_seat_0, String::new(), 4000, 0)
    } else {
        (String::new(), near_seat_1, 0, 4000)
    };
    format!(
        r#"(
    name: "decided_fixture",
    map: "m1",
    seed: 1,
    players: [
        (id: 0, faction: Meridian, credits: {credits0},
         objects: [(role: "construction_center", cell: (12, 68)), {army0}],
         controller: Ai(profile: "normal")),
        (id: 1, faction: Veyran, credits: {credits1},
         objects: [(role: "construction_center", cell: (72, 14)), {army1}],
         controller: Ai(profile: "normal")),
    ],
)"#
    )
}

/// Twelve cells: three columns of four from `(x, y)`.
fn grid(x: u32, y: u32) -> Vec<(u32, u32)> {
    (0..12).map(|i| (x + i / 4, y + i % 4)).collect()
}

/// `text` with the number after `key:` replaced (the value of the shipped profile may change; the key may not).
fn set_number(text: &str, key: &str, value: u32) -> String {
    let at = text
        .find(&format!("{key}:"))
        .unwrap_or_else(|| panic!("the shipped profile has no {key}"));
    let from = at + key.len() + 1;
    let to = from + text[from..].find(',').unwrap();
    format!("{} {value}{}", &text[..from], &text[to..])
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

/// A scratch asset directory: the shipped definitions and map, and a profile `normal` that is the shipped one
/// with its army timing shortened (attack at 800 value, from tick 100, after 300 ticks with 4 units), so the
/// army that stands at home in the fixture marches at once.
fn scratch_assets(dir: &TempDir) -> PathBuf {
    let root = dir.join("assets");
    copy_dir(&assets().join("config"), &root.join("config"));
    copy_dir(&assets().join("maps/m1"), &root.join("maps/m1"));
    std::fs::create_dir_all(root.join("ai")).unwrap();
    let mut text = std::fs::read_to_string(assets().join("ai/normal.ron")).unwrap();
    for (key, value) in [
        ("attack_value", 800),
        ("max_wait", 300),
        ("first_attack_tick", 100),
        // WP9.2a: the fixture is about a decided game in a few thousand ticks, not about the groups: staging, reinforcement and
        // partial defence are switched off here (with them on the strong seat 0 wins at 1,455 instead of 1,315).
        ("stage_distance", 0),
        ("reinforce_value", 0),
        ("defend_ratio_permille", 0),
    ] {
        text = set_number(&text, key, value);
    }
    std::fs::write(root.join("ai/normal.ron"), text).unwrap();
    root
}

fn setup(strong: usize, dir: &TempDir) -> sf_headless::tournament::Setup {
    let assets = scratch_assets(dir);
    let path = dir.join(&format!("decided_{strong}.ron"));
    std::fs::write(&path, fixture(strong)).unwrap();
    load_setup(&assets, &path).unwrap()
}

/// (strong seat, ticks the game ends at): pinned on the shipped definitions with base seed 1, game 0. A
/// change of the unit or weapon numbers moves them and is a re-pin, not a failure of the tournament.
// WP9.1c-3 re-pin: the fixture copies the shipped `normal.ron`, whose opening moved (seat 1 strong: 5,599 -> 5,546; seat 0 unchanged).
const DECIDED: [(usize, u64); 2] = [(0, 1315), (1, 5546)];

#[test]
fn the_seat_with_the_army_wins_and_the_tick_numbers_agree() {
    let dir = TempDir::new("tournament_decided");
    for (strong, ticks) in DECIDED {
        let s = setup(strong, &dir);
        let r = play_game(&s, 1, 0, 8000, STALEMATE_WINDOW).unwrap();
        // The winner is the seat the fixture gave the army, whichever seat that is: the mapping from the
        // player that won to the seat is checked on a real game.
        assert_eq!(r.ending, Ending::Won(strong), "strong seat {strong}");
        // All tick numbers are counts of ticks run: the game ends at the defeat, and the last loss is the
        // one that decided it (the construction centre).
        assert_eq!(r.ticks, ticks, "strong seat {strong}");
        assert_eq!(r.defeat_tick, Some(ticks));
        assert_eq!(r.last_loss, ticks);
        // The AI of the winning seat launched an attack; the other did not.
        assert!(
            r.ais[strong].attacks >= 1 && r.ais[1 - strong].attacks == 0,
            "{:?}",
            r.ais
        );
    }
}

#[test]
fn the_winner_is_the_seat_not_the_faction() {
    let dir = TempDir::new("tournament_decided_swap");
    let s = setup(0, &dir);
    // Game 1 puts Veyran on seat 0 and Meridian on seat 1; seat 0 still has the army and still wins.
    let r = play_seeded(
        &s,
        1,
        [Faction::Veyran, Faction::Meridian],
        (1, 2),
        8000,
        STALEMATE_WINDOW,
    )
    .unwrap();
    assert_eq!(r.factions, [Faction::Veyran, Faction::Meridian]);
    assert_eq!(r.ending, Ending::Won(0));
}
