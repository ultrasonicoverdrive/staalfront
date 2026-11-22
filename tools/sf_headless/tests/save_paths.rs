//! The movement scenarios of CI run through save and restore (WP13.1b, `docs/rules/save.md`).
//!
//! Every [`EVERY`] ticks the world is saved, restored onto a fresh world built from the same
//! scenario ([`Scenario::build_fresh_world`]) and the run goes on with the restored one. The final
//! checksum must be the one committed in `<name>.expected` and the resave of every restored world
//! is byte for byte the save. A test also names the states it claims to carry across (a route
//! being planned, a flowfield being built, a formation, a unit in the middle of a turn): the run
//! saves besides at ticks where a claim holds, asserts at each such tick that the state is in the
//! world and in the restored world, and asserts that every claim had its saves.
//!
//! The runner is `common/saves.rs`. The combat scenarios are in `save_combat.rs` (WP13.1c-1); the
//! others hold building, economy, power, vision or match state (WP13.1c-2),
//! `probe_which_scenarios_save` lists them.

mod common;

use common::saves::{
    BUILDING, EVERY, FIELDS, FORMATION, JOB, OVERLAY, ROUTES, TURNING, check, expected,
    run_through_saves_until,
};
use common::scenario_path;
use sf_headless::scenario::Scenario;
use sf_sim::save::SaveError;

#[test]
fn bridge_200_continues_through_saves() {
    check(
        "bridge_200",
        EVERY,
        &[JOB, BUILDING, FIELDS, ROUTES, TURNING],
    );
}

#[test]
fn replan_continues_through_saves() {
    check("replan", EVERY, &[ROUTES, FIELDS, FORMATION, OVERLAY]);
}

#[test]
fn formation_move_continues_through_saves() {
    check(
        "formation_move",
        EVERY,
        &[FORMATION, BUILDING, TURNING, ROUTES],
    );
}

#[test]
fn crowd_jam_continues_through_saves() {
    check("crowd_jam", EVERY, &[ROUTES, FIELDS, FORMATION]);
}

#[test]
fn turning_continues_through_saves() {
    check("turning", EVERY, &[TURNING]);
}

/// A save after every one of the first 400 ticks of the two scenarios with formations (the run goes
/// on to the end and its pin): a member leaves its formation on arrival (`orders`) after the
/// formation pass pruned the list, so for a tick a formation lists a unit whose own memory is no
/// longer in it (`crowd_jam` tick 219). That world saves and must restore.
#[test]
fn formation_scenarios_restore_after_every_tick() {
    for name in ["formation_move", "crowd_jam"] {
        let last = run_through_saves_until(name, 1, 400, &[FORMATION])
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(last, expected(name), "{name}");
    }
}

#[test]
fn the_other_movement_scenarios_continue_through_saves() {
    for name in [
        "wander_5000",
        "command_mix",
        "terrain_256",
        "units_spawn",
        "orders",
        "terrain_costs",
        "pathing",
        "pathing_256",
    ] {
        check(name, EVERY, &[]);
    }
}

/// The other CI scenarios without an AI hold state of a section WP13.1c-2 saves by the first save
/// tick: their save is refused, naming the section (this list shrinks as those sections save). The
/// combat scenarios that save now are in `save_combat.rs`.
#[test]
fn probe_which_scenarios_save() {
    let mut wrong = Vec::new();
    for (name, section) in [
        ("placement", "buildings"),
        ("construction", "buildings"),
        ("ore_256", "economy"),
        ("harvest", "economy"),
        ("storage", "economy"),
        ("money", "economy"),
        ("production", "economy"),
        ("production_repeat", "economy"),
        ("match", "economy"),
        ("power", "economy"),
        ("defences", "power"),
        ("repair_sell", "economy"),
        ("vision", "vision"),
        ("vision_targeting", "vision"),
        ("silo", "economy"),
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
