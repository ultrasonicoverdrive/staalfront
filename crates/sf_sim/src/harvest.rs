//! Harvesters and the refinery's dock (WP7.2, `docs/rules/economy.md`).
//!
//! A harvester runs a state machine: search the ore with [`World::nearest_ore_where`], drive
//! there, take ore a bite at a time, drive to a refinery when it is full (or when no ore is
//! left), check in, unload, and begin again. The refinery is the structure whose function is
//! `HarvesterDock`. Harvester and refinery never call each other: they talk by **messages**
//! (analysis sections 11 and 13).
//!
//! # The messages
//!
//! | From the harvester to the dock | Means |
//! |---|---|
//! | `Request` | "May I unload?" (sent once it is within `call_range` cells of the dock point) |
//! | `Arrived` | "I stand at the dock point." |
//! | `Cancel` | "Forget me" (a player order took the harvester away) |
//!
//! | From the dock to the harvester | Means |
//! |---|---|
//! | `Granted` | The bay is yours: drive in. |
//! | `Queued(place)` | The bay is taken; stand and wait. |
//! | `Revoked` | The grant timed out before you arrived; ask again. |
//! | `Unloaded(n)` | `n` units of your load were taken (and credited to your player). |
//! | `Released` | You are empty; the bay is free; go. |
//!
//! A message sent in a tick is read in the next tick's dock stage; an answer is read in the same
//! tick by the harvester stage that follows it, so a request costs one tick.
//!
//! # A tick of harvesting, in two passes
//!
//! The stage runs after the commands and the regrowth of the tick and before anyone moves (the
//! orders it gives are carried out in the same tick).
//!
//! (1) **Sync.** The harvesters and the docks of the world are found; the state of one that is
//!    gone (or whose slot was reused) is dropped and its claim on an ore cell released.
//! (2) **Docks.** Each dock, in ascending slot order, reads its inbox in the order it was sent,
//!    answers, counts down its grant and its unloading and grants the bay to the next in the
//!    queue. A dock writes only its own state, the harvesters' mailboxes and the player's
//!    delivered counter.
//! (3) **Decide.** Every harvester, in ascending slot order, works out what it does from the
//!    state at the start of the stage: nothing is changed in this pass. The result is a [`Plan`].
//! (4) **Apply.** The plans are carried out in ascending slot order: claims on ore cells (the
//!    first harvester of the slot order keeps a cell two of them chose in the same tick; the
//!    other searches again next tick), orders, bites of ore (where two harvesters bite one
//!    cell that cannot feed both, the lower slot is served first) and the messages to the
//!    docks.
//!
//! Nothing here depends on a hash order; every list is a vector in ascending order or a
//! `BTreeMap`.

use std::collections::BTreeMap;

use sf_math::hash::Fnv1a64;
use sf_math::{Fx, Vec2};

use crate::buildings::{BuildingFunction, Side};
use crate::entities::NO_DEF;
use crate::ids::{EntityId, Kind, PlayerId, Tick};
use crate::orders::{self, Order};
use crate::placement::exit_cell;
use crate::terrain::{Cell, NO_CELL};
use crate::world::World;

/// A distance in tenths of a cell (the reach and parking radii of [`HarvestRules`]) as a fixed-point
/// number of cells.
fn tenths(n: u16) -> Fx {
    Fx::from_ratio(i64::from(n), 10)
}

/// The numbers of the harvesters and of the refinery's dock, data in `assets/config/harvest.ron`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct HarvestRules {
    /// Units of ore a harvester carries.
    pub capacity: u16,
    /// Ore taken in one bite.
    pub mine_amount: u16,
    /// Ticks from one bite to the next.
    pub mine_period: u16,
    /// Ore the dock takes off a harvester in one bite.
    pub unload_amount: u16,
    /// Ticks from one unloading bite to the next (and from the harvester entering the bay to the
    /// first one).
    pub unload_period: u16,
    /// How far, in cells, a harvester looks for ore.
    pub search_radius: u16,
    /// Ticks between two searches of a harvester that found no ore, and between two looks for a
    /// refinery of one that found none.
    pub retry_period: u16,
    /// A harvester sends its request when it is this many cells from the dock point.
    pub call_range: u16,
    /// Ticks a harvester that was granted the bay has to arrive before the grant is withdrawn.
    pub grant_timeout: u16,
    /// Ticks a harvester waits for the answer to its request before it asks again. At least 1, at most 1000 (inclusive).
    pub call_timeout: u16,
    /// Walks to one goal that end short of it before the harvester gives that goal up. At least 1, at most 20 (inclusive).
    pub max_tries: u8,
    /// How far from the dock point a harvester without ore waits, along the exit side, in cells.
    /// At least 1, at most 32 (inclusive).
    pub park_distance: u16,
    /// Lateral spread of the parking spots: `slot % park_spread - park_spread / 2` cells. At least 1, at most 32 (inclusive).
    pub park_spread: u16,
    /// A parked harvester within this many tenths of a cell of its parking spot stays where it is.
    /// At least 1, at most 320 (inclusive).
    pub park_near_tenths: u16,
    /// A harvester this near (tenths of a cell) to the middle of its ore cell stops and mines,
    /// whatever is left of its walk. At least 1, at most `mine_reach_tenths`.
    pub mine_start_tenths: u16,
    /// How near (tenths of a cell) to the middle of the ore cell a harvester has to be, when its
    /// walk is over, to count as there. Below `mine_lose_tenths`.
    pub mine_reach_tenths: u16,
    /// A harvester that mines and is pushed farther than this (tenths of a cell) from the cell's
    /// middle drives back. Not above 320 (inclusive).
    pub mine_lose_tenths: u16,
    /// How near (tenths of a cell) to the dock point a harvester has to be, when its walk is over,
    /// to count as docked. At least 1, at most 320 (inclusive).
    pub dock_reach_tenths: u16,
}

impl HarvestRules {
    /// Whether the numbers fit together: every one at least 1, a bite no bigger than the load, the
    /// tuning numbers within their bounds and `mine_start <= mine_reach < mine_lose`.
    pub const fn is_valid(&self) -> bool {
        self.capacity >= 1
            && self.mine_amount >= 1
            && self.mine_amount <= self.capacity
            && self.mine_period >= 1
            && self.unload_amount >= 1
            && self.unload_period >= 1
            && self.search_radius >= 1
            && self.retry_period >= 1
            && self.call_range >= 1
            && self.grant_timeout >= 1
            && self.call_timeout >= 1
            && self.call_timeout <= 1000
            && self.max_tries >= 1
            && self.max_tries <= 20
            && self.park_distance >= 1
            && self.park_distance <= 32
            && self.park_spread >= 1
            && self.park_spread <= 32
            && self.park_near_tenths >= 1
            && self.park_near_tenths <= 320
            && self.mine_start_tenths >= 1
            && self.mine_start_tenths <= self.mine_reach_tenths
            && self.mine_reach_tenths < self.mine_lose_tenths
            && self.mine_lose_tenths <= 320
            && self.dock_reach_tenths >= 1
            && self.dock_reach_tenths <= 320
    }
}

/// What a harvester is doing, as the outside sees it ([`World::harvester_of`]).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum HarvestStage {
    /// About to look for ore.
    Searching,
    /// Driving to an ore cell.
    ToOre,
    /// Taking ore.
    Mining,
    /// Driving to a refinery, not yet asked for a place or still unanswered.
    ToRefinery,
    /// Granted the bay and driving in.
    Granted,
    /// Asked for a place and was queued: standing and waiting.
    Queued,
    /// In the bay, being unloaded.
    Unloading,
    /// No ore in reach: standing near its refinery until the next look.
    Parked,
    /// Carrying ore and has no standing refinery: waiting for one.
    NoRefinery,
    /// Obeying a player order that was not a field.
    Manual,
}

/// A harvester's state, for the presentation and the tests.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct HarvesterView {
    /// What it is doing.
    pub stage: HarvestStage,
    /// Units of ore it carries.
    pub load: u16,
    /// The ore cell it has claimed.
    pub cell: Option<Cell>,
    /// The refinery it is going to, queued at or docked in.
    pub dock: Option<EntityId>,
}

/// A refinery dock's state, for the presentation and the tests.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DockView {
    /// The harvester that has been granted the bay, driving in or unloading.
    pub holder: Option<EntityId>,
    /// Whether the holder is already in the bay and being unloaded.
    pub unloading: bool,
    /// The harvesters waiting, in the order they will be granted the bay.
    pub queue: Vec<EntityId>,
    /// The cell centre harvesters drive to, in cells.
    pub point: Vec2,
}

// ---- state --------------------------------------------------------------------------------

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Call {
    /// Driving toward the dock, not yet within call range.
    Driving,
    /// Request sent, no answer yet.
    Called,
    /// Queued: standing.
    Queued,
    /// Granted: driving in.
    Granted,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Phase {
    Search,
    Park,
    ToOre,
    Mine,
    NoDock,
    ToDock(Call),
    Unload,
    Manual,
}

impl Phase {
    fn tag(self) -> u8 {
        match self {
            Phase::Search => 0,
            Phase::Park => 1,
            Phase::ToOre => 2,
            Phase::Mine => 3,
            Phase::NoDock => 4,
            Phase::ToDock(Call::Driving) => 5,
            Phase::ToDock(Call::Called) => 6,
            Phase::ToDock(Call::Queued) => 7,
            Phase::ToDock(Call::Granted) => 8,
            Phase::Unload => 9,
            Phase::Manual => 10,
        }
    }
}

/// Everything a harvester remembers apart from its mailbox. `Copy`, so a plan can hold the next
/// one.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct Core {
    /// Whether the slot holds a harvester at all.
    bound: bool,
    /// The generation of the entity this state belongs to.
    generation: u32,
    load: u16,
    phase: Phase,
    /// Ticks to the next bite, search or retry, by phase.
    wait: u16,
    /// The ore cell (row-major index) this harvester has claimed, or [`NO_CELL`].
    cell: u32,
    /// Where the next search starts instead of at the harvester (a player pointed it at a
    /// field), or [`NO_CELL`].
    focus: u32,
    /// A cell this harvester could not reach, to skip in its searches, or [`NO_CELL`].
    bad: u32,
    dock: Option<EntityId>,
    /// The order the harvester gave itself and still expects to find as its current one.
    own: Order,
    tries: u8,
}

impl Core {
    const NONE: Core = Core {
        bound: false,
        generation: 0,
        load: 0,
        phase: Phase::Search,
        wait: 0,
        cell: NO_CELL,
        focus: NO_CELL,
        bad: NO_CELL,
        dock: None,
        own: Order::Idle,
        tries: 0,
    };
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Reply {
    Granted,
    Queued(u8),
    Revoked,
    Unloaded(u16),
    Released,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Hv {
    core: Core,
    mail: Vec<Reply>,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Msg {
    Request,
    Arrived,
    Cancel,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct Mail {
    from: EntityId,
    msg: Msg,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Bay {
    Free,
    /// Granted, the harvester is driving in; `left` ticks to arrive.
    Reserved {
        who: EntityId,
        left: u16,
    },
    /// In the bay; `wait` ticks to the next bite.
    Unloading {
        who: EntityId,
        wait: u16,
    },
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Dock {
    id: EntityId,
    owner: PlayerId,
    point: Vec2,
    /// The cell the dock point is the middle of.
    exit: (i64, i64),
    side: Side,
    bay: Bay,
    queue: Vec<EntityId>,
    inbox: Vec<Mail>,
}

impl Dock {
    fn holder(&self) -> Option<EntityId> {
        match self.bay {
            Bay::Free => None,
            Bay::Reserved { who, .. } | Bay::Unloading { who, .. } => Some(who),
        }
    }

    fn knows(&self, who: EntityId) -> bool {
        self.holder() == Some(who) || self.queue.contains(&who)
    }
}

/// What a harvester wants done to its order.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Act {
    Keep,
    Clear,
    Go(Vec2),
}

/// The outcome of one harvester's decision, carried out in the apply pass.
#[derive(Copy, Clone, Debug)]
struct Plan {
    core: Core,
    act: Act,
    take: Option<(u32, u16)>,
    send: Option<(EntityId, Msg)>,
    /// The cell in `core.cell` was chosen with room round it: if another harvester has claimed a
    /// neighbouring cell in this same pass, the choice is not kept.
    spaced: bool,
    /// The field focus the search of this pass started from: if the cell it chose is not kept, the
    /// focus is given back, so the next search still starts at the field the player chose.
    focus: u32,
}

/// The harvest state of a world: the rules, the harvester kinds, one record per slot, the docks,
/// the claims on ore cells and the ore delivered so far.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Harvest {
    rules: HarvestRules,
    /// The harvester kinds, ascending.
    kinds: Vec<Kind>,
    /// Per position in the unit table: is it a harvester kind?
    harvester_def: Vec<bool>,
    /// Per position in the unit table: is it a structure with the dock function?
    dock_def: Vec<bool>,
    units: Vec<Hv>,
    /// Ascending by slot.
    docks: Vec<Dock>,
    /// Ore cell (row-major index) to the slot of the harvester that claimed it.
    claims: BTreeMap<u32, u32>,
    /// Ore delivered to the refineries so far, by player (ascending).
    delivered: Vec<(PlayerId, u64)>,
    /// Scratch: the live harvester slots of this tick.
    live: Vec<u32>,
    /// Storage (WP7.3): `None` for a world without storage rules, whose docks take every load.
    storage: Option<Storage>,
}

/// Ore storage (WP7.3, `docs/rules/economy.md`, "Storage"): what each standing building holds
/// and what each player has stored.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Storage {
    /// The capacities as given: kind and units of ore, ascending by kind, no zeros.
    table: Vec<(Kind, u32)>,
    /// Per position in the unit table: the capacity of that kind (zero for most).
    per_def: Vec<u32>,
    /// Ore stored per player (ascending). Nothing spends it yet (WP7.4).
    stored: Vec<(PlayerId, u64)>,
    /// The capacity per player (ascending), summed over the standing buildings in the sync pass
    /// of this tick.
    capacity: Vec<(PlayerId, u64)>,
}

fn add_to(v: &mut Vec<(PlayerId, u64)>, player: PlayerId, n: u64) {
    match v.binary_search_by_key(&player, |&(p, _)| p) {
        Ok(k) => v[k].1 = v[k].1.saturating_add(n),
        Err(k) => v.insert(k, (player, n)),
    }
}

fn get_of(v: &[(PlayerId, u64)], player: PlayerId) -> u64 {
    v.binary_search_by_key(&player, |&(p, _)| p)
        .map_or(0, |k| v[k].1)
}

fn hash_pairs(h: &mut Fnv1a64, v: &[(PlayerId, u64)]) {
    h.write_u64(v.len() as u64);
    for &(player, n) in v {
        h.write_u8(player.0);
        h.write_u64(n);
    }
}

fn centre(index: u32, width: u32) -> Vec2 {
    let (x, y) = (index % width, index / width);
    Vec2::new(
        Fx::from_i64(i64::from(x)) + Fx::HALF,
        Fx::from_i64(i64::from(y)) + Fx::HALF,
    )
}

fn cell_of_index(index: u32, width: u32) -> Cell {
    Cell::new(index % width, index / width)
}

impl Harvest {
    pub(crate) fn new(
        rules: HarvestRules,
        mut kinds: Vec<Kind>,
        units: &crate::units::UnitTable,
        buildings: &crate::buildings::BuildingTable,
    ) -> Harvest {
        kinds.sort_unstable();
        kinds.dedup();
        let harvester_def = units.iter().map(|s| kinds.contains(&s.kind)).collect();
        let dock_def = units
            .iter()
            .map(|s| {
                buildings
                    .get(s.kind)
                    .is_some_and(|b| b.function == BuildingFunction::HarvesterDock)
            })
            .collect();
        Harvest {
            rules,
            kinds,
            harvester_def,
            dock_def,
            units: Vec::new(),
            docks: Vec::new(),
            claims: BTreeMap::new(),
            delivered: Vec::new(),
            live: Vec::new(),
            storage: None,
        }
    }

    /// Turns storage on (WP7.3) with the capacity of each kind in `table`; kinds not in the
    /// table hold nothing. `table` is ascending by kind without duplicates or zeros (the caller,
    /// `World::with_storage`, checks that).
    pub(crate) fn set_storage(&mut self, table: Vec<(Kind, u32)>, units: &crate::units::UnitTable) {
        let per_def = units
            .iter()
            .map(|s| {
                table
                    .binary_search_by_key(&s.kind, |&(k, _)| k)
                    .map_or(0, |j| table[j].1)
            })
            .collect();
        self.storage = Some(Storage {
            table,
            per_def,
            stored: Vec::new(),
            capacity: Vec::new(),
        });
    }

    pub(crate) fn has_storage(&self) -> bool {
        self.storage.is_some()
    }

    /// The capacity of one structure of `kind`; zero without storage rules.
    pub(crate) fn storage_of_kind(&self, kind: Kind) -> u32 {
        self.storage.as_ref().map_or(0, |s| {
            s.table
                .binary_search_by_key(&kind, |&(k, _)| k)
                .map_or(0, |j| s.table[j].1)
        })
    }

    /// Ore stored by `player`; zero without storage rules.
    pub(crate) fn stored(&self, player: PlayerId) -> u64 {
        self.storage
            .as_ref()
            .map_or(0, |s| get_of(&s.stored, player))
    }

    /// The storage capacity of `player` as of the last tick; zero without storage rules.
    pub(crate) fn capacity(&self, player: PlayerId) -> u64 {
        self.storage
            .as_ref()
            .map_or(0, |s| get_of(&s.capacity, player))
    }

    /// Room left for `player`: `None` without storage rules (no limit).
    fn room(&self, player: PlayerId) -> Option<u64> {
        let s = self.storage.as_ref()?;
        Some(get_of(&s.capacity, player).saturating_sub(get_of(&s.stored, player)))
    }

    pub(crate) fn rules(&self) -> &HarvestRules {
        &self.rules
    }

    pub(crate) fn kinds(&self) -> &[Kind] {
        &self.kinds
    }

    pub(crate) fn delivered(&self, player: PlayerId) -> u64 {
        self.delivered
            .binary_search_by_key(&player, |&(p, _)| p)
            .map_or(0, |k| self.delivered[k].1)
    }

    fn credit(&mut self, player: PlayerId, n: u64) {
        add_to(&mut self.delivered, player, n);
        if let Some(s) = &mut self.storage {
            add_to(&mut s.stored, player, n);
        }
    }

    pub(crate) fn view_of(&self, id: EntityId, width: u32) -> Option<HarvesterView> {
        let hv = self.units.get(id.index() as usize)?;
        let c = &hv.core;
        if !c.bound || c.generation != id.generation() {
            return None;
        }
        Some(HarvesterView {
            stage: match c.phase {
                Phase::Search => HarvestStage::Searching,
                Phase::Park => HarvestStage::Parked,
                Phase::ToOre => HarvestStage::ToOre,
                Phase::Mine => HarvestStage::Mining,
                Phase::NoDock => HarvestStage::NoRefinery,
                Phase::ToDock(Call::Queued) => HarvestStage::Queued,
                Phase::ToDock(Call::Granted) => HarvestStage::Granted,
                Phase::ToDock(_) => HarvestStage::ToRefinery,
                Phase::Unload => HarvestStage::Unloading,
                Phase::Manual => HarvestStage::Manual,
            },
            load: c.load,
            cell: (c.cell != NO_CELL).then(|| cell_of_index(c.cell, width)),
            dock: c.dock,
        })
    }

    pub(crate) fn dock_view(&self, id: EntityId) -> Option<DockView> {
        let d = self.dock_of(id)?;
        Some(DockView {
            holder: d.holder(),
            unloading: matches!(d.bay, Bay::Unloading { .. }),
            queue: d.queue.clone(),
            point: d.point,
        })
    }

    fn dock_index(&self, id: EntityId) -> Option<usize> {
        self.docks
            .binary_search_by_key(&id.index(), |d| d.id.index())
            .ok()
            .filter(|&k| self.docks[k].id == id)
    }

    fn dock_of(&self, id: EntityId) -> Option<&Dock> {
        self.dock_index(id).map(|k| &self.docks[k])
    }

    /// Whether `id` names a live harvester that this stage has registered.
    fn hv_live(&self, id: EntityId) -> bool {
        self.units
            .get(id.index() as usize)
            .is_some_and(|hv| hv.core.bound && hv.core.generation == id.generation())
    }

    fn reply(&mut self, who: EntityId, r: Reply) {
        if let Some(hv) = self.units.get_mut(who.index() as usize) {
            hv.mail.push(r);
        }
    }

    /// The nearest standing dock of `owner` to `pos`: the smallest squared distance to the dock
    /// point, a tie to the lower slot.
    fn nearest_dock(&self, owner: PlayerId, pos: Vec2) -> Option<&Dock> {
        let mut best: Option<(Fx, &Dock)> = None;
        for d in &self.docks {
            if d.owner != owner {
                continue;
            }
            let dist = (d.point - pos).length_squared();
            if best.is_none_or(|(b, _)| dist < b) {
                best = Some((dist, d));
            }
        }
        best.map(|(_, d)| d)
    }

    /// Where a harvester without ore waits: a few cells out from the dock along its exit side,
    /// spread sideways by the harvester's slot so that they do not stand on one spot.
    fn park_point(&self, w: &World, d: &Dock, slot: u32) -> Vec2 {
        let (dx, dy): (i64, i64) = match d.side {
            Side::PosX => (1, 0),
            Side::NegX => (-1, 0),
            Side::PosY => (0, 1),
            Side::NegY => (0, -1),
        };
        let (spread, distance) = (
            i64::from(self.rules.park_spread),
            i64::from(self.rules.park_distance),
        );
        let lateral = i64::from(slot) % spread - spread / 2;
        let (ox, oy) = (dx * distance + dy * lateral, dy * distance + dx * lateral);
        let (width, height) = (i64::from(w.config.width), i64::from(w.config.height));
        let x = (d.exit.0 + ox).clamp(0, width - 1);
        let y = (d.exit.1 + oy).clamp(0, height - 1);
        Vec2::new(Fx::from_i64(x) + Fx::HALF, Fx::from_i64(y) + Fx::HALF)
    }

    // ---- the tick -------------------------------------------------------------------------

    pub(crate) fn tick(&mut self, w: &mut World) {
        self.sync(w);
        self.run_docks(w);
        let plans = self.decide_all(w);
        self.apply(w, plans);
    }

    fn sync(&mut self, w: &World) {
        let e = &w.entities;
        let slots = e.len();
        if self.units.len() < slots {
            self.units.resize(
                slots,
                Hv {
                    core: Core::NONE,
                    mail: Vec::new(),
                },
            );
        }
        let mut live = std::mem::take(&mut self.live);
        live.clear();
        let mut docks: Vec<Dock> = Vec::new();
        let mut capacity = self
            .storage
            .as_mut()
            .map(|s| std::mem::take(&mut s.capacity));
        if let Some(c) = &mut capacity {
            c.clear();
        }
        for i in 0..slots {
            let def = e.def[i];
            let alive = e.alive[i] && def != NO_DEF;
            // Storage counts the standing buildings only (complete, with hit points).
            if alive
                && let (Some(c), Some(s)) = (&mut capacity, &self.storage)
                && let Some(&cap) = s.per_def.get(def as usize)
                && cap > 0
                && w.is_standing(i)
            {
                add_to(c, e.owner[i], u64::from(cap));
            }
            let is_harvester = alive
                && self
                    .harvester_def
                    .get(def as usize)
                    .copied()
                    .unwrap_or(false);
            let c = &mut self.units[i].core;
            if c.bound && (!is_harvester || c.generation != e.generation[i]) {
                if c.cell != NO_CELL {
                    self.claims.remove(&c.cell);
                }
                self.units[i].core = Core::NONE;
                self.units[i].mail.clear();
            }
            if is_harvester {
                let c = &mut self.units[i].core;
                if !c.bound {
                    *c = Core {
                        bound: true,
                        generation: e.generation[i],
                        ..Core::NONE
                    };
                }
                live.push(i as u32);
            } else if alive
                && self.dock_def.get(def as usize).copied().unwrap_or(false)
                && w.is_standing(i)
            {
                let id = EntityId::new(i as u32, e.generation[i]);
                // The dock keeps its state while it stands; a new one starts free.
                let kept = self.dock_index(id).map(|k| self.docks[k].clone());
                let dock = match kept {
                    Some(d) => d,
                    None => match self.new_dock(w, i, id) {
                        Some(d) => d,
                        None => continue,
                    },
                };
                docks.push(dock);
            }
        }
        self.docks = docks;
        self.live = live;
        if let (Some(s), Some(c)) = (&mut self.storage, capacity) {
            s.capacity = c;
        }
    }

    fn new_dock(&self, w: &World, i: usize, id: EntityId) -> Option<Dock> {
        let e = &w.entities;
        let stats = w.buildings.get(e.kind[i])?;
        let side = stats.exit_side?;
        let anchor = w.cell_of(id)?;
        let (x, y) = exit_cell(anchor, stats.footprint, side);
        let x = x.clamp(0, i64::from(w.config.width) - 1);
        let y = y.clamp(0, i64::from(w.config.height) - 1);
        Some(Dock {
            id,
            owner: e.owner[i],
            point: Vec2::new(Fx::from_i64(x) + Fx::HALF, Fx::from_i64(y) + Fx::HALF),
            exit: (x, y),
            side,
            bay: Bay::Free,
            queue: Vec::new(),
            inbox: Vec::new(),
        })
    }

    // ---- the docks ------------------------------------------------------------------------

    fn run_docks(&mut self, w: &World) {
        for k in 0..self.docks.len() {
            let mut d = std::mem::replace(
                &mut self.docks[k],
                Dock {
                    id: EntityId::new(0, 0),
                    owner: PlayerId(0),
                    point: Vec2::ZERO,
                    exit: (0, 0),
                    side: Side::PosX,
                    bay: Bay::Free,
                    queue: Vec::new(),
                    inbox: Vec::new(),
                },
            );
            self.run_dock(w, &mut d);
            self.docks[k] = d;
        }
    }

    fn run_dock(&mut self, w: &World, d: &mut Dock) {
        let rules = self.rules;
        // (1) The holder of the bay: gone, or too slow to arrive.
        match d.bay {
            Bay::Reserved { who, left } => {
                if !self.hv_live(who) {
                    d.bay = Bay::Free;
                } else if left == 0 {
                    d.bay = Bay::Free;
                    self.reply(who, Reply::Revoked);
                } else {
                    d.bay = Bay::Reserved {
                        who,
                        left: left - 1,
                    };
                }
            }
            Bay::Unloading { who, .. } if !self.hv_live(who) => d.bay = Bay::Free,
            _ => {}
        }
        // The queue loses whoever is gone.
        let mut k = 0;
        while k < d.queue.len() {
            if self.hv_live(d.queue[k]) {
                k += 1;
            } else {
                d.queue.remove(k);
            }
        }
        // (2) The inbox, in the order it was sent.
        let inbox = std::mem::take(&mut d.inbox);
        for m in inbox {
            if !self.hv_live(m.from) || w.entities.owner[m.from.index() as usize] != d.owner {
                continue;
            }
            match m.msg {
                Msg::Request => {
                    if d.holder() == Some(m.from) {
                        self.reply(m.from, Reply::Granted);
                    } else if let Some(p) = d.queue.iter().position(|&q| q == m.from) {
                        self.reply(m.from, Reply::Queued(place(p)));
                    } else if matches!(d.bay, Bay::Free) && d.queue.is_empty() {
                        d.bay = Bay::Reserved {
                            who: m.from,
                            left: rules.grant_timeout,
                        };
                        self.reply(m.from, Reply::Granted);
                    } else {
                        d.queue.push(m.from);
                        self.reply(m.from, Reply::Queued(place(d.queue.len() - 1)));
                    }
                }
                Msg::Arrived => {
                    if let Bay::Reserved { who, .. } = d.bay
                        && who == m.from
                    {
                        d.bay = Bay::Unloading {
                            who,
                            wait: rules.unload_period,
                        };
                    }
                }
                Msg::Cancel => {
                    if d.holder() == Some(m.from) {
                        d.bay = Bay::Free;
                    }
                    d.queue.retain(|&q| q != m.from);
                }
            }
        }
        // (3) Unloading.
        if let Bay::Unloading { who, wait } = d.bay {
            if wait == 0 {
                let slot = who.index() as usize;
                let load = self.units[slot].core.load;
                // A full store takes nothing: the harvester stays in the bay, holding the rest of
                // its load, and the dock tries again after `unload_period` (WP7.3).
                let room = self
                    .room(w.entities.owner[slot])
                    .map_or(u16::MAX, |r| u16::try_from(r).unwrap_or(u16::MAX));
                let n = load.min(rules.unload_amount).min(room);
                if n > 0 {
                    self.credit(w.entities.owner[slot], u64::from(n));
                    self.reply(who, Reply::Unloaded(n));
                }
                if load == n {
                    self.reply(who, Reply::Released);
                    d.bay = Bay::Free;
                } else {
                    d.bay = Bay::Unloading {
                        who,
                        wait: rules.unload_period - 1,
                    };
                }
            } else {
                d.bay = Bay::Unloading {
                    who,
                    wait: wait - 1,
                };
            }
        }
        // (4) A free bay goes to the first in the queue.
        if matches!(d.bay, Bay::Free) && !d.queue.is_empty() {
            let who = d.queue.remove(0);
            d.bay = Bay::Reserved {
                who,
                left: rules.grant_timeout,
            };
            self.reply(who, Reply::Granted);
        }
    }

    // ---- deciding -------------------------------------------------------------------------

    fn decide_all(&self, w: &World) -> Vec<(u32, Plan)> {
        self.live
            .iter()
            .map(|&i| (i, self.decide(w, i as usize)))
            .collect()
    }

    /// Looks for ore for harvester `i` and sets its next step in `plan`.
    fn search(&self, w: &World, i: usize, plan: &mut Plan) {
        let width = w.config.width;
        plan.focus = plan.core.focus;
        let c = &mut plan.core;
        if c.load >= self.rules.capacity {
            return self.go_return(w, i, plan);
        }
        let from = if c.focus != NO_CELL {
            cell_of_index(c.focus, width)
        } else {
            match w.cell_at(w.entities.pos[i]) {
                Some(cell) => cell,
                None => return,
            }
        };
        let bad = c.bad;
        let height = w.config.height;
        let radius = u32::from(self.rules.search_radius);
        // First a cell with room round it: not next to a cell another harvester has claimed (their
        // bodies are wider than a cell). If there is none, any cell nobody has claimed.
        let found = [true, false].into_iter().find_map(|spaced| {
            w.nearest_ore_where(from, radius, |cell| {
                let index = cell.y * width + cell.x;
                index != bad && self.free_for(i, cell, (width, height), spaced)
            })
            .map(|cell| (cell, spaced))
        });
        match found {
            Some((cell, spaced)) => {
                plan.spaced = spaced;
                let index = cell.y * width + cell.x;
                let to = centre(index, width);
                c.phase = Phase::ToOre;
                c.cell = index;
                c.focus = NO_CELL;
                c.tries = 0;
                c.wait = 0;
                c.own = Order::Move(to);
                plan.act = Act::Go(to);
            }
            None if c.load > 0 => self.go_return(w, i, plan),
            None => self.park(w, i, plan),
        }
    }

    /// Whether harvester `slot` may claim `cell`: nobody else has, and (`spaced`) nobody else has
    /// claimed a cell next to it either.
    fn free_for(&self, slot: usize, cell: Cell, (width, height): (u32, u32), spaced: bool) -> bool {
        let taken = |x: u32, y: u32| {
            self.claims
                .get(&(y * width + x))
                .is_some_and(|&holder| holder as usize != slot)
        };
        if !spaced {
            return !taken(cell.x, cell.y);
        }
        let (x0, x1) = (cell.x.saturating_sub(1), (cell.x + 1).min(width - 1));
        let (y0, y1) = (cell.y.saturating_sub(1), (cell.y + 1).min(height - 1));
        !(y0..=y1).any(|y| (x0..=x1).any(|x| taken(x, y)))
    }

    /// Nothing to harvest: stand near the refinery and look again after the retry period.
    fn park(&self, w: &World, i: usize, plan: &mut Plan) {
        let pos = w.entities.pos[i];
        let owner = w.entities.owner[i];
        let c = &mut plan.core;
        c.phase = Phase::Park;
        c.wait = self.rules.retry_period;
        c.cell = NO_CELL;
        c.focus = NO_CELL;
        if let Some(d) = self.nearest_dock(owner, pos) {
            let spot = self.park_point(w, d, i as u32);
            if (spot - pos).length_squared() > tenths(self.rules.park_near_tenths).square() {
                c.own = Order::Move(spot);
                plan.act = Act::Go(spot);
            }
        }
    }

    /// Carrying ore: drive to the nearest standing refinery, or wait for one.
    fn go_return(&self, w: &World, i: usize, plan: &mut Plan) {
        let pos = w.entities.pos[i];
        let owner = w.entities.owner[i];
        let c = &mut plan.core;
        c.cell = NO_CELL;
        c.focus = NO_CELL;
        c.tries = 0;
        match self.nearest_dock(owner, pos) {
            Some(d) => {
                c.phase = Phase::ToDock(Call::Driving);
                c.dock = Some(d.id);
                c.wait = 0;
                c.own = Order::Move(d.point);
                plan.act = Act::Go(d.point);
            }
            None => {
                c.phase = Phase::NoDock;
                c.dock = None;
                c.wait = self.rules.retry_period;
                if c.own != Order::Idle {
                    c.own = Order::Idle;
                    plan.act = Act::Clear;
                }
            }
        }
    }

    fn decide(&self, w: &World, i: usize) -> Plan {
        let e = &w.entities;
        let hv = &self.units[i];
        let id = EntityId::new(i as u32, e.generation[i]);
        let pos = e.pos[i];
        let cur = e.order[i];
        let rules = &self.rules;
        let width = w.config.width;
        let mut plan = Plan {
            core: hv.core,
            act: Act::Keep,
            take: None,
            send: None,
            spaced: false,
            focus: NO_CELL,
        };

        // (1) What was taken off the load (always true, whatever the harvester does now).
        for r in &hv.mail {
            if let Reply::Unloaded(n) = *r {
                plan.core.load = plan.core.load.saturating_sub(n);
            }
        }

        // (2) A walk of its own that is over, or an order from somewhere else. Judged on what the
        // harvester expected at the start of the tick, before any answer changes it.
        let c = &mut plan.core;
        let ended = matches!((c.own, cur), (Order::Move(_), Order::Idle));
        let mut external = false;
        if ended {
            c.own = Order::Idle;
        } else if c.own != cur {
            external = true;
            // A player order. Anything the harvester had with a dock is over.
            if let (
                Phase::ToDock(Call::Called | Call::Queued | Call::Granted) | Phase::Unload,
                Some(dock),
            ) = (c.phase, c.dock)
            {
                plan.send = Some((dock, Msg::Cancel));
            }
            c.dock = None;
            c.cell = NO_CELL;
            c.focus = NO_CELL;
            c.tries = 0;
            c.own = cur;
            let field = match cur {
                Order::Move(p) if c.load < rules.capacity => w
                    .cell_at(p)
                    .filter(|&cell| w.ore_at(cell) > 0)
                    .map(|cell| cell.y * width + cell.x),
                _ => None,
            };
            match field {
                Some(index) => {
                    // A field: harvest there instead of wherever the search would go.
                    c.phase = Phase::Search;
                    c.focus = index;
                    c.wait = 0;
                }
                None => {
                    c.phase = Phase::Manual;
                    return plan;
                }
            }
        }

        // What the docks said (to a harvester that was not just taken away from them).
        if !external {
            for r in &hv.mail {
                let c = &mut plan.core;
                match *r {
                    Reply::Unloaded(_) => {}
                    Reply::Released => {
                        if c.phase == Phase::Unload {
                            c.phase = Phase::Search;
                            c.wait = 0;
                            c.dock = None;
                            c.tries = 0;
                        }
                    }
                    Reply::Granted => {
                        if matches!(c.phase, Phase::ToDock(_)) {
                            c.phase = Phase::ToDock(Call::Granted);
                            c.tries = 0;
                        }
                    }
                    Reply::Queued(_) => {
                        if matches!(c.phase, Phase::ToDock(Call::Called | Call::Driving)) {
                            c.phase = Phase::ToDock(Call::Queued);
                            if c.own != Order::Idle {
                                c.own = Order::Idle;
                                plan.act = Act::Clear;
                            }
                        }
                    }
                    Reply::Revoked => {
                        if matches!(c.phase, Phase::ToDock(_)) {
                            c.phase = Phase::ToDock(Call::Driving);
                            c.tries = 0;
                        }
                    }
                }
            }
        }

        // (3) The phase.
        let dock_pt = |dock: Option<EntityId>| dock.and_then(|d| self.dock_of(d));
        match plan.core.phase {
            Phase::Manual => {
                if ended {
                    plan.core.phase = Phase::Search;
                    plan.core.wait = 0;
                }
            }
            Phase::Search => {
                if plan.core.wait > 0 {
                    plan.core.wait -= 1;
                } else {
                    self.search(w, i, &mut plan);
                }
            }
            Phase::Park => {
                if plan.core.wait > 0 {
                    plan.core.wait -= 1;
                } else {
                    plan.core.phase = Phase::Search;
                    self.search(w, i, &mut plan);
                }
            }
            Phase::NoDock => {
                if plan.core.wait > 0 {
                    plan.core.wait -= 1;
                } else {
                    self.go_return(w, i, &mut plan);
                }
            }
            Phase::ToOre => {
                let index = plan.core.cell;
                let cell = cell_of_index(index, width);
                if w.ore_at(cell) == 0 {
                    plan.core.cell = NO_CELL;
                    self.search(w, i, &mut plan);
                } else if (centre(index, width) - pos).length_squared()
                    <= tenths(rules.mine_start_tenths).square()
                {
                    // Close enough to reach the ore from where it stands: stop and mine.
                    let c = &mut plan.core;
                    c.phase = Phase::Mine;
                    c.wait = 0;
                    c.tries = 0;
                    c.bad = NO_CELL;
                    if c.own != Order::Idle {
                        c.own = Order::Idle;
                        plan.act = Act::Clear;
                    }
                } else if ended {
                    let to = centre(index, width);
                    if (to - pos).length_squared() <= tenths(rules.mine_reach_tenths).square() {
                        let c = &mut plan.core;
                        c.phase = Phase::Mine;
                        c.wait = 0;
                        c.tries = 0;
                        c.bad = NO_CELL;
                    } else {
                        let c = &mut plan.core;
                        c.tries += 1;
                        if c.tries >= rules.max_tries {
                            c.bad = index;
                            c.cell = NO_CELL;
                            self.search(w, i, &mut plan);
                        } else {
                            // The walk ended short (a jam gave it up): ask for a new route.
                            c.own = Order::Move(to);
                            plan.act = Act::Go(to);
                        }
                    }
                }
            }
            Phase::Mine => {
                let index = plan.core.cell;
                let cell = cell_of_index(index, width);
                let to = centre(index, width);
                let held = w.ore_at(cell);
                if plan.core.load >= rules.capacity {
                    plan.core.cell = NO_CELL;
                    self.go_return(w, i, &mut plan);
                } else if held == 0 {
                    plan.core.cell = NO_CELL;
                    self.search(w, i, &mut plan);
                } else if (to - pos).length_squared() > tenths(rules.mine_lose_tenths).square() {
                    let c = &mut plan.core;
                    c.phase = Phase::ToOre;
                    c.tries = 0;
                    c.own = Order::Move(to);
                    plan.act = Act::Go(to);
                } else if plan.core.wait > 0 {
                    plan.core.wait -= 1;
                } else {
                    let room = rules.capacity - plan.core.load;
                    let want = rules.mine_amount.min(room).min(held);
                    plan.take = Some((index, want));
                    plan.core.wait = rules.mine_period - 1;
                }
            }
            Phase::ToDock(call) => {
                let Some(d) = dock_pt(plan.core.dock) else {
                    // The refinery is gone (destroyed, not standing): keep the load, choose again.
                    self.go_return(w, i, &mut plan);
                    return plan;
                };
                let dock_id = d.id;
                let dist_sq = (d.point - pos).length_squared();
                let point = d.point;
                match call {
                    Call::Driving => {
                        if dist_sq <= Fx::from_int(i32::from(rules.call_range)).square() {
                            plan.send = Some((dock_id, Msg::Request));
                            plan.core.phase = Phase::ToDock(Call::Called);
                            plan.core.wait = rules.call_timeout;
                        } else if ended {
                            self.dock_walk_failed(&mut plan, dock_id, point, false, w, i);
                        }
                    }
                    Call::Called => {
                        if plan.core.wait > 0 {
                            plan.core.wait -= 1;
                        } else {
                            plan.core.phase = Phase::ToDock(Call::Driving);
                        }
                    }
                    Call::Queued => {
                        if !d.knows(id) {
                            // The dock does not remember us: ask again.
                            plan.core.phase = Phase::ToDock(Call::Driving);
                        }
                    }
                    Call::Granted => {
                        let idle = plan.core.own == Order::Idle;
                        if idle && dist_sq <= tenths(rules.dock_reach_tenths).square() {
                            plan.send = Some((dock_id, Msg::Arrived));
                            plan.core.phase = Phase::Unload;
                        } else if ended {
                            self.dock_walk_failed(&mut plan, dock_id, point, true, w, i);
                        } else if idle {
                            plan.core.own = Order::Move(point);
                            plan.act = Act::Go(point);
                        }
                    }
                }
            }
            Phase::Unload => match dock_pt(plan.core.dock) {
                None => {
                    // The refinery was destroyed under it: what it still carries stays with it.
                    if plan.core.load == 0 {
                        plan.core.phase = Phase::Search;
                        plan.core.wait = 0;
                        plan.core.dock = None;
                    } else {
                        self.go_return(w, i, &mut plan);
                    }
                }
                Some(d) => {
                    if !d.knows(id) {
                        plan.core.phase = Phase::ToDock(Call::Driving);
                    }
                }
            },
        }
        plan
    }

    /// A walk to the dock point ended short of it: try the walk again (a new route), or give the
    /// dock up after `max_tries` of the rules.
    fn dock_walk_failed(
        &self,
        plan: &mut Plan,
        dock: EntityId,
        point: Vec2,
        holds_place: bool,
        w: &World,
        i: usize,
    ) {
        let c = &mut plan.core;
        c.tries += 1;
        if c.tries >= self.rules.max_tries {
            if holds_place {
                plan.send = Some((dock, Msg::Cancel));
            }
            // Give this dock up and wait the retry period before choosing again.
            let _ = (w, i);
            c.dock = None;
            c.phase = Phase::NoDock;
            c.wait = self.rules.retry_period;
            c.tries = 0;
        } else {
            c.own = Order::Move(point);
            plan.act = Act::Go(point);
        }
    }

    // ---- applying -------------------------------------------------------------------------

    fn apply(&mut self, w: &mut World, plans: Vec<(u32, Plan)>) {
        let bounds = w.pos_range();
        let tick: Tick = w.tick;
        for (slot, mut plan) in plans {
            let i = slot as usize;
            let old = self.units[i].core.cell;
            // A cell two harvesters chose in one tick goes to the first of them.
            let (width, height) = (w.config.width, w.config.height);
            if plan.core.cell != old
                && plan.core.cell != NO_CELL
                && !self.free_for(
                    i,
                    cell_of_index(plan.core.cell, width),
                    (width, height),
                    plan.spaced,
                )
            {
                plan.core.cell = NO_CELL;
                plan.core.focus = plan.focus;
                plan.core.phase = Phase::Search;
                plan.core.wait = 0;
                // Nothing was ordered: the order it has is the one it expects.
                plan.core.own = w.entities.order[i];
                plan.act = Act::Keep;
                plan.take = None;
            }
            if plan.core.cell != old {
                if old != NO_CELL {
                    self.claims.remove(&old);
                }
                if plan.core.cell != NO_CELL {
                    self.claims.insert(plan.core.cell, slot);
                }
            }
            match plan.act {
                Act::Keep => {}
                Act::Clear => {
                    orders::forget_give_up(&mut w.entities, i);
                    orders::clear(&mut w.entities, i);
                }
                Act::Go(p) => {
                    orders::give(&mut w.entities, i, Order::Move(p), false, tick, bounds);
                }
            }
            if let Some((index, want)) = plan.take {
                let width = w.config.width;
                if let Some(ore) = &mut w.ore {
                    let got = ore.take(cell_of_index(index, width), want);
                    plan.core.load = plan.core.load.saturating_add(got);
                }
            }
            if let Some((dock, msg)) = plan.send
                && let Some(k) = self.dock_index(dock)
            {
                let from = EntityId::new(slot, w.entities.generation[i]);
                self.docks[k].inbox.push(Mail { from, msg });
            }
            self.units[i].core = plan.core;
            self.units[i].mail.clear();
        }
    }

    // ---- hashing --------------------------------------------------------------------------

    /// The storage section (WP7.3); writes nothing without storage rules.
    pub(crate) fn write_storage_hash(&self, h: &mut Fnv1a64) {
        let Some(s) = &self.storage else {
            return;
        };
        h.write_u64(s.table.len() as u64);
        for &(k, n) in &s.table {
            h.write_bytes(&k.0.to_le_bytes());
            h.write_bytes(&n.to_le_bytes());
        }
        hash_pairs(h, &s.stored);
        hash_pairs(h, &s.capacity);
    }

    pub(crate) fn write_hash(&self, h: &mut Fnv1a64) {
        let r = &self.rules;
        for v in [
            r.capacity,
            r.mine_amount,
            r.mine_period,
            r.unload_amount,
            r.unload_period,
            r.search_radius,
            r.retry_period,
            r.call_range,
            r.grant_timeout,
            r.call_timeout,
            u16::from(r.max_tries),
            r.park_distance,
            r.park_spread,
            r.park_near_tenths,
            r.mine_start_tenths,
            r.mine_reach_tenths,
            r.mine_lose_tenths,
            r.dock_reach_tenths,
        ] {
            h.write_bytes(&v.to_le_bytes());
        }
        h.write_u64(self.kinds.len() as u64);
        for k in &self.kinds {
            h.write_bytes(&k.0.to_le_bytes());
        }
        hash_pairs(h, &self.delivered);
        let bound = self.units.iter().filter(|hv| hv.core.bound).count();
        h.write_u64(bound as u64);
        for (slot, hv) in self.units.iter().enumerate() {
            let c = &hv.core;
            if !c.bound {
                continue;
            }
            h.write_u32(slot as u32);
            h.write_u32(c.generation);
            h.write_bytes(&c.load.to_le_bytes());
            h.write_u8(c.phase.tag());
            h.write_bytes(&c.wait.to_le_bytes());
            h.write_u32(c.cell);
            h.write_u32(c.focus);
            h.write_u32(c.bad);
            write_option_id(h, c.dock);
            c.own.write_hash(h);
            h.write_u8(c.tries);
            h.write_u64(hv.mail.len() as u64);
            for m in &hv.mail {
                match *m {
                    Reply::Granted => h.write_u8(0),
                    Reply::Queued(p) => {
                        h.write_u8(1);
                        h.write_u8(p);
                    }
                    Reply::Revoked => h.write_u8(2),
                    Reply::Unloaded(n) => {
                        h.write_u8(3);
                        h.write_bytes(&n.to_le_bytes());
                    }
                    Reply::Released => h.write_u8(4),
                }
            }
        }
        h.write_u64(self.docks.len() as u64);
        for d in &self.docks {
            write_id(h, d.id);
            match d.bay {
                Bay::Free => h.write_u8(0),
                Bay::Reserved { who, left } => {
                    h.write_u8(1);
                    write_id(h, who);
                    h.write_bytes(&left.to_le_bytes());
                }
                Bay::Unloading { who, wait } => {
                    h.write_u8(2);
                    write_id(h, who);
                    h.write_bytes(&wait.to_le_bytes());
                }
            }
            h.write_u64(d.queue.len() as u64);
            for &q in &d.queue {
                write_id(h, q);
            }
            h.write_u64(d.inbox.len() as u64);
            for m in &d.inbox {
                write_id(h, m.from);
                h.write_u8(match m.msg {
                    Msg::Request => 0,
                    Msg::Arrived => 1,
                    Msg::Cancel => 2,
                });
            }
        }
    }
}

fn place(p: usize) -> u8 {
    u8::try_from(p + 1).unwrap_or(u8::MAX)
}

fn write_id(h: &mut Fnv1a64, id: EntityId) {
    h.write_u32(id.index());
    h.write_u32(id.generation());
}

fn write_option_id(h: &mut Fnv1a64, id: Option<EntityId>) {
    match id {
        None => h.write_u8(0),
        Some(id) => {
            h.write_u8(1);
            write_id(h, id);
        }
    }
}

#[cfg(test)]
mod tests;
