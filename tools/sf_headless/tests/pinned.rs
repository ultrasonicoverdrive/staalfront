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
//! value only). Do the same for `command_mix`, `terrain_256`, `units_spawn`, `orders` and
//! `terrain_costs`. A change in `sf_sim` also moves
//! the golden values in `crates/sf_sim/tests/golden.rs`.

mod common;

use common::{deterministic_part, scenario_path, string_field};
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
/// when it was added; the other four did not change.
#[test]
fn orders_matches_the_pinned_checksum() {
    check("orders", ORDERS_EXPECTED);
}

/// Units of every movement class across road, grass, ore and a bridged river,
/// stopped by rock and water (WP4.1). A new scenario: its value was pinned when it
/// was added; the other five did not change.
#[test]
fn terrain_costs_matches_the_pinned_checksum() {
    check("terrain_costs", TERRAIN_COSTS_EXPECTED);
}

/// 5,000 wanderers for 10,000 ticks: the milestone M0 scenario. Too slow for
/// a debug build; `cargo test --release -p sf_headless` runs it, and CI runs
/// it through the binary on both operating systems.
#[test]
#[cfg_attr(debug_assertions, ignore = "full-size run: use --release")]
fn wander_5000_matches_the_pinned_checksum() {
    check("wander_5000", WANDER_5000_EXPECTED);
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
    ] {
        let line = text.trim_end_matches(['\n', '\r']);
        assert!(!line.contains(['\n', ' ']), "{text:?}");
        assert_eq!(line.len(), 18, "{text:?}");
        assert!(line.starts_with("0x"), "{text:?}");
        parse_checksum(line).unwrap();
    }
}
