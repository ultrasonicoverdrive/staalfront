//! One request being served: the stages of planning a route.
//!
//! A request goes through these stages, each of which spends from the tick's
//! node budget and none of which allocates:
//!
//! * **Direct**: if the destination is within sight (a thick line of sight of
//!   at most a sector, [`crate::path::los`]) the route is that one waypoint.
//! * **Connect start**: a Dijkstra inside the start sector gives the cost from
//!   the unit to each portal of its sector (and, when the goal is in the same
//!   sector, an A* straight to it).
//! * **Connect goal**: the same from the goal, in the goal's sector.
//! * **Abstract**: A* over the portals ([`AbstractSearch`]), resumable.
//! * **Nearest**: only when the goal cannot be reached: the reachable cell
//!   nearest to it, found in the start sector and around the portal the
//!   abstract search got nearest with.
//! * **Refine** and **Smooth**: the next few legs of the abstract path are
//!   searched cell by cell, pulled taut by line of sight and pushed on the
//!   unit's ring. A leg is one sector, or two side by side when it crosses a
//!   border, so that the route crosses where it suits it and not at the portal.
//!   This is also the whole of a **refill** (the ring ran low), which skips the
//!   stages before it.
//!
//! Every stage can stop wherever the tick's budget runs out and go on next
//! tick: the abstract search and the local searches between two expansions
//! (`Local::advance`), the smoothing and the direct line of sight between two
//! lines of sight (a line of sight is atomic and costs at most 60 nodes). A
//! tick's budget is therefore spent to the last node whenever a request is
//! waiting, never exceeded, and a request is finished the same way whatever the
//! budget (only later).

use sf_math::{Fx, Vec2};

use crate::path::abstract_search::{AbstractSearch, BAN_CAP, Step};
use crate::path::graph::{
    ClassGraph, NO_PORTAL, SLOTS, SLOTS_PER_SIDE, node, node_sector, node_slot,
};
use crate::path::grid::Heuristic;
use crate::path::grid::INF;
use crate::path::local::{Local, Outcome, Query, Rect, union};
use crate::path::los;
use crate::path::route::{ABS_CAP, RING_CAP, Route, RouteState};

/// Legs (a sector, or two sectors side by side) refined per job.
pub(crate) const REFINE_LEGS: u8 = 3;

/// The farthest ahead in the raw cell route smoothing looks for a vertex it
/// can see.
const REACH_INDEX: usize = 24;

/// The scratch space of the planner, shared by every job (there is one at a
/// time).
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Work {
    pub local: Local,
    pub search: AbstractSearch,
    start_links: Vec<(u32, u32)>,
    goal_links: Vec<(u32, u32)>,
    /// A leg of the route, cell by cell.
    cells: Vec<u32>,
    /// A path through the portals.
    nodes: Vec<u32>,
    /// The cell route being smoothed: position and cell of each point.
    raw: Vec<(Vec2, u32)>,
}

impl Work {
    pub fn new(sectors: usize) -> Work {
        // A leg is at most the cells of two sectors, plus the anchor and a few.
        let leg = crate::path::local::WINDOW_CELLS + 8;
        Work {
            local: Local::new(),
            search: AbstractSearch::new(sectors),
            start_links: Vec::with_capacity(SLOTS),
            goal_links: Vec::with_capacity(SLOTS),
            cells: Vec::with_capacity(leg),
            nodes: Vec::with_capacity(sectors * SLOTS),
            raw: Vec::with_capacity(leg),
        }
    }
}

/// What kind of work a job does for its unit.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Mode {
    /// A new route from where the unit stands.
    Fresh,
    /// More legs of a route the unit has.
    Refill,
    /// A new plan from the end of the route the unit has, because the plan was
    /// longer than the abstract path can keep.
    Continue,
}

/// Where a job stands.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Phase {
    Begin,
    Direct,
    ConnectStart,
    ConnectGoal,
    Abstract,
    Nearest,
    Refine,
    Smooth,
    Finish,
}

impl Phase {
    const fn tag(self) -> u8 {
        match self {
            Phase::Begin => 0,
            Phase::Direct => 1,
            Phase::ConnectStart => 2,
            Phase::ConnectGoal => 3,
            Phase::Abstract => 4,
            Phase::Nearest => 5,
            Phase::Refine => 6,
            Phase::Smooth => 7,
            Phase::Finish => 8,
        }
    }
}

/// How a call of [`Job::run`] ended.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Progress {
    /// The job is finished; the route is as far as it goes for now.
    Done,
    /// The budget ran out; call again with more.
    Yield,
    /// The unit no longer wants this route (a newer request, a new order).
    Dropped,
    /// The graph changed under the job, or the route no longer fits it: the
    /// route is to be asked for again.
    Replan,
    /// A request that carried an overlay (WP4.7) found no way round the bodies in it:
    /// the destination cannot be reached with them there, or a sector the overlay cuts
    /// cannot be crossed any more although the graph says it can. The unit keeps no
    /// route and stands; asking again would give the same answer.
    Failed,
}

/// The job being served.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Job {
    pub slot: u32,
    pub ticket: u32,
    pub graph: usize,
    mode: Mode,
    /// The request carries an overlay and the destination could be reached without it
    /// (WP4.7): a search that finds no way is a failure of the request, not a
    /// destination that cannot be reached.
    strict: bool,
    /// The request carries an overlay (WP4.7): the searches see its cells as blocked.
    overlay_on: bool,
    /// The sectors the abstract search may not cross (WP4.7): those the overlay cuts,
    /// whose edges in the graph were made without it. The start and goal sectors from
    /// the beginning (their links are exact), another when a leg in it could not be
    /// refined.
    banned: [u32; BAN_CAP],
    banned_len: u8,
    epoch: u32,
    phase: Phase,
    /// Where the unit stood when the job began.
    pos: Vec2,
    start_cell: u32,
    abs_started: bool,
    /// Legs refined so far.
    legs: u8,
    /// `Work::cells` already holds the next leg.
    leg_ready: bool,
    /// Portals of the abstract path to take off once the leg being smoothed has
    /// been put on the ring whole.
    pop_after: u8,
    /// Smoothing: the point the next vertex is searched from, the nearest and
    /// the farthest point it can still be, and whether the farthest has been
    /// tried yet.
    sm_from: usize,
    sm_lo: usize,
    sm_hi: usize,
    sm_active: bool,
    sm_far_tried: bool,
    /// A local search was begun and is not over (it goes on next call).
    searching: bool,
    /// The nearest-cell stage: 0 while the search around the portal is to be
    /// done (or there is none), 1 once it is, with its candidate in `near_node`.
    near_stage: u8,
    near_node: Option<((u64, u64, u32), u32)>,
}

/// What the next leg of a route is.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Leg {
    /// The frontier is the goal of the whole route.
    Arrived,
    /// The abstract path is used up and the plan was cut short.
    PlanUsedUp,
    /// The frontier is on the next portal already: take it off the path.
    Pop,
    /// The frontier is next to the next portal, across a border: one step.
    Step { target: u32 },
    /// Search inside `rect` from the frontier to `target`. The cells the leg
    /// puts on the ring end at the first one inside sector `stop_in`, if there
    /// is one (the leg crosses a border), otherwise at `target`. `pop` portals
    /// of the abstract path are passed when the leg is whole.
    Search {
        rect: Rect,
        target: u32,
        pop: u8,
        stop_in: Option<u32>,
    },
}

fn centre(x: u32, y: u32) -> Vec2 {
    Vec2::new(
        Fx::from_i64(i64::from(x)) + Fx::HALF,
        Fx::from_i64(i64::from(y)) + Fx::HALF,
    )
}

impl Job {
    pub fn new(
        slot: u32,
        ticket: u32,
        graph: usize,
        mode: Mode,
        epoch: u32,
        pos: Vec2,
        route: &Route,
    ) -> Job {
        Job {
            slot,
            ticket,
            graph,
            mode,
            strict: mode == Mode::Fresh && !route.overlay.is_empty() && !route.unreachable,
            overlay_on: mode == Mode::Fresh && !route.overlay.is_empty(),
            banned: [0; BAN_CAP],
            banned_len: 0,
            epoch,
            phase: Phase::Begin,
            pos,
            start_cell: route.frontier,
            abs_started: false,
            legs: 0,
            leg_ready: false,
            pop_after: 0,
            sm_from: 0,
            sm_lo: 0,
            sm_hi: 0,
            sm_active: false,
            sm_far_tried: false,
            searching: false,
            near_stage: 0,
            near_node: None,
        }
    }

    /// Whether a local search of this job is begun and not over.
    pub fn is_searching(&self) -> bool {
        self.searching
    }

    /// One slice of a local search: begins it if it was not begun, expands as
    /// many cells as the budget allows and charges them. `None` when the budget
    /// ran out first; call again with the same query next tick.
    fn local_search(
        &mut self,
        local: &mut Local,
        g: &ClassGraph,
        heur: &Heuristic,
        q: &Query<'_>,
        rem: &mut u32,
    ) -> Option<Outcome> {
        if !self.searching {
            local.begin(&g.grid, heur, q);
            self.searching = true;
        }
        let before = local.expanded();
        let done = local.advance(&g.grid, heur, q, *rem);
        *rem -= local.expanded() - before;
        if done.is_some() {
            self.searching = false;
        }
        done
    }

    /// What the checksum sees of a job in flight.
    pub fn write_hash(&self, h: &mut sf_math::hash::Fnv1a64) {
        h.write_u32(self.slot);
        h.write_u32(self.ticket);
        h.write_u8(match self.mode {
            Mode::Fresh => 0,
            Mode::Refill => 1,
            Mode::Continue => 2,
        });
        h.write_u8(self.phase.tag());
        if self.overlay_on {
            h.write_u8(0x5E | u8::from(self.strict) << 7);
            h.write_u8(self.banned_len);
            for &b in &self.banned[..usize::from(self.banned_len)] {
                h.write_u32(b);
            }
        }
        h.write_u8(self.legs);
        h.write_u32(self.start_cell);
        h.write_u8(
            u8::from(self.abs_started) | u8::from(self.leg_ready) << 1 | self.pop_after << 2,
        );
        h.write_u32(self.sm_from as u32);
        h.write_u32(self.sm_lo as u32);
        h.write_u32(self.sm_hi as u32);
        h.write_u8(u8::from(self.sm_active) | u8::from(self.sm_far_tried) << 1);
        h.write_u8(u8::from(self.searching) | self.near_stage << 1);
        match self.near_node {
            Some(((a, b, c), cell)) => {
                h.write_u8(1);
                h.write_u64(a);
                h.write_u64(b);
                h.write_u32(c);
                h.write_u32(cell);
            }
            None => h.write_u8(0),
        }
    }

    /// The cell of a position, kept on the map.
    fn cell_of(g: &ClassGraph, pos: Vec2) -> u32 {
        let (x, y) = pos.cell();
        let x = (x.max(0) as u32).min(g.grid.width() - 1);
        let y = (y.max(0) as u32).min(g.grid.height() - 1);
        g.grid.index(x, y)
    }

    fn portal(g: &ClassGraph, n: u32) -> u32 {
        g.sector[node_sector(n) as usize].portal[node_slot(n)]
    }

    fn sector_of(g: &ClassGraph, cell: u32) -> u32 {
        let (x, y) = g.grid.xy(cell);
        g.sectors.of_cell(x, y)
    }

    /// Puts the nodes of `nodes` in the route's abstract path, as many as fit.
    fn install(route: &mut Route, nodes: &[u32]) {
        route.abs_clear();
        route.truncated = false;
        for &n in nodes {
            if !route.abs_push(n) {
                route.truncated = true;
                break;
            }
        }
        debug_assert!(nodes.len() <= ABS_CAP || route.truncated);
    }

    /// Whether a cell of the request's overlay lies in `sector`.
    fn dirty(g: &ClassGraph, route: &Route, sector: u32) -> bool {
        route
            .overlay
            .cells()
            .iter()
            .any(|&c| Job::sector_of(g, c) == sector)
    }

    /// Adds `sector` to the sectors the abstract search may not cross. False if it
    /// was there already or there is no room.
    fn ban(&mut self, sector: u32) -> bool {
        let n = usize::from(self.banned_len);
        if self.banned[..n].contains(&sector) || n == BAN_CAP {
            return false;
        }
        self.banned[n] = sector;
        self.banned_len += 1;
        true
    }

    /// A leg inside `rect` could not be refined and the request has an overlay: the
    /// graph's edges through the sectors of `rect` that the overlay cuts are not to be
    /// trusted. Bans them and plans the abstract path again (`None`: carry on); with no
    /// sector to ban the request fails, there is no way round.
    fn ban_and_retry(
        &mut self,
        g: &ClassGraph,
        route: &mut Route,
        rect: (u32, u32, u32, u32),
    ) -> Option<Progress> {
        let (a, b) = (
            g.sectors.of_cell(rect.0, rect.1),
            g.sectors.of_cell(rect.2 - 1, rect.3 - 1),
        );
        let ((ax, ay), (bx, by)) = (g.sectors.coords(a), g.sectors.coords(b));
        let mut added = false;
        for sy in ay..=by {
            for sx in ax..=bx {
                let sector = sy * g.sectors.across + sx;
                if Job::dirty(g, route, sector) {
                    added |= self.ban(sector);
                }
            }
        }
        if !added {
            return Some(Progress::Failed);
        }
        route.clear_plan();
        route.frontier = self.start_cell;
        self.legs = 0;
        self.leg_ready = false;
        self.searching = false;
        self.pop_after = 0;
        self.sm_active = false;
        self.abs_started = false;
        self.phase = Phase::Abstract;
        None
    }

    /// Whether this is the job of the unit in `slot` for the request `ticket`.
    pub fn is_for(&self, slot: u32, ticket: u32) -> bool {
        self.slot == slot && self.ticket == ticket
    }

    /// The epoch of the graph the job began in.
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// Works on the job while the budget lasts. `epoch` is the graph's epoch
    /// now; a job begun in another one is over.
    pub fn run(
        &mut self,
        g: &ClassGraph,
        w: &mut Work,
        route: &mut Route,
        rem: &mut u32,
        epoch: u32,
    ) -> Progress {
        if route.ticket != self.ticket
            || !matches!(route.state, RouteState::Wanted | RouteState::Active)
        {
            return Progress::Dropped;
        }
        if self.epoch != epoch {
            return Progress::Replan;
        }
        match self.run_phases(g, w, route, rem, epoch) {
            // With an overlay the graph and the grid disagree on purpose: a leg that
            // cannot be refined is the answer to the request, and the request would
            // only be made again with the same result.
            Progress::Replan if self.overlay_on => Progress::Failed,
            p => p,
        }
    }

    fn run_phases(
        &mut self,
        g: &ClassGraph,
        w: &mut Work,
        route: &mut Route,
        rem: &mut u32,
        epoch: u32,
    ) -> Progress {
        loop {
            match self.phase {
                Phase::Begin => self.begin(g, route),
                Phase::Direct => {
                    if let Some(p) = self.direct(g, route, rem) {
                        return p;
                    }
                }
                Phase::ConnectStart => {
                    if let Some(p) = self.connect_start(g, w, route, rem) {
                        return p;
                    }
                }
                Phase::ConnectGoal => {
                    if let Some(p) = self.connect_goal(g, w, route, rem) {
                        return p;
                    }
                }
                Phase::Abstract => {
                    if let Some(p) = self.abstract_stage(g, w, route, rem) {
                        return p;
                    }
                }
                Phase::Nearest => {
                    if let Some(p) = self.nearest(g, w, route, rem) {
                        return p;
                    }
                }
                Phase::Refine => {
                    if let Some(p) = self.refine(g, w, route, rem) {
                        return p;
                    }
                }
                Phase::Smooth => {
                    if let Some(p) = self.smooth(g, w, route, rem) {
                        return p;
                    }
                }
                Phase::Finish => {
                    route.epoch = epoch;
                    if self.mode == Mode::Fresh {
                        route.overlay.clear();
                    }
                    route.state = if route.complete && route.unreachable && route.ring_len() == 0 {
                        RouteState::Stranded
                    } else {
                        RouteState::Active
                    };
                    return Progress::Done;
                }
            }
        }
    }

    fn begin(&mut self, g: &ClassGraph, route: &mut Route) {
        match self.mode {
            Mode::Refill => {
                self.legs = 0;
                self.phase = Phase::Refine;
                return;
            }
            Mode::Fresh => {
                route.clear_plan();
                route.ring_clear();
                route.state = RouteState::Wanted;
                self.start_cell = Job::cell_of(g, self.pos);
                let (x, y) = g.grid.xy(self.start_cell);
                if !g.grid.open(i64::from(x), i64::from(y)) {
                    // Standing in a cell the class cannot enter (a structure
                    // was placed on it): walk out cell by cell.
                    route.cautious = true;
                }
            }
            Mode::Continue => {
                route.abs_clear();
                route.truncated = false;
                route.complete = false;
                route.unreachable = false;
                self.start_cell = route.frontier;
                self.pos = {
                    let (x, y) = g.grid.xy(route.frontier);
                    centre(x, y)
                };
            }
        }
        route.frontier = self.start_cell;
        let goal = route.want;
        let (gx, gy) = g.grid.xy(goal);
        if self.start_cell == goal && g.grid.open(i64::from(gx), i64::from(gy)) {
            route.goal = goal;
            route.complete = true;
            self.phase = Phase::Finish;
        } else {
            self.phase = Phase::Direct;
        }
    }

    fn direct(&mut self, g: &ClassGraph, route: &mut Route, rem: &mut u32) -> Option<Progress> {
        self.phase = Phase::ConnectStart;
        let goal = route.want;
        let (gx, gy) = g.grid.xy(goal);
        if route.cautious || !g.grid.open(i64::from(gx), i64::from(gy)) {
            return None;
        }
        let (p, q) = (self.pos, centre(gx, gy));
        if !los::short_enough(p, q) {
            return None;
        }
        if *rem < los::bound(p, q) {
            self.phase = Phase::Direct;
            return Some(Progress::Yield);
        }
        let (clear, used) = los::thick_clear(&g.grid, p, q);
        *rem -= used;
        if clear && route.ring_push(goal) {
            route.goal = goal;
            route.frontier = goal;
            route.complete = true;
            self.phase = Phase::Finish;
        }
        None
    }

    fn connect_start(
        &mut self,
        g: &ClassGraph,
        w: &mut Work,
        route: &mut Route,
        rem: &mut u32,
    ) -> Option<Progress> {
        let (sx, sy) = g.grid.xy(self.start_cell);
        let s = g.sectors.of_cell(sx, sy);
        let rect = g.sectors.rect(s);
        let goal = route.want;
        let (gx, gy) = g.grid.xy(goal);
        let goal_open = g.grid.open(i64::from(gx), i64::from(gy));
        let same = Job::sector_of(g, goal) == s && goal_open;
        // The cost to each portal of the sector; or, when the goal is in the
        // sector too, the way to it.
        let mut wanted = [0_u16; SLOTS];
        let mut n = 0;
        for k in 0..SLOTS {
            let p = g.sector[s as usize].portal[k];
            if p != NO_PORTAL && !same {
                let (x, y) = g.grid.xy(p);
                wanted[n] = Local::number(rect, x, y);
                n += 1;
            }
        }
        let query = Query {
            rect,
            src: (sx, sy),
            goal: same.then_some((gx, gy)),
            toward: None,
            wanted: &wanted[..n],
        };
        let Some(out) = self.local_search(&mut w.local, g, &g.heuristic_in(rect), &query, rem)
        else {
            return Some(Progress::Yield);
        };
        if same && out.reached {
            w.cells.clear();
            w.local.path(&g.grid, rect, (gx, gy), &mut w.cells);
            route.abs_clear();
            route.truncated = false;
            route.goal = goal;
            route.unreachable = false;
            self.leg_ready = true;
            self.legs = 0;
            self.phase = Phase::Refine;
            return None;
        }
        w.start_links.clear();
        for k in 0..SLOTS {
            let p = g.sector[s as usize].portal[k];
            if p == NO_PORTAL {
                continue;
            }
            let (x, y) = g.grid.xy(p);
            let d = w.local.dist[usize::from(Local::number(rect, x, y))];
            if d != INF {
                w.start_links.push((node(s, k), d));
            }
        }
        self.phase = Phase::ConnectGoal;
        None
    }

    fn connect_goal(
        &mut self,
        g: &ClassGraph,
        w: &mut Work,
        route: &mut Route,
        rem: &mut u32,
    ) -> Option<Progress> {
        let goal = route.want;
        let (gx, gy) = g.grid.xy(goal);
        let gs = g.sectors.of_cell(gx, gy);
        w.goal_links.clear();
        if g.grid.open(i64::from(gx), i64::from(gy)) {
            let rect = g.sectors.rect(gs);
            let mut wanted = [0_u16; SLOTS];
            let mut n = 0;
            for k in 0..SLOTS {
                let p = g.sector[gs as usize].portal[k];
                if p != NO_PORTAL {
                    let (x, y) = g.grid.xy(p);
                    wanted[n] = Local::number(rect, x, y);
                    n += 1;
                }
            }
            let query = Query {
                rect,
                src: (gx, gy),
                goal: None,
                toward: None,
                wanted: &wanted[..n],
            };
            if self
                .local_search(&mut w.local, g, &g.heur, &query, rem)
                .is_none()
            {
                return Some(Progress::Yield);
            }
            for k in 0..SLOTS {
                let p = g.sector[gs as usize].portal[k];
                if p == NO_PORTAL {
                    continue;
                }
                let (x, y) = g.grid.xy(p);
                let d = w.local.dist[usize::from(Local::number(rect, x, y))];
                if d != INF {
                    w.goal_links.push((node(gs, k), d));
                }
            }
        }
        self.abs_started = false;
        self.phase = Phase::Abstract;
        None
    }

    fn abstract_stage(
        &mut self,
        g: &ClassGraph,
        w: &mut Work,
        route: &mut Route,
        rem: &mut u32,
    ) -> Option<Progress> {
        let goal = route.want;
        let goal_xy = g.grid.xy(goal);
        if !self.abs_started {
            w.search.begin(g, &w.start_links, goal_xy, &w.goal_links);
            if self.overlay_on {
                // The links of the start and goal sectors were made with the overlay,
                // so these two are exact; the way through them by the graph's edges
                // is not, if the overlay cuts them.
                for sector in [Job::sector_of(g, self.start_cell), Job::sector_of(g, goal)] {
                    if Job::dirty(g, route, sector) {
                        self.ban(sector);
                    }
                }
                for k in 0..usize::from(self.banned_len) {
                    w.search.ban(self.banned[k]);
                }
            }
            self.abs_started = true;
        }
        match w.search.run(g, rem) {
            Step::Yield => Some(Progress::Yield),
            Step::Found => {
                w.nodes.clear();
                w.search.path_to_goal(&mut w.nodes);
                Job::install(route, &w.nodes);
                route.goal = goal;
                route.unreachable = false;
                self.legs = 0;
                self.phase = Phase::Refine;
                None
            }
            Step::Exhausted => {
                if self.strict {
                    // The destination could be reached before the overlay; with it
                    // there is no way: the request fails (WP4.7).
                    return Some(Progress::Failed);
                }
                self.phase = Phase::Nearest;
                None
            }
        }
    }

    /// The goal cannot be reached: choose the reachable cell nearest to it.
    fn nearest(
        &mut self,
        g: &ClassGraph,
        w: &mut Work,
        route: &mut Route,
        rem: &mut u32,
    ) -> Option<Progress> {
        let goal_xy = g.grid.xy(route.want);
        let best_node = w.search.nearest();
        let (sx, sy) = g.grid.xy(self.start_cell);
        let start_sector = g.sectors.of_cell(sx, sy);
        // First the search round the portal the abstract search got nearest
        // with, then the one in the start sector; either may take several
        // ticks, and the first one's candidate is kept in the job meanwhile.
        if self.near_stage == 0 {
            if let Some((_, cost, n)) = best_node {
                let s = node_sector(n);
                let (x, y) = g.grid.xy(Job::portal(g, n));
                let query = Query {
                    rect: g.sectors.rect(s),
                    src: (x, y),
                    goal: None,
                    toward: Some(goal_xy),
                    wanted: &[],
                };
                let Some(out) = self.local_search(&mut w.local, g, &g.heur, &query, rem) else {
                    return Some(Progress::Yield);
                };
                if let Some((cx, cy)) = out.nearest {
                    let cell = g.grid.index(cx, cy);
                    let total = cost + u64::from(out.nearest_cost);
                    self.near_node = Some(((g.heur.octile((cx, cy), goal_xy), total, cell), cell));
                }
            }
            self.near_stage = 1;
        }
        // The nearest cell of the start sector itself.
        let query = Query {
            rect: g.sectors.rect(start_sector),
            src: (sx, sy),
            goal: None,
            toward: Some(goal_xy),
            wanted: &[],
        };
        let Some(out) = self.local_search(&mut w.local, g, &g.heur, &query, rem) else {
            return Some(Progress::Yield);
        };
        self.near_stage = 0;
        let by_node = self.near_node.take();
        let by_start = out.nearest.map(|(cx, cy)| {
            let cell = g.grid.index(cx, cy);
            (
                (
                    g.heur.octile((cx, cy), goal_xy),
                    u64::from(out.nearest_cost),
                    cell,
                ),
                cell,
            )
        });
        // The nearer of the two by the heuristic; on a tie the one cheaper to
        // reach, then the lower cell number.
        let use_node = match (by_node, by_start) {
            (Some(a), Some(b)) => a.0 < b.0,
            (Some(_), None) => true,
            _ => false,
        };
        if use_node {
            let cell = by_node.map_or(self.start_cell, |c| c.1);
            w.nodes.clear();
            if let Some((_, _, n)) = best_node {
                w.search.path_to(n, &mut w.nodes);
            }
            Job::install(route, &w.nodes);
            route.goal = cell;
        } else {
            route.abs_clear();
            route.truncated = false;
            route.goal = by_start.map_or(self.start_cell, |c| c.1);
        }
        route.unreachable = true;
        self.legs = 0;
        self.phase = Phase::Refine;
        None
    }

    /// Decides the next leg. `Err` when the route and the graph disagree.
    fn next_leg(g: &ClassGraph, route: &Route) -> Result<Leg, ()> {
        let frontier = route.frontier;
        let here = Job::sector_of(g, frontier);
        let Some(n) = route.abs_front() else {
            if route.truncated {
                return Ok(Leg::PlanUsedUp);
            }
            let goal = route.goal;
            if frontier == goal {
                return Ok(Leg::Arrived);
            }
            if Job::sector_of(g, goal) != here {
                return Err(());
            }
            return Ok(Leg::Search {
                rect: g.sectors.rect(here),
                target: goal,
                pop: 0,
                stop_in: None,
            });
        };
        let exit = Job::portal(g, n);
        if exit == NO_PORTAL {
            return Err(());
        }
        if Job::sector_of(g, exit) != here {
            // The front of the path is a portal on the other side of a border,
            // and the frontier must be the cell next to it.
            let (fx, fy) = g.grid.xy(frontier);
            let (ex, ey) = g.grid.xy(exit);
            return if fx.abs_diff(ex) + fy.abs_diff(ey) == 1 {
                Ok(Leg::Step { target: exit })
            } else {
                Err(())
            };
        }
        // Across the border: search both sectors at once, so that the route
        // crosses where it suits it and not at the portal.
        if let Some(m) = route.abs_get(1)
            && g.sectors.neighbor(here, node_slot(n) / SLOTS_PER_SIDE) == Some(node_sector(m))
            && Job::portal(g, m) != NO_PORTAL
        {
            // Where the search is to end: the portal after, or the goal; at the
            // end of a plan that was cut short, the portal across the border.
            let onward = match route.abs_get(2) {
                Some(k) => Job::portal(g, k),
                None if !route.truncated => route.goal,
                None => Job::portal(g, m),
            };
            let next = node_sector(m);
            if onward != NO_PORTAL && Job::sector_of(g, onward) == next {
                return Ok(Leg::Search {
                    rect: union(g.sectors.rect(here), g.sectors.rect(next)),
                    target: onward,
                    pop: 2,
                    stop_in: Some(next),
                });
            }
        }
        if frontier == exit {
            return Ok(Leg::Pop);
        }
        Ok(Leg::Search {
            rect: g.sectors.rect(here),
            target: exit,
            pop: 1,
            stop_in: None,
        })
    }

    /// Searches the next leg cell by cell and either smooths it or, on a
    /// cautious route, puts its cells on the ring as they are.
    fn refine(
        &mut self,
        g: &ClassGraph,
        w: &mut Work,
        route: &mut Route,
        rem: &mut u32,
    ) -> Option<Progress> {
        loop {
            if self.legs >= REFINE_LEGS || route.ring_len() + 2 > RING_CAP {
                self.phase = Phase::Finish;
                return None;
            }
            let leg = match Job::next_leg(g, route) {
                Ok(leg) => leg,
                Err(()) => return Some(Progress::Replan),
            };
            let (rect, target, pop, stop_in) = match leg {
                Leg::Arrived => {
                    route.complete = true;
                    self.phase = Phase::Finish;
                    return None;
                }
                Leg::PlanUsedUp => {
                    self.phase = Phase::Finish;
                    return None;
                }
                Leg::Pop => {
                    route.abs_pop_front();
                    continue;
                }
                Leg::Step { target } => {
                    // No search: the two cells are next to each other.
                    if !self.leg_ready {
                        w.cells.clear();
                        w.cells.push(route.frontier);
                        w.cells.push(target);
                    }
                    self.leg_ready = false;
                    self.pop_after = 1;
                    if route.cautious {
                        self.push_cells(route, w);
                        if self.phase == Phase::Finish {
                            return None;
                        }
                        continue;
                    }
                    self.start_smoothing(g, w, route);
                    self.phase = Phase::Smooth;
                    return None;
                }
                Leg::Search {
                    rect,
                    target,
                    pop,
                    stop_in,
                } => (rect, target, pop, stop_in),
            };
            if !self.leg_ready {
                let (fx, fy) = g.grid.xy(route.frontier);
                let (tx, ty) = g.grid.xy(target);
                let query = Query {
                    rect,
                    src: (fx, fy),
                    goal: Some((tx, ty)),
                    toward: None,
                    wanted: &[],
                };
                let Some(out) =
                    self.local_search(&mut w.local, g, &g.heuristic_in(rect), &query, rem)
                else {
                    return Some(Progress::Yield);
                };
                if !out.reached {
                    // The graph said there is a way and the grid says no. With an
                    // overlay that is the overlay: another way is looked for.
                    if self.overlay_on {
                        return self.ban_and_retry(g, route, rect);
                    }
                    return Some(Progress::Replan);
                }
                w.cells.clear();
                w.local.path(&g.grid, rect, (tx, ty), &mut w.cells);
                if let Some(sector) = stop_in {
                    match w.cells.iter().position(|&c| Job::sector_of(g, c) == sector) {
                        Some(at) => w.cells.truncate(at + 1),
                        None => return Some(Progress::Replan),
                    }
                }
            }
            self.leg_ready = false;
            self.pop_after = pop;
            if route.cautious {
                self.push_cells(route, w);
                if self.phase == Phase::Finish {
                    return None;
                }
                continue;
            }
            self.start_smoothing(g, w, route);
            self.phase = Phase::Smooth;
            return None;
        }
    }

    /// Cautious route: the leg's cells go on the ring one by one (the first is
    /// the cell the route is in already).
    fn push_cells(&mut self, route: &mut Route, w: &Work) {
        for &cell in &w.cells[1..] {
            if !route.ring_push(cell) {
                route.frontier = route.ring_back().unwrap_or(route.frontier);
                self.pop_after = 0;
                self.phase = Phase::Finish;
                return;
            }
            route.frontier = cell;
        }
        self.finish_leg(route);
    }

    /// The leg is on the ring whole: the portals it passed are done with.
    fn finish_leg(&mut self, route: &mut Route) {
        for _ in 0..self.pop_after {
            route.abs_pop_front();
        }
        self.pop_after = 0;
        self.legs += 1;
    }

    /// Sets up the smoothing of the leg in `Work::cells`: the raw route starts
    /// at the point the ring (or the unit) is at.
    fn start_smoothing(&mut self, g: &ClassGraph, w: &mut Work, route: &mut Route) {
        // The last waypoint of the ring is the cell the leg starts in: take it
        // off, so that smoothing may cut across it.
        if route.ring_len() >= 2 {
            route.ring_pop_back();
        }
        w.raw.clear();
        let first = w.cells[0];
        match route.ring_back() {
            Some(c) => {
                let (x, y) = g.grid.xy(c);
                w.raw.push((centre(x, y), c));
            }
            None if self.mode != Mode::Refill && self.legs == 0 => {
                w.raw.push((self.pos, self.start_cell));
            }
            None => {
                let (x, y) = g.grid.xy(first);
                w.raw.push((centre(x, y), first));
            }
        }
        for &c in &w.cells {
            let (x, y) = g.grid.xy(c);
            let at = centre(x, y);
            if w.raw.last().is_some_and(|&(p, _)| p == at) {
                continue;
            }
            w.raw.push((at, c));
        }
        self.sm_from = 0;
        self.sm_active = false;
    }

    /// Pulls the leg taut: from each vertex, the farthest raw point it can see
    /// becomes the next vertex. The farthest candidate is tried first, then a
    /// binary search between the nearest and the farthest it can still be, so a
    /// vertex costs a handful of lines of sight and not one per point.
    fn smooth(
        &mut self,
        g: &ClassGraph,
        w: &mut Work,
        route: &mut Route,
        rem: &mut u32,
    ) -> Option<Progress> {
        loop {
            let last = w.raw.len() - 1;
            if self.sm_from >= last {
                route.frontier = w.raw[last].1;
                self.finish_leg(route);
                self.phase = Phase::Refine;
                return None;
            }
            if !self.sm_active {
                self.sm_lo = self.sm_from + 1;
                self.sm_hi = (self.sm_from + REACH_INDEX).min(last);
                self.sm_active = true;
                self.sm_far_tried = false;
            }
            if self.sm_lo < self.sm_hi {
                let test = if self.sm_far_tried {
                    (self.sm_lo + self.sm_hi).div_ceil(2)
                } else {
                    self.sm_hi
                };
                let (p, q) = (w.raw[self.sm_from].0, w.raw[test].0);
                let visible = if los::short_enough(p, q) {
                    if *rem < los::bound(p, q) {
                        return Some(Progress::Yield);
                    }
                    let (clear, used) = los::thick_clear(&g.grid, p, q);
                    *rem -= used;
                    clear
                } else {
                    false
                };
                self.sm_far_tried = true;
                if visible {
                    self.sm_lo = test;
                } else {
                    self.sm_hi = test - 1;
                }
                continue;
            }
            // The farthest point in sight is the next vertex.
            let cell = w.raw[self.sm_lo].1;
            if !route.ring_push(cell) {
                // The ring is full: the leg is cut at the last vertex.
                route.frontier = route.ring_back().unwrap_or(route.frontier);
                self.pop_after = 0;
                self.phase = Phase::Finish;
                return None;
            }
            self.sm_from = self.sm_lo;
            self.sm_active = false;
        }
    }
}

#[cfg(test)]
mod tests {
    // The planner is tested through `Pathing` (see `path/tests.rs`).
}
