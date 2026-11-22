//! The economy, building, production and power scenarios of CI run through save and restore
//! (WP13.1c-2, `docs/rules/save.md`).
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

/// The scenarios that save since WP13.1c-2, with the states each carries across its saves.
const SCENARIOS: [(&str, &[Claim]); 11] = [
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
        &[CONSTRUCTING, STORED, UNLOADING, DOCK_QUEUE, DELIVERED],
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

/// `silo` sells a structure: from then until the sale is over the save is refused as `repair`
/// (repair and sale are saved in WP13.1c-3). The save at a tick inside that window is refused with
/// exactly that and nothing else; the saves before and after it go on to the pin.
#[test]
fn silo_continues_through_saves() {
    let mut gated = Gated {
        allowed: Some(SaveError::Unsupported("repair")),
        extra: vec![SILO_SALE.0 - 1, SILO_SALE.0, SILO_SALE.1, SILO_SALE.1 + 1],
        refused: Vec::new(),
    };
    let last = run_through_saves_gated(
        "silo",
        EVERY,
        u64::MAX,
        claims_of("silo"),
        &mut BTreeSet::new(),
        &mut gated,
    )
    .unwrap_or_else(|e| panic!("silo: {e}"));
    assert_eq!(last, expected("silo"));
    assert_eq!(gated.refused, [SILO_SALE.0, SILO_SALE.1], "silo");
}

/// The first and the last tick after which the save of `silo` is refused as `repair` (the sale),
/// measured by `every_tick_silo`.
const SILO_SALE: (u64, u64) = (4401, 4460);

/// A save after every tick of `name` to the end and its pin: every save restores and every resave
/// is the save. One test per scenario so the harness runs them side by side; slow (tens of ms a
/// save), so `#[ignore]`d: `cargo test --release -p sf_headless --test save_economy -- --ignored
/// every_tick` (`docs/rules/save.md` gives the time). In `silo` the ticks of the sale are refused
/// as `repair` (WP13.1c-3) and only those: the saves go on after it.
fn every_tick(name: &str) {
    let mut gated = Gated::default();
    if name == "silo" {
        gated.allowed = Some(SaveError::Unsupported("repair"));
    }
    let start = std::time::Instant::now();
    let last = run_through_saves_gated(
        name,
        1,
        u64::MAX,
        claims_of(name),
        &mut BTreeSet::new(),
        &mut gated,
    )
    .unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(last, expected(name), "{name}");
    if name == "silo" {
        let want: Vec<u64> = (SILO_SALE.0..=SILO_SALE.1).collect();
        assert_eq!(gated.refused, want, "silo: the ticks refused as repair");
    }
    eprintln!(
        "{name}: every tick in {:?}, refused {:?}..{:?}",
        start.elapsed(),
        gated.refused.first(),
        gated.refused.last()
    );
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
}

/// The short every-tick case for every build: the first 200 ticks of `harvest` (harvesters mining,
/// driving back, queued and unloading) and of `production_repeat`, then on to the pin.
#[test]
fn the_first_ticks_of_harvest_and_production_restore_after_every_tick() {
    for name in ["harvest", "production_repeat"] {
        let last =
            run_through_saves_until(name, 1, 200, &[]).unwrap_or_else(|e| panic!("{name}: {e}"));
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

/// The CI scenarios without an AI that still refuse to save hold state of a section WP13.1c-3
/// saves by the first save tick: their save is refused, naming the section.
#[test]
fn probe_which_scenarios_still_refuse() {
    let mut wrong = Vec::new();
    for (name, section) in [
        ("match", "vision"),
        ("repair_sell", "outcome"),
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
