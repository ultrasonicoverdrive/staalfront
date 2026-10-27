//! [`World`]: the simulation state and the fixed tick.

use core::fmt;

use sf_math::{Facing, Fx, Rng, Vec2};

use crate::behavior::{self, Area, Behavior, Motion};
use crate::buildings::{BuildingTable, BuildingsError};
use crate::command::{Command, CommandQueue, ScheduleError};
use crate::crowd::{CollisionConfig, Crowd, CrowdStats};
use crate::destruction::{Death, Destruction, DestructionConfig, Tally, Wreck};
use crate::entities::{Entities, NO_DEF, NewEntity};
use crate::formation::{FormationConfig, FormationStats, FormationView, Formations};
use crate::harvest::{DockView, Harvest, HarvestRules, HarvesterView};
use crate::ids::{EntityId, Kind, PlayerId, Tick, per_tick};
use crate::movement::{Cost, Ground, MovementTable};
use crate::orders::{self, GuardTarget, Order};
use crate::ore::{Ore, OreRules};
use crate::path::{MIN_NODE_BUDGET, PathStats, Pathing, PlannedRoute, ReplanConfig, RouteView};
use crate::placement::PlacementState;
use crate::projectiles::ProjectileId;
use crate::snapshot::{EntityView, ProjectileView, Snapshot, TurretView};
use crate::targeting::{self, Engagement, Targeting, TargetingConfig};
use crate::terrain::{Cell, NO_CELL, Occupancy, Terrain, TerrainId};
use crate::turret;
use crate::units::{Footprint, UnitClass, UnitStats, UnitTable};
use crate::weapons::{self, Arsenal, Impact, Shot, WeaponTable, WeaponsError};

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
    /// The terrain layer's size is not the map size of the configuration.
    TerrainSize,
    /// The unit table was given to a world that has already started (a tick
    /// ran, an entity exists or a command is pending). The table is part of
    /// how the world is made.
    UnitsAfterStart,
    /// The destruction settings were given to a world that has already started, or are not valid
    /// ([`DestructionConfig::is_valid`]).
    Destruction,
    /// The movement table was given to a world that has already started, for
    /// the same reason.
    MovementAfterStart,
    /// The path budget was given to a world that has already started, or is
    /// below [`MIN_NODE_BUDGET`](crate::MIN_NODE_BUDGET).
    PathBudget,
    /// The flowfield threshold was given to a world that has already started,
    /// or is one (a group has at least two units; zero turns flowfields off).
    FlowfieldThreshold,
    /// The collision settings were given to a world that has already started, or
    /// are not valid ([`CollisionConfig::is_valid`](crate::CollisionConfig::is_valid)).
    Collision,
    /// The replan settings were given to a world that has already started, or are
    /// not valid ([`ReplanConfig::is_valid`](crate::ReplanConfig::is_valid)).
    Replan,
    /// The targeting settings were given to a world that has already started, or are not valid
    /// ([`TargetingConfig::is_valid`]).
    Targeting,
    /// The formation settings were given to a world that has already started, or are
    /// not valid ([`FormationConfig::is_valid`](crate::FormationConfig::is_valid)).
    Formation,
    /// The weapon table was given to a world that has already started, or before
    /// its unit table.
    WeaponsAfterStart,
    /// The weapon table is not valid, or does not fit the unit table.
    Weapons(WeaponsError),
    /// The building table was given to a world that has already started. Given
    /// before the unit table, it is refused as
    /// [`BuildingsError::NotAStructure`] instead.
    BuildingsAfterStart,
    /// The building table is not valid, or does not fit the unit table.
    Buildings(BuildingsError),
    /// The ore rules were given to a world that has already started, or are not
    /// valid ([`OreRules::is_valid`]).
    OreRules,
    /// The harvest rules were given to a world that has already started, before the unit or the
    /// building table, with invalid numbers, or with a harvester kind that is not a mobile unit of
    /// the table (WP7.2).
    Harvest,
    /// The player factions were given to a world that has already started.
    PlayersAfterStart,
    /// A player faction is not exactly one faction bit, or a player is listed twice.
    PlayerFaction(PlayerId),
    /// The movement table has no cost for a terrain type of the map: a class
    /// would not know what to do on such a cell.
    MovementGap {
        /// The movement class.
        class: u16,
        /// The terrain type it leaves out.
        terrain: TerrainId,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ConfigError::EmptyMap => "the map has no cells",
            ConfigError::MapTooLarge => "a map side is above the maximum",
            ConfigError::SpeedTooLow => "the speed rounds to zero per tick",
            ConfigError::NoTurning => "the turn rate per tick is zero",
            ConfigError::TerrainSize => "the terrain layer is not the size of the map",
            ConfigError::UnitsAfterStart => "the unit table must be given before the world starts",
            ConfigError::PathBudget => {
                "the path budget must be given before the world starts and be at least the minimum"
            }
            ConfigError::FlowfieldThreshold => {
                "the flowfield threshold must be given before the world starts and be 0 or at least 2"
            }
            ConfigError::Collision => {
                "the collision settings must be given before the world starts and be valid"
            }
            ConfigError::Replan => {
                "the replan settings must be given before the world starts and be valid"
            }
            ConfigError::Targeting => {
                "the targeting settings must be given before the world starts and be valid"
            }
            ConfigError::Destruction => {
                "the destruction settings must be given before the world starts and be valid"
            }
            ConfigError::Formation => {
                "the formation settings must be given before the world starts and be valid"
            }
            ConfigError::MovementAfterStart => {
                "the movement table must be given before the world starts"
            }
            ConfigError::WeaponsAfterStart => {
                "the weapon table must be given before the world starts, after its unit table"
            }
            ConfigError::Weapons(_) => "the weapon table does not fit the unit table",
            ConfigError::BuildingsAfterStart => {
                "the building table must be given before the world starts, after its unit table"
            }
            ConfigError::Buildings(_) => "the building table does not fit the unit table",
            ConfigError::OreRules => {
                "the ore rules must be given before the world starts and be valid"
            }
            ConfigError::Harvest => {
                "the harvest rules must be given before the world starts, after the unit and building tables, \
                 with valid numbers and mobile harvester kinds"
            }
            ConfigError::PlayersAfterStart => {
                "the player factions must be given before the world starts"
            }
            ConfigError::PlayerFaction(_) => {
                "a player faction must be exactly one faction bit, once per player"
            }
            ConfigError::MovementGap { .. } => {
                "a movement class has no cost for a terrain type of the map"
            }
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
    /// Units and structures destroyed in that tick (WP5.5); always zero in a world without destruction.
    /// They are listed by [`World::deaths`] until the next step.
    pub deaths: u32,
    /// Shots the weapons fired in that tick (WP5.1; always zero in a world without a
    /// weapon table). They are listed by [`World::shots`] until the next step.
    pub shots: u32,
    /// Projectiles launched in that tick (WP5.2): the shots of every weapon that is not `Instant`.
    pub launched: u32,
    /// Projectiles that went off in that tick, hit or miss. They are listed by [`World::impacts`] until
    /// the next step.
    pub impacts: u32,
    /// [`Command::BuildStructure`] commands refused in that tick (WP6.2); they are listed by
    /// [`World::placement_rejections`] until the next step. Also counted in `commands_ignored`.
    pub placements_rejected: u32,
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
    /// The terrain type of every cell. Fixed at creation.
    pub(crate) terrain: Terrain,
    /// Who stands on each cell. Changed only by commands, inside the tick.
    pub(crate) occupancy: Occupancy,
    /// What the simulation knows about each unit kind (WP3.2). Fixed at
    /// creation; empty for a world without data-driven units.
    pub(crate) units: UnitTable,
    /// What each movement class does on each terrain type (WP4.1). Fixed at
    /// creation; empty for a world without movement classes, which is open
    /// ground for everyone.
    pub(crate) movement: MovementTable,
    /// The sectors, portals and routes (WP4.2): the abstract graph of every
    /// movement class that terrain restricts, the job being served and the
    /// budget. Empty (inactive) on a world without terrain or movement table.
    pub(crate) pathing: Pathing,
    /// The collision pass (WP4.4): the settings, the bodies of the unit kinds, the
    /// scratch memory.
    pub(crate) crowd: Crowd,
    /// The formations of group move orders (WP4.6): their settings and the groups that
    /// keep their shape.
    pub(crate) formations: Formations,
    /// The weapons (WP5.1): their table, and the shots of the last tick. Empty (nothing
    /// fires) for a world that was given no table.
    pub(crate) arsenal: Arsenal,
    /// What the structure kinds are for (WP6.1). Fixed at creation; empty for a
    /// world without building rules.
    pub(crate) buildings: BuildingTable,
    /// The ore on the map (WP7.1): an amount per field cell. `None` for a world
    /// that was given no ore rules.
    pub(crate) ore: Option<Ore>,
    /// The harvesters and the refinery docks (WP7.2). `None` for a world that was given no harvest
    /// rules.
    pub(crate) harvest: Option<Box<Harvest>>,
    /// Who is which faction, and the builds refused in the last tick (WP6.2).
    pub(crate) placement: PlacementState,
    /// Target selection and return fire (WP5.4): the settings, if the world was given any.
    pub(crate) targeting: Targeting,
    /// Destruction (WP5.5): the settings, the wrecks and the scores. Nothing is destroyed in a world
    /// that was given no settings.
    pub(crate) destruction: Destruction,
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
pub(crate) const fn mix64(mut z: u64) -> u64 {
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
    /// A world at tick zero with no entities and no terrain layer: every cell
    /// is open ground ([`Terrain::open`]).
    pub fn new(config: WorldConfig) -> Result<World, ConfigError> {
        config.validate()?;
        let terrain = Terrain::open(config.width, config.height);
        Ok(World::with_parts(config, terrain))
    }

    /// A world at tick zero with no entities, on the given terrain. The
    /// terrain must be exactly the size of the map in the configuration
    /// ([`ConfigError::TerrainSize`] otherwise).
    pub fn with_terrain(config: WorldConfig, terrain: Terrain) -> Result<World, ConfigError> {
        config.validate()?;
        if (terrain.width(), terrain.height()) != (config.width, config.height) {
            return Err(ConfigError::TerrainSize);
        }
        Ok(World::with_parts(config, terrain))
    }

    /// The same world with the stats of the unit kinds it can spawn
    /// ([`Command::SpawnUnit`], [`Command::PlaceStructure`]). Call it right
    /// after the world is made, before anything is scheduled
    /// ([`ConfigError::UnitsAfterStart`] otherwise). `sf_sim` does not read
    /// `sf_data`: the caller converts the definitions to [`UnitStats`].
    pub fn with_units(mut self, units: UnitTable) -> Result<World, ConfigError> {
        if self.tick != Tick::ZERO || self.entities.len() != 0 || !self.queue.is_empty() {
            return Err(ConfigError::UnitsAfterStart);
        }
        self.units = units;
        self.crowd
            .set_tables(&self.units, &self.movement, self.map_size());
        Ok(self)
    }

    /// The same world with the weapons of its unit kinds (WP5.1, `docs/rules/weapons.md`):
    /// the numbers of every weapon and which weapons each kind carries. Without a table a
    /// world fires nothing. Call it right after [`World::with_units`] and before anything
    /// is scheduled ([`ConfigError::WeaponsAfterStart`] otherwise); the table has to fit
    /// the unit table ([`ConfigError::Weapons`]: a loadout for an unknown kind, a first
    /// weapon whose range is not the kind's `weapon_range`, a turret mount on a kind without
    /// a turret). `sf_sim` does not read `sf_data`: `sf_bridge::weapon_table` converts the
    /// definitions.
    pub fn with_weapons(mut self, table: WeaponTable) -> Result<World, ConfigError> {
        if self.tick != Tick::ZERO || self.entities.len() != 0 || !self.queue.is_empty() {
            return Err(ConfigError::WeaponsAfterStart);
        }
        self.arsenal = Arsenal::new(table, &self.units).map_err(ConfigError::Weapons)?;
        Ok(self)
    }

    /// The same world with the building rules of its structure kinds (WP6.1,
    /// `docs/rules/buildings.md`): function, exit side, build time, price, power,
    /// factions and prerequisites. Nothing acts on them yet. Call it right after
    /// [`World::with_units`] and before anything is scheduled
    /// ([`ConfigError::BuildingsAfterStart`] otherwise); every entry has to be a
    /// structure of the unit table with the same footprint
    /// ([`ConfigError::Buildings`]). `sf_bridge::building_table` converts the
    /// definitions.
    pub fn with_buildings(mut self, table: BuildingTable) -> Result<World, ConfigError> {
        if self.tick != Tick::ZERO || self.entities.len() != 0 || !self.queue.is_empty() {
            return Err(ConfigError::BuildingsAfterStart);
        }
        for b in table.iter() {
            let Some(unit) = self.units.get(b.kind) else {
                return Err(ConfigError::Buildings(BuildingsError::NotAStructure(
                    b.kind,
                )));
            };
            match unit.footprint {
                None => {
                    return Err(ConfigError::Buildings(BuildingsError::NotAStructure(
                        b.kind,
                    )));
                }
                Some(fp) if fp != b.footprint => {
                    return Err(ConfigError::Buildings(BuildingsError::FootprintMismatch(
                        b.kind,
                    )));
                }
                Some(_) => {}
            }
        }
        self.buildings = table;
        Ok(self)
    }

    /// The same world with ore on its map (WP7.1, `docs/rules/economy.md`): every
    /// harvestable cell of the terrain holds `rules.start`, and the cells grow back by
    /// the rules. Call it right after the world is made, before anything is scheduled
    /// ([`ConfigError::OreRules`] otherwise, and for invalid numbers). A world without
    /// ore rules has no ore state and hashes as before.
    pub fn with_ore(mut self, rules: OreRules) -> Result<World, ConfigError> {
        if self.tick != Tick::ZERO
            || self.entities.len() != 0
            || !self.queue.is_empty()
            || !rules.is_valid()
        {
            return Err(ConfigError::OreRules);
        }
        self.ore = Some(Ore::new(rules, &self.terrain));
        Ok(self)
    }

    /// The same world with harvesters (WP7.2, `docs/rules/economy.md`): the units of the given
    /// kinds search ore, mine it, drive to a refinery (a standing structure with the
    /// `HarvesterDock` function) and unload there, over and over. Call it after the unit table and
    /// the building table and before anything is scheduled ([`ConfigError::Harvest`] otherwise, for
    /// invalid numbers and for a kind that is not a mobile unit of the table). Without an ore state
    /// ([`World::with_ore`]) the harvesters find nothing. The delivered ore is counted per player
    /// ([`World::ore_delivered`]); a world without the rules hashes as before.
    pub fn with_harvest(
        mut self,
        rules: HarvestRules,
        harvesters: &[Kind],
    ) -> Result<World, ConfigError> {
        if self.tick != Tick::ZERO
            || self.entities.len() != 0
            || !self.queue.is_empty()
            || !rules.is_valid()
            || harvesters.is_empty()
            || self.units.is_empty()
        {
            return Err(ConfigError::Harvest);
        }
        let mobile = |k: &Kind| {
            self.units
                .get(*k)
                .is_some_and(|s| s.class() == UnitClass::Mobile)
        };
        if !harvesters.iter().all(mobile) {
            return Err(ConfigError::Harvest);
        }
        self.harvest = Some(Box::new(Harvest::new(
            rules,
            harvesters.to_vec(),
            &self.units,
            &self.buildings,
        )));
        Ok(self)
    }

    /// The same world with the terrain costs of the movement classes (WP4.1,
    /// [`MovementTable`]). Call it right after the world is made, before
    /// anything is scheduled ([`ConfigError::MovementAfterStart`] otherwise).
    /// On a world with a terrain layer every class has to list every terrain
    /// type of the map ([`ConfigError::MovementGap`] otherwise); a world
    /// without a layer is open ground and takes any table.
    pub fn with_movement(mut self, movement: MovementTable) -> Result<World, ConfigError> {
        if self.tick != Tick::ZERO || self.entities.len() != 0 || !self.queue.is_empty() {
            return Err(ConfigError::MovementAfterStart);
        }
        if self.terrain.has_layer()
            && let Some((class, terrain)) = movement.first_gap(self.terrain.defined_ids())
        {
            return Err(ConfigError::MovementGap { class, terrain });
        }
        self.movement = movement;
        let replan = self.pathing.replan_config();
        self.pathing = Pathing::build(
            &self.terrain,
            &self.movement,
            &self.occupancy,
            self.pathing.budget(),
            self.pathing.flowfield_threshold(),
        );
        self.pathing.set_replan(replan);
        self.crowd
            .set_tables(&self.units, &self.movement, self.map_size());
        Ok(self)
    }

    /// The same world with other collision settings (WP4.4,
    /// `docs/rules/collision.md`): whether bodies collide at all, how many ticks a
    /// blocked unit waits before it detours, how many without progress make it
    /// step to a free neighbouring spot, and how many such steps before it asks
    /// for a new route. A world that is given none uses
    /// [`CollisionConfig::DEFAULT`](crate::CollisionConfig::DEFAULT), the numbers
    /// of the shipped `sim.ron`. Call it right after the world is made
    /// ([`ConfigError::Collision`] after it has started, or for invalid numbers).
    pub fn with_collision(mut self, config: CollisionConfig) -> Result<World, ConfigError> {
        if self.tick != Tick::ZERO
            || self.entities.len() != 0
            || !self.queue.is_empty()
            || !config.is_valid()
        {
            return Err(ConfigError::Collision);
        }
        self.crowd.config = config;
        Ok(self)
    }

    /// The same world with other replan settings (WP4.7, `docs/rules/pathfinding.md`,
    /// "Replanning"): how many ticks pass between two replans of a stuck unit, how many
    /// it may ask for in one order before the order is given up, and how many ticks
    /// pass between two new searches for a target that keeps moving. A world that is
    /// given none uses [`ReplanConfig::DEFAULT`](crate::ReplanConfig::DEFAULT), the
    /// numbers of the shipped `sim.ron`. Call it right after the world is made
    /// ([`ConfigError::Replan`] after it has started, or for invalid numbers).
    pub fn with_replan(mut self, config: ReplanConfig) -> Result<World, ConfigError> {
        if self.tick != Tick::ZERO
            || self.entities.len() != 0
            || !self.queue.is_empty()
            || !config.is_valid()
        {
            return Err(ConfigError::Replan);
        }
        self.pathing.set_replan(config);
        Ok(self)
    }

    /// The same world with other formation settings (WP4.6, `docs/rules/formations.md`):
    /// whether a group move order forms a formation at all and how it keeps its shape and
    /// its pace. A world that is given none uses
    /// [`FormationConfig::DEFAULT`](crate::FormationConfig::DEFAULT), the numbers of the
    /// shipped `sim.ron`. Call it right after the world is made
    /// ([`ConfigError::Formation`] after it has started, or for invalid numbers).
    pub fn with_formation(mut self, config: FormationConfig) -> Result<World, ConfigError> {
        if self.tick != Tick::ZERO
            || self.entities.len() != 0
            || !self.queue.is_empty()
            || !config.is_valid()
        {
            return Err(ConfigError::Formation);
        }
        self.formations.config = config;
        Ok(self)
    }

    /// The same world with its units choosing their own targets (WP5.4, `docs/rules/targeting.md`):
    /// an idle unit, a unit on hold, a guard and a unit on attack-move pick an enemy in the reach of
    /// their weapons, and a unit that is shot at turns on its attacker. A world that is given none
    /// does none of it: a unit fires only at the victim of its attack order, which is what every
    /// scenario of the phases before this one is pinned on. Needs a weapon table to do anything. Call
    /// it right after the world is made ([`ConfigError::Targeting`] after it has started, or for
    /// invalid numbers); [`TargetingConfig::DEFAULT`] is the numbers of the shipped `sim.ron`.
    pub fn with_targeting(mut self, config: TargetingConfig) -> Result<World, ConfigError> {
        if self.tick != Tick::ZERO
            || self.entities.len() != 0
            || !self.queue.is_empty()
            || !config.is_valid()
        {
            return Err(ConfigError::Targeting);
        }
        self.targeting.config = Some(config);
        Ok(self)
    }

    /// The same world with destruction (WP5.5, `docs/rules/destruction.md`): a unit or structure whose hit
    /// points the weapons take to zero is removed from the world at the end of the weapon stage, its
    /// cells are freed, it may leave a wreck or rubble, and the kill is credited. A world that is given no
    /// settings destroys nothing: a unit at zero stays where it fell, which is what the scenarios of
    /// WP5.1 to WP5.4 are pinned on. Call it right after the world is made
    /// ([`ConfigError::Destruction`] after it has started, or for invalid numbers);
    /// [`DestructionConfig::DEFAULT`] is the numbers of the shipped `sim.ron`.
    pub fn with_destruction(mut self, config: DestructionConfig) -> Result<World, ConfigError> {
        if self.tick != Tick::ZERO
            || self.entities.len() != 0
            || !self.queue.is_empty()
            || !config.is_valid()
        {
            return Err(ConfigError::Destruction);
        }
        self.destruction.config = Some(config);
        Ok(self)
    }

    /// The same world with a different path budget: the nodes of search work it
    /// may do in one tick ([`DEFAULT_NODE_BUDGET`](crate::DEFAULT_NODE_BUDGET)
    /// otherwise). Call it right after the world is made
    /// ([`ConfigError::PathBudget`] after it has started, or below
    /// [`MIN_NODE_BUDGET`](crate::MIN_NODE_BUDGET)). It decides when a unit gets
    /// its route, never which route.
    pub fn with_path_budget(mut self, nodes: u32) -> Result<World, ConfigError> {
        if self.tick != Tick::ZERO
            || self.entities.len() != 0
            || !self.queue.is_empty()
            || nodes < MIN_NODE_BUDGET
        {
            return Err(ConfigError::PathBudget);
        }
        self.pathing.set_budget(nodes);
        Ok(self)
    }

    /// The same world with a different flowfield threshold: the number of units
    /// of one movement class that a single order must send to one place for the
    /// world to build one flowfield for them instead of routing each
    /// (WP4.3, `docs/rules/pathfinding.md`). Zero turns flowfields off; one is
    /// refused (a lone unit is no group). A world that is given none uses
    /// [`DEFAULT_FLOWFIELD_THRESHOLD`](crate::DEFAULT_FLOWFIELD_THRESHOLD), the
    /// number of the shipped `sim.ron`. Call it right after the world is made
    /// ([`ConfigError::FlowfieldThreshold`] after it has started, or for one).
    pub fn with_flowfield_threshold(mut self, threshold: u32) -> Result<World, ConfigError> {
        if self.tick != Tick::ZERO
            || self.entities.len() != 0
            || !self.queue.is_empty()
            || threshold == 1
        {
            return Err(ConfigError::FlowfieldThreshold);
        }
        self.pathing.set_flowfield_threshold(threshold);
        Ok(self)
    }

    fn with_parts(config: WorldConfig, terrain: Terrain) -> World {
        let mut crowd = Crowd::new();
        crowd.set_tables(
            &UnitTable::empty(),
            &MovementTable::empty(),
            (config.width, config.height),
        );
        World {
            config,
            tick: Tick::ZERO,
            entities: Entities::default(),
            free: Vec::new(),
            live: 0,
            spawn_serial: 0,
            occupancy: Occupancy::new(config.width, config.height),
            terrain,
            units: UnitTable::empty(),
            movement: MovementTable::empty(),
            pathing: Pathing::inactive(),
            crowd,
            formations: Formations::new(),
            arsenal: Arsenal::default(),
            buildings: BuildingTable::empty(),
            ore: None,
            harvest: None,
            placement: PlacementState::default(),
            targeting: Targeting::default(),
            destruction: Destruction::default(),
            queue: CommandQueue::new(),
            history: Vec::new(),
        }
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

    /// The terrain: a type and its flags for every cell. Read-only; it never
    /// changes after the world is created.
    pub fn terrain(&self) -> &Terrain {
        &self.terrain
    }

    /// The ore state (WP7.1); `None` for a world that was given no ore rules.
    pub fn ore(&self) -> Option<&Ore> {
        self.ore.as_ref()
    }

    /// The ore `cell` holds; zero outside the map, on a cell that is no field cell and in a
    /// world without ore rules.
    pub fn ore_at(&self, cell: Cell) -> u16 {
        self.ore.as_ref().map_or(0, |o| o.amount_at(cell))
    }

    /// The nearest cell with ore from `from` within `max_radius` cells: see
    /// [`Ore::nearest_where`] for the order. `None` in a world without ore.
    pub fn nearest_ore(&self, from: Cell, max_radius: u32) -> Option<Cell> {
        self.nearest_ore_where(from, max_radius, |_| true)
    }

    /// [`World::nearest_ore`] among the cells `accept` takes (a harvester skips the cells
    /// another one has claimed).
    pub fn nearest_ore_where(
        &self,
        from: Cell,
        max_radius: u32,
        accept: impl FnMut(Cell) -> bool,
    ) -> Option<Cell> {
        self.ore.as_ref()?.nearest_where(from, max_radius, accept)
    }

    /// The harvest rules (WP7.2); `None` for a world that was given none.
    pub fn harvest_rules(&self) -> Option<&HarvestRules> {
        self.harvest.as_deref().map(Harvest::rules)
    }

    /// The kinds that harvest, ascending; empty without harvest rules.
    pub fn harvester_kinds(&self) -> &[Kind] {
        self.harvest.as_deref().map_or(&[], Harvest::kinds)
    }

    /// The units of ore the refineries have taken off the harvesters of `player` so far
    /// (WP7.2). Zero without harvest rules. WP7.3 (storage) and WP7.4 (money) replace or extend
    /// this counter; see `docs/rules/economy.md`.
    pub fn ore_delivered(&self, player: PlayerId) -> u64 {
        self.harvest.as_deref().map_or(0, |h| h.delivered(player))
    }

    /// What harvester `id` is doing and how much it carries; `None` for anything that is not a
    /// live harvester of a world with harvest rules (and for a harvester that was spawned in the
    /// current tick, before the first tick has run).
    pub fn harvester_of(&self, id: EntityId) -> Option<HarvesterView> {
        let harvest = self.harvest.as_deref()?;
        self.live_index(id)?;
        harvest.view_of(id, self.config.width)
    }

    /// The bay and the queue of the refinery `id`; `None` for anything that is not a standing
    /// dock that the harvest stage has seen.
    pub fn dock_of(&self, id: EntityId) -> Option<DockView> {
        self.live_index(id)?;
        self.harvest.as_deref()?.dock_view(id)
    }

    /// The terrain type of `cell`; `None` outside the map.
    pub fn terrain_at(&self, cell: Cell) -> Option<TerrainId> {
        self.terrain.id_at(cell)
    }

    /// The entity that occupies `cell`; `None` if it is free or outside the
    /// map. Only structures created by [`Command::Place`] occupy cells.
    pub fn occupant_at(&self, cell: Cell) -> Option<EntityId> {
        let slot = self.occupancy.slot_at(self.occupancy.index(cell)?)? as usize;
        Some(EntityId::new(slot as u32, self.entities.generation[slot]))
    }

    /// True if `cell` is inside the map and occupied.
    pub fn is_occupied(&self, cell: Cell) -> bool {
        self.occupant_at(cell).is_some()
    }

    /// The cell an entity occupies; `None` for a stale id and for an entity
    /// that occupies none (every wanderer and every mobile unit). A structure
    /// with a footprint of several cells answers with its anchor, the cell
    /// with the smallest x and y; [`World::footprint_of`] gives the rest.
    pub fn cell_of(&self, id: EntityId) -> Option<Cell> {
        let i = self.live_index(id)?;
        let cell = self.entities.cell[i];
        (cell != NO_CELL).then(|| self.occupancy.cell(cell))
    }

    /// The cells a structure covers, counted from its anchor
    /// ([`World::cell_of`]): `None` for a stale id and for an entity that is
    /// not a structure. A structure made by the legacy [`Command::Place`]
    /// covers one cell.
    pub fn footprint_of(&self, id: EntityId) -> Option<Footprint> {
        let i = self.live_index(id)?;
        (self.entities.cell[i] != NO_CELL).then(|| self.footprint_at(i))
    }

    /// The unit table the world was made with (empty without data-driven
    /// units).
    pub fn unit_table(&self) -> &UnitTable {
        &self.units
    }

    /// The building table the world was made with (empty without building
    /// rules, WP6.1).
    pub fn building_table(&self) -> &BuildingTable {
        &self.buildings
    }

    /// The weapon table the world was made with (empty without weapons).
    pub fn weapon_table(&self) -> &WeaponTable {
        &self.arsenal.table
    }

    /// The shots fired in the last tick (see [`crate::weapons`]), in the order of the attackers'
    /// slots and, for one attacker, of its weapon slots: a report, so compare them as a set. Empty
    /// before the first step and in a world without weapons.
    pub fn shots(&self) -> &[Shot] {
        &self.arsenal.shots
    }

    /// The projectiles that went off in the last tick (WP5.2, see [`crate::projectiles`]), hit or
    /// miss, in the order of their slots. A report: the order follows the slots, so compare the
    /// impacts of a tick as a set. Empty before the first step and in a world without weapons.
    pub fn impacts(&self) -> &[Impact] {
        &self.arsenal.impacts
    }

    /// The number of projectiles in flight.
    pub fn projectile_count(&self) -> usize {
        self.arsenal.projectiles.len()
    }

    /// One projectile in flight; `None` if it has gone off or the id is stale.
    pub fn projectile(&self, id: ProjectileId) -> Option<ProjectileView> {
        let p = self.arsenal.projectiles.get(id)?;
        Some(self.projectile_view(id, p))
    }

    pub(crate) fn projectile_view(
        &self,
        id: ProjectileId,
        p: &crate::projectiles::Projectile,
    ) -> ProjectileView {
        let kind = self
            .arsenal
            .table
            .weapon(p.weapon)
            .map_or(weapons::ProjectileKind::Straight, |w| w.projectile);
        ProjectileView {
            id,
            weapon: p.weapon,
            kind,
            owner: p.owner,
            player: p.player,
            target: match p.target {
                crate::projectiles::Target::Entity(t) => Some(t),
                crate::projectiles::Target::Point => None,
            },
            aim: p.aim,
            prev_pos: p.prev_pos,
            pos: p.pos,
            prev_height: p.prev_height,
            height: p.height,
            heading: p.heading,
        }
    }

    /// The ticks until weapon slot `slot` of entity `id` may fire again, zero when it is
    /// ready; `None` for a stale id, an entity without weapons, or a slot it does not have.
    pub fn weapon_wait(&self, id: EntityId, slot: usize) -> Option<u32> {
        let i = self.live_index(id)?;
        self.weapon_slot(i, slot).map(|s| s.wait)
    }

    /// The shots still to come in the burst weapon slot `slot` of entity `id` is firing;
    /// zero when none runs. `None` as for [`World::weapon_wait`].
    pub fn weapon_burst_left(&self, id: EntityId, slot: usize) -> Option<u8> {
        let i = self.live_index(id)?;
        self.weapon_slot(i, slot).map(|s| s.burst_left)
    }

    fn weapon_slot(&self, i: usize, slot: usize) -> Option<&weapons::SlotState> {
        let def = self.entities.def[i];
        if def == NO_DEF {
            return None;
        }
        let kind = self.units.at(def as usize).kind;
        (slot < self.arsenal.table.loadout(kind).len()).then(|| &self.entities.weapon[i][slot])
    }

    /// The movement table the world was made with (empty without movement
    /// classes).
    pub fn movement_table(&self) -> &MovementTable {
        &self.movement
    }

    /// What movement class `class` (a permanent kind number, as in
    /// [`UnitStats::movement_class`]) does on `cell` (WP4.1): [`Cost::Impassable`],
    /// or [`Cost::Speed`] with the thousandths of its own speed a unit moves at
    /// there. A cell outside the map is impassable; a world without a terrain
    /// layer, a world without a movement table and a class the table does not
    /// have are open ground, [`Cost::OPEN`]. The movement itself asks this same
    /// question, so the answer is what a unit will do. Read-only.
    pub fn cost(&self, class: u16, cell: Cell) -> Cost {
        self.ground().cost(class, cell)
    }

    /// Whether a unit of movement class `class` may enter `cell`: `cost` is
    /// not impassable.
    pub fn passable(&self, class: u16, cell: Cell) -> bool {
        self.cost(class, cell).is_passable()
    }

    /// The table entry an entity was spawned from; `None` for a stale id and
    /// for an entity made by the legacy `Spawn` or `Place`.
    pub fn stats_of(&self, id: EntityId) -> Option<&UnitStats> {
        let i = self.live_index(id)?;
        self.stats_at(i)
    }

    /// The current hit points of an entity made from the unit table; `None`
    /// for a stale id and for an entity without an entry (the legacy ones have
    /// no hit points). An entry with `hp` zero (an object that cannot be
    /// damaged) reports zero.
    pub fn hp_of(&self, id: EntityId) -> Option<u32> {
        let i = self.live_index(id)?;
        (self.entities.def[i] != NO_DEF).then(|| self.entities.hp[i])
    }

    /// The turret facing of a unit that has one (WP4.5): an absolute direction
    /// in the world. `None` for a stale id and for a unit without a turret.
    pub fn turret_of(&self, id: EntityId) -> Option<Facing> {
        let i = self.live_index(id)?;
        turret::has_turret(&self.entities, i, &self.units).then(|| self.entities.turret[i])
    }

    /// What the turret of a unit is told to aim at (`Command::AimTurret`);
    /// `None` for a stale id, for a unit without a turret and when nothing is
    /// set. An attack order aims the turret too, without being set here.
    pub fn turret_target_of(&self, id: EntityId) -> Option<EntityId> {
        let i = self.live_index(id)?;
        self.entities.turret_target[i]
    }

    /// The current order of a unit (WP3.4); `None` for a stale id. A unit
    /// with no order, and everything that cannot be ordered, has
    /// [`Order::Idle`]. An order that was a scatter reads as the
    /// [`Order::Move`] it turned into.
    pub fn order_of(&self, id: EntityId) -> Option<Order> {
        let i = self.live_index(id)?;
        Some(self.entities.order[i])
    }

    /// The targeting settings, if the world was given any (WP5.4).
    pub fn targeting_config(&self) -> Option<TargetingConfig> {
        self.targeting.config
    }

    /// The target a unit chose itself and what it goes back to, while its order is a `Pursue`
    /// (WP5.4); `None` for a stale id and for a unit that has not chosen.
    pub fn engagement_of(&self, id: EntityId) -> Option<Engagement> {
        let i = self.live_index(id)?;
        let Order::Pursue(target) = self.entities.order[i] else {
            return None;
        };
        let eng = &self.entities.eng[i];
        Some(Engagement {
            target,
            following: eng.chase,
            resumes: eng.resume.unwrap_or(Order::Idle),
        })
    }

    /// The ticks until a unit looks round for a target again (WP5.4); `None` for a stale id.
    pub fn scan_wait_of(&self, id: EntityId) -> Option<u16> {
        let i = self.live_index(id)?;
        Some(self.entities.eng[i].cool)
    }

    /// The orders waiting behind the current one, front first; `None` for a
    /// stale id. At most [`ORDER_QUEUE_CAP`](crate::ORDER_QUEUE_CAP).
    pub fn queued_orders_of(&self, id: EntityId) -> Option<Vec<Order>> {
        let i = self.live_index(id)?;
        Some(self.entities.queue[i].iter().collect())
    }

    /// The point a unit is steering at right now: the next waypoint of its
    /// route, and once the route is done (or for a unit that is not routed) the
    /// destination itself. `None` for a stale id, for a unit that is not walking
    /// and for one that waits for its route or stands at the end of it.
    pub fn waypoint_of(&self, id: EntityId) -> Option<Vec2> {
        let i = self.live_index(id)?;
        self.entities.target[i]
    }

    /// The arrival slot of a unit that follows a flowfield (WP4.3, WP4.4): the point
    /// it stands on when it has arrived, once the field has picked its slots.
    /// `None` for any other unit.
    pub fn arrival_slot_of(&self, id: EntityId) -> Option<Vec2> {
        let i = self.live_index(id)?;
        self.pathing.arrival_point(&self.entities, i)
    }

    /// The arrival slot a unit of a group has taken for itself (WP4.4b,
    /// `docs/rules/collision.md`, "Arrival"): the slot it took when it came near the
    /// goal, which is not the one its rank would give it. `None` before it has come
    /// near, for a unit that shares a slot because none was free, for any other unit,
    /// and once the group's field is gone (a few seconds after the last member
    /// stopped asking it).
    pub fn claimed_slot_of(&self, id: EntityId) -> Option<Vec2> {
        let i = self.live_index(id)?;
        self.pathing.claimed_point(&self.entities, i)
    }

    /// The route of a unit (WP4.2): whether it follows waypoints, waits for
    /// them or stands at the end of its route, and whether its destination
    /// could be reached; `None` for a stale id. A unit without a walking
    /// order, a class that terrain does not restrict and a world without
    /// pathfinding all read as no route.
    pub fn route_of(&self, id: EntityId) -> Option<RouteView> {
        let i = self.live_index(id)?;
        Some(self.pathing.view(&self.entities, i))
    }

    /// What the path service did with its budget (WP4.2).
    pub fn flowfield_threshold(&self) -> u32 {
        self.pathing.flowfield_threshold()
    }

    /// The collision settings of the world (WP4.4).
    pub fn collision_config(&self) -> CollisionConfig {
        self.crowd.config
    }

    /// What the crowd pass did so far.
    pub fn crowd_stats(&self) -> CrowdStats {
        self.crowd.stats
    }

    /// The radius of the body of entity `id`, in cells: half the `size` of its
    /// definition. `None` for an entity that is not a unit of the table or whose
    /// kind does not collide (aircraft, structures, objects).
    pub fn collision_radius_of(&self, id: EntityId) -> Option<Fx> {
        let i = self.live_index(id)?;
        let def = self.entities.def[i];
        if def == NO_DEF {
            return None;
        }
        let r = self.crowd.radius_of_def(def as usize);
        (r > Fx::ZERO).then_some(r)
    }

    /// Whether the unit has asked for a new route because sidesteps did not get it
    /// out of a crowd (WP4.7: it asks round the bodies that stand still near it,
    /// `docs/rules/pathfinding.md`, "Replanning"). It stays set until the unit's order
    /// changes or ends.
    pub fn replan_requested(&self, id: EntityId) -> Option<bool> {
        let i = self.live_index(id)?;
        Some(self.entities.crowd[i].replan)
    }

    /// Ticks the unit has gone without coming closer to its waypoint (zero for a
    /// unit that is getting on, or has no waypoint).
    pub fn stuck_ticks_of(&self, id: EntityId) -> Option<u32> {
        let i = self.live_index(id)?;
        Some(u32::from(self.entities.crowd[i].quiet))
    }

    /// The formation settings of the world (WP4.6).
    pub fn formation_config(&self) -> FormationConfig {
        self.formations.config
    }

    /// What the formations did so far (WP4.6).
    pub fn formation_stats(&self) -> FormationStats {
        self.formations.stats
    }

    /// The formation a unit marches in (WP4.6): its place in it, what it does this tick and
    /// how the formation is doing. `None` for a unit that is in none, and for a stale id.
    pub fn formation_of(&self, id: EntityId) -> Option<FormationView> {
        let i = self.live_index(id)?;
        self.formations.view(&self.entities, i)
    }

    /// The replan settings of the world (WP4.7).
    pub fn replan_config(&self) -> ReplanConfig {
        self.pathing.replan_config()
    }

    /// The new routes the unit has asked for in its current order because it was
    /// stuck (WP4.7); at most [`ReplanConfig::max`], the order is given up when it
    /// would ask for one more. Back to zero with a new order.
    pub fn replans_of(&self, id: EntityId) -> Option<u8> {
        let i = self.live_index(id)?;
        Some(self.entities.crowd[i].replans)
    }

    /// The tick at which the unit last gave an order up because it was stuck after all
    /// its replans (WP4.7), until a command gives it another order or stops it. `None`
    /// for a unit that has not, or a stale id. In the checksum.
    pub fn gave_up_at(&self, id: EntityId) -> Option<Tick> {
        let i = self.live_index(id)?;
        self.entities.crowd[i].gave_up_at
    }

    /// The point a settled member of a group closes up toward (WP4.4b,
    /// `docs/rules/collision.md`, "Arrival"): the point its order sent it to. `None`
    /// for a unit that is walking, has no group or has been given another order
    /// since.
    pub fn home_of(&self, id: EntityId) -> Option<Vec2> {
        let i = self.live_index(id)?;
        self.entities.crowd[i].home
    }

    pub fn path_stats(&self) -> PathStats {
        self.pathing.stats()
    }

    /// Whether routes are found for movement class `class`: the world has
    /// terrain and a movement table, and some terrain type stops the class.
    pub fn paths_class(&self, class: u16) -> bool {
        self.pathing.paths_class(class)
    }

    /// Plans the whole route of a unit of `class` from `from` to `to` in one
    /// go, with no budget and no unit: the waypoints it would take, in order.
    /// `None` for a class without pathfinding or a point outside the map. It
    /// changes nothing in the world.
    pub fn plan_route(&self, class: u16, from: Vec2, to: Vec2) -> Option<PlannedRoute> {
        self.pathing.plan(class, from, to)
    }

    /// Whether a unit of `class` could walk the straight line between two
    /// positions by the rules routes are pulled taut with (the thick line of
    /// sight of `docs/rules/pathfinding.md`; structures count, mobile units do
    /// not). `false` for a class without pathfinding or a position off the map.
    /// Reads the world only; tests and tools use it to judge routes.
    pub fn sight_clear(&self, class: u16, from: Vec2, to: Vec2) -> bool {
        self.pathing.sight_clear(class, from, to)
    }

    /// How many cells are occupied.
    pub fn occupied_cells(&self) -> usize {
        self.occupancy.count()
    }

    /// The cell a position lies in; `None` outside the map.
    pub fn cell_at(&self, pos: Vec2) -> Option<Cell> {
        let (x, y) = pos.cell();
        let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
        (x < self.config.width && y < self.config.height).then_some(Cell::new(x, y))
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

    /// The destruction settings, if the world was given any.
    pub fn destruction_config(&self) -> Option<DestructionConfig> {
        self.destruction.config
    }

    /// What was destroyed in the last tick, by where it stood (WP5.5). Derived: it is a report, not state.
    pub fn deaths(&self) -> &[Death] {
        &self.destruction.deaths
    }

    /// The wrecks and the rubble that lie in the world, by where they lie (WP5.5).
    pub fn wrecks(&self) -> &[Wreck] {
        &self.destruction.wrecks
    }

    /// What `player` has killed, helped to kill and lost since the world began (WP5.5).
    pub fn tally_of(&self, player: PlayerId) -> Tally {
        self.destruction
            .tally
            .binary_search_by_key(&player, |t| t.0)
            .map_or_else(|_| Tally::default(), |at| self.destruction.tally[at].1)
    }

    /// How many kills the unit `id` has been credited with (WP5.5); `None` if it is gone.
    pub fn kills_of(&self, id: EntityId) -> Option<u16> {
        self.live_index(id).map(|i| self.entities.kills[i])
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

    /// Like [`World::schedule`], for a command that carries the sequence
    /// number its sender gave it (WP3.4). Commands run by `(tick, player,
    /// sequence)`, so two machines that receive the same commands in
    /// different orders still run them in the same order. Refused for a
    /// `tick` that has run, and when the player already has a command for
    /// that tick under that number ([`ScheduleError::DuplicateSequence`]).
    pub fn schedule_sequenced(
        &mut self,
        tick: Tick,
        player: PlayerId,
        sequence: u64,
        command: Command,
    ) -> Result<(), ScheduleError> {
        self.queue
            .schedule_sequenced(self.tick, tick, player, sequence, command)
    }

    // ---- the tick ---------------------------------------------------------

    /// Runs exactly one tick and returns what happened.
    ///
    /// In order: (1) every command scheduled for this tick runs, in
    /// `(player, insertion)` order; (1a) the ore grows back on the cells whose
    /// turn it is (WP7.1, only in a world with ore rules); (2) every live entity runs its behaviour,
    /// in ascending slot order, after recording its previous position and
    /// facing (the path service has served its requests just before); (3) the
    /// crowd pass pushes overlapping bodies apart and looks after the units that
    /// do not get on (WP4.4); (4) the tick counter advances; (5) the checksum of
    /// the new state is computed and appended to the history.
    pub fn step(&mut self) -> StepReport {
        let tick = self.tick;

        let mut executed: u32 = 0;
        let mut ignored: u32 = 0;
        self.placement.rejections.clear();
        while let Some((player, command)) = self.queue.pop_due(tick) {
            if self.apply_from(player, command) {
                executed = executed.saturating_add(1);
            } else {
                ignored = ignored.saturating_add(1);
            }
        }

        // The ore grows back (WP7.1): after the commands of this tick, so a take and the
        // growth of the same cell in one tick are seen in that order.
        if let Some(ore) = &mut self.ore {
            ore.regrow(tick.0);
        }

        // The harvesters and the refinery docks (WP7.2): the docks answer, the harvesters decide,
        // then the decisions are carried out. Before the units move, so the orders it gives are
        // walked in this tick.
        if let Some(mut harvest) = self.harvest.take() {
            harvest.tick(self);
            self.harvest = Some(harvest);
        }

        // The path service first: it gives the requests of units that asked
        // earlier their routes (within the budget), going round the slots.
        self.pathing.serve(&mut self.entities, &self.units);

        let motion = self.motion();
        let ground = Ground {
            terrain: &self.terrain,
            movement: &self.movement,
            occupancy: Some(&self.occupancy),
        };
        // The formations work out where every member's slot is and what it does (WP4.6).
        self.formations
            .update(&mut self.entities, &self.units, &ground, &mut self.pathing);
        behavior::advance_all(
            &mut self.entities,
            &motion,
            &self.units,
            &ground,
            &mut self.pathing,
            tick,
        );
        // Then the bodies are pushed apart, head-on units steer, and units that do
        // not get on are looked after (WP4.4).
        self.crowd.resolve(
            &mut self.entities,
            &ground,
            (motion.pos_min, motion.pos_max),
            &mut self.pathing,
        );
        // The units that choose their own targets look round, from where the bodies ended (WP5.4).
        let bounds = (motion.pos_min, motion.pos_max);
        let aim = self.targeting.config.filter(|_| !self.arsenal.is_empty());
        if let Some(config) = aim {
            let cx = targeting::Context {
                units: &self.units,
                arsenal: &self.arsenal,
                config,
                tick,
                bounds,
                map: (self.config.width, self.config.height),
            };
            self.targeting.update(&mut self.entities, &cx);
        }
        // The turrets turn toward their targets, from where the hulls ended (WP4.5).
        turret::update_all(&mut self.entities, &self.units);
        // The weapons reload, and fire at the victims of attack orders and at what the units chose (WP5.1).
        weapons::update_all(
            &mut self.entities,
            &self.units,
            &mut self.arsenal,
            tick,
            (self.config.width, self.config.height),
            aim.is_some(),
            self.destruction.config.is_some(),
        );
        // What the weapons took to zero is destroyed now, after all the damage of the tick is in (WP5.5).
        self.reap(tick);
        // The units that were hurt turn on their attackers (WP5.4).
        if let Some(config) = aim {
            let mut attacks = std::mem::take(&mut self.arsenal.attacks);
            let cx = targeting::Context {
                units: &self.units,
                arsenal: &self.arsenal,
                config,
                tick,
                bounds,
                map: (self.config.width, self.config.height),
            };
            self.targeting
                .return_fire(&mut self.entities, &cx, &mut attacks);
            self.arsenal.attacks = attacks;
        }

        self.tick = tick.next();
        let checksum = self.checksum();
        self.history.push(checksum);
        StepReport {
            tick,
            checksum,
            commands_executed: executed,
            commands_ignored: ignored,
            deaths: self.destruction.deaths.len() as u32,
            shots: self.arsenal.shots.len() as u32,
            launched: self
                .arsenal
                .shots
                .iter()
                .filter(|s| {
                    self.arsenal
                        .table
                        .weapon(s.weapon)
                        .is_some_and(|w| w.projectile != weapons::ProjectileKind::Instant)
                })
                .count() as u32,
            impacts: self.arsenal.impacts.len() as u32,
            placements_rejected: self.placement.rejections.len() as u32,
        }
    }

    // ---- internals --------------------------------------------------------

    fn ground(&self) -> Ground<'_> {
        Ground {
            terrain: &self.terrain,
            movement: &self.movement,
            occupancy: None,
        }
    }

    fn motion(&self) -> Motion {
        let step = per_tick(self.config.speed);
        let (pos_min, pos_max) = self.pos_range();
        Motion {
            step,
            step_sq: step.square(),
            turn_per_tick: self.config.turn_per_tick,
            pos_min,
            pos_max,
            class: None,
            crowd_wait: if self.crowd.config.enabled {
                self.crowd.config.wait_ticks
            } else {
                0
            },
            slide: self.crowd.config.enabled,
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
    pub(crate) fn pos_range(&self) -> (Vec2, Vec2) {
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
            has_stats: e.def[i] != NO_DEF,
            hp: e.hp[i],
            turret: self
                .stats_at(i)
                .is_some_and(UnitStats::has_turret)
                .then_some(TurretView {
                    prev_facing: e.prev_turret[i],
                    facing: e.turret[i],
                }),
        }
    }

    /// Creates an entity and returns its id, or `None` at the entity limit.
    ///
    /// The slot is the most recently freed one if there is any, otherwise a
    /// new one at the end. The entity starts wandering over the whole map with
    /// a random heading from its own stream.
    pub(crate) fn spawn(&mut self, kind: Kind, pos: Vec2, owner: PlayerId) -> Option<EntityId> {
        self.spawn_as(kind, pos, owner, Behavior::Wander)
    }

    /// [`World::spawn`] with a given starting behaviour.
    fn spawn_as(
        &mut self,
        kind: Kind,
        pos: Vec2,
        owner: PlayerId,
        behavior: Behavior,
    ) -> Option<EntityId> {
        self.spawn_with(kind, pos, owner, behavior, Birth::LEGACY)
    }

    /// Creates an entity: the slot, the random stream and the columns. `birth`
    /// says whether it comes from the unit table (its entry and hit points)
    /// and whether it takes a random facing from its own stream.
    fn spawn_with(
        &mut self,
        kind: Kind,
        pos: Vec2,
        owner: PlayerId,
        behavior: Behavior,
        birth: Birth,
    ) -> Option<EntityId> {
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
        let facing = if birth.random_facing {
            rng.facing()
        } else {
            Facing::default()
        };
        let (pos_min, pos_max) = self.pos_range();
        self.entities.fill(
            index,
            NewEntity {
                kind,
                owner,
                pos: pos.clamp(pos_min, pos_max),
                facing,
                behavior,
                area: self.bounds(),
                rng,
                def: birth.def,
                hp: birth.hp,
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
        // A structure frees all its cells in the same command.
        if self.entities.cell[i] != NO_CELL {
            let anchor = self.occupancy.cell(self.entities.cell[i]);
            let footprint = self.footprint_at(i);
            for cell in footprint_cells(anchor, footprint) {
                if let Some(index) = self.occupancy.index(cell) {
                    self.occupancy.release(index);
                }
            }
            // The cells are free again: the routes and portals around them.
            self.pathing.refresh_cells(
                &self.terrain,
                &self.movement,
                &self.occupancy,
                footprint_cells(anchor, footprint),
            );
        }
        self.entities.clear(i);
        if let Some(next) = self.entities.generation[i].checked_add(1) {
            self.entities.generation[i] = next;
            self.free.push(id.index());
        }
        self.live = self.live.saturating_sub(1);
        true
    }

    /// Places a structure of the legacy kind: an entity that stands still on
    /// `cell` and occupies it until it is despawned. `None` (nothing happens)
    /// if the cell is outside the map, its terrain is not buildable, it is
    /// already occupied, or the entity limit is reached.
    fn place(&mut self, kind: Kind, cell: Cell, owner: PlayerId) -> Option<EntityId> {
        self.place_footprint(kind, cell, Footprint::ONE, owner, Birth::LEGACY)
    }

    /// Places a structure from the unit table. `None` (nothing happens) for a
    /// kind the table does not define, for a kind that is not a structure, and
    /// for everything [`World::place_footprint`] refuses.
    pub(crate) fn place_structure(
        &mut self,
        kind: Kind,
        cell: Cell,
        owner: PlayerId,
    ) -> Option<EntityId> {
        let def = self.units.index_of(kind)?;
        let stats = *self.units.at(def);
        let footprint = stats.footprint?;
        let birth = Birth {
            def: def as u32,
            hp: stats.hp,
            random_facing: false,
        };
        self.place_footprint(kind, cell, footprint, owner, birth)
    }

    /// Places an entity on the `footprint` cells counted from `anchor` (the
    /// corner with the smallest x and y). All or nothing: every cell must be
    /// inside the map, buildable and free, otherwise nothing happens. The
    /// entity stands in the middle of the footprint and its anchor index is
    /// kept in the `cell` column.
    fn place_footprint(
        &mut self,
        kind: Kind,
        anchor: Cell,
        footprint: Footprint,
        owner: PlayerId,
        birth: Birth,
    ) -> Option<EntityId> {
        let anchor_index = self.occupancy.index(anchor)?;
        let (width, height) = (u32::from(footprint.width), u32::from(footprint.height));
        if anchor.x.checked_add(width)? > self.config.width
            || anchor.y.checked_add(height)? > self.config.height
        {
            return None;
        }
        // Check every cell first and keep the indices: nothing below can fail
        // half way, so a refusal never leaves a half-made entity behind.
        let mut indices =
            Vec::with_capacity(usize::from(footprint.width) * usize::from(footprint.height));
        for cell in footprint_cells(anchor, footprint) {
            let index = self.occupancy.index(cell)?;
            if !self.terrain.flags_at(cell)?.buildable || self.occupancy.slot_at(index).is_some() {
                return None;
            }
            indices.push(index);
        }
        // The middle of the footprint: the anchor corner plus half its size.
        let pos = Vec2::new(
            Fx::from_i64(i64::from(anchor.x)) + Fx::from_ratio(i64::from(width), 2),
            Fx::from_i64(i64::from(anchor.y)) + Fx::from_ratio(i64::from(height), 2),
        );
        // The only step that can still refuse (the entity limit); it comes
        // before anything is claimed.
        let id = self.spawn_with(kind, pos, owner, Behavior::Idle, birth)?;
        for index in indices {
            let claimed = self.occupancy.claim(index, id.index());
            debug_assert!(claimed, "the cells were free a moment ago");
        }
        self.entities.cell[id.index() as usize] = anchor_index;
        // A structure built on rubble or a wreck clears it away.
        self.destruction.clear_under(anchor, footprint);
        // The cells are taken: ground units can no longer cross them.
        self.pathing.refresh_cells(
            &self.terrain,
            &self.movement,
            &self.occupancy,
            footprint_cells(anchor, footprint),
        );
        Some(id)
    }

    /// Spawns a mobile unit or an object from the unit table at `pos`. `None`
    /// (nothing happens) for a kind the table does not define, for a
    /// structure (that is [`Command::PlaceStructure`]), for a position outside
    /// the map or on a cell a structure occupies, for a mobile unit on a cell
    /// its movement class cannot enter (WP4.1), and at the entity limit.
    /// The unit occupies no cell and starts idle, a mobile one with a random
    /// facing, an object with facing zero.
    fn spawn_unit(&mut self, kind: Kind, pos: Vec2, owner: PlayerId) -> Option<EntityId> {
        let def = self.units.index_of(kind)?;
        let stats = *self.units.at(def);
        if stats.class() == UnitClass::Structure {
            return None;
        }
        let cell = self.cell_at(pos)?;
        if self.is_occupied(cell) {
            return None;
        }
        if let Some(class) = stats.movement_class
            && !self.passable(class, cell)
        {
            return None;
        }
        let birth = Birth {
            def: def as u32,
            hp: stats.hp,
            random_facing: stats.class() == UnitClass::Mobile,
        };
        self.spawn_with(kind, pos, owner, Behavior::Idle, birth)
    }

    /// The unit table entry of the entity in slot `i`, if it has one.
    pub(crate) fn stats_at(&self, i: usize) -> Option<&UnitStats> {
        let def = self.entities.def[i];
        (def != NO_DEF).then(|| self.units.at(def as usize))
    }

    /// The footprint of the structure in slot `i`: its table entry's, or one
    /// cell for a legacy structure.
    pub(crate) fn footprint_at(&self, i: usize) -> Footprint {
        self.stats_at(i)
            .and_then(|s| s.footprint)
            .unwrap_or(Footprint::ONE)
    }

    /// Whether the entity in slot `i` can never move: a structure (it
    /// occupies a cell) or an object of the unit table. They do not wander and
    /// ignore `MoveTo`.
    fn is_immobile(&self, i: usize) -> bool {
        self.entities.cell[i] != NO_CELL
            || self
                .stats_at(i)
                .is_some_and(|s| s.class() != UnitClass::Mobile)
    }

    /// Whether unit `i` can take `order` (WP3.4): it can move, and the order
    /// makes sense for it. `Idle` is not an order to give.
    fn accepts_order(&self, i: usize, order: Order) -> bool {
        if self.is_immobile(i) {
            return false;
        }
        let def = self.entities.def[i];
        if self.motion().of_entity(def, &self.units).step <= Fx::ZERO {
            return false;
        }
        // A unit at zero hit points takes no orders: it stands where it fell until it is removed
        // (WP5.5). Only in a world with targeting, which is what looks after such a unit.
        if self.targeting.config.is_some()
            && self.entities.hp[i] == 0
            && self.stats_at(i).is_some_and(|s| s.hp > 0)
        {
            return false;
        }
        let itself = EntityId::new(i as u32, self.entities.generation[i]);
        match order {
            // `Pursue` is what a unit gives itself (WP5.4), never a command.
            Order::Idle | Order::Pursue(_) => false,
            Order::Move(_) | Order::AttackMove(_) | Order::Scatter | Order::HoldPosition => true,
            Order::Guard(GuardTarget::Point(_)) => true,
            Order::Guard(GuardTarget::Entity(watched)) => {
                watched != itself && self.live_index(watched).is_some()
            }
            Order::Attack(victim) => {
                victim != itself
                    && self.live_index(victim).is_some()
                    && orders::weapon_range_of(&self.entities, i, &self.units) > Fx::ZERO
            }
        }
    }

    /// Runs one command. True if it changed anything.
    pub(crate) fn apply(&mut self, command: Command) -> bool {
        match command {
            Command::Spawn { kind, pos, owner } => self.spawn(kind, pos, owner).is_some(),
            Command::Place { kind, cell, owner } => self.place(kind, cell, owner).is_some(),
            Command::SpawnUnit { kind, pos, owner } => self.spawn_unit(kind, pos, owner).is_some(),
            Command::PlaceStructure { kind, cell, owner } => {
                self.place_structure(kind, cell, owner).is_some()
            }
            // Needs to know who sent it: `World::apply_from` runs it (placement.rs).
            Command::BuildStructure { .. } => false,
            Command::Despawn { id } => self.despawn(id),
            Command::Order { ids, order, queued } => {
                let (pos_min, pos_max) = self.pos_range();
                let order = match order {
                    Order::Move(p) => Order::Move(p.clamp(pos_min, pos_max)),
                    Order::AttackMove(p) => Order::AttackMove(p.clamp(pos_min, pos_max)),
                    Order::Guard(GuardTarget::Point(p)) => {
                        Order::Guard(GuardTarget::Point(p.clamp(pos_min, pos_max)))
                    }
                    other => other,
                };
                let mut changed = false;
                // The units that start walking to a point now: a group order
                // gives them a flowfield (WP4.3) when there are enough of them.
                let group_point = match order {
                    Order::Move(p) | Order::AttackMove(p) => Some(p),
                    _ => None,
                };
                let mut started: Vec<usize> = Vec::new();
                for id in ids {
                    if let Some(i) = self.live_index(id)
                        && self.accepts_order(i, order)
                    {
                        let starts_now = !queued || self.entities.order[i] == Order::Idle;
                        if orders::give(
                            &mut self.entities,
                            i,
                            order,
                            queued,
                            self.tick,
                            (pos_min, pos_max),
                        ) {
                            changed = true;
                            if starts_now && group_point.is_some() {
                                started.push(i);
                            }
                        }
                    }
                }
                if let Some(point) = group_point {
                    started.sort_unstable();
                    started.dedup();
                    // A move order that sends enough units across the same ground forms a
                    // formation (WP4.6); the arrival slots of the goal then stand across the
                    // way the group came.
                    let mut formed: Vec<(usize, Facing)> = Vec::new();
                    if let Order::Move(_) = order {
                        let ground = Ground {
                            terrain: &self.terrain,
                            movement: &self.movement,
                            occupancy: Some(&self.occupancy),
                        };
                        formed = self.formations.create(
                            &mut self.entities,
                            &self.units,
                            &ground,
                            &self.pathing,
                            &started,
                            point,
                        );
                    }
                    self.pathing.group_order(
                        &mut self.entities,
                        &self.units,
                        &started,
                        point,
                        &formed,
                    );
                }
                changed
            }
            Command::Stop { ids } => {
                let mut changed = false;
                for id in ids {
                    if let Some(i) = self.live_index(id)
                        && !self.is_immobile(i)
                    {
                        orders::forget_give_up(&mut self.entities, i);
                        orders::clear(&mut self.entities, i);
                        self.entities.turret_target[i] = None;
                        changed = true;
                    }
                }
                changed
            }
            Command::TakeOre { cell, amount } => self
                .ore
                .as_mut()
                .is_some_and(|ore| ore.take(cell, amount) > 0),
            Command::AimTurret { ids, target } => {
                let mut changed = false;
                for id in ids {
                    if let Some(i) = self.live_index(id)
                        && turret::has_turret(&self.entities, i, &self.units)
                    {
                        self.entities.turret_target[i] =
                            target.filter(|&t| t != id && self.live_index(t).is_some());
                        changed = true;
                    }
                }
                changed
            }
            Command::SetWanderArea { ids, area } => {
                let area = area.clamped_to(self.bounds());
                let mut changed = false;
                for id in ids {
                    if let Some(i) = self.live_index(id)
                        && !self.is_immobile(i)
                    {
                        orders::forget_give_up(&mut self.entities, i);
                        orders::clear(&mut self.entities, i);
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
                    if let Some(i) = self.live_index(id)
                        && !self.is_immobile(i)
                    {
                        orders::forget_give_up(&mut self.entities, i);
                        orders::clear(&mut self.entities, i);
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

/// How a new entity relates to the unit table.
#[derive(Copy, Clone, Debug)]
struct Birth {
    /// Position in the unit table, or [`NO_DEF`].
    def: u32,
    /// Hit points at birth.
    hp: u32,
    /// Whether the facing is drawn from the entity's own stream (units; the
    /// legacy entities) or left at zero (structures made from the table, whose
    /// look does not turn).
    random_facing: bool,
}

impl Birth {
    /// An entity that is not made from the unit table: what `Spawn` and
    /// `Place` have always made.
    const LEGACY: Birth = Birth {
        def: NO_DEF,
        hp: 0,
        random_facing: true,
    };
}

/// The cells of a footprint counted from its anchor, row by row.
fn footprint_cells(anchor: Cell, footprint: Footprint) -> impl Iterator<Item = Cell> {
    let (w, h) = (u32::from(footprint.width), u32::from(footprint.height));
    (anchor.y..anchor.y.saturating_add(h))
        .flat_map(move |y| (anchor.x..anchor.x.saturating_add(w)).map(move |x| Cell::new(x, y)))
}

#[cfg(test)]
mod tests;
