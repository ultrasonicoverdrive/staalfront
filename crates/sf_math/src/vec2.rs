//! [`Vec2`], a 2D vector of [`Fx`] (positions, velocities, offsets).

use core::fmt;
use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

use crate::facing::Facing;
use crate::fx::{FRAC_BITS, Fx};
use crate::int::{div_round, isqrt_round, sat_i64, sat_round_shr};
use crate::trig::sin_cos_raw;

/// A 2D vector of [`Fx`]. `x` runs along columns and `y` along rows of the
/// cell grid; the unit is one cell.
///
/// All operations are total and saturate like [`Fx`]. Lengths and
/// normalisation are computed from the raw 128-bit components, so they stay
/// exact for any magnitude (no premature overflow of `x*x + y*y`).
///
/// ```
/// use sf_math::{Fx, Vec2};
/// let v = Vec2::from_ints(3, 4);
/// assert_eq!(v.length(), Fx::from_int(5));
/// assert_eq!(v.normalize_or_zero().length(), Fx::ONE);
/// ```
#[derive(Copy, Clone, Default, PartialEq, Eq, Hash)]
pub struct Vec2 {
    /// Horizontal component (cells).
    pub x: Fx,
    /// Vertical component (cells).
    pub y: Fx,
}

/// `x^2 + y^2` of raw components, exact (fits `u128`: each term is at most
/// `2^126`).
#[inline]
const fn norm_sq_raw(x: i64, y: i64) -> u128 {
    let (x, y) = (x.unsigned_abs() as u128, y.unsigned_abs() as u128);
    x * x + y * y
}

impl Vec2 {
    /// The zero vector.
    pub const ZERO: Vec2 = Vec2::new(Fx::ZERO, Fx::ZERO);
    /// `(1, 1)`.
    pub const ONE: Vec2 = Vec2::new(Fx::ONE, Fx::ONE);
    /// `(1, 0)`.
    pub const X: Vec2 = Vec2::new(Fx::ONE, Fx::ZERO);
    /// `(0, 1)`.
    pub const Y: Vec2 = Vec2::new(Fx::ZERO, Fx::ONE);

    /// A vector from its components.
    #[inline]
    pub const fn new(x: Fx, y: Fx) -> Vec2 {
        Vec2 { x, y }
    }

    /// A vector of whole cells.
    #[inline]
    pub const fn from_ints(x: i32, y: i32) -> Vec2 {
        Vec2::new(Fx::from_int(x), Fx::from_int(y))
    }

    /// Both components equal to `v`.
    #[inline]
    pub const fn splat(v: Fx) -> Vec2 {
        Vec2::new(v, v)
    }

    /// `self == (0, 0)`.
    #[inline]
    pub const fn is_zero(self) -> bool {
        self.x.raw() == 0 && self.y.raw() == 0
    }

    /// The vector of length `len` pointing along `facing`.
    #[inline]
    pub const fn from_facing(facing: Facing, len: Fx) -> Vec2 {
        facing.to_vec2().scale(len)
    }

    // ---- component-wise ---------------------------------------------------------

    /// Component-wise sum, saturating.
    #[inline]
    pub const fn saturating_add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x.saturating_add(o.x), self.y.saturating_add(o.y))
    }

    /// Component-wise difference, saturating.
    #[inline]
    pub const fn saturating_sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x.saturating_sub(o.x), self.y.saturating_sub(o.y))
    }

    /// Component-wise negation, saturating.
    #[inline]
    pub const fn saturating_neg(self) -> Vec2 {
        Vec2::new(self.x.saturating_neg(), self.y.saturating_neg())
    }

    /// Both components multiplied by `k`.
    #[inline]
    pub const fn scale(self, k: Fx) -> Vec2 {
        Vec2::new(self.x.saturating_mul(k), self.y.saturating_mul(k))
    }

    /// Both components multiplied by the integer `n` (exact).
    #[inline]
    pub const fn mul_int(self, n: i64) -> Vec2 {
        Vec2::new(self.x.mul_int(n), self.y.mul_int(n))
    }

    /// Both components divided by `k` (rounded to nearest; zero `k` follows
    /// the [`Fx::saturating_div`] rule).
    #[inline]
    pub const fn div_scalar(self, k: Fx) -> Vec2 {
        Vec2::new(self.x.saturating_div(k), self.y.saturating_div(k))
    }

    /// Both components divided by the integer `n`.
    #[inline]
    pub const fn div_int(self, n: i64) -> Vec2 {
        Vec2::new(self.x.div_int(n), self.y.div_int(n))
    }

    /// Component-wise absolute value.
    #[inline]
    pub const fn abs(self) -> Vec2 {
        Vec2::new(self.x.abs(), self.y.abs())
    }

    /// Component-wise minimum.
    #[inline]
    pub const fn min(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x.min(o.x), self.y.min(o.y))
    }

    /// Component-wise maximum.
    #[inline]
    pub const fn max(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x.max(o.x), self.y.max(o.y))
    }

    /// Component-wise clamp (`lo` wins where `lo > hi`, see [`Fx::clamp`]).
    #[inline]
    pub const fn clamp(self, lo: Vec2, hi: Vec2) -> Vec2 {
        Vec2::new(self.x.clamp(lo.x, hi.x), self.y.clamp(lo.y, hi.y))
    }

    /// `self + (to - self) * t` per component (see [`Fx::lerp`]).
    #[inline]
    pub const fn lerp(self, to: Vec2, t: Fx) -> Vec2 {
        Vec2::new(self.x.lerp(to.x, t), self.y.lerp(to.y, t))
    }

    /// The cell `(column, row)` containing this point: component-wise
    /// [`Fx::cell`].
    #[inline]
    pub const fn cell(self) -> (i32, i32) {
        (self.x.cell(), self.y.cell())
    }

    // ---- products ---------------------------------------------------------------

    /// Dot product, with a single rounding step. Saturates.
    #[inline]
    pub const fn dot(self, o: Vec2) -> Fx {
        let a = (self.x.raw() as i128) * (o.x.raw() as i128);
        let b = (self.y.raw() as i128) * (o.y.raw() as i128);
        // Only `MIN*MIN + MIN*MIN` reaches 2^127; saturate instead of wrap.
        let s = a.saturating_add(b);
        Fx::from_raw(sat_round_shr(s, FRAC_BITS))
    }

    /// 2D cross product `x1*y2 - y1*x2` (twice the signed area of the
    /// triangle; positive when `o` is counter-clockwise of `self` in a
    /// `y`-up frame). Single rounding step; saturates.
    #[inline]
    pub const fn cross(self, o: Vec2) -> Fx {
        let a = (self.x.raw() as i128) * (o.y.raw() as i128);
        let b = (self.y.raw() as i128) * (o.x.raw() as i128);
        let d = a.saturating_sub(b);
        Fx::from_raw(sat_round_shr(d, FRAC_BITS))
    }

    /// `self` turned a quarter turn toward `+y`: `(x, y)` becomes `(-y, x)`.
    #[inline]
    pub const fn perp(self) -> Vec2 {
        Vec2::new(self.y.saturating_neg(), self.x)
    }

    // ---- lengths ----------------------------------------------------------------

    /// Squared length, saturating. Prefer this for comparisons: it needs no
    /// square root.
    #[inline]
    pub const fn length_squared(self) -> Fx {
        self.dot(self)
    }

    /// Euclidean length, correctly rounded to the nearest representable
    /// value (error at most half of [`Fx::EPSILON`]); saturates only for
    /// vectors longer than the whole range.
    #[inline]
    pub const fn length(self) -> Fx {
        let l = isqrt_round(norm_sq_raw(self.x.raw(), self.y.raw()));
        if l > i64::MAX as u128 {
            Fx::MAX
        } else {
            Fx::from_raw(l as i64)
        }
    }

    /// Distance to `o`, with the precision of [`Vec2::length`].
    #[inline]
    pub const fn distance(self, o: Vec2) -> Fx {
        self.saturating_sub(o).length()
    }

    /// Squared distance to `o`, saturating.
    #[inline]
    pub const fn distance_squared(self, o: Vec2) -> Fx {
        self.saturating_sub(o).length_squared()
    }

    // ---- normalisation ----------------------------------------------------------

    /// Scales the raw components so the larger one is in `2^62..2^63`
    /// (maximal precision) and returns them with the exact norm of the
    /// scaled vector. `None` for the zero vector.
    #[inline]
    const fn scaled_for_division(self) -> Option<(i128, i128, i128)> {
        let (ax, ay) = (self.x.raw().unsigned_abs(), self.y.raw().unsigned_abs());
        let m = if ax > ay { ax } else { ay };
        if m == 0 {
            return None;
        }
        // Scale so the larger component is in 2^62..2^63 (only the
        // magnitude 2^63 of `i64::MIN` stays put: the shift saturates at 0).
        let lz = m.leading_zeros();
        let shift = if lz > 0 { lz - 1 } else { 0 };
        let x = (self.x.raw() as i128) << shift;
        let y = (self.y.raw() as i128) << shift;
        let n = isqrt_round(norm_sq_raw_wide(x, y)) as i128;
        Some((x, y, n))
    }

    /// The vector scaled to length 1, or the zero vector if `self` is zero.
    /// Each component is correctly rounded, so the result's length differs
    /// from one by at most two raw units, even for tiny inputs.
    #[inline]
    pub const fn normalize_or_zero(self) -> Vec2 {
        match self.scaled_for_division() {
            None => Vec2::ZERO,
            Some((x, y, n)) => Vec2::new(
                Fx::from_raw(sat_i64(div_round(x << FRAC_BITS, n))),
                Fx::from_raw(sat_i64(div_round(y << FRAC_BITS, n))),
            ),
        }
    }

    /// The vector rescaled to exactly `len` (same direction), or the zero
    /// vector if `self` is zero. A negative `len` flips the direction.
    #[inline]
    pub const fn with_length(self, len: Fx) -> Vec2 {
        match self.scaled_for_division() {
            None => Vec2::ZERO,
            Some((x, y, n)) => {
                let l = len.raw() as i128;
                Vec2::new(
                    Fx::from_raw(sat_i64(div_round(x * l, n))),
                    Fx::from_raw(sat_i64(div_round(y * l, n))),
                )
            }
        }
    }

    /// `self` unchanged if it is no longer than `max`, otherwise shortened to
    /// length `max`. A negative `max` is treated as zero.
    #[inline]
    pub const fn clamp_length(self, max: Fx) -> Vec2 {
        let m = if max.raw() < 0 { 0 } else { max.raw() as u128 };
        if norm_sq_raw(self.x.raw(), self.y.raw()) <= m * m {
            self
        } else {
            self.with_length(Fx::from_raw(m as i64))
        }
    }

    // ---- directions -------------------------------------------------------------

    /// The nearest of the 256 facings (see [`Facing::from_xy`]); `None` for
    /// the zero vector.
    #[inline]
    pub const fn facing(self) -> Option<Facing> {
        Facing::from_xy(self.x, self.y)
    }

    /// `self` rotated by `facing` (turning from `+x` toward `+y`), with a
    /// single rounding step per component. The length changes by at most the
    /// table precision (about one part in 30000).
    #[inline]
    pub const fn rotate(self, facing: Facing) -> Vec2 {
        let (s, c) = sin_cos_raw(facing.raw());
        let (x, y) = (self.x.raw() as i128, self.y.raw() as i128);
        let (s, c) = (s as i128, c as i128);
        Vec2::new(
            Fx::from_raw(sat_round_shr(x * c - y * s, FRAC_BITS)),
            Fx::from_raw(sat_round_shr(x * s + y * c, FRAC_BITS)),
        )
    }
}

/// `x^2 + y^2` for already-widened components of magnitude below `2^63`.
#[inline]
const fn norm_sq_raw_wide(x: i128, y: i128) -> u128 {
    let (x, y) = (x.unsigned_abs(), y.unsigned_abs());
    x * x + y * y
}

impl Add for Vec2 {
    type Output = Vec2;
    #[inline]
    fn add(self, o: Vec2) -> Vec2 {
        self.saturating_add(o)
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    #[inline]
    fn sub(self, o: Vec2) -> Vec2 {
        self.saturating_sub(o)
    }
}

impl Neg for Vec2 {
    type Output = Vec2;
    #[inline]
    fn neg(self) -> Vec2 {
        self.saturating_neg()
    }
}

impl Mul<Fx> for Vec2 {
    type Output = Vec2;
    #[inline]
    fn mul(self, k: Fx) -> Vec2 {
        self.scale(k)
    }
}

impl Mul<Vec2> for Fx {
    type Output = Vec2;
    #[inline]
    fn mul(self, v: Vec2) -> Vec2 {
        v.scale(self)
    }
}

impl Div<Fx> for Vec2 {
    type Output = Vec2;
    #[inline]
    fn div(self, k: Fx) -> Vec2 {
        self.div_scalar(k)
    }
}

impl AddAssign for Vec2 {
    #[inline]
    fn add_assign(&mut self, o: Vec2) {
        *self = *self + o;
    }
}

impl SubAssign for Vec2 {
    #[inline]
    fn sub_assign(&mut self, o: Vec2) {
        *self = *self - o;
    }
}

impl MulAssign<Fx> for Vec2 {
    #[inline]
    fn mul_assign(&mut self, k: Fx) {
        *self = *self * k;
    }
}

impl DivAssign<Fx> for Vec2 {
    #[inline]
    fn div_assign(&mut self, k: Fx) {
        *self = *self / k;
    }
}

impl fmt::Debug for Vec2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Vec2({}, {})", self.x, self.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pythagorean_lengths_are_exact() {
        assert_eq!(Vec2::from_ints(3, 4).length(), Fx::from_int(5));
        assert_eq!(Vec2::from_ints(-5, 12).length(), Fx::from_int(13));
        assert_eq!(Vec2::from_ints(0, 0).length(), Fx::ZERO);
        assert_eq!(Vec2::from_ints(3, 4).length_squared(), Fx::from_int(25));
        assert_eq!(
            Vec2::from_ints(1, 1).distance(Vec2::from_ints(4, 5)),
            Fx::from_int(5)
        );
        assert_eq!(
            Vec2::from_ints(1, 1).distance_squared(Vec2::from_ints(4, 5)),
            Fx::from_int(25)
        );
    }

    #[test]
    fn length_does_not_overflow_for_huge_vectors() {
        let big = Vec2::new(Fx::MAX, Fx::MAX);
        // Longer than the range: saturates.
        assert_eq!(big.length(), Fx::MAX);
        // But a vector near the limit on one axis keeps its exact length.
        let v = Vec2::new(Fx::MAX, Fx::ZERO);
        assert_eq!(v.length(), Fx::MAX);
        assert_eq!(Vec2::new(Fx::MIN, Fx::ZERO).length(), Fx::MAX);
    }

    #[test]
    fn normalize_cases() {
        assert_eq!(Vec2::ZERO.normalize_or_zero(), Vec2::ZERO);
        assert_eq!(Vec2::from_ints(7, 0).normalize_or_zero(), Vec2::X);
        assert_eq!(
            Vec2::from_ints(0, -7).normalize_or_zero(),
            Vec2::new(Fx::ZERO, Fx::NEG_ONE)
        );
        // A one-raw-unit vector still gets a sensible direction.
        let tiny = Vec2::new(Fx::EPSILON, Fx::EPSILON).normalize_or_zero();
        assert_eq!(tiny.x, tiny.y);
        let len = tiny.length().raw();
        assert!((len - 65536).abs() <= 2, "len {len}");
        let big = Vec2::new(Fx::MAX, Fx::MAX).normalize_or_zero();
        assert!((big.length().raw() - 65536).abs() <= 2);
    }

    #[test]
    fn with_length_and_clamp_length() {
        let v = Vec2::from_ints(3, 4);
        assert_eq!(v.with_length(Fx::from_int(10)), Vec2::from_ints(6, 8));
        assert_eq!(v.with_length(Fx::ZERO), Vec2::ZERO);
        assert_eq!(v.with_length(Fx::from_int(-5)), Vec2::from_ints(-3, -4));
        assert_eq!(Vec2::ZERO.with_length(Fx::ONE), Vec2::ZERO);
        assert_eq!(v.clamp_length(Fx::from_int(10)), v);
        assert_eq!(v.clamp_length(Fx::from_int(5)), v);
        assert_eq!(v.clamp_length(Fx::from_int(1)).length(), Fx::ONE);
        assert_eq!(v.clamp_length(Fx::from_int(-3)), Vec2::ZERO);
        assert_eq!(
            Vec2::from_ints(6, 8).clamp_length(Fx::from_int(5)),
            Vec2::from_ints(3, 4)
        );
    }

    #[test]
    fn dot_cross_perp() {
        let a = Vec2::from_ints(2, 3);
        let b = Vec2::from_ints(-4, 5);
        assert_eq!(a.dot(b), Fx::from_int(7)); // -8 + 15
        assert_eq!(a.cross(b), Fx::from_int(22)); // 2*5 - 3*(-4)
        assert_eq!(b.cross(a), Fx::from_int(-22));
        assert_eq!(a.perp(), Vec2::from_ints(-3, 2));
        assert_eq!(a.dot(a.perp()), Fx::ZERO);
        // The most extreme dot product saturates instead of overflowing.
        let m = Vec2::new(Fx::MIN, Fx::MIN);
        assert_eq!(m.dot(m), Fx::MAX);
    }

    #[test]
    fn rotate_quarter_turns_are_exact() {
        let v = Vec2::from_ints(3, 4);
        assert_eq!(v.rotate(Facing::PLUS_X), v);
        assert_eq!(v.rotate(Facing::PLUS_Y), Vec2::from_ints(-4, 3));
        assert_eq!(v.rotate(Facing::MINUS_X), Vec2::from_ints(-3, -4));
        assert_eq!(v.rotate(Facing::MINUS_Y), Vec2::from_ints(4, -3));
        assert_eq!(
            Vec2::from_facing(Facing::PLUS_Y, Fx::from_int(7)),
            Vec2::from_ints(0, 7)
        );
    }

    #[test]
    fn operators_and_cells() {
        let a = Vec2::from_ints(1, 2);
        let b = Vec2::from_ints(10, 20);
        assert_eq!(a + b, Vec2::from_ints(11, 22));
        assert_eq!(b - a, Vec2::from_ints(9, 18));
        assert_eq!(-a, Vec2::from_ints(-1, -2));
        assert_eq!(a * Fx::from_int(3), Vec2::from_ints(3, 6));
        assert_eq!(Fx::from_int(3) * a, Vec2::from_ints(3, 6));
        assert_eq!(b / Fx::from_int(2), Vec2::from_ints(5, 10));
        assert_eq!(
            a.lerp(b, Fx::HALF),
            Vec2::new(Fx::from_ratio(11, 2), Fx::from_int(11))
        );
        let p = Vec2::new(Fx::from_ratio(37, 4), Fx::from_ratio(-1, 4));
        assert_eq!(p.cell(), (9, -1));
        let mut c = a;
        c += b;
        c -= a;
        c *= Fx::from_int(2);
        c /= Fx::from_int(4);
        assert_eq!(c, Vec2::from_ints(5, 10));
    }
}
