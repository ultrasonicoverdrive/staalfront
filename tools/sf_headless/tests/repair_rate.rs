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
use sf_headless::tournament::{load_setup, prepare};
use sf_sim::PlayerId;

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

/// A scratch asset directory with the shipped data. The repair rate is **not** in it: the world is given
/// `RepairSettings::embedded()`, compiled in from `assets/config/repair.ron`, so a copy of that file here is
/// never read. (The rates 2 and 3 of WP9.1e were measured by editing the shipped file and rebuilding.)
fn scratch_assets(dir: &TempDir) -> PathBuf {
    let root = dir.join("assets");
    copy_dir(&assets().join("config"), &root.join("config"));
    copy_dir(&assets().join("maps/m1"), &root.join("maps/m1"));
    std::fs::create_dir_all(root.join("ai")).unwrap();
    let mut ai = std::fs::read_to_string(assets().join("ai/normal.ron")).unwrap();
    // No attack is launched within the 40,000 ticks played (72,000 is the profile maximum): the tanks stay where the fixture puts them.
    ai = set_number(&ai, "first_attack_tick", 72_000);
    std::fs::write(root.join("ai/normal.ron"), ai).unwrap();
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

/// What a game of the fixture showed.
struct Run {
    /// The tick the enemy centre fell at, `None` when it stood after `cap` ticks.
    kill: Option<u64>,
    /// The longest stretch of ticks in which the centre's hit points made no new low, from the first hit to
    /// the kill (or the cap), seen every tick. This is what `army.rs` `stuck` compares with
    /// `attack_patience` (it sees the hit points at the AI's thinks, every 10 ticks, so its number is
    /// about this one: 30 to 40 ticks sampled at the thinks, 31 to 40 per tick).
    longest_no_new_low: u64,
}

fn run(tanks: u32, cap: u64) -> Run {
    let dir = TempDir::new("repair_rate");
    let assets = scratch_assets(&dir);
    let path = dir.join("repair_rate.ron");
    std::fs::write(&path, fixture(tanks)).unwrap();
    let setup = load_setup(&assets, &path).unwrap();
    let seat1 = PlayerId(1); // the fixture's player 1
    let factions = setup.file_factions();
    let sim = prepare(&setup, factions, 1, 1).unwrap();
    let (mut world, mut ais) = (sim.world, sim.ais);
    let centre = world
        .snapshot()
        .iter()
        .find(|v| v.owner == seat1)
        .expect("seat 1 has its centre")
        .id;
    let mut wanted = Vec::new();
    let mut low = world.hp_of(centre).expect("the centre stands");
    let (mut since, mut longest, mut hit) = (0u64, 0u64, false);
    let mut kill = None;
    while world.tick().0 < cap {
        for ai in &mut ais {
            if ai.due(world.tick()) {
                wanted.clear();
                ai.think(&world, &mut wanted);
                for command in wanted.drain(..) {
                    world.schedule(world.tick(), ai.player(), command).unwrap();
                }
            }
        }
        world.step();
        let now = world.tick().0;
        if world.match_result().is_some() {
            kill = Some(now);
            if hit {
                longest = longest.max(now - since);
            }
            break;
        }
        let hp = world.hp_of(centre).unwrap_or(0);
        if hp < low {
            // The first hit starts the clock; every later new low restarts it.
            if hit {
                longest = longest.max(now - since);
            }
            (low, since, hit) = (hp, now, true);
        }
    }
    if kill.is_none() && hit {
        longest = longest.max(world.tick().0 - since);
    }
    Run {
        kill,
        longest_no_new_low: longest,
    }
}

/// The shipped `attack_patience` (assets/ai/normal.ron).
fn attack_patience() -> u64 {
    let text = std::fs::read_to_string(assets().join("ai/normal.ron")).unwrap();
    let at = text.find("attack_patience:").unwrap() + "attack_patience:".len();
    let to = at + text[at..].find(',').unwrap();
    text[at..to].trim().parse().unwrap()
}

/// With the shipped repair rate (read by the world through `embedded()`) a centre that repairs itself for free
/// keeps making new hit-point lows under 2 to 12 idle light tanks and falls. The AI's `stuck` rule
/// (`army.rs`: the victim's hit points made no new low for `attack_patience` ticks) therefore never fires in
/// this fixture: the longest stretch without a new low is a few dozen ticks, far under 2,400. Measured in
/// WP9.1e (kill tick for 2/3/4/5/6/8/12 tanks, 40,000 ticks played): at 3 hit points a tick never / never / never
/// / never / 6,052 / 851 / 332, and where it stood the hit points never went below the first low again
/// (longest stretch without a new low 39,549 to 39,829 ticks: `stuck` would fire); at 2: never / never /
/// 10,251 / 1,771 / 971 / 531 / 290 (never for 2 and 3 tanks, same stretch of about 39,600 to 39,700); at 1 (shipped): 20,451 / 2,051 / 1,131 / 806 / 607 / 411 / 251.
/// A best case: tanks in range and never shot at, no towers, a defender with 0 credits.
#[test]
fn with_the_shipped_rate_idle_tanks_keep_taking_the_repairing_centre_down_and_stuck_never_fires() {
    let patience = attack_patience();
    assert_eq!(patience, 2400, "the shipped profile");
    let runs: Vec<_> = [2, 3, 4, 5, 6, 8, 12]
        .iter()
        .map(|&n| run(n, 40_000))
        .collect();
    let kills: Vec<_> = runs.iter().map(|r| r.kill).collect();
    assert_eq!(
        kills,
        [
            Some(20_451),
            Some(2_051),
            Some(1_131),
            Some(806),
            Some(607),
            Some(411),
            Some(251)
        ],
        "kill ticks for 2, 3, 4, 5, 6, 8 and 12 tanks"
    );
    for (n, r) in [2, 3, 4, 5, 6, 8, 12].iter().zip(&runs) {
        assert!(
            r.longest_no_new_low < patience,
            "{n} tanks: {} ticks without a new low",
            r.longest_no_new_low
        );
    }
}

/// The measurement of WP9.1e (`cargo test --release -p sf_headless --test repair_rate -- --ignored --nocapture`),
/// for whatever rate the shipped `repair.ron` has when the test binary is built.
#[test]
#[ignore = "a measurement, not a check: prints the kill tick and the longest stretch without a new low per tank count, at the rate the shipped file had when this was built"]
fn measure_kill_ticks() {
    for tanks in [2, 3, 4, 5, 6, 8, 12] {
        let r = run(tanks, 40_000);
        println!(
            "tanks {tanks:2}: kill {:?} longest_no_new_low {}",
            r.kill, r.longest_no_new_low
        );
    }
}
