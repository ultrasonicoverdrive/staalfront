//! The scenario file: everything a headless run starts from.
//!
//! A scenario is a RON file (see the crate documentation for the format and an
//! example). It is data only: a world seed, a map size, a run length, groups of
//! entities to spawn and an optional list of timed commands. [`Scenario::build_world`]
//! turns it into an `sf_sim` [`World`] with every command already scheduled, so
//! a run never feeds anything into the simulation after tick 0 except the
//! `step` calls themselves.
//!
//! # Determinism
//!
//! Loading is deterministic: the same file gives the same commands in the same
//! order on every machine. The only randomness is the *placement stream*, a
//! PCG32 stream seeded with the world seed (see [`PLACEMENT_STREAM`]) that
//! draws the spawn positions, never the simulation's own streams. No clock, no
//! environment, no file other than the scenario itself is read.
//!
//! # Scheduling order
//!
//! 1. every spawn group, in file order, entity by entity;
//! 2. every timed command, in file order (a `Despawn` with several ids becomes
//!    one `sf_sim` command per id).
//!
//! `sf_sim` runs the commands of one tick by `(player, scheduling order)`, so
//! the file order matters only between commands of the same tick and player.
//! Everything is scheduled before tick 0, as a replay would be; pending
//! commands are part of the checksum, so this is also what the checksums of a
//! scenario mean.
//!
//! # Entity ids
//!
//! In a fresh world the `n`-th `Spawn` command that runs gets the id
//! `{ n, 0 }` as long as nothing was despawned before it. Commands name
//! entities by that id ([`IdSpec`]); an id that is stale or never exists is
//! skipped by `sf_sim`, which counts the command as ignored when none of its
//! ids hit.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sf_data::map::Map;
use sf_data::match_def::MatchDef;
use sf_data::movement::MovementCosts;
use sf_data::terrain::TerrainTypes;
use sf_data::units::{Faction, UnitDefs};
use sf_math::{Fx, Rng, Vec2};
use sf_sim::{
    Area, BuildingTable, Cell, Command, DEFAULT_MAP_SIDE, EntityId, GuardTarget, Kind,
    MAX_ENTITIES, MAX_MAP_SIDE, MovementTable, Order, PlayerId, ProductionTable, Terrain, Tick,
    UnitTable, WeaponTable, World, WorldConfig,
};

/// The stream number of the placement generator: spawn positions come from
/// `Rng::with_stream(seed, PLACEMENT_STREAM)`, one `x` then one `y` per entity,
/// across all spawn groups in file order.
///
/// The value is the one `crates/sf_sim/tests/common/mod.rs` uses for its
/// golden script, on purpose: a spawn group over the whole 256 by 256 map with
/// kinds `[0, 1, 2]` and owners `[0, 1, 2, 3]` and the seed of the golden
/// scenario is *the same crowd* as the sf_sim golden test (`tests/sim_crowd.rs`
/// proves it). Changing it changes the checksum of every scenario.
pub const PLACEMENT_STREAM: u64 = 0x5C21;

/// A parsed, validated scenario file.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    /// Name used in logs. Empty means "the file name without extension"
    /// ([`Scenario::load`]) or "unnamed" ([`Scenario::from_ron_str`]).
    #[serde(default)]
    pub name: String,
    /// Seeds the world and the placement stream.
    pub seed: u64,
    /// The map size in cells; 256 by 256 when left out.
    #[serde(default)]
    pub map: MapSize,
    /// The name of a map directory whose terrain layer the world is made on:
    /// `assets/maps/<name>/`, found next to the scenario file (the scenario
    /// is `assets/scenarios/<file>.ron`). The map's size in gameplay cells
    /// must equal `map`. Left out: no terrain layer, every cell is open ground.
    /// Only [`Scenario::load`] can read the map; see [`Scenario::terrain`].
    #[serde(default)]
    pub map_name: Option<String>,
    /// The terrain read from `map_name` by [`Scenario::load`], handed to the
    /// world by [`Scenario::build_world`]. Not part of the file.
    #[serde(skip)]
    pub terrain: Option<Terrain>,
    /// The unit definitions the world is made with, as the name of a directory
    /// `assets/scenarios/<name>/` (next to the scenario file) holding
    /// `unit_classes.ron`, `weapon_defs.ron` and `unit_defs.ron` (WP3.2). The
    /// definitions become the world's unit table, which `SpawnUnit` and
    /// `PlaceStructure` need. A scenario that is pinned by a checksum names a
    /// **frozen copy** of the files, so that a balance change in
    /// `assets/config/` does not move its checksum. Read by
    /// [`Scenario::load`] only, see [`Scenario::unit_table`]. The legacy `Spawn`
    /// groups and `Place` ignore the table.
    #[serde(default)]
    pub unit_defs: Option<String>,
    /// The unit table read from `unit_defs` by [`Scenario::load`], handed to the
    /// world by [`Scenario::build_world`]. Not part of the file.
    #[serde(skip)]
    pub unit_table: Option<UnitTable>,
    /// The terrain costs of the movement classes in the same definitions
    /// (WP4.1, `unit_classes.ron` of the `unit_defs` directory), handed to the
    /// world with the unit table. Not part of the file. A scenario without
    /// `unit_defs` has none: its units, if any, walk on open ground. A frozen
    /// copy of the definitions freezes the costs too.
    #[serde(skip)]
    pub movement_table: Option<MovementTable>,
    /// Whether the world is given the building rules of the `unit_defs` directory (WP6.2,
    /// `docs/rules/buildings.md`): the building table and, with `factions`, who is which
    /// faction. `BuildStructure` is checked against them; without it a build is refused as
    /// "no building rules". Off when left out, so every scenario of the earlier phases is the
    /// world it was pinned with. Needs `unit_defs`.
    #[serde(default)]
    pub buildings: bool,
    /// The faction of each player (WP6.2), as `(player, Meridian | Veyran | Neutral)`;
    /// a player left out has none and builds nothing. Needs `buildings`.
    #[serde(default)]
    pub factions: Vec<(u8, Faction)>,
    /// The building table read from `unit_defs` by [`Scenario::load`] when `buildings` is on,
    /// handed to the world by [`Scenario::build_world`]. Not part of the file.
    #[serde(skip)]
    pub building_table: Option<BuildingTable>,
    /// The nodes of path search work the world may do in one tick (WP4.2,
    /// `docs/rules/pathfinding.md`); the simulation's default
    /// ([`sf_sim::DEFAULT_NODE_BUDGET`]) when left out. It decides when a unit
    /// gets its route, never which. At least [`sf_sim::MIN_NODE_BUDGET`].
    #[serde(default)]
    pub path_budget: Option<u32>,
    /// How many units of one movement class a single order must send to one
    /// place for the world to build a flowfield for them (WP4.3,
    /// `docs/rules/pathfinding.md`); the simulation's default
    /// ([`sf_sim::DEFAULT_FLOWFIELD_THRESHOLD`], the number of `sim.ron`) when
    /// left out. `Some(0)` turns flowfields off: every unit is routed on its
    /// own. A scenario that is about the queue of single requests says so.
    #[serde(default)]
    pub flowfield_threshold: Option<u32>,
    /// Whether bodies collide (WP4.4, `docs/rules/collision.md`); the
    /// simulation's default (on, as `sim.ron` ships it) when left out.
    /// `Some(false)` lets units pass through each other, as before WP4.4: for a
    /// scenario that pins routes or terrain costs with exact positions and has
    /// units that start side by side or share a goal.
    #[serde(default)]
    pub collision: Option<bool>,
    /// Whether a group move order forms a formation (WP4.6,
    /// `docs/rules/formations.md`); the simulation's default (on, as `sim.ron` ships
    /// it) when left out. `Some(false)` moves the group as a group order did before
    /// WP4.6: for a scenario that pins routes, arrival or turning with exact positions
    /// and sends several units across the same ground to one place.
    #[serde(default)]
    pub formation: Option<bool>,
    /// The most units one move order may send and still make formations
    /// (`FormationConfig::max_units`); the simulation's default (as `sim.ron` ships it) when
    /// left out. For a probe of sizes above the shipped limit, as the sweep of
    /// `tests/formation_sweep.rs` makes to show why the limit is where it is.
    #[serde(default)]
    pub formation_max_units: Option<u16>,
    /// Whether the units fire (WP5.1, `docs/rules/weapons.md`): the world is given the
    /// weapon table of the `unit_defs` directory, and a unit with an `Attack` order shoots
    /// its victim. Off when left out, so every scenario of the earlier phases (several of
    /// them have attack orders) is the world it was pinned with. Needs `unit_defs`.
    #[serde(default)]
    pub combat: bool,
    /// Whether the units choose their own targets and fire back (WP5.4, `docs/rules/targeting.md`):
    /// the world is given the simulation's own targeting settings (the numbers of `sim.ron`). Off when
    /// left out, so every scenario of the earlier phases, and the combat scenarios of WP5.1 to WP5.3
    /// that have victims standing still, are the worlds they were pinned with. Needs `combat`.
    #[serde(default)]
    pub targeting: bool,
    /// Ore on the map (WP7.1, `docs/rules/economy.md`): the rules of the ore, written out in the
    /// scenario so that editing the shipped `assets/config/ore.ron` never moves its checksum.
    /// Every harvestable cell of the terrain then holds `start`. Left out: the world has no ore
    /// state, which is every scenario of the earlier phases. Needs a terrain layer (`map_name`)
    /// to have any ore cell.
    #[serde(default)]
    pub ore: Option<OreSpec>,
    /// Harvesters (WP7.2, `docs/rules/economy.md`): the harvest rules and the unit kinds that
    /// harvest, written out in the scenario so that editing the shipped
    /// `assets/config/harvest.ron` never moves its checksum. The refineries are the structures of
    /// the `buildings` table with the `HarvesterDock` function. Left out: no unit harvests, which
    /// is every scenario of the earlier phases. Needs `unit_defs` and `buildings`.
    #[serde(default)]
    pub harvest: Option<HarvestSpec>,
    /// Ore storage (WP7.3, `docs/rules/economy.md`, "Storage"): `(kind, units of ore)` per
    /// structure kind that stores ore, written out in the scenario so that editing the shipped
    /// `storage` of `unit_defs.ron` never moves its checksum. Left out: no limit, which is every
    /// scenario of the earlier phases (`harvest` included). Needs `harvest`.
    #[serde(default)]
    pub storage: Option<Vec<(u16, u32)>>,
    /// Money (WP7.4, `docs/rules/economy.md`, "Money"): what a unit of ore is worth and what each
    /// player starts with, written out in the scenario so that editing the shipped `money.ron`
    /// never moves its checksum. Left out: structures are built for free, which is every
    /// scenario of the earlier phases. Needs `storage` (the store is the pool).
    #[serde(default)]
    pub money: Option<MoneySpec>,
    /// Production (WP6.5, `docs/rules/production.md`): the queue limit and the exit search radius,
    /// written out in the scenario so that editing the shipped `production.ron` never moves its
    /// checksum. What each unit costs and which factory builds it come from the `unit_defs`
    /// directory (a frozen copy freezes them). Left out: the factories build nothing, which is
    /// every scenario of the earlier phases. Needs `buildings`; without `money` units are free.
    #[serde(default)]
    pub production: Option<ProductionSpec>,
    /// The production table read from `unit_defs` by [`Scenario::load`] when `production` is on,
    /// handed to the world by [`Scenario::build_world`]. Not part of the file.
    #[serde(skip)]
    pub production_table: Option<ProductionTable>,
    /// Whether what the weapons take to zero is destroyed (WP5.5, `docs/rules/destruction.md`): removed
    /// from the world with its cells freed, a vehicle leaving a wreck and a structure rubble, the kill
    /// credited. The world is given the simulation's own settings (the numbers of `sim.ron`). Off when
    /// left out, so every scenario of the earlier phases is the world it was pinned with: a unit at
    /// zero stays where it fell. Needs `combat`.
    #[serde(default)]
    pub destruction: bool,
    /// The weapon table read from `unit_defs` by [`Scenario::load`] when `combat` is on,
    /// handed to the world by [`Scenario::build_world`]. Not part of the file.
    #[serde(skip)]
    pub weapon_table: Option<WeaponTable>,
    /// A match file (WP7.6, `docs/rules/match.md`): the name of a file in the `unit_defs` directory
    /// (a frozen copy of `assets/matches/m1.ron` next to the frozen definitions, so that editing the
    /// shipped match never moves the checksum). It gives the world its match rules (who takes part,
    /// hence the outcome section `Z`), the faction and the starting credits of each player and the
    /// objects each starts with, which enter the world on tick 0 before everything else of the
    /// scenario, in the order of the file. The map is the scenario's own `map_name`. Left out: no
    /// match, which is every scenario of the earlier phases. Needs `unit_defs`, `buildings`, `combat`
    /// and `destruction`; `factions` and the `starting` list of `money` stay empty (the file has them).
    #[serde(default)]
    pub match_file: Option<String>,
    /// The match file read from `match_file` by [`Scenario::load`]. Not part of the file.
    #[serde(skip)]
    pub match_def: Option<MatchDef>,
    /// The commands that make the starting objects of `match_def` (`sf_bridge::match_commands`), read
    /// by [`Scenario::load`], scheduled for tick 0 by [`Scenario::build_world`]. Not part of the file.
    #[serde(skip)]
    pub match_commands: Vec<(PlayerId, Command)>,

    /// How many ticks to run; at least 1.
    pub ticks: u64,
    /// Groups of entities to create.
    #[serde(default)]
    pub spawns: Vec<SpawnGroup>,
    /// Commands for later ticks.
    #[serde(default)]
    pub commands: Vec<TimedCommand>,
}

/// The ore rules of a scenario (`sf_sim::OreRules`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OreSpec {
    /// What every ore cell holds at the start.
    pub start: u16,
    /// The most a cell can hold.
    pub max: u16,
    /// What a cell below the maximum gains in its turn.
    pub regrow_amount: u16,
    /// Ticks between two turns of a cell.
    pub regrow_period: u32,
}

/// The production rules of a scenario (`sf_sim::ProductionRules`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionSpec {
    /// Most items in one factory's queue, the one being built included.
    pub queue_limit: u8,
    /// Cells round the exit cell in which a finished unit looks for a free spot.
    pub exit_radius: u8,
}

impl ProductionSpec {
    /// The simulation's rules for these numbers.
    pub fn rules(self) -> sf_sim::ProductionRules {
        sf_sim::ProductionRules {
            queue_limit: self.queue_limit,
            exit_radius: self.exit_radius,
        }
    }
}

/// The money rules of a scenario (`sf_sim::MoneyRules`) and the credits each player starts with.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoneySpec {
    /// Credits one unit of ore is worth. Left out: 1.
    #[serde(default = "one")]
    pub credits_per_ore: u32,
    /// `(player, credits)` at the start. A player not listed starts with nothing.
    #[serde(default)]
    pub starting: Vec<(u8, u64)>,
}

fn one() -> u32 {
    1
}

/// The harvest rules of a scenario (`sf_sim::HarvestRules`) and the kinds that harvest.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarvestSpec {
    /// The unit kinds that harvest (permanent kind numbers of the `unit_defs`), not empty.
    pub harvesters: Vec<u16>,
    /// Units of ore a harvester carries.
    pub capacity: u16,
    /// Ore taken in one bite.
    pub mine_amount: u16,
    /// Ticks between two bites.
    pub mine_period: u16,
    /// Ore the refinery takes off a harvester in one bite.
    pub unload_amount: u16,
    /// Ticks between two unloading bites.
    pub unload_period: u16,
    /// How far, in cells, a harvester looks for ore.
    pub search_radius: u16,
    /// Ticks between two looks of a harvester that found nothing.
    pub retry_period: u16,
    /// Cells from the refinery at which a harvester asks for a place.
    pub call_range: u16,
    /// Ticks a granted harvester has to arrive.
    pub grant_timeout: u16,
    /// Ticks a harvester waits for the refinery's answer before it asks again.
    pub call_timeout: u16,
    /// Walks to one goal that end short of it before the harvester gives that goal up.
    pub max_tries: u8,
    /// Cells from the dock point, along the exit side, where a harvester without ore waits.
    pub park_distance: u16,
    /// Lateral spread of the parking spots, in cells.
    pub park_spread: u16,
    /// Tenths of a cell from its parking spot within which a parked harvester stays.
    pub park_near_tenths: u16,
    /// Tenths of a cell from the ore cell's middle at which a harvester stops and mines.
    pub mine_start_tenths: u16,
    /// Tenths of a cell from the ore cell's middle that count as there when a walk is over.
    pub mine_reach_tenths: u16,
    /// Tenths of a cell from the ore cell's middle beyond which a mining harvester drives back.
    pub mine_lose_tenths: u16,
    /// Tenths of a cell from the dock point that count as docked when a walk is over.
    pub dock_reach_tenths: u16,
    /// Choice of refinery: ticks one cell of straight-line distance to the dock point is worth.
    pub travel_cost: u16,
    /// Ticks every harvester already committed to a refinery adds to the expected wait there.
    pub queue_cost: u16,
    /// Ticks a refinery has to be cheaper than the one a driving harvester is bound for before it switches.
    pub switch_margin: u16,
    /// Ticks between two looks of a driving harvester at the other refineries.
    pub recheck_period: u16,
    /// Cells beside the approach where queued harvesters wait; 0 leaves them where they stand.
    pub queue_gap: u16,
    /// Cells out from the dock point of the first row of queue spots.
    pub queue_start: u16,
    /// Cells between two rows of queue spots.
    pub queue_step: u16,
    /// Rows of queue spots (two a row); a longer queue shares the last row.
    pub queue_rows: u16,
}

impl HarvestSpec {
    fn rules(&self) -> sf_sim::HarvestRules {
        sf_sim::HarvestRules {
            capacity: self.capacity,
            mine_amount: self.mine_amount,
            mine_period: self.mine_period,
            unload_amount: self.unload_amount,
            unload_period: self.unload_period,
            search_radius: self.search_radius,
            retry_period: self.retry_period,
            call_range: self.call_range,
            grant_timeout: self.grant_timeout,
            call_timeout: self.call_timeout,
            max_tries: self.max_tries,
            park_distance: self.park_distance,
            park_spread: self.park_spread,
            park_near_tenths: self.park_near_tenths,
            mine_start_tenths: self.mine_start_tenths,
            mine_reach_tenths: self.mine_reach_tenths,
            mine_lose_tenths: self.mine_lose_tenths,
            dock_reach_tenths: self.dock_reach_tenths,
            travel_cost: self.travel_cost,
            queue_cost: self.queue_cost,
            switch_margin: self.switch_margin,
            recheck_period: self.recheck_period,
            queue_gap: self.queue_gap,
            queue_start: self.queue_start,
            queue_step: self.queue_step,
            queue_rows: self.queue_rows,
        }
    }
}

impl OreSpec {
    fn rules(self) -> sf_sim::OreRules {
        sf_sim::OreRules {
            start: self.start,
            max: self.max,
            regrow_amount: self.regrow_amount,
            regrow_period: self.regrow_period,
        }
    }
}

/// The map size in cells.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapSize {
    /// Width in cells, `1..=1024`.
    #[serde(default = "default_side")]
    pub width: u32,
    /// Height in cells, `1..=1024`.
    #[serde(default = "default_side")]
    pub height: u32,
}

fn default_side() -> u32 {
    DEFAULT_MAP_SIDE
}

impl Default for MapSize {
    fn default() -> MapSize {
        MapSize {
            width: DEFAULT_MAP_SIDE,
            height: DEFAULT_MAP_SIDE,
        }
    }
}

/// A rectangle of the map in whole cells: `min` inclusive, `max` exclusive.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    /// The corner with the smallest coordinates, `(x, y)`.
    pub min: (i32, i32),
    /// The corner with the largest coordinates, `(x, y)`.
    pub max: (i32, i32),
}

impl Rect {
    fn to_area(self) -> Area {
        Area::from_cells(self.min.0, self.min.1, self.max.0, self.max.1)
    }

    fn describe(self) -> String {
        format!(
            "({}, {})-({}, {})",
            self.min.0, self.min.1, self.max.0, self.max.1
        )
    }
}

/// `count` entities created at `tick`, at random positions inside `area`.
///
/// Entity number `i` of the group (counting from 0) gets kind
/// `kinds[i % kinds.len()]` and owner `owners[i % owners.len()]`, so
/// `kinds: [0, 1, 2]` splits the group into three equal kinds.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnGroup {
    /// The tick the spawns run at; 0 when left out.
    #[serde(default)]
    pub tick: u64,
    /// The player the spawn commands are issued by (decides their order
    /// within a tick); 0 when left out.
    #[serde(default)]
    pub player: u8,
    /// How many entities, at least 1.
    pub count: u32,
    /// The kinds to cycle through, not empty.
    pub kinds: Vec<u16>,
    /// The owners to cycle through, not empty.
    pub owners: Vec<u8>,
    /// Where they appear, inside the map.
    pub area: Rect,
}

/// A command for a later tick.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimedCommand {
    /// The tick it runs at, before the last tick of the scenario.
    pub tick: u64,
    /// The player who issues it; 0 when left out.
    #[serde(default)]
    pub player: u8,
    /// What it does.
    pub command: Action,
}

/// What a timed command does. These are the `sf_sim` commands apart from
/// `Spawn`, which is what [`SpawnGroup`] is for.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Action {
    /// `Command::SetWanderArea`: wander inside `area`.
    SetWanderArea {
        /// The entities.
        ids: Vec<IdSpec>,
        /// Where they may wander, inside the map.
        area: Rect,
    },
    /// `Command::MoveTo`: walk to `target` and stand there.
    MoveTo {
        /// The entities.
        ids: Vec<IdSpec>,
        /// The target cell `(x, y)`, inside the map.
        target: (i32, i32),
    },
    /// `Command::Order` with `Order::Move`: go to the middle of `target` (WP3.4).
    /// With `queued` the order is added behind the unit's others.
    OrderMove {
        /// The units.
        ids: Vec<IdSpec>,
        /// The cell `(x, y)`, inside the map.
        target: (i32, i32),
        /// Append to the queue instead of replacing the orders.
        #[serde(default)]
        queued: bool,
    },
    /// `Command::Order` with `Order::AttackMove`: go to the middle of `target`,
    /// engaging on the way (the engaging is phase 5).
    OrderAttackMove {
        /// The units.
        ids: Vec<IdSpec>,
        /// The cell `(x, y)`, inside the map.
        target: (i32, i32),
        /// Append to the queue instead of replacing the orders.
        #[serde(default)]
        queued: bool,
    },
    /// `Command::Order` with `Order::Attack`: go for the entity `target` until
    /// it is in weapon reach.
    OrderAttack {
        /// The units.
        ids: Vec<IdSpec>,
        /// The target, as `(index, generation)`.
        target: (u32, u32),
        /// Append to the queue instead of replacing the orders.
        #[serde(default)]
        queued: bool,
    },
    /// `Command::Order` with `Order::Guard` of a spot: go to the middle of
    /// `target` and stay.
    OrderGuardPoint {
        /// The units.
        ids: Vec<IdSpec>,
        /// The cell `(x, y)`, inside the map.
        target: (i32, i32),
        /// Append to the queue instead of replacing the orders.
        #[serde(default)]
        queued: bool,
    },
    /// `Command::Order` with `Order::Guard` of an entity: follow it and stay
    /// near it.
    OrderGuardEntity {
        /// The units.
        ids: Vec<IdSpec>,
        /// The watched entity, as `(index, generation)`.
        target: (u32, u32),
        /// Append to the queue instead of replacing the orders.
        #[serde(default)]
        queued: bool,
    },
    /// `Command::Order` with `Order::Scatter`: a short step in a deterministic
    /// direction.
    OrderScatter {
        /// The units.
        ids: Vec<IdSpec>,
        /// Append to the queue instead of replacing the orders.
        #[serde(default)]
        queued: bool,
    },
    /// `Command::Order` with `Order::HoldPosition`: stand still on purpose.
    OrderHold {
        /// The units.
        ids: Vec<IdSpec>,
        /// Append to the queue instead of replacing the orders.
        #[serde(default)]
        queued: bool,
    },
    /// `Command::Stop`: clear the order and the queue; the units stand still.
    Stop {
        /// The units.
        ids: Vec<IdSpec>,
    },
    /// `Command::AimTurret` (WP4.5): the turrets of the units follow `target`,
    /// an entity as `(index, generation)`, or, with no target, go back to
    /// hull-forward. The hull and the order are not touched.
    AimTurret {
        /// The units.
        ids: Vec<IdSpec>,
        /// What the turrets aim at; none lets them return to the hull.
        #[serde(default)]
        target: Option<(u32, u32)>,
    },
    /// `Command::TakeOre` (WP7.1): take up to `amount` of ore from `cell`, as a harvester will.
    /// Ignored by the simulation without `ore`, on an empty cell and off a field.
    TakeOre {
        /// The cell `(x, y)`, inside the map.
        cell: (u32, u32),
        /// The most to take.
        amount: u16,
    },
    /// `Command::Despawn` for each id.
    Despawn {
        /// The entities.
        ids: Vec<IdSpec>,
    },
    /// `Command::SpawnUnit`: a unit or object of the unit table, standing in
    /// the middle of `cell`. Needs `unit_defs`. Refused by the
    /// simulation (the command counts as ignored) for an unknown kind, for a
    /// structure, and on a cell a structure occupies. The entity takes the next
    /// free id, like a spawn.
    SpawnUnit {
        /// The permanent kind number of the role.
        kind: u16,
        /// Who owns it.
        owner: u8,
        /// The cell `(x, y)`, inside the map.
        cell: (u32, u32),
    },
    /// `Command::PlaceStructure`: a structure of the unit table with its
    /// footprint anchored on `cell` (its smallest x and y). Needs
    /// `unit_defs`. Refused by the simulation unless the kind is a
    /// structure and every cell of the footprint is on the map, buildable and
    /// free.
    PlaceStructure {
        /// The permanent kind number of the role.
        kind: u16,
        /// Who owns it.
        owner: u8,
        /// The anchor cell `(x, y)`, inside the map.
        cell: (u32, u32),
    },
    /// `Command::BuildStructure` (WP6.2): the player who sends the command (`player` of the
    /// timed command) builds a structure of `kind` with its footprint anchored on `cell`.
    /// Needs `buildings`. Unlike `PlaceStructure` it is checked against the placement rules
    /// (terrain, overlap, base radius, prerequisites, faction, exit); a refusal counts as
    /// ignored and is listed in the run's rejections.
    BuildStructure {
        /// The permanent kind number of the role.
        kind: u16,
        /// The anchor cell `(x, y)`, inside the map.
        cell: (u32, u32),
    },
    /// `Command::QueueUnit` (WP6.5): the player who sends the command queues a unit of `kind`
    /// at the factory `factory`, an entity as `(index, generation)`. Needs `production`.
    QueueUnit {
        /// The factory.
        factory: (u32, u32),
        /// The permanent kind number of the unit.
        kind: u16,
    },
    /// `Command::CancelUnit` (WP6.5): take the item at place `index` (0 is the one being built)
    /// out of the queue of `factory` and give back what was paid. Needs `production`.
    CancelUnit {
        /// The factory.
        factory: (u32, u32),
        /// The place in the queue.
        index: u8,
    },
    /// `Command::SetRally` (WP6.5): where the units of `factory` walk to; none clears it.
    /// Needs `production`.
    SetRally {
        /// The factory.
        factory: (u32, u32),
        /// The rally cell `(x, y)`, inside the map.
        #[serde(default)]
        cell: Option<(u32, u32)>,
    },
    /// `Command::Place`: a standing structure on a cell. Refused by the
    /// simulation (the command counts as ignored) on terrain that is not
    /// buildable and on an occupied cell. The entity takes the next free id,
    /// like a spawn.
    Place {
        /// What to create.
        kind: u16,
        /// Who owns it.
        owner: u8,
        /// The cell `(x, y)`, inside the map.
        cell: (u32, u32),
    },
}

/// Names entities by their `sf_sim` id.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum IdSpec {
    /// One id: slot index and generation.
    Id(u32, u32),
    /// The slots `from..to` (end exclusive) at generation 0, which in a world
    /// without despawns are the entities spawned `from`-th up to `to`-th.
    Range(u32, u32),
}

impl IdSpec {
    fn push_ids(self, out: &mut Vec<EntityId>) {
        match self {
            IdSpec::Id(index, generation) => out.push(EntityId::new(index, generation)),
            IdSpec::Range(from, to) => out.extend((from..to).map(|i| EntityId::new(i, 0))),
        }
    }
}

/// The simulation's table of terrain types from the data table: the ids and
/// the flags, no names. The `ground` summary is derived from `costs`. (`sf_game`
/// does the same when it starts a map; the data crate cannot name the
/// simulation's types, so `sf_bridge` converts for both.)
pub fn sim_definitions(types: &TerrainTypes, costs: &MovementCosts) -> Vec<sf_sim::TerrainDef> {
    sf_bridge::terrain_defs(types, costs)
}

/// Reads the unit definitions in `<scenario directory>/<name>/` (the model and
/// icon files they name are looked up under `<scenario directory>/..`, the
/// assets directory) and turns them into the simulation's unit table and
/// movement table, with the movement costs they came from. The weapon table is
/// built only for a scenario with `combat` (it is the only one that hands it to
/// a world), so a frozen copy that predates a rule the table has to meet (WP5.2:
/// a projectile that flies needs a speed) still loads for the scenarios that
/// never fire.
fn load_unit_defs(
    name: &str,
    scenario_file: &Path,
    combat: bool,
    buildings: bool,
    production: bool,
) -> Result<LoadedDefs, ScenarioError> {
    let invalid = |problem: String| ScenarioError::Invalid {
        path: Some(scenario_file.to_path_buf()),
        problems: vec![format!("unit_defs \"{name}\": {problem}")],
    };
    let scenarios = scenario_file.parent().unwrap_or(Path::new(""));
    let defs = UnitDefs::load(&scenarios.join(name), &scenarios.join(".."))
        .map_err(|e| invalid(e.to_string()))?;
    let units = sf_bridge::unit_table(&defs).map_err(invalid)?;
    let weapons = if combat {
        sf_bridge::weapon_table(&defs).map_err(invalid)?
    } else {
        WeaponTable::empty()
    };
    let costs = MovementCosts::from_defs(&defs, &TerrainTypes::embedded())
        .map_err(|e| invalid(e.to_string()))?;
    let movement = sf_bridge::movement_table(&costs).map_err(invalid)?;
    // Built only for a scenario with `buildings`, like the weapons: a frozen copy that predates
    // the block still loads for the scenarios that never build.
    let building_table = if buildings {
        Some(sf_bridge::building_table(&defs).map_err(invalid)?)
    } else {
        None
    };
    // Built only for a scenario with `production`, for the same reason.
    let production_table = if production {
        Some(sf_bridge::production_table(&defs).map_err(invalid)?)
    } else {
        None
    };
    Ok(LoadedDefs {
        units,
        weapons,
        movement,
        costs,
        buildings: building_table,
        production: production_table,
    })
}

/// What [`load_unit_defs`] reads: the unit, weapon and movement tables, the movement costs they
/// came from, the building table of a scenario with `buildings` and the production table of one
/// with `production`.
struct LoadedDefs {
    units: UnitTable,
    weapons: WeaponTable,
    movement: MovementTable,
    costs: MovementCosts,
    buildings: Option<BuildingTable>,
    production: Option<ProductionTable>,
}

/// The middle of a cell, in cells: where an order sends a unit.
fn cell_middle(cell: (i32, i32)) -> Vec2 {
    Vec2::new(
        Fx::from_int(cell.0) + Fx::HALF,
        Fx::from_int(cell.1) + Fx::HALF,
    )
}

fn expand(ids: &[IdSpec]) -> Vec<EntityId> {
    let mut out = Vec::new();
    for spec in ids {
        spec.push_ids(&mut out);
    }
    out
}

/// Why a scenario could not be loaded or run.
#[derive(Debug)]
pub enum ScenarioError {
    /// The file could not be read.
    Io {
        /// The file.
        path: PathBuf,
        /// What the operating system said.
        source: io::Error,
    },
    /// The text is not valid RON for a scenario: syntax, an unknown field or
    /// variant, a wrong type. `message` carries line and column.
    Syntax {
        /// The file, when the text came from one.
        path: Option<PathBuf>,
        /// The parser's message.
        message: String,
    },
    /// The file parsed but describes something that cannot run. Lists every
    /// problem found, not just the first.
    Invalid {
        /// The file, when the text came from one.
        path: Option<PathBuf>,
        /// One line per problem.
        problems: Vec<String>,
    },
    /// `sf_sim` refused the world or a command.
    Sim(String),
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn at(f: &mut fmt::Formatter<'_>, path: &Option<PathBuf>) -> fmt::Result {
            match path {
                Some(p) => write!(f, "{}: ", p.display()),
                None => Ok(()),
            }
        }
        match self {
            ScenarioError::Io { path, source } => {
                write!(f, "cannot read {}: {source}", path.display())
            }
            ScenarioError::Syntax { path, message } => {
                at(f, path)?;
                write!(f, "invalid scenario: {message}")
            }
            ScenarioError::Invalid { path, problems } => {
                at(f, path)?;
                write!(f, "invalid scenario:")?;
                for p in problems {
                    write!(f, "\n  - {p}")?;
                }
                Ok(())
            }
            ScenarioError::Sim(message) => {
                write!(f, "the simulation refused the scenario: {message}")
            }
        }
    }
}

impl std::error::Error for ScenarioError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ScenarioError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl Scenario {
    /// Reads and validates a scenario file. A scenario without a `name` is
    /// named after the file.
    ///
    /// The terrain's `ground` summary (part of its digest) is derived from the
    /// movement costs of the scenario's own definitions: the frozen copy named
    /// by `unit_defs`, or, for a scenario without one, the shipped costs. So
    /// editing the shipped `unit_classes.ron` moves only scenarios that run on
    /// the shipped definitions.
    pub fn load(path: &Path) -> Result<Scenario, ScenarioError> {
        Scenario::load_with(path, None)
    }

    /// [`Scenario::load`] with the costs standing in for the shipped ones
    /// (`None`: the real shipped costs). It exists to test that a change to the
    /// shipped costs leaves the scenarios with frozen definitions alone.
    pub fn load_with_shipped_costs(
        path: &Path,
        shipped: &MovementCosts,
    ) -> Result<Scenario, ScenarioError> {
        Scenario::load_with(path, Some(shipped))
    }

    fn load_with(path: &Path, shipped: Option<&MovementCosts>) -> Result<Scenario, ScenarioError> {
        let text = std::fs::read_to_string(path).map_err(|source| ScenarioError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut scenario = Scenario::parse(&text, Some(path))?;
        if scenario.name.is_empty()
            && let Some(stem) = path.file_stem()
        {
            scenario.name = stem.to_string_lossy().into_owned();
        }
        // The unit definitions first: their costs decide the terrain's summary.
        let mut own_costs = None;
        if let Some(name) = scenario.unit_defs.clone() {
            let loaded = load_unit_defs(
                &name,
                path,
                scenario.combat,
                scenario.buildings,
                scenario.production.is_some(),
            )?;
            scenario.unit_table = Some(loaded.units);
            scenario.building_table = loaded.buildings;
            scenario.production_table = loaded.production;
            scenario.weapon_table = scenario.combat.then_some(loaded.weapons);
            scenario.movement_table = Some(loaded.movement);
            own_costs = Some(loaded.costs);
        }
        if let (Some(dir), Some(file)) = (scenario.unit_defs.clone(), scenario.match_file.clone()) {
            scenario.load_match(&dir, &file, path)?;
        }
        if let Some(name) = scenario.map_name.clone() {
            let costs = match (own_costs, shipped) {
                (Some(own), _) => own,
                (None, Some(shipped)) => shipped.clone(),
                (None, None) => MovementCosts::embedded(),
            };
            scenario.terrain = Some(scenario.load_terrain(&name, path, &costs)?);
        }
        Ok(scenario)
    }

    /// Reads the match file `file` of the `unit_defs` directory `dir`, checks it against that
    /// directory's definitions and the scenario's map size, and makes the commands of its starting
    /// objects.
    fn load_match(&mut self, dir: &str, file: &str, scenario_file: &Path) -> Result<(), ScenarioError> {
        let invalid = |problem: String| ScenarioError::Invalid {
            path: Some(scenario_file.to_path_buf()),
            problems: vec![format!("match_file \"{file}\": {problem}")],
        };
        let scenarios = scenario_file.parent().unwrap_or(Path::new(""));
        let text = std::fs::read_to_string(scenarios.join(dir).join(file))
            .map_err(|e| invalid(e.to_string()))?;
        let def = MatchDef::from_ron(&text).map_err(|e| invalid(e.to_string()))?;
        let defs = UnitDefs::load(&scenarios.join(dir), &scenarios.join(".."))
            .map_err(|e| invalid(e.to_string()))?;
        def.check(&defs, (self.map.width, self.map.height))
            .map_err(|e| invalid(e.to_string()))?;
        self.match_commands = sf_bridge::match_commands(&def, &defs).map_err(invalid)?;
        self.match_def = Some(def);
        Ok(())
    }

    /// Reads the terrain layer of map `name`, which lives in
    /// `<scenario directory>/../maps/<name>`, checks that it is the scenario's
    /// map size and turns it into the simulation's [`Terrain`].
    fn load_terrain(
        &self,
        name: &str,
        scenario_file: &Path,
        costs: &MovementCosts,
    ) -> Result<Terrain, ScenarioError> {
        let invalid = |problem: String| ScenarioError::Invalid {
            path: Some(scenario_file.to_path_buf()),
            problems: vec![format!("map_name \"{name}\": {problem}")],
        };
        let dir = scenario_file
            .parent()
            .unwrap_or(Path::new(""))
            .join("..")
            .join("maps")
            .join(name);
        let map = Map::load(&dir).map_err(|e| invalid(e.to_string()))?;
        let layer = map
            .terrain()
            .ok_or_else(|| invalid("the map has no terrain layer".into()))?;
        let (width, height) = (self.map.width, self.map.height);
        if (layer.width(), layer.height()) != (width, height) {
            return Err(invalid(format!(
                "the map is {}x{} cells but the scenario's map is {width}x{height}",
                layer.width(),
                layer.height()
            )));
        }
        // The `ground` summary of the terrain types is part of the terrain
        // digest: it comes from the scenario's own costs (see `Scenario::load`).
        let defs = sim_definitions(&TerrainTypes::embedded(), costs);
        Terrain::from_layer(width, height, layer.ids(), &defs).map_err(|e| invalid(e.to_string()))
    }

    /// Parses and validates scenario text.
    pub fn from_ron_str(text: &str) -> Result<Scenario, ScenarioError> {
        Scenario::parse(text, None)
    }

    fn parse(text: &str, path: Option<&Path>) -> Result<Scenario, ScenarioError> {
        let path = path.map(Path::to_path_buf);
        let scenario: Scenario = ron::from_str(text).map_err(|e| ScenarioError::Syntax {
            path: path.clone(),
            message: e.to_string(),
        })?;
        let problems = scenario.problems();
        if problems.is_empty() {
            Ok(scenario)
        } else {
            Err(ScenarioError::Invalid { path, problems })
        }
    }

    /// The name for logs: the scenario's own, or "unnamed".
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            "unnamed"
        } else {
            &self.name
        }
    }

    /// Every reason this scenario cannot run, one line each; empty when it can.
    ///
    /// Rules: `ticks` is at least 1; the map sides are in `1..=1024`; every
    /// spawn group has a count, kinds, owners and a non-empty area inside the
    /// map, and all groups together stay under the entity limit of `sf_sim`;
    /// every tick (of a spawn group or a command) is before the last tick of
    /// the run, otherwise it would never execute; command areas are non-empty
    /// and inside the map, targets inside the map, id lists non-empty with ids
    /// below the entity limit and ranges that are not empty. (A tick in the
    /// past cannot be written: the world starts at tick 0 and everything is
    /// scheduled before it.)
    pub fn problems(&self) -> Vec<String> {
        let mut p = Vec::new();
        let MapSize { width, height } = self.map;
        let map_ok = (1..=MAX_MAP_SIDE).contains(&width) && (1..=MAX_MAP_SIDE).contains(&height);
        if !map_ok {
            p.push(format!(
                "map: width and height must be between 1 and {MAX_MAP_SIDE}, got {width}x{height}"
            ));
        }
        if self.ticks == 0 {
            p.push("ticks must be at least 1".into());
        }
        if let Some(name) = &self.map_name {
            // The same file name rules as the files of a map (no path out of
            // the maps directory, no Windows device names), and a single name.
            if let Err(e) = sf_data::map::check_file_name("map_name", name) {
                p.push(e.to_string());
            } else if name.contains('/') {
                p.push(format!(
                    "map_name \"{name}\" must be the name of one map directory, without '/'"
                ));
            }
        }

        if let Some(name) = &self.unit_defs {
            // A single directory name next to the scenario, like `map_name`.
            if let Err(e) = sf_data::map::check_file_name("unit_defs", name) {
                p.push(e.to_string());
            } else if name.contains('/') {
                p.push(format!(
                    "unit_defs \"{name}\" must be the name of one directory, without '/'"
                ));
            }
        }

        if self.combat && self.unit_defs.is_none() {
            p.push(
                "combat needs `unit_defs` (the weapons are part of the definitions)".to_string(),
            );
        }

        if self.buildings && self.unit_defs.is_none() {
            p.push(
                "buildings needs `unit_defs` (the rules are part of the definitions)".to_string(),
            );
        }

        if let Some(h) = &self.harvest {
            if self.unit_defs.is_none() || !self.buildings {
                p.push(
                    "harvest needs `unit_defs` and `buildings` (the refineries are structures of \
                     the building table)"
                        .to_string(),
                );
            }
            if h.harvesters.is_empty() {
                p.push("harvest: `harvesters` must name at least one unit kind".to_string());
            }
            if !h.rules().is_valid() {
                p.push(
                    "harvest: every number must be at least 1, mine_amount at most capacity and the tuning \
                     numbers within their bounds (docs/rules/economy.md)"
                        .to_string(),
                );
            }
        }

        if let Some(st) = &self.storage {
            if self.harvest.is_none() {
                p.push("storage needs `harvest` (only refineries fill a store)".to_string());
            }
            if st.iter().any(|&(_, n)| n == 0) {
                p.push(
                    "storage: every capacity must be above zero (leave the kind out)".to_string(),
                );
            }
        }

        if let Some(m) = &self.money {
            if self.storage.is_none() {
                p.push("money needs `storage` (the store is the money pool)".to_string());
            }
            if m.credits_per_ore == 0 {
                p.push("money: credits_per_ore must be above zero".to_string());
            }
        }

        if let Some(pr) = &self.production {
            if !self.buildings {
                p.push("production needs `buildings` (the factories are structures)".to_string());
            }
            if !pr.rules().is_valid() {
                p.push(
                    "production: queue_limit must be 1 to 20 and exit_radius 1 to 8".to_string(),
                );
            }
        }

        if !self.factions.is_empty() && !self.buildings {
            p.push(
                "factions needs `buildings` (they are checked when a player builds)".to_string(),
            );
        }

        let mut seen = Vec::new();
        for (player, faction) in &self.factions {
            if seen.contains(player) {
                p.push(format!("factions: player {player} is listed twice"));
            }
            seen.push(*player);
            if *faction == Faction::Neutral {
                p.push(format!(
                    "factions: player {player} cannot be Neutral (nobody plays it)"
                ));
            }
        }

        if let Some(file) = &self.match_file {
            if let Err(e) = sf_data::map::check_file_name("match_file", file) {
                p.push(e.to_string());
            } else if file.contains('/') {
                p.push(format!(
                    "match_file \"{file}\" must be the name of one file in the unit_defs directory, without '/'"
                ));
            }
            if self.unit_defs.is_none()
                || !self.buildings
                || !self.combat
                || !self.destruction
            {
                p.push(
                    "match_file needs `unit_defs`, `buildings`, `combat` and `destruction` (a match is \
                     decided by what is destroyed)"
                        .to_string(),
                );
            }
            if !self.factions.is_empty() {
                p.push("match_file gives the factions: `factions` must be left out".to_string());
            }
            if self.money.as_ref().is_some_and(|m| !m.starting.is_empty()) {
                p.push(
                    "match_file gives the starting credits: `money.starting` must be left empty"
                        .to_string(),
                );
            }
        }

        if self.targeting && !self.combat {
            p.push("targeting needs `combat` (a unit picks targets for its weapons)".to_string());
        }

        if self.destruction && !self.combat {
            p.push("destruction needs `combat` (nothing is destroyed without weapons)".to_string());
        }

        if self.flowfield_threshold == Some(1) {
            p.push(
                "flowfield_threshold 1 is not a group (use 0 to turn flowfields off, or 2 and up)"
                    .to_string(),
            );
        }

        if let Some(budget) = self.path_budget
            && budget < sf_sim::MIN_NODE_BUDGET
        {
            p.push(format!(
                "path_budget {budget} is below the minimum of {}",
                sf_sim::MIN_NODE_BUDGET
            ));
        }

        // A tick at or after `ticks` never runs. Not checked when `ticks` is
        // itself wrong (everything would be reported).
        let tick_problem = |at: &str, tick: u64| -> Option<String> {
            (self.ticks > 0 && tick >= self.ticks).then(|| {
                format!(
                    "{at}: tick {tick} is after the last tick ({}); it would never run",
                    self.ticks - 1
                )
            })
        };
        let area_problems = |at: &str, what: &str, r: Rect, out: &mut Vec<String>| {
            if r.min.0 >= r.max.0 || r.min.1 >= r.max.1 {
                out.push(format!(
                    "{at}: {what} {} is empty or reversed (min must be below max on both axes)",
                    r.describe()
                ));
            } else if map_ok
                && (r.min.0 < 0
                    || r.min.1 < 0
                    || i64::from(r.max.0) > i64::from(width)
                    || i64::from(r.max.1) > i64::from(height))
            {
                out.push(format!(
                    "{at}: {what} {} lies outside the {width}x{height} map",
                    r.describe()
                ));
            }
        };

        let mut total: u64 = 0;
        for (i, g) in self.spawns.iter().enumerate() {
            let at = format!("spawns[{i}]");
            if g.count == 0 {
                p.push(format!("{at}: count must be at least 1"));
            }
            total += u64::from(g.count);
            if g.kinds.is_empty() {
                p.push(format!("{at}: kinds must not be empty"));
            }
            if g.owners.is_empty() {
                p.push(format!("{at}: owners must not be empty"));
            }
            p.extend(tick_problem(&at, g.tick));
            area_problems(&at, "area", g.area, &mut p);
        }
        if total > u64::from(MAX_ENTITIES) {
            p.push(format!(
                "spawns: {total} entities in total, the limit is {MAX_ENTITIES}"
            ));
        }

        for (i, c) in self.commands.iter().enumerate() {
            let at = format!("commands[{i}]");
            p.extend(tick_problem(&at, c.tick));
            match &c.command {
                Action::SetWanderArea { ids, area } => {
                    id_problems(&at, ids, &mut p);
                    area_problems(&at, "area", *area, &mut p);
                }
                Action::MoveTo { ids, target } => {
                    id_problems(&at, ids, &mut p);
                    if map_ok
                        && (target.0 < 0
                            || target.1 < 0
                            || i64::from(target.0) >= i64::from(width)
                            || i64::from(target.1) >= i64::from(height))
                    {
                        p.push(format!(
                            "{at}: target ({}, {}) lies outside the {width}x{height} map",
                            target.0, target.1
                        ));
                    }
                }
                Action::OrderMove { ids, target, .. }
                | Action::OrderAttackMove { ids, target, .. }
                | Action::OrderGuardPoint { ids, target, .. } => {
                    id_problems(&at, ids, &mut p);
                    if map_ok
                        && (target.0 < 0
                            || target.1 < 0
                            || i64::from(target.0) >= i64::from(width)
                            || i64::from(target.1) >= i64::from(height))
                    {
                        p.push(format!(
                            "{at}: target ({}, {}) lies outside the {width}x{height} map",
                            target.0, target.1
                        ));
                    }
                }
                Action::OrderAttack { ids, target, .. }
                | Action::OrderGuardEntity { ids, target, .. } => {
                    id_problems(&at, ids, &mut p);
                    if target.0 >= MAX_ENTITIES {
                        p.push(format!(
                            "{at}: target ({}, ..) is not a valid slot (the limit is \
                             {MAX_ENTITIES} slots)",
                            target.0
                        ));
                    }
                }
                Action::AimTurret { ids, target } => {
                    id_problems(&at, ids, &mut p);
                    if let Some((slot, _)) = target
                        && *slot >= MAX_ENTITIES
                    {
                        p.push(format!(
                            "{at}: target ({slot}, ..) is not a valid slot (the limit is \
                             {MAX_ENTITIES} slots)"
                        ));
                    }
                }
                Action::OrderScatter { ids, .. }
                | Action::OrderHold { ids, .. }
                | Action::Stop { ids } => {
                    id_problems(&at, ids, &mut p);
                }
                Action::Despawn { ids } => id_problems(&at, ids, &mut p),
                Action::QueueUnit { factory, .. } | Action::CancelUnit { factory, .. } => {
                    if self.production.is_none() {
                        p.push(format!("{at}: production commands need `production`"));
                    }
                    if factory.0 >= MAX_ENTITIES {
                        p.push(format!(
                            "{at}: factory ({}, ..) is not a valid slot (the limit is \
                             {MAX_ENTITIES} slots)",
                            factory.0
                        ));
                    }
                }
                Action::SetRally { factory, cell } => {
                    if self.production.is_none() {
                        p.push(format!("{at}: production commands need `production`"));
                    }
                    if factory.0 >= MAX_ENTITIES {
                        p.push(format!(
                            "{at}: factory ({}, ..) is not a valid slot (the limit is \
                             {MAX_ENTITIES} slots)",
                            factory.0
                        ));
                    }
                    if let Some(cell) = cell
                        && map_ok
                        && (cell.0 >= width || cell.1 >= height)
                    {
                        p.push(format!(
                            "{at}: cell ({}, {}) lies outside the {width}x{height} map",
                            cell.0, cell.1
                        ));
                    }
                }
                Action::TakeOre { cell, .. } => {
                    if map_ok && (cell.0 >= width || cell.1 >= height) {
                        p.push(format!(
                            "{at}: cell ({}, {}) lies outside the {width}x{height} map",
                            cell.0, cell.1
                        ));
                    }
                    if self.ore.is_none() {
                        p.push(format!("{at}: TakeOre needs `ore`"));
                    }
                }
                Action::Place { cell, .. }
                | Action::SpawnUnit { cell, .. }
                | Action::BuildStructure { cell, .. }
                | Action::PlaceStructure { cell, .. } => {
                    if map_ok && (cell.0 >= width || cell.1 >= height) {
                        p.push(format!(
                            "{at}: cell ({}, {}) lies outside the {width}x{height} map",
                            cell.0, cell.1
                        ));
                    }
                    if self.unit_defs.is_none()
                        && matches!(
                            c.command,
                            Action::SpawnUnit { .. }
                                | Action::PlaceStructure { .. }
                                | Action::BuildStructure { .. }
                        )
                    {
                        p.push(format!(
                            "{at}: SpawnUnit, PlaceStructure and BuildStructure need `unit_defs`"
                        ));
                    }
                    if !self.buildings && matches!(c.command, Action::BuildStructure { .. }) {
                        p.push(format!("{at}: BuildStructure needs `buildings`"));
                    }
                }
            }
        }
        p
    }

    /// The world this scenario starts from: created with the scenario's seed
    /// and map, every spawn and command scheduled, no tick run yet.
    ///
    /// Validates first, so a scenario edited in memory cannot run invalid.
    pub fn build_world(&self) -> Result<World, ScenarioError> {
        let problems = self.problems();
        if !problems.is_empty() {
            return Err(ScenarioError::Invalid {
                path: None,
                problems,
            });
        }
        if self.map_name.is_some() && self.terrain.is_none() {
            return Err(ScenarioError::Invalid {
                path: None,
                problems: vec![
                    "map_name: the terrain is read from the map's files, so the scenario \
                     must be loaded with Scenario::load"
                        .into(),
                ],
            });
        }
        let config = WorldConfig::new(self.seed).with_map_size(self.map.width, self.map.height);
        let mut world = match &self.terrain {
            Some(terrain) => World::with_terrain(config, terrain.clone()),
            None => World::new(config),
        }
        .map_err(|e| ScenarioError::Sim(e.to_string()))?;
        if self.unit_defs.is_some() {
            let Some(table) = self.unit_table.clone() else {
                return Err(ScenarioError::Invalid {
                    path: None,
                    problems: vec![
                        "unit_defs: the definitions are read from the files next to the \
                         scenario, so the scenario must be loaded with Scenario::load"
                            .into(),
                    ],
                });
            };
            world = world
                .with_units(table)
                .map_err(|e| ScenarioError::Sim(e.to_string()))?;
            if self.combat {
                let Some(weapons) = self.weapon_table.clone() else {
                    return Err(ScenarioError::Invalid {
                        path: None,
                        problems: vec![
                            "combat: the weapons are read from the files next to the scenario, \
                             so the scenario must be loaded with Scenario::load"
                                .into(),
                        ],
                    });
                };
                world = world
                    .with_weapons(weapons)
                    .map_err(|e| ScenarioError::Sim(e.to_string()))?;
                if self.targeting {
                    world = world
                        .with_targeting(sf_sim::TargetingConfig::DEFAULT)
                        .map_err(|e| ScenarioError::Sim(e.to_string()))?;
                }
                if self.destruction {
                    world = world
                        .with_destruction(sf_sim::DestructionConfig::DEFAULT)
                        .map_err(|e| ScenarioError::Sim(e.to_string()))?;
                }
            }
            if let Some(movement) = self.movement_table.clone() {
                world = world
                    .with_movement(movement)
                    .map_err(|e| ScenarioError::Sim(e.to_string()))?;
            }
            if self.buildings {
                let Some(table) = self.building_table.clone() else {
                    return Err(ScenarioError::Invalid {
                        path: None,
                        problems: vec![
                            "buildings: the rules are read from the files next to the scenario, \
                             so the scenario must be loaded with Scenario::load"
                                .into(),
                        ],
                    });
                };
                world = world
                    .with_buildings(table)
                    .map_err(|e| ScenarioError::Sim(e.to_string()))?;
                let players: Vec<(PlayerId, u8)> = match &self.match_def {
                    Some(def) => sf_bridge::match_factions(def),
                    None => self
                        .factions
                        .iter()
                        .map(|&(player, faction)| (PlayerId(player), faction.bit()))
                        .collect(),
                };
                world = world
                    .with_player_factions(&players)
                    .map_err(|e| ScenarioError::Sim(e.to_string()))?;
            }
        }
        if let Some(ore) = self.ore {
            world = world
                .with_ore(ore.rules())
                .map_err(|e| ScenarioError::Sim(e.to_string()))?;
        }
        if let Some(harvest) = &self.harvest {
            let kinds: Vec<Kind> = harvest.harvesters.iter().map(|&k| Kind(k)).collect();
            world = world
                .with_harvest(harvest.rules(), &kinds)
                .map_err(|e| ScenarioError::Sim(e.to_string()))?;
        }
        if let Some(storage) = &self.storage {
            let table: Vec<(Kind, u32)> = storage.iter().map(|&(k, n)| (Kind(k), n)).collect();
            world = world
                .with_storage(&table)
                .map_err(|e| ScenarioError::Sim(e.to_string()))?;
        }
        if let Some(m) = &self.money {
            let mut starting: Vec<(PlayerId, u64)> =
                m.starting.iter().map(|&(p, n)| (PlayerId(p), n)).collect();
            if let Some(def) = &self.match_def {
                starting.extend(sf_bridge::match_credits(def));
            }
            world = world
                .with_money(
                    sf_sim::MoneyRules {
                        credits_per_ore: m.credits_per_ore,
                    },
                    &starting,
                )
                .map_err(|e| ScenarioError::Sim(e.to_string()))?;
        }
        if let Some(pr) = self.production {
            let Some(table) = self.production_table.clone() else {
                return Err(ScenarioError::Invalid {
                    path: None,
                    problems: vec![
                        "production: the table is read from the files next to the scenario, \
                         so the scenario must be loaded with Scenario::load"
                            .into(),
                    ],
                });
            };
            world = world
                .with_production(pr.rules(), table)
                .map_err(|e| ScenarioError::Sim(e.to_string()))?;
        }
        if let Some(budget) = self.path_budget {
            world = world
                .with_path_budget(budget)
                .map_err(|e| ScenarioError::Sim(e.to_string()))?;
        }
        if let Some(threshold) = self.flowfield_threshold {
            world = world
                .with_flowfield_threshold(threshold)
                .map_err(|e| ScenarioError::Sim(e.to_string()))?;
        }
        if let Some(enabled) = self.collision {
            world = world
                .with_collision(sf_sim::CollisionConfig {
                    enabled,
                    ..sf_sim::CollisionConfig::DEFAULT
                })
                .map_err(|e| ScenarioError::Sim(e.to_string()))?;
        }
        if self.formation.is_some() || self.formation_max_units.is_some() {
            let base = sf_sim::FormationConfig::DEFAULT;
            world = world
                .with_formation(sf_sim::FormationConfig {
                    enabled: self.formation.unwrap_or(base.enabled),
                    max_units: self.formation_max_units.unwrap_or(base.max_units),
                    ..base
                })
                .map_err(|e| ScenarioError::Sim(e.to_string()))?;
        }
        // The match (WP7.6): its rules last, after everything they need; its starting objects first.
        if self.match_file.is_some() {
            let Some(def) = &self.match_def else {
                return Err(ScenarioError::Invalid {
                    path: None,
                    problems: vec![
                        "match_file: the match is read from the file next to the definitions, so the \
                         scenario must be loaded with Scenario::load"
                            .into(),
                    ],
                });
            };
            let rules = sf_bridge::match_rules(def).map_err(ScenarioError::Sim)?;
            world = world
                .with_match(rules)
                .map_err(|e| ScenarioError::Sim(e.to_string()))?;
        }
        let sim = |e: sf_sim::ScheduleError| ScenarioError::Sim(e.to_string());
        for (player, command) in &self.match_commands {
            world
                .schedule(Tick(0), *player, command.clone())
                .map_err(sim)?;
        }

        let mut placement = Rng::with_stream(self.seed, PLACEMENT_STREAM);
        for g in &self.spawns {
            let (x0, x1) = (Fx::from_int(g.area.min.0), Fx::from_int(g.area.max.0));
            let (y0, y1) = (Fx::from_int(g.area.min.1), Fx::from_int(g.area.max.1));
            for i in 0..g.count as usize {
                let x = placement.fx_range(x0, x1);
                let y = placement.fx_range(y0, y1);
                let command = Command::Spawn {
                    kind: Kind(g.kinds[i % g.kinds.len()]),
                    pos: Vec2::new(x, y),
                    owner: PlayerId(g.owners[i % g.owners.len()]),
                };
                world
                    .schedule(Tick(g.tick), PlayerId(g.player), command)
                    .map_err(sim)?;
            }
        }

        for c in &self.commands {
            let (tick, player) = (Tick(c.tick), PlayerId(c.player));
            match &c.command {
                Action::SetWanderArea { ids, area } => {
                    let command = Command::SetWanderArea {
                        ids: expand(ids),
                        area: area.to_area(),
                    };
                    world.schedule(tick, player, command).map_err(sim)?;
                }
                Action::MoveTo { ids, target } => {
                    let command = Command::MoveTo {
                        ids: expand(ids),
                        target: Vec2::from_ints(target.0, target.1),
                    };
                    world.schedule(tick, player, command).map_err(sim)?;
                }
                Action::OrderMove {
                    ids,
                    target,
                    queued,
                } => {
                    let order = Order::Move(cell_middle(*target));
                    schedule_order(&mut world, tick, player, ids, order, *queued)?;
                }
                Action::OrderAttackMove {
                    ids,
                    target,
                    queued,
                } => {
                    let order = Order::AttackMove(cell_middle(*target));
                    schedule_order(&mut world, tick, player, ids, order, *queued)?;
                }
                Action::OrderAttack {
                    ids,
                    target,
                    queued,
                } => {
                    let order = Order::Attack(EntityId::new(target.0, target.1));
                    schedule_order(&mut world, tick, player, ids, order, *queued)?;
                }
                Action::OrderGuardPoint {
                    ids,
                    target,
                    queued,
                } => {
                    let order = Order::Guard(GuardTarget::Point(cell_middle(*target)));
                    schedule_order(&mut world, tick, player, ids, order, *queued)?;
                }
                Action::OrderGuardEntity {
                    ids,
                    target,
                    queued,
                } => {
                    let order =
                        Order::Guard(GuardTarget::Entity(EntityId::new(target.0, target.1)));
                    schedule_order(&mut world, tick, player, ids, order, *queued)?;
                }
                Action::OrderScatter { ids, queued } => {
                    schedule_order(&mut world, tick, player, ids, Order::Scatter, *queued)?;
                }
                Action::OrderHold { ids, queued } => {
                    schedule_order(&mut world, tick, player, ids, Order::HoldPosition, *queued)?;
                }
                Action::Stop { ids } => {
                    let command = Command::Stop { ids: expand(ids) };
                    world.schedule(tick, player, command).map_err(sim)?;
                }
                Action::AimTurret { ids, target } => {
                    let command = Command::AimTurret {
                        ids: expand(ids),
                        target: target.map(|(index, generation)| EntityId::new(index, generation)),
                    };
                    world.schedule(tick, player, command).map_err(sim)?;
                }
                Action::Despawn { ids } => {
                    for id in expand(ids) {
                        world
                            .schedule(tick, player, Command::Despawn { id })
                            .map_err(sim)?;
                    }
                }
                Action::TakeOre { cell, amount } => {
                    let command = Command::TakeOre {
                        cell: Cell::new(cell.0, cell.1),
                        amount: *amount,
                    };
                    world.schedule(tick, player, command).map_err(sim)?;
                }
                Action::Place { kind, owner, cell } => {
                    let command = Command::Place {
                        kind: Kind(*kind),
                        cell: Cell::new(cell.0, cell.1),
                        owner: PlayerId(*owner),
                    };
                    world.schedule(tick, player, command).map_err(sim)?;
                }
                Action::SpawnUnit { kind, owner, cell } => {
                    let command = Command::SpawnUnit {
                        kind: Kind(*kind),
                        pos: Vec2::new(
                            Fx::from_i64(i64::from(cell.0)) + Fx::HALF,
                            Fx::from_i64(i64::from(cell.1)) + Fx::HALF,
                        ),
                        owner: PlayerId(*owner),
                    };
                    world.schedule(tick, player, command).map_err(sim)?;
                }
                Action::BuildStructure { kind, cell } => {
                    let command = Command::BuildStructure {
                        kind: Kind(*kind),
                        cell: Cell::new(cell.0, cell.1),
                    };
                    world.schedule(tick, player, command).map_err(sim)?;
                }
                Action::QueueUnit { factory, kind } => {
                    let command = Command::QueueUnit {
                        factory: EntityId::new(factory.0, factory.1),
                        kind: Kind(*kind),
                    };
                    world.schedule(tick, player, command).map_err(sim)?;
                }
                Action::CancelUnit { factory, index } => {
                    let command = Command::CancelUnit {
                        factory: EntityId::new(factory.0, factory.1),
                        index: *index,
                    };
                    world.schedule(tick, player, command).map_err(sim)?;
                }
                Action::SetRally { factory, cell } => {
                    let command = Command::SetRally {
                        factory: EntityId::new(factory.0, factory.1),
                        cell: cell.map(|c| Cell::new(c.0, c.1)),
                    };
                    world.schedule(tick, player, command).map_err(sim)?;
                }
                Action::PlaceStructure { kind, owner, cell } => {
                    let command = Command::PlaceStructure {
                        kind: Kind(*kind),
                        cell: Cell::new(cell.0, cell.1),
                        owner: PlayerId(*owner),
                    };
                    world.schedule(tick, player, command).map_err(sim)?;
                }
            }
        }
        Ok(world)
    }
}

/// Schedules one `Command::Order` for all `ids`.
fn schedule_order(
    world: &mut World,
    tick: Tick,
    player: PlayerId,
    ids: &[IdSpec],
    order: Order,
    queued: bool,
) -> Result<(), ScenarioError> {
    let command = Command::Order {
        ids: expand(ids),
        order,
        queued,
    };
    world
        .schedule(tick, player, command)
        .map_err(|e| ScenarioError::Sim(e.to_string()))
}

fn id_problems(at: &str, ids: &[IdSpec], out: &mut Vec<String>) {
    if ids.is_empty() {
        out.push(format!("{at}: ids must not be empty"));
    }
    for spec in ids {
        match *spec {
            IdSpec::Id(index, _) if index >= MAX_ENTITIES => out.push(format!(
                "{at}: Id({index}, ..) is not a valid slot (the limit is {MAX_ENTITIES} slots)"
            )),
            IdSpec::Range(from, to) if from >= to => out.push(format!(
                "{at}: Range({from}, {to}) is empty (it covers from..to, end exclusive)"
            )),
            IdSpec::Range(_, to) if to > MAX_ENTITIES => out.push(format!(
                "{at}: Range(.., {to}) goes past the limit of {MAX_ENTITIES} slots"
            )),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
