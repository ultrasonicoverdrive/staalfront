//! Running a scenario and collecting the checksums.
//!
//! [`run`] is a pure function of the scenario and the options: it builds the
//! world, calls `World::step` the requested number of times and records the
//! checksums. It reads no clock, no environment and no file. Wall time is
//! measured around it by the caller ([`crate::cli`]) and goes into the `host`
//! block of the log, which no checksum ever depends on.

use crate::scenario::{Scenario, ScenarioError};

/// How often a checksum is recorded when `--every` is not given.
pub const DEFAULT_EVERY: u64 = 100;

/// What to run.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RunOptions {
    /// How many ticks to run, at least 1. May differ from the scenario's own
    /// `ticks` (`--ticks`).
    pub ticks: u64,
    /// Record a checksum after every `every`-th tick, at least 1. The last
    /// tick is always recorded.
    pub every: u64,
}

impl RunOptions {
    /// The scenario's own length and [`DEFAULT_EVERY`].
    pub fn for_scenario(scenario: &Scenario) -> RunOptions {
        RunOptions {
            ticks: scenario.ticks,
            every: DEFAULT_EVERY,
        }
    }
}

/// The checksum of the world after `ticks` ticks have run.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    /// Ticks executed so far (`World::tick`), so the first sample of
    /// `--every 100` is at 100.
    pub ticks: u64,
    /// `World::checksum` at that point.
    pub checksum: u64,
}

/// Everything deterministic a run produces. Two runs of the same scenario with
/// the same options give equal outcomes on every machine; that is what CI
/// compares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// [`Scenario::display_name`].
    pub scenario: String,
    /// The world seed.
    pub seed: u64,
    /// Map width in cells.
    pub map_width: u32,
    /// Map height in cells.
    pub map_height: u32,
    /// Ticks run.
    pub ticks: u64,
    /// Checksum interval.
    pub every: u64,
    /// Commands scheduled before tick 0 (spawns included).
    pub commands_scheduled: usize,
    /// Commands that ran and changed something.
    pub commands_executed: u64,
    /// Commands that ran and changed nothing (stale ids, entity limit).
    pub commands_ignored: u64,
    /// Live entities at the end.
    pub entities: usize,
    /// The checksum of the world before the first tick (spawns still
    /// pending): catches a difference in loading, apart from stepping.
    pub initial_checksum: u64,
    /// The checksum after the last tick; equals the last sample.
    pub final_checksum: u64,
    /// Checksums at multiples of `every` and at the last tick, ascending.
    pub samples: Vec<Sample>,
}

/// Runs `scenario` for `options.ticks` ticks.
pub fn run(scenario: &Scenario, options: &RunOptions) -> Result<Outcome, ScenarioError> {
    let mut problems = Vec::new();
    if options.ticks == 0 {
        problems.push("ticks must be at least 1".to_string());
    }
    if options.every == 0 {
        problems.push("every must be at least 1".to_string());
    }
    if !problems.is_empty() {
        return Err(ScenarioError::Invalid {
            path: None,
            problems,
        });
    }

    let mut world = scenario.build_world()?;
    let commands_scheduled = world.pending_commands().len();
    let initial_checksum = world.checksum();

    let mut samples = Vec::new();
    let (mut executed, mut ignored) = (0u64, 0u64);
    let mut last = initial_checksum;
    for done in 1..=options.ticks {
        let report = world.step();
        executed += u64::from(report.commands_executed);
        ignored += u64::from(report.commands_ignored);
        last = report.checksum;
        if done % options.every == 0 || done == options.ticks {
            samples.push(Sample {
                ticks: done,
                checksum: last,
            });
        }
    }

    Ok(Outcome {
        scenario: scenario.display_name().to_string(),
        seed: scenario.seed,
        map_width: scenario.map.width,
        map_height: scenario.map.height,
        ticks: options.ticks,
        every: options.every,
        commands_scheduled,
        commands_executed: executed,
        commands_ignored: ignored,
        entities: world.len(),
        initial_checksum,
        final_checksum: last,
        samples,
    })
}

#[cfg(test)]
mod tests;
