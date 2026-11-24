//! WP13.1c-3b: the CI scenarios with vision rules through save and restore (`docs/rules/save.md`).
//! `match`, `vision` and `vision_targeting` save every 97 ticks (and at the claim ticks), each save
//! restores onto the scenario's fresh world, resaves to the same bytes and the run goes on to the
//! committed pin; the claimed vision state is asserted at its save ticks. A save after every tick
//! of each, to the pin, is `every_tick_<name>` (`#[ignore]`, release); the first 150 ticks of each
//! are saved after every tick in every build.

mod common;

use std::collections::BTreeSet;

use common::saves::*;

/// The scenarios with vision rules and no AI, with the states each carries across its saves.
const SCENARIOS: [(&str, &[Claim]); 3] = [
    (
        "match",
        &[
            VISION,
            TWO_PICTURES,
            FOG,
            STAMP_DEAD,
            STAMP_BLIND,
            MATCH_BEGUN,
            DEFEAT,
            DECIDED,
        ],
    ),
    ("vision", &[VISION, TWO_PICTURES, FOG, STAMP_DEAD]),
    (
        "vision_targeting",
        &[VISION, TWO_PICTURES, FOG, STAMP_DEAD, SHARED, ENGAGED],
    ),
];

fn claims_of(name: &str) -> &'static [Claim] {
    SCENARIOS.iter().find(|s| s.0 == name).unwrap().1
}

#[test]
fn match_continues_through_saves() {
    check("match", EVERY, claims_of("match"));
}

#[test]
fn vision_continues_through_saves() {
    check("vision", EVERY, claims_of("vision"));
}

#[test]
fn vision_targeting_continues_through_saves() {
    check("vision_targeting", EVERY, claims_of("vision_targeting"));
}

fn every_tick(name: &str) {
    let start = std::time::Instant::now();
    let last = run_through_saves_covered(name, 1, u64::MAX, claims_of(name), &mut BTreeSet::new())
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(last, expected(name), "{name}");
    eprintln!("{name}: every tick in {:?}", start.elapsed());
}

macro_rules! every_tick_tests {
    ($($test:ident => $name:literal),* $(,)?) => {$(
        #[test]
        #[ignore = "a save after every tick: run in release"]
        fn $test() {
            every_tick($name);
        }
    )*};
}

every_tick_tests! {
    every_tick_match => "match",
    every_tick_vision => "vision",
    every_tick_vision_targeting => "vision_targeting",
}

/// The short every-tick case for every build: the first 150 ticks of each scenario saved after
/// every tick, then on to the pin.
#[test]
fn the_first_ticks_with_vision_restore_after_every_tick() {
    for (name, _) in SCENARIOS {
        let last = run_through_saves_until(name, 1, 150, &[VISION])
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(last, expected(name), "{name}");
    }
}

/// The vision part of the coverage record: the names some vision scenario holds at a save tick
/// (every 97 ticks). The others are reached by the unit round trips in
/// `crates/sf_sim/src/vision/save_tests.rs`. A change in either direction fails.
#[test]
fn the_vision_coverage_record_is_pinned() {
    let mut covered = BTreeSet::new();
    for (name, _) in SCENARIOS {
        run_through_saves_covered(name, EVERY, u64::MAX, &[], &mut covered)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    let got: BTreeSet<&str> = covered
        .into_iter()
        .filter(|n| n.starts_with("vision"))
        .collect();
    eprintln!("vision coverage: {got:?}");
    let want: BTreeSet<&str> = [
        "vision.fog",
        "vision.player",
        "vision.shared",
        "vision.stamp",
        "vision.stamp.blind",
        "vision.stamp.building",
    ]
    .into_iter()
    .collect();
    assert_eq!(got, want);
}
