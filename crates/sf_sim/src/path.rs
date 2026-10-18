//! Hierarchical pathfinding for single units (WP4.2).
//!
//! A unit with a walking order asks for a **route** to the order's destination.
//! The route is found in three levels, the way hierarchical A* does it, and
//! always inside a fixed budget per tick:
//!
//! * the map is cut into **sectors** of [`SECTOR_SIZE`] by [`SECTOR_SIZE`]
//!   cells; for every movement class that terrain restricts, **portals** on the
//!   borders between sectors and the **cost of crossing a sector** from portal
//!   to portal form an abstract graph, built when the world is made and
//!   rebuilt one sector (and its neighbours) at a time when a structure is
//!   placed or destroyed (by the tick, from inside the world: [`Pathing::refresh_cells`]);
//! * a request runs A* over that graph, then refines the next one or two legs
//!   cell by cell inside their sectors and pulls them taut with a line of
//!   sight, and only does the next legs when the unit has used these up;
//! * the unit follows the waypoints of its **ring** (fixed size, in its own
//!   column), steering at the front one; the destination stays in the order.
//!
//! The rules (costs, portals, tie-breaks, the budget, unreachable goals) are
//! in `docs/rules/pathfinding.md`; the stages of a request are described in `path/job.rs`.
//!
//! What is *not* pathfinding here: aircraft and any class that terrain does
//! not restrict walk the straight line, units are not obstacles (collision is
//! WP4.4), and a route made unwalkable by a change is only re-asked when the
//! change touches it (replanning is WP4.7).

mod abstract_search;
mod graph;
mod grid;
mod heap;
mod job;
mod local;
mod los;
mod route;

#[cfg(test)]
mod tests;

use sf_math::hash::Fnv1a64;
use sf_math::{Fx, Vec2};

use crate::entities::{Entities, NO_DEF};
use crate::movement::{Cost, Ground, MovementTable};
use crate::terrain::{Cell, Occupancy, Terrain};
use crate::units::UnitTable;

use graph::{ClassGraph, node_sector};
use grid::{Grid, Sectors};
use job::{Job, Mode, Progress, Work};
pub(crate) use route::{Route, RouteState, Routes};

pub use grid::SECTOR_SIZE;
pub use route::{ABS_CAP, RING_CAP};

/// Path search work a world may do in one tick, as a number of nodes: cells
/// expanded in the searches, plus cells looked at in lines of sight.
pub const DEFAULT_NODE_BUDGET: u32 = 4096;

/// The smallest budget a world accepts: a search of a whole sector must fit in
/// one tick, or a request could never finish.
pub const MIN_NODE_BUDGET: u32 = 512;

/// A destination that moves further than this many cells from the cell a
/// route was asked for makes the unit ask again.
const RETARGET_CELLS: u32 = 3;

/// What a world did with its budget; for tests and the debug panel. Not part
/// of the checksum.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct PathStats {
    /// The budget per tick.
    pub budget: u32,
    /// Nodes the last tick spent. Never more than `budget`.
    pub nodes_last_tick: u32,
    /// The most nodes any tick spent.
    pub peak_nodes: u32,
    /// Jobs finished so far (routes and refills).
    pub jobs_done: u64,
    /// Units whose route is asked for and not there yet, after the last tick.
    pub waiting: u32,
    /// Sectors rebuilt so far (not counting the creation of the world).
    pub sectors_rebuilt: u64,
}

/// Where a unit stands with its route, for the presentation and tests.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum RouteStatus {
    /// No route: the unit is not walking an order, or has not asked yet.
    None,
    /// A route is asked for and the unit has no waypoint yet: it waits.
    Waiting,
    /// The unit follows its waypoints.
    Following,
    /// The unit stands at the end of its route and the order is not done.
    Stranded,
}

/// A read-only view of a unit's route.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct RouteView {
    /// Where the unit stands with it.
    pub status: RouteStatus,
    /// The destination cannot be reached; the route leads to the reachable cell
    /// nearest to it (`goal`). This is the mark in the order state: it stays
    /// until the order is replaced.
    pub unreachable: bool,
    /// The cell the route leads to: the destination's cell, or when
    /// `unreachable` the nearest reachable one.
    pub goal: Option<Cell>,
    /// Waypoints in the ring now.
    pub waypoints: usize,
    /// Every leg is refined; what follows the ring is the destination.
    pub complete: bool,
    /// Smoothing is off for this route (a step on it was refused).
    pub cautious: bool,
}

/// A whole route, planned in one go with no budget: `World::plan_route`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PlannedRoute {
    /// The waypoints in the order a unit takes them, ending in the destination
    /// itself unless it is unreachable (then in the nearest reachable cell).
    pub waypoints: Vec<Vec2>,
    /// The destination cannot be reached.
    pub unreachable: bool,
    /// The cell the route leads to.
    pub goal: Cell,
    /// Nodes of work it took.
    pub nodes: u32,
}

/// What `go_to` should do with the unit this tick.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Steer {
    /// Walk toward this point.
    Go(Vec2, Aim),
    /// Stand still: the route is not there yet.
    Wait,
    /// Stand still: the route ended short of the destination.
    Stranded,
}

/// What the point a unit steers at is.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Aim {
    /// A waypoint of the ring; taken off the ring on arrival.
    Waypoint,
    /// The destination itself.
    Destination,
}

/// All the pathfinding state of a world: the graphs, the job in flight, the
/// budget. Immutable apart from the sector rebuilds and the service of requests.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Pathing {
    /// One graph per movement class that terrain restricts, sorted by class.
    graphs: Vec<ClassGraph>,
    work: Work,
    job: Option<Job>,
    /// Counts the changes of the graph; a route made before a change that
    /// touches it is made again.
    epoch: u32,
    /// The epoch of the last change of each sector.
    sector_epoch: Vec<u32>,
    budget: u32,
    next_ticket: u32,
    /// Some route may be waiting for work: the service looks.
    scan: bool,
    stats: PathStats,
}

impl Pathing {
    /// A world without pathfinding: no graphs, every route is the straight line.
    pub fn inactive() -> Pathing {
        Pathing {
            graphs: Vec::new(),
            work: Work::new(0),
            job: None,
            epoch: 0,
            sector_epoch: Vec::new(),
            budget: DEFAULT_NODE_BUDGET,
            next_ticket: 0,
            scan: false,
            stats: PathStats {
                budget: DEFAULT_NODE_BUDGET,
                ..PathStats::default()
            },
        }
    }

    /// Builds the graphs for every class of `movement` that some terrain type
    /// stops, on `terrain` with the structures in `occupancy`. A map without a
    /// terrain layer has none (open ground).
    pub fn build(
        terrain: &Terrain,
        movement: &MovementTable,
        occupancy: &Occupancy,
        budget: u32,
    ) -> Pathing {
        let mut p = Pathing::inactive();
        p.budget = budget;
        p.stats.budget = budget;
        if !terrain.has_layer() {
            return p;
        }
        let sectors = Sectors::new(terrain.width(), terrain.height());
        p.work = Work::new(sectors.count());
        p.sector_epoch = vec![0; sectors.count()];
        for class in movement.class_ids().filter(|&c| movement.restricts(c)) {
            let grid = class_grid(class, terrain, movement, occupancy);
            p.graphs
                .push(ClassGraph::build(class, grid, &mut p.work.local));
        }
        p
    }

    /// Whether any class has a graph.
    pub fn is_active(&self) -> bool {
        !self.graphs.is_empty()
    }

    /// Changes the budget (before the world starts).
    pub fn set_budget(&mut self, budget: u32) {
        self.budget = budget;
        self.stats.budget = budget;
    }

    pub fn budget(&self) -> u32 {
        self.budget
    }

    pub fn stats(&self) -> PathStats {
        self.stats
    }

    fn graph_index(&self, class: u16) -> Option<usize> {
        self.graphs.binary_search_by_key(&class, |g| g.class).ok()
    }

    /// Whether routes are found for this class (terrain restricts it).
    pub fn paths_class(&self, class: u16) -> bool {
        self.graph_index(class).is_some()
    }

    fn new_ticket(&mut self) -> u32 {
        self.next_ticket = self.next_ticket.wrapping_add(1);
        self.next_ticket
    }

    /// The sector a cell is in, if the world has graphs.
    pub fn sector_of(&self, cell: Cell) -> Option<u32> {
        let g = self.graphs.first()?;
        (cell.x < g.grid.width() && cell.y < g.grid.height())
            .then(|| g.sectors.of_cell(cell.x, cell.y))
    }

    // ---- changes of the map -------------------------------------------------

    /// The cells of the given sectors have changed: reads them again from the
    /// terrain and the occupancy, rewrites the portals and edges that depend on
    /// them, and marks the routes through them for a new plan. `sectors` must
    /// be ascending and without repeats.
    pub fn refresh_sectors(
        &mut self,
        terrain: &Terrain,
        movement: &MovementTable,
        occupancy: &Occupancy,
        sectors: &[u32],
    ) {
        if self.graphs.is_empty() || sectors.is_empty() {
            return;
        }
        let ground = Ground {
            terrain,
            movement,
            occupancy: Some(occupancy),
        };
        for g in &mut self.graphs {
            for &s in sectors {
                let (x0, y0, x1, y1) = g.sectors.rect(s);
                for y in y0..y1 {
                    for x in x0..x1 {
                        let speed = cell_speed(&ground, g.class, x, y);
                        let i = g.grid.index(x, y);
                        g.grid.set(i, speed);
                    }
                }
            }
            g.rebuild(sectors, &mut self.work.local);
        }
        self.epoch = self.epoch.wrapping_add(1);
        for &s in sectors {
            self.sector_epoch[s as usize] = self.epoch;
        }
        self.stats.sectors_rebuilt += sectors.len() as u64;
        self.scan = true;
    }

    /// [`Pathing::refresh_sectors`] for the sectors the given cells are in.
    pub fn refresh_cells(
        &mut self,
        terrain: &Terrain,
        movement: &MovementTable,
        occupancy: &Occupancy,
        cells: impl Iterator<Item = Cell>,
    ) {
        let mut sectors: Vec<u32> = cells.filter_map(|c| self.sector_of(c)).collect();
        sectors.sort_unstable();
        sectors.dedup();
        self.refresh_sectors(terrain, movement, occupancy, &sectors);
    }

    // ---- the service ----------------------------------------------------------

    /// Serves the requests of this tick, lowest slot first, until the budget is
    /// spent. Called once per tick after the commands and before the units
    /// move.
    pub fn serve(&mut self, e: &mut Entities, units: &UnitTable) {
        self.stats.nodes_last_tick = 0;
        if self.graphs.is_empty() || (self.job.is_none() && !self.scan) {
            return;
        }
        let mut rem = self.budget;
        let mut cursor = 0;
        loop {
            if self.job.is_none() {
                match self.next_job(e, units, &mut cursor) {
                    Some(job) => self.job = Some(job),
                    None => {
                        self.scan = false;
                        break;
                    }
                }
            }
            let Some(job) = self.job.as_mut() else { break };
            let slot = job.slot as usize;
            let before = rem;
            let progress = job.run(
                &self.graphs[job.graph],
                &mut self.work,
                &mut e.route[slot],
                &mut rem,
                self.epoch,
            );
            self.stats.nodes_last_tick += before - rem;
            match progress {
                Progress::Yield => break,
                Progress::Done => {
                    self.job = None;
                    self.stats.jobs_done += 1;
                }
                Progress::Dropped => self.job = None,
                Progress::Replan => {
                    self.job = None;
                    let ticket = self.new_ticket();
                    let route = &mut e.route[slot];
                    route.clear_plan();
                    route.state = RouteState::Wanted;
                    route.ticket = ticket;
                    // The cursor is past this unit: it is tried again next tick,
                    // and a route that cannot be made does not hold up the others.
                }
            }
        }
        self.stats.peak_nodes = self.stats.peak_nodes.max(self.stats.nodes_last_tick);
        self.stats.waiting = e
            .route
            .iter_active()
            .filter(|&(i, r)| e.alive[i] && r.state == RouteState::Wanted)
            .count() as u32;
    }

    /// The lowest slot from `cursor` on that has work waiting.
    fn next_job(&mut self, e: &Entities, units: &UnitTable, cursor: &mut usize) -> Option<Job> {
        // Slots beyond the routes' vector have never asked for one.
        while *cursor < e.route.len().min(e.len()) {
            let i = *cursor;
            *cursor += 1;
            if !e.alive[i] || e.def[i] == NO_DEF {
                continue;
            }
            let route = &e.route[i];
            let mode = match route.state {
                RouteState::Wanted => Mode::Fresh,
                RouteState::Active if !route.complete && route.ring_len() <= route::REFILL_AT => {
                    if route.truncated && route.abs_len() == 0 {
                        Mode::Continue
                    } else {
                        Mode::Refill
                    }
                }
                _ => continue,
            };
            let Some(graph) = units
                .at(e.def[i] as usize)
                .movement_class
                .and_then(|c| self.graph_index(c))
            else {
                continue;
            };
            return Some(Job::new(
                i as u32,
                route.ticket,
                graph,
                mode,
                self.epoch,
                e.pos[i],
                route,
            ));
        }
        None
    }

    // ---- what a unit asks every tick -------------------------------------------

    /// Where unit `i` of class `class` should steer to reach `destination`,
    /// asking for a route if it has none. Units without a class, and classes
    /// that terrain does not restrict, steer straight at the destination.
    pub fn steer(
        &mut self,
        e: &mut Entities,
        i: usize,
        destination: Vec2,
        class: Option<u16>,
    ) -> Steer {
        let Some(gi) = class.and_then(|c| self.graph_index(c)) else {
            return Steer::Go(destination, Aim::Destination);
        };
        let g = &self.graphs[gi];
        let want = Pathing::cell_index(g, destination);
        let route = &mut e.route[i];
        let far = |a: u32, b: u32| {
            let (ax, ay) = g.grid.xy(a);
            let (bx, by) = g.grid.xy(b);
            ax.abs_diff(bx).max(ay.abs_diff(by)) > RETARGET_CELLS
        };
        match route.state {
            RouteState::Idle => {
                self.next_ticket = self.next_ticket.wrapping_add(1);
                *route = Route::IDLE;
                route.want = want;
                route.state = RouteState::Wanted;
                route.ticket = self.next_ticket;
                self.scan = true;
                return Steer::Wait;
            }
            RouteState::Stranded if far(route.want, want) => {
                self.next_ticket = self.next_ticket.wrapping_add(1);
                let cautious = route.cautious;
                *route = Route::IDLE;
                route.cautious = cautious;
                route.want = want;
                route.state = RouteState::Wanted;
                route.ticket = self.next_ticket;
                self.scan = true;
                return Steer::Wait;
            }
            RouteState::Wanted | RouteState::Active if far(route.want, want) => {
                // The destination moved: ask again, and keep following the
                // waypoints there are until the new route is ready.
                self.next_ticket = self.next_ticket.wrapping_add(1);
                route.want = want;
                route.state = RouteState::Wanted;
                route.ticket = self.next_ticket;
                self.scan = true;
            }
            _ => {}
        }
        // A change of the map since the route was made: is it in the way? A
        // route that waits for a search (its destination moved) still follows
        // its old waypoints, and one whose search is half done has the first
        // legs of the new route on the ring: both are checked as well, against
        // the epoch they were made in. One with no waypoints has nothing to
        // walk through, and its search notices the change itself.
        let (since, own_job) = match &self.job {
            Some(j) if route.state == RouteState::Wanted && j.is_for(i as u32, route.ticket) => {
                (j.epoch(), true)
            }
            _ => (route.epoch, false),
        };
        if since != self.epoch
            && (matches!(route.state, RouteState::Active | RouteState::Stranded)
                || (route.state == RouteState::Wanted && route.ring_len() > 0))
        {
            let pos = Pathing::cell_index(g, e.pos[i]);
            if self.touched(g, &e.route[i], pos, since) {
                self.next_ticket = self.next_ticket.wrapping_add(1);
                let route = &mut e.route[i];
                route.clear_plan();
                route.state = RouteState::Wanted;
                route.ticket = self.next_ticket;
                self.scan = true;
            } else if !own_job {
                e.route[i].epoch = self.epoch;
            }
        }
        let route = &e.route[i];
        match route.state {
            RouteState::Wanted => match route.ring_front() {
                Some(c) => Steer::Go(Pathing::centre_of(g, c), Aim::Waypoint),
                None => Steer::Wait,
            },
            RouteState::Active => {
                if !route.complete && route.ring_len() <= route::REFILL_AT {
                    self.scan = true;
                }
                if let Some(c) = route.ring_front() {
                    Steer::Go(Pathing::centre_of(g, c), Aim::Waypoint)
                } else if route.complete {
                    if route.unreachable {
                        e.route[i].state = RouteState::Stranded;
                        Steer::Stranded
                    } else {
                        Steer::Go(destination, Aim::Destination)
                    }
                } else {
                    Steer::Wait
                }
            }
            RouteState::Stranded => Steer::Stranded,
            RouteState::Idle => Steer::Wait,
        }
    }

    /// The unit reached the front waypoint of its ring.
    pub fn reached(&mut self, e: &mut Entities, i: usize) {
        let route = &mut e.route[i];
        route.ring_pop_front();
        if !route.complete && route.ring_len() <= route::REFILL_AT {
            self.scan = true;
        }
    }

    /// The unit was refused the step toward its waypoint. A smoothed route is
    /// asked for again cell by cell; a route that was cell by cell already
    /// ends: the unit stands where it is with its order kept.
    pub fn blocked(&mut self, e: &mut Entities, i: usize) {
        let route = &mut e.route[i];
        if route.state == RouteState::Idle {
            return;
        }
        if route.cautious {
            route.state = RouteState::Stranded;
            return;
        }
        self.next_ticket = self.next_ticket.wrapping_add(1);
        route.clear_plan();
        route.cautious = true;
        route.state = RouteState::Wanted;
        route.ticket = self.next_ticket;
        self.scan = true;
    }

    fn cell_index(g: &ClassGraph, pos: Vec2) -> u32 {
        let (x, y) = pos.cell();
        let x = (x.max(0) as u32).min(g.grid.width() - 1);
        let y = (y.max(0) as u32).min(g.grid.height() - 1);
        g.grid.index(x, y)
    }

    fn centre_of(g: &ClassGraph, cell: u32) -> Vec2 {
        let (x, y) = g.grid.xy(cell);
        Vec2::new(
            Fx::from_i64(i64::from(x)) + Fx::HALF,
            Fx::from_i64(i64::from(y)) + Fx::HALF,
        )
    }

    /// Whether a change of the map since `route` was made is near enough to
    /// it to matter: some sector within one of a sector the route, the unit or
    /// the goal is in changed after the route was made. Line-of-sight segments
    /// are at most one sector long, so this covers everything they cross.
    fn touched(&self, g: &ClassGraph, route: &Route, pos: u32, since: u32) -> bool {
        let mut near = |cell: u32| {
            let (x, y) = g.grid.xy(cell);
            let s = g.sectors.of_cell(x, y);
            let (sx, sy) = g.sectors.coords(s);
            for ny in sy.saturating_sub(1)..=(sy + 1).min(g.sectors.down - 1) {
                for nx in sx.saturating_sub(1)..=(sx + 1).min(g.sectors.across - 1) {
                    if self.sector_epoch[(ny * g.sectors.across + nx) as usize] > since {
                        return true;
                    }
                }
            }
            false
        };
        if near(pos) || near(route.goal) || near(route.want) || near(route.frontier) {
            return true;
        }
        for k in 0..route.ring_len() {
            if route.ring_get(k).is_some_and(&mut near) {
                return true;
            }
        }
        for k in 0..route.abs_len() {
            if let Some(n) = route.abs_get(k) {
                let p = g.sector[node_sector(n) as usize].portal[graph::node_slot(n)];
                if p == graph::NO_PORTAL || near(p) {
                    return true;
                }
            }
        }
        false
    }

    // ---- reading ----------------------------------------------------------------

    /// A view of the route of unit `i`.
    pub fn view(&self, e: &Entities, i: usize) -> RouteView {
        let route = &e.route[i];
        let status = match route.state {
            RouteState::Idle => RouteStatus::None,
            RouteState::Stranded => RouteStatus::Stranded,
            RouteState::Wanted | RouteState::Active => {
                if route.ring_len() == 0 && !route.complete {
                    RouteStatus::Waiting
                } else {
                    RouteStatus::Following
                }
            }
        };
        let goal = match (self.graphs.first(), status) {
            (Some(g), RouteStatus::Following | RouteStatus::Stranded) => {
                let (x, y) = g.grid.xy(route.goal);
                Some(Cell::new(x, y))
            }
            _ => None,
        };
        RouteView {
            status,
            unreachable: route.unreachable,
            goal,
            waypoints: route.ring_len(),
            complete: route.complete,
            cautious: route.cautious,
        }
    }

    /// Whether a unit of `class` could walk the straight line from `from` to
    /// `to` by the rules the path finder pulls routes taut with: no cell it
    /// cannot enter (a structure included) within a quarter of a cell of the
    /// line, and no longer than one sector. `false` for a class without a graph
    /// or a point outside the map. Read only.
    pub fn sight_clear(&self, class: u16, from: Vec2, to: Vec2) -> bool {
        let Some(gi) = self.graph_index(class) else {
            return false;
        };
        let g = &self.graphs[gi];
        let inside = |p: Vec2| {
            let (x, y) = p.cell();
            x >= 0 && y >= 0 && (x as u32) < g.grid.width() && (y as u32) < g.grid.height()
        };
        inside(from)
            && inside(to)
            && los::short_enough(from, to)
            && los::thick_clear(&g.grid, from, to).0
    }

    /// Plans a whole route for class `class` with no budget and no unit: the
    /// waypoints a unit would take, in order. `None` for a class that has no
    /// graph or a point outside the map. Meant for tests and tools (the unit
    /// itself is served by [`Pathing::serve`]); it does not touch the world.
    pub fn plan(&self, class: u16, from: Vec2, to: Vec2) -> Option<PlannedRoute> {
        let gi = self.graph_index(class)?;
        let g = &self.graphs[gi];
        let inside = |p: Vec2| {
            let (x, y) = p.cell();
            x >= 0 && y >= 0 && (x as u32) < g.grid.width() && (y as u32) < g.grid.height()
        };
        if !inside(from) || !inside(to) {
            return None;
        }
        let mut work = Work::new(g.sectors.count());
        let mut route = Route::IDLE;
        route.state = RouteState::Wanted;
        route.want = Pathing::cell_index(g, to);
        route.ticket = 1;
        let mut mode = Mode::Fresh;
        let mut waypoints = Vec::new();
        let mut nodes: u32 = 0;
        loop {
            let mut job = Job::new(0, 1, gi, mode, self.epoch, from, &route);
            let mut rem = u32::MAX;
            let progress = job.run(g, &mut work, &mut route, &mut rem, self.epoch);
            nodes = nodes.saturating_add(u32::MAX - rem);
            if progress != Progress::Done {
                return None;
            }
            // The unit walks the ring down to the refill mark.
            while route.ring_len() > route::REFILL_AT || (route.complete && route.ring_len() > 0) {
                if let Some(c) = route.ring_pop_front() {
                    waypoints.push(Pathing::centre_of(g, c));
                }
            }
            if route.complete {
                break;
            }
            mode = if route.truncated && route.abs_len() == 0 {
                Mode::Continue
            } else {
                Mode::Refill
            };
        }
        if !route.unreachable {
            waypoints.push(to);
        }
        let (gx, gy) = g.grid.xy(route.goal);
        Some(PlannedRoute {
            waypoints,
            unreachable: route.unreachable,
            goal: Cell::new(gx, gy),
            nodes,
        })
    }

    // ---- the checksum -------------------------------------------------------------

    /// Writes the part of the state the checksum covers: the budget, the
    /// epoch, the job in flight and every route that exists.
    pub fn write_hash(&self, h: &mut Fnv1a64, e: &Entities) {
        h.write_u32(self.budget);
        h.write_u32(self.epoch);
        h.write_u32(self.next_ticket);
        match &self.job {
            Some(job) => {
                h.write_u8(1);
                job.write_hash(h);
            }
            None => h.write_u8(0),
        }
        let routed = || e.route.iter_active().filter(|&(i, _)| e.alive[i]);
        h.write_u64(routed().count() as u64);
        for (i, route) in routed() {
            h.write_u32(i as u32);
            route.write_hash(h);
        }
    }

    /// Whether any live unit has a route (the checksum has a section then).
    pub fn any_route(&self, e: &Entities) -> bool {
        self.is_active() && e.route.iter_active().any(|(i, _)| e.alive[i])
    }
}

/// The speed of a cell for a class, in thousandths: zero where the class
/// cannot go, and on a cell a structure occupies.
fn cell_speed(ground: &Ground<'_>, class: u16, x: u32, y: u32) -> u16 {
    match ground.cost(class, Cell::new(x, y)) {
        Cost::Speed(p) => p,
        Cost::Impassable => 0,
    }
}

/// The grid of a class on a terrain with its structures.
fn class_grid(
    class: u16,
    terrain: &Terrain,
    movement: &MovementTable,
    occupancy: &Occupancy,
) -> Grid {
    let ground = Ground {
        terrain,
        movement,
        occupancy: Some(occupancy),
    };
    let mut grid = Grid::new(terrain.width(), terrain.height());
    for y in 0..terrain.height() {
        for x in 0..terrain.width() {
            let speed = cell_speed(&ground, class, x, y);
            let i = grid.index(x, y);
            grid.set(i, speed);
        }
    }
    grid
}
