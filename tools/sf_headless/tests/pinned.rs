//! The shipped scenarios against the checksums committed next to them.
//!
//! `assets/scenarios/<name>.expected` holds the final checksum of
//! `<name>.ron`. CI runs the same scenarios through the binary with
//! `--expect "$(cat <name>.expected)"` on Linux and on Windows, so one value
//! in the repository is checked on both systems; these tests check it in debug
//! and release builds of the library.
//!
//! # Changing a pinned value
//!
//! Only when the *specified behaviour* of the simulation (or the scenario
//! file, or the placement stream) changes on purpose: that is a deliberate,
//! reviewed act, never a fix to make a test pass. Say in the PR that and why
//! the checksum changes. To regenerate:
//!
//! ```text
//! cargo run --release -p sf_headless -- assets/scenarios/wander_5000.ron
//! ```
//!
//! and copy the `final:` value into the `.expected` file (one line, the hex
//! value only). Do the same for `command_mix`, `terrain_256`, `units_spawn`, `orders`,
//! `terrain_costs`, `pathing`, `pathing_256`, `bridge_200`, `crowd_jam`, `turning`, `replan`, `formation_move`, `weapons_reload`, `projectiles`, `armour_splash` and `tanks_vs_infantry`. A change in `sf_sim` also moves
//! the golden values in `crates/sf_sim/tests/golden.rs`.

mod common;

use common::{deterministic_part, scenario_path, string_field};
use sf_data::movement::MovementCosts;
use sf_data::terrain::TerrainTypes;
use sf_data::units::{
    DefFile, DefSources, EMBEDDED_CLASSES, EMBEDDED_UNITS, EMBEDDED_WEAPONS, UnitDefs,
};
use sf_headless::cli::parse_checksum;
use sf_headless::log::{self, Host};
use sf_headless::run::{RunOptions, run};
use sf_headless::scenario::Scenario;

const WANDER_5000_EXPECTED: &str = include_str!("../../../assets/scenarios/wander_5000.expected");
const COMMAND_MIX_EXPECTED: &str = include_str!("../../../assets/scenarios/command_mix.expected");
const TERRAIN_256_EXPECTED: &str = include_str!("../../../assets/scenarios/terrain_256.expected");
const UNITS_SPAWN_EXPECTED: &str = include_str!("../../../assets/scenarios/units_spawn.expected");
const ORDERS_EXPECTED: &str = include_str!("../../../assets/scenarios/orders.expected");
const TERRAIN_COSTS_EXPECTED: &str =
    include_str!("../../../assets/scenarios/terrain_costs.expected");
const PATHING_EXPECTED: &str = include_str!("../../../assets/scenarios/pathing.expected");
const PATHING_256_EXPECTED: &str = include_str!("../../../assets/scenarios/pathing_256.expected");
const BRIDGE_200_EXPECTED: &str = include_str!("../../../assets/scenarios/bridge_200.expected");
const CROWD_JAM_EXPECTED: &str = include_str!("../../../assets/scenarios/crowd_jam.expected");
const TURNING_EXPECTED: &str = include_str!("../../../assets/scenarios/turning.expected");
const REPLAN_EXPECTED: &str = include_str!("../../../assets/scenarios/replan.expected");
const FORMATION_MOVE_EXPECTED: &str =
    include_str!("../../../assets/scenarios/formation_move.expected");
const WEAPONS_RELOAD_EXPECTED: &str =
    include_str!("../../../assets/scenarios/weapons_reload.expected");
const PROJECTILES_EXPECTED: &str = include_str!("../../../assets/scenarios/projectiles.expected");
const ARMOUR_SPLASH_EXPECTED: &str =
    include_str!("../../../assets/scenarios/armour_splash.expected");
const TANKS_VS_INFANTRY_EXPECTED: &str =
    include_str!("../../../assets/scenarios/tanks_vs_infantry.expected");

fn check(name: &str, expected_text: &str) {
    let expected = parse_checksum(expected_text)
        .unwrap_or_else(|e| panic!("{name}.expected is not a checksum: {e}"));
    let scenario = Scenario::load(&scenario_path(&format!("{name}.ron"))).unwrap();
    let outcome = run(&scenario, &RunOptions::for_scenario(&scenario)).unwrap();
    assert_eq!(
        outcome.final_checksum,
        expected,
        "{name}: final checksum is {}, the committed value is {}",
        log::hex(outcome.final_checksum),
        log::hex(expected)
    );
    // The log says the same, in the format CI reads.
    let text = log::render(
        &outcome,
        &Host::current(std::time::Duration::from_millis(1)),
    );
    assert_eq!(string_field(&text, "final_checksum"), log::hex(expected));
    assert!(deterministic_part(&text).contains(&format!("\"scenario\": \"{name}\"")));
}

#[test]
fn command_mix_matches_the_pinned_checksum() {
    check("command_mix", COMMAND_MIX_EXPECTED);
}

/// The 256 by 256 cell playfield with its terrain layer and placed structures
/// (WP2.1). A new scenario: its value was pinned when it was added, the other
/// two did not change.
#[test]
fn terrain_256_matches_the_pinned_checksum() {
    check("terrain_256", TERRAIN_256_EXPECTED);
}

/// Every unit and structure role of the definitions spawned on the playfield
/// (WP3.2). A new scenario: its value was pinned when it was added; the other
/// three did not change.
#[test]
fn units_spawn_matches_the_pinned_checksum() {
    check("units_spawn", UNITS_SPAWN_EXPECTED);
}

/// Every order type, a shift queue, stop, scatter, an attack on a moving target
/// and one on a target that dies (WP3.4). A new scenario: its value was pinned
/// when it was added; the other four did not change. **Re-pinned at WP4.4**
/// (`0xbdcd8bcd753b5829` to the value in the file): its units have bodies now and
/// the ones sent to the same point no longer stand on it together. **Re-pinned at
/// WP4.5** (`0x38b6882e777306bb` to the value in the file): infantry turn at once,
/// wheeled vehicles have to move to turn (`docs/rules/turning.md`).
#[test]
fn orders_matches_the_pinned_checksum() {
    check("orders", ORDERS_EXPECTED);
}

/// Units of every movement class across road, grass, ore and a bridged river,
/// stopped by rock and water (WP4.1). Pinned when it was added; **re-pinned once at
/// WP4.2** (`0x8c43b875a877488c` to the value in the file), because its `Move`
/// orders now find their way round the river and the rock instead of stopping at
/// them (`docs/rules/movement.md`). **Re-pinned at WP4.5** (`0x2f74d916504df7cf` to the
/// value in the file): the turning rules of the classes (`docs/rules/turning.md`).
#[test]
fn terrain_costs_matches_the_pinned_checksum() {
    check("terrain_costs", TERRAIN_COSTS_EXPECTED);
}

/// Hierarchical path finding (WP4.2): a maze, a bridge, an island, a structure that
/// closes a gap, a crowd. A new scenario: pinned when it was added. **Re-pinned at
/// WP4.5** (`0x09f819e7bfeb22a2` to the value in the file): the turning rules.
#[test]
fn pathing_matches_the_pinned_checksum() {
    check("pathing", PATHING_EXPECTED);
}

/// 200 path requests on the 256 by 256 playfield (WP4.2). A new scenario: pinned when
/// it was added. **Re-pinned at WP4.4** (`0xa52ecf79cfd7b1ea` to the value in the
/// file): the 200 tanks have bodies now and push each other apart where their ways cross.
/// **Re-pinned at WP4.5** (`0x78acbca87df9a5f0` to the value in the file): the turning
/// rules.
#[test]
fn pathing_256_matches_the_pinned_checksum() {
    check("pathing_256", PATHING_256_EXPECTED);
}

/// 200 mixed units cross one bridge to a goal area on one order, on flowfields
/// (WP4.3). A new scenario: pinned when it was added (`0x5db78c7d785670f0`; `pathing`
/// runs with flowfields off, see its header). **Re-pinned at WP4.4** (to the value in
/// the file): the units have bodies, the bridge is a bottleneck and the arrival
/// slots are spaced for them, so the crossing takes about twice as long. **Re-pinned at
/// WP4.5** (`0xa55348bf0b519c50` to the value in the file):
/// the turning rules, infantry turning at once and wheeled vehicles in arcs. **Not moved by
/// WP4.6** (`docs/rules/formations.md`): the 200 are a crowd, an order that sends more than
/// `formation_max_units` (19) units makes no formation, not even among its five hover craft, so
/// the run ends on the value it had before formations.
#[test]
fn bridge_200_matches_the_pinned_checksum() {
    check("bridge_200", BRIDGE_200_EXPECTED);
}

/// Two groups pass head-on through a narrow gap, and a unit holding position stands
/// in a corridor while others route round it (WP4.4). A new scenario: pinned when it
/// was added. **Re-pinned at WP4.5** (`0xd5e9d6e45a14b0ef` to the value in the file): the
/// turning rules. **Re-pinned at WP4.6** (`0x69e7a06fe17a33e5` to the value in the file): the
/// two groups of 18 and the 8 tanks along the corridor are formations now (orders of at most
/// `formation_max_units`, 19); `tests/crowd_jam.rs` asserts what it did before and holds.
#[test]
fn crowd_jam_matches_the_pinned_checksum() {
    check("crowd_jam", CROWD_JAM_EXPECTED);
}

/// The hull turns before it drives and the turret turns apart from it (WP4.5): a tank
/// ordered behind itself pivots, a wheeled vehicle swings round in an arc, a rifleman
/// turns at once, a turret follows a moving target while its hull drives on. A new
/// scenario: pinned when it was added.
#[test]
fn turning_matches_the_pinned_checksum() {
    check("turning", TURNING_EXPECTED);
}

/// Replanning (WP4.7): a structure built in a corridor, a unit on hold with a way round
/// and with none, a target that walks away and one that disappears, a group whose goal is
/// built over. A new scenario: pinned when it was added. **Re-pinned at WP4.6**
/// (`0x2f05c63b9046ecc8` to the value in the file): its group order makes a formation; the
/// assertions of `tests/replan.rs` are unchanged and hold.
#[test]
fn replan_matches_the_pinned_checksum() {
    check("replan", REPLAN_EXPECTED);
}

/// A mixed group of 16 marches across open ground, through a passage and on to a goal as one
/// formation (WP4.6). A new scenario: pinned when it was added (review round 3: 16 units, under
/// the limit of 19 that the sweep of `tests/formation_sweep.rs` measured; it had 20 before).
#[test]
fn formation_move_matches_the_pinned_checksum() {
    check("formation_move", FORMATION_MOVE_EXPECTED);
}

/// Weapons and reload (WP5.1): riflemen, two tanks and two howitzers fire at what their attack
/// orders name, one shot per reload window, a burst of two, a minimum range, a victim shot down to
/// zero. Pinned in WP5.1; re-pinned in WP5.2 (`0x5a1bca1ebf1efa16` to `0x20a067e16e098d0b`) **only**
/// because the world hashes a projectile section (`J`) now: the run ends in the same state, as every
/// shell has landed by the last tick, but the shells take time on the way (`tests/weapons_reload.rs`
/// asserts the shots and impacts). Re-pinned again in WP5.3 (`0x20a067e16e098d0b` to
/// `0x59d08622efd30483`) **only** because the digest of the weapon table, which the weapons section
/// hashes from the first tick, has a new layout (the splash's full-strength radius, the friendly-fire
/// flag and the armor table; the table of this scenario's frozen definitions has none): with the old
/// digest layout the run has the WP5.2 value at all 30 checkpoints, 20 ticks apart.
#[test]
fn weapons_reload_matches_the_pinned_checksum() {
    check("weapons_reload", WEAPONS_RELOAD_EXPECTED);
}

/// Projectiles (WP5.2): one of each kind in flight (straight, arcing, guided, falling), a miss on a
/// moving target and a hit on a stopped one, a shooter shot down with its shell in the air, and two
/// rocket throwers who shoot each other down. A new scenario: pinned when it was added; no other
/// value moved but `weapons_reload` (`tests/projectiles.rs` asserts the shots and impacts).
/// Re-pinned in WP5.3 (`0x770192e1d2d6da1c` to `0xd9912751868da230`) for two causes: the new layout
/// of the digest of the weapon table, and a real change in what the run does. The frozen definitions
/// (untouched) give the howitzer `splash_radius: 1.5`, which is real since WP5.3: the shell that
/// lands where the walking scout stood (tick 31) misses the direct hit, but its splash kills the scout,
/// so the howitzer fires once. With the old digest layout the new code ends on `0x99d52b29b228098c`.
#[test]
fn projectiles_matches_the_pinned_checksum() {
    check("projectiles", PROJECTILES_EXPECTED);
}

/// Armor and splash (WP5.3): bullets, rockets and a sniper against heavy, light and no armor, a howitzer
/// shell into a crowd, two bombs (one that spares the shooter's side) and a bomb team that bombs
/// what is next to it. A new scenario: pinned when it was added (`tests/armour_splash.rs` asserts the
/// numbers against hand calculations).
#[test]
fn armour_splash_matches_the_pinned_checksum() {
    check("armour_splash", ARMOUR_SPLASH_EXPECTED);
}

/// Ten tanks against twenty infantry on automatic targeting (WP5.4): both sides attack-move onto the
/// other's ground, the units pick their own targets, stop to fight and go on. The headless half of the
/// phase-5 done-when. A new scenario: pinned when it was added; no value of an earlier scenario moved
/// (`tests/tanks_vs_infantry.rs` runs it twice, asserts the outcome and the rules along the run).
#[test]
fn tanks_vs_infantry_matches_the_pinned_checksum() {
    check("tanks_vs_infantry", TANKS_VS_INFANTRY_EXPECTED);
}

/// 5,000 wanderers for 10,000 ticks: the milestone M0 scenario. Too slow for
/// a debug build; `cargo test --release -p sf_headless` runs it, and CI runs
/// it through the binary on both operating systems.
#[test]
#[cfg_attr(debug_assertions, ignore = "full-size run: use --release")]
fn wander_5000_matches_the_pinned_checksum() {
    check("wander_5000", WANDER_5000_EXPECTED);
}

/// The shipped costs with one cost changed: foot can no longer enter ore, so
/// ore stops being open ground for all three ground classes.
fn shipped_costs_with_ore_closed_to_foot() -> MovementCosts {
    let classes = EMBEDDED_CLASSES.replace("\r\n", "\n");
    let changed = classes.replacen("(\"ore\", Speed(800))", "(\"ore\", Impassable)", 1);
    assert_ne!(changed, classes, "test setup: foot's cost on ore not found");
    let sources = DefSources {
        classes: DefFile {
            name: "classes.ron",
            text: &changed,
        },
        weapons: DefFile {
            name: "weapons.ron",
            text: EMBEDDED_WEAPONS,
        },
        units: DefFile {
            name: "units.ron",
            text: EMBEDDED_UNITS,
        },
    };
    let defs = UnitDefs::from_sources(&sources, None).unwrap();
    MovementCosts::from_defs(&defs, &TerrainTypes::embedded()).unwrap()
}

/// A change to the shipped costs moves the scenarios that run on the shipped
/// definitions and no scenario that runs on a frozen copy: the terrain's
/// `ground` summary, which is hashed, comes from the scenario's own costs.
#[test]
fn a_change_to_the_shipped_costs_leaves_the_frozen_scenarios_alone() {
    let altered = shipped_costs_with_ore_closed_to_foot();
    assert_ne!(altered, MovementCosts::embedded());
    let final_checksum = |name: &str, costs: &MovementCosts| {
        let path = scenario_path(&format!("{name}.ron"));
        let scenario = Scenario::load_with_shipped_costs(&path, costs).unwrap();
        run(&scenario, &RunOptions::for_scenario(&scenario))
            .unwrap()
            .final_checksum
    };
    let pinned = |text: &str| parse_checksum(text).unwrap();
    // Frozen definitions (units_spawn, terrain_costs and the pathing scenarios have a
    // terrain layer):
    // the pinned value, whatever the shipped costs are.
    assert_eq!(
        final_checksum("units_spawn", &altered),
        pinned(UNITS_SPAWN_EXPECTED)
    );
    assert_eq!(
        final_checksum("terrain_costs", &altered),
        pinned(TERRAIN_COSTS_EXPECTED)
    );
    assert_eq!(
        final_checksum("pathing", &altered),
        pinned(PATHING_EXPECTED)
    );
    assert_eq!(
        final_checksum("pathing_256", &altered),
        pinned(PATHING_256_EXPECTED)
    );
    assert_eq!(
        final_checksum("bridge_200", &altered),
        pinned(BRIDGE_200_EXPECTED)
    );
    assert_eq!(
        final_checksum("crowd_jam", &altered),
        pinned(CROWD_JAM_EXPECTED)
    );
    assert_eq!(
        final_checksum("turning", &altered),
        pinned(TURNING_EXPECTED)
    );
    assert_eq!(
        final_checksum("formation_move", &altered),
        pinned(FORMATION_MOVE_EXPECTED)
    );
    assert_eq!(final_checksum("orders", &altered), pinned(ORDERS_EXPECTED));
    // The scenario on the shipped definitions (no `unit_defs`) does move: the
    // test would prove nothing if the change reached nobody.
    assert_eq!(
        final_checksum("terrain_256", &MovementCosts::embedded()),
        pinned(TERRAIN_256_EXPECTED)
    );
    assert_ne!(
        final_checksum("terrain_256", &altered),
        pinned(TERRAIN_256_EXPECTED)
    );
}

#[test]
fn the_expected_files_hold_one_bare_checksum() {
    for text in [
        WANDER_5000_EXPECTED,
        COMMAND_MIX_EXPECTED,
        TERRAIN_256_EXPECTED,
        UNITS_SPAWN_EXPECTED,
        ORDERS_EXPECTED,
        TERRAIN_COSTS_EXPECTED,
        PATHING_EXPECTED,
        PATHING_256_EXPECTED,
        BRIDGE_200_EXPECTED,
        CROWD_JAM_EXPECTED,
        TURNING_EXPECTED,
        REPLAN_EXPECTED,
        FORMATION_MOVE_EXPECTED,
        WEAPONS_RELOAD_EXPECTED,
        PROJECTILES_EXPECTED,
    ] {
        let line = text.trim_end_matches(['\n', '\r']);
        assert!(!line.contains(['\n', ' ']), "{text:?}");
        assert_eq!(line.len(), 18, "{text:?}");
        assert!(line.starts_with("0x"), "{text:?}");
        parse_checksum(line).unwrap();
    }
}
