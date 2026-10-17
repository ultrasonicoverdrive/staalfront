//! Commands and the command queue.
//!
//! Input never touches the world directly (PLAN.md quality rule 1). The UI,
//! the AI, a replay file or a test turns what it wants into a [`Command`] and
//! *schedules* it for a tick. The world executes the commands of tick N at the
//! start of tick N, before any behaviour runs.
//!
//! # Order of execution
//!
//! Within one tick, commands run in ascending order of
//! `(player, insertion sequence)`: all commands of player 0 first, in the
//! order they were scheduled, then player 1, and so on. The order does not
//! depend on which player's command *arrived* first, which is what lockstep
//! networking needs (peers see packets in different orders but agree on who
//! the players are and on each player's own sequence).
//!
//! # Past ticks
//!
//! Scheduling for a tick that has already been executed is an error
//! ([`ScheduleError::PastTick`]). The alternative, running it late, would make
//! the result depend on when it arrived. Scheduling for the tick that is about
//! to run (the world's current tick) is fine.
//!
//! # Stale targets
//!
//! A command names entities by [`EntityId`]. By the time it runs, some of them
//! may be gone (despawned by an earlier command, say). That is normal in a
//! lockstep game, so such ids are skipped silently; only a command that ends up
//! changing nothing at all is counted as ignored in the
//! [`StepReport`](crate::StepReport). Whether a player *may* command an entity
//! is a game rule and is not checked here.

use core::fmt;
use std::collections::BTreeMap;

use sf_math::Vec2;

use crate::behavior::Area;
use crate::ids::{EntityId, Kind, PlayerId, Tick};
use crate::terrain::Cell;

/// Something a player asks the simulation to do. Engine-generic: no game
/// rules, only what the placeholder world can do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Command {
    /// Create an entity of `kind` for `owner` at `pos` (clamped into the map).
    /// It starts out wandering over the whole map.
    Spawn {
        /// What to create.
        kind: Kind,
        /// Where, in cells.
        pos: Vec2,
        /// Who owns it.
        owner: PlayerId,
    },
    /// Place a structure of `kind` for `owner` on `cell` (WP2.1): an entity
    /// that stands still in the middle of the cell and occupies it until it is
    /// despawned. Ignored (it counts as a command that changed nothing) when
    /// the cell is outside the map, its terrain is not buildable, or another
    /// structure already occupies it. A structure does not wander and ignores
    /// `MoveTo` and `SetWanderArea`. See `docs/rules/terrain.md`.
    Place {
        /// What to create.
        kind: Kind,
        /// Where.
        cell: Cell,
        /// Who owns it.
        owner: PlayerId,
    },
    /// Create a mobile unit or an object of `kind` for `owner` at `pos`, with
    /// the stats of its entry in the world's unit table (WP3.2): hit points,
    /// speed per tick and turn rate. It occupies no cell and stands idle (it
    /// does not wander). Ignored (it counts as a command that changed nothing)
    /// when the table does not define `kind` or defines it as a structure,
    /// when `pos` is outside the map or on a cell a structure occupies, or at
    /// the entity limit. See `docs/rules/units.md`.
    SpawnUnit {
        /// What to create.
        kind: Kind,
        /// Where, in cells.
        pos: Vec2,
        /// Who owns it.
        owner: PlayerId,
    },
    /// Place a structure of `kind` for `owner` with its footprint anchored on
    /// `cell` (WP3.2): the cell with the smallest x and y, the footprint
    /// extending east and south. The structure takes its hit points from the
    /// unit table, stands in the middle of its footprint and occupies every
    /// cell of it. Ignored when the table does not define `kind` or defines it
    /// as something other than a structure, when any cell of the footprint is
    /// outside the map, not buildable or already occupied, or at the entity
    /// limit; nothing is placed in part. See `docs/rules/units.md`.
    PlaceStructure {
        /// What to create.
        kind: Kind,
        /// The anchor cell.
        cell: Cell,
        /// Who owns it.
        owner: PlayerId,
    },
    /// Remove an entity.
    Despawn {
        /// The entity.
        id: EntityId,
    },
    /// Make the entities wander inside `area` (clamped to the map). Each one
    /// draws a fresh target at once.
    SetWanderArea {
        /// The entities.
        ids: Vec<EntityId>,
        /// Where they may wander.
        area: Area,
    },
    /// Make the entities walk to `target` (clamped into the map) and stand
    /// there.
    MoveTo {
        /// The entities.
        ids: Vec<EntityId>,
        /// Where to, in cells.
        target: Vec2,
    },
}

/// Why a command could not be scheduled.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ScheduleError {
    /// The command was for a tick that has already been executed.
    PastTick {
        /// The tick the command asked for.
        scheduled: Tick,
        /// The next tick to run; `scheduled` is before it.
        current: Tick,
    },
}

impl fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScheduleError::PastTick { scheduled, current } => write!(
                f,
                "command scheduled for tick {scheduled}, but tick {current} is next to run"
            ),
        }
    }
}

impl std::error::Error for ScheduleError {}

/// The pending commands, ordered by `(tick, player, insertion sequence)`.
///
/// A `BTreeMap` keyed by that triple: its iteration order is sorted by key, so
/// the order is the same on every machine and nothing depends on hashing.
///
/// Two queues are equal when they hold the same commands in the same
/// execution order. The insertion sequence numbers themselves are bookkeeping
/// (the tie-breaker for equal ticks and players) and are not compared: queues
/// that were filled in a different arrival order but hold the same per-player
/// order behave identically and compare equal.
#[derive(Clone, Debug, Default)]
pub struct CommandQueue {
    pending: BTreeMap<(Tick, PlayerId, u64), Command>,
    next_seq: u64,
}

impl PartialEq for CommandQueue {
    fn eq(&self, other: &CommandQueue) -> bool {
        self.iter().eq(other.iter())
    }
}

impl Eq for CommandQueue {}

impl CommandQueue {
    /// An empty queue.
    pub fn new() -> CommandQueue {
        CommandQueue::default()
    }

    /// The number of pending commands.
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// True if nothing is pending.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Queues `command` from `player` for `tick`, given that `now` is the
    /// next tick to run. Fails if `tick` is before `now`.
    pub fn schedule(
        &mut self,
        now: Tick,
        tick: Tick,
        player: PlayerId,
        command: Command,
    ) -> Result<(), ScheduleError> {
        if tick < now {
            return Err(ScheduleError::PastTick {
                scheduled: tick,
                current: now,
            });
        }
        let seq = self.next_seq;
        self.next_seq = seq.wrapping_add(1);
        self.pending.insert((tick, player, seq), command);
        Ok(())
    }

    /// Removes and returns the next command that is due at `tick` (scheduled
    /// for `tick` or earlier), in execution order, or `None` when the rest is
    /// for later.
    pub fn pop_due(&mut self, tick: Tick) -> Option<(PlayerId, Command)> {
        let entry = self.pending.first_entry()?;
        if entry.key().0 > tick {
            return None;
        }
        let ((_, player, _), command) = entry.remove_entry();
        Some((player, command))
    }

    /// The pending commands in execution order, as
    /// `(tick, player, command)`.
    pub fn iter(&self) -> impl Iterator<Item = (Tick, PlayerId, &Command)> {
        self.pending
            .iter()
            .map(|(&(tick, player, _), command)| (tick, player, command))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn despawn(n: u32) -> Command {
        Command::Despawn {
            id: EntityId::new(n, 0),
        }
    }

    fn drain(q: &mut CommandQueue, tick: Tick) -> Vec<(u8, Command)> {
        let mut out = Vec::new();
        while let Some((p, c)) = q.pop_due(tick) {
            out.push((p.0, c));
        }
        out
    }

    #[test]
    fn executes_by_tick_then_player_then_insertion_order() {
        let mut q = CommandQueue::new();
        let now = Tick(5);
        // Scheduled in a scrambled order.
        q.schedule(now, Tick(7), PlayerId(0), despawn(70)).unwrap();
        q.schedule(now, Tick(6), PlayerId(2), despawn(62)).unwrap();
        q.schedule(now, Tick(6), PlayerId(1), despawn(61)).unwrap();
        q.schedule(now, Tick(6), PlayerId(1), despawn(611)).unwrap();
        q.schedule(now, Tick(6), PlayerId(0), despawn(60)).unwrap();
        q.schedule(now, Tick(6), PlayerId(1), despawn(6111))
            .unwrap();
        q.schedule(now, Tick(5), PlayerId(3), despawn(53)).unwrap();
        assert_eq!(q.len(), 7);

        assert_eq!(drain(&mut q, Tick(5)), vec![(3, despawn(53))]);
        assert_eq!(
            drain(&mut q, Tick(6)),
            vec![
                (0, despawn(60)),
                (1, despawn(61)),
                (1, despawn(611)),
                (1, despawn(6111)),
                (2, despawn(62)),
            ]
        );
        assert_eq!(q.len(), 1);
        assert_eq!(drain(&mut q, Tick(7)), vec![(0, despawn(70))]);
        assert!(q.is_empty());
    }

    #[test]
    fn nothing_is_due_before_its_tick() {
        let mut q = CommandQueue::new();
        q.schedule(Tick(0), Tick(3), PlayerId(0), despawn(1))
            .unwrap();
        assert_eq!(q.pop_due(Tick(0)), None);
        assert_eq!(q.pop_due(Tick(2)), None);
        assert_eq!(q.len(), 1);
        assert_eq!(q.pop_due(Tick(3)), Some((PlayerId(0), despawn(1))));
    }

    #[test]
    fn past_ticks_are_rejected_and_leave_the_queue_alone() {
        let mut q = CommandQueue::new();
        let err = q
            .schedule(Tick(10), Tick(9), PlayerId(0), despawn(1))
            .unwrap_err();
        assert_eq!(
            err,
            ScheduleError::PastTick {
                scheduled: Tick(9),
                current: Tick(10)
            }
        );
        assert!(q.is_empty());
        assert_eq!(
            err.to_string(),
            "command scheduled for tick 9, but tick 10 is next to run"
        );
        // The tick that is about to run is not in the past.
        q.schedule(Tick(10), Tick(10), PlayerId(0), despawn(1))
            .unwrap();
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn iter_lists_the_execution_order() {
        let mut q = CommandQueue::new();
        q.schedule(Tick(0), Tick(2), PlayerId(1), despawn(21))
            .unwrap();
        q.schedule(Tick(0), Tick(2), PlayerId(0), despawn(20))
            .unwrap();
        q.schedule(Tick(0), Tick(1), PlayerId(9), despawn(19))
            .unwrap();
        let order: Vec<(u64, u8)> = q.iter().map(|(t, p, _)| (t.0, p.0)).collect();
        assert_eq!(order, vec![(1, 9), (2, 0), (2, 1)]);
    }
}
