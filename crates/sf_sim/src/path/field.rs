//! Flowfields for groups (WP4.3): one search for a whole group order.
//!
//! When one [`Command::Order`](crate::Command::Order) sends at least
//! `threshold` units of one movement class to one place, the units do not ask
//! for a route each. The world builds one **flowfield** for the class and the
//! units follow it:
//!
//! * the **integration field** is the cost of the cheapest walk to the goal from
//!   every cell of a region (Dijkstra from the goal, with the costs of the
//!   class, `path/grid.rs`); the **direction** of a cell is the neighbour that
//!   gives that cost, ties to the first in [`DIRS`] order;
//! * the **region** is the sectors of the abstract corridor between where the
//!   group stands and the goal, with one sector of margin round them
//!   (`path/corridor.rs`), at most [`FIELD_SECTORS`] sectors: memory is bounded
//!   whatever the map;
//! * a unit samples the direction of its cell and looks down the chain of
//!   directions for the farthest cell it can see (the same thick line of sight
//!   a route is pulled taut with), and steers at that; near the goal it walks
//!   straight to its own **arrival slot** (`path/slots.rs`);
//! * a [`Fields`] pool of [`FIELD_POOL`] fields is kept; the least recently
//!   used goes when it is full, and an idle one expires.
//!
//! The build (`path/field_build.rs`) is a resumable job inside the same node
//! budget as the routes: the budget is split evenly while both kinds of work
//! wait and goes whole to whichever has any (`Pathing::serve`). A unit waits
//! where it stands until its own cell has been integrated; the cells nearest the
//! goal are done first, the farthest last.
//!
//! A structure placed or removed inside a region (or next to it: the corner
//! rule looks one cell over a border) makes a **successor** of the field, a new
//! build under the same serial that replaces it when it is done. Until then the
//! units follow the old field as far as it is still valid, that is while the
//! step from cell to cell is allowed by the grid as it is now; they stop where
//! it is not and never step into a blocked cell.
//!
//! The rules are in `docs/rules/pathfinding.md`.

use sf_math::hash::Fnv1a64;
use sf_math::{Fx, Vec2};

use crate::crowd::Claim;
use crate::path::claim;
use crate::path::graph::ClassGraph;
use crate::path::grid::{DIRS, SECTOR_SIZE, Sectors};
use crate::path::los;
use crate::path::route::RouteState;
use crate::path::slots;
use crate::path::{Aim, Steer};

use crate::entities::Entities;

/// Fields the pool holds at once. A group order that arrives when the pool is
/// full takes the place of the least recently used field.
pub const FIELD_POOL: usize = 8;

/// The most sectors one field covers. A corridor longer than this gets no field
/// (its units are routed one by one), a margin that would not fit is cut short.
pub const FIELD_SECTORS: usize = 160;

/// The most arrival slots one field hands out; a bigger group shares them.
pub const MAX_SLOTS: usize = 256;

/// The longest chain of cells a unit looks down for the farthest it can see.
pub(crate) const LOOKAHEAD: usize = 16;

/// A field that no unit has asked for in this many ticks is dropped: the group
/// has arrived or been given other orders. (20 s at 20 ticks a second.)
pub const IDLE_TICKS: u32 = 400;

/// The threshold of the shipped game data (`assets/config/sim.ron`), which a
/// world that is given none uses.
pub const DEFAULT_FLOWFIELD_THRESHOLD: u32 = 8;

/// Cells in the part of a field that belongs to one sector.
const SECTOR_CELLS: usize = (SECTOR_SIZE * SECTOR_SIZE) as usize;

/// A cost not found yet.
pub(crate) const INF: u32 = u32::MAX;

/// The direction value of a cell that is not integrated yet.
pub(crate) const UNSET: u8 = 255;

/// The direction value of the goal cell itself.
pub(crate) const GOAL: u8 = 8;

/// `sector_slot` of a sector that is not in the region.
const NO_SLOT: u8 = 255;

// A place in the region is a `u8` below `NO_SLOT`.
const _: () = assert!(FIELD_SECTORS < NO_SLOT as usize);

/// Where a field is in its life.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum FieldState {
    /// Waiting for the builder, which does one field at a time.
    Queued,
    /// The builder is working on it.
    Building,
    /// Every cell of the region is integrated.
    Ready,
}

impl FieldState {
    pub(crate) const fn tag(self) -> u8 {
        match self {
            FieldState::Queued => 0,
            FieldState::Building => 1,
            FieldState::Ready => 2,
        }
    }
}

/// Where a field's arrival slots lie (WP4.4, `path/slots.rs`): which group order it
/// belongs to, its place among the classes of that order, the spacing of its
/// lattice and the radius at which its ring begins.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct Layout {
    /// The group order the field belongs to; the classes of one `Command::Order`
    /// share it.
    pub order: u32,
    /// The ring of the class: zero is the innermost (the fastest class).
    pub rank: u8,
    /// The distance between two slots, in cells.
    pub spacing: Fx,
    /// The distance from the goal at which the ring begins, in cells.
    pub inner: Fx,
}

impl Layout {
    /// A field with no neighbours: one ring from the middle, at half a cell.
    pub(crate) const ALONE: Layout = Layout {
        order: 0,
        rank: 0,
        spacing: Fx::HALF,
        inner: Fx::ZERO,
    };
}

/// One flowfield: the identity of the order it serves and the data of its
/// region.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Field {
    /// The group this field serves; zero for a free entry.
    pub serial: u32,
    /// Counts the builds of the group's field: a structure placed in the region
    /// makes a successor with the next number.
    pub version: u32,
    /// Counts the entries made, to tell a reused entry from the one a build
    /// began on and to order the queue of builds.
    pub born: u32,
    pub state: FieldState,
    /// The index of the class graph the field is for.
    pub graph: usize,
    /// The goal cell, as a row-major index of the map.
    pub goal: u32,
    /// How the slots of this field lie among those of the other classes of the
    /// same order (WP4.4).
    pub layout: Layout,
    /// How many units the field was made for: the slots it hands out.
    pub members: u32,
    /// The tick (of the pool's clock) a unit last asked it for a direction.
    pub last_used: u32,
    /// The region is known: `sectors` and `sector_slot` are valid.
    pub region_ready: bool,
    /// The slots are known.
    pub slots_ready: bool,
    /// The sectors of the region, ascending.
    pub sectors: Vec<u32>,
    /// For each sector of the map the place in `sectors`, or [`NO_SLOT`].
    sector_slot: Vec<u8>,
    /// Per cell of the region (`place * 256 + row * 16 + column` inside the
    /// sector): the cost to the goal. Valid where `dir` is set.
    pub cost: Vec<u32>,
    /// Per cell of the region: one of the eight [`DIRS`], [`GOAL`], or [`UNSET`]
    /// while the cell is not integrated.
    pub dir: Vec<u8>,
    /// The arrival slots: points around the goal, in the order of the ranks.
    pub slots: Vec<Vec2>,
    /// Who has taken each slot (WP4.4b, `path/claim.rs`): the entity index of
    /// the unit, or [`claim::NO_HOLDER`]. As long as `slots`; a taken slot only
    /// counts while the unit's own claim says so.
    pub taken: Vec<u32>,
    /// How far the slots reach from the goal, in cells: the farthest of them.
    pub reach: Fx,
    /// Cells integrated.
    pub finalized: u32,
    /// A running hash of what was integrated, in the order it was: what the
    /// checksum sees of the arrays.
    pub digest: Fnv1a64,
}

impl core::fmt::Debug for Field {
    /// The identity and the progress, not the arrays.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Field(#{} v{} {:?}, goal {}, {} sectors, {} cells, {} slots)",
            self.serial,
            self.version,
            self.state,
            self.goal,
            self.sectors.len(),
            self.finalized,
            self.slots.len()
        )
    }
}

impl Field {
    fn empty(sector_count: usize) -> Field {
        Field {
            serial: 0,
            version: 0,
            born: 0,
            state: FieldState::Queued,
            graph: 0,
            goal: 0,
            layout: Layout::ALONE,
            members: 0,
            last_used: 0,
            region_ready: false,
            slots_ready: false,
            sectors: Vec::new(),
            sector_slot: vec![NO_SLOT; sector_count],
            cost: Vec::new(),
            dir: Vec::new(),
            slots: Vec::new(),
            taken: Vec::new(),
            reach: Fx::ZERO,
            finalized: 0,
            digest: Fnv1a64::new(),
        }
    }

    /// Forgets what a build made, keeping the identity and the memory.
    pub(crate) fn clear_build(&mut self) {
        self.region_ready = false;
        self.slots_ready = false;
        self.sectors.clear();
        self.sector_slot.iter_mut().for_each(|s| *s = NO_SLOT);
        self.cost.clear();
        self.dir.clear();
        self.slots.clear();
        self.taken.clear();
        self.reach = Fx::ZERO;
        self.finalized = 0;
        self.digest = Fnv1a64::new();
    }

    /// Makes the entry free.
    fn free(&mut self) {
        self.clear_build();
        self.serial = 0;
        self.state = FieldState::Queued;
    }

    #[inline]
    fn local(x: u32, y: u32) -> usize {
        ((y % SECTOR_SIZE) * SECTOR_SIZE + x % SECTOR_SIZE) as usize
    }

    /// The place in the field's arrays of the cell `(x, y)` of the map, if its
    /// sector is in the region. The cell must be on the map.
    #[inline]
    pub(crate) fn index(&self, sectors: &Sectors, x: u32, y: u32) -> Option<usize> {
        let place = *self.sector_slot.get(sectors.of_cell(x, y) as usize)?;
        (place != NO_SLOT).then(|| usize::from(place) * SECTOR_CELLS + Field::local(x, y))
    }

    /// The cell of the map a place in the arrays is.
    #[inline]
    pub(crate) fn xy(&self, sectors: &Sectors, index: usize) -> (u32, u32) {
        let (sx, sy) = sectors.coords(self.sectors[index / SECTOR_CELLS]);
        let local = (index % SECTOR_CELLS) as u32;
        (
            sx * SECTOR_SIZE + local % SECTOR_SIZE,
            sy * SECTOR_SIZE + local / SECTOR_SIZE,
        )
    }

    /// The direction value of the cell `(x, y)`: [`UNSET`] outside the region.
    #[inline]
    pub(crate) fn dir_at(&self, sectors: &Sectors, x: u32, y: u32) -> u8 {
        self.index(sectors, x, y).map_or(UNSET, |i| self.dir[i])
    }

    /// Whether the sector is in the region.
    pub(crate) fn has_sector(&self, sector: u32) -> bool {
        self.sector_slot
            .get(sector as usize)
            .is_some_and(|&p| p != NO_SLOT)
    }

    /// Sets the region: the (ascending) sectors, and room for their cells.
    pub(crate) fn set_region(&mut self, sectors: &[u32]) {
        self.sectors.clear();
        self.sectors.extend_from_slice(sectors);
        self.sector_slot.iter_mut().for_each(|s| *s = NO_SLOT);
        for (place, &s) in sectors.iter().enumerate() {
            self.sector_slot[s as usize] = place as u8;
        }
        self.cost.clear();
        self.cost.resize(sectors.len() * SECTOR_CELLS, INF);
        self.dir.clear();
        self.dir.resize(sectors.len() * SECTOR_CELLS, UNSET);
        self.region_ready = true;
    }

    /// The arrival slot of the unit with rank `rank`; units beyond the slots
    /// there are share them round again.
    pub(crate) fn slot_for(&self, rank: u16) -> Option<Vec2> {
        if !self.slots_ready || self.slots.is_empty() {
            return None;
        }
        Some(self.slots[usize::from(rank) % self.slots.len()])
    }

    /// The arrival slot of unit `i`: the one it has taken (`path/claim.rs`), or,
    /// before it has taken one and when none was left for it, the one its rank
    /// gives.
    pub(crate) fn slot_of(&self, e: &Entities, i: usize) -> Option<Vec2> {
        if !self.slots_ready || self.slots.is_empty() {
            return None;
        }
        let c = e.crowd[i].claim;
        if c.serial == self.serial
            && c.slot != Claim::SHARED
            && self.taken.get(usize::from(c.slot)) == Some(&(i as u32))
        {
            return Some(self.slots[usize::from(c.slot)]);
        }
        self.slot_for(e.route[i].field_rank)
    }

    /// Whether a change in `sector` can alter the field: the sector or one of
    /// its eight neighbours is in the region (a corner step looks one cell
    /// over a border).
    fn touches(&self, sectors: &Sectors, sector: u32) -> bool {
        let (sx, sy) = sectors.coords(sector);
        for ny in sy.saturating_sub(1)..=(sy + 1).min(sectors.down - 1) {
            for nx in sx.saturating_sub(1)..=(sx + 1).min(sectors.across - 1) {
                if self.has_sector(ny * sectors.across + nx) {
                    return true;
                }
            }
        }
        false
    }

    /// The neighbour of `(x, y)` that is cheapest to walk through to the goal,
    /// among the integrated ones a step to which is allowed, ties to the first
    /// in [`DIRS`] order; the value for [`Field::dir`]. [`UNSET`] when there is
    /// none.
    pub(crate) fn best_dir(&self, g: &ClassGraph, x: u32, y: u32) -> u8 {
        let mut best = UNSET;
        let mut best_cost = u64::MAX;
        for (k, &(dx, dy)) in DIRS.iter().enumerate() {
            if !g.grid.can_step(x, y, dx, dy) {
                continue;
            }
            let (nx, ny) = ((i64::from(x) + dx) as u32, (i64::from(y) + dy) as u32);
            let Some(m) = self.index(&g.sectors, nx, ny) else {
                continue;
            };
            if self.dir[m] == UNSET {
                continue;
            }
            let total = u64::from(self.cost[m]) + u64::from(g.grid.step_cost(x, y, dx, dy));
            if total < best_cost {
                best_cost = total;
                best = k as u8;
            }
        }
        best
    }
}

/// Where the builder is in making one field.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Stage {
    /// Finds the sectors the group stands in.
    Gather,
    /// The cost from the goal to the portals of its sector.
    GoalLinks,
    /// The abstract search from the goal to those sectors.
    Corridor,
    /// Marks the sectors on the paths found.
    Collect,
    /// Makes the region of the marked sectors and their margin.
    Region,
    /// Picks the arrival slots.
    Slots,
    /// The integration field.
    Integrate,
}

impl Stage {
    pub(crate) const fn tag(self) -> u8 {
        match self {
            Stage::Gather => 0,
            Stage::GoalLinks => 1,
            Stage::Corridor => 2,
            Stage::Collect => 3,
            Stage::Region => 4,
            Stage::Slots => 5,
            Stage::Integrate => 6,
        }
    }
}

/// The field being built.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Cur {
    /// Its place in the pool, and the `born` that tells it from a reused entry.
    pub entry: usize,
    pub born: u32,
    pub stage: Stage,
    /// A cursor: slots looked at (gather), sectors looked at (collect), points
    /// of the spiral looked at (slots).
    pub cursor: usize,
    /// The node of a path being marked back to the goal.
    pub chain: Option<u32>,
    /// A search of the stage was begun.
    pub started: bool,
    /// A local search of the stage was begun and is not over.
    pub searching: bool,
    /// Units found standing in the group.
    pub found: u32,
}

impl Cur {
    pub(crate) fn new(entry: usize, born: u32) -> Cur {
        Cur {
            entry,
            born,
            stage: Stage::Gather,
            cursor: 0,
            chain: None,
            started: false,
            searching: false,
            found: 0,
        }
    }

    /// Back to the start of the build.
    pub(crate) fn restart(&mut self) {
        *self = Cur::new(self.entry, self.born);
    }
}

/// The pool of fields, the build in flight and everything the builder needs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Fields {
    /// Units at least this many of a class in one order get a field; zero turns
    /// flowfields off.
    pub threshold: u32,
    pub entries: Vec<Field>,
    pub cur: Option<Cur>,
    /// The builder's memory, made when the first field is.
    pub scratch: Option<Box<crate::path::field_build::Scratch>>,
    pub spiral: Vec<slots::Point>,
    next_serial: u32,
    next_born: u32,
    /// Counts the group orders that made fields (WP4.4).
    pub next_order: u32,
    /// Counts ticks, to age the fields.
    pub clock: u32,
    /// Slots units may still take this tick (`path/claim.rs`).
    pub claims_left: u32,
    /// The slots already taken by the group, gathered for one claim.
    pub held: claim::HeldBuf,
    /// Fields finished so far (and rebuilds).
    pub built: u64,
    /// Nodes the builder spent in the last tick.
    pub nodes_last_tick: u32,
}

impl Fields {
    pub(crate) fn new(threshold: u32) -> Fields {
        Fields {
            threshold,
            entries: Vec::new(),
            cur: None,
            scratch: None,
            spiral: slots::spiral(),
            next_serial: 1,
            next_born: 1,
            next_order: 1,
            clock: 0,
            claims_left: claim::CLAIMS_PER_TICK,
            held: claim::HeldBuf::default(),
            built: 0,
            nodes_last_tick: 0,
        }
    }

    /// Whether some field is waiting to be built or is being built.
    pub(crate) fn has_work(&self) -> bool {
        self.entries
            .iter()
            .any(|f| f.serial != 0 && f.state != FieldState::Ready)
    }

    /// Fields in use.
    pub(crate) fn live(&self) -> u32 {
        self.entries.iter().filter(|f| f.serial != 0).count() as u32
    }

    /// Whether the world has any field: the checksum has a section then.
    pub(crate) fn any(&self) -> bool {
        self.entries.iter().any(|f| f.serial != 0)
    }

    /// The entry that serves group `serial`: the newest finished one, or when
    /// none is finished the newest build. A finished field stays in use while
    /// its successor is built.
    pub(crate) fn find(&self, serial: u32) -> Option<usize> {
        let mut best: Option<(bool, u32, usize)> = None;
        for (i, f) in self.entries.iter().enumerate() {
            if f.serial != serial || serial == 0 {
                continue;
            }
            let key = (f.state == FieldState::Ready, f.version, i);
            if best.is_none_or(|b| (b.0, b.1) < (key.0, key.1)) {
                best = Some(key);
            }
        }
        best.map(|b| b.2)
    }

    /// Marks every entry of the group as used now.
    pub(crate) fn touch(&mut self, serial: u32) {
        let now = self.clock;
        for f in self.entries.iter_mut().filter(|f| f.serial == serial) {
            f.last_used = now;
        }
    }

    /// Frees entry `i`, and the build if it was the one in flight.
    fn free_entry(&mut self, i: usize) {
        self.entries[i].free();
        if self.cur.as_ref().is_some_and(|c| c.entry == i) {
            self.cur = None;
        }
    }

    /// A place for a new entry: a free one, a new one if the pool is not full,
    /// or the least recently used (ties to the lowest place) of the groups other
    /// than `keep`.
    fn alloc(&mut self, keep: u32, sector_count: usize) -> usize {
        if let Some(i) = self.entries.iter().position(|f| f.serial == 0) {
            return i;
        }
        if self.entries.len() < FIELD_POOL {
            self.entries.push(Field::empty(sector_count));
            return self.entries.len() - 1;
        }
        // The one unused longest; ties to the lowest place.
        let victim = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, f)| f.serial != keep)
            .max_by_key(|&(i, f)| (self.clock.wrapping_sub(f.last_used), core::cmp::Reverse(i)))
            .map_or(0, |(i, _)| i);
        self.free_entry(victim);
        victim
    }

    /// Makes the field of a new group order and returns its serial. The caller
    /// gives the units their ranks.
    pub(crate) fn create(
        &mut self,
        graph: usize,
        goal: u32,
        layout: Layout,
        members: u32,
        sector_count: usize,
    ) -> u32 {
        let i = self.alloc(0, sector_count);
        let serial = self.next_serial;
        self.next_serial = self.next_serial.checked_add(1).unwrap_or(1);
        let born = self.take_born();
        let now = self.clock;
        let f = &mut self.entries[i];
        f.clear_build();
        f.serial = serial;
        f.version = 0;
        f.born = born;
        f.state = FieldState::Queued;
        f.graph = graph;
        f.goal = goal;
        f.layout = layout;
        f.members = members;
        f.last_used = now;
        serial
    }

    /// The number of a new group order: its classes' fields share it.
    pub(crate) fn new_order(&mut self) -> u32 {
        let o = self.next_order;
        self.next_order = self.next_order.wrapping_add(1).max(1);
        o
    }

    fn take_born(&mut self) -> u32 {
        let b = self.next_born;
        self.next_born = self.next_born.wrapping_add(1).max(1);
        b
    }

    /// Queues a new build of the group of entry `of`, to replace it.
    fn make_successor(&mut self, of: usize, sector_count: usize) {
        let (serial, version, graph, goal, base, members) = {
            let f = &self.entries[of];
            (f.serial, f.version, f.graph, f.goal, f.layout, f.members)
        };
        let i = self.alloc(serial, sector_count);
        let born = self.take_born();
        let now = self.clock;
        let f = &mut self.entries[i];
        f.clear_build();
        f.serial = serial;
        f.version = version.wrapping_add(1);
        f.born = born;
        f.state = FieldState::Queued;
        f.graph = graph;
        f.goal = goal;
        f.layout = base;
        f.members = members;
        f.last_used = now;
    }

    /// Ages the fields by one tick and drops those nobody has asked for in
    /// [`IDLE_TICKS`], the one being built included: a unit that waits for its
    /// cell to be integrated asks every tick (`follow` touches the field before it
    /// answers `Wait`), so a build that nobody has asked for in all that time has
    /// no members left that want it, and dropping it frees the builder for the
    /// queue (`free_entry` ends the build in flight).
    pub(crate) fn tick(&mut self) {
        self.clock = self.clock.wrapping_add(1);
        self.nodes_last_tick = 0;
        self.claims_left = claim::CLAIMS_PER_TICK;
        for i in 0..self.entries.len() {
            let f = &self.entries[i];
            if f.serial != 0 && self.clock.wrapping_sub(f.last_used) > IDLE_TICKS {
                self.free_entry(i);
            }
        }
    }

    /// The sectors in `changed` (ascending) have new cells: every field whose
    /// region they touch is built again.
    pub(crate) fn changed(&mut self, graphs: &[ClassGraph], changed: &[u32]) {
        for i in 0..self.entries.len() {
            let f = &self.entries[i];
            if f.serial == 0 {
                continue;
            }
            let g = &graphs[f.graph];
            let hit = !f.region_ready || changed.iter().any(|&s| f.touches(&g.sectors, s));
            if !hit {
                continue;
            }
            match f.state {
                // Not started: it reads the grid as it is when it does.
                FieldState::Queued => {}
                FieldState::Building => {
                    let born = f.born;
                    if let Some(c) = self.cur.as_mut().filter(|c| c.entry == i && c.born == born) {
                        c.restart();
                        self.entries[i].clear_build();
                    }
                }
                FieldState::Ready => {
                    let (serial, version) = (f.serial, f.version);
                    let superseded = self
                        .entries
                        .iter()
                        .any(|o| o.serial == serial && o.version > version);
                    if !superseded {
                        let count = g.sectors.count();
                        self.make_successor(i, count);
                    }
                }
            }
        }
    }

    // ---- following ---------------------------------------------------------

    /// Where the unit in slot `i`, a member of a group, should steer. `None`
    /// when it cannot use the field (the field is gone, its cell is not in the
    /// region or cannot reach the goal): the caller routes it on its own.
    pub(crate) fn follow(
        &mut self,
        graphs: &[ClassGraph],
        e: &mut Entities,
        i: usize,
        epoch: u32,
    ) -> Option<Steer> {
        let serial = e.route[i].field_serial;
        let fi = self.find(serial)?;
        // A stranded member does not keep its field alive: if every member that
        // is left is stranded the field expires, they find it gone and are
        // routed on their own (or the crowd pass asks for that earlier).
        if e.route[i].state != RouteState::Stranded {
            self.touch(serial);
        }
        // A unit that has come near takes its arrival slot (WP4.4b).
        self.claim_if_due(graphs, e, fi, i);
        let f = &self.entries[fi];
        let g = &graphs[f.graph];
        if !f.region_ready {
            return Some(Steer::Wait);
        }
        let pos = e.pos[i];
        let (cx, cy) = cell_of(g, pos);
        let idx = f.index(&g.sectors, cx, cy)?;
        let d = f.dir[idx];
        if d == UNSET {
            return if f.state == FieldState::Ready {
                None
            } else {
                Some(Steer::Wait)
            };
        }
        let slot = f.slot_of(e, i);
        let route = &mut e.route[i];
        // A change of the map since the leg was chosen: choose it again. This
        // comes before the stranded check: a refusal that stranded the unit may
        // be what the change took away, so the unit gets another try (smoothing
        // allowed again). A unit stranded with nothing changed stays so until
        // the crowd pass or the field's expiry gets it out.
        if route.epoch != epoch {
            route.ring_clear();
            route.final_leg = false;
            route.epoch = epoch;
            if route.state == RouteState::Stranded {
                route.state = RouteState::Active;
                route.cautious = false;
            }
        }
        if route.state == RouteState::Stranded {
            return Some(Steer::Stranded);
        }
        if route.final_leg {
            if let Some(s) = slot {
                return Some(Steer::Go(s, Aim::Destination));
            }
            route.final_leg = false;
        }
        // Straight on to the slot when it can be seen: before the waypoints of the
        // ring, which lead to the goal's middle, where a crowd would be standing.
        if (!route.cautious || d == GOAL)
            && let Some(s) = slot
            && los::short_enough(pos, s)
            && los::thick_clear(&g.grid, pos, s).0
        {
            route.final_leg = true;
            return Some(Steer::Go(s, Aim::Destination));
        }
        // A waypoint the unit has been pushed past (the crowd pass moves bodies,
        // WP4.4) is behind it: its cost to the goal is higher than the cost of the
        // cell the unit is in. It is dropped and the leg chosen again, instead of
        // the unit walking back for it.
        if let Some(c) = route.ring_front() {
            let (wx, wy) = g.grid.xy(c);
            let behind = f
                .index(&g.sectors, wx, wy)
                .is_some_and(|k| f.cost[k] > f.cost[idx]);
            if behind {
                route.ring_clear();
            } else {
                return Some(Steer::Go(centre_of(g, c), Aim::Waypoint));
            }
        }
        // The chain of cells down the directions, as far as the grid still
        // allows the steps.
        let reach = if route.cautious { 1 } else { LOOKAHEAD };
        let mut chain = [0_u32; LOOKAHEAD];
        let mut n = 0;
        let (mut x, mut y, mut dd) = (cx, cy, d);
        while n < reach && dd != GOAL && dd != UNSET {
            let (dx, dy) = DIRS[usize::from(dd)];
            if !g.grid.can_step(x, y, dx, dy) {
                break;
            }
            x = (i64::from(x) + dx) as u32;
            y = (i64::from(y) + dy) as u32;
            chain[n] = g.grid.index(x, y);
            n += 1;
            dd = f.dir_at(&g.sectors, x, y);
        }
        if n == 0 {
            // At the goal and the slot is out of sight, or the step the
            // direction says is no longer allowed: the field is out of date.
            if d == GOAL {
                route.ring_push(f.goal);
                return Some(Steer::Go(centre_of(g, f.goal), Aim::Waypoint));
            }
            return Some(Steer::Wait);
        }
        let mut pick = 1;
        if !route.cautious {
            pick = (1..=n)
                .rev()
                .find(|&k| {
                    let q = centre_of(g, chain[k - 1]);
                    los::short_enough(pos, q) && los::thick_clear(&g.grid, pos, q).0
                })
                .unwrap_or(1);
        }
        route.ring_push(chain[pick - 1]);
        Some(Steer::Go(centre_of(g, chain[pick - 1]), Aim::Waypoint))
    }

    /// The point a field member stops at: its slot, once the field has them.
    pub(crate) fn arrival_point(&self, e: &Entities, i: usize) -> Option<Vec2> {
        let r = &e.route[i];
        if r.field_serial == 0 {
            return None;
        }
        let f = &self.entries[self.find(r.field_serial)?];
        f.slot_of(e, i)
    }

    /// Whether the unit in slot `i` is being moved by its field: the field is
    /// there and has integrated the unit's cell.
    pub(crate) fn ready_for(&self, graphs: &[ClassGraph], e: &Entities, i: usize) -> bool {
        let Some(fi) = self.find(e.route[i].field_serial) else {
            return false;
        };
        let f = &self.entries[fi];
        let g = &graphs[f.graph];
        let (cx, cy) = cell_of(g, e.pos[i]);
        f.region_ready && f.dir_at(&g.sectors, cx, cy) != UNSET
    }

    // ---- the checksum -------------------------------------------------------

    /// What the checksum sees of the pool and the build.
    pub(crate) fn write_hash(&self, h: &mut Fnv1a64) {
        h.write_u32(self.threshold);
        h.write_u32(self.next_serial);
        h.write_u32(self.next_born);
        h.write_u32(self.next_order);
        h.write_u32(self.clock);
        h.write_u64(self.built);
        h.write_u32(self.live());
        for (i, f) in self.entries.iter().enumerate() {
            if f.serial == 0 {
                continue;
            }
            h.write_u32(i as u32);
            h.write_u32(f.serial);
            h.write_u32(f.version);
            h.write_u32(f.born);
            h.write_u8(f.state.tag());
            h.write_u32(f.graph as u32);
            h.write_u32(f.goal);
            h.write_u32(f.layout.order);
            h.write_u8(f.layout.rank);
            h.write_fx(f.layout.spacing);
            h.write_fx(f.layout.inner);
            h.write_u32(f.members);
            h.write_u32(f.last_used);
            h.write_u8(u8::from(f.region_ready) | u8::from(f.slots_ready) << 1);
            h.write_u32(f.sectors.len() as u32);
            for &s in &f.sectors {
                h.write_u32(s);
            }
            h.write_u32(f.slots.len() as u32);
            for &p in &f.slots {
                h.write_vec2(p);
            }
            // Who holds which slot (WP4.4b): only the slots that are held, so a
            // field nobody has claimed in hashes by its slots alone.
            for (k, &holder) in f.taken.iter().enumerate() {
                if holder != claim::NO_HOLDER {
                    h.write_u32(k as u32);
                    h.write_u32(holder);
                }
            }
            h.write_u32(f.finalized);
            h.write_u64(f.digest.finish());
        }
        match &self.cur {
            Some(c) => {
                h.write_u8(1);
                h.write_u32(c.entry as u32);
                h.write_u32(c.born);
                h.write_u8(c.stage.tag());
                h.write_u32(c.cursor as u32);
                h.write_u32(c.chain.unwrap_or(u32::MAX));
                h.write_u8(u8::from(c.started) | u8::from(c.searching) << 1);
                h.write_u32(c.found);
                if c.searching
                    && let Some(s) = &self.scratch
                {
                    h.write_u32(s.local.expanded());
                }
            }
            None => h.write_u8(0),
        }
    }
}

/// The cell of a position, kept on the map.
pub(crate) fn cell_of(g: &ClassGraph, pos: Vec2) -> (u32, u32) {
    let (x, y) = pos.cell();
    (
        (x.max(0) as u32).min(g.grid.width() - 1),
        (y.max(0) as u32).min(g.grid.height() - 1),
    )
}

/// The middle of a cell of the map, by its row-major index.
pub(crate) fn centre_of(g: &ClassGraph, cell: u32) -> Vec2 {
    let (x, y) = g.grid.xy(cell);
    centre_xy(x, y)
}

/// The middle of the cell `(x, y)`.
pub(crate) fn centre_xy(x: u32, y: u32) -> Vec2 {
    Vec2::new(
        Fx::from_i64(i64::from(x)) + Fx::HALF,
        Fx::from_i64(i64::from(y)) + Fx::HALF,
    )
}
