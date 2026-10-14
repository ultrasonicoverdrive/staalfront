//! [`World`]: the simulation state and the fixed tick.

use core::fmt;

use sf_math::{Fx, Rng, Vec2};

use crate::behavior::{self, Area, Behavior, Motion};
use crate::command::{Command, CommandQueue, ScheduleError};
use crate::entities::{Entities, NewEntity};
use crate::ids::{EntityId, Kind, PlayerId, Tick, per_tick};
use crate::snapshot::{EntityView, Snapshot};

/// The most slots a world will ever allocate. A `Spawn` beyond it is ignored.
/// It bounds memory against a runaway command stream and keeps slot indices
/// far below `u32::MAX`.
pub const MAX_ENTITIES: u32 = 1 << 20;

/// The largest map side, in cells.
pub const MAX_MAP_SIDE: u32 = 1024;

/// The default map side: 256 cells, the largest map of PLAN.md.
pub const DEFAULT_MAP_SIDE: u32 = 256;

/// The default movement speed, in cells per second.
pub const DEFAULT_SPEED: Fx = Fx::from_int(4);

/// The default turning speed: 8 of 256 facing steps per tick (225 degrees per
/// second).
pub const DEFAULT_TURN_PER_TICK: u8 = 8;

/// Everything that is fixed when a world is created. It is part of the
/// checksum, so two worlds with different settings never compare equal.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct WorldConfig {
    /// Seeds every random stream of the world.
    pub seed: u64,
    /// Map width in cells, `1..=MAX_MAP_SIDE`.
    pub width: u32,
    /// Map height in cells, `1..=MAX_MAP_SIDE`.
    pub height: u32,
    /// Walking speed in cells per second, for every behaviour (a placeholder
    /// until movement classes exist). Must come out above zero per tick.
    pub speed: Fx,
    /// Facing steps (256 per full turn) an entity may turn per tick; zero is refused.
    pub turn_per_tick: u8,
}

impl WorldConfig {
    /// The default 256 by 256 map with the given seed.
    pub const fn new(seed: u64) -> WorldConfig {
        WorldConfig {
            seed,
            width: DEFAULT_MAP_SIDE,
            height: DEFAULT_MAP_SIDE,
            speed: DEFAULT_SPEED,
            turn_per_tick: DEFAULT_TURN_PER_TICK,
        }
    }

    /// The same configuration on a `width` by `height` map.
    pub const fn with_map_size(mut self, width: u32, height: u32) -> WorldConfig {
        self.width = width;
        self.height = height;
        self
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.width == 0 || self.height == 0 {
            return Err(ConfigError::EmptyMap);
        }
        if self.width > MAX_MAP_SIDE || self.height > MAX_MAP_SIDE {
            return Err(ConfigError::MapTooLarge);
        }
        if per_tick(self.speed) <= Fx::ZERO {
            return Err(ConfigError::SpeedTooLow);
        }
        if self.turn_per_tick == 0 {
            return Err(ConfigError::NoTurning);
        }
        Ok(())
    }
}

impl Default for WorldConfig {
    fn default() -> WorldConfig {
        WorldConfig::new(0)
    }
}

/// Why a [`WorldConfig`] was refused.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ConfigError {
    /// The map has no cells.
    EmptyMap,
    /// A map side is above [`MAX_MAP_SIDE`].
    MapTooLarge,
    /// The speed rounds to nothing per tick, so nothing could ever arrive.
    SpeedTooLow,
    /// A turn rate of zero would leave misaligned entities stuck forever.
    NoTurning,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ConfigError::EmptyMap => "the map has no cells",
            ConfigError::MapTooLarge => "a map side is above the maximum",
            ConfigError::SpeedTooLow => "the speed rounds to zero per tick",
            ConfigError::NoTurning => "the turn rate per tick is zero",
        })
    }
}

impl std::error::Error for ConfigError {}

/// What one call of [`World::step`] did.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct StepReport {
    /// The tick that was just executed.
    pub tick: Tick,
    /// The checksum of the world after that tick.
    pub checksum: u64,
    /// Commands that ran and changed something.
    pub commands_executed: u32,
    /// Commands that ran and changed nothing (every target stale, the entity
    /// limit reached).
    pub commands_ignored: u32,
}

/// The complete simulation state. See the crate documentation for the design.
#[derive(Clone, PartialEq, Eq)]
pub struct World {
    pub(crate) config: WorldConfig,
    /// Ticks executed so far, which is the number of the next tick to run.
    pub(crate) tick: Tick,
    pub(crate) entities: Entities,
    /// Free slot indices, used as a stack: the last freed is reused first.
    pub(crate) free: Vec<u32>,
    /// Number of live entities (derived from `entities.alive`).
    live: u32,
    /// Spawns so far; gives every entity its own random stream.
    pub(crate) spawn_serial: u64,
    pub(crate) queue: CommandQueue,
    /// `history[k]` is the checksum after tick `k` ran.
    pub(crate) history: Vec<u64>,
}

impl fmt::Debug for World {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("World")
            .field("config", &self.config)
            .field("tick", &self.tick)
            .field("entities", &self.live)
            .field("slots", &self.entities.len())
            .field("pending_commands", &self.queue.len())
            .field("checksum", &format_args!("{:#018x}", self.checksum()))
            .finish()
    }
}

/// A SplitMix64-style finalizer: spreads nearby inputs over all 64 bits, so
/// consecutive spawn numbers give unrelated generator seeds.
const fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The random stream of the `serial`-th entity ever spawned in a world with
/// the given seed. Depends only on those two numbers, never on the slot, the
/// tick or any other entity.
fn entity_rng(seed: u64, serial: u64) -> Rng {
    let a = mix64(seed ^ 0x9E37_79B9_7F4A_7C15);
    let b = mix64(serial.wrapping_add(0xD1B5_4A32_D192_ED03));
    Rng::with_stream(mix64(a ^ b), mix64(b.wrapping_add(a)))
}

impl World {
    /// A world at tick zero with no entities.
    pub fn new(config: WorldConfig) -> Result<World, ConfigError> {
        config.validate()?;
        Ok(World {
            config,
            tick: Tick::ZERO,
            entities: Entities::default(),
            free: Vec::new(),
            live: 0,
            spawn_serial: 0,
            queue: CommandQueue::new(),
            history: Vec::new(),
        })
    }

    // ---- reading ----------------------------------------------------------

    /// The configuration the world was created with.
    pub fn config(&self) -> &WorldConfig {
        &self.config
    }

    /// The map size in cells, `(width, height)`.
    pub fn map_size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Ticks executed so far, which is also the number of the next tick.
    pub fn tick(&self) -> Tick {
        self.tick
    }

    /// The number of live entities.
    pub fn len(&self) -> usize {
        self.live as usize
    }

    /// True if there are no live entities.
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// The number of slots in use (live plus free). Never shrinks.
    pub fn slot_count(&self) -> usize {
        self.entities.len()
    }

    /// True if `id` names a live entity (right slot, right generation).
    pub fn is_alive(&self, id: EntityId) -> bool {
        self.live_index(id).is_some()
    }

    /// The entity `id` names, or `None` if it is gone or the id is stale.
    pub fn get(&self, id: EntityId) -> Option<EntityView> {
        self.live_index(id).map(|i| self.view(i))
    }

    /// A read-only view of every live entity for the presentation.
    pub fn snapshot(&self) -> Snapshot<'_> {
        Snapshot::new(self)
    }

    /// The commands waiting for their tick.
    pub fn pending_commands(&self) -> &CommandQueue {
        &self.queue
    }

    /// The checksums of all executed ticks: entry `k` is the checksum of the
    /// world right after tick `k` ran (when [`World::tick`] became `k + 1`).
    /// Two worlds that stay in sync have equal histories.
    pub fn checksum_history(&self) -> &[u64] {
        &self.history
    }

    /// The checksum right after tick `tick` ran, if it has.
    pub fn checksum_after(&self, tick: Tick) -> Option<u64> {
        usize::try_from(tick.0)
            .ok()
            .and_then(|k| self.history.get(k).copied())
    }

    // ---- input ------------------------------------------------------------

    /// Queues `command` from `player` to run at the start of `tick`.
    ///
    /// This is the only way input reaches the world. A `tick` before
    /// [`World::tick`] is refused (see [`ScheduleError`]); the tick that is
    /// about to run is fine.
    pub fn schedule(
        &mut self,
        tick: Tick,
        player: PlayerId,
        command: Command,
    ) -> Result<(), ScheduleError> {
        self.queue.schedule(self.tick, tick, player, command)
    }

    // ---- the tick ---------------------------------------------------------

    /// Runs exactly one tick and returns what happened.
    ///
    /// In order: (1) every command scheduled for this tick runs, in
    /// `(player, insertion)` order; (2) every live entity runs its behaviour,
    /// in ascending slot order, after recording its previous position and
    /// facing; (3) the tick counter advances; (4) the checksum of the new state
    /// is computed and appended to the history.
    pub fn step(&mut self) -> StepReport {
        let tick = self.tick;

        let mut executed: u32 = 0;
        let mut ignored: u32 = 0;
        while let Some((_player, command)) = self.queue.pop_due(tick) {
            if self.apply(command) {
                executed = executed.saturating_add(1);
            } else {
                ignored = ignored.saturating_add(1);
            }
        }

        let motion = self.motion();
        behavior::advance_all(&mut self.entities, &motion);

        self.tick = tick.next();
        let checksum = self.checksum();
        self.history.push(checksum);
        StepReport {
            tick,
            checksum,
            commands_executed: executed,
            commands_ignored: ignored,
        }
    }

    // ---- internals --------------------------------------------------------

    fn motion(&self) -> Motion {
        let step = per_tick(self.config.speed);
        let (pos_min, pos_max) = self.pos_range();
        Motion {
            step,
            step_sq: step.square(),
            turn_per_tick: self.config.turn_per_tick,
            pos_min,
            pos_max,
        }
    }

    /// The map as an area: corners `(0, 0)` and `(width, height)`.
    fn bounds(&self) -> Area {
        Area::new(
            Vec2::ZERO,
            Vec2::from_ints(self.config.width as i32, self.config.height as i32),
        )
    }

    /// The smallest and largest valid position: the far edge of the map is
    /// exclusive, so the maximum is one raw unit inside it.
    fn pos_range(&self) -> (Vec2, Vec2) {
        let far = self.bounds().max - Vec2::splat(Fx::EPSILON);
        (Vec2::ZERO, far)
    }

    /// The slot of `id` if it names a live entity.
    fn live_index(&self, id: EntityId) -> Option<usize> {
        let i = id.index() as usize;
        let live = *self.entities.alive.get(i)? && self.entities.generation[i] == id.generation();
        live.then_some(i)
    }

    pub(crate) fn view(&self, i: usize) -> EntityView {
        let e = &self.entities;
        EntityView {
            id: EntityId::new(i as u32, e.generation[i]),
            kind: e.kind[i],
            owner: e.owner[i],
            prev_pos: e.prev_pos[i],
            pos: e.pos[i],
            prev_facing: e.prev_facing[i],
            facing: e.facing[i],
        }
    }

    /// Creates an entity and returns its id, or `None` at the entity limit.
    ///
    /// The slot is the most recently freed one if there is any, otherwise a
    /// new one at the end. The entity starts wandering over the whole map with
    /// a random heading from its own stream.
    pub(crate) fn spawn(&mut self, kind: Kind, pos: Vec2, owner: PlayerId) -> Option<EntityId> {
        let index = match self.free.pop() {
            Some(i) => i as usize,
            None => {
                if self.entities.len() >= MAX_ENTITIES as usize {
                    return None;
                }
                self.entities.push_free_slot()
            }
        };
        let serial = self.spawn_serial;
        self.spawn_serial = serial.wrapping_add(1);
        let mut rng = entity_rng(self.config.seed, serial);
        let facing = rng.facing();
        let (pos_min, pos_max) = self.pos_range();
        self.entities.fill(
            index,
            NewEntity {
                kind,
                owner,
                pos: pos.clamp(pos_min, pos_max),
                facing,
                behavior: Behavior::Wander,
                area: self.bounds(),
                rng,
            },
        );
        self.live = self.live.saturating_add(1);
        Some(EntityId::new(index as u32, self.entities.generation[index]))
    }

    /// Removes an entity. False if `id` is stale or unknown.
    ///
    /// The slot becomes a tombstone: it keeps its place (nothing is moved, so
    /// the slot order of everything else is untouched) and its generation goes
    /// up by one, which invalidates every id issued for the old entity. The
    /// index goes on the free-list stack. A slot whose generation counter
    /// would overflow is retired for good instead of being reused.
    pub(crate) fn despawn(&mut self, id: EntityId) -> bool {
        let Some(i) = self.live_index(id) else {
            return false;
        };
        self.entities.clear(i);
        if let Some(next) = self.entities.generation[i].checked_add(1) {
            self.entities.generation[i] = next;
            self.free.push(id.index());
        }
        self.live = self.live.saturating_sub(1);
        true
    }

    /// Runs one command. True if it changed anything.
    fn apply(&mut self, command: Command) -> bool {
        match command {
            Command::Spawn { kind, pos, owner } => self.spawn(kind, pos, owner).is_some(),
            Command::Despawn { id } => self.despawn(id),
            Command::SetWanderArea { ids, area } => {
                let area = area.clamped_to(self.bounds());
                let mut changed = false;
                for id in ids {
                    if let Some(i) = self.live_index(id) {
                        self.entities.area[i] = area;
                        self.entities.behavior[i] = Behavior::Wander;
                        self.entities.target[i] = None;
                        changed = true;
                    }
                }
                changed
            }
            Command::MoveTo { ids, target } => {
                let (pos_min, pos_max) = self.pos_range();
                let target = target.clamp(pos_min, pos_max);
                let mut changed = false;
                for id in ids {
                    if let Some(i) = self.live_index(id) {
                        self.entities.behavior[i] = Behavior::MoveTo;
                        self.entities.target[i] = Some(target);
                        changed = true;
                    }
                }
                changed
            }
        }
    }
}

#[cfg(test)]
mod tests;
