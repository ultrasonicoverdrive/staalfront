//! The search inside one sector, or inside two sectors side by side.
//!
//! Dijkstra when there is no goal (the costs between the portals of a sector,
//! the cost from a unit to each portal of its sector) and A* when there is one
//! (the leg of a route inside a sector, or across the border of two). A sector
//! is at most 16 by 16 cells and two neighbours make a rectangle of at most
//! 512, so a search expands at most that many cells; the arrays are fixed size
//! and nothing is allocated. Cells are numbered inside the rectangle, `(y -
//! y0) * width + (x - x0)`.

use crate::path::grid::{DIRS, Grid, Heuristic, INF, SECTOR_SIZE};
use crate::path::heap::IndexedHeap;

/// Cells in a full sector.
pub(crate) const SECTOR_CELLS: usize = (SECTOR_SIZE * SECTOR_SIZE) as usize;

/// Cells in two full sectors side by side: the most one local search can
/// expand.
pub(crate) const WINDOW_CELLS: usize = 2 * SECTOR_CELLS;

/// The parent of a cell the search did not reach (or of its start).
const NO_PARENT: u16 = u16::MAX;

/// A sector, or two next to each other, as a rectangle of the map: `(x0, y0,
/// x1, y1)`, far ends exclusive.
pub(crate) type Rect = (u32, u32, u32, u32);

/// The number of cells in a rectangle.
pub(crate) fn cells_in(rect: Rect) -> u32 {
    (rect.2 - rect.0) * (rect.3 - rect.1)
}

/// The smallest rectangle holding both.
pub(crate) fn union(a: Rect, b: Rect) -> Rect {
    (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))
}

/// What to find.
pub(crate) struct Query<'a> {
    /// The sector (or the two sectors) to search in.
    pub rect: Rect,
    /// Where to start, a cell inside `rect`.
    pub src: (u32, u32),
    /// A cell to find the cheapest way to; the search is A* toward it and stops
    /// when it is reached. `None`: Dijkstra over the whole sector.
    pub goal: Option<(u32, u32)>,
    /// A cell to get as near to as possible (by the heuristic): the search
    /// remembers the cell it has expanded that is nearest to it.
    pub toward: Option<(u32, u32)>,
    /// Cells (inside the rectangle, as [`Local::number`]s) whose cost is wanted:
    /// the search stops when it has expanded all of them.
    pub wanted: &'a [u16],
}

/// How a search went.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct Outcome {
    /// Cells expanded: what the budget is charged.
    pub expanded: u32,
    /// Whether `goal` was reached.
    pub reached: bool,
    /// The expanded cell nearest to `toward`: the smallest heuristic, then the
    /// cheapest to reach from the start, then the lowest row-major cell. As a
    /// map cell.
    pub nearest: Option<(u32, u32)>,
    /// What it costs to reach `nearest` from the start.
    pub nearest_cost: u32,
}

/// The arrays of a local search, reused for every search.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Local {
    /// The cost from the start to each cell, [`INF`] where not reached.
    pub dist: [u32; WINDOW_CELLS],
    parent: [u16; WINDOW_CELLS],
    wanted: [bool; WINDOW_CELLS],
    heap: IndexedHeap,
}

impl Local {
    pub fn new() -> Local {
        Local {
            dist: [INF; WINDOW_CELLS],
            parent: [NO_PARENT; WINDOW_CELLS],
            wanted: [false; WINDOW_CELLS],
            heap: IndexedHeap::new(WINDOW_CELLS),
        }
    }

    /// The number of a map cell inside the rectangle `rect`.
    #[inline]
    pub fn number(rect: Rect, x: u32, y: u32) -> u16 {
        ((y - rect.1) * (rect.2 - rect.0) + (x - rect.0)) as u16
    }

    /// The map cell of a number inside the rectangle.
    #[inline]
    pub fn cell(rect: Rect, number: u16) -> (u32, u32) {
        let width = rect.2 - rect.0;
        (
            rect.0 + u32::from(number) % width,
            rect.1 + u32::from(number) / width,
        )
    }

    /// Runs the search. Afterwards [`Local::dist`] holds the costs and
    /// [`Local::path`] can read the way back from any cell it reached.
    pub fn search(&mut self, grid: &Grid, heur: &Heuristic, q: &Query<'_>) -> Outcome {
        let used = cells_in(q.rect) as usize;
        self.dist[..used].fill(INF);
        self.parent[..used].fill(NO_PARENT);
        self.heap.clear();
        let rect = q.rect;
        // Two portals can be the same cell (the corner of a sector): count it once.
        let mut still_wanted = 0;
        for &w in q.wanted {
            if !self.wanted[usize::from(w)] {
                self.wanted[usize::from(w)] = true;
                still_wanted += 1;
            }
        }

        let h = |x: u32, y: u32| q.goal.map_or(0, |g| heur.octile((x, y), g));
        let start = usize::from(Local::number(rect, q.src.0, q.src.1));
        self.dist[start] = 0;
        self.heap
            .push_or_lower(start as u32, (h(q.src.0, q.src.1), h(q.src.0, q.src.1)));

        let mut out = Outcome {
            expanded: 0,
            reached: false,
            nearest: None,
            nearest_cost: 0,
        };
        let mut nearest_key: Option<(u64, u32, u32)> = None;
        while let Some(top) = self.heap.pop() {
            let at = top as usize;
            let (x, y) = Local::cell(rect, at as u16);
            out.expanded += 1;
            if let Some(t) = q.toward {
                let key = (heur.octile((x, y), t), self.dist[at], grid.index(x, y));
                if nearest_key.is_none_or(|best| key < best) {
                    nearest_key = Some(key);
                    out.nearest = Some((x, y));
                    out.nearest_cost = self.dist[at];
                }
            }
            if q.goal == Some((x, y)) {
                out.reached = true;
                break;
            }
            if self.wanted[at] {
                self.wanted[at] = false;
                still_wanted -= 1;
                if still_wanted == 0 {
                    break;
                }
            }
            let g = self.dist[at];
            for &(dx, dy) in &DIRS {
                let (nx, ny) = (i64::from(x) + dx, i64::from(y) + dy);
                if nx < i64::from(rect.0)
                    || ny < i64::from(rect.1)
                    || nx >= i64::from(rect.2)
                    || ny >= i64::from(rect.3)
                    || !grid.can_step(x, y, dx, dy)
                {
                    continue;
                }
                let (nx, ny) = (nx as u32, ny as u32);
                let n = usize::from(Local::number(rect, nx, ny));
                let cost = g.saturating_add(grid.step_cost(x, y, dx, dy));
                if cost < self.dist[n] {
                    self.dist[n] = cost;
                    self.parent[n] = at as u16;
                    let hn = h(nx, ny);
                    self.heap
                        .push_or_lower(n as u32, (u64::from(cost) + hn, hn));
                }
            }
        }
        // Leave the wanted marks clean for the next search.
        for &w in q.wanted {
            self.wanted[usize::from(w)] = false;
        }
        out
    }

    /// The cells from the start to `to` (both included), as map cell indices,
    /// appended to `out`. `to` must have been reached by the last search.
    pub fn path(&self, grid: &Grid, rect: Rect, to: (u32, u32), out: &mut Vec<u32>) {
        let from = out.len();
        let mut at = Local::number(rect, to.0, to.1);
        loop {
            let (x, y) = Local::cell(rect, at);
            out.push(grid.index(x, y));
            let p = self.parent[usize::from(at)];
            if p == NO_PARENT {
                break;
            }
            at = p;
        }
        out[from..].reverse();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::grid::COST_ONE;

    fn open_grid(w: u32, h: u32) -> Grid {
        let mut g = Grid::new(w, h);
        for i in 0..w * h {
            g.set(i, 1000);
        }
        g
    }

    fn heur() -> Heuristic {
        Heuristic::from_best_speed(1000)
    }

    #[test]
    fn dijkstra_over_open_ground_costs_the_octile_distance() {
        let g = open_grid(16, 16);
        let mut local = Local::new();
        let rect = (0, 0, 16, 16);
        let out = local.search(
            &g,
            &heur(),
            &Query {
                rect,
                src: (0, 0),
                goal: None,
                toward: None,
                wanted: &[],
            },
        );
        assert_eq!(out.expanded, 256);
        let far = usize::from(Local::number(rect, 15, 15));
        assert_eq!(u64::from(local.dist[far]), heur().octile((0, 0), (15, 15)));
        let side = usize::from(Local::number(rect, 15, 0));
        assert_eq!(local.dist[side], 15 * COST_ONE);
    }

    #[test]
    fn a_star_finds_the_goal_and_the_path_walks_around_a_wall() {
        let mut g = open_grid(16, 16);
        for y in 0..12 {
            let i = g.index(8, y);
            g.set(i, 0);
        }
        let mut local = Local::new();
        let rect = (0, 0, 16, 16);
        let out = local.search(
            &g,
            &heur(),
            &Query {
                rect,
                src: (2, 2),
                goal: Some((14, 2)),
                toward: None,
                wanted: &[],
            },
        );
        assert!(out.reached);
        let mut cells = Vec::new();
        local.path(&g, rect, (14, 2), &mut cells);
        assert_eq!(cells.first(), Some(&g.index(2, 2)));
        assert_eq!(cells.last(), Some(&g.index(14, 2)));
        // It goes below the wall, which ends in row eleven.
        assert!(cells.iter().any(|&c| g.xy(c).1 >= 12));
        assert!(cells.iter().all(|&c| g.speed(g.xy(c).0, g.xy(c).1) != 0));
        // Each step is to a neighbour.
        for pair in cells.windows(2) {
            let (a, b) = (g.xy(pair[0]), g.xy(pair[1]));
            assert!(a.0.abs_diff(b.0) <= 1 && a.1.abs_diff(b.1) <= 1);
        }
    }

    #[test]
    fn the_nearest_cell_toward_an_unreachable_goal_is_the_closest_one_reached() {
        let mut g = open_grid(16, 16);
        // A closed room around (12, 12): the goal is inside it.
        for x in 10..=14 {
            for y in [10, 14] {
                let i = g.index(x, y);
                g.set(i, 0);
            }
        }
        for y in 10..=14 {
            for x in [10, 14] {
                let i = g.index(x, y);
                g.set(i, 0);
            }
        }
        let mut local = Local::new();
        let out = local.search(
            &g,
            &heur(),
            &Query {
                rect: (0, 0, 16, 16),
                src: (0, 0),
                goal: Some((12, 12)),
                toward: Some((12, 12)),
                wanted: &[],
            },
        );
        assert!(!out.reached);
        // Three cells straight out from the goal, above the wall (the cell left
        // of the wall is as near; the lower cell number wins the tie).
        assert_eq!(out.nearest, Some((12, 9)));
    }

    #[test]
    fn wanted_cells_end_the_search_early_and_a_blocked_start_may_be_left() {
        let mut g = open_grid(16, 16);
        let i = g.index(1, 1);
        g.set(i, 0);
        let mut local = Local::new();
        let rect = (0, 0, 16, 16);
        let wanted = [Local::number(rect, 2, 1)];
        let out = local.search(
            &g,
            &heur(),
            &Query {
                rect,
                src: (1, 1),
                goal: None,
                toward: None,
                wanted: &wanted,
            },
        );
        assert!(out.expanded < 30, "stopped early: {}", out.expanded);
        assert_eq!(local.dist[usize::from(wanted[0])], COST_ONE);
    }
}
