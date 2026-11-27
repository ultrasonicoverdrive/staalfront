//! Running a scenario and collecting the checksums.
//!
//! [`run`] is a pure function of the scenario and the options: it builds the
//! world, calls `World::step` the requested number of times and records the
//! checksums. It reads no clock, no environment and no file. Wall time is
//! measured around it by the caller ([`crate::cli`]) and goes into the `host`
//! block of the log, which no checksum ever depends on.

use sf_ai::Ai;
use sf_sim::{Command, Tick, World};

use crate::savefile::{Loaded, LoggedCommand};
use crate::scenario::{Arrivals, Scenario, ScenarioError};

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
    /// Every command an AI sent, `(tick it was scheduled for, player, command)`, in the order sent.
    pub ai_commands: Vec<(u64, u8, Command)>,
    /// Every command handed to the world after its start, in the order handed: the arrivals of a record and the
    /// AIs' commands (section `L` of a save file, WP13.1e-1). For a resumed run it begins with the loaded log.
    pub log: Vec<LoggedCommand>,
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
    run_watched(scenario, options, |_| {})
}

/// [`run_full`] with a look at the world after every step (`watch`), for tests that check a property at
/// every tick. The watcher only reads; the run is the same as without it.
pub fn run_watched(
    scenario: &Scenario,
    options: &RunOptions,
    mut watch: impl FnMut(&World),
) -> Result<Finished, ScenarioError> {
    run_observed(scenario, options, |world, _| watch(world))
}

/// [`run_watched`] that also shows the AIs (ascending by player) after every step, for tests that ask what
/// an AI knows or has decided at a given tick (WP9.1c-1). The watcher only reads.
pub fn run_observed(
    scenario: &Scenario,
    options: &RunOptions,
    watch: impl FnMut(&World, &[Ai]),
) -> Result<Finished, ScenarioError> {
    run_hooked(scenario, options, |_, _| {}, watch)
}

/// [`run_observed`] with a hook that may replace the AIs (ascending by player) right before they think at each
/// tick, the save point of `docs/rules/save.md` ("Save point"): WP13.1d's tests put a restored AI in place of the live one
/// there and the run goes on with it. Without a change by `between` the run is that of [`run_observed`].
pub fn run_hooked(
    scenario: &Scenario,
    options: &RunOptions,
    mut between: impl FnMut(&World, &mut [Ai]),
    watch: impl FnMut(&World, &[Ai]),
) -> Result<Finished, ScenarioError> {
    run_saving(scenario, options, |p| between(p.world, p.ais), watch)
}

/// The save point of a run (`docs/rules/save.md`, "Save point"; WP13.1e-1): the world after the arrivals of the
/// tick about to run are scheduled and before any AI thinks at it, the AIs (ascending by player) and the command
/// log so far. A hook may replace all three (a test puts the state loaded from a save file in their place) and
/// the run goes on with what it finds.
pub struct SavePoint<'a> {
    /// The world.
    pub world: &'a mut Box<World>,
    /// The AIs, ascending by player.
    pub ais: &'a mut Vec<Ai>,
    /// Every command handed to the world after its start (the arrivals of a record and the AIs' commands),
    /// in the order handed.
    pub log: &'a mut Vec<LoggedCommand>,
}

/// [`run_observed`] with a hook at the save point of every tick ([`SavePoint`]) that may replace the world, the
/// AIs and the log. Without a change by `hook` the run is that of [`run_observed`].
pub fn run_saving(
    scenario: &Scenario,
    options: &RunOptions,
    hook: impl FnMut(SavePoint<'_>),
    watch: impl FnMut(&World, &[Ai]),
) -> Result<Finished, ScenarioError> {
    check(options)?;
    let world = Box::new(scenario.build_world()?);
    let commands_scheduled = world.pending_commands().len() + scenario.recorded.len();
    let start = Start {
        world,
        ais: scenario.ais.clone(),
        log: Vec::new(),
        arrivals: scenario.arrivals(),
        commands_scheduled,
    };
    drive(scenario, options, start, hook, watch)
}

/// Goes on from a loaded save file: the world, the AIs and the log of `loaded` (WP13.1e-1), with the arrivals of
/// the scenario's record that the save already holds passed over ([`Arrivals::skip_to`]), up to `options.ticks`.
/// The run from the save tick on is the one the saved run would have gone on with. `initial_checksum` is the loaded
/// world's checksum and `samples` start after the save tick.
pub fn resume(
    scenario: &Scenario,
    options: &RunOptions,
    loaded: Loaded,
    hook: impl FnMut(SavePoint<'_>),
    watch: impl FnMut(&World, &[Ai]),
) -> Result<Finished, ScenarioError> {
    check(options)?;
    let mut arrivals = scenario.arrivals();
    arrivals.skip_to(loaded.world.tick());
    let world = Box::new(loaded.world);
    let commands_scheduled = world.pending_commands().len() + arrivals.remaining();
    let start = Start {
        world,
        ais: loaded.ais,
        log: loaded.log,
        arrivals,
        commands_scheduled,
    };
    drive(scenario, options, start, hook, watch)
}

/// Replays a command log (section `L` of a save file) on the start world of `scenario` (`Scenario::build_world`,
/// which schedules what the scenario itself schedules) up to `tick`: each command is scheduled when the world has
/// run its `arrived` ticks, as it was handed to the world the first time. It stops at `tick` with every command of
/// the log that arrived by `tick` scheduled (and none that arrived later). For the log of a save at `tick` that is
/// the save point (the arrivals of `tick`, none of that tick's AI commands, which the log does not hold yet), so the
/// checksum is the one the save stores; a longer log also puts the commands sent at `tick` after the save in. No AI
/// runs: its commands are in the log.
pub fn replay_log(
    scenario: &Scenario,
    log: &[LoggedCommand],
    tick: Tick,
) -> Result<Box<World>, ScenarioError> {
    let mut world = Box::new(scenario.build_world()?);
    let mut next = 0;
    loop {
        while let Some(c) = log.get(next)
            && c.arrived <= world.tick()
        {
            world
                .schedule(c.effective, c.player, c.command.clone())
                .map_err(|e| ScenarioError::Sim(e.to_string()))?;
            next += 1;
        }
        if world.tick() >= tick {
            break;
        }
        world.step();
    }
    Ok(world)
}

fn check(options: &RunOptions) -> Result<(), ScenarioError> {
    let mut problems = Vec::new();
    if options.ticks == 0 {
        problems.push("ticks must be at least 1".to_string());
    }
    if options.every == 0 {
        problems.push("every must be at least 1".to_string());
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(ScenarioError::Invalid {
            path: None,
            problems,
        })
    }
}

/// Where a run starts: a built world or a loaded one.
struct Start<'a> {
    world: Box<World>,
    ais: Vec<Ai>,
    log: Vec<LoggedCommand>,
    arrivals: Arrivals<'a>,
    commands_scheduled: usize,
}

fn drive(
    scenario: &Scenario,
    options: &RunOptions,
    start: Start<'_>,
    mut hook: impl FnMut(SavePoint<'_>),
    mut watch: impl FnMut(&World, &[Ai]),
) -> Result<Finished, ScenarioError> {
    let Start {
        mut world,
        mut ais,
        mut log,
        mut arrivals,
        commands_scheduled,
    } = start;
    let initial_checksum = world.checksum();

    let mut samples = Vec::new();
    let (mut executed, mut ignored) = (0u64, 0u64);
    let mut last = initial_checksum;
    // A recorded game starts after its first tick (`Scenario::game`), a loaded one at its save tick: the loop
    // counts world ticks.
    let first = world.tick().0 + 1;
    let mut wanted = Vec::new();
    let mut ai_commands = Vec::new();
    for done in first..=options.ticks {
        arrivals.schedule_due_logged(&mut world, &mut log)?;
        hook(SavePoint {
            world: &mut world,
            ais: &mut ais,
            log: &mut log,
        });
        for ai in &mut ais {
            if ai.due(world.tick()) {
                wanted.clear();
                ai.think(&world, &mut wanted);
                for command in wanted.drain(..) {
                    ai_commands.push((world.tick().0, ai.player().0, command.clone()));
                    log.push(LoggedCommand {
                        arrived: world.tick(),
                        effective: world.tick(),
                        player: ai.player(),
                        command: command.clone(),
                    });
                    world
                        .schedule(world.tick(), ai.player(), command)
                        .map_err(|e| ScenarioError::Sim(e.to_string()))?;
                }
            }
        }
        let report = world.step();
        watch(&world, &ais);
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
        ai_commands,
        log,
    })
}

#[cfg(test)]
mod tests;
