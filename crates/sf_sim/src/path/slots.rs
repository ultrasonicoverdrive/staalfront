//! The arrival slots around a goal: where each unit of a group stands when it
//! has arrived.
//!
//! A group order sends many units to one goal, and a unit that stops anywhere
//! within a few cells of it is where the player asked. If all of them aimed at
//! the middle of the goal cell they would pile up on one point, so every unit
//! has a **slot** of its own: a point on a lattice around the goal, handed out
//! going outwards in a spiral, by rank (the unit's place among the group's
//! members in ascending entity id order).
//!
//! The lattice has four points to a cell, half a cell apart, a quarter of a
//! cell in from the cell's sides (a point on a cell's edge would be a point two
//! cells share, and a point near an edge is near the neighbour's rock). The
//! spiral orders the points by their distance from the middle of the goal cell,
//! ties by row and then column, so the group fills a disc: 200 units need a
//! radius of about four cells, inside the five the phase asks for. Half a
//! cell is closer than a vehicle is long; units overlap until collision
//! (WP4.4) and formations (WP4.6) give them room, and the spacing is the one
//! constant to revisit then ([`SPACING_QUARTERS`]).
//!
//! A point is a **slot** for a class when its cell can be entered and the line
//! from the middle of the goal cell to it is clear by the thick line of sight
//! (`path/los.rs`): a unit that has followed the field down to the goal can
//! always walk straight on to its slot. The lattice is the same for every
//! class, but which points are valid differs (a bridge cell is open ground for
//! a tank and water to a boat), so each field filters its own, starting its
//! scan at an offset so that the classes of one order take different stretches
//! of the spiral instead of all starting in the middle.

use sf_math::{Fx, Vec2};

/// Lattice points on each side of the middle, in each direction: the table is
/// `2 * HALF_WIDTH` points wide and high.
pub(crate) const HALF_WIDTH: i32 = 12;

/// The distance between two lattice points, in quarters of a cell. Two quarters:
/// half a cell.
pub(crate) const SPACING_QUARTERS: i32 = 2;

/// A lattice point as a pair of integers `(a, b)`: it lies `(a + 1/2)` spacings
/// east and `(b + 1/2)` spacings south of the middle of the goal cell.
pub(crate) type Point = (i8, i8);

/// All the lattice points, nearest to the middle first. A pure function of the
/// constants above; computed once per world.
pub(crate) fn spiral() -> Vec<Point> {
    let mut all: Vec<(i32, i32, i32)> = Vec::with_capacity((4 * HALF_WIDTH * HALF_WIDTH) as usize);
    for b in -HALF_WIDTH..HALF_WIDTH {
        for a in -HALF_WIDTH..HALF_WIDTH {
            // Distance squared in units of half a spacing, so it is an integer.
            let (x, y) = (2 * a + 1, 2 * b + 1);
            all.push((x * x + y * y, b, a));
        }
    }
    all.sort_unstable();
    all.into_iter()
        .map(|(_, b, a)| (a as i8, b as i8))
        .collect()
}

/// The position of a lattice point around the middle of a goal cell.
pub(crate) fn position(centre: Vec2, p: Point) -> Vec2 {
    // (a + 1/2) spacings of SPACING_QUARTERS/4 cells each, that is (2a + 1) * SPACING_QUARTERS / 8 cells.
    let to = |k: i8| {
        Fx::from_ratio(
            i64::from(2 * i32::from(k) + 1) * i64::from(SPACING_QUARTERS),
            8,
        )
    };
    Vec2::new(centre.x + to(p.0), centre.y + to(p.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spiral_goes_outwards_and_covers_the_table_once() {
        let s = spiral();
        assert_eq!(s.len(), (4 * HALF_WIDTH * HALF_WIDTH) as usize);
        let d2 = |p: &Point| {
            let (x, y) = (2 * i32::from(p.0) + 1, 2 * i32::from(p.1) + 1);
            x * x + y * y
        };
        assert!(s.windows(2).all(|w| d2(&w[0]) <= d2(&w[1])));
        let mut sorted = s.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), s.len(), "no point twice");
        // The first four are the quarter points of the goal cell itself.
        let centre = Vec2::from_ints(10, 10);
        let first: Vec<Vec2> = s[..4].iter().map(|&p| position(centre, p)).collect();
        for p in first {
            assert_eq!((p.x - centre.x).abs(), Fx::from_ratio(1, 4));
            assert_eq!((p.y - centre.y).abs(), Fx::from_ratio(1, 4));
        }
    }

    #[test]
    fn four_points_to_a_cell_half_a_cell_apart() {
        let centre = Vec2::new(Fx::from_ratio(21, 2), Fx::from_ratio(21, 2)); // middle of cell (10, 10)
        let s = spiral();
        let mut in_cell = 0;
        for &p in &s {
            let q = position(centre, p);
            if q.cell() == (10, 10) {
                in_cell += 1;
                // A quarter or three quarters in: never on an edge.
                let f = q.x - Fx::from_int(10);
                assert!(f == Fx::from_ratio(1, 4) || f == Fx::from_ratio(3, 4));
            }
        }
        assert_eq!(in_cell, 4);
    }

    #[test]
    fn two_hundred_slots_fit_inside_five_cells() {
        let centre = Vec2::new(Fx::from_ratio(1, 2), Fx::from_ratio(1, 2));
        let s = spiral();
        let far = position(centre, s[199]);
        let d = (far - centre).length();
        assert!(d < Fx::from_int(5), "{d:?}");
        assert!(d > Fx::from_int(3), "{d:?}");
    }
}
