//! The abstract graph of one movement class: portals on the sector borders
//! and the cost of crossing a sector between them.
//!
//! # Portals
//!
//! Along the border between two sectors, a **run** is a maximal stretch of
//! border positions where the cell on both sides can be entered. A run gets
//! **portals**: a pair of cells facing each other, one in each sector. A short
//! run (up to 5 positions) gets one, in the middle (the lower middle for an even
//! run); one of 6 to 11 gets three, at both ends and in the middle, and from 12
//! positions four, spread evenly. One portal per run is enough for connectivity: the cells of a
//! run are all open and next to each other, so they reach each other along the
//! border on either side, and a diagonal step across a border needs both its
//! orthogonal cells open, which is a run position of its own. The extra portals
//! on a long run are for quality: a route has to pass a portal, so the nearer
//! the portals are to the way the route would go, the less it is bent, and the
//! ends of a run are where an optimal route hugs the corner of an obstacle.
//! The smoothing of a route pulls it taut again after that.
//!
//! A sector has at most 8 runs per side (the sixteen positions alternate
//! open and blocked at worst), so 8 portal **slots** per side and 32 per
//! sector, numbered `side * 8 + k` with `k` the run counted along the border.
//! The two cells of a portal are two nodes of the graph, one in each sector,
//! in the same slot number on the opposite sides; the **node number** is
//! `sector * 32 + slot`. A side's slots are rewritten as a whole when its
//! border changes, so a changed border never moves the nodes of another one.
//!
//! # Edges
//!
//! * the **crossing** of a border: from a portal node to the one facing it, the
//!   cost of that one orthogonal step ([`SectorGraph::cross`]);
//! * the **intra-sector** edges: from a portal node to each other portal node of
//!   the same sector, the cost of the cheapest walk between them inside the
//!   sector (a Dijkstra from every portal, [`SectorGraph::adj`]); no edge when
//!   there is no walk inside the sector.
//!
//! A walk that leaves a sector and comes back is not an intra-sector edge; the
//! graph finds it through the portals.
//!
//! # Changing a sector
//!
//! [`ClassGraph::rebuild`] takes the sectors whose cells changed, rewrites the
//! four borders of each (both sides) and the edges of those sectors and of
//! their neighbours (whose portals on the shared border may have changed), and
//! nothing else.

use crate::path::grid::{EAST, Grid, Heuristic, NORTH, SECTOR_SIZE, SOUTH, Sectors, opposite};
use crate::path::grid::{FULL_SPEED, orth_cost};
use crate::path::local::{Local, Query};

/// Portal slots per side of a sector.
pub(crate) const SLOTS_PER_SIDE: usize = (SECTOR_SIZE / 2) as usize;

/// Portal slots per sector.
pub(crate) const SLOTS: usize = 4 * SLOTS_PER_SIDE;

/// The portal cell of an empty slot.
pub(crate) const NO_PORTAL: u32 = u32::MAX;

/// The portals and edges of one sector.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct SectorGraph {
    /// The map cell (row-major index) of each portal slot, or [`NO_PORTAL`].
    pub portal: [u32; SLOTS],
    /// The cost of the step across the border from each portal slot.
    pub cross: [u32; SLOTS],
    /// Where each slot's intra-sector edges start in [`SectorGraph::adj`]
    /// (`adj_start[slot]..adj_start[slot + 1]`).
    pub adj_start: [u16; SLOTS + 1],
    /// The intra-sector edges, slot by slot: `(other slot, cost)`, in
    /// ascending order of the other slot.
    pub adj: Vec<(u8, u32)>,
}

impl SectorGraph {
    fn empty() -> SectorGraph {
        SectorGraph {
            portal: [NO_PORTAL; SLOTS],
            cross: [0; SLOTS],
            adj_start: [0; SLOTS + 1],
            adj: Vec::new(),
        }
    }

    /// The intra-sector edges of `slot`.
    #[inline]
    pub fn edges(&self, slot: usize) -> &[(u8, u32)] {
        &self.adj[usize::from(self.adj_start[slot])..usize::from(self.adj_start[slot + 1])]
    }
}

/// The most portals one run gets.
const MOST_PER_RUN: usize = 4;

/// Where along a run of `len` crossable positions starting at `start` the
/// portals go: the middle of a short run (up to 5), both ends and the middle of
/// one of 6 to 11, and four spread evenly (both ends included) from 12 on.
/// Never more than one portal for every two positions, so a side's 8 slots
/// always suffice.
///
/// The count matters for the quality of the routes: the abstract search picks
/// the way round an obstacle from the cost of walking to the portals, and a
/// portal far from the place where the best route crosses a border makes the
/// wrong way look cheaper. Measured against the best any-angle route on 1,900
/// random routes, two portals for a run of 6 to 11 and three from 12 on gave a
/// worst case of 1150 per thousand of the best; these counts give 1075
/// (`docs/rules/pathfinding.md`).
fn portal_positions(start: u32, len: u32) -> ([u32; MOST_PER_RUN], usize) {
    let mut out = [0; MOST_PER_RUN];
    let count: u32 = match len {
        0..=5 => 1,
        6..=11 => 3,
        _ => 4,
    };
    if count == 1 {
        out[0] = start + (len - 1) / 2;
        return (out, 1);
    }
    for k in 0..count {
        out[k as usize] = start + k * (len - 1) / (count - 1);
    }
    (out, count as usize)
}

/// The node number of a portal slot of a sector.
#[inline]
pub(crate) const fn node(sector: u32, slot: usize) -> u32 {
    sector * SLOTS as u32 + slot as u32
}

/// The sector of a node.
#[inline]
pub(crate) const fn node_sector(node: u32) -> u32 {
    node / SLOTS as u32
}

/// The slot of a node.
#[inline]
pub(crate) const fn node_slot(node: u32) -> usize {
    (node % SLOTS as u32) as usize
}

/// Everything the search needs to know about one movement class.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct ClassGraph {
    /// The permanent kind number of the movement class.
    pub class: u16,
    /// What each cell allows this class.
    pub grid: Grid,
    /// The sector decomposition of the map.
    pub sectors: Sectors,
    /// The cheapest per-cell cost of the class on this map.
    pub heur: Heuristic,
    /// Per sector.
    pub sector: Vec<SectorGraph>,
    /// The highest speed multiplier of any cell of each sector (zero if none
    /// can be entered): a search inside one or two sectors takes its
    /// heuristic from these, which is exact on ground of one speed.
    best: Vec<u16>,
}

impl ClassGraph {
    /// Builds the graph of a grid.
    pub fn build(class: u16, grid: Grid, local: &mut Local) -> ClassGraph {
        let sectors = Sectors::new(grid.width(), grid.height());
        // At least normal speed, as in `heuristic_in`: a unit standing in a
        // blocked cell leaves it at normal speed, so no step is cheaper than
        // the average of that and the best cell.
        let heur = Heuristic::from_best_speed(grid.max_speed().max(FULL_SPEED));
        let mut graph = ClassGraph {
            class,
            grid,
            sectors,
            heur,
            sector: vec![SectorGraph::empty(); sectors.count()],
            best: vec![0; sectors.count()],
        };
        for s in 0..sectors.count() as u32 {
            graph.measure(s);
        }
        for s in 0..sectors.count() as u32 {
            for side in [EAST, SOUTH] {
                graph.build_border(s, side);
            }
        }
        for s in 0..sectors.count() as u32 {
            graph.build_edges(s, local);
        }
        graph
    }

    /// Rebuilds what depends on the cells of the `changed` sectors (whose
    /// grid cells the caller has already updated). Returns the sectors whose
    /// portals or edges were rewritten: the changed ones and their neighbours.
    pub fn rebuild(&mut self, changed: &[u32], local: &mut Local) -> Vec<u32> {
        let mut touched: Vec<u32> = Vec::new();
        for &s in changed {
            self.measure(s);
            touched.push(s);
            for side in 0..4 {
                self.build_border(s, side);
                if let Some(n) = self.sectors.neighbor(s, side) {
                    touched.push(n);
                }
            }
        }
        touched.sort_unstable();
        touched.dedup();
        for &s in &touched {
            self.build_edges(s, local);
        }
        touched
    }

    /// Reads the best speed of sector `s` from the grid.
    fn measure(&mut self, s: u32) {
        let (x0, y0, x1, y1) = self.sectors.rect(s);
        let mut best = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                best = best.max(self.grid.speed(x, y));
            }
        }
        self.best[s as usize] = best;
    }

    /// The heuristic for a search inside the rectangle `rect` of one sector or
    /// two next to each other: the cheapest per-cell cost of the cells in it
    /// (at least the cost of normal speed, which is what a unit in a blocked
    /// cell leaves it at).
    pub fn heuristic_in(&self, rect: (u32, u32, u32, u32)) -> Heuristic {
        let a = self.sectors.of_cell(rect.0, rect.1);
        let b = self.sectors.of_cell(rect.2 - 1, rect.3 - 1);
        let ((ax, ay), (bx, by)) = (self.sectors.coords(a), self.sectors.coords(b));
        let mut best = crate::path::grid::FULL_SPEED;
        for sy in ay..=by {
            for sx in ax..=bx {
                best = best.max(self.best[(sy * self.sectors.across + sx) as usize]);
            }
        }
        Heuristic::from_best_speed(best)
    }

    /// The cells along the border of sector `s` on `side`, as pairs `(inside,
    /// outside)` of map cells, in ascending order along the border.
    fn border_pair(&self, s: u32, side: usize, i: u32) -> ((u32, u32), (u32, u32)) {
        let (x0, y0, x1, y1) = self.sectors.rect(s);
        match side {
            NORTH => ((x0 + i, y0), (x0 + i, y0.wrapping_sub(1))),
            SOUTH => ((x0 + i, y1 - 1), (x0 + i, y1)),
            EAST => ((x1 - 1, y0 + i), (x1, y0 + i)),
            _ => ((x0, y0 + i), (x0.wrapping_sub(1), y0 + i)),
        }
    }

    /// Finds the runs of the border of sector `s` on `side` and rewrites the
    /// portal slots of both sides of it. Does nothing at the edge of the map.
    fn build_border(&mut self, s: u32, side: usize) {
        let Some(other) = self.sectors.neighbor(s, side) else {
            return;
        };
        let (x0, y0, x1, y1) = self.sectors.rect(s);
        let length = if side == NORTH || side == SOUTH {
            x1 - x0
        } else {
            y1 - y0
        };
        let back = opposite(side);
        for k in 0..SLOTS_PER_SIDE {
            let (a, b) = (side * SLOTS_PER_SIDE + k, back * SLOTS_PER_SIDE + k);
            self.sector[s as usize].portal[a] = NO_PORTAL;
            self.sector[s as usize].cross[a] = 0;
            self.sector[other as usize].portal[b] = NO_PORTAL;
            self.sector[other as usize].cross[b] = 0;
        }
        let mut slot = 0;
        let mut run_start: Option<u32> = None;
        for i in 0..=length {
            let crossable = i < length && {
                let (inside, outside) = self.border_pair(s, side, i);
                self.grid.speed(inside.0, inside.1) != 0
                    && self.grid.speed(outside.0, outside.1) != 0
            };
            match (crossable, run_start) {
                (true, None) => run_start = Some(i),
                (false, Some(start)) => {
                    run_start = None;
                    let len = i - start;
                    let (picks, count) = portal_positions(start, len);
                    for &at in &picks[..count] {
                        if slot >= SLOTS_PER_SIDE {
                            break;
                        }
                        let (inside, outside) = self.border_pair(s, side, at);
                        let cost = orth_cost(
                            self.grid.speed(inside.0, inside.1),
                            self.grid.speed(outside.0, outside.1),
                        );
                        let (a, b) = (side * SLOTS_PER_SIDE + slot, back * SLOTS_PER_SIDE + slot);
                        let (here, there) = (
                            self.grid.index(inside.0, inside.1),
                            self.grid.index(outside.0, outside.1),
                        );
                        self.sector[s as usize].portal[a] = here;
                        self.sector[s as usize].cross[a] = cost;
                        self.sector[other as usize].portal[b] = there;
                        self.sector[other as usize].cross[b] = cost;
                        slot += 1;
                    }
                }
                _ => {}
            }
        }
    }

    /// Rewrites the intra-sector edges of sector `s`.
    fn build_edges(&mut self, s: u32, local: &mut Local) {
        let rect = self.sectors.rect(s);
        let present: Vec<usize> = (0..SLOTS)
            .filter(|&k| self.sector[s as usize].portal[k] != NO_PORTAL)
            .collect();
        let numbers: Vec<u16> = present
            .iter()
            .map(|&k| {
                let (x, y) = self.grid.xy(self.sector[s as usize].portal[k]);
                Local::number(rect, x, y)
            })
            .collect();
        let mut edges: Vec<(u8, u8, u32)> = Vec::new();
        for (i, &k) in present.iter().enumerate() {
            let (x, y) = self.grid.xy(self.sector[s as usize].portal[k]);
            let rest: Vec<u16> = numbers[i + 1..].to_vec();
            if rest.is_empty() {
                continue;
            }
            local.search(
                &self.grid,
                &self.heur,
                &Query {
                    rect,
                    src: (x, y),
                    goal: None,
                    toward: None,
                    wanted: &rest,
                },
            );
            for (j, &k2) in present.iter().enumerate().skip(i + 1) {
                let cost = local.dist[usize::from(numbers[j])];
                if cost != crate::path::grid::INF {
                    edges.push((k as u8, k2 as u8, cost));
                }
            }
        }
        // Both directions, grouped by slot: counting sort into a list.
        let mut count = [0_u16; SLOTS + 1];
        for &(a, b, _) in &edges {
            count[usize::from(a) + 1] += 1;
            count[usize::from(b) + 1] += 1;
        }
        for k in 0..SLOTS {
            count[k + 1] += count[k];
        }
        let sg = &mut self.sector[s as usize];
        sg.adj_start = count;
        sg.adj = vec![(0, 0); usize::from(count[SLOTS])];
        let mut fill = count;
        // Edges were found in ascending (a, b) order; a slot's list gets its
        // lower partners first (from earlier edges) and its higher ones after,
        // so it is ascending by partner.
        for &(a, b, cost) in &edges {
            sg.adj[usize::from(fill[usize::from(a)])] = (b, cost);
            fill[usize::from(a)] += 1;
            sg.adj[usize::from(fill[usize::from(b)])] = (a, cost);
            fill[usize::from(b)] += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::grid::WEST;

    fn grid_from(rows: &[&str]) -> Grid {
        let (w, h) = (rows[0].len() as u32, rows.len() as u32);
        let mut g = Grid::new(w, h);
        for (y, row) in rows.iter().enumerate() {
            for (x, c) in row.bytes().enumerate() {
                if c == b'.' {
                    let i = g.index(x as u32, y as u32);
                    g.set(i, 1000);
                }
            }
        }
        g
    }

    fn open(w: u32, h: u32) -> Grid {
        let mut g = Grid::new(w, h);
        for i in 0..w * h {
            g.set(i, 1000);
        }
        g
    }

    fn portals(g: &ClassGraph, sector: u32) -> Vec<(usize, (u32, u32))> {
        (0..SLOTS)
            .filter(|&k| g.sector[sector as usize].portal[k] != NO_PORTAL)
            .map(|k| (k, g.grid.xy(g.sector[sector as usize].portal[k])))
            .collect()
    }

    #[test]
    fn open_ground_gets_four_portals_spread_along_every_full_border() {
        let mut local = Local::new();
        let g = ClassGraph::build(0, open(32, 32), &mut local);
        // Sector 0 is the top left: east and south borders only.
        let p = portals(&g, 0);
        assert_eq!(p.len(), 8);
        // East border at x = 15, rows 0..15: four, spread from end to end.
        let east = EAST * SLOTS_PER_SIDE;
        assert_eq!(p[0], (east, (15, 0)));
        assert_eq!(p[1], (east + 1, (15, 5)));
        assert_eq!(p[2], (east + 2, (15, 10)));
        assert_eq!(p[3], (east + 3, (15, 15)));
        // South border at y = 15: the same along the columns.
        let south = SOUTH * SLOTS_PER_SIDE;
        assert_eq!(p[4], (south, (0, 15)));
        assert_eq!(p[5], (south + 1, (5, 15)));
        assert_eq!(p[6], (south + 2, (10, 15)));
        assert_eq!(p[7], (south + 3, (15, 15)));
        // The other side of the east border is in sector 1, facing west.
        let q = portals(&g, 1);
        assert!(q.contains(&(WEST * SLOTS_PER_SIDE + 1, (16, 5))));
        // The two portal nodes of a crossing agree on its cost.
        assert_eq!(
            g.sector[0].cross[east + 1],
            g.sector[1].cross[WEST * SLOTS_PER_SIDE + 1]
        );
    }

    #[test]
    fn short_runs_get_one_portal_and_long_ones_three_or_four() {
        assert_eq!(portal_positions(4, 1), ([4, 0, 0, 0], 1));
        assert_eq!(portal_positions(4, 5), ([6, 0, 0, 0], 1));
        assert_eq!(portal_positions(4, 6), ([4, 6, 9, 0], 3));
        assert_eq!(portal_positions(0, 11), ([0, 5, 10, 0], 3));
        assert_eq!(portal_positions(0, 12), ([0, 3, 7, 11], 4));
        assert_eq!(portal_positions(0, 16), ([0, 5, 10, 15], 4));
        // Never more than one portal for every two positions: eight slots do.
        for len in 1..=16_u32 {
            let (_, n) = portal_positions(0, len);
            assert!(2 * n as u32 <= len + 1, "{len}: {n}");
        }
    }

    #[test]
    fn a_wall_with_a_gap_leaves_one_portal_at_the_gap() {
        let mut rows = vec![String::new(); 16];
        for (y, row) in rows.iter_mut().enumerate() {
            // Sector 0 is columns 0..15; column 15 is its east edge. Block the
            // cells of sector 1 just across the border, except rows 3..5.
            *row = (0..32)
                .map(|x| {
                    if x == 16 && !(3..=5).contains(&y) {
                        '#'
                    } else {
                        '.'
                    }
                })
                .collect();
        }
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        let mut local = Local::new();
        let g = ClassGraph::build(0, grid_from(&refs), &mut local);
        let east: Vec<_> = portals(&g, 0)
            .into_iter()
            .filter(|&(k, _)| k / SLOTS_PER_SIDE == EAST)
            .collect();
        assert_eq!(east, [(EAST * SLOTS_PER_SIDE, (15, 4))]);
    }

    #[test]
    fn alternating_open_and_blocked_positions_use_all_eight_slots() {
        let rows: Vec<String> = (0..16)
            .map(|y| {
                (0..32)
                    .map(|x| if x == 16 && y % 2 == 1 { '#' } else { '.' })
                    .collect()
            })
            .collect();
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        let mut local = Local::new();
        let g = ClassGraph::build(0, grid_from(&refs), &mut local);
        let east = portals(&g, 0)
            .into_iter()
            .filter(|&(k, _)| k / SLOTS_PER_SIDE == EAST)
            .count();
        assert_eq!(east, SLOTS_PER_SIDE);
    }

    #[test]
    fn intra_sector_edges_are_symmetric_and_cost_the_walk_inside_the_sector() {
        let mut local = Local::new();
        let g = ClassGraph::build(0, open(32, 32), &mut local);
        // Sector 0: the first portals east (15, 0) and south (0, 15) are fifteen
        // cells apart along a diagonal: fifteen diagonal steps.
        let s = &g.sector[0];
        let (a, b) = (EAST * SLOTS_PER_SIDE, SOUTH * SLOTS_PER_SIDE);
        let ab = s.edges(a).iter().find(|e| usize::from(e.0) == b).unwrap().1;
        let ba = s.edges(b).iter().find(|e| usize::from(e.0) == a).unwrap().1;
        assert_eq!(ab, ba);
        let expected = g.heur.octile((15, 0), (0, 15));
        assert_eq!(u64::from(ab), expected);
    }

    #[test]
    fn rebuilding_a_changed_sector_equals_building_from_scratch() {
        let mut local = Local::new();
        let mut g = ClassGraph::build(0, open(48, 32), &mut local);
        // Block a wall in sector 1 (columns 16..31, rows 0..15): a vertical
        // wall at x = 24 with a gap at rows six and seven.
        for y in 0..16 {
            if y != 6 && y != 7 {
                let i = g.grid.index(24, y);
                g.grid.set(i, 0);
            }
        }
        g.rebuild(&[1], &mut local);
        let fresh = ClassGraph::build(0, g.grid.clone(), &mut local);
        assert_eq!(g, fresh);
        // And the neighbours really were rewritten: remove the wall again.
        for y in 0..16 {
            let i = g.grid.index(24, y);
            g.grid.set(i, 1000);
        }
        g.rebuild(&[1], &mut local);
        let fresh = ClassGraph::build(0, g.grid.clone(), &mut local);
        assert_eq!(g, fresh);
    }

    #[test]
    fn the_whole_map_heuristic_has_the_floor_of_normal_speed_and_never_overestimates() {
        // A class whose fastest ground is slow (500): a unit standing in a
        // blocked cell leaves it at normal speed, so the step out of it costs
        // less than a step between two cells of the best ground.
        let mut grid = Grid::new(20, 4);
        for i in 0..20 * 4 {
            grid.set(i, 500);
        }
        let start = grid.index(2, 1);
        grid.set(start, 0);
        let mut local = Local::new();
        let g = ClassGraph::build(0, grid, &mut local);
        let step_out = g.grid.step_cost(2, 1, 1, 0);
        assert!(
            g.heur.octile((2, 1), (3, 1)) <= u64::from(step_out),
            "the heuristic {} is above the real step {step_out}",
            g.heur.octile((2, 1), (3, 1))
        );
        // The same floor as the heuristic of a window.
        assert_eq!(
            g.heur.octile((0, 0), (9, 0)),
            g.heuristic_in((0, 0, 20, 4)).octile((0, 0), (9, 0))
        );
        // And the floor is not a ceiling: faster ground still counts.
        let mut fast = Grid::new(8, 8);
        for i in 0..64 {
            fast.set(i, 1500);
        }
        let g = ClassGraph::build(0, fast, &mut local);
        assert!(g.heur.octile((0, 0), (4, 0)) < 4 * u64::from(crate::path::grid::COST_ONE));
    }

    #[test]
    fn node_numbers_round_trip() {
        let n = node(7, 13);
        assert_eq!((node_sector(n), node_slot(n)), (7, 13));
    }
}
