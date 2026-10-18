//! The builder of flowfields: a resumable job, one field at a time.
//!
//! A field is made in stages (`Stage` in `field.rs`), each of which spends from
//! the node budget it is given and can stop wherever the budget runs out and go
//! on next tick, so a build takes as many ticks as the budget allows and gives
//! the same field whatever the slices:
//!
//! * (1) **Gather**: look at the units of the group (their route says which field
//!   they follow) and mark the sectors they stand in. A group with no unit left
//!   is dropped. One node pays for 16 slots looked at.
//! * (2) **GoalLinks**: a Dijkstra from the goal cell inside its sector to the
//!   portals of that sector (`Local::advance`, as a request does).
//! * (3) **Corridor**: the abstract Dijkstra from those portals until every marked
//!   sector has been reached (`path/corridor.rs`).
//! * (4) **Collect**: the portals on the paths back from the reached sectors mark
//!   the sectors a unit of the group crosses. One node per portal.
//! * (5) **Region**: the marked sectors and one ring of neighbours round them,
//!   ascending, cut short at [`FIELD_SECTORS`]; a corridor of more sectors than
//!   that gets no field. A fixed charge.
//! * (6) **Slots**: the arrival slots, each one a line of sight from the middle of
//!   the goal cell, charged as the cells looked at (`path/slots.rs`).
//! * (7) **Integrate**: Dijkstra over the cells of the region from the goal, one
//!   node per cell. As a cell is integrated its direction is chosen
//!   ([`Field::best_dir`]) and it is ready for the units standing in it.
//!
//! The scratch memory (the searches' arrays) is made when the first field is.

use sf_math::hash::Fnv1a64;
use sf_math::{Fx, Vec2};

use crate::entities::Entities;
use crate::path::corridor::{Corridor, Reach};
use crate::path::field::{
    Cur, FIELD_SECTORS, Field, FieldState, Fields, GOAL, INF, MAX_SLOTS, Stage, UNSET, cell_of,
    centre_xy,
};
use crate::path::graph::{ClassGraph, NO_PORTAL, SLOTS, node, node_sector};
use crate::path::grid::{DIRS, SECTOR_SIZE};
use crate::path::heap::IndexedHeap;
use crate::path::local::{Local, Query};
use crate::path::{los, slots};

/// Slots looked at for one node of the budget while gathering.
const GATHER_PER_NODE: usize = 16;

/// The memory a build uses; one per world, made on first use.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Scratch {
    pub local: Local,
    /// The slots of the earlier classes of the order being picked for: the points
    /// and the spacing of their lattice.
    taken: Vec<(Vec2, Fx)>,
    corridor: Corridor,
    /// Per portal node: the generation of the walk that marked it.
    walked: Vec<u32>,
    walk_generation: u32,
    /// Per sector: bit 0 on a path, bit 1 in the margin.
    mark: Vec<u8>,
    goal_links: Vec<(u32, u32)>,
    /// The sectors of the region being made.
    region: Vec<u32>,
    wanted: [u16; SLOTS],
    wanted_len: usize,
    /// The heap of the integration, over the cells of the largest region.
    heap: IndexedHeap,
}

impl Scratch {
    fn new(g: &ClassGraph) -> Scratch {
        let sectors = g.sectors.count();
        Scratch {
            local: Local::new(),
            taken: Vec::new(),
            corridor: Corridor::new(sectors),
            walked: vec![0; sectors * SLOTS],
            walk_generation: 0,
            mark: vec![0; sectors],
            goal_links: Vec::with_capacity(SLOTS),
            region: Vec::with_capacity(FIELD_SECTORS.min(sectors)),
            wanted: [0; SLOTS],
            wanted_len: 0,
            heap: IndexedHeap::new(
                FIELD_SECTORS.min(sectors) * (SECTOR_SIZE * SECTOR_SIZE) as usize,
            ),
        }
    }
}

/// What a stage did.
enum Flow {
    /// On to the next stage (or the next call of this one).
    Next,
    /// The budget ran out.
    Yield,
    /// The field cannot be made: its units are routed one by one.
    Abort,
    /// The field is finished.
    Done,
}

impl Fields {
    /// Works on the builds while `rem` lasts: the field in flight, then the next
    /// in the queue. `e` is read to find the units of a group.
    pub(crate) fn advance(&mut self, graphs: &[ClassGraph], e: &Entities, rem: &mut u32) {
        let before = *rem;
        while *rem > 0 {
            if self.cur.is_none() && !self.start_next() {
                break;
            }
            let Fields {
                entries,
                cur,
                scratch,
                spiral,
                ..
            } = self;
            let Some(c) = cur.as_mut() else { break };
            if !entries
                .get(c.entry)
                .is_some_and(|f| f.serial != 0 && f.born == c.born)
            {
                *cur = None;
                continue;
            }
            let entry = c.entry;
            // The slots of the earlier classes of this order, for the stage that
            // picks the slots of this one (WP4.4): no two classes share a point
            // or stand closer than their bodies allow.
            if c.stage == Stage::Slots && !c.started {
                let (order, rank) = (entries[entry].layout.order, entries[entry].layout.rank);
                let g = &graphs[entries[entry].graph];
                let s = scratch.get_or_insert_with(|| Box::new(Scratch::new(g)));
                s.taken.clear();
                for (j, o) in entries.iter().enumerate() {
                    if j != entry
                        && order != 0
                        && o.serial != 0
                        && o.slots_ready
                        && o.layout.order == order
                        && o.layout.rank < rank
                    {
                        s.taken
                            .extend(o.slots.iter().map(|&p| (p, o.layout.spacing)));
                    }
                }
            }
            let f = &mut entries[entry];
            let g = &graphs[f.graph];
            let s = scratch.get_or_insert_with(|| Box::new(Scratch::new(g)));
            let flow = match c.stage {
                Stage::Gather => gather(f, c, s, g, e, rem),
                Stage::GoalLinks => goal_links(f, c, s, g, rem),
                Stage::Corridor => corridor(c, s, g, rem),
                Stage::Collect => collect(c, s, g, rem),
                Stage::Region => region(f, c, s, g, rem),
                Stage::Slots => pick_slots(f, c, g, spiral, &s.taken, rem),
                Stage::Integrate => integrate(f, c, s, g, rem),
            };
            match flow {
                Flow::Next => {}
                Flow::Yield => break,
                Flow::Abort => {
                    self.free_entry_public(entry);
                }
                Flow::Done => {
                    self.finish(entry);
                }
            }
        }
        self.nodes_last_tick += before - *rem;
    }

    /// The next queued field, oldest first, becomes the build in flight.
    fn start_next(&mut self) -> bool {
        let next = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, f)| f.serial != 0 && f.state == FieldState::Queued)
            .min_by_key(|&(i, f)| (f.born, i))
            .map(|(i, f)| (i, f.born));
        let Some((i, born)) = next else { return false };
        self.entries[i].state = FieldState::Building;
        self.entries[i].clear_build();
        self.cur = Some(Cur::new(i, born));
        true
    }

    /// A field that cannot be made is dropped; its units go on alone.
    fn free_entry_public(&mut self, i: usize) {
        self.entries[i].serial = 0;
        self.entries[i].clear_build();
        self.entries[i].state = FieldState::Queued;
        self.cur = None;
    }

    /// The build of entry `i` is over: it replaces the older ones of its group.
    fn finish(&mut self, i: usize) {
        self.entries[i].state = FieldState::Ready;
        self.built += 1;
        self.cur = None;
        let (serial, version) = (self.entries[i].serial, self.entries[i].version);
        for (j, o) in self.entries.iter_mut().enumerate() {
            if j != i && o.serial == serial && o.version < version {
                o.serial = 0;
                o.clear_build();
                o.state = FieldState::Queued;
            }
        }
    }
}

/// Stage one of the build.
fn gather(
    f: &mut Field,
    c: &mut Cur,
    s: &mut Scratch,
    g: &ClassGraph,
    e: &Entities,
    rem: &mut u32,
) -> Flow {
    if !c.started {
        s.corridor.clear_targets();
        c.started = true;
        c.cursor = 0;
        c.found = 0;
    }
    let total = e.route.len().min(e.len());
    let end = c
        .cursor
        .saturating_add((*rem as usize).saturating_mul(GATHER_PER_NODE))
        .min(total);
    let (gx, gy) = g.grid.xy(f.goal);
    let goal_sector = g.sectors.of_cell(gx, gy);
    for i in c.cursor..end {
        if e.alive[i] && e.route[i].field_serial == f.serial {
            c.found += 1;
            let (x, y) = cell_of(g, e.pos[i]);
            let sector = g.sectors.of_cell(x, y);
            if sector != goal_sector {
                s.corridor.targets[sector as usize] = true;
            }
        }
    }
    *rem -= ((end - c.cursor).div_ceil(GATHER_PER_NODE) as u32).min(*rem);
    c.cursor = end;
    if end < total {
        return Flow::Yield;
    }
    if c.found == 0 {
        return Flow::Abort;
    }
    c.started = false;
    c.stage = Stage::GoalLinks;
    Flow::Next
}

/// Stage two of the build.
fn goal_links(f: &Field, c: &mut Cur, s: &mut Scratch, g: &ClassGraph, rem: &mut u32) -> Flow {
    let (gx, gy) = g.grid.xy(f.goal);
    let gs = g.sectors.of_cell(gx, gy);
    let rect = g.sectors.rect(gs);
    if !c.started {
        s.wanted_len = 0;
        for k in 0..SLOTS {
            let p = g.sector[gs as usize].portal[k];
            if p != NO_PORTAL {
                let (x, y) = g.grid.xy(p);
                s.wanted[s.wanted_len] = Local::number(rect, x, y);
                s.wanted_len += 1;
            }
        }
        c.started = true;
        c.searching = false;
    }
    let query = Query {
        rect,
        src: (gx, gy),
        goal: None,
        toward: None,
        wanted: &s.wanted[..s.wanted_len],
    };
    if !c.searching {
        s.local.begin(&g.grid, &g.heur, &query);
        c.searching = true;
    }
    let before = s.local.expanded();
    let done = s.local.advance(&g.grid, &g.heur, &query, *rem);
    *rem -= (s.local.expanded() - before).min(*rem);
    if done.is_none() {
        return Flow::Yield;
    }
    c.searching = false;
    c.started = false;
    s.goal_links.clear();
    for k in 0..SLOTS {
        let p = g.sector[gs as usize].portal[k];
        if p == NO_PORTAL {
            continue;
        }
        let (x, y) = g.grid.xy(p);
        let d = s.local.dist[usize::from(Local::number(rect, x, y))];
        if d != INF {
            s.goal_links.push((node(gs, k), d));
        }
    }
    c.stage = Stage::Corridor;
    Flow::Next
}

/// Stage three of the build.
fn corridor(c: &mut Cur, s: &mut Scratch, g: &ClassGraph, rem: &mut u32) -> Flow {
    if !c.started {
        s.corridor.begin(&s.goal_links);
        c.started = true;
    }
    if s.corridor.run(g, rem) == Reach::Yield {
        return Flow::Yield;
    }
    c.started = false;
    c.cursor = 0;
    c.chain = None;
    s.walk_generation = s.walk_generation.wrapping_add(1);
    if s.walk_generation == 0 {
        s.walked.iter_mut().for_each(|w| *w = 0);
        s.walk_generation = 1;
    }
    s.mark.iter_mut().for_each(|m| *m = 0);
    c.stage = Stage::Collect;
    Flow::Next
}

/// Stage four of the build.
fn collect(c: &mut Cur, s: &mut Scratch, g: &ClassGraph, rem: &mut u32) -> Flow {
    let count = g.sectors.count();
    loop {
        if let Some(n) = c.chain {
            if *rem == 0 {
                return Flow::Yield;
            }
            *rem -= 1;
            if s.walked[n as usize] == s.walk_generation {
                c.chain = None;
                continue;
            }
            s.walked[n as usize] = s.walk_generation;
            s.mark[node_sector(n) as usize] |= 1;
            c.chain = s.corridor.toward_goal(n);
            continue;
        }
        let mut found = false;
        while c.cursor < count {
            let sector = c.cursor;
            c.cursor += 1;
            if s.corridor.targets[sector]
                && let Some(n) = s.corridor.reached(sector as u32)
            {
                c.chain = Some(n);
                found = true;
                break;
            }
        }
        if !found {
            c.stage = Stage::Region;
            return Flow::Next;
        }
    }
}

/// Stage five of the build.
fn region(f: &mut Field, c: &mut Cur, s: &mut Scratch, g: &ClassGraph, rem: &mut u32) -> Flow {
    let count = g.sectors.count();
    let charge = (count.div_ceil(32) as u32).max(1);
    if *rem < charge {
        return Flow::Yield;
    }
    *rem -= charge;
    let (gx, gy) = g.grid.xy(f.goal);
    s.mark[g.sectors.of_cell(gx, gy) as usize] |= 1;
    let cap = FIELD_SECTORS.min(count);
    let mut total = s.mark.iter().filter(|&&m| m & 1 != 0).count();
    if total > cap {
        return Flow::Abort;
    }
    // One ring of margin, in ascending order of the sector it surrounds, while
    // there is room.
    for sector in 0..count as u32 {
        if s.mark[sector as usize] & 1 == 0 {
            continue;
        }
        let (sx, sy) = g.sectors.coords(sector);
        for ny in sy.saturating_sub(1)..=(sy + 1).min(g.sectors.down - 1) {
            for nx in sx.saturating_sub(1)..=(sx + 1).min(g.sectors.across - 1) {
                let n = (ny * g.sectors.across + nx) as usize;
                if s.mark[n] == 0 && total < cap {
                    s.mark[n] |= 2;
                    total += 1;
                }
            }
        }
    }
    s.region.clear();
    s.region
        .extend((0..count as u32).filter(|&sector| s.mark[sector as usize] != 0));
    f.set_region(&s.region);
    c.stage = Stage::Slots;
    c.cursor = 0;
    c.started = false;
    Flow::Next
}

/// Stage six of the build.
///
/// The slots are the points of the field's lattice, scanned outwards from the radius
/// where its ring begins (`Layout::inner`), that are in sight of the goal and not too
/// near a slot of an earlier class of the same order (`taken`: the point and the
/// spacing of its lattice). The scan stops when there are as many slots as members
/// or the points are further than [`slots::MAX_RADIUS`] from the goal; units beyond
/// the slots there are share them round again.
fn pick_slots(
    f: &mut Field,
    c: &mut Cur,
    g: &ClassGraph,
    spiral: &[slots::Point],
    taken: &[(Vec2, Fx)],
    rem: &mut u32,
) -> Flow {
    let (gx, gy) = g.grid.xy(f.goal);
    let centre = centre_xy(gx, gy);
    let spacing = f.layout.spacing;
    if !c.started {
        f.slots.clear();
        c.started = true;
        c.cursor = slots::first_beyond(spiral, f.layout.inner, spacing);
    }
    let want = (f.members as usize).min(MAX_SLOTS);
    let len = spiral.len();
    // One node pays for 32 comparisons with the earlier classes' slots.
    let compare = (taken.len() / 32) as u32;
    while f.slots.len() < want && c.cursor < len {
        let p = slots::position(centre, spiral[c.cursor], spacing);
        if (p - centre).length() > slots::MAX_RADIUS {
            // The spiral goes outwards: everything after this is further.
            c.cursor = len;
            break;
        }
        if *rem < los::bound(centre, p) + compare {
            return Flow::Yield;
        }
        *rem -= compare;
        let crowded = taken.iter().any(|&(q, other)| {
            let apart = (spacing + other).div_int(2) - slots::GAP;
            (p - q).length_squared() < apart.square()
        });
        c.cursor += 1;
        if crowded {
            continue;
        }
        let (clear, used) = los::thick_clear(&g.grid, centre, p);
        *rem -= used.min(*rem);
        if clear {
            f.slots.push(p);
        }
    }
    if f.slots.is_empty() {
        f.slots.push(centre);
    }
    f.slots_ready = true;
    c.started = false;
    c.stage = Stage::Integrate;
    Flow::Next
}

/// Stage seven of the build.
fn integrate(f: &mut Field, c: &mut Cur, s: &mut Scratch, g: &ClassGraph, rem: &mut u32) -> Flow {
    let (gx, gy) = g.grid.xy(f.goal);
    if !c.started {
        let Some(goal) = f.index(&g.sectors, gx, gy) else {
            return Flow::Abort;
        };
        s.heap.clear();
        f.cost[goal] = 0;
        s.heap.push_or_lower(goal as u32, (0, 0));
        f.finalized = 0;
        f.digest = Fnv1a64::new();
        c.started = true;
    }
    while *rem > 0 {
        let Some(top) = s.heap.pop() else {
            return Flow::Done;
        };
        *rem -= 1;
        let at = top as usize;
        let (x, y) = f.xy(&g.sectors, at);
        let cost = f.cost[at];
        let dir = if x == gx && y == gy {
            GOAL
        } else {
            f.best_dir(g, x, y)
        };
        f.dir[at] = dir;
        f.finalized += 1;
        f.digest.write_u32(top);
        f.digest.write_u32(cost);
        f.digest.write_u8(dir);
        // A unit on a neighbour steps to this cell: the neighbour is a cell
        // from which that step is allowed.
        for &(dx, dy) in &DIRS {
            let (nx, ny) = (i64::from(x) + dx, i64::from(y) + dy);
            if nx < 0
                || ny < 0
                || nx >= i64::from(g.grid.width())
                || ny >= i64::from(g.grid.height())
            {
                continue;
            }
            let (nx, ny) = (nx as u32, ny as u32);
            let Some(n) = f.index(&g.sectors, nx, ny) else {
                continue;
            };
            if f.dir[n] != UNSET || !g.grid.can_step(nx, ny, -dx, -dy) {
                continue;
            }
            let cand = cost.saturating_add(g.grid.step_cost(nx, ny, -dx, -dy));
            if cand < f.cost[n] {
                f.cost[n] = cand;
                s.heap.push_or_lower(n as u32, (u64::from(cand), 0));
            }
        }
    }
    if s.heap.is_empty() {
        Flow::Done
    } else {
        Flow::Yield
    }
}
