//! Does the free repair of the construction centre out-heal a moderate wave? (WP9.1e, docs/rules/repair_sell.md)
//!
//! The fixture: seat 0 has its own centre and N light tanks standing beside the enemy centre; seat 1 has only its
//! centre and 0 credits, so its repair (the AI repairs below `repair_below_percent`) is the free repair of a
//! structure without a price. Both seats are computer controlled with the shipped profile whose army timing is
//! pushed out of reach (no attack is ever launched), so the tanks only do what an idle unit does: fire at what
//! is in reach.

mod common;

use std::path::{Path, PathBuf};

use common::TempDir;
use sf_headless::tournament::{Ending, STALEMATE_WINDOW, load_setup, play_game};

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
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

/// `text` with the number after `key:` replaced.
fn set_number(text: &str, key: &str, value: u32) -> String {
    let at = text
        .find(&format!("{key}:"))
        .unwrap_or_else(|| panic!("no {key}"));
    let from = at + key.len() + 1;
    let to = from + text[from..].find(',').unwrap();
    format!("{} {value}{}", &text[..from], &text[to..])
}

/// A scratch asset directory with the shipped data; `hp_per_tick` replaces the shipped repair rate when given
/// (the measurement of other rates; the pinned test passes `None` and so reads the shipped file).
fn scratch_assets(dir: &TempDir, hp_per_tick: Option<u32>) -> PathBuf {
    let root = dir.join("assets");
    copy_dir(&assets().join("config"), &root.join("config"));
    copy_dir(&assets().join("maps/m1"), &root.join("maps/m1"));
    std::fs::create_dir_all(root.join("ai")).unwrap();
    let mut ai = std::fs::read_to_string(assets().join("ai/normal.ron")).unwrap();
    // No attack is launched within the 20,000 ticks played (72,000 is the profile maximum): the tanks stay where the fixture puts them.
    ai = set_number(&ai, "first_attack_tick", 72_000);
    std::fs::write(root.join("ai/normal.ron"), ai).unwrap();
    if let Some(rate) = hp_per_tick {
        let path = root.join("config/repair.ron");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, set_number(&text, "hp_per_tick", rate)).unwrap();
    }
    root
}

/// The match file: `tanks` light tanks of seat 0 beside the centre of seat 1 at (72, 14) (4 by 4 cells).
fn fixture(tanks: u32) -> String {
    let cells = (0..tanks)
        .map(|i| {
            format!(
                "(role: \"light_tank\", cell: ({}, {})),",
                67 + i % 4,
                15 + i / 4
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"(
    name: "repair_rate",
    map: "m1",
    seed: 1,
    players: [
        (id: 0, faction: Meridian, credits: 0,
         objects: [(role: "construction_center", cell: (12, 68)), {cells}],
         controller: Ai(profile: "normal")),
        (id: 1, faction: Veyran, credits: 0,
         objects: [(role: "construction_center", cell: (72, 14))],
         controller: Ai(profile: "normal")),
    ],
)"#
    )
}

/// The tick the enemy centre falls at, or `None` when it has not fallen within `cap` ticks.
fn kill_tick(tanks: u32, hp_per_tick: Option<u32>, cap: u64) -> Option<u64> {
    let dir = TempDir::new("repair_rate");
    let assets = scratch_assets(&dir, hp_per_tick);
    let path = dir.join("repair_rate.ron");
    std::fs::write(&path, fixture(tanks)).unwrap();
    let setup = load_setup(&assets, &path).unwrap();
    let r = play_game(&setup, 1, 0, cap, STALEMATE_WINDOW).unwrap();
    match r.ending {
        Ending::Won(0) => Some(r.ticks),
        _ => None,
    }
}

/// The shipped `attack_patience` (assets/ai/normal.ron): ticks without progress after which an attack is
/// called off.
fn attack_patience() -> u64 {
    let text = std::fs::read_to_string(assets().join("ai/normal.ron")).unwrap();
    let at = text.find("attack_patience:").unwrap() + "attack_patience:".len();
    let to = at + text[at..].find(',').unwrap();
    text[at..to].trim().parse().unwrap()
}

/// With the shipped repair rate (`assets/config/repair.ron`, read by the world through `embedded()`): four
/// light tanks take down a centre that repairs itself for free, well before the attack would be called off,
/// and the kill tick falls with every tank added. Measured in WP9.1e (kill tick, 20,000 ticks played, tanks
/// 3/4/5/6/8/12): at 3 hit points a tick: never/never/never/6,051/851/331; at 2: never/9,451/1,770/969/530/288;
/// at 1 (shipped): 1,970/1,091/807/607/409/250.
#[test]
fn four_tanks_take_down_a_repairing_centre_before_the_attack_patience_runs_out() {
    let patience = attack_patience();
    assert_eq!(patience, 2400, "the shipped profile");
    let kills: Vec<_> = [3, 4, 5, 6, 8, 12]
        .iter()
        .map(|&n| kill_tick(n, None, 20_000))
        .collect();
    assert_eq!(
        kills,
        [
            Some(1970),
            Some(1091),
            Some(807),
            Some(607),
            Some(409),
            Some(250)
        ],
        "kill ticks for 3, 4, 5, 6, 8 and 12 tanks"
    );
    assert!(kills[1].unwrap() < patience);
}

/// Two tanks deal about 1.05 hit points a tick to a structure against the 1 of repair: the centre is not
/// taken down in 20,000 ticks. The rule is "three or more", not "any".
#[test]
fn two_tanks_cannot_take_down_a_repairing_centre() {
    assert_eq!(kill_tick(2, None, 20_000), None);
}

/// The measurement of WP9.1e (`cargo test --release -p sf_headless --test repair_rate -- --ignored --nocapture`).
#[test]
#[ignore = "a measurement, not a check: prints the kill tick per tank count and repair rate"]
fn measure_kill_ticks() {
    for tanks in [3, 4, 5, 6, 8, 12] {
        let t = kill_tick(tanks, None, 20_000);
        println!("tanks {tanks:2}: {t:?}");
    }
}
