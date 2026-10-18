//! A* over the portals: the first stage of a route.
//!
//! The graph is the portal nodes of [`ClassGraph`] with their crossing and
//! intra-sector edges, plus two virtual ends for one request: the **start**
//! (linked to the portals of the start sector, with the cost of the walk
//! there) and the **goal** (linked from the portals of the goal sector). The
//! search is **resumable**: it keeps its heap and costs between calls and does
//! at most the number of expansions it is given, so a request that needs more
//! than a tick's budget continues on the next tick.
//!
//! The order of expansion is `(f, h, node)`, lowest first (see
//! [`IndexedHeap`]): total, so the route does not depend on anything but the
//! graph. The heuristic is the octile distance with the class's cheapest cost
//! (`Heuristic`), never more than the real cost.
//!
//! When the goal cannot be reached the search runs out of nodes after
//! expanding the whole reachable part of the graph; it remembers the expanded
//! node nearest to the goal by the heuristic ([`AbstractSearch::nearest`]).

use crate::path::graph::{
    ClassGraph, NO_PORTAL, SLOTS, SLOTS_PER_SIDE, node, node_sector, node_slot,
};
use crate::path::grid::opposite;
use crate::path::heap::IndexedHeap;

/// The parent of a node reached from the start.
pub(crate) const FROM_START: u32 = u32::MAX;

/// How a run of the search ended.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Step {
    /// The goal was reached; read the path with [`AbstractSearch::path_to_goal`].
    Found,
    /// Every reachable node was expanded and the goal is not among them.
    Exhausted,
    /// The budget ran out; call again with more.
    Yield,
}

/// The state of one search, reused for every request.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct AbstractSearch {
    /// Elements `0..nodes` are portal nodes, `nodes` is the goal.
    heap: IndexedHeap,
    g: Vec<u64>,
    parent: Vec<u32>,
    /// `seen[n] == generation` when `g[n]` and `parent[n]` are from this search.
    seen: Vec<u32>,
    generation: u32,
    nodes: u32,
    goal_cell: (u32, u32),
    goal_sector: u32,
    goal_links: Vec<(u32, u32)>,
    /// The expanded node nearest the goal: `(heuristic, cost, node)`.
    nearest: Option<(u64, u64, u32)>,
}

impl AbstractSearch {
    /// For graphs of `sectors` sectors.
    pub fn new(sectors: usize) -> AbstractSearch {
        let nodes = sectors * SLOTS;
        AbstractSearch {
            heap: IndexedHeap::new(nodes + 1),
            g: vec![0; nodes + 1],
            parent: vec![FROM_START; nodes + 1],
            seen: vec![0; nodes + 1],
            generation: 0,
            nodes: nodes as u32,
            goal_cell: (0, 0),
            goal_sector: 0,
            goal_links: Vec::new(),
            nearest: None,
        }
    }

    /// Starts a search from the portals in `start_links` (node, cost of the
    /// walk there) to `goal_cell`, which the portals in `goal_links` (node,
    /// cost of the walk from there) lead to. An empty `goal_links` is a goal
    /// that cannot be reached.
    pub fn begin(
        &mut self,
        graph: &ClassGraph,
        start_links: &[(u32, u32)],
        goal_cell: (u32, u32),
        goal_links: &[(u32, u32)],
    ) {
        self.heap.clear();
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.seen.iter_mut().for_each(|s| *s = 0);
            self.generation = 1;
        }
        self.goal_cell = goal_cell;
        self.goal_sector = graph.sectors.of_cell(goal_cell.0, goal_cell.1);
        self.goal_links.clear();
        self.goal_links.extend_from_slice(goal_links);
        self.nearest = None;
        for &(n, cost) in start_links {
            self.relax(graph, n, u64::from(cost), FROM_START);
        }
    }

    /// The cell of a portal node.
    fn cell_of(graph: &ClassGraph, n: u32) -> (u32, u32) {
        let cell = graph.sector[node_sector(n) as usize].portal[node_slot(n)];
        graph.grid.xy(cell)
    }

    fn heuristic(&self, graph: &ClassGraph, n: u32) -> u64 {
        if n == self.nodes {
            0
        } else {
            graph.heur.octile(Self::cell_of(graph, n), self.goal_cell)
        }
    }

    fn relax(&mut self, graph: &ClassGraph, n: u32, cost: u64, from: u32) {
        let at = n as usize;
        if self.seen[at] == self.generation && cost >= self.g[at] {
            return;
        }
        self.seen[at] = self.generation;
        self.g[at] = cost;
        self.parent[at] = from;
        let h = self.heuristic(graph, n);
        self.heap.push_or_lower(n, (cost + h, h));
    }

    /// Expands nodes while `budget` lasts, one unit per node.
    pub fn run(&mut self, graph: &ClassGraph, budget: &mut u32) -> Step {
        loop {
            if self.heap.is_empty() {
                return Step::Exhausted;
            }
            if *budget == 0 {
                return Step::Yield;
            }
            let Some(n) = self.heap.pop() else {
                return Step::Exhausted;
            };
            *budget -= 1;
            if n == self.nodes {
                return Step::Found;
            }
            let g = self.g[n as usize];
            let (s, k) = (node_sector(n), node_slot(n));
            let h = self.heuristic(graph, n);
            if self.nearest.is_none_or(|best| (h, g, n) < best) {
                self.nearest = Some((h, g, n));
            }
            let sg = &graph.sector[s as usize];
            if s == self.goal_sector {
                for i in 0..self.goal_links.len() {
                    let (link, cost) = self.goal_links[i];
                    if link == n {
                        self.relax(graph, self.nodes, g + u64::from(cost), n);
                    }
                }
            }
            // Across the border to the facing portal.
            let side = k / SLOTS_PER_SIDE;
            if let Some(other) = graph.sectors.neighbor(s, side) {
                let facing = opposite(side) * SLOTS_PER_SIDE + k % SLOTS_PER_SIDE;
                if graph.sector[other as usize].portal[facing] != NO_PORTAL {
                    self.relax(graph, node(other, facing), g + u64::from(sg.cross[k]), n);
                }
            }
            // Through the sector to its other portals.
            for &(k2, cost) in sg.edges(k) {
                self.relax(graph, node(s, usize::from(k2)), g + u64::from(cost), n);
            }
        }
    }

    /// The expanded node nearest the goal by the heuristic (on a tie the
    /// cheaper to reach, then the lower node number), with that heuristic and
    /// its cost from the start: the ground to fall back on when the goal is
    /// unreachable.
    pub fn nearest(&self) -> Option<(u64, u64, u32)> {
        self.nearest
    }

    /// The portal nodes from the start to the goal, appended to `out`. Only
    /// after [`Step::Found`].
    pub fn path_to_goal(&self, out: &mut Vec<u32>) {
        self.path_through(self.parent[self.nodes as usize], out);
    }

    /// The portal nodes from the start to `last` (included), appended to
    /// `out`. `last` must have been reached by this search.
    pub fn path_to(&self, last: u32, out: &mut Vec<u32>) {
        self.path_through(last, out);
    }

    fn path_through(&self, last: u32, out: &mut Vec<u32>) {
        let from = out.len();
        let mut at = last;
        while at != FROM_START {
            out.push(at);
            at = self.parent[at as usize];
        }
        out[from..].reverse();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::grid::Grid;
    use crate::path::local::{Local, Query};

    fn open(w: u32, h: u32) -> Grid {
        let mut g = Grid::new(w, h);
        for i in 0..w * h {
            g.set(i, 1000);
        }
        g
    }

    /// The links from a cell to the portals of its sector, the way a request
    /// finds them.
    fn links(graph: &ClassGraph, local: &mut Local, cell: (u32, u32)) -> Vec<(u32, u32)> {
        let s = graph.sectors.of_cell(cell.0, cell.1);
        let rect = graph.sectors.rect(s);
        local.search(
            &graph.grid,
            &graph.heur,
            &Query {
                rect,
                src: cell,
                goal: None,
                toward: None,
                wanted: &[],
            },
        );
        (0..SLOTS)
            .filter_map(|k| {
                let p = graph.sector[s as usize].portal[k];
                if p == NO_PORTAL {
                    return None;
                }
                let (x, y) = graph.grid.xy(p);
                let d = local.dist[usize::from(Local::number(rect, x, y))];
                (d != u32::MAX).then_some((node(s, k), d))
            })
            .collect()
    }

    fn search(
        graph: &ClassGraph,
        local: &mut Local,
        from: (u32, u32),
        to: (u32, u32),
    ) -> (Step, Vec<u32>) {
        let mut a = AbstractSearch::new(graph.sectors.count());
        let (start, goal) = (links(graph, local, from), links(graph, local, to));
        a.begin(graph, &start, to, &goal);
        let mut budget = u32::MAX;
        let step = a.run(graph, &mut budget);
        let mut path = Vec::new();
        if step == Step::Found {
            a.path_to_goal(&mut path);
        }
        (step, path)
    }

    #[test]
    fn crosses_open_sectors_through_facing_portals() {
        let mut local = Local::new();
        let graph = ClassGraph::build(0, open(48, 16), &mut local);
        let (step, path) = search(&graph, &mut local, (2, 8), (45, 8));
        assert_eq!(step, Step::Found);
        // Out of sector 0, into 1, out of 1, into 2: four portal nodes.
        assert_eq!(path.len(), 4);
        let sectors: Vec<u32> = path.iter().map(|&n| node_sector(n)).collect();
        assert_eq!(sectors, [0, 1, 1, 2]);
    }

    #[test]
    fn a_goal_behind_a_wall_is_exhausted_and_the_nearest_node_is_remembered() {
        let mut g = open(48, 16);
        // The wall, all the way across (column 24, in sector 1).
        for y in 0..16 {
            let i = g.index(24, y);
            g.set(i, 0);
        }
        let mut local = Local::new();
        let graph = ClassGraph::build(0, g, &mut local);
        let mut a = AbstractSearch::new(graph.sectors.count());
        let start = links(&graph, &mut local, (2, 8));
        a.begin(&graph, &start, (45, 8), &[]);
        let mut budget = u32::MAX;
        assert_eq!(a.run(&graph, &mut budget), Step::Exhausted);
        let (_, _, nearest) = a.nearest().unwrap();
        // It got as far as the portals in sector 1 on the near side.
        assert_eq!(node_sector(nearest), 1);
    }

    #[test]
    fn a_small_budget_yields_and_resuming_finds_the_same_path() {
        let mut g = open(96, 96);
        // A few walls to make it search.
        for y in 10..80 {
            let i = g.index(48, y);
            g.set(i, 0);
        }
        let mut local = Local::new();
        let graph = ClassGraph::build(0, g, &mut local);
        let (_, whole) = search(&graph, &mut local, (3, 3), (90, 40));

        let mut a = AbstractSearch::new(graph.sectors.count());
        let (start, goal) = (
            links(&graph, &mut local, (3, 3)),
            links(&graph, &mut local, (90, 40)),
        );
        a.begin(&graph, &start, (90, 40), &goal);
        let mut calls = 0;
        let step = loop {
            let mut budget = 3;
            calls += 1;
            match a.run(&graph, &mut budget) {
                Step::Yield => {}
                other => break other,
            }
        };
        assert_eq!(step, Step::Found);
        assert!(calls > 3);
        let mut path = Vec::new();
        a.path_to_goal(&mut path);
        assert_eq!(path, whole);
    }

    #[test]
    fn abstract_connectivity_equals_grid_connectivity_on_random_maps() {
        let mut s: u64 = 0xDEAD_BEEF_CAFE_F00D;
        let mut next = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let mut local = Local::new();
        let mut reachable = 0;
        let mut unreachable = 0;
        for _ in 0..12 {
            let (w, h) = (50_u32, 38_u32);
            let mut g = Grid::new(w, h);
            for i in 0..w * h {
                g.set(i, if next() % 100 < 38 { 0 } else { 1000 });
            }
            let graph = ClassGraph::build(0, g.clone(), &mut local);
            // Flood fill from a cell, on the whole grid.
            let open_cells: Vec<(u32, u32)> = (0..w * h)
                .map(|i| g.xy(i))
                .filter(|&(x, y)| g.speed(x, y) != 0)
                .collect();
            if open_cells.len() < 10 {
                continue;
            }
            let from = open_cells[(next() as usize) % open_cells.len()];
            let mut seen = vec![false; (w * h) as usize];
            let mut stack = vec![from];
            seen[g.index(from.0, from.1) as usize] = true;
            while let Some((x, y)) = stack.pop() {
                for &(dx, dy) in &crate::path::grid::DIRS {
                    if g.can_step(x, y, dx, dy) {
                        let (nx, ny) = ((i64::from(x) + dx) as u32, (i64::from(y) + dy) as u32);
                        if !seen[g.index(nx, ny) as usize] {
                            seen[g.index(nx, ny) as usize] = true;
                            stack.push((nx, ny));
                        }
                    }
                }
            }
            for _ in 0..25 {
                let to = open_cells[(next() as usize) % open_cells.len()];
                let truth = seen[g.index(to.0, to.1) as usize];
                let same_sector =
                    graph.sectors.of_cell(from.0, from.1) == graph.sectors.of_cell(to.0, to.1);
                if same_sector {
                    continue; // a request in one sector is a local search
                }
                let (step, path) = search(&graph, &mut local, from, to);
                assert_eq!(
                    step == Step::Found,
                    truth || false,
                    "from {from:?} to {to:?}: abstract {step:?}, grid {truth}"
                );
                if truth {
                    reachable += 1;
                    assert!(!path.is_empty());
                } else {
                    unreachable += 1;
                }
            }
        }
        assert!(
            reachable > 20 && unreachable > 20,
            "{reachable} {unreachable}"
        );
    }
}
