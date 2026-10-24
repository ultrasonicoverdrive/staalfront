//! The complete game simulation: no GPU, no I/O, no clock, no floats.
//!
//! `sf_sim` holds the world as a struct of arrays, advances it in fixed
//! ticks, accepts input only as scheduled [`Command`]s, and fingerprints its
//! state after every tick. Everything it computes is integer or `sf_math`
//! fixed point, so the same seed and the same commands give bit-identical
//! state on every machine, in debug and in release (PLAN.md decision B3).
//! That is what replays, saves, lockstep multiplayer and headless tests stand
//! on.
//!
//! The world contains one placeholder behaviour, "wander", so thousands of
//! entities move around without any gameplay. There is no collision and no
//! combat yet. Since WP2.1 the map has a terrain type and an occupant per
//! cell (see "Terrain and occupancy" below); since WP3.2 a world can spawn
//! units and structures with the stats of a unit table (see "Units" below);
//! since WP4.1 a unit's movement class decides which cells it may enter and
//! how fast it moves there (see "Movement classes" below); since WP5.1 a world
//! can be given a weapon table, and a unit with an attack order then fires; since WP5.2
//! a shot is an instant hit or a projectile that flies and detonates (see [`weapons`] and
//! [`projectiles`]).
//!
//! ```
//! use sf_math::Vec2;
//! use sf_sim::{Command, Kind, PlayerId, Tick, World, WorldConfig};
//!
//! let mut world = World::new(WorldConfig::new(42)).unwrap();
//! // Input is a command for a tick. Tick 0 is the one about to run.
//! world
//!     .schedule(
//!         Tick(0),
//!         PlayerId(0),
//!         Command::Spawn { kind: Kind(1), pos: Vec2::from_ints(10, 10), owner: PlayerId(0) },
//!     )
//!     .unwrap();
//! let report = world.step();
//! assert_eq!(report.commands_executed, 1);
//! assert_eq!(world.len(), 1);
//! assert_eq!(world.checksum_history(), &[report.checksum]);
//! // Let it wander for five seconds of game time.
//! for _ in 0..100 {
//!     world.step();
//! }
//! assert_eq!(world.tick(), Tick(101));
//! // The presentation reads a snapshot: previous and current state.
//! let view = world.snapshot().iter().next().unwrap();
//! assert_ne!(view.pos, Vec2::from_ints(10, 10));
//! // A command for a tick that already ran is refused.
//! assert!(world.schedule(Tick(0), PlayerId(0), Command::Despawn { id: view.id }).is_err());
//! ```
//!
//! # Time
//!
//! The simulation runs at [`TICKS_PER_SECOND`] (20) ticks per second, a fixed
//! step. [`World::tick`] counts the ticks executed so far, which is also the
//! number of the tick the next [`World::step`] runs. [`World::step`] advances
//! exactly one tick and does, in this order:
//!
//! * run every command scheduled for this tick, in `(player, insertion)`
//!   order;
//! * for every live entity in ascending slot order: remember the previous
//!   position and facing, then run its behaviour;
//! * resolve the bodies of the units (the crowd pass, WP4.4) and turn the
//!   turrets (WP4.5);
//! * count the weapons down and fire them, move the projectiles in flight and detonate them, when the
//!   world has a weapon table (WP5.1, WP5.2): every decision first, then all the damage at once, each
//!   hit through the armor table and with its splash (WP5.3);
//! * advance the tick counter;
//! * compute the checksum of the new state and append it to the history.
//!
//! There is no clock: the caller decides how often to call `step` (the game
//! binary from the engine's fixed-step loop, 20 times per second of game time;
//! the headless runner as fast as it can). Speeds and durations are written per
//! second and converted with [`per_tick`].
//!
//! # Entity storage
//!
//! Entities live in one struct-of-arrays store with a column per property:
//! generation, live flag, kind, owner, position, previous position, facing,
//! previous facing, behaviour, target, wander area, a random stream, the
//! cell a structure occupies (none for a wanderer or a unit), and for an entity
//! made from the unit table its table entry and its hit points. An
//! entity is a slot number, nothing more; the columns are plain `Vec`s walked
//! in index order, which is cache friendly and the only iteration order the
//! simulation ever uses.
//!
//! **Ids.** An [`EntityId`] is `{ index, generation }`. Each slot counts how
//! often it has been freed; an id is valid only while its generation equals the
//! slot's. A stale id (the entity was despawned, maybe replaced) is simply not
//! found by [`World::get`], [`World::is_alive`] or any command.
//!
//! **Spawn.** A spawn takes the most recently freed slot (the free list is a
//! stack, last freed is reused first), or appends a new slot when the free
//! list is empty. Ids are therefore fully determined by the command history:
//! in a fresh world the first spawn is `{ 0, 0 }`, the next `{ 1, 0 }`, and so
//! on, which lets a scenario script name entities before they exist.
//!
//! **Despawn.** The slot becomes a tombstone *in place*: its live flag clears,
//! its columns reset, its generation goes up by one, and its index is pushed on
//! the free list. Nothing moves, there is no swap-remove, so the slot of every
//! other entity, and therefore the iteration order, is unaffected by a despawn.
//! Iteration order is ascending slot index, *not* age: a new entity that
//! reuses a low slot is visited before older ones in higher slots. The columns
//! never shrink (shrinking would let a stale id match a new entity of the same
//! generation); a slot whose generation counter would overflow is retired
//! instead of reused. The number of slots is capped at [`MAX_ENTITIES`].
//!
//! # Randomness
//!
//! Every entity owns a PCG32 stream ([`sf_math::Rng`]) seeded from the world
//! seed and the number of entities spawned before it (not from its slot, which
//! can be reused). Compared with one world generator this costs 16 bytes per
//! entity and buys: an entity's random choices depend only on its own history,
//! so spawning, despawning or commanding *other* entities never changes them;
//! results do not depend on the order entities are processed in; and a later
//! parallel update would not change results. Every stream state is part of the
//! checksum.
//!
//! # Commands
//!
//! [`World::schedule`] is the only way input enters. See [`command`] for the
//! ordering rules: commands run at the start of their tick by
//! `(tick, player, insertion)`, a command for an already executed tick is
//! rejected with [`ScheduleError::PastTick`], and commands naming stale ids
//! skip those ids. The set: [`Command::Spawn`], [`Command::Place`] (the
//! placeholders), [`Command::SpawnUnit`], [`Command::PlaceStructure`] (from the
//! unit table), [`Command::Despawn`], [`Command::SetWanderArea`],
//! [`Command::MoveTo`] and, since WP3.4, [`Command::Order`] and
//! [`Command::Stop`] (see "Orders" below) and, since WP4.5, [`Command::AimTurret`]
//! (see "Turning and turrets" below). Commands run by `(tick, player, player
//! sequence)`; [`World::schedule_sequenced`] takes the sequence number the sender gave.
//! Pending commands are simulation state: they are in the checksum and survive a clone of the
//! world.
//!
//! # Checksum
//!
//! [`World::checksum`] hashes the whole state in a canonical order with
//! FNV-1a (see [`checksum`] for exactly what is and is not covered). It is
//! computed after every tick and kept in [`World::checksum_history`], where
//! entry `k` is the state after tick `k`. `sf_headless` writes the history to
//! disk and CI compares it between Linux and Windows; `tests/golden.rs` pins a
//! scenario to committed values.
//!
//! # Wander
//!
//! New entities wander over the whole map: pick a random target in the
//! entity's [`Area`], turn toward it, walk there at the configured speed, pick
//! another on arrival. [`behavior`] describes the motion model. Positions are
//! kept inside the map (default 256 by 256 cells, see [`WorldConfig`]) by
//! construction: spawn positions, areas and targets are clamped, and every
//! move is clamped again.
//!
//! # Terrain and occupancy
//!
//! Every cell of the map (a gameplay cell is 4 m, so `Fx::ONE` of position)
//! has a terrain type and an occupant, both in fixed-size row-major arrays
//! (see [`terrain`]). [`World::new`] gives open ground everywhere;
//! [`World::with_terrain`] takes a [`Terrain`] that the caller built from a
//! map's terrain layer (`sf_data` reads the file, `sf_game` or `sf_headless`
//! hands the numbers over, so the simulation does no I/O). The queries are
//! read-only: [`World::terrain_at`], [`World::occupant_at`],
//! [`World::cell_of`], [`World::cell_at`]. The only way to change occupancy is
//! the tick: [`Command::Place`] spawns a standing structure that claims its
//! cell if the terrain is buildable and the cell is free, and despawning it
//! frees the cell. Wanderers occupy nothing and ignore terrain, so scenarios
//! without `Place` and without a layer behave, and checksum, exactly as before
//! (see [`checksum`]).
//!
//! ```
//! use sf_sim::{Cell, Command, Kind, PlayerId, Terrain, TerrainDef, TerrainFlags, TerrainId, Tick, World, WorldConfig};
//!
//! // A 4 x 1 map: three open cells and one cell of water.
//! let water = TerrainDef { id: TerrainId(2), flags: TerrainFlags::default() };
//! let grass = TerrainDef { id: TerrainId(0), flags: TerrainFlags::OPEN };
//! let terrain = Terrain::from_layer(4, 1, &[0, 0, 2, 0], &[grass, water]).unwrap();
//! let config = WorldConfig::new(1).with_map_size(4, 1);
//! let mut world = World::with_terrain(config, terrain).unwrap();
//! let place = |x| Command::Place { kind: Kind(7), cell: Cell::new(x, 0), owner: PlayerId(0) };
//! for x in [0, 2, 0] {
//!     world.schedule(Tick(0), PlayerId(0), place(x)).unwrap();
//! }
//! let report = world.step();
//! // The first one stands; the one on water and the second one on the same cell are ignored.
//! assert_eq!((report.commands_executed, report.commands_ignored), (1, 2));
//! assert_eq!(world.terrain_at(Cell::new(2, 0)), Some(TerrainId(2)));
//! assert!(world.occupant_at(Cell::new(0, 0)).is_some());
//! assert!(world.occupant_at(Cell::new(2, 0)).is_none());
//! ```
//!
//! # Units
//!
//! [`World::with_units`] takes a [`UnitTable`]: for each unit kind its hit
//! points, movement class, speed per tick, turn rate, size and footprint,
//! which the caller copies out of the `sf_data` definitions (the simulation
//! does not depend on `sf_data`). [`Command::SpawnUnit`] makes a mobile unit
//! or an object, [`Command::PlaceStructure`] a building on its footprint; both
//! refuse a kind the table does not know, and the structure is refused unless
//! every cell of its footprint is on the map, buildable and free. A mobile unit
//! occupies no cell. A unit moves by its own speed per tick and turn rate
//! instead of the world's placeholder speed. The legacy [`Command::Spawn`] and
//! [`Command::Place`] ignore the table: their kind is an opaque number, so the
//! wanderer scenarios and their checksums stay as they were. The rules are in
//! `docs/rules/units.md`.
//!
//! # Presentation
//!
//! [`World::snapshot`] gives a read-only [`Snapshot`] of every live entity
//! with previous and current position and facing, for interpolation between
//! ticks. The crate has no rendering dependency and cannot reach the float
//! conversions of `sf_math`.
//!
//! # Determinism rules and how they are enforced
//!
//! * No floating point types and no float literals:
//!   `scripts/check-sim-determinism.sh` scans `src/` for them, and clippy's
//!   float lints are denied for the crate.
//! * No `std` hash collections: the same script, and `clippy.toml`
//!   (`disallowed-types`). Ordered containers only (`BTreeMap`, `Vec`).
//! * No clock, no I/O, no threads, no global state: nothing in `src/` touches
//!   them; the `sf_math` generator is always seeded explicitly.
//! * Every integer operation that could overflow is saturating or wrapping on
//!   purpose, so debug builds (overflow checks on) and release builds compute
//!   the same bits.
//! * `sf_math`'s float helpers sit behind its `render` feature, which this
//!   crate does not enable: built on its own, `sf_sim` cannot name them.
//!
//! # Orders
//!
//! A unit has a current [`Order`] and a ring of [`ORDER_QUEUE_CAP`] queued ones.
//! [`Command::Order`] replaces them or, queued, appends; [`Command::Stop`] clears
//! them. The behaviour pass carries the order out: it steers at the next waypoint of
//! the unit's route (`path::Pathing::steer`; the straight line for a unit that terrain
//! does not bind) and judges arrival against the destination. Attack,
//! attack-move and guard are state and movement only: combat is phase five. The rules
//! are in `docs/rules/orders.md`.
//!
//! # Movement classes
//!
//! The terrain says what a cell *is*; the movement class of a unit says what it
//! can do there. [`World::with_movement`] takes a [`MovementTable`] (built by
//! `sf_data`/`sf_bridge` from `unit_classes.ron`) with, for every class and
//! terrain type, [`Cost::Impassable`] or a speed multiplier in thousandths.
//! [`World::cost`] and [`World::passable`] are the read-only queries; a unit
//! made from the unit table moves at its speed times the multiplier of the cell
//! it stands in and stops at the edge of a cell it cannot enter (see
//! [`movement`]). Wanderers and worlds without a layer or without a table are
//! open ground and behave, and checksum, exactly as before.
//!
//! # Pathfinding
//!
//! A unit that walks an order and is bound by terrain asks for a **route** and follows
//! it: hierarchical A* over sectors of [`SECTOR_SIZE`] cells and the portals between
//! them, built when the world is made, rebuilt one sector at a time when a structure is
//! placed or removed (by the tick itself), refined lazily into a fixed-size
//! ring of waypoints pulled taut by a line of sight, and served inside a node budget
//! per tick, going round the entity ids ([`World::with_path_budget`],
//! [`World::path_stats`]). A destination that cannot be reached ends the route in the
//! nearest cell that can, marked in [`World::route_of`]. Aircraft, the legacy `MoveTo`
//! and the wanderers walk the straight line as before. The rules are in
//! `docs/rules/pathfinding.md`; the module is [`path`].
//!
//! # Flowfields for groups
//!
//! A single [`Command::Order`] that walks at least a threshold of units of one movement
//! class to one point ([`World::with_flowfield_threshold`], 8 as shipped in `sim.ron`) gets
//! one **flowfield** for the class instead of a route each: the cost to the goal and a
//! direction in every cell of the sectors on the abstract corridor between the group and the
//! goal, built by a resumable job inside the same node budget as the routes, followed by the
//! units to the farthest visible cell down the chain of directions, and a spiral of arrival
//! slots round the goal so that the group spreads. A fixed pool of fields with least recently
//! used eviction; a structure in the region builds the field again while the units go on with
//! the old one where it is valid. The rules are in `docs/rules/pathfinding.md`.
//!
//! # Turning and turrets
//!
//! Every movement class has a [`Turning`] profile (in the [`MovementTable`], `sf_data`'s `turning` block
//! of `unit_classes.ron`), and a walk turns the hull by it before and while it drives: infantry
//! at once ([`TurnMode::Instant`]), tracked units on the spot, driving within a tolerance
//! ([`TurnMode::Pivot`]), wheeled, hover and water units only while they move, in an arc of a
//! smallest radius, with a turn on the spot for a target inside the turning circle
//! ([`TurnMode::Arc`]). The shortest way round is always the one taken, the tie at half a
//! turn goes the decreasing way. A unit with a `turret_rate` has a turret facing of its own
//! ([`EntityView::turret`], [`World::turret_of`]): an absolute direction that follows
//! [`Command::AimTurret`], the victim of an attack order or the hull, at the turret's rate. The rules
//! are in `docs/rules/turning.md`; the modules are [`turning`] and the private `turret`.
//!
//! # Collisions
//!
//! A unit made from the unit table has a body, a disc with half the `size` of its
//! definition for a radius. After the units have taken their steps ([`behavior`]),
//! the crowd pass ([`crowd`]) pushes overlapping bodies apart in a few passes over
//! a spatial index, the same whatever order the units are visited in: units that
//! move the same way follow, ones that meet head-on slip to their own right, a unit
//! that has waited detours, an idle friend is shoved aside (a little, and never
//! into rock) and a unit holding position never is. A unit that does not get closer
//! to where it is going steps to a free neighbouring spot and, if that does not
//! help, asks for a new route ([`World::replan_requested`]), round the bodies that stand still
//! near it, and gives its order up when that does not help either (WP4.7, [`ReplanConfig`]).
//! [`World::with_collision`] turns it off or tunes it; the rules are in
//! `docs/rules/collision.md`.
//!
//! # Formations
//!
//! A single [`Command::Order`] with an [`Order::Move`] that sends at least
//! [`FormationConfig::min_units`] units (and at most `max_units`) that cross the same ground to one
//! point makes them a **formation** ([`formation`], [`World::with_formation`],
//! [`World::formation_of`]): the layout of the group becomes a lattice with room for the widest body,
//! every unit has a slot that hangs from the leader and turns with the way it goes, the leader goes no
//! faster than the slowest member, the others walk to their slots, the shape lets go in narrow places
//! and forms up again on open ground, and at the goal the members take the arrival slots of the group
//! order. The rules are in `docs/rules/formations.md`.
//!
//! A world given targeting settings ([`targeting`], [`World::with_targeting`]) lets its idle, holding,
//! guarding and attack-moving units pick the target their weapon does most to, and turn on whoever shoots
//! them, by taking an [`Order::Pursue`] on it and going back to the order they had afterwards. The rules
//! are in `docs/rules/targeting.md`.
//!
//! # Not here yet
//!
//! The destruction of what is shot down (WP5.5), and
//! saving the state to bytes (phase 13). The world is `Clone`, which already
//! gives a perfect in-memory snapshot for tests.

pub mod armor;
pub mod behavior;
pub mod buildings;
pub mod checksum;
pub mod command;
pub mod crowd;
mod entities;
pub mod formation;
pub mod ids;
pub mod movement;
pub mod orders;
pub mod path;
pub mod projectiles;
pub mod snapshot;
pub mod targeting;
pub mod terrain;
pub mod turning;
mod turret;
pub mod units;
pub mod weapons;
pub mod world;

pub use armor::{ArmorError, ArmorTable, MAX_PERCENT as MAX_ARMOR_PERCENT};
pub use behavior::{ALIGN_TOLERANCE, Area, Behavior};
pub use buildings::{BuildingFunction, BuildingStats, BuildingTable, BuildingsError, Side};
pub use command::{Command, CommandQueue, ScheduleError};
pub use crowd::{
    CollisionConfig, CrowdStats, DEFAULT_STUCK_ATTEMPTS, DEFAULT_STUCK_TICKS, DEFAULT_WAIT_TICKS,
};
pub use formation::{FormationConfig, FormationState, FormationStats, FormationView, MemberMode};
pub use ids::{
    EntityId, Kind, PlayerId, TICK_DURATION, TICK_MILLIS, TICKS_PER_SECOND, Tick, per_tick,
};
pub use movement::{
    ClassCosts, Cost, FULL_PERMILLE, MAX_STEP, MovementTable, MovementTableError, scale_step,
};
pub use orders::{
    ARRIVE_TOLERANCE, GUARD_CLOSE, GUARD_LEASH, GuardTarget, ORDER_QUEUE_CAP, Order, SCATTER_MAX,
    SCATTER_MIN, scatter_offset,
};
pub use path::{
    ABS_CAP, DEFAULT_FLOWFIELD_THRESHOLD, DEFAULT_NODE_BUDGET, DEFAULT_REPLAN_COOLDOWN,
    DEFAULT_REPLAN_MAX, DEFAULT_RETARGET_COOLDOWN, FIELD_IDLE_TICKS, FIELD_POOL, FIELD_SECTORS,
    MAX_FIELD_SLOTS, MAX_REPLAN_COOLDOWN, MAX_REPLAN_MAX, MIN_NODE_BUDGET, OVERLAY_CAP, PathStats,
    PlannedRoute, RING_CAP, ReplanConfig, RouteStatus, RouteView, SECTOR_SIZE,
};
pub use projectiles::{GUIDED_FLIGHT_FACTOR, HIT_SLACK, ImpactCause, ProjectileId};
pub use snapshot::{EntityView, ProjectileView, Snapshot, TurretView};
pub use targeting::{
    DEFAULT_PURSUIT_CELLS, DEFAULT_PURSUIT_RESET_TICKS, DEFAULT_SCAN_COOLDOWN, Engagement,
    MAX_PURSUIT_CELLS, MAX_PURSUIT_RESET_TICKS, MAX_SCAN_COOLDOWN, TargetingConfig,
};
pub use terrain::{
    Cell, NO_OCCUPANT, Occupancy, Terrain, TerrainDef, TerrainError, TerrainFlags, TerrainId,
};
pub use turning::{
    DEFAULT_DRIVE_TOLERANCE, MAX_DRIVE_TOLERANCE, MIN_ARC_RADIUS, TurnMode, Turning, TurningError,
};
pub use units::{Footprint, UnitClass, UnitStats, UnitTable, UnitTableError};
pub use weapons::{
    ALL_ROUND, Impact, Loadout, MAX_WEAPON_SLOTS, Mount, ProjectileKind, Shot, WeaponStats,
    WeaponTable, WeaponsError,
};
pub use world::{
    ConfigError, DEFAULT_MAP_SIDE, DEFAULT_SPEED, DEFAULT_TURN_PER_TICK, MAX_ENTITIES,
    MAX_MAP_SIDE, StepReport, World, WorldConfig,
};
