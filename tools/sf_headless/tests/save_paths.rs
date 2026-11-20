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
//! Only scenarios whose sections all save are here: the others hold weapon, targeting,
//! destruction, building or economy state (WP13.1c), `probe_which_scenarios_save` lists them.

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::log;
use sf_headless::run::RunOptions;
use sf_headless::scenario::Scenario;
use sf_sim::save::{SaveContents, SaveError};
use sf_sim::world::World;

/// The save interval: a prime, so the save ticks fall on every phase of the periodic rules
/// (formation windows of 20, the flowfield expiry of 400).
const EVERY: u64 = 97;

/// Saves made per claim at most, at ticks where it holds, at least [`CLAIM_GAP`] ticks apart; a
/// run must make at least one for every claim.
const CLAIM_SAVES: u32 = 4;
const CLAIM_GAP: u64 = 3;

/// A state a test claims a save carries across: its name and when it is there.
type Claim = (&'static str, fn(&SaveContents) -> bool);

const JOB: Claim = ("a route being planned", |c| c.path_job);
const BUILDING: Claim = ("a flowfield being built", |c| c.field_building);
const FIELDS: Claim = ("a flowfield", |c| c.fields > 0);
const ROUTES: Claim = ("a route", |c| c.routes > 0);
const OVERLAY: Claim = ("a stuck unit's request with an overlay", |c| c.overlays > 0);
const FORMATION: Claim = ("a formation", |c| c.formations > 0);
const TURNING: Claim = ("a turn carry", |c| c.turning > 0);

/// Runs `name` with a save and restore every `every` ticks, and for each claim at up to
/// [`CLAIM_SAVES`] ticks where it holds. At each claim save the state is asserted in the world
/// and in the restored one. The final checksum, or `Err` with the section of a refused save.
fn run_through_saves(name: &str, every: u64, claims: &[Claim]) -> Result<u64, SaveError> {
    run_through_saves_until(name, every, u64::MAX, claims)
}

/// [`run_through_saves`] with the saves only up to tick `until` (the run itself goes on to the end).
fn run_through_saves_until(
    name: &str,
    every: u64,
    until: u64,
    claims: &[Claim],
) -> Result<u64, SaveError> {
    let scenario = Scenario::load(&scenario_path(&format!("{name}.ron"))).unwrap();
    assert!(scenario.ais.is_empty(), "{name}: no AI here (WP13.1d)");
    let options = RunOptions::for_scenario(&scenario);
    let mut world = Box::new(scenario.build_world().unwrap());
    let mut arrivals = scenario.arrivals();
    let mut last = world.checksum();
    let mut made = vec![0u32; claims.len()];
    let mut last_at = vec![0u64; claims.len()];
    for done in 1..=options.ticks {
        arrivals.schedule_due(&mut world).unwrap();
        last = world.step().checksum;
        if done == options.ticks {
            break;
        }
        let now = world.save_contents();
        let mut due: Vec<usize> = Vec::new();
        for (k, &(_, holds)) in claims.iter().enumerate() {
            if made[k] < CLAIM_SAVES && done >= last_at[k] + CLAIM_GAP && holds(&now) {
                due.push(k);
            }
        }
        if done > until || (done % every != 0 && due.is_empty()) {
            continue;
        }
        let bytes = world.save()?;
        let fresh = scenario.build_fresh_world().unwrap();
        let back: World = World::restore(fresh, &bytes)
            .unwrap_or_else(|e| panic!("{name}: restore after tick {done}: {e}"));
        assert_eq!(
            back.save().unwrap(),
            bytes,
            "{name}: the resave after tick {done} is not the save"
        );
        assert_eq!(back.checksum(), world.checksum(), "{name}: tick {done}");
        let restored = back.save_contents();
        assert_eq!(
            restored, now,
            "{name}: what the restored world holds, tick {done}"
        );
        for k in due {
            let (what, holds) = claims[k];
            assert!(
                holds(&now) && holds(&restored),
                "{name}: {what} at tick {done}"
            );
            made[k] += 1;
            last_at[k] = done;
        }
        *world = back;
    }
    for (k, &(what, _)) in claims.iter().enumerate() {
        assert!(made[k] >= 1, "{name}: no save with {what}");
        eprintln!("{name}: {} saves with {what}", made[k]);
    }
    Ok(last)
}

fn expected(name: &str) -> u64 {
    let text = std::fs::read_to_string(scenario_path(&format!("{name}.expected"))).unwrap();
    parse_checksum(&text).unwrap()
}

fn check(name: &str, every: u64, claims: &[Claim]) {
    let last = run_through_saves(name, every, claims).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(
        last,
        expected(name),
        "{name}: final checksum {} through saves, committed {}",
        log::hex(last),
        log::hex(expected(name))
    );
}

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

/// The other CI scenarios without an AI hold state of a section WP13.1c saves by the first save
/// tick: their save is refused, naming the section (this list shrinks as those sections save).
#[test]
fn probe_which_scenarios_save() {
    for (name, section) in [
        ("weapons_reload", "projectiles"),
        ("projectiles", "projectiles"),
        ("armour_splash", "projectiles"),
        ("tanks_vs_infantry", "targeting"),
        ("destruction", "projectiles"),
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
        ("defences", "projectiles"),
        ("repair_sell", "projectiles"),
        ("vision", "vision"),
        ("vision_targeting", "projectiles"),
        ("silo", "economy"),
    ] {
        let scenario = Scenario::load(&scenario_path(&format!("{name}.ron"))).unwrap();
        let mut world = Box::new(scenario.build_world().unwrap());
        let mut arrivals = scenario.arrivals();
        for _ in 0..EVERY {
            arrivals.schedule_due(&mut world).unwrap();
            world.step();
        }
        assert_eq!(
            world.save().err(),
            Some(SaveError::Unsupported(section)),
            "{name}"
        );
    }
}
