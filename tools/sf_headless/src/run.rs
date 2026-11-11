//! Running a scenario and collecting the checksums.
//!
//! [`run`] is a pure function of the scenario and the options: it builds the
//! world, calls `World::step` the requested number of times and records the
//! checksums. It reads no clock, no environment and no file. Wall time is
//! measured around it by the caller ([`crate::cli`]) and goes into the `host`
//! block of the log, which no checksum ever depends on.

use sf_ai::Ai;
use sf_sim::World;

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
    /// Commands scheduled before tick 0 (spawns included), plus the commands of a record
    /// (`recorded`), which are handed to the world when they arrived.
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
    /// `(player, Ai::digest)` of every AI of the scenario at the end, ascending by player; empty without
    /// `ai`. The AIs live outside the world and are not in its checksum, so this is how a diverging AI
    /// shows up; the log carries it and CI compares it between Linux and Windows.
    pub ai_digests: Vec<(u8, u64)>,
}

/// A finished run with the world and the AIs it ended with, for tests that look inside.
pub struct Finished {
    /// What [`run`] returns.
    pub outcome: Outcome,
    /// The world after the last tick. Boxed: a `World` is large and Windows gives the main thread 1 MB of stack, which
    /// a few debug-build copies of it would fill (the CI run of the first version overflowed there).
    pub world: Box<World>,
    /// The AIs after the last tick, ascending by player.
    pub ais: Vec<Ai>,
}

/// Runs `scenario` for `options.ticks` ticks.
pub fn run(scenario: &Scenario, options: &RunOptions) -> Result<Outcome, ScenarioError> {
    run_full(scenario, options).map(|f| f.outcome)
}

/// Runs `scenario` for `options.ticks` ticks, like [`run`], and also hands back the world and the AIs.
///
/// The AIs (`Scenario::ais`, ascending by player) are driven here, immediately before each step: an AI
/// that is [`due`](Ai::due) at the tick about to run thinks on the world as it is and the commands it
/// pushes are scheduled for that tick under its player id, so they are in the command queue and the
/// checksum like any other. The AI reads the world only through the fog-filtered observation of `sf_ai`.
pub fn run_full(scenario: &Scenario, options: &RunOptions) -> Result<Finished, ScenarioError> {
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

    let mut world = Box::new(scenario.build_world()?);
    let commands_scheduled = world.pending_commands().len() + scenario.recorded.len();
    let initial_checksum = world.checksum();

    let mut samples = Vec::new();
    let (mut executed, mut ignored) = (0u64, 0u64);
    let mut last = initial_checksum;
    // A recorded game starts after its first tick (`Scenario::game`): the loop counts world ticks.
    let first = world.tick().0 + 1;
    let mut arrivals = scenario.arrivals();
    let mut ais = scenario.ais.clone();
    let mut wanted = Vec::new();
    for done in first..=options.ticks {
        arrivals.schedule_due(&mut world)?;
        for ai in &mut ais {
            if ai.due(world.tick()) {
                wanted.clear();
                ai.think(&world, &mut wanted);
                for command in wanted.drain(..) {
                    world
                        .schedule(world.tick(), ai.player(), command)
                        .map_err(|e| ScenarioError::Sim(e.to_string()))?;
                }
            }
        }
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

    let outcome = Outcome {
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
        ai_digests: ais.iter().map(|a| (a.player().0, a.digest())).collect(),
    };
    Ok(Finished {
        outcome,
        world,
        ais,
    })
}

#[cfg(test)]
mod tests;
