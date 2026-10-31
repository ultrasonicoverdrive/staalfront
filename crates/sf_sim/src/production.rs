//! Production (WP6.5, `docs/rules/production.md`): every factory builds the units its owner
//! queues, one at a time, pays for them while they are built, puts them on a free spot by its
//! exit and sends them to a rally point.
//!
//! **The data.** [`ProductionTable`] says what each producible unit kind costs, how long it takes,
//! which factory function builds it (`sf_data` derives that from the role's category), who may
//! have it and which structures must stand; and which structure comes with a free unit (the
//! refinery's harvester). [`ProductionRules`] holds the two tuning numbers (queue length, exit
//! search radius). `sf_bridge` converts them from the data files; the world takes them once
//! ([`World::with_production`]) after the unit and building tables.
//!
//! **The state.** One [`Factory`] record per structure that has a queue, a rally point or a unit
//! on its way out, kept sorted by slot. A record whose structure is gone (despawned, destroyed,
//! slot reused for something else) is dropped at the start of the next production tick, queue and
//! progress with it and without a refund (what was paid stays spent, as with a structure destroyed
//! half built).
//!
//! **A tick**, after the commands, in two passes over the factories in ascending slot order:
//! (1) the front item of each queue pays for and gains its progress; (2) the front items that are
//! finished are put on the map. Money is taken per tick with the same exact rule as a structure
//! (`C * (k + 1) / T - C * k / T`, [`crate::tick_cost`]), only on a scale of thousandths of a tick
//! so that the speed factor can slow it ([`World::production_speed_permille`], fixed at full speed
//! for now; the hook of power, WP6.4). Without the money the item waits and nothing is paid.

use core::fmt;

use sf_math::hash::Fnv1a64;
use sf_math::{Fx, Vec2};

use crate::buildings::{BuildingFunction, BuildingTable};
use crate::command::Command;
use crate::entities::NO_DEF;
use crate::ids::{EntityId, Kind, PlayerId, Tick};
use crate::orders::Order;
use crate::placement::exit_cell;
use crate::terrain::{Cell, NO_CELL};
use crate::units::{UnitClass, UnitTable};
use crate::world::{ConfigError, World};

/// Written first into the digest; bump when its layout changes.
const DIGEST_TAG: &[u8] = b"sf_sim/production/v1";

/// The production speed of a factory that is not slowed: thousandths of a tick of progress per
/// tick. The hook of power (WP6.4): [`World::production_speed_permille`] returns this for now.
pub const FULL_SPEED_PERMILLE: u32 = 1000;
/// The most items a queue may be given a room for (`queue_limit`).
pub const MAX_QUEUE_LIMIT: u8 = 20;
/// The widest search for a free spot, in cells round the exit cell (`exit_radius`).
pub const MAX_EXIT_RADIUS: u8 = 8;

/// The tuning numbers of production, from `assets/config/production.ron`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ProductionRules {
    /// Most items in one queue, the one being built included: 1 to [`MAX_QUEUE_LIMIT`].
    pub queue_limit: u8,
    /// How far, in cells (chessboard distance), from the exit cell a finished unit looks for a
    /// free spot: 1 to [`MAX_EXIT_RADIUS`].
    pub exit_radius: u8,
}

impl ProductionRules {
    /// Whether the numbers are inside their bounds.
    pub const fn is_valid(&self) -> bool {
        self.queue_limit >= 1
            && self.queue_limit <= MAX_QUEUE_LIMIT
            && self.exit_radius >= 1
            && self.exit_radius <= MAX_EXIT_RADIUS
    }
}

/// One unit kind a factory can build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitBuild {
    /// The unit kind.
    pub kind: Kind,
    /// The function of the factory that builds it.
    pub factory: BuildingFunction,
    /// Price in credits, above zero.
    pub cost: u32,
    /// Ticks to build, above zero.
    pub build_ticks: u32,
    /// Bit mask of the factions that may have it.
    pub factions: u8,
    /// Structures its owner must have standing, in any order.
    pub requires: Vec<Kind>,
}

/// Why a production table was refused.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ProductionTableError {
    /// Two entries have the same unit kind.
    DuplicateKind(Kind),
    /// A unit with a price of zero or no build time.
    NoCostOrTime(Kind),
    /// The factory function of a unit is not a factory.
    NotAFactory(Kind),
    /// A unit that nobody may have (no faction bit).
    NoFaction(Kind),
    /// The same kind comes with two structures, or a structure lists two units.
    DuplicateComesWith(Kind),
}

impl fmt::Display for ProductionTableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProductionTableError::DuplicateKind(k) => {
                write!(f, "unit kind {} is listed twice", k.0)
            }
            ProductionTableError::NoCostOrTime(k) => {
                write!(f, "unit kind {} has no price or no build time", k.0)
            }
            ProductionTableError::NotAFactory(k) => {
                write!(
                    f,
                    "unit kind {} names a function that is not a factory",
                    k.0
                )
            }
            ProductionTableError::NoFaction(k) => {
                write!(f, "unit kind {} is open to no faction", k.0)
            }
            ProductionTableError::DuplicateComesWith(k) => {
                write!(f, "structure kind {} comes with more than one unit", k.0)
            }
        }
    }
}

impl std::error::Error for ProductionTableError {}

/// What can be built, sorted by unit kind, and which structure comes with a free unit.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct ProductionTable {
    builds: Vec<UnitBuild>,
    /// `(structure kind, unit kind)`, ascending by structure.
    comes_with: Vec<(Kind, Kind)>,
    digest: u64,
}

impl fmt::Debug for ProductionTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProductionTable")
            .field("units", &self.builds.len())
            .field("comes_with", &self.comes_with.len())
            .finish()
    }
}

impl ProductionTable {
    /// Checks the entries, sorts them by kind (each `requires` list too) and takes their digest.
    /// The order of the input does not matter. Whether the kinds exist in the unit and building
    /// tables is checked by [`World::with_production`].
    pub fn new(
        mut builds: Vec<UnitBuild>,
        mut comes_with: Vec<(Kind, Kind)>,
    ) -> Result<ProductionTable, ProductionTableError> {
        builds.sort_by_key(|b| b.kind);
        comes_with.sort();
        for pair in builds.windows(2) {
            if pair[0].kind == pair[1].kind {
                return Err(ProductionTableError::DuplicateKind(pair[0].kind));
            }
        }
        for pair in comes_with.windows(2) {
            if pair[0].0 == pair[1].0 {
                return Err(ProductionTableError::DuplicateComesWith(pair[0].0));
            }
        }
        for b in &mut builds {
            b.requires.sort();
            b.requires.dedup();
            if b.cost == 0 || b.build_ticks == 0 {
                return Err(ProductionTableError::NoCostOrTime(b.kind));
            }
            if !b.factory.is_factory() {
                return Err(ProductionTableError::NotAFactory(b.kind));
            }
            if b.factions == 0 {
                return Err(ProductionTableError::NoFaction(b.kind));
            }
        }
        let mut h = Fnv1a64::new();
        h.write_bytes(DIGEST_TAG);
        h.write_u64(builds.len() as u64);
        for b in &builds {
            h.write_bytes(&b.kind.0.to_le_bytes());
            h.write_u8(b.factory.number());
            h.write_u32(b.cost);
            h.write_u32(b.build_ticks);
            h.write_u8(b.factions);
            h.write_u64(b.requires.len() as u64);
            for r in &b.requires {
                h.write_bytes(&r.0.to_le_bytes());
            }
        }
        h.write_u64(comes_with.len() as u64);
        for &(s, u) in &comes_with {
            h.write_bytes(&s.0.to_le_bytes());
            h.write_bytes(&u.0.to_le_bytes());
        }
        let digest = h.finish();
        Ok(ProductionTable {
            builds,
            comes_with,
            digest,
        })
    }

    /// The entry of unit kind `kind`.
    pub fn get(&self, kind: Kind) -> Option<&UnitBuild> {
        self.builds
            .binary_search_by_key(&kind, |b| b.kind)
            .ok()
            .map(|i| &self.builds[i])
    }

    /// Every producible unit, ascending by kind.
    pub fn iter(&self) -> impl Iterator<Item = &UnitBuild> {
        self.builds.iter()
    }

    /// The unit that comes free with a finished structure of kind `structure`.
    pub fn comes_with(&self, structure: Kind) -> Option<Kind> {
        self.comes_with
            .binary_search_by_key(&structure, |&(s, _)| s)
            .ok()
            .map(|i| self.comes_with[i].1)
    }

    /// Every `(structure, unit)` pair that comes with a structure, ascending by structure.
    pub fn comes_with_all(&self) -> &[(Kind, Kind)] {
        &self.comes_with
    }

    /// A hash of every entry.
    pub fn digest(&self) -> u64 {
        self.digest
    }
}

/// Why a production command was refused.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ProductionError {
    /// The factory id is stale or not a structure.
    NoSuchFactory,
    /// The sender does not own the factory.
    NotYours,
    /// The structure's function is not a factory.
    NotAFactory,
    /// The factory is under construction or has no hit points left.
    NotStanding,
    /// The kind is not a producible unit.
    UnknownUnit,
    /// The unit is built by another kind of factory.
    WrongFactory,
    /// The sender has no faction.
    NoFaction,
    /// The unit is not open to the sender's faction.
    WrongFaction,
    /// A structure the unit needs does not stand (the first missing kind).
    MissingPrerequisite(Kind),
    /// The queue holds as many items as `queue_limit` allows.
    QueueFull,
    /// A cancel for a place in the queue that holds nothing, or a unit that came for free.
    NothingToCancel,
    /// A rally point outside the map.
    OffMap,
}

impl fmt::Display for ProductionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProductionError::NoSuchFactory => f.write_str("no such factory"),
            ProductionError::NotYours => f.write_str("not your factory"),
            ProductionError::NotAFactory => f.write_str("not a factory"),
            ProductionError::NotStanding => f.write_str("the factory is not standing"),
            ProductionError::UnknownUnit => f.write_str("that unit cannot be built"),
            ProductionError::WrongFactory => f.write_str("another kind of factory builds that"),
            ProductionError::NoFaction => f.write_str("the player has no faction"),
            ProductionError::WrongFaction => f.write_str("not available to this faction"),
            ProductionError::MissingPrerequisite(k) => write!(f, "needs structure {} first", k.0),
            ProductionError::QueueFull => f.write_str("the queue is full"),
            ProductionError::NothingToCancel => f.write_str("nothing to cancel there"),
            ProductionError::OffMap => f.write_str("outside the map"),
        }
    }
}

impl std::error::Error for ProductionError {}

/// A refused production command, kept until the next step.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ProductionRejection {
    /// The tick the command ran in.
    pub tick: Tick,
    /// Who sent it.
    pub player: PlayerId,
    /// Why not.
    pub reason: ProductionError,
}

/// A unit that left a factory in the last step. Replaced by every step.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Produced {
    /// The tick whose step put it on the map.
    pub tick: Tick,
    /// The factory it came out of.
    pub factory: EntityId,
    /// The unit.
    pub unit: EntityId,
    /// Its kind.
    pub kind: Kind,
    /// Its owner.
    pub owner: PlayerId,
    /// The cell it stands on.
    pub cell: Cell,
}

/// What a factory is doing, for the readout. Derived from the last tick, not hashed.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FactoryStatus {
    /// Nothing queued.
    Idle,
    /// The front item gains progress.
    Building,
    /// The owner cannot pay for the next tick; nothing is taken and nothing advances.
    WaitingForMoney,
    /// The front item is finished but no spot near the exit is free: the queue holds.
    WaitingForExit,
    /// A structure the front unit needs no longer stands.
    MissingPrerequisite(Kind),
    /// The production speed is zero (the power hook).
    Stopped,
    /// The factory is not standing (under construction, or no hit points left): nothing happens.
    NotStanding,
}

/// One item of a queue.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct QueueItem {
    /// The unit.
    pub kind: Kind,
    /// Comes for free with a structure (the refinery's harvester): no price, no build time.
    pub free: bool,
}

/// The queue of one structure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Factory {
    pub(crate) id: EntityId,
    /// Where finished units walk to; `None`: they stay on their spot.
    pub(crate) rally: Option<Cell>,
    /// The items, the one being built first.
    pub(crate) items: Vec<QueueItem>,
    /// Progress of the front item in thousandths of a tick.
    pub(crate) done: u32,
    /// What it did last tick. Not hashed.
    pub(crate) status: FactoryStatus,
}

/// The state of production in a world.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Production {
    pub(crate) rules: ProductionRules,
    pub(crate) table: ProductionTable,
    /// Ascending by slot index of the structure.
    pub(crate) factories: Vec<Factory>,
    /// The commands refused in the last step; cleared at the start of the next.
    pub(crate) rejections: Vec<ProductionRejection>,
    /// The units put on the map in the last step.
    pub(crate) produced: Vec<Produced>,
}

impl Production {
    fn position(&self, slot: u32) -> Result<usize, usize> {
        self.factories.binary_search_by_key(&slot, |f| f.id.index())
    }

    /// The record of the structure `id`, if it has one.
    pub(crate) fn get(&self, id: EntityId) -> Option<&Factory> {
        let k = self.position(id.index()).ok()?;
        (self.factories[k].id == id).then(|| &self.factories[k])
    }

    /// The record of `id`, made if it has none. A record of an older entity in the same slot is
    /// replaced.
    fn get_or_insert(&mut self, id: EntityId) -> &mut Factory {
        let fresh = Factory {
            id,
            rally: None,
            items: Vec::new(),
            done: 0,
            status: FactoryStatus::Idle,
        };
        let k = match self.position(id.index()) {
            Ok(k) => {
                if self.factories[k].id != id {
                    self.factories[k] = fresh;
                }
                k
            }
            Err(k) => {
                self.factories.insert(k, fresh);
                k
            }
        };
        &mut self.factories[k]
    }

    /// Writes the production section of the checksum.
    pub(crate) fn write_hash(&self, h: &mut Fnv1a64) {
        h.write_u8(self.rules.queue_limit);
        h.write_u8(self.rules.exit_radius);
        h.write_u64(self.table.digest());
        h.write_u64(self.factories.len() as u64);
        for f in &self.factories {
            h.write_u32(f.id.index());
            h.write_u32(f.id.generation());
            match f.rally {
                Some(c) => {
                    h.write_u8(1);
                    h.write_u32(c.x);
                    h.write_u32(c.y);
                }
                None => h.write_u8(0),
            }
            h.write_u32(f.done);
            h.write_u64(f.items.len() as u64);
            for item in &f.items {
                h.write_bytes(&item.kind.0.to_le_bytes());
                h.write_u8(u8::from(item.free));
            }
        }
    }
}

/// What a unit of price `cost` and `ticks` build ticks has cost when `done` thousandths of a tick
/// of it are done: `cost * done / (ticks * 1000)`, whole credits. The tick that takes the
/// progress from `a` to `b` costs `paid_at(b) - paid_at(a)`, so the costs add up to exactly the
/// price, and at full speed (1000 a tick) each tick costs what [`crate::tick_cost`] says.
pub fn paid_at(cost: u32, ticks: u32, done: u32) -> u64 {
    if ticks == 0 {
        return 0;
    }
    u64::from(cost) * u64::from(done) / (u64::from(ticks) * 1000)
}

/// A read-only view of a factory's queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactoryView {
    /// The items, the one being built first.
    pub queue: Vec<QueueItem>,
    /// Progress of the front item in thousandths (0 to 1000); zero for an empty queue.
    pub front_permille: u16,
    /// Ticks the front item takes in all; zero for an empty queue or a free unit.
    pub front_ticks: u32,
    /// What it is doing.
    pub status: FactoryStatus,
    /// The rally point.
    pub rally: Option<Cell>,
    /// Credits paid for the front item so far.
    pub front_paid: u64,
}

impl World {
    /// The same world with production (WP6.5, `docs/rules/production.md`): factories build the
    /// units of `table` from queues. Call it after the unit table and the building table and before
    /// the world starts; refused with [`ConfigError::Production`] otherwise, for invalid rules,
    /// for a unit that is not a mobile kind of the unit table, for a unit whose factory function
    /// no structure of the building table has, for a prerequisite that is not a structure someone
    /// can build, for a `comes_with` structure without an exit side or a unit that is not mobile,
    /// and given twice. A later [`World::with_units`] or [`World::with_buildings`] is refused with
    /// [`ConfigError::Production`] too, since the table froze kinds from them. A world without
    /// production hashes as before.
    pub fn with_production(
        mut self,
        rules: ProductionRules,
        table: ProductionTable,
    ) -> Result<World, ConfigError> {
        if self.tick != Tick::ZERO
            || self.entities.len() != 0
            || !self.queue.is_empty()
            || self.production.is_some()
            || !rules.is_valid()
            || self.units.is_empty()
            || self.buildings.is_empty()
            || !table_fits(&table, &self.units, &self.buildings)
        {
            return Err(ConfigError::Production);
        }
        self.production = Some(Box::new(Production {
            rules,
            table,
            factories: Vec::new(),
            rejections: Vec::new(),
            produced: Vec::new(),
        }));
        Ok(self)
    }

    /// The tuning numbers of production; `None` for a world without it.
    pub fn production_rules(&self) -> Option<ProductionRules> {
        self.production.as_ref().map(|p| p.rules)
    }

    /// The production table; `None` for a world without production.
    pub fn production_table(&self) -> Option<&ProductionTable> {
        self.production.as_ref().map(|p| &p.table)
    }

    /// The production commands refused in the last step, in the order they ran.
    pub fn production_rejections(&self) -> &[ProductionRejection] {
        self.production.as_ref().map_or(&[], |p| &p.rejections)
    }

    /// The units put on the map in the last step, in the order they were. Replaced by every step.
    pub fn produced(&self) -> &[Produced] {
        self.production.as_ref().map_or(&[], |p| &p.produced)
    }

    /// The queue of the structure `id`, if it has a record (a queue, a rally point, or a unit on
    /// its way out).
    pub fn production_of(&self, id: EntityId) -> Option<FactoryView> {
        let p = self.production.as_ref()?;
        let f = p.get(id)?;
        let front = f.items.first();
        let build = front.and_then(|i| (!i.free).then(|| p.table.get(i.kind)).flatten());
        let (front_permille, front_ticks, front_paid) = match build {
            Some(b) => {
                let total = u64::from(b.build_ticks) * 1000;
                let permille = (u64::from(f.done) * 1000 / total).min(1000) as u16;
                (
                    permille,
                    b.build_ticks,
                    paid_at(b.cost, b.build_ticks, f.done),
                )
            }
            None => (u16::from(front.is_some()) * 1000, 0, 0),
        };
        Some(FactoryView {
            queue: f.items.clone(),
            front_permille,
            front_ticks,
            status: f.status,
            rally: f.rally,
            front_paid,
        })
    }

    /// The factories that have a record, in ascending slot order.
    pub fn factories(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.production
            .iter()
            .flat_map(|p| p.factories.iter().map(|f| f.id))
    }

    /// How fast the factories of `player` build, in thousandths of a tick of progress per tick.
    /// **The hook of power (WP6.4):** fixed at [`FULL_SPEED_PERMILLE`] until power exists. Zero
    /// stops production without losing progress; the price follows the progress, so a slow
    /// factory pays slowly.
    pub fn production_speed_permille(&self, _player: PlayerId) -> u32 {
        FULL_SPEED_PERMILLE
    }

    /// Whether `player` has a standing structure of `kind`.
    fn has_standing(&self, player: PlayerId, kind: Kind) -> bool {
        (0..self.entities.len()).any(|i| {
            self.entities.alive[i]
                && self.entities.owner[i] == player
                && self.entities.kind[i] == kind
                && self.entities.cell[i] != NO_CELL
                && self.is_standing(i)
        })
    }

    /// The first structure of `requires` that `player` does not have standing.
    fn missing_prerequisite(&self, player: PlayerId, requires: &[Kind]) -> Option<Kind> {
        requires
            .iter()
            .copied()
            .find(|&k| !self.has_standing(player, k))
    }

    /// Checks a queue command against the factory: the live index of its slot.
    fn production_factory(
        &self,
        player: PlayerId,
        factory: EntityId,
    ) -> Result<(usize, BuildingFunction), ProductionError> {
        let i = self
            .live_index(factory)
            .filter(|&i| self.entities.cell[i] != NO_CELL)
            .ok_or(ProductionError::NoSuchFactory)?;
        if self.entities.owner[i] != player {
            return Err(ProductionError::NotYours);
        }
        let function = self
            .buildings
            .get(self.entities.kind[i])
            .map_or(BuildingFunction::None, |b| b.function);
        if !function.is_factory() {
            return Err(ProductionError::NotAFactory);
        }
        if !self.is_standing(i) {
            return Err(ProductionError::NotStanding);
        }
        Ok((i, function))
    }

    fn queue_unit(
        &mut self,
        player: PlayerId,
        factory: EntityId,
        kind: Kind,
    ) -> Result<(), ProductionError> {
        let (_, function) = self.production_factory(player, factory)?;
        let Some(p) = self.production.as_deref() else {
            return Err(ProductionError::UnknownUnit);
        };
        let build = p.table.get(kind).ok_or(ProductionError::UnknownUnit)?;
        if build.factory != function {
            return Err(ProductionError::WrongFactory);
        }
        let bit = self.faction_of(player).ok_or(ProductionError::NoFaction)?;
        if build.factions & bit == 0 {
            return Err(ProductionError::WrongFaction);
        }
        if let Some(missing) = self.missing_prerequisite(player, &build.requires) {
            return Err(ProductionError::MissingPrerequisite(missing));
        }
        let limit = usize::from(p.rules.queue_limit);
        let len = p.get(factory).map_or(0, |f| f.items.len());
        if len >= limit {
            return Err(ProductionError::QueueFull);
        }
        if let Some(p) = self.production.as_deref_mut() {
            p.get_or_insert(factory)
                .items
                .push(QueueItem { kind, free: false });
        }
        Ok(())
    }

    fn cancel_unit(
        &mut self,
        player: PlayerId,
        factory: EntityId,
        index: u8,
    ) -> Result<(), ProductionError> {
        self.production_factory(player, factory)?;
        let Some(p) = self.production.as_deref_mut() else {
            return Err(ProductionError::NothingToCancel);
        };
        let table = &p.table;
        let Some(f) = p.factories.iter_mut().find(|f| f.id == factory) else {
            return Err(ProductionError::NothingToCancel);
        };
        let index = usize::from(index);
        match f.items.get(index) {
            Some(item) if !item.free => {}
            _ => return Err(ProductionError::NothingToCancel),
        }
        let item = f.items.remove(index);
        let mut refund = 0;
        if index == 0 {
            if let Some(b) = table.get(item.kind) {
                refund = paid_at(b.cost, b.build_ticks, f.done);
            }
            f.done = 0;
        }
        if refund > 0 {
            self.refund(player, refund);
        }
        Ok(())
    }

    fn set_rally(
        &mut self,
        player: PlayerId,
        factory: EntityId,
        cell: Option<Cell>,
    ) -> Result<(), ProductionError> {
        self.production_factory(player, factory)?;
        if let Some(c) = cell
            && (c.x >= self.config.width || c.y >= self.config.height)
        {
            return Err(ProductionError::OffMap);
        }
        if let Some(p) = self.production.as_deref_mut() {
            p.get_or_insert(factory).rally = cell;
        }
        Ok(())
    }

    /// Runs a production command of `player`. False when there are no production rules, and
    /// when it is refused (then it is listed in [`World::production_rejections`]).
    pub(crate) fn apply_production(&mut self, player: PlayerId, command: Command) -> bool {
        if self.production.is_none() {
            return false;
        }
        let result = match command {
            Command::QueueUnit { factory, kind } => self.queue_unit(player, factory, kind),
            Command::CancelUnit { factory, index } => self.cancel_unit(player, factory, index),
            Command::SetRally { factory, cell } => self.set_rally(player, factory, cell),
            _ => return false,
        };
        match result {
            Ok(()) => true,
            Err(reason) => {
                let tick = self.tick;
                if let Some(p) = self.production.as_deref_mut() {
                    p.rejections.push(ProductionRejection {
                        tick,
                        player,
                        reason,
                    });
                }
                false
            }
        }
    }

    /// The production tick: see the module documentation. Runs after the commands of the tick.
    pub(crate) fn advance_production(&mut self) {
        let Some(mut p) = self.production.take() else {
            return;
        };
        p.produced.clear();

        // The structures that came with a unit: a finished structure with a free unit puts it
        // in the queue of its own (no price, no build time).
        for c in &self.completions {
            if let Some(unit) = p.table.comes_with(c.kind) {
                p.get_or_insert(c.id).items.push(QueueItem {
                    kind: unit,
                    free: true,
                });
            }
        }

        // A factory that is gone loses its queue.
        p.factories.retain(|f| self.live_index(f.id).is_some());

        // Pass 1: the front item of each queue pays and gains progress.
        for f in &mut p.factories {
            let Some(i) = self.live_index(f.id) else {
                continue;
            };
            let Some(&front) = f.items.first() else {
                f.status = FactoryStatus::Idle;
                continue;
            };
            if !self.is_standing(i) {
                f.status = FactoryStatus::NotStanding;
                continue;
            }
            if front.free {
                f.status = FactoryStatus::WaitingForExit;
                continue;
            }
            let Some(b) = p.table.get(front.kind) else {
                f.status = FactoryStatus::Idle;
                continue;
            };
            let owner = self.entities.owner[i];
            let total = b.build_ticks.saturating_mul(1000);
            if f.done >= total {
                f.status = FactoryStatus::WaitingForExit;
                continue;
            }
            if let Some(missing) = self.missing_prerequisite(owner, &b.requires) {
                f.status = FactoryStatus::MissingPrerequisite(missing);
                continue;
            }
            let speed = self.production_speed_permille(owner);
            if speed == 0 {
                f.status = FactoryStatus::Stopped;
                continue;
            }
            let next = f.done.saturating_add(speed).min(total);
            let charge =
                paid_at(b.cost, b.build_ticks, next) - paid_at(b.cost, b.build_ticks, f.done);
            if charge > 0 && self.money.is_some() && !self.pay(owner, charge) {
                f.status = FactoryStatus::WaitingForMoney;
                continue;
            }
            f.done = next;
            f.status = if f.done >= total {
                FactoryStatus::WaitingForExit
            } else {
                FactoryStatus::Building
            };
        }

        // Pass 2: the finished front items are put on the map, in slot order. One that finds no
        // free spot stays where it is: the queue holds.
        for k in 0..p.factories.len() {
            if p.factories[k].status != FactoryStatus::WaitingForExit {
                continue;
            }
            let id = p.factories[k].id;
            let Some(&front) = p.factories[k].items.first() else {
                continue;
            };
            let Some(unit) = self.put_on_the_map(id, front.kind, p.rules.exit_radius) else {
                continue;
            };
            let f = &mut p.factories[k];
            f.items.remove(0);
            f.done = 0;
            f.status = if f.items.is_empty() {
                FactoryStatus::Idle
            } else {
                FactoryStatus::Building
            };
            let rally = if front.free { None } else { f.rally };
            let cell = self.cell_at(self.entities.pos[unit.index() as usize]);
            p.produced.push(Produced {
                tick: self.tick,
                factory: id,
                unit,
                kind: front.kind,
                owner: self.entities.owner[unit.index() as usize],
                cell: cell.unwrap_or_default(),
            });
            if let Some(c) = rally {
                let half = Fx::from_ratio(1, 2);
                let target = Vec2::new(
                    Fx::from_i64(i64::from(c.x)) + half,
                    Fx::from_i64(i64::from(c.y)) + half,
                );
                self.apply(Command::Order {
                    ids: vec![unit],
                    order: Order::Move(target),
                    queued: false,
                });
            }
        }
        self.production = Some(p);
    }

    /// Creates a unit of `kind` for the owner of the structure `factory` on the first free spot
    /// by its exit. `None` when there is none, or the entity limit is reached.
    fn put_on_the_map(&mut self, factory: EntityId, kind: Kind, radius: u8) -> Option<EntityId> {
        let i = self.live_index(factory)?;
        let side = self.buildings.get(self.entities.kind[i])?.exit_side?;
        let owner = self.entities.owner[i];
        let anchor = self.occupancy.cell(self.entities.cell[i]);
        let (ex, ey) = exit_cell(anchor, self.footprint_at(i), side);
        let class = self.units.get(kind)?.movement_class;
        let spot = self.free_spot(ex, ey, radius, class)?;
        let half = Fx::from_ratio(1, 2);
        let pos = Vec2::new(
            Fx::from_i64(i64::from(spot.x)) + half,
            Fx::from_i64(i64::from(spot.y)) + half,
        );
        self.spawn_unit(kind, pos, owner)
    }

    /// The first free cell in the search order round `(ex, ey)`: rings of growing chessboard
    /// distance from the exit cell itself (ring 0) out to `radius`, each ring row by row from
    /// the north-west. A cell is free when it is on the map, no structure stands on it, a unit
    /// of `class` can enter it, and no unit stands on it or is walking to it.
    fn free_spot(&self, ex: i64, ey: i64, radius: u8, class: Option<u16>) -> Option<Cell> {
        let r = i64::from(radius);
        let (x0, y0, x1, y1) = (ex - r, ey - r, ex + r, ey + r);
        // The cells in the window that a unit stands on or walks to.
        let mut taken: Vec<(i64, i64)> = Vec::new();
        for i in 0..self.entities.len() {
            if !self.entities.alive[i]
                || self.entities.cell[i] != NO_CELL
                || self.entities.def[i] == NO_DEF
                || self
                    .stats_at(i)
                    .is_none_or(|s| s.class() != UnitClass::Mobile)
            {
                continue;
            }
            let mut mark = |p: Vec2| {
                let (cx, cy) = p.cell();
                let (cx, cy) = (i64::from(cx), i64::from(cy));
                if (x0..=x1).contains(&cx) && (y0..=y1).contains(&cy) {
                    taken.push((cx, cy));
                }
            };
            mark(self.entities.pos[i]);
            if let Order::Move(to) = self.entities.order[i] {
                mark(to);
            }
        }
        for d in 0..=r {
            for y in (ey - d)..=(ey + d) {
                for x in (ex - d)..=(ex + d) {
                    if (x - ex).abs().max((y - ey).abs()) != d {
                        continue;
                    }
                    let (Ok(cx), Ok(cy)) = (u32::try_from(x), u32::try_from(y)) else {
                        continue;
                    };
                    if cx >= self.config.width || cy >= self.config.height {
                        continue;
                    }
                    let cell = Cell::new(cx, cy);
                    if self.is_occupied(cell)
                        || class.is_some_and(|c| !self.passable(c, cell))
                        || taken.contains(&(x, y))
                    {
                        continue;
                    }
                    return Some(cell);
                }
            }
        }
        None
    }

    /// Gives `n` credits back to `player` (a cancelled unit): into the store, whatever its
    /// capacity, and off the statistic of what was spent.
    fn refund(&mut self, player: PlayerId, n: u64) {
        if let (Some(money), Some(harvest)) = (&mut self.money, self.harvest.as_deref_mut()) {
            harvest.refund(player, n);
            if let Ok(k) = money.spent.binary_search_by_key(&player, |&(p, _)| p) {
                money.spent[k].1 = money.spent[k].1.saturating_sub(n);
            }
        }
    }
}

/// Whether every kind of `table` exists as it should in the unit and building tables.
fn table_fits(table: &ProductionTable, units: &UnitTable, buildings: &BuildingTable) -> bool {
    let mobile = |k: Kind| units.get(k).is_some_and(|s| s.class() == UnitClass::Mobile);
    let has_factory = |f: BuildingFunction| {
        buildings
            .iter()
            .any(|b| b.function == f && b.cost.is_some())
    };
    table.iter().all(|b| {
        mobile(b.kind)
            && has_factory(b.factory)
            && b.requires
                .iter()
                .all(|&r| buildings.get(r).is_some_and(|s| s.cost.is_some()))
    }) && table
        .comes_with_all()
        .iter()
        .all(|&(s, u)| buildings.get(s).is_some_and(|b| b.exit_side.is_some()) && mobile(u))
}
