//! The economy, building, production, power and (WP13.1c-3a) repair, sale and match-outcome
//! scenarios of CI run through save and restore (WP13.1c-2, `docs/rules/save.md`).
//!
//! Every [`EVERY`] ticks the world is saved and restored onto a fresh world built from the same
//! scenario, and the run goes on with the restored one (the runner is `common/saves.rs`). The final
//! checksum is the committed `<name>.expected` pin and every resave is byte for byte the save. Each
//! scenario names the states it claims to carry across; the run saves besides at ticks where a
//! claim holds and asserts it there, in the world and the restored one.
//!
//! `every_tick_<name>` (one per scenario, `#[ignore]`, release) saves, restores and resaves after
//! every tick of each (a save the game writes must always restore). `the_coverage_record_is_pinned` keeps
//! which saved fields some scenario saves non-default; the others have a round trip of their own in
//! `crates/sf_sim/src/save/economy_tests.rs`.

mod common;

use std::collections::BTreeSet;

use common::saves::*;
use common::scenario_path;
use sf_headless::scenario::Scenario;
use sf_sim::save::SaveError;

const HARVEST: &[Claim] = &[
    ORE_CHANGED,
    HARVESTING,
    RETURNING,
    UNLOADING,
    CARRYING,
    DOCK_QUEUE,
    DOCK_INBOX,
    BAY_RESERVED,
    DELIVERED,
    ROUTES,
];

/// The scenarios that save since WP13.1c-2 (and `repair_sell` since WP13.1c-3a), with the states
/// each carries across its saves.
const SCENARIOS: [(&str, &[Claim]); 12] = [
    ("placement", &[CONSTRUCTING, ECONOMY_REPORTS]),
    ("construction", &[CONSTRUCTING, ECONOMY_REPORTS]),
    ("ore_256", &[ORE_CHANGED]),
    ("harvest", HARVEST),
    ("storage", HARVEST),
    ("money", HARVEST),
    (
        "production",
        &[
            FACTORY,
            PRODUCING,
            RALLY,
            SPENT,
            STORED,
            RETURNING,
            UNLOADING,
            ECONOMY_REPORTS,
        ],
    ),
    (
        "production_repeat",
        &[FACTORY, PRODUCING, RALLY, REPEATING, SPENT, DOCK_INBOX],
    ),
    (
        "power",
        &[
            POWER,
            POWER_LOW,
            UNPOWERED,
            CONSTRUCTING,
            PRODUCING,
            SPENT,
            STORED,
            WRECKS,
            PROJECTILES,
        ],
    ),
    (
        "defences",
        &[POWER, POWER_LOW, UNPOWERED, RELOADING, PROJECTILES, WRECKS],
    ),
    (
        "silo",
        &[
            CONSTRUCTING,
            STORED,
            UNLOADING,
            DOCK_QUEUE,
            DELIVERED,
            SELLING,
        ],
    ),
    (
        "repair_sell",
        &[
            REPAIRING,
            SELLING,
            MATCH_BEGUN,
            DEFEAT,
            DECIDED,
            SPENT,
            STORED,
            POWER,
            WRECKS,
            ECONOMY_REPORTS,
        ],
    ),
];

fn claims_of(name: &str) -> &'static [Claim] {
    SCENARIOS.iter().find(|s| s.0 == name).unwrap().1
}

#[test]
fn placement_continues_through_saves() {
    check("placement", EVERY, claims_of("placement"));
}

#[test]
fn construction_continues_through_saves() {
    check("construction", EVERY, claims_of("construction"));
}

#[test]
fn ore_256_continues_through_saves() {
    check("ore_256", EVERY, claims_of("ore_256"));
}

#[test]
fn harvest_continues_through_saves() {
    check("harvest", EVERY, claims_of("harvest"));
}

#[test]
fn storage_continues_through_saves() {
    check("storage", EVERY, claims_of("storage"));
}

#[test]
fn money_continues_through_saves() {
    let mut claims = claims_of("money").to_vec();
    claims.extend([STORED, SPENT, CONSTRUCTING]);
    check("money", EVERY, &claims);
}

#[test]
fn production_continues_through_saves() {
    check("production", EVERY, claims_of("production"));
}

#[test]
fn production_repeat_continues_through_saves() {
    check("production_repeat", EVERY, claims_of("production_repeat"));
}

#[test]
fn power_continues_through_saves() {
    check("power", EVERY, claims_of("power"));
}

#[test]
fn defences_continues_through_saves() {
    check("defences", EVERY, claims_of("defences"));
}

/// `silo` sells a structure (ticks 4401 to 4460): since WP13.1c-3a every save restores, the sale
/// too, and the run goes on to the pin.
#[test]
fn silo_continues_through_saves() {
    check("silo", EVERY, claims_of("silo"));
}

/// `repair_sell` repairs, waits for money, sells and ends in a won match (WP13.1c-3a).
#[test]
fn repair_sell_continues_through_saves() {
    check("repair_sell", EVERY, claims_of("repair_sell"));
}

/// A save after every tick of `name` to the end and its pin: every save restores and every resave
/// is the save. One test per scenario so the harness runs them side by side; slow (tens of ms a
/// save), so `#[ignore]`d: `cargo test --release -p sf_headless --test save_economy -- --ignored
/// every_tick` (`docs/rules/save.md` gives the time). No tick is refused (WP13.1c-3a: the sale of
/// `silo` saves too).
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
    every_tick_placement => "placement",
    every_tick_construction => "construction",
    every_tick_ore_256 => "ore_256",
    every_tick_harvest => "harvest",
    every_tick_storage => "storage",
    every_tick_money => "money",
    every_tick_production => "production",
    every_tick_production_repeat => "production_repeat",
    every_tick_power => "power",
    every_tick_defences => "defences",
    every_tick_silo => "silo",
    every_tick_repair_sell => "repair_sell",
}

/// The short every-tick case of WP13.1c-3a for every build: `repair_sell` from tick 380 to 500
/// saved after every tick (the repairs start at 400 and 410, the sales at 400 and 480: a sale at
/// `sell_ticks` right after its order), then on to the pin.
#[test]
fn the_repairs_and_sales_of_repair_sell_restore_after_every_tick() {
    let last = run_through_saves_window("repair_sell", 380, 500, &[REPAIRING, SELLING, DEFEAT])
        .unwrap_or_else(|e| panic!("repair_sell: {e}"));
    assert_eq!(last, expected("repair_sell"));
}

/// The short every-tick case for every build: the first 200 ticks of `harvest` and of
/// `production_repeat`, then on to the pin, with the claims each reaches by tick 200 (ore taken,
/// harvesters driving to ore or mining on a route; a factory with a rally point repeating a unit
/// half built, money spent). The trips back, the docks and the inbox come later: the every-97 runs
/// and `every_tick_<name>` assert those.
#[test]
fn the_first_ticks_of_harvest_and_production_restore_after_every_tick() {
    let early: [(&str, &[Claim]); 2] = [
        ("harvest", &[ORE_CHANGED, HARVESTING, ROUTES]),
        (
            "production_repeat",
            &[FACTORY, PRODUCING, RALLY, REPEATING, SPENT],
        ),
    ];
    for (name, claims) in early {
        let last =
            run_through_saves_until(name, 1, 200, claims).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(last, expected(name), "{name}");
    }
}

/// Lesson 3 of WP13.1c-2: the saved fields some CI scenario holds non-default at a save tick (every
/// 97 ticks). The fields not in this list never are; each has a targeted round trip in
/// `economy_tests.rs` (`fields_no_scenario_reaches_round_trip`). A change in either direction fails.
#[test]
fn the_coverage_record_is_pinned() {
    let mut covered = BTreeSet::new();
    for (name, _) in SCENARIOS {
        run_through_saves_covered(name, EVERY, u64::MAX, &[], &mut covered)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    let want: BTreeSet<&str> = [
        "build_left",
        "capacity",
        "delivered",
        "dock",
        "dock.bay.free",
        "dock.bay.reserved",
        "dock.bay.unloading",
        "dock.inbox.arrived",
        "dock.queue",
        "factory",
        "factory.done",
        "factory.items",
        "factory.rally",
        "factory.repeat",
        "factory.status.building",
        "factory.status.idle",
        "factory.status.waiting_for_exit",
        "factory.status.waiting_for_money",
        "harvester.bound",
        "harvester.cell",
        "harvester.dock",
        "harvester.load",
        "harvester.own",
        "harvester.phase.driving",
        "harvester.phase.granted",
        "harvester.phase.mine",
        "harvester.phase.no_dock",
        "harvester.phase.queued",
        "harvester.phase.to_ore",
        "harvester.phase.unload",
        "harvester.spot",
        "harvester.unbound",
        "harvester.wait",
        "ore.amount",
        "ore.version",
        "power.made",
        "power.used",
        "spent",
        "stored",
    ]
    .into_iter()
    .collect();
    assert_eq!(covered, want);
}

/// The CI scenarios without an AI that still refuse to save hold state of a section WP13.1c-3b
/// saves by the first save tick: their save is refused, naming the section (only vision is left).
#[test]
fn probe_which_scenarios_still_refuse() {
    let mut wrong = Vec::new();
    for (name, section) in [
        ("match", "vision"),
        ("vision", "vision"),
        ("vision_targeting", "vision"),
    ] {
        let scenario = Scenario::load(&scenario_path(&format!("{name}.ron"))).unwrap();
        let mut world = Box::new(scenario.build_world().unwrap());
        let mut arrivals = scenario.arrivals();
        for _ in 0..EVERY {
            arrivals.schedule_due(&mut world).unwrap();
            world.step();
        }
        let got = world.save().err();
        if got != Some(SaveError::Unsupported(section)) {
            wrong.push(format!("{name}: {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:?}");
}
