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
//! A group of units sent to one place by one order does not ask for a route each
//! (WP4.3): the world builds one **flowfield** for each class with at least a
//! threshold of them, over the sectors of the abstract corridor (`path/field.rs`,
//! built by a resumable job in `path/field_build.rs`, inside the same node budget),
//! and the units follow it to their own arrival slot (`path/slots.rs`).
//!
//! What is *not* pathfinding here: aircraft and any class that terrain does
//! not restrict walk the straight line, routes and fields do not know the other
//! units (they are bodies that the crowd pass of WP4.4 pushes apart, `crate::crowd`;
//! a unit that is stuck in a crowd asks for a new route through
//! [`Pathing::request_replan`], which carries the bodies that do not move as an overlay of
//! blocked cells for that request only, `path/replan.rs`), and a route made unwalkable by
//! a change is re-asked when the change touches it. A target that moves is searched for
//! again behind a cooldown (WP4.7).

mod abstract_search;
mod claim;
mod corridor;
mod field;
mod field_build;
mod graph;
mod grid;
mod heap;
mod job;
mod local;
mod los;
mod replan;
mod route;
mod slots;

#[cfg(test)]
mod field_tests;
#[cfg(test)]
mod tests;

use sf_math::hash::Fnv1a64;
use sf_math::{Facing, Fx, Vec2};

use crate::entities::{Entities, NO_DEF};
use crate::movement::{Cost, Ground, MovementTable};
use crate::terrain::{Cell, Occupancy, Terrain};
use crate::units::UnitTable;

use field::{Fields, Layout, centre_xy};
use graph::{ClassGraph, node_sector};
use grid::{Grid, Sectors};
use job::{Job, Mode, Progress, Work};
pub(crate) use replan::{OVERLAY_RANGE, Overlay, OverlayBuilder, ShutCells};
pub(crate) use route::{Route, RouteState, Routes};
pub(crate) use slots::spacing_for;

pub use field::{
    DEFAULT_FLOWFIELD_THRESHOLD, FIELD_POOL, FIELD_SECTORS, IDLE_TICKS as FIELD_IDLE_TICKS,
    MAX_SLOTS as MAX_FIELD_SLOTS,
};
pub use grid::SECTOR_SIZE;
pub use replan::{
    DEFAULT_REPLAN_COOLDOWN, DEFAULT_REPLAN_MAX, DEFAULT_RETARGET_COOLDOWN, MAX_REPLAN_COOLDOWN,
    MAX_REPLAN_MAX, OVERLAY_CAP, ReplanConfig,
};
pub use route::{ABS_CAP, RING_CAP};

/// Path search work a world may do in one tick, as a number of nodes: cells
/// expanded in the searches, plus cells looked at in lines of sight.
pub const DEFAULT_NODE_BUDGET: u32 = 4096;

/// The smallest budget a world accepts. A line of sight (60 nodes at most) is
/// the one step the service cannot cut, so it must fit in a tick or a request
/// could never finish; the minimum leaves room to make progress besides.
pub const MIN_NODE_BUDGET: u32 = 512;

/// A waypoint of a route farther than this many cells from a unit that has been away from
/// its route is not trusted (`Pathing::way_is_stale`).
const STALE_WAYPOINT: Fx = Fx::from_int(12);

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
    /// Nodes of the last tick that went to building flowfields (part of
    /// `nodes_last_tick`).
    pub field_nodes_last_tick: u32,
    /// Flowfields in the pool now, finished or not.
    pub fields_live: u32,
    /// Flowfields finished so far, rebuilds included.
    pub fields_built: u64,
    /// Units that asked again for a route because a change of the map touched it
    /// (WP4.7).
    pub map_replans: u64,
    /// Units that asked again for a route because the target they go for moved
    /// (WP4.7), after the cooldown.
    pub target_replans: u64,
    /// Requests of stuck units for a new route (WP4.7): with the bodies round them as an
    /// overlay, or without when nothing that stands still was near.
    pub stuck_replans: u64,
    /// Of those, the ones that carried an overlay.
    pub overlay_replans: u64,
    /// Requests with an overlay that found no way round (WP4.7).
    pub detours_failed: u64,
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
    /// The unit follows the flowfield of its group (WP4.3) instead of a route
    /// of its own. `waypoints` is then at most the one it steers at, and
    /// `complete` is true.
    pub field: bool,
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
    /// Where the service looks for the next request: the slot after the one
    /// it started last. Hashed.
    next_slot: u32,
    /// Some route may be waiting for work: the service looks.
    scan: bool,
    /// The flowfields of group orders (WP4.3).
    fields: Fields,
    /// The settings of the replan (WP4.7).
    replan: ReplanConfig,
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
            next_slot: 0,
            scan: false,
            fields: Fields::new(DEFAULT_FLOWFIELD_THRESHOLD),
            replan: ReplanConfig::DEFAULT,
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
        flowfield_threshold: u32,
    ) -> Pathing {
        let mut p = Pathing::inactive();
        p.budget = budget;
        p.stats.budget = budget;
        p.fields.threshold = flowfield_threshold;
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

    /// Changes the flowfield threshold (before the world starts).
    pub fn set_flowfield_threshold(&mut self, threshold: u32) {
        self.fields.threshold = threshold;
    }

    pub fn flowfield_threshold(&self) -> u32 {
        self.fields.threshold
    }

    /// Changes the replan settings (before the world starts).
    pub fn set_replan(&mut self, config: ReplanConfig) {
        self.replan = config;
    }

    pub fn replan_config(&self) -> ReplanConfig {
        self.replan
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
        self.fields.changed(&self.graphs, sectors);
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

    /// Serves the requests of this tick until the budget is spent, going round
    /// the slots from the one after the last request that was started (the
    /// lowest slot first when nothing was served yet), so that the highest ids
    /// do not always come last. Called once per tick after the commands and
    /// before the units move.
    pub fn serve(&mut self, e: &mut Entities, units: &UnitTable) {
        self.stats.nodes_last_tick = 0;
        self.stats.field_nodes_last_tick = 0;
        self.fields.tick();
        let fields_work = !self.graphs.is_empty() && self.fields.has_work();
        if self.graphs.is_empty() || (self.job.is_none() && !self.scan && !fields_work) {
            self.stats.fields_live = self.fields.live();
            return;
        }
        let mut rem = self.budget;
        let routes_wait = self.job.is_some() || self.scan;
        // The flowfield builder shares the budget: half of it while routes wait
        // for theirs, all of it when nothing does. What it leaves is the routes'
        // and what they leave is its (below), so a tick that has work for either
        // spends its budget to the last node.
        if fields_work {
            let mut share = if routes_wait { rem / 2 } else { rem };
            let given = share;
            self.fields.advance(&self.graphs, e, &mut share);
            rem -= given - share;
        }
        // The slots looked at this tick, counted from `next_slot`: a pass
        // visits each slot once, so a unit that needs a refill again at once
        // waits for the next tick.
        let (start, mut looked) = (self.next_slot as usize, 0);
        loop {
            // No request waits, or everything is spent: the rest waits for the
            // next tick.
            if !routes_wait || rem == 0 {
                break;
            }
            if self.job.is_none() {
                match self.next_job(e, units, start, &mut looked) {
                    Some(job) => self.job = Some(job),
                    None => {
                        self.scan = false;
                        break;
                    }
                }
            }
            let Some(job) = self.job.as_mut() else { break };
            let slot = job.slot as usize;
            // The bodies round a stuck unit are blocked cells for its request alone
            // (WP4.7): shut while its job works, open again before anything else is
            // served, so the graph and every other request see the map as it is.
            let overlay = e.route[slot].overlay;
            let shut = !overlay.is_empty() && e.route[slot].ticket == job.ticket;
            // The guard opens the cells again when it is dropped, a panic of the job
            // included. It holds the graph, which the job reads through it.
            let progress = if shut {
                let guard = ShutCells::new(&mut self.graphs[job.graph], overlay);
                job.run(
                    guard.graph(),
                    &mut self.work,
                    &mut e.route[slot],
                    &mut rem,
                    self.epoch,
                )
            } else {
                job.run(
                    &self.graphs[job.graph],
                    &mut self.work,
                    &mut e.route[slot],
                    &mut rem,
                    self.epoch,
                )
            };
            match progress {
                Progress::Yield => break,
                Progress::Done => {
                    self.job = None;
                    self.stats.jobs_done += 1;
                }
                Progress::Dropped => self.job = None,
                Progress::Failed => {
                    // No way round the bodies of the overlay: the unit stands, with
                    // its order kept; its own stuck rule asks again, up to the limit
                    // of the replan settings, and then gives the order up.
                    self.job = None;
                    self.stats.detours_failed += 1;
                    let route = &mut e.route[slot];
                    route.overlay.clear();
                    route.clear_plan();
                    route.state = RouteState::Stranded;
                    route.epoch = self.epoch;
                }
                Progress::Replan => {
                    self.job = None;
                    let ticket = self.new_ticket();
                    let route = &mut e.route[slot];
                    route.clear_plan();
                    route.state = RouteState::Wanted;
                    route.ticket = ticket;
                    // Looked at again on the next pass, and a route that cannot
                    // be made does not hold up the others.
                    self.scan = true;
                }
            }
        }
        if rem > 0 && self.fields.has_work() {
            self.fields.advance(&self.graphs, e, &mut rem);
        }
        self.stats.nodes_last_tick = self.budget - rem;
        self.stats.field_nodes_last_tick = self.fields.nodes_last_tick;
        self.stats.fields_live = self.fields.live();
        self.stats.fields_built = self.fields.built;
        self.stats.peak_nodes = self.stats.peak_nodes.max(self.stats.nodes_last_tick);
        self.stats.waiting = e
            .route
            .iter_active()
            .filter(|&(i, r)| e.alive[i] && r.state == RouteState::Wanted)
            .count() as u32;
    }

    /// The first slot with work waiting, looking from slot `start` (where the
    /// tick began) round the ring of slots, `looked` of them already this tick. The slot after the one
    /// it picks is where the next look starts, so that service goes on after the
    /// last request started instead of from the lowest id every tick.
    fn next_job(
        &mut self,
        e: &Entities,
        units: &UnitTable,
        start: usize,
        looked: &mut usize,
    ) -> Option<Job> {
        // Slots beyond the routes' vector have never asked for one.
        let n = e.route.len().min(e.len());
        while *looked < n {
            let i = (start + *looked) % n;
            *looked += 1;
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
            self.next_slot = ((i + 1) % n) as u32;
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
        // A member of a group with a flowfield follows that (WP4.3); one the
        // field cannot serve (it is gone, or the unit's cell cannot reach the
        // goal in it) is routed on its own from where it stands.
        if e.route[i].field_serial != 0 {
            match self.fields.follow(&self.graphs, e, i, self.epoch) {
                Some(steer) => return steer,
                None => e.route[i] = Route::IDLE,
            }
        }
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
                // A target that keeps moving is searched for at most once in a while.
                if e.crowd[i].retarget > 0 {
                    return Steer::Stranded;
                }
                e.crowd[i].retarget = self.replan.retarget_cooldown;
                self.stats.target_replans += 1;
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
            RouteState::Wanted | RouteState::Active
                if far(route.want, want) && e.crowd[i].retarget == 0 =>
            {
                // The destination moved: ask again, and keep following the
                // waypoints there are until the new route is ready. Then not again
                // for the cooldown: the unit follows the route it has meanwhile.
                e.crowd[i].retarget = self.replan.retarget_cooldown;
                self.stats.target_replans += 1;
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
                self.stats.map_replans += 1;
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
        if route.field_serial != 0 {
            // A member of a field: the same two steps. The first refusal makes
            // it go cell by cell, the second stands it still.
            if route.cautious {
                route.state = RouteState::Stranded;
            } else {
                route.cautious = true;
                route.ring_clear();
                route.final_leg = false;
            }
            return;
        }
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

    /// The unit is stuck in a crowd and the sidesteps did not help (WP4.4, WP4.7): asks
    /// again for what it follows, from where it stands, and says which bodies that
    /// stand still are round it (`overlay`, from [`Pathing::overlay_for`]; empty when
    /// the ones in its way are walking).
    ///
    /// * **With an overlay** the unit gets a new route of its own whatever it had: the
    ///   search treats the cells of the overlay as blocked, for this request only, so
    ///   the route goes round a body the graph does not know. A member of a flowfield
    ///   leaves its field for it (the field leads through the gap the body stands in),
    ///   and lets go of its arrival slot. The unit stands until the search has run, as
    ///   a unit with a new order does: the way it was following is the one that is
    ///   blocked.
    /// * **Without** a member of a flowfield stays on the field, forgets the leg it
    ///   chose (the ring, the straight run to the slot) and recovers from `Stranded`:
    ///   it chooses again from the cell it stands in, smoothing allowed. A unit with a
    ///   route of its own gets a new search from where it stands and goes on along the
    ///   waypoints it has until that is ready.
    pub fn request_replan(&mut self, e: &mut Entities, i: usize, class: u16, overlay: Overlay) {
        self.stats.stuck_replans += 1;
        if !overlay.is_empty() {
            self.stats.overlay_replans += 1;
            let want = crate::orders::destination_of(e, i)
                .zip(self.graph_index(class))
                .map(|(d, gi)| Pathing::cell_index(&self.graphs[gi], d));
            let route = &mut e.route[i];
            if route.state == RouteState::Idle {
                return;
            }
            if route.field_serial != 0 {
                let Some(want) = want else { return };
                e.crowd[i].claim = crate::crowd::Claim::default();
                *route = Route::IDLE;
                route.want = want;
                route.goal = want;
                route.frontier = want;
            }
            // The waypoints it has stay until the search begins; a route that
            // already leads to the nearest cell of an unreachable goal stays marked,
            // so that a search that finds no way is not a failure but the same
            // answer as before.
            let route = &mut e.route[i];
            route.overlay = overlay;
            route.state = RouteState::Wanted;
            self.next_ticket = self.next_ticket.wrapping_add(1);
            route.ticket = self.next_ticket;
            self.scan = true;
            return;
        }
        let route = &mut e.route[i];
        if route.field_serial != 0 {
            route.ring_clear();
            route.final_leg = false;
            route.cautious = false;
            if route.state == RouteState::Stranded {
                route.state = RouteState::Active;
            }
            return;
        }
        if route.state == RouteState::Idle {
            return;
        }
        // The unit keeps following the waypoints it has until the new route is
        // ready, as when its destination moves: standing still to wait for a search
        // would only make the crowd more stuck.
        self.next_ticket = self.next_ticket.wrapping_add(1);
        route.state = RouteState::Wanted;
        route.ticket = self.next_ticket;
        self.scan = true;
    }

    /// The overlay for a stuck unit of class `class` standing at `from` that is going to
    /// `to`: the cells the bodies in `blockers` (their middle and radius) fill, with the
    /// unit's own radius and a margin of clearance added, up to [`OVERLAY_CAP`] of them,
    /// the nearest to the unit first. The cell the unit stands in and the cell of its
    /// destination are never in it (the unit must be able to leave, and the route must
    /// have somewhere to end). Empty when the class has no graph.
    pub fn overlay_for(
        &self,
        class: u16,
        from: Vec2,
        to: Vec2,
        mover_radius: Fx,
        blockers: impl Iterator<Item = (Vec2, Fx)>,
    ) -> Overlay {
        let Some(gi) = self.graph_index(class) else {
            return Overlay::EMPTY;
        };
        let g = &self.graphs[gi];
        let (w, h) = (i64::from(g.grid.width()), i64::from(g.grid.height()));
        let own = Pathing::cell_index(g, from);
        let goal = Pathing::cell_index(g, to);
        let mut builder = OverlayBuilder::new();
        for (at, radius) in blockers {
            let reach = radius + mover_radius + replan::OVERLAY_MARGIN;
            let reach2 = reach.square();
            let (x0, x1) = ((at.x - reach).floor_int(), (at.x + reach).floor_int());
            let (y0, y1) = ((at.y - reach).floor_int(), (at.y + reach).floor_int());
            let home = Pathing::cell_index(g, at);
            for y in y0.max(0)..=y1.min(h - 1) {
                for x in x0.max(0)..=x1.min(w - 1) {
                    let cell = g.grid.index(x as u32, y as u32);
                    if cell == own || cell == goal || !g.grid.open(x, y) {
                        continue;
                    }
                    let middle = centre_xy(x as u32, y as u32);
                    if cell == home || (middle - at).length_squared() <= reach2 {
                        builder.offer((middle - from).length_squared(), cell);
                    }
                }
            }
        }
        builder.finish()
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

    // ---- group orders (WP4.3) ----------------------------------------------------

    /// One `Command::Order` to walk to `destination` was just given to the
    /// units in `started` (ascending slots, each once, each starting the order
    /// now). For every movement class that has at least the threshold of them,
    /// makes a flowfield and hands the units their places in it; the others stay
    /// with their single routes.
    ///
    /// The classes of the order take **rings** round the goal (WP4.4,
    /// `path/slots.rs`), the fastest first (ties to the lower class): a class's
    /// slots lie on a lattice whose spacing is what its biggest body needs, and
    /// begin where the area of the classes before it ends.
    pub fn group_order(
        &mut self,
        e: &mut Entities,
        units: &UnitTable,
        started: &[usize],
        destination: Vec2,
        formed: &[(usize, Facing)],
    ) {
        let threshold = self.fields.threshold as usize;
        let in_formation = |m: usize| formed.binary_search_by_key(&m, |&(slot, _)| slot).is_ok();
        // A class whose units all march in a formation gets the arrival slots of a field from two
        // units (below), whatever the other units of the order do: a unit that forms none (an
        // aircraft, a family of one, a vessel among tanks) is no reason for the tanks, the foot
        // soldiers and the trucks that did form to be sent to the one point (WP4.6b).
        if threshold == 0
            || self.graphs.is_empty()
            || (started.len() < threshold && formed.is_empty())
        {
            return;
        }
        // The disc the bodies of the group fill, for the bound on how far out a crowd
        // held up on its destination counts as arrived (`orders::go_to`): one
        // `spacing * spacing` of ground to each body. Every member knows it, whether or
        // not its class gets a field, and it outlives the fields.
        let mut area = Fx::ZERO;
        for &i in started {
            if e.alive[i] && e.def[i] != NO_DEF {
                area += slots::spacing_for(units.at(e.def[i] as usize).size).square();
            }
        }
        let disc = area.mul_div(Fx::from_int(113), Fx::from_int(355)).sqrt();
        for &i in started {
            e.crowd[i].disc = disc;
        }
        // The classes that get a field: their members, the widest body, whether
        // all are small, and the fastest speed.
        struct Class {
            gi: usize,
            members: Vec<usize>,
            widest: Fx,
            speed: Fx,
        }
        let mut classes: Vec<Class> = Vec::new();
        for gi in 0..self.graphs.len() {
            let g = &self.graphs[gi];
            let members: Vec<usize> = started
                .iter()
                .copied()
                .filter(|&i| {
                    e.alive[i]
                        && e.def[i] != NO_DEF
                        && units.at(e.def[i] as usize).movement_class == Some(g.class)
                })
                .collect();
            // The units of a class that is only formation members get the arrival slots of a
            // field whatever the threshold (two of a class are enough): without a field they
            // would all be sent to the one point and arrive in a scramble (WP4.6). The rule is per
            // class: only a class with a unit outside every formation still needs the threshold,
            // also in a crowd with a formation inside it.
            let formed_class = members.len() >= 2 && members.iter().all(|&m| in_formation(m));
            if members.len() < threshold && !formed_class {
                continue;
            }
            let (gx, gy) = field::cell_of(g, destination);
            // A goal the class cannot enter is every unit's own affair: each is
            // sent to the reachable cell nearest to it.
            if !g.grid.open(i64::from(gx), i64::from(gy)) {
                continue;
            }
            let stats = |i: usize| units.at(e.def[i] as usize);
            let widest = members
                .iter()
                .map(|&i| stats(i).size)
                .max()
                .unwrap_or(Fx::ONE);
            let speed = members
                .iter()
                .map(|&i| stats(i).speed_per_tick)
                .max()
                .unwrap_or(Fx::ZERO);
            classes.push(Class {
                gi,
                members,
                widest,
                speed,
            });
        }
        // The fastest takes the innermost ring: it arrives first.
        classes.sort_by_key(|c| (core::cmp::Reverse(c.speed), c.gi));
        let order = self.fields.new_order();
        let mut inner = Fx::ZERO;
        for (rank, class) in classes.iter().enumerate() {
            let g = &self.graphs[class.gi];
            let (gx, gy) = field::cell_of(g, destination);
            let goal = g.grid.index(gx, gy);
            let spacing = slots::spacing_for(class.widest);
            // A class whose units march in a formation has its slots turned across the
            // direction they come in (WP4.6); `formed` is ascending by slot.
            let turn = class
                .members
                .iter()
                .find_map(|m| {
                    formed
                        .binary_search_by_key(m, |&(slot, _)| slot)
                        .ok()
                        .map(|k| formed[k].1)
                })
                .unwrap_or(Facing::PLUS_X);
            let count = u32::try_from(class.members.len()).unwrap_or(u32::MAX);
            let layout = Layout {
                order,
                rank: u8::try_from(rank).unwrap_or(u8::MAX),
                spacing,
                inner,
                turn,
            };
            let serial = self
                .fields
                .create(class.gi, goal, layout, count, g.sectors.count());
            for (rank, &i) in class.members.iter().enumerate() {
                let route = &mut e.route[i];
                *route = Route::IDLE;
                route.state = RouteState::Active;
                route.complete = true;
                route.want = goal;
                route.goal = goal;
                route.frontier = goal;
                route.epoch = self.epoch;
                route.field_serial = serial;
                route.field_rank = u16::try_from(rank).unwrap_or(u16::MAX);
            }
            inner = slots::outer_radius(inner, count.min(field::MAX_SLOTS as u32), spacing);
        }
    }

    /// Whether the route of unit `i`, which is not a member of a flowfield, leads the wrong
    /// way for a unit that went off it to walk to a formation slot and now goes back to
    /// the order (WP4.6): its first waypoint lies behind the unit as seen from `goal`, or
    /// farther than [`STALE_WAYPOINT`] cells away. A route with no waypoint left, or none at
    /// all, is not stale.
    pub fn way_is_stale(&self, e: &Entities, i: usize, class: u16, goal: Vec2) -> bool {
        let Some(gi) = self.graph_index(class) else {
            return false;
        };
        let route = &e.route[i];
        let Some(cell) = route.ring_front() else {
            return false;
        };
        let at = Pathing::centre_of(&self.graphs[gi], cell);
        let (to_way, to_goal) = (at - e.pos[i], goal - e.pos[i]);
        to_way.dot(to_goal) <= Fx::ZERO || to_way.length_squared() > STALE_WAYPOINT.square()
    }

    /// Marks the fields of the groups the given units follow as used now (WP4.6): a
    /// formation that marches without its members asking their field for a direction
    /// (they steer for their slots) keeps the fields from expiring that the members go
    /// back to when it breaks up or arrives.
    pub fn keep_field(&mut self, serial: u32) {
        self.fields.touch(serial);
    }

    /// Where unit `i` stands when it has arrived, if it is a member of a group
    /// whose field has handed out its slots: its own arrival slot. Arrival is
    /// judged against this instead of the order's point.
    pub fn arrival_point(&self, e: &Entities, i: usize) -> Option<Vec2> {
        self.fields.arrival_point(e, i)
    }

    /// The slot unit `i` has taken for itself, while its field lives: see
    /// [`World::claimed_slot_of`](crate::World::claimed_slot_of).
    pub fn claimed_point(&self, e: &Entities, i: usize) -> Option<Vec2> {
        self.fields.claimed_point(e, i)
    }

    // ---- reading ----------------------------------------------------------------

    /// A view of the route of unit `i`.
    pub fn view(&self, e: &Entities, i: usize) -> RouteView {
        let route = &e.route[i];
        let status = match route.state {
            RouteState::Idle => RouteStatus::None,
            RouteState::Stranded => RouteStatus::Stranded,
            RouteState::Wanted | RouteState::Active => {
                let waits = if route.field_serial != 0 {
                    !self.fields.ready_for(&self.graphs, e, i)
                } else {
                    route.ring_len() == 0 && !route.complete
                };
                if waits {
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
            field: route.field_serial != 0,
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
        h.write_u32(self.next_slot);
        // The replan settings (WP4.7), when they are not the defaults.
        if self.replan != ReplanConfig::DEFAULT {
            h.write_u8(0xE7);
            h.write_u32(u32::from(self.replan.cooldown));
            h.write_u32(u32::from(self.replan.max));
            h.write_u32(u32::from(self.replan.retarget_cooldown));
        }
        match &self.job {
            Some(job) => {
                h.write_u8(1);
                job.write_hash(h);
                if job.is_searching() {
                    h.write_u32(self.work.local.expanded());
                }
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

    /// Whether the pool holds a flowfield (the checksum has a section then).
    pub fn any_fields(&self) -> bool {
        self.fields.any()
    }

    /// Writes the flowfield section of the checksum.
    pub fn write_field_hash(&self, h: &mut Fnv1a64) {
        self.fields.write_hash(h);
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
