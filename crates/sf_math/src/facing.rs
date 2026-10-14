//! [`Facing`]: one of 256 directions, with exact trig tables.

use crate::fx::Fx;
use crate::trig::{SIN_QUADRANT, sin_cos_raw};
use crate::vec2::Vec2;

/// A direction out of 256, as a full turn split into `u8` steps.
///
/// Facing `0` points along `+x`; increasing facings turn from `+x` toward
/// `+y` (the mathematical convention `atan2(y, x)`; whether that looks
/// clockwise or counter-clockwise on screen depends on how `y` is drawn).
/// One step is 1/256 of a turn. Because the value is a `u8`, turning wraps
/// naturally: `Facing::new(250).turn(10) == Facing::new(4)`.
///
/// There is deliberately no `Ord`: directions are circular, so `a < b`
/// means nothing.
///
/// ```
/// use sf_math::{Facing, Fx, Vec2};
/// assert_eq!(Facing::PLUS_Y.to_vec2(), Vec2::new(Fx::ZERO, Fx::ONE));
/// let v = Vec2::from_ints(3, 3);
/// assert_eq!(Facing::from_vec2(v), Some(Facing::new(32))); // 45 degrees
/// ```
#[derive(Copy, Clone, Default, PartialEq, Eq, Hash, Debug)]
#[repr(transparent)]
pub struct Facing(u8);

impl Facing {
    /// Number of distinct facings.
    pub const COUNT: u32 = 256;
    /// Along `+x` (a full turn is 256 steps).
    pub const PLUS_X: Facing = Facing(0);
    /// Along `+y`, a quarter turn from `+x`.
    pub const PLUS_Y: Facing = Facing(64);
    /// Along `-x`, a half turn from `+x`.
    pub const MINUS_X: Facing = Facing(128);
    /// Along `-y`, three quarter turns from `+x`.
    pub const MINUS_Y: Facing = Facing(192);

    /// A facing from its step number.
    #[inline]
    pub const fn new(steps: u8) -> Facing {
        Facing(steps)
    }

    /// The step number, `0..=255`.
    #[inline]
    pub const fn raw(self) -> u8 {
        self.0
    }

    /// Turns by `steps` (positive: toward `+y`), wrapping around.
    #[inline]
    pub const fn turn(self, steps: i8) -> Facing {
        Facing(self.0.wrapping_add(steps as u8))
    }

    /// The opposite direction.
    #[inline]
    pub const fn opposite(self) -> Facing {
        Facing(self.0.wrapping_add(128))
    }

    /// The signed shortest turn from `self` to `target`, in `-128..=127`.
    /// A half turn is reported as `-128`, so turning "the short way" toward
    /// an exactly opposite direction goes in the decreasing direction.
    #[inline]
    pub const fn signed_diff(self, target: Facing) -> i8 {
        target.0.wrapping_sub(self.0) as i8
    }

    /// The size of the shortest turn between two facings, `0..=128`.
    #[inline]
    pub const fn abs_diff(self, other: Facing) -> u8 {
        let d = self.signed_diff(other);
        if d == i8::MIN { 128 } else { d.unsigned_abs() }
    }

    /// Turns toward `target` by at most `max_step` steps, the short way
    /// round, never overshooting. Reaches `target` exactly when it is within
    /// `max_step`; `max_step == 0` leaves the facing unchanged.
    #[inline]
    pub const fn turn_toward(self, target: Facing, max_step: u8) -> Facing {
        let d = self.signed_diff(target) as i16;
        let m = max_step as i16;
        let step = if d > m {
            m
        } else if d < -m {
            -m
        } else {
            d
        };
        Facing(self.0.wrapping_add(step as u8))
    }

    /// `sin` of this facing as an [`Fx`] (the `y` of the unit vector).
    #[inline]
    pub const fn sin(self) -> Fx {
        Fx::from_raw(sin_cos_raw(self.0).0)
    }

    /// `cos` of this facing as an [`Fx`] (the `x` of the unit vector).
    #[inline]
    pub const fn cos(self) -> Fx {
        Fx::from_raw(sin_cos_raw(self.0).1)
    }

    /// `(sin, cos)` in one lookup.
    #[inline]
    pub const fn sin_cos(self) -> (Fx, Fx) {
        let (s, c) = sin_cos_raw(self.0);
        (Fx::from_raw(s), Fx::from_raw(c))
    }

    /// The unit vector `(cos, sin)` for this facing. Components are exact
    /// table entries (error at most half of [`Fx::EPSILON`]), so the length
    /// is one to within a few raw units.
    #[inline]
    pub const fn to_vec2(self) -> Vec2 {
        let (s, c) = sin_cos_raw(self.0);
        Vec2::new(Fx::from_raw(c), Fx::from_raw(s))
    }

    /// The facing closest to the direction of `(x, y)`: a deterministic,
    /// integer-only replacement for `atan2`. Only the direction matters, not
    /// the length. Returns `None` for the zero vector.
    ///
    /// The result is the nearest of the 256 table directions. Halfway
    /// directions (to within the table precision) round toward the larger
    /// facing; since the boundaries are fixed integer comparisons the result
    /// is the same everywhere. Scaling `(x, y)` by a positive integer never
    /// changes the answer, and `from_vec2(f.to_vec2()) == Some(f)` for every
    /// facing.
    pub const fn from_xy(x: Fx, y: Fx) -> Option<Facing> {
        let (x, y) = (x.raw() as i128, y.raw() as i128);
        if x == 0 && y == 0 {
            return None;
        }
        // Rotate the vector by whole quarter turns into the first quadrant
        // (qx > 0, qy >= 0) and remember how many.
        let (quadrant, qx, qy) = if x > 0 && y >= 0 {
            (0u8, x, y)
        } else if x <= 0 && y > 0 {
            (1, y, -x)
        } else if x < 0 && y <= 0 {
            (2, -x, -y)
        } else {
            (3, -y, x)
        };
        // Count the bisectors the vector has passed. The bisector between
        // facings j-1 and j is the sum of their unit vectors; the vector is
        // at or past it when cross(bisector, v) is not negative. Both lie
        // within a quarter turn of each other, so the sign of the cross
        // product is reliable. `passed(j)` is monotone in j: binary search.
        let mut lo: usize = 0; // bisectors 1..=lo are passed
        let mut hi: usize = 64;
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            // unit vector of step j: (cos, sin) = (S[64 - j], S[j])
            let bx = SIN_QUADRANT[64 - (mid - 1)] as i128 + SIN_QUADRANT[64 - mid] as i128;
            let by = SIN_QUADRANT[mid - 1] as i128 + SIN_QUADRANT[mid] as i128;
            if bx * qy - by * qx >= 0 {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        Some(Facing((quadrant << 6).wrapping_add(lo as u8)))
    }

    /// [`Facing::from_xy`] for a [`Vec2`].
    #[inline]
    pub const fn from_vec2(v: Vec2) -> Option<Facing> {
        Facing::from_xy(v.x, v.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes() {
        assert_eq!(Facing::PLUS_X.to_vec2(), Vec2::new(Fx::ONE, Fx::ZERO));
        assert_eq!(Facing::PLUS_Y.to_vec2(), Vec2::new(Fx::ZERO, Fx::ONE));
        assert_eq!(Facing::MINUS_X.to_vec2(), Vec2::new(Fx::NEG_ONE, Fx::ZERO));
        assert_eq!(Facing::MINUS_Y.to_vec2(), Vec2::new(Fx::ZERO, Fx::NEG_ONE));
    }

    #[test]
    fn round_trip_every_facing() {
        for f in 0..=255u8 {
            let face = Facing::new(f);
            assert_eq!(Facing::from_vec2(face.to_vec2()), Some(face), "facing {f}");
        }
    }

    #[test]
    fn from_xy_axes_and_diagonals() {
        let one = Fx::ONE;
        let zero = Fx::ZERO;
        assert_eq!(Facing::from_xy(zero, zero), None);
        assert_eq!(Facing::from_xy(one, zero), Some(Facing::PLUS_X));
        assert_eq!(Facing::from_xy(zero, one), Some(Facing::PLUS_Y));
        assert_eq!(Facing::from_xy(-one, zero), Some(Facing::MINUS_X));
        assert_eq!(Facing::from_xy(zero, -one), Some(Facing::MINUS_Y));
        assert_eq!(Facing::from_xy(one, one), Some(Facing::new(32)));
        assert_eq!(Facing::from_xy(-one, one), Some(Facing::new(96)));
        assert_eq!(Facing::from_xy(-one, -one), Some(Facing::new(160)));
        assert_eq!(Facing::from_xy(one, -one), Some(Facing::new(224)));
        // Smallest possible vectors still have a direction.
        let e = Fx::EPSILON;
        assert_eq!(Facing::from_xy(e, zero), Some(Facing::PLUS_X));
        assert_eq!(Facing::from_xy(zero, -e), Some(Facing::MINUS_Y));
        // Extreme magnitudes do not overflow.
        assert_eq!(Facing::from_xy(Fx::MIN, Fx::MIN), Some(Facing::new(160)));
        assert_eq!(Facing::from_xy(Fx::MAX, Fx::MIN), Some(Facing::new(224)));
        assert_eq!(Facing::from_xy(Fx::MIN, Fx::MAX), Some(Facing::new(96)));
    }

    #[test]
    fn turning_wraps_and_stays_short() {
        let a = Facing::new(250);
        assert_eq!(a.turn(10), Facing::new(4));
        assert_eq!(a.turn(-10), Facing::new(240));
        assert_eq!(a.opposite(), Facing::new(122));
        assert_eq!(a.signed_diff(Facing::new(4)), 10);
        assert_eq!(Facing::new(4).signed_diff(a), -10);
        assert_eq!(Facing::new(0).signed_diff(Facing::new(128)), -128);
        assert_eq!(Facing::new(0).abs_diff(Facing::new(128)), 128);
        assert_eq!(a.abs_diff(Facing::new(4)), 10);
    }

    #[test]
    fn turn_toward_rules() {
        let a = Facing::new(250);
        let t = Facing::new(4); // 10 steps ahead, across the wrap
        assert_eq!(a.turn_toward(t, 3), Facing::new(253));
        assert_eq!(a.turn_toward(t, 10), t);
        assert_eq!(a.turn_toward(t, 200), t); // never overshoots
        assert_eq!(a.turn_toward(t, 0), a);
        assert_eq!(t.turn_toward(a, 3), Facing::new(1)); // the other way
        // Exactly opposite: the decreasing direction.
        assert_eq!(
            Facing::new(0).turn_toward(Facing::new(128), 5),
            Facing::new(251)
        );
    }
}
