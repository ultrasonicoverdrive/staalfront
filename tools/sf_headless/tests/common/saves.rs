//! Running a CI scenario through save and restore (WP13.1b, WP13.1c-1, `docs/rules/save.md`):
//! the runner `save_paths.rs` and `save_combat.rs` share.

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
                ..restored
            },
            now,
            "{name}: what the restored world holds, tick {done}"
        );
        for k in due {
            let (what, holds) = claims[k];
            let in_restored = holds(&restored) || what == REPORTS.0;
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
