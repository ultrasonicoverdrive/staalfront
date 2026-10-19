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
use sf_data::movement::MovementCosts;
use sf_data::terrain::TerrainTypes;
use sf_data::units::UnitDefs;
use sf_math::{Fx, Rng, Vec2};
use sf_sim::{
    Area, Cell, Command, DEFAULT_MAP_SIDE, EntityId, GuardTarget, Kind, MAX_ENTITIES, MAX_MAP_SIDE,
    MovementTable, Order, PlayerId, Terrain, Tick, UnitTable, World, WorldConfig,
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

    /// How many ticks to run; at least 1.
    pub ticks: u64,
    /// Groups of entities to create.
    #[serde(default)]
    pub spawns: Vec<SpawnGroup>,
    /// Commands for later ticks.
    #[serde(default)]
    pub commands: Vec<TimedCommand>,
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
/// movement table, with the movement costs they came from.
fn load_unit_defs(
    name: &str,
    scenario_file: &Path,
) -> Result<(UnitTable, MovementTable, MovementCosts), ScenarioError> {
    let invalid = |problem: String| ScenarioError::Invalid {
        path: Some(scenario_file.to_path_buf()),
        problems: vec![format!("unit_defs \"{name}\": {problem}")],
    };
    let scenarios = scenario_file.parent().unwrap_or(Path::new(""));
    let defs = UnitDefs::load(&scenarios.join(name), &scenarios.join(".."))
        .map_err(|e| invalid(e.to_string()))?;
    let units = sf_bridge::unit_table(&defs).map_err(invalid)?;
    let costs = MovementCosts::from_defs(&defs, &TerrainTypes::embedded())
        .map_err(|e| invalid(e.to_string()))?;
    let movement = sf_bridge::movement_table(&costs).map_err(invalid)?;
    Ok((units, movement, costs))
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
            let (units, movement, costs) = load_unit_defs(&name, path)?;
            scenario.unit_table = Some(units);
            scenario.movement_table = Some(movement);
            own_costs = Some(costs);
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
                Action::Place { cell, .. }
                | Action::SpawnUnit { cell, .. }
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
                            Action::SpawnUnit { .. } | Action::PlaceStructure { .. }
                        )
                    {
                        p.push(format!(
                            "{at}: SpawnUnit and PlaceStructure need `unit_defs`"
                        ));
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
            if let Some(movement) = self.movement_table.clone() {
                world = world
                    .with_movement(movement)
                    .map_err(|e| ScenarioError::Sim(e.to_string()))?;
            }
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
        let sim = |e: sf_sim::ScheduleError| ScenarioError::Sim(e.to_string());

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
