//! Commands and the command queue.
//!
//! Input never touches the world directly (PLAN.md quality rule 1). The UI,
//! the AI, a replay file or a test turns what it wants into a [`Command`] and
//! *schedules* it for a tick. The world executes the commands of tick N at the
//! start of tick N, before any behaviour runs.
//!
//! # Order of execution
//!
//! Commands run in ascending order of `(tick, player, player sequence)`: all
//! commands of player 0 for a tick first, in the order of their sequence
//! numbers, then player 1, and so on. The sequence number is the player's own
//! counter: [`World::schedule`](crate::World::schedule) numbers a command with
//! the next number of its player, and a command that arrives from another
//! machine carries the number its sender gave it
//! ([`World::schedule_sequenced`](crate::World::schedule_sequenced)). The order
//! therefore depends on neither the machine nor on which player's (or which
//! packet's) command *arrived* first, which is what lockstep networking needs:
//! peers see packets in different orders but agree on who the players are and on
//! each player's own sequence.
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
use crate::orders::Order;
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
    /// Give the units an order (WP3.4, `docs/rules/orders.md`): replace their
    /// current order and queue with `order`, or, with `queued`, add it behind
    /// the orders they have (a unit with none starts on it at once). A unit
    /// that cannot take it is skipped: a structure, an object, a stale id, an
    /// attack without a weapon or on a target that is gone or is the unit
    /// itself, a guard of itself, an append to a full queue. `Order::Idle` is no
    /// order to give: use [`Command::Stop`]. The command counts as ignored when
    /// no unit took it. A point is clamped into the map.
    Order {
        /// The units.
        ids: Vec<EntityId>,
        /// The order.
        order: Order,
        /// Append to the queue instead of replacing the orders.
        queued: bool,
    },
    /// Clear the current order and the queue of the units: they stand still
    /// where they are (WP3.4). Skips what cannot move.
    Stop {
        /// The units.
        ids: Vec<EntityId>,
    },
    /// Point the turrets of the units at an entity, or let them go back to the
    /// hull (WP4.5, `docs/rules/turning.md`): with `target: Some(id)` the turret of
    /// every unit that has one turns toward that entity and keeps following it
    /// wherever it goes, until it is gone, until another `AimTurret` says
    /// otherwise or until a [`Command::Stop`]; with `None` it returns to
    /// hull-forward. It does not touch the hull or the order: a tank can drive
    /// one way and aim another. A target that is gone or is the unit itself is
    /// `None`. Skips what has no turret; the command counts as ignored when no
    /// unit took it.
    AimTurret {
        /// The units.
        ids: Vec<EntityId>,
        /// What the turrets aim at.
        target: Option<EntityId>,
    },
    /// Take up to `amount` of ore from `cell` (WP7.1, `docs/rules/economy.md`). The
    /// harvester (WP7.2) takes ore by itself inside the tick; this command is the same take
    /// for scenarios and tests, so that depletion and regrowth can be pinned without a
    /// harvester. It counts as ignored in a world without ore rules and when the cell gave
    /// nothing (empty, not a field cell, off the map). It changes no money (WP7.4).
    TakeOre {
        /// The cell to take from.
        cell: Cell,
        /// The most to take; the cell gives what it has if that is less.
        amount: u16,
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
    /// The player already has a pending command for that tick under that
    /// sequence number (a packet delivered twice, or a sender that reused a
    /// number). The first one stays; the order of the two would be a guess.
    DuplicateSequence {
        /// The tick of the command.
        tick: Tick,
        /// Its player.
        player: PlayerId,
        /// The sequence number that is taken.
        sequence: u64,
    },
    /// The sequence number was `u64::MAX`. That number is reserved: a
    /// player's counter stands on it when the numbers are used up, so a
    /// command carrying it could never be told apart from "no number left".
    ReservedSequence,
    /// The player's counter has no number left to give (it stands on
    /// `u64::MAX`, because a sender brought a number just below it). Refused
    /// instead of reusing a number: a reused key would overwrite a command.
    SequenceExhausted {
        /// The player.
        player: PlayerId,
    },
}

impl fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScheduleError::PastTick { scheduled, current } => write!(
                f,
                "command scheduled for tick {scheduled}, but tick {current} is next to run"
            ),
            ScheduleError::DuplicateSequence {
                tick,
                player,
                sequence,
            } => write!(
                f,
                "player {} already has a command with sequence number {sequence} for tick {tick}",
                player.0
            ),
            ScheduleError::ReservedSequence => {
                write!(
                    f,
                    "sequence number {} is reserved and cannot be used",
                    u64::MAX
                )
            }
            ScheduleError::SequenceExhausted { player } => {
                write!(f, "player {} has used up its sequence numbers", player.0)
            }
        }
    }
}

impl std::error::Error for ScheduleError {}

/// The pending commands, ordered by `(tick, player, player sequence)`.
///
/// A `BTreeMap` keyed by that triple: its iteration order is sorted by key, so
/// the order is the same on every machine and nothing depends on hashing or on
/// the order the commands arrived in. The sequence number belongs to the
/// player: [`CommandQueue::schedule`] takes the next one of the player's own
/// counter, [`CommandQueue::schedule_sequenced`] takes the one the sender
/// numbered the command with.
///
/// Two queues are equal when they hold the same commands in the same
/// execution order. The sequence numbers themselves are bookkeeping (the
/// tie-breaker for equal ticks and players) and are not compared.
#[derive(Clone, Debug, Default)]
pub struct CommandQueue {
    pending: BTreeMap<(Tick, PlayerId, u64), Command>,
    /// The next sequence number [`CommandQueue::schedule`] hands out, per
    /// player. Raised past every number a sender brought along, so a numbered
    /// and an unnumbered command of one player never meet.
    next_seq: BTreeMap<PlayerId, u64>,
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
        let counter = self.next_seq.entry(player).or_insert(0);
        let seq = *counter;
        if seq == u64::MAX {
            return Err(ScheduleError::SequenceExhausted { player });
        }
        let key = (tick, player, seq);
        // Never overwrite: a command that is already there stays.
        if self.pending.contains_key(&key) {
            return Err(ScheduleError::DuplicateSequence {
                tick,
                player,
                sequence: seq,
            });
        }
        *counter = seq + 1;
        self.pending.insert(key, command);
        Ok(())
    }

    /// Like [`CommandQueue::schedule`], for a command that carries the
    /// sequence number its sender gave it (WP3.4; the lockstep package of
    /// WP14.1 will carry it in the packet). The command runs by
    /// `(tick, player, sequence)`, wherever and whenever it arrived. Numbers
    /// need not be consecutive: gaps are allowed and the commands still run in
    /// number order (that none is missing is for the network layer to check).
    /// Fails if `tick` is before `now`, if the number is `u64::MAX` (reserved)
    /// or if the player already has a pending command for that tick under that
    /// number. A refused command changes nothing.
    pub fn schedule_sequenced(
        &mut self,
        now: Tick,
        tick: Tick,
        player: PlayerId,
        sequence: u64,
        command: Command,
    ) -> Result<(), ScheduleError> {
        if tick < now {
            return Err(ScheduleError::PastTick {
                scheduled: tick,
                current: now,
            });
        }
        if sequence == u64::MAX {
            return Err(ScheduleError::ReservedSequence);
        }
        let key = (tick, player, sequence);
        if self.pending.contains_key(&key) {
            return Err(ScheduleError::DuplicateSequence {
                tick,
                player,
                sequence,
            });
        }
        // `sequence` is below `u64::MAX`, so the sum cannot overflow.
        let counter = self.next_seq.entry(player).or_insert(0);
        *counter = (*counter).max(sequence + 1);
        self.pending.insert(key, command);
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
    fn sequence_numbers_belong_to_the_player() {
        let mut q = CommandQueue::new();
        // Player 1 schedules first, but player 0's commands still run first,
        // and each player's own order is the order of its numbers.
        q.schedule(Tick(0), Tick(2), PlayerId(1), despawn(11))
            .unwrap();
        q.schedule(Tick(0), Tick(2), PlayerId(0), despawn(1))
            .unwrap();
        q.schedule(Tick(0), Tick(2), PlayerId(1), despawn(12))
            .unwrap();
        q.schedule(Tick(0), Tick(2), PlayerId(0), despawn(2))
            .unwrap();
        let keys: Vec<(u8, u64)> = q.pending.keys().map(|&(_, p, s)| (p.0, s)).collect();
        // Each player counts from zero.
        assert_eq!(keys, vec![(0, 0), (0, 1), (1, 0), (1, 1)]);
    }

    #[test]
    fn numbered_commands_run_in_number_order_whatever_the_arrival_order() {
        let arrivals = vec![
            (PlayerId(1), 7, despawn(17)),
            (PlayerId(0), 3, despawn(3)),
            (PlayerId(1), 5, despawn(15)),
            (PlayerId(0), 2, despawn(2)),
            (PlayerId(1), 6, despawn(16)),
        ];
        let mut reversed = arrivals.clone();
        reversed.reverse();
        let mut rotated = arrivals.clone();
        rotated.rotate_left(2);
        let mut expected: Option<CommandQueue> = None;
        for arrival in [arrivals, reversed, rotated] {
            let mut q = CommandQueue::new();
            for (player, seq, command) in arrival {
                q.schedule_sequenced(Tick(0), Tick(4), player, seq, command)
                    .unwrap();
            }
            match &expected {
                None => expected = Some(q.clone()),
                // Queues filled in different arrival orders are equal.
                Some(first) => assert_eq!(first, &q),
            }
            assert_eq!(
                drain(&mut q, Tick(4)),
                vec![
                    (0, despawn(2)),
                    (0, despawn(3)),
                    (1, despawn(15)),
                    (1, despawn(16)),
                    (1, despawn(17)),
                ]
            );
        }
    }

    #[test]
    fn a_reused_sequence_number_is_refused_and_the_first_stays() {
        let mut q = CommandQueue::new();
        q.schedule_sequenced(Tick(0), Tick(4), PlayerId(0), 9, despawn(1))
            .unwrap();
        let err = q
            .schedule_sequenced(Tick(0), Tick(4), PlayerId(0), 9, despawn(2))
            .unwrap_err();
        assert_eq!(
            err,
            ScheduleError::DuplicateSequence {
                tick: Tick(4),
                player: PlayerId(0),
                sequence: 9
            }
        );
        assert!(err.to_string().contains("sequence number 9"));
        assert_eq!(q.len(), 1);
        // The same number for another tick or another player is fine.
        q.schedule_sequenced(Tick(0), Tick(5), PlayerId(0), 9, despawn(3))
            .unwrap();
        q.schedule_sequenced(Tick(0), Tick(4), PlayerId(1), 9, despawn(4))
            .unwrap();
        // A past tick is refused first.
        assert!(matches!(
            q.schedule_sequenced(Tick(6), Tick(5), PlayerId(0), 1, despawn(5)),
            Err(ScheduleError::PastTick { .. })
        ));
        assert_eq!(
            drain(&mut q, Tick(4)),
            vec![(0, despawn(1)), (1, despawn(4))]
        );
    }

    #[test]
    fn the_reserved_number_is_refused_and_nothing_is_overwritten() {
        let mut q = CommandQueue::new();
        // u64::MAX is refused outright and changes nothing.
        assert_eq!(
            q.schedule_sequenced(Tick(0), Tick(1), PlayerId(0), u64::MAX, despawn(1)),
            Err(ScheduleError::ReservedSequence)
        );
        assert!(q.is_empty());
        // The highest usable number leaves the counter on u64::MAX: the next
        // unnumbered command is refused instead of reusing a key.
        q.schedule_sequenced(Tick(0), Tick(1), PlayerId(0), u64::MAX - 1, despawn(2))
            .unwrap();
        for tick in [1, 2] {
            assert_eq!(
                q.schedule(Tick(0), Tick(tick), PlayerId(0), despawn(3)),
                Err(ScheduleError::SequenceExhausted {
                    player: PlayerId(0)
                })
            );
        }
        assert_eq!(q.len(), 1);
        assert_eq!(drain(&mut q, Tick(1)), vec![(0, despawn(2))]);
        // Other players are not affected.
        q.schedule(Tick(0), Tick(1), PlayerId(1), despawn(4))
            .unwrap();
        assert!(
            ScheduleError::ReservedSequence
                .to_string()
                .contains("reserved")
        );
        assert!(
            ScheduleError::SequenceExhausted {
                player: PlayerId(0)
            }
            .to_string()
            .contains("used up")
        );
    }

    #[test]
    fn schedule_never_overwrites_an_existing_key() {
        let mut q = CommandQueue::new();
        // A sender's number 0 for tick 3 is there; the local counter is also on 0
        // before it has seen the number, but raising it keeps them apart.
        q.schedule_sequenced(Tick(0), Tick(3), PlayerId(0), 0, despawn(10))
            .unwrap();
        q.schedule(Tick(0), Tick(3), PlayerId(0), despawn(11))
            .unwrap();
        assert_eq!(
            drain(&mut q, Tick(3)),
            vec![(0, despawn(10)), (0, despawn(11))]
        );
        // Gaps are allowed and the numbers still decide the order.
        q.schedule_sequenced(Tick(4), Tick(5), PlayerId(0), 900, despawn(12))
            .unwrap();
        q.schedule_sequenced(Tick(4), Tick(5), PlayerId(0), 7, despawn(13))
            .unwrap();
        assert_eq!(
            drain(&mut q, Tick(5)),
            vec![(0, despawn(13)), (0, despawn(12))]
        );
    }

    #[test]
    fn an_unnumbered_command_never_meets_a_numbered_one() {
        let mut q = CommandQueue::new();
        q.schedule_sequenced(Tick(0), Tick(1), PlayerId(0), 40, despawn(40))
            .unwrap();
        // The local counter went past the number the sender brought along.
        q.schedule(Tick(0), Tick(1), PlayerId(0), despawn(41))
            .unwrap();
        let ran = drain(&mut q, Tick(1));
        assert_eq!(ran, vec![(0, despawn(40)), (0, despawn(41))]);
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
