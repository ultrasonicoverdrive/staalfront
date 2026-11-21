//! Running a CI scenario through save and restore (WP13.1b, WP13.1c-1, `docs/rules/save.md`):
//! the runner `save_paths.rs` and `save_combat.rs` share.

use std::collections::BTreeSet;

use sf_headless::cli::parse_checksum;
use sf_headless::log;
use sf_headless::run::RunOptions;
use sf_headless::scenario::Scenario;
use sf_sim::save::{SaveContents, SaveError};
use sf_sim::world::World;

use super::scenario_path;

/// The save interval: a prime, so the save ticks fall on every phase of the periodic rules
/// (formation windows of 20, the flowfield expiry of 400, reloads of 30, 40, 100).
pub const EVERY: u64 = 97;

/// Saves made per claim at most, at ticks where it holds, at least [`CLAIM_GAP`] ticks apart; a
/// run must make at least one for every claim.
const CLAIM_SAVES: u32 = 4;
const CLAIM_GAP: u64 = 3;

/// A state a test claims a save carries across: its name and when it is there.
pub type Claim = (&'static str, fn(&SaveContents) -> bool);

pub const JOB: Claim = ("a route being planned", |c| c.path_job);
pub const BUILDING: Claim = ("a flowfield being built", |c| c.field_building);
pub const FIELDS: Claim = ("a flowfield", |c| c.fields > 0);
pub const ROUTES: Claim = ("a route", |c| c.routes > 0);
pub const OVERLAY: Claim = ("a stuck unit's request with an overlay", |c| c.overlays > 0);
pub const FORMATION: Claim = ("a formation", |c| c.formations > 0);
pub const TURNING: Claim = ("a turn carry", |c| c.turning > 0);
// WP13.1c-1.
pub const PROJECTILES: Claim = ("a projectile in flight", |c| c.projectiles > 0);
pub const RELOADING: Claim = ("a weapon reloading", |c| c.reloading > 0);
pub const BURSTING: Claim = ("a weapon in the middle of a burst", |c| c.bursting > 0);
pub const ENGAGED: Claim = ("a unit on a target it chose", |c| c.engaged > 0);
pub const ENGAGE_MEMORY: Claim = ("engagement memory", |c| c.engage_memory > 0);
pub const WRECKS: Claim = ("a wreck", |c| c.wrecks > 0);
pub const TALLY: Claim = ("a tally", |c| c.tallies > 0);
pub const KILLS: Claim = ("a unit with kills", |c| c.killers > 0);
pub const REPORTS: Claim = ("reports of the weapon stage", |c| c.reports);
pub const JOB_BEHIND: Claim = ("a job behind the service's epoch", |c| c.job_behind);
// WP13.1c-2.
pub const ORE_CHANGED: Claim = ("ore taken or grown back", |c| c.ore_changed);
pub const HARVESTING: Claim = ("a harvester driving to ore or mining", |c| c.harvesting > 0);
pub const RETURNING: Claim = ("a harvester on its way to a refinery", |c| c.returning > 0);
pub const UNLOADING: Claim = ("a harvester docked and unloading", |c| c.unloading > 0);
pub const HARVEST_WAITING: Claim = ("a harvester parked or waiting for a refinery", |c| {
    c.harvest_waiting > 0
});
pub const CARRYING: Claim = ("a harvester with a load", |c| c.carrying > 0);
pub const DOCK_QUEUE: Claim = ("a harvester in a dock queue", |c| c.dock_queued > 0);
pub const DOCK_INBOX: Claim = ("a message in a dock inbox", |c| c.dock_inbox > 0);
pub const BAY_RESERVED: Claim = ("a dock bay granted, not yet reached", |c| {
    c.bay_reserved > 0
});
pub const DELIVERED: Claim = ("ore delivered", |c| c.delivered);
pub const STORED: Claim = ("something in store", |c| c.stored);
pub const SPENT: Claim = ("money spent", |c| c.spent);
pub const CONSTRUCTING: Claim = ("a structure half built", |c| c.constructing > 0);
pub const FACTORY: Claim = ("a factory record", |c| c.factories > 0);
pub const PRODUCING: Claim = ("a unit half built", |c| c.producing > 0);
pub const QUEUE_FULL: Claim = ("a full queue", |c| c.queue_full > 0);
pub const RALLY: Claim = ("a rally point", |c| c.rallies > 0);
pub const REPEATING: Claim = ("a repeating factory", |c| c.repeating > 0);
pub const FREE_ITEM: Claim = ("a free unit queued", |c| c.free_items > 0);
pub const POWER: Claim = ("power totals", |c| c.power_players > 0);
pub const POWER_LOW: Claim = ("a player short of power", |c| c.power_low > 0);
pub const UNPOWERED: Claim = ("defences without power", |c| c.unpowered > 0);
pub const ECONOMY_REPORTS: Claim = ("reports of construction, placement or production", |c| {
    c.economy_reports
});

/// The claims whose state is a report of the last tick: derived, empty in a restored world, so
/// asserted in the saved world only.
const DERIVED: [&str; 2] = [REPORTS.0, ECONOMY_REPORTS.0];

/// Runs `name` with a save and restore every `every` ticks up to tick `until` (the run itself goes
/// on to the end), and for each claim at up to [`CLAIM_SAVES`] ticks where it holds. Each restore is
/// onto a copy of the fresh world of the scenario ([`Scenario::build_fresh_world`]), the resave is
/// the save byte for byte, the checksum and [`SaveContents`] of the restored world are the
/// original's, and at each claim save the claimed state is asserted in both. The final checksum, or
/// `Err` with the section of a refused save.
pub fn run_through_saves_until(
    name: &str,
    every: u64,
    until: u64,
    claims: &[Claim],
) -> Result<u64, SaveError> {
    run_through_saves_covered(name, every, until, claims, &mut BTreeSet::new())
}

/// [`run_through_saves_until`], adding to `covered` the saved fields of WP13.1c-2 that are not at
/// their default at a save tick (`World::save_coverage`): the coverage record.
pub fn run_through_saves_covered(
    name: &str,
    every: u64,
    until: u64,
    claims: &[Claim],
    covered: &mut BTreeSet<&'static str>,
) -> Result<u64, SaveError> {
    let scenario = Scenario::load(&scenario_path(&format!("{name}.ron"))).unwrap();
    assert!(scenario.ais.is_empty(), "{name}: no AI here (WP13.1d)");
    let options = RunOptions::for_scenario(&scenario);
    let mut world = Box::new(scenario.build_world().unwrap());
    let fresh = Box::new(scenario.build_fresh_world().unwrap());
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
        covered.extend(world.save_coverage());
        let back: World = World::restore((*fresh).clone(), &bytes)
            .unwrap_or_else(|e| panic!("{name}: restore after tick {done}: {e}"));
        assert_eq!(
            back.save().unwrap(),
            bytes,
            "{name}: the resave after tick {done} is not the save"
        );
        assert_eq!(back.checksum(), world.checksum(), "{name}: tick {done}");
        let restored = back.save_contents();
        // The reports of the last tick are derived and start empty in a restored world.
        assert_eq!(
            SaveContents {
                reports: now.reports,
                economy_reports: now.economy_reports,
                ..restored
            },
            now,
            "{name}: what the restored world holds, tick {done}"
        );
        for k in due {
            let (what, holds) = claims[k];
            let in_restored = holds(&restored) || DERIVED.contains(&what);
            assert!(holds(&now) && in_restored, "{name}: {what} at tick {done}");
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

/// [`run_through_saves_until`] with the saves up to the end.
pub fn run_through_saves(name: &str, every: u64, claims: &[Claim]) -> Result<u64, SaveError> {
    run_through_saves_until(name, every, u64::MAX, claims)
}

/// The committed pin of `name`.
pub fn expected(name: &str) -> u64 {
    let text = std::fs::read_to_string(scenario_path(&format!("{name}.expected"))).unwrap();
    parse_checksum(&text).unwrap()
}

/// [`run_through_saves`], and the final checksum is the committed pin.
pub fn check(name: &str, every: u64, claims: &[Claim]) {
    let last = run_through_saves(name, every, claims).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(
        last,
        expected(name),
        "{name}: final checksum {} through saves, committed {}",
        log::hex(last),
        log::hex(expected(name))
    );
}
