//! Line of sight with a margin, for pulling a route taut.
//!
//! A unit does not walk the exact line between two waypoints: it turns at the
//! unit's own rate, moves while still up to 16 facing steps off, and so strays
//! from the line by a fraction of a cell near a corner. A line of sight that
//! only asks "does the line touch a blocked cell" would let a route graze a
//! corner and the unit would then be refused a step (`Ground::may_step`). So
//! the test here is **thick**: a segment is clear when no blocked cell (and no
//! cell outside the map) lies within [`MARGIN`], a quarter of a cell, of it.
//! Two waypoints that cannot see each other like this keep the cell by cell
//! route between them.
//!
//! The test sweeps the segment along its longer axis, one column of cells at a
//! time, and in each column looks at the rows the segment, widened by the
//! margin, reaches. All integer arithmetic on the raw fixed-point numbers; the
//! number of cells looked at is the cost the budget is charged.

use sf_math::Vec2;

use crate::path::grid::Grid;

/// How far from a segment a blocked cell may be before the segment is not
/// clear: a quarter of a cell, in raw fixed point.
pub(crate) const MARGIN: i64 = 1 << 14;

/// One cell in raw fixed point.
const ONE: i64 = 1 << 16;

/// The longest segment smoothing will try, in cells along the longer axis.
/// One sector: a segment is then never longer than a sector side, so it stays
/// inside the block of three by three sectors around the sector it starts in,
/// which is how a changed sector finds the routes it touches.
pub(crate) const LOS_MAX_CELLS: i64 = 16;

/// The most cells a test of a segment can look at: used to decide whether the
/// budget has room for it before it starts. At most three rows per column and
/// three columns more than the segment is long.
pub(crate) fn bound(p: Vec2, q: Vec2) -> u32 {
    let long = (q.x.raw() - p.x.raw())
        .abs()
        .max((q.y.raw() - p.y.raw()).abs());
    3 * ((long / ONE) as u32 + 4)
}

/// Whether the segment is short enough for [`thick_clear`].
pub(crate) fn short_enough(p: Vec2, q: Vec2) -> bool {
    let long = (q.x.raw() - p.x.raw())
        .abs()
        .max((q.y.raw() - p.y.raw()).abs());
    long <= LOS_MAX_CELLS * ONE
}

fn floor_div(a: i64, b: i64) -> i64 {
    a.div_euclid(b)
}

/// Sweeps one axis. `(a_major, a_minor)` and `(b_major, b_minor)` are the ends
/// of the segment with the longer axis first; `open(major, minor)` says whether
/// the cell at those coordinates may be entered. Returns whether the segment is
/// clear; `examined` is increased by the number of cells looked at.
fn sweep(
    a: (i64, i64),
    b: (i64, i64),
    open: impl Fn(i64, i64) -> bool,
    examined: &mut u32,
) -> bool {
    let (a, b) = if a.0 <= b.0 { (a, b) } else { (b, a) };
    let d_major = b.0 - a.0;
    let d_minor = b.1 - a.1;
    let first = floor_div(a.0 - MARGIN, ONE);
    let last = floor_div(b.0 + MARGIN, ONE);
    for column in first..=last {
        // The stretch of the segment within reach of this column.
        let lo = (column * ONE - MARGIN).max(a.0);
        let hi = ((column + 1) * ONE + MARGIN).min(b.0);
        if lo > hi {
            continue;
        }
        let at = |major: i64| {
            if d_major == 0 {
                a.1
            } else {
                a.1 + (major - a.0) * d_minor / d_major
            }
        };
        let (m0, m1) = (at(lo), at(hi));
        let rows = (
            floor_div(m0.min(m1) - MARGIN, ONE),
            floor_div(m0.max(m1) + MARGIN, ONE),
        );
        for row in rows.0..=rows.1 {
            *examined += 1;
            if !open(column, row) {
                return false;
            }
        }
    }
    true
}

/// Whether the segment from `p` to `q` is clear of blocked cells by the margin,
/// and how many cells were looked at (the cost; at most `bound(p, q)`).
pub(crate) fn thick_clear(grid: &Grid, p: Vec2, q: Vec2) -> (bool, u32) {
    let (px, py, qx, qy) = (p.x.raw(), p.y.raw(), q.x.raw(), q.y.raw());
    let mut examined = 0;
    let clear = if (qx - px).abs() >= (qy - py).abs() {
        sweep((px, py), (qx, qy), |c, r| grid.open(c, r), &mut examined)
    } else {
        sweep((py, px), (qy, qx), |c, r| grid.open(r, c), &mut examined)
    };
    (clear, examined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_math::Fx;

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

    fn pt(x: i64, y: i64) -> Vec2 {
        // In raw units, so the tests can aim at exact fractions.
        Vec2::new(Fx::from_raw(x), Fx::from_raw(y))
    }

    fn centre(x: i64, y: i64) -> Vec2 {
        pt(x * ONE + ONE / 2, y * ONE + ONE / 2)
    }

    #[test]
    fn open_ground_is_clear_in_every_direction() {
        let g = grid_from(&["........", "........", "........", "........"]);
        for (a, b) in [
            ((1, 1), (6, 2)),
            ((6, 2), (1, 1)),
            ((2, 0), (2, 3)),
            ((1, 1), (1, 1)),
        ] {
            let (clear, n) = thick_clear(&g, centre(a.0, a.1), centre(b.0, b.1));
            assert!(clear, "{a:?} to {b:?}");
            assert!(n <= bound(centre(a.0, a.1), centre(b.0, b.1)));
        }
    }

    #[test]
    fn a_blocked_cell_on_or_near_the_line_stops_it() {
        let g = grid_from(&["........", "...#....", "........"]);
        // Through the block, and grazing it: the margin refuses both.
        assert!(!thick_clear(&g, centre(0, 1), centre(7, 1)).0);
        // Along the top row the line runs through the middle of row 0, half a
        // cell from the row below and more than the margin from the block in
        // row 1: clear. Along row 2 the same.
        assert!(thick_clear(&g, centre(0, 0), centre(7, 0)).0);
        assert!(thick_clear(&g, centre(0, 2), centre(7, 2)).0);
        // Cutting the corner of the block diagonally touches it.
        assert!(!thick_clear(&g, centre(2, 0), centre(4, 2)).0);
    }

    #[test]
    fn a_line_that_passes_a_corner_closer_than_the_margin_is_refused() {
        // The block is the cell (2, 1). A line through the middle of row 0 is
        // half a cell from it: clear. The same line lowered to nine tenths of
        // the way down the row is a tenth of a cell from it: refused.
        let g = grid_from(&["....", "..#.", "...."]);
        assert!(thick_clear(&g, pt(ONE / 2, ONE / 2), pt(3 * ONE + ONE / 2, ONE / 2)).0);
        let high = ONE * 9 / 10;
        assert!(!thick_clear(&g, pt(ONE / 2, high), pt(3 * ONE + ONE / 2, high)).0);
    }

    #[test]
    fn the_map_edge_counts_as_blocked() {
        let g = grid_from(&["....", "....", "...."]);
        // A line along the top edge, a tenth of a cell inside the map, is within the
        // margin of the edge.
        assert!(!thick_clear(&g, pt(ONE / 2, ONE / 10), pt(3 * ONE, ONE / 10)).0);
        assert!(thick_clear(&g, pt(ONE / 2, ONE / 2), pt(3 * ONE, ONE / 2)).0);
    }

    /// Sound: whenever the test says clear, a fine sampling of the segment
    /// widened by the margin finds no blocked cell.
    #[test]
    fn clear_means_clear_on_random_grids() {
        let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let (w, h) = (12_u32, 12_u32);
        let mut clear_cases = 0;
        for _ in 0..400 {
            let mut g = Grid::new(w, h);
            for i in 0..w * h {
                // About one cell in seven blocked.
                g.set(i, if next() % 7 == 0 { 0 } else { 1000 });
            }
            let mut point = || {
                let x = (next() % (w as u64 * ONE as u64 - 1)) as i64;
                let y = (next() % (h as u64 * ONE as u64 - 1)) as i64;
                pt(x, y)
            };
            let (p, q) = (point(), point());
            let (ok, n) = thick_clear(&g, p, q);
            assert!(n <= bound(p, q));
            if !ok {
                continue;
            }
            clear_cases += 1;
            let steps = 400;
            for k in 0..=steps {
                let t = k as i64;
                let x = p.x.raw() + (q.x.raw() - p.x.raw()) * t / steps as i64;
                let y = p.y.raw() + (q.y.raw() - p.y.raw()) * t / steps as i64;
                for (ox, oy) in [
                    (-1, -1),
                    (0, -1),
                    (1, -1),
                    (-1, 0),
                    (0, 0),
                    (1, 0),
                    (-1, 1),
                    (0, 1),
                    (1, 1),
                ] {
                    let (sx, sy) = (x + ox * MARGIN, y + oy * MARGIN);
                    assert!(
                        g.open(floor_div(sx, ONE), floor_div(sy, ONE)),
                        "a blocked cell within the margin of {p:?} to {q:?}"
                    );
                }
            }
        }
        assert!(clear_cases > 20, "the test must see clear segments too");
    }

    #[test]
    fn short_enough_is_the_longer_axis() {
        assert!(short_enough(centre(0, 0), centre(16, 3)));
        assert!(!short_enough(centre(0, 0), centre(17, 0)));
        assert!(!short_enough(centre(0, 0), centre(2, 17)));
    }
}
