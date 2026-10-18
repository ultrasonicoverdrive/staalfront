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
//! Since WP4.4 the units have bodies (`docs/rules/collision.md`), so the lattice
//! is made for the bodies it holds. Each class of the order has a **spacing**: the
//! smallest distance its bodies keep from each other, plus a sixteenth of a cell
//! ([`spacing_for`]). A square lattice of that spacing has one slot for every
//! `spacing * spacing` of ground, and the spiral orders its points by their
//! distance from the middle of the goal cell, ties by row and then column, so the
//! group fills a disc. The classes of one order take **rings**, the fastest
//! first (it arrives first, and the slower ones then walk to the rim without
//! crossing it): a class starts where the area of the classes before it ends
//! ([`outer_radius`]). Whether two classes' slots overlap at a ring's edge, or
//! where blocked points were left out, is decided when the later field is built,
//! which skips every point too near a slot of an earlier one
//! (`path/field_build.rs`).
//!
//! A point is a **slot** for a class when its cell can be entered and the line
//! from the middle of the goal cell to it is clear by the thick line of sight
//! (`path/los.rs`): a unit that has followed the field down to the goal can
//! always walk straight on to its slot. The lattice is the same shape for every
//! class but which points are valid differs (a bridge cell is open ground for a
//! tank and water to a boat), so each field keeps only its own.

use sf_math::{Fx, Vec2};

/// Lattice points on each side of the middle, in each direction: the table is
/// `2 * HALF_WIDTH` points wide and high. Forty spacings reach 14 cells at the
/// spacing of infantry and more than 50 at the spacing of a tank, far enough for
/// the 256 slots a field hands out.
pub(crate) const HALF_WIDTH: i32 = 40;

/// Room left between two bodies at their slots, in cells: a sixteenth, so that a
/// unit standing on its slot is not pushed by its neighbours.
pub(crate) const GAP: Fx = Fx::from_ratio(1, 16);

/// Slots further from the goal than this many cells are not used: the line of
/// sight that checks them is at most one sector long.
pub(crate) const MAX_RADIUS: Fx = Fx::from_int(15);

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

/// The position of a lattice point around the middle of a goal cell, for a
/// lattice of `spacing` cells.
pub(crate) fn position(centre: Vec2, p: Point, spacing: Fx) -> Vec2 {
    // (a + 1/2) spacings: (2a + 1) * spacing / 2 cells.
    let to = |k: i8| spacing.mul_int(2 * i64::from(k) + 1).div_int(2);
    Vec2::new(centre.x + to(p.0), centre.y + to(p.1))
}

/// The squared distance of a point from the middle in units of half a spacing:
/// what the spiral is ordered by.
pub(crate) fn d2(p: Point) -> i32 {
    let (x, y) = (2 * i32::from(p.0) + 1, 2 * i32::from(p.1) + 1);
    x * x + y * y
}

/// The spacing of the slots for bodies of this size: the smallest distance two of
/// them keep (infantry-sized bodies stand closer, `crowd::SOFT_*`) and a
/// [`GAP`]. `max_size` is the biggest diameter in the class's group.
pub(crate) fn spacing_for(max_size: Fx) -> Fx {
    let body = if max_size <= crate::crowd::SOFT_SIZE {
        max_size
            .mul_int(crate::crowd::SOFT_NUM)
            .div_int(crate::crowd::SOFT_DEN)
    } else {
        max_size
    };
    body + GAP
}

/// Where the spiral of a lattice of `spacing` cells reaches `inner` cells from
/// the middle: the index of the first point at least that far.
pub(crate) fn first_beyond(spiral: &[Point], inner: Fx, spacing: Fx) -> usize {
    if inner <= Fx::ZERO || spacing <= Fx::ZERO {
        return 0;
    }
    // Half spacings from the middle.
    let t = inner.mul_int(2).saturating_div(spacing);
    let t2 = t.square();
    spiral.partition_point(|&p| Fx::from_int(d2(p)) < t2)
}

/// The radius at which `members` bodies on a lattice of `spacing` cells end if
/// they start `inner` cells out: the area of the disc grows by one lattice cell
/// for each of them.
pub(crate) fn outer_radius(inner: Fx, members: u32, spacing: Fx) -> Fx {
    let area = spacing.square().mul_int(i64::from(members));
    // Divided by pi (355/113 is exact enough for a radius that is only planned).
    let disc = area.mul_int(113).div_int(355);
    (inner.square() + disc).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spiral_goes_outwards_and_covers_the_table_once() {
        let s = spiral();
        assert_eq!(s.len(), (4 * HALF_WIDTH * HALF_WIDTH) as usize);
        assert!(s.windows(2).all(|w| d2(w[0]) <= d2(w[1])));
        let mut sorted = s.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), s.len(), "no point twice");
        // The first four are the quarter points of the goal cell itself (at a
        // spacing of half a cell).
        let centre = Vec2::from_ints(10, 10);
        for &p in &s[..4] {
            let q = position(centre, p, Fx::HALF);
            assert_eq!((q.x - centre.x).abs(), Fx::from_ratio(1, 4));
            assert_eq!((q.y - centre.y).abs(), Fx::from_ratio(1, 4));
        }
    }

    #[test]
    fn four_points_to_a_cell_at_half_a_cell() {
        let centre = Vec2::new(Fx::from_ratio(21, 2), Fx::from_ratio(21, 2)); // middle of cell (10, 10)
        let s = spiral();
        let mut in_cell = 0;
        for &p in &s {
            let q = position(centre, p, Fx::HALF);
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
    fn neighbouring_points_are_one_spacing_apart() {
        let centre = Vec2::from_ints(30, 30);
        let spacing = Fx::from_ratio(13, 10);
        let a = position(centre, (3, 4), spacing);
        let b = position(centre, (4, 4), spacing);
        let c = position(centre, (3, 5), spacing);
        assert_eq!((b - a).length(), spacing);
        assert_eq!((c - a).length(), spacing);
    }

    #[test]
    fn the_spacing_is_what_the_bodies_keep_and_a_gap() {
        // A tank: its whole diameter. A rifleman: three quarters of it.
        assert_eq!(
            spacing_for(Fx::from_ratio(6, 5)),
            Fx::from_ratio(6, 5) + GAP
        );
        assert_eq!(
            spacing_for(Fx::from_ratio(2, 5)),
            Fx::from_ratio(3, 10) + GAP
        );
    }

    #[test]
    fn a_ring_starts_where_the_inner_radius_is() {
        let s = spiral();
        let spacing = Fx::from_ratio(13, 10);
        assert_eq!(first_beyond(&s, Fx::ZERO, spacing), 0);
        let k = first_beyond(&s, Fx::from_int(5), spacing);
        let near = position(Vec2::ZERO, s[k], spacing).length();
        let before = position(Vec2::ZERO, s[k - 1], spacing).length();
        assert!(near >= Fx::from_int(5) - Fx::from_ratio(1, 100), "{near:?}");
        assert!(before < Fx::from_int(5), "{before:?}");
    }

    #[test]
    fn a_group_of_two_hundred_infantry_fits_inside_five_cells_and_a_hundred_tanks_do_not() {
        let r = outer_radius(Fx::ZERO, 200, spacing_for(Fx::from_ratio(2, 5)));
        assert!(r < Fx::from_int(5), "{r:?}");
        let tanks = outer_radius(Fx::ZERO, 100, spacing_for(Fx::from_ratio(6, 5)));
        assert!(tanks > Fx::from_int(5), "{tanks:?}");
    }

    #[test]
    fn planned_rings_follow_each_other() {
        let spacing = spacing_for(Fx::from_ratio(6, 5));
        let a = outer_radius(Fx::ZERO, 45, spacing);
        let b = outer_radius(a, 60, spacing);
        assert!(a > Fx::from_int(4) && a < Fx::from_int(6), "{a:?}");
        assert!(b > a && b < Fx::from_int(9), "{b:?}");
    }
}
