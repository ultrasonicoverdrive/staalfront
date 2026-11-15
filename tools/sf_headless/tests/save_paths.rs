//! The movement scenarios of CI run through save and restore (WP13.1b, `docs/rules/save.md`).
//!
//! Every [`EVERY`] ticks the world is saved, restored onto a fresh world built from the same
//! scenario ([`Scenario::build_fresh_world`]) and the run goes on with the restored one. The final
//! checksum must be the one committed in `<name>.expected`, the resave of every restored world is
//! byte for byte the save, and at the save ticks the state the section claims to carry across is
//! really there (a route being planned, a flowfield, a formation, a unit in the middle of a turn).
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

/// Saves made besides the regular ones, at ticks where the state a test wants carried across is
/// there.
const EXTRA_SAVES: u32 = 6;

fn nothing(_: &SaveContents) -> bool {
    false
}

/// What the save ticks of one run held.
#[derive(Default, Debug)]
struct Seen {
    saves: u32,
    path_job: u32,
    routes: u32,
    fields: u32,
    field_building: u32,
    formations: u32,
    turning: u32,
}

impl Seen {
    fn add(&mut self, c: SaveContents) {
        self.saves += 1;
        self.path_job += u32::from(c.path_job);
        self.routes += u32::from(c.routes > 0);
        self.fields += u32::from(c.fields > 0);
        self.field_building += u32::from(c.field_building);
        self.formations += u32::from(c.formations > 0);
        self.turning += u32::from(c.turning > 0);
    }
}

/// Runs `name` with a save and restore every `every` ticks, and also after the first
/// [`EXTRA_SAVES`] ticks at which `also` holds (a state that is rare at the regular save ticks);
/// the final checksum and what the save ticks held. `Err` with the section when a save is
/// refused.
fn run_through_saves(
    name: &str,
    every: u64,
    also: fn(&SaveContents) -> bool,
) -> Result<(u64, Seen), SaveError> {
    let scenario = Scenario::load(&scenario_path(&format!("{name}.ron"))).unwrap();
    assert!(scenario.ais.is_empty(), "{name}: no AI here (WP13.1d)");
    let options = RunOptions::for_scenario(&scenario);
    let mut world = Box::new(scenario.build_world().unwrap());
    let mut arrivals = scenario.arrivals();
    let mut seen = Seen::default();
    let mut last = world.checksum();
    let mut extra = 0;
    for done in 1..=options.ticks {
        arrivals.schedule_due(&mut world).unwrap();
        last = world.step().checksum;
        let wanted = extra < EXTRA_SAVES && also(&world.save_contents());
        extra += u32::from(wanted);
        if (done % every == 0 || wanted) && done < options.ticks {
            let bytes = world.save()?;
            seen.add(world.save_contents());
            let fresh = scenario.build_fresh_world().unwrap();
            let back: World = World::restore(fresh, &bytes)
                .unwrap_or_else(|e| panic!("{name}: restore after tick {done}: {e}"));
            assert_eq!(
                back.save().unwrap(),
                bytes,
                "{name}: the resave after tick {done} is not the save"
            );
            assert_eq!(back.checksum(), world.checksum(), "{name}: tick {done}");
            *world = back;
        }
    }
    Ok((last, seen))
}

fn expected(name: &str) -> u64 {
    let text = std::fs::read_to_string(scenario_path(&format!("{name}.expected"))).unwrap();
    parse_checksum(&text).unwrap()
}

fn check(name: &str, also: fn(&SaveContents) -> bool) -> Seen {
    let (last, seen) =
        run_through_saves(name, EVERY, also).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(
        last,
        expected(name),
        "{name}: final checksum {} through saves, committed {}",
        log::hex(last),
        log::hex(expected(name))
    );
    assert!(seen.saves > 0, "{name}: saved at least once");
    eprintln!("{name}: {seen:?}");
    seen
}

#[test]
fn bridge_200_continues_through_saves() {
    let seen = check("bridge_200", |c| c.path_job || c.field_building);
    // Three flowfields and the hover craft on their own routes: fields at the save ticks, and
    // routes being planned while the budget is spent.
    assert!(
        seen.fields > 0 && seen.routes > 0 && seen.turning > 0,
        "{seen:?}"
    );
    assert!(
        seen.path_job > 0,
        "a route was being planned at a save: {seen:?}"
    );
    assert!(
        seen.field_building > 0,
        "a flowfield was being built at a save: {seen:?}"
    );
}

#[test]
fn replan_continues_through_saves() {
    let seen = check("replan", |c| c.path_job || c.fields > 0);
    assert!(
        seen.routes > 0 && seen.fields > 0 && seen.formations > 0,
        "{seen:?}"
    );
}

#[test]
fn formation_move_continues_through_saves() {
    let seen = check("formation_move", |c| c.formations > 0 && c.turning > 0);
    assert!(
        seen.formations > 0,
        "a formation was alive at a save: {seen:?}"
    );
    assert!(seen.field_building > 0 && seen.turning > 0, "{seen:?}");
}

#[test]
fn crowd_jam_continues_through_saves() {
    let seen = check("crowd_jam", |c| c.path_job);
    assert!(
        seen.routes > 0 && seen.fields > 0 && seen.formations > 0,
        "{seen:?}"
    );
}

#[test]
fn turning_continues_through_saves() {
    let seen = check("turning", |c| c.turning > 0);
    assert!(
        seen.turning > 0,
        "a unit was in the middle of a turn at a save: {seen:?}"
    );
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
        check(name, nothing);
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
