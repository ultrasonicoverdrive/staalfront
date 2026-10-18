//! The corridor of a group: the sectors between where its units stand and the
//! goal, found on the abstract graph.
//!
//! One Dijkstra over the portal nodes of [`ClassGraph`], started at the goal
//! (the portals of the goal's sector, each at the cost of the walk from the
//! goal) and stopped as soon as it has reached a portal of every sector the
//! group stands in. The costs are symmetric, so a path from the goal to a
//! sector is the path from the sector to the goal read backwards. Walking each
//! reached sector's parents back to the goal marks the sectors a unit of the
//! group would cross (`FieldBuild` does the marking, a step at a time inside the
//! budget); that set, with a margin round it, is where the flowfield is
//! computed.
//!
//! It is the same graph and the same costs as the abstract search of a single
//! request (`abstract_search.rs`), without a heuristic and with many targets.
//! It is **resumable**: it keeps its heap between calls and expands at most the
//! nodes it is given. A target that cannot be reached (another part of the map)
//! costs the whole reachable graph before the search runs dry; the unit that
//! stands there is sent on its own, as one that cannot reach its goal always
//! was.

use crate::path::graph::{
    ClassGraph, NO_PORTAL, SLOTS, SLOTS_PER_SIDE, node, node_sector, node_slot,
};
use crate::path::grid::opposite;
use crate::path::heap::IndexedHeap;

/// The parent of a node reached straight from the goal.
pub(crate) const FROM_GOAL: u32 = u32::MAX;

/// No node reached yet in a sector.
pub(crate) const NO_NODE: u32 = u32::MAX;

/// How a run of the search ended.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Reach {
    /// Every target sector has a reached portal.
    All,
    /// The reachable part of the graph is used up; some targets were not
    /// reached.
    Some,
    /// The budget ran out; call again with more.
    Yield,
}

/// The state of one corridor search, reused for every field.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Corridor {
    heap: IndexedHeap,
    g: Vec<u64>,
    parent: Vec<u32>,
    /// `seen[n] == generation` when `g[n]` and `parent[n]` are from this search.
    seen: Vec<u32>,
    generation: u32,
    /// The sectors the group stands in (and that are not the goal's): marked by
    /// the caller before [`Corridor::begin`].
    pub targets: Vec<bool>,
    /// Per sector: the first portal node of it the search expanded.
    reached: Vec<u32>,
    remaining: u32,
}

impl Corridor {
    /// For graphs of `sectors` sectors.
    pub fn new(sectors: usize) -> Corridor {
        let nodes = sectors * SLOTS;
        Corridor {
            heap: IndexedHeap::new(nodes),
            g: vec![0; nodes],
            parent: vec![FROM_GOAL; nodes],
            seen: vec![0; nodes],
            generation: 0,
            targets: vec![false; sectors],
            reached: vec![NO_NODE; sectors],
            remaining: 0,
        }
    }

    /// Forgets the targets.
    pub fn clear_targets(&mut self) {
        self.targets.iter_mut().for_each(|t| *t = false);
    }

    /// Starts a search from the portals in `goal_links` (node, cost of the walk
    /// from the goal) toward the sectors marked in `targets`.
    pub fn begin(&mut self, goal_links: &[(u32, u32)]) {
        self.heap.clear();
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.seen.iter_mut().for_each(|s| *s = 0);
            self.generation = 1;
        }
        self.reached.iter_mut().for_each(|r| *r = NO_NODE);
        self.remaining = self.targets.iter().filter(|&&t| t).count() as u32;
        for &(n, cost) in goal_links {
            self.relax(n, u64::from(cost), FROM_GOAL);
        }
    }

    fn relax(&mut self, n: u32, cost: u64, from: u32) {
        let at = n as usize;
        if self.seen[at] == self.generation && cost >= self.g[at] {
            return;
        }
        self.seen[at] = self.generation;
        self.g[at] = cost;
        self.parent[at] = from;
        self.heap.push_or_lower(n, (cost, 0));
    }

    /// Expands nodes while `budget` lasts, one unit per node.
    pub fn run(&mut self, graph: &ClassGraph, budget: &mut u32) -> Reach {
        loop {
            if self.remaining == 0 {
                return Reach::All;
            }
            if self.heap.is_empty() {
                return Reach::Some;
            }
            if *budget == 0 {
                return Reach::Yield;
            }
            let Some(n) = self.heap.pop() else {
                return Reach::Some;
            };
            *budget -= 1;
            let g = self.g[n as usize];
            let (s, k) = (node_sector(n), node_slot(n));
            if self.targets[s as usize] && self.reached[s as usize] == NO_NODE {
                self.reached[s as usize] = n;
                self.remaining -= 1;
                if self.remaining == 0 {
                    return Reach::All;
                }
            }
            let sg = &graph.sector[s as usize];
            // Across the border to the facing portal.
            let side = k / SLOTS_PER_SIDE;
            if let Some(other) = graph.sectors.neighbor(s, side) {
                let facing = opposite(side) * SLOTS_PER_SIDE + k % SLOTS_PER_SIDE;
                if graph.sector[other as usize].portal[facing] != NO_PORTAL {
                    self.relax(node(other, facing), g + u64::from(sg.cross[k]), n);
                }
            }
            // Through the sector to its other portals.
            for &(k2, cost) in sg.edges(k) {
                self.relax(node(s, usize::from(k2)), g + u64::from(cost), n);
            }
        }
    }

    /// The portal node of `sector` the search reached first, if it did.
    pub fn reached(&self, sector: u32) -> Option<u32> {
        let n = self.reached[sector as usize];
        (n != NO_NODE).then_some(n)
    }

    /// The node `n` was reached from: the next one toward the goal, `None` for
    /// a node reached straight from the goal.
    pub fn toward_goal(&self, n: u32) -> Option<u32> {
        let p = self.parent[n as usize];
        (p != FROM_GOAL).then_some(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::grid::Grid;
    use crate::path::local::Local;

    fn open(w: u32, h: u32) -> ClassGraph {
        let mut g = Grid::new(w, h);
        for i in 0..w * h {
            g.set(i, 1000);
        }
        ClassGraph::build(0, g, &mut Local::new())
    }

    /// Links from a goal cell to the portals of its sector.
    fn goal_links(graph: &ClassGraph, local: &mut Local, cell: (u32, u32)) -> Vec<(u32, u32)> {
        use crate::path::local::Query;
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

    #[test]
    fn it_reaches_every_target_sector_and_the_parents_lead_back_to_the_goal() {
        let graph = open(80, 48);
        let mut local = Local::new();
        let mut c = Corridor::new(graph.sectors.count());
        let goal = (70, 40);
        let gs = graph.sectors.of_cell(goal.0, goal.1);
        let links = goal_links(&graph, &mut local, goal);
        for cell in [(2, 2), (40, 3), (5, 44)] {
            let s = graph.sectors.of_cell(cell.0, cell.1);
            c.targets[s as usize] = true;
        }
        c.begin(&links);
        let mut budget = u32::MAX;
        assert_eq!(c.run(&graph, &mut budget), Reach::All);
        for cell in [(2, 2), (40, 3), (5, 44)] {
            let s = graph.sectors.of_cell(cell.0, cell.1);
            let mut n = c.reached(s).expect("reached");
            assert_eq!(node_sector(n), s);
            // Back to the goal's sector, one border at a time.
            let mut steps = 0;
            while let Some(p) = c.toward_goal(n) {
                n = p;
                steps += 1;
                assert!(steps < 1000);
            }
            assert_eq!(node_sector(n), gs, "the chain ends in the goal's sector");
        }
    }

    #[test]
    fn a_small_budget_yields_and_resuming_finds_the_same() {
        let graph = open(96, 64);
        let mut local = Local::new();
        let goal = (90, 60);
        let links = goal_links(&graph, &mut local, goal);
        let targets: Vec<u32> = [(3, 3), (50, 10)]
            .iter()
            .map(|&(x, y)| graph.sectors.of_cell(x, y))
            .collect();
        let run = |slice: u32| {
            let mut c = Corridor::new(graph.sectors.count());
            for &t in &targets {
                c.targets[t as usize] = true;
            }
            c.begin(&links);
            let mut rounds = 0;
            loop {
                let mut budget = slice;
                match c.run(&graph, &mut budget) {
                    Reach::Yield => rounds += 1,
                    other => {
                        return (
                            other,
                            targets.iter().map(|&t| c.reached(t)).collect::<Vec<_>>(),
                            rounds,
                        );
                    }
                }
            }
        };
        let whole = run(u32::MAX);
        let sliced = run(7);
        assert_eq!(whole.0, Reach::All);
        assert_eq!(whole.1, sliced.1);
        assert!(sliced.2 > 3, "the slices were small enough to matter");
    }

    #[test]
    fn a_target_in_another_part_of_the_map_is_not_reached() {
        // A wall across the whole map.
        let (w, h) = (48, 32);
        let mut g = Grid::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let wall = x == 24;
                g.set(g.index(x, y), if wall { 0 } else { 1000 });
            }
        }
        let graph = ClassGraph::build(0, g, &mut Local::new());
        let mut local = Local::new();
        let links = goal_links(&graph, &mut local, (40, 10));
        let mut c = Corridor::new(graph.sectors.count());
        c.targets[graph.sectors.of_cell(3, 3) as usize] = true;
        c.targets[graph.sectors.of_cell(40, 30) as usize] = true;
        c.begin(&links);
        let mut budget = u32::MAX;
        assert_eq!(c.run(&graph, &mut budget), Reach::Some);
        assert!(c.reached(graph.sectors.of_cell(3, 3)).is_none());
        assert!(c.reached(graph.sectors.of_cell(40, 30)).is_some());
    }
}
