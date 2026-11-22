//! The combat scenarios of CI run through save and restore (WP13.1c-1, `docs/rules/save.md`):
//! weapons, projectiles, targeting and destruction.
//!
//! Every [`EVERY`] ticks the world is saved and restored onto a fresh world built from the same
//! scenario, and the run goes on with the restored one (the runner is `common/saves.rs`). The final
//! checksum is the committed `<name>.expected` pin and every resave is byte for byte the save. Each
//! test names the states it claims to carry across (a projectile in flight, a weapon in a reload
//! or a burst, a unit on a target it chose, wrecks, a tally, kills, a job behind the service's
//! epoch); the run saves besides at ticks where a claim holds and asserts it there.
//!
//! `every_combat_scenario_restores_after_every_tick` saves, restores and resaves after every tick
//! of each of them (a save the game writes must always restore).

mod common;

use common::saves::{
    BURSTING, Claim, ENGAGE_MEMORY, ENGAGED, EVERY, KILLS, PROJECTILES, RELOADING, REPORTS, ROUTES,
    TALLY, WRECKS, check, expected, run_through_saves,
};

/// The combat scenarios that save, with the states each carries across its saves.
const SCENARIOS: [(&str, &[Claim]); 5] = [
    (
        "weapons_reload",
        &[RELOADING, BURSTING, PROJECTILES, REPORTS],
    ),
    ("projectiles", &[PROJECTILES, RELOADING, REPORTS]),
    ("armour_splash", &[PROJECTILES, RELOADING, REPORTS]),
    (
        "tanks_vs_infantry",
        &[
            ENGAGED,
            ENGAGE_MEMORY,
            RELOADING,
            PROJECTILES,
            WRECKS,
            TALLY,
            KILLS,
            REPORTS,
        ],
    ),
    (
        "destruction",
        &[
            RELOADING,
            PROJECTILES,
            WRECKS,
            TALLY,
            KILLS,
            REPORTS,
            ROUTES,
        ],
    ),
];

fn claims_of(name: &str) -> &'static [Claim] {
    SCENARIOS.iter().find(|s| s.0 == name).unwrap().1
}

#[test]
fn weapons_reload_continues_through_saves() {
    check("weapons_reload", EVERY, claims_of("weapons_reload"));
}

#[test]
fn projectiles_continues_through_saves() {
    check("projectiles", EVERY, claims_of("projectiles"));
}

#[test]
fn armour_splash_continues_through_saves() {
    check("armour_splash", EVERY, claims_of("armour_splash"));
}

#[test]
fn tanks_vs_infantry_continues_through_saves() {
    check("tanks_vs_infantry", EVERY, claims_of("tanks_vs_infantry"));
}

#[test]
fn destruction_continues_through_saves() {
    check("destruction", EVERY, claims_of("destruction"));
}

/// A save after every tick of every combat scenario that saves, to the end and its pin: every save
/// restores, every resave is the save. Slow in a debug build; CI runs it in release
/// (`cargo test --release -- --ignored`, see `docs/rules/save.md` for the time).
#[test]
#[ignore = "every tick of five scenarios: run in release"]
fn every_combat_scenario_restores_after_every_tick() {
    for (name, claims) in SCENARIOS {
        let last = run_through_saves(name, 1, claims).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(last, expected(name), "{name}");
    }
}
