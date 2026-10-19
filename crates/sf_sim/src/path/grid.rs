//! The cell grid one movement class walks on, the cost of a step, the
//! heuristic and the sector geometry.
//!
//! # Costs
//!
//! A cost is the time a step takes at the unit's normal speed, as an integer:
//! [`COST_ONE`] (4096, a twelve bit fixed point fraction) for one cell at a
//! speed multiplier of 1000; a step from cell `a` to cell `b` covers half its
//! length in each cell (a unit walks from the middle of one cell to the middle
//! of the next), so it costs the average of the two cells' per-cell costs,
//! `4096 * 1000 / multiplier`; a diagonal step is that times the square root of
//! two, as the integer ratio 5793 over 4096; the cost of a step does not depend on
//! its direction, which the abstract graph relies on.
//!
//! The heuristic is the octile distance with the cheapest per-cell cost the
//! class has anywhere on the map (the highest speed multiplier among the cells
//! it can enter): never more than the real cost, so A* stays exact.

/// A cost not found yet.
pub(crate) const INF: u32 = u32::MAX;

/// The edge length of a sector, in cells. See `docs/rules/pathfinding.md` for
/// why sixteen.
pub const SECTOR_SIZE: u32 = 16;

/// `log2(SECTOR_SIZE)`.
pub(crate) const SECTOR_SHIFT: u32 = 4;

/// One cell at normal speed.
pub(crate) const COST_ONE: u32 = 4096;

/// Normal speed, in thousandths.
pub(crate) const FULL_SPEED: u16 = 1000;

/// The square root of two as a ratio: 5793 over 4096, rounded.
const SQRT2_Q12: u64 = 5793;

/// The cost of one cell at `permille` thousandths of normal speed. `permille`
/// must be above zero.
pub(crate) fn unit_cost(permille: u16) -> u32 {
    (u64::from(COST_ONE) * 1000 / u64::from(permille.max(1))) as u32
}

/// The cost of an orthogonal step between cells of the given speeds.
pub(crate) fn orth_cost(from: u16, to: u16) -> u32 {
    unit_cost(from).saturating_add(unit_cost(to)).div_ceil(2)
}

/// The cost of a diagonal step whose orthogonal cost would be `orth`.
pub(crate) fn diag_of(orth: u32) -> u32 {
    ((u64::from(orth) * SQRT2_Q12) >> 12) as u32
}

/// The grid of one movement class: the speed multiplier of every cell in
/// thousandths, zero where the class cannot go (impassable terrain, or a cell
/// a structure occupies).
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Grid {
    w: u32,
    h: u32,
    permille: Vec<u16>,
    /// The cost of one cell, `unit_cost` of its speed, kept so that a step does
    /// not divide; the cost of normal speed where the cell is blocked (a unit
    /// may be standing in one and leave it at normal speed).
    unit: Vec<u32>,
}

impl core::fmt::Debug for Grid {
    /// The size and the number of open cells, not the cells: a grid is tens of
    /// thousands of numbers and a failing assertion must stay readable.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let open = self.permille.iter().filter(|&&p| p != 0).count();
        write!(f, "Grid({}x{}, {open} open)", self.w, self.h)
    }
}

impl Grid {
    /// A grid with every cell blocked.
    pub fn new(w: u32, h: u32) -> Grid {
        Grid {
            w,
            h,
            permille: vec![0; w as usize * h as usize],
            unit: vec![COST_ONE; w as usize * h as usize],
        }
    }

    pub fn width(&self) -> u32 {
        self.w
    }

    pub fn height(&self) -> u32 {
        self.h
    }

    /// The row-major index of a cell inside the map.
    #[inline]
    pub fn index(&self, x: u32, y: u32) -> u32 {
        y * self.w + x
    }

    /// The column and row of a row-major index.
    #[inline]
    pub fn xy(&self, index: u32) -> (u32, u32) {
        (index % self.w, index / self.w)
    }

    /// Sets the speed of the cell at `index` (zero blocks it).
    pub fn set(&mut self, index: u32, permille: u16) {
        self.permille[index as usize] = permille;
        self.unit[index as usize] = if permille == 0 {
            COST_ONE
        } else {
            unit_cost(permille)
        };
    }

    /// Shuts the cells of a request's overlay (WP4.7) and writes the speeds they had
    /// to `saved`, for [`Grid::reopen`]. Only ever held for the length of one slice of
    /// one job: nothing else is served while the cells are shut.
    pub fn shut(&mut self, cells: &[u32], saved: &mut [u16]) {
        for (k, &c) in cells.iter().enumerate() {
            saved[k] = self.permille[c as usize];
            self.set(c, 0);
        }
    }

    /// Puts the cells shut by [`Grid::shut`] back as they were.
    pub fn reopen(&mut self, cells: &[u32], saved: &[u16]) {
        for (k, &c) in cells.iter().enumerate() {
            self.set(c, saved[k]);
        }
    }

    /// The speed multiplier of an in-map cell; zero if blocked.
    #[inline]
    pub fn speed(&self, x: u32, y: u32) -> u16 {
        self.permille[(y * self.w + x) as usize]
    }

    /// Whether the cell is inside the map and may be entered.
    #[inline]
    pub fn open(&self, x: i64, y: i64) -> bool {
        x >= 0
            && y >= 0
            && x < i64::from(self.w)
            && y < i64::from(self.h)
            && self.permille[(y as u32 * self.w + x as u32) as usize] != 0
    }

    /// The highest multiplier of any cell; zero for a grid with none open.
    pub fn max_speed(&self) -> u16 {
        self.permille.iter().copied().max().unwrap_or(0)
    }

    /// Whether a unit may step from `(x, y)` to its neighbour `(x + dx, y +
    /// dy)`: the neighbour is open and, for a diagonal step, so are the two
    /// cells it cuts across. This is the rule of `Ground::may_step`; the cell
    /// stepped from is not looked at (a unit standing in a blocked cell may
    /// leave it).
    #[inline]
    pub fn can_step(&self, x: u32, y: u32, dx: i64, dy: i64) -> bool {
        let (nx, ny) = (i64::from(x) + dx, i64::from(y) + dy);
        if !self.open(nx, ny) {
            return false;
        }
        dx == 0 || dy == 0 || (self.open(nx, i64::from(y)) && self.open(i64::from(x), ny))
    }

    /// The cost of the step `can_step` allows.
    #[inline]
    pub fn step_cost(&self, x: u32, y: u32, dx: i64, dy: i64) -> u32 {
        let (nx, ny) = ((i64::from(x) + dx) as u32, (i64::from(y) + dy) as u32);
        let here = self.unit[(y * self.w + x) as usize];
        let there = self.unit[(ny * self.w + nx) as usize];
        let orth = here.saturating_add(there).div_ceil(2);
        if dx == 0 || dy == 0 {
            orth
        } else {
            diag_of(orth)
        }
    }
}

/// The eight neighbours, in the order the searches try them: the four
/// orthogonal ones, then the diagonals. The order only matters for ties and is
/// fixed.
pub(crate) const DIRS: [(i64, i64); 8] = [
    (0, -1),
    (1, 0),
    (0, 1),
    (-1, 0),
    (1, -1),
    (1, 1),
    (-1, 1),
    (-1, -1),
];

/// The cheapest per-cell costs of a class, for the heuristic.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct Heuristic {
    orth: u32,
    diag: u32,
}

impl Heuristic {
    /// From the highest speed multiplier the class has on the map.
    pub fn from_best_speed(best: u16) -> Heuristic {
        let orth = unit_cost(best.max(1));
        Heuristic {
            orth,
            diag: diag_of(orth),
        }
    }

    /// The octile distance between two cells, in cost units.
    #[inline]
    pub fn octile(&self, a: (u32, u32), b: (u32, u32)) -> u64 {
        let dx = u64::from(a.0.abs_diff(b.0));
        let dy = u64::from(a.1.abs_diff(b.1));
        let (long, short) = if dx >= dy { (dx, dy) } else { (dy, dx) };
        u64::from(self.orth) * (long - short) + u64::from(self.diag) * short
    }

    /// The cost of one orthogonal step at best speed.
    #[cfg(test)]
    pub fn orth(&self) -> u32 {
        self.orth
    }
}

/// The sector decomposition of a map: sectors of [`SECTOR_SIZE`] by
/// [`SECTOR_SIZE`] cells, the last row and column smaller when the map side
/// is not a multiple of it. A sector is a number, `sy * sectors_wide + sx`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct Sectors {
    /// The map size in cells.
    w: u32,
    h: u32,
    /// The number of sectors across and down.
    pub across: u32,
    pub down: u32,
}

/// The four sides of a sector, in the order of the portal slots.
pub(crate) const NORTH: usize = 0;
pub(crate) const EAST: usize = 1;
pub(crate) const SOUTH: usize = 2;
pub(crate) const WEST: usize = 3;

/// The side a border is seen from on the other side.
#[inline]
pub(crate) const fn opposite(side: usize) -> usize {
    (side + 2) % 4
}

impl Sectors {
    pub fn new(w: u32, h: u32) -> Sectors {
        Sectors {
            w,
            h,
            across: w.div_ceil(SECTOR_SIZE),
            down: h.div_ceil(SECTOR_SIZE),
        }
    }

    pub fn count(&self) -> usize {
        self.across as usize * self.down as usize
    }

    /// The sector a cell is in.
    #[inline]
    pub fn of_cell(&self, x: u32, y: u32) -> u32 {
        (y >> SECTOR_SHIFT) * self.across + (x >> SECTOR_SHIFT)
    }

    /// The column and row of a sector.
    #[inline]
    pub fn coords(&self, sector: u32) -> (u32, u32) {
        (sector % self.across, sector / self.across)
    }

    /// The cells of a sector: `(x0, y0, x1, y1)`, the far ends exclusive.
    pub fn rect(&self, sector: u32) -> (u32, u32, u32, u32) {
        let (sx, sy) = self.coords(sector);
        let (x0, y0) = (sx * SECTOR_SIZE, sy * SECTOR_SIZE);
        (
            x0,
            y0,
            (x0 + SECTOR_SIZE).min(self.w),
            (y0 + SECTOR_SIZE).min(self.h),
        )
    }

    /// The sector next to `sector` across `side`, if the map has one.
    pub fn neighbor(&self, sector: u32, side: usize) -> Option<u32> {
        let (sx, sy) = self.coords(sector);
        let (nx, ny) = match side {
            NORTH => (sx, sy.checked_sub(1)?),
            EAST => (sx + 1, sy),
            SOUTH => (sx, sy + 1),
            WEST => (sx.checked_sub(1)?, sy),
            _ => return None,
        };
        (nx < self.across && ny < self.down).then(|| ny * self.across + nx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_at_normal_speed_costs_one_unit_and_slower_costs_more() {
        assert_eq!(unit_cost(1000), COST_ONE);
        assert_eq!(unit_cost(500), 2 * COST_ONE);
        assert!(unit_cost(1200) < COST_ONE);
        assert_eq!(orth_cost(1000, 1000), COST_ONE);
        // Symmetric: the abstract graph needs it.
        assert_eq!(orth_cost(800, 1200), orth_cost(1200, 800));
        // The diagonal is about the square root of two longer.
        let d = diag_of(COST_ONE);
        assert!((5790..=5796).contains(&d), "{d}");
    }

    #[test]
    fn the_octile_heuristic_never_exceeds_the_cheapest_walk() {
        let h = Heuristic::from_best_speed(1200);
        // Straight and diagonal, at the best speed: exact.
        assert_eq!(h.octile((0, 0), (10, 0)), 10 * u64::from(h.orth()));
        assert_eq!(h.octile((0, 0), (3, 3)), 3 * u64::from(diag_of(h.orth())));
        assert_eq!(
            h.octile((0, 0), (5, 2)),
            3 * u64::from(h.orth()) + 2 * u64::from(diag_of(h.orth()))
        );
        // A slower class pays more for the same walk.
        assert!(h.octile((0, 0), (10, 0)) < 10 * u64::from(unit_cost(1000)));
    }

    #[test]
    fn steps_obey_the_movement_rule_including_the_corner() {
        let mut g = Grid::new(3, 3);
        for i in 0..9 {
            g.set(i, 1000);
        }
        assert!(g.can_step(1, 1, 1, 1));
        // Block one of the two cells a diagonal step cuts across.
        g.set(g.index(2, 1), 0);
        assert!(!g.can_step(1, 1, 1, 1));
        assert!(g.can_step(1, 1, 0, 1));
        // The edge of the map.
        assert!(!g.can_step(0, 0, -1, 0));
        // A blocked cell may be left, not entered.
        g.set(g.index(0, 0), 0);
        assert!(g.can_step(0, 0, 1, 0));
        assert!(!g.can_step(1, 0, -1, 0));
        // Leaving a blocked cell is at normal speed.
        assert_eq!(g.step_cost(0, 0, 1, 0), COST_ONE);
    }

    #[test]
    fn sectors_cover_the_map_and_the_edge_ones_are_smaller() {
        let s = Sectors::new(40, 20);
        assert_eq!((s.across, s.down, s.count()), (3, 2, 6));
        assert_eq!(s.rect(0), (0, 0, 16, 16));
        assert_eq!(s.rect(2), (32, 0, 40, 16));
        assert_eq!(s.rect(5), (32, 16, 40, 20));
        assert_eq!(s.of_cell(39, 19), 5);
        assert_eq!(s.neighbor(0, EAST), Some(1));
        assert_eq!(s.neighbor(0, WEST), None);
        assert_eq!(s.neighbor(0, NORTH), None);
        assert_eq!(s.neighbor(0, SOUTH), Some(3));
        assert_eq!(s.neighbor(5, EAST), None);
        assert_eq!(opposite(NORTH), SOUTH);
        assert_eq!(opposite(EAST), WEST);
    }

    #[test]
    fn shutting_cells_and_opening_them_again_leaves_the_grid_as_it_was() {
        let mut g = Grid::new(6, 4);
        for i in 0..24 {
            g.set(
                i,
                if i % 5 == 0 {
                    0
                } else {
                    500 + 100 * (i as u16 % 4)
                },
            );
        }
        let before = g.clone();
        // Cells with different speeds and one that is blocked already.
        let cells = [0_u32, 3, 7, 14, 20];
        let mut saved = [0_u16; 8];
        g.shut(&cells, &mut saved);
        for &c in &cells {
            assert_eq!(g.speed(c % 6, c / 6), 0);
        }
        assert!(!g.can_step(2, 0, 1, 0), "the cell east of (2, 0) is shut");
        g.reopen(&cells, &saved);
        assert!(g == before, "{g:?} {before:?}");
        assert_eq!(g.step_cost(2, 0, 1, 0), before.step_cost(2, 0, 1, 0));
    }
}
