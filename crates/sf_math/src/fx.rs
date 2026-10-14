//! [`Fx`], the fixed-point scalar the whole simulation computes with.

use core::fmt;
use core::iter::Sum;
use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

use crate::int::{div_round, isqrt_round, round_shr, sat_i64, sat_round_shr};

/// Number of fractional bits of [`Fx`] (`Q47.16`).
pub const FRAC_BITS: u32 = 16;

/// Leptons per cell: the sub-cell unit of the PLAN.md Coordinates section.
/// One lepton is 1/256 of a cell, i.e. exactly 256 raw units of [`Fx`].
pub const LEPTONS_PER_CELL: i64 = 256;

const ONE_RAW: i64 = 1 << FRAC_BITS;
const HALF_RAW: i64 = ONE_RAW >> 1;
const FRAC_MASK: i64 = ONE_RAW - 1;
/// Raw units per lepton (`2^16 / 256`), as a shift.
const LEPTON_SHIFT: u32 = FRAC_BITS - 8;

/// A signed binary fixed-point number: an `i64` holding `value * 2^16`.
///
/// The unit is **one cell**, so `Fx::ONE` is the width of a map cell and the
/// smallest step ([`Fx::EPSILON`]) is 1/65536 cell, 1/256 of a lepton. See the
/// [crate docs](crate) for range, precision and the overflow policy; the short
/// version is that every operation is *total* (no panic, no wrap, the same in
/// debug and release) and saturates at [`Fx::MIN`] / [`Fx::MAX`].
///
/// ```
/// use sf_math::Fx;
/// let third = Fx::from_ratio(1, 3);
/// assert_eq!((third + third + third).raw(), 65535); // 3 * 21845
/// assert_eq!(Fx::from_int(7) / Fx::from_int(2), Fx::from_ratio(7, 2));
/// ```
#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Fx(i64);

impl Fx {
    /// Zero.
    pub const ZERO: Fx = Fx(0);
    /// One cell.
    pub const ONE: Fx = Fx(ONE_RAW);
    /// Minus one cell.
    pub const NEG_ONE: Fx = Fx(-ONE_RAW);
    /// Half a cell.
    pub const HALF: Fx = Fx(HALF_RAW);
    /// One lepton (1/256 cell).
    pub const LEPTON: Fx = Fx(1 << LEPTON_SHIFT);
    /// The smallest positive value, 1/65536 cell.
    pub const EPSILON: Fx = Fx(1);
    /// The largest value, just below 2^47 cells.
    pub const MAX: Fx = Fx(i64::MAX);
    /// The smallest value, exactly minus 2^47 cells.
    pub const MIN: Fx = Fx(i64::MIN);

    // ---- construction and conversion -------------------------------------

    /// Wraps a raw value (`value * 2^16`).
    #[inline]
    pub const fn from_raw(raw: i64) -> Fx {
        Fx(raw)
    }

    /// The raw value (`value * 2^16`).
    #[inline]
    pub const fn raw(self) -> i64 {
        self.0
    }

    /// An exact whole number of cells.
    #[inline]
    pub const fn from_int(n: i32) -> Fx {
        Fx((n as i64) << FRAC_BITS)
    }

    /// A whole number of cells, saturating if it is out of range.
    #[inline]
    pub const fn from_i64(n: i64) -> Fx {
        Fx(sat_i64((n as i128) << FRAC_BITS))
    }

    /// `num / den` rounded to nearest (ties away from zero): a constant
    /// without floats, e.g. `Fx::from_ratio(3, 2)` for one and a half.
    /// A zero `den` follows the [`Fx::saturating_div`] rule.
    #[inline]
    pub const fn from_ratio(num: i64, den: i64) -> Fx {
        Fx::div_wide((num as i128) << FRAC_BITS, den as i128)
    }

    /// A whole number of leptons (1/256 cell), saturating.
    #[inline]
    pub const fn from_leptons(leptons: i64) -> Fx {
        Fx(sat_i64((leptons as i128) << LEPTON_SHIFT))
    }

    /// `n / d` with ties away from zero and the zero-divisor rule; the
    /// numerator is a pre-scaled wide value.
    #[inline]
    const fn div_wide(n: i128, d: i128) -> Fx {
        if d == 0 {
            return Fx::div_by_zero(n > 0, n < 0);
        }
        Fx(sat_i64(div_round(n, d)))
    }

    /// The result of dividing by zero: `MAX` for a positive dividend, `MIN`
    /// for a negative one, zero for zero.
    #[inline]
    const fn div_by_zero(positive: bool, negative: bool) -> Fx {
        if positive {
            Fx::MAX
        } else if negative {
            Fx::MIN
        } else {
            Fx::ZERO
        }
    }

    // ---- predicates -------------------------------------------------------

    /// `self == 0`.
    #[inline]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// `self < 0`.
    #[inline]
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// `self > 0`.
    #[inline]
    pub const fn is_positive(self) -> bool {
        self.0 > 0
    }

    /// `-1`, `0` or `1` as an `Fx`.
    #[inline]
    pub const fn signum(self) -> Fx {
        if self.0 > 0 {
            Fx::ONE
        } else if self.0 < 0 {
            Fx::NEG_ONE
        } else {
            Fx::ZERO
        }
    }

    // ---- saturating arithmetic (what the operators do) ---------------------

    /// `self + rhs`, saturating at the range limits.
    #[inline]
    pub const fn saturating_add(self, rhs: Fx) -> Fx {
        Fx(self.0.saturating_add(rhs.0))
    }

    /// `self - rhs`, saturating at the range limits.
    #[inline]
    pub const fn saturating_sub(self, rhs: Fx) -> Fx {
        Fx(self.0.saturating_sub(rhs.0))
    }

    /// `self * rhs` rounded to nearest (ties away from zero), saturating.
    /// The intermediate product is exact (128 bits), so there is a single
    /// rounding step.
    #[inline]
    pub const fn saturating_mul(self, rhs: Fx) -> Fx {
        let p = (self.0 as i128) * (rhs.0 as i128);
        Fx(sat_round_shr(p, FRAC_BITS))
    }

    /// `self / rhs` rounded to nearest (ties away from zero), saturating.
    ///
    /// Division by zero does **not** panic: it yields [`Fx::MAX`] for a
    /// positive dividend, [`Fx::MIN`] for a negative one and zero for
    /// `0 / 0`. Use [`Fx::checked_div`] to detect it.
    #[inline]
    pub const fn saturating_div(self, rhs: Fx) -> Fx {
        Fx::div_wide((self.0 as i128) << FRAC_BITS, rhs.0 as i128)
    }

    /// `-self`, saturating (`-MIN == MAX`).
    #[inline]
    pub const fn saturating_neg(self) -> Fx {
        Fx(self.0.saturating_neg())
    }

    /// `|self|`, saturating (`|MIN| == MAX`).
    #[inline]
    pub const fn abs(self) -> Fx {
        Fx(self.0.saturating_abs())
    }

    /// `self * n` for a plain integer `n`, exact, saturating.
    #[inline]
    pub const fn mul_int(self, n: i64) -> Fx {
        Fx(sat_i64((self.0 as i128) * (n as i128)))
    }

    /// `self / n` for a plain integer `n`, rounded to nearest, with the
    /// zero-divisor rule of [`Fx::saturating_div`].
    #[inline]
    pub const fn div_int(self, n: i64) -> Fx {
        Fx::div_wide(self.0 as i128, n as i128)
    }

    /// `self * num / den` with one rounding step and an exact 128-bit
    /// intermediate: the right way to scale by a ratio (percentages, speed
    /// factors) without losing precision to an intermediate rounding. Zero
    /// `den` follows the zero-divisor rule.
    #[inline]
    pub const fn mul_div(self, num: Fx, den: Fx) -> Fx {
        let p = (self.0 as i128) * (num.0 as i128);
        Fx::div_wide(p, den.0 as i128)
    }

    /// `self * self`, saturating.
    #[inline]
    pub const fn square(self) -> Fx {
        self.saturating_mul(self)
    }

    // ---- checked arithmetic -------------------------------------------------

    /// `self + rhs`, or `None` if it does not fit.
    #[inline]
    pub const fn checked_add(self, rhs: Fx) -> Option<Fx> {
        match self.0.checked_add(rhs.0) {
            Some(v) => Some(Fx(v)),
            None => None,
        }
    }

    /// `self - rhs`, or `None` if it does not fit.
    #[inline]
    pub const fn checked_sub(self, rhs: Fx) -> Option<Fx> {
        match self.0.checked_sub(rhs.0) {
            Some(v) => Some(Fx(v)),
            None => None,
        }
    }

    /// `self * rhs` (same rounding as the operator), or `None` if it does
    /// not fit.
    #[inline]
    pub const fn checked_mul(self, rhs: Fx) -> Option<Fx> {
        let v = round_shr((self.0 as i128) * (rhs.0 as i128), FRAC_BITS);
        if v > i64::MAX as i128 || v < i64::MIN as i128 {
            None
        } else {
            Some(Fx(v as i64))
        }
    }

    /// `self / rhs` (same rounding as the operator), or `None` if `rhs` is
    /// zero or the quotient does not fit.
    #[inline]
    pub const fn checked_div(self, rhs: Fx) -> Option<Fx> {
        if rhs.0 == 0 {
            return None;
        }
        let v = div_round((self.0 as i128) << FRAC_BITS, rhs.0 as i128);
        if v > i64::MAX as i128 || v < i64::MIN as i128 {
            None
        } else {
            Some(Fx(v as i64))
        }
    }

    /// `-self`, or `None` for [`Fx::MIN`].
    #[inline]
    pub const fn checked_neg(self) -> Option<Fx> {
        match self.0.checked_neg() {
            Some(v) => Some(Fx(v)),
            None => None,
        }
    }

    // ---- ordering helpers -----------------------------------------------------

    /// The smaller of two values.
    #[inline]
    pub const fn min(self, other: Fx) -> Fx {
        if self.0 <= other.0 { self } else { other }
    }

    /// The larger of two values.
    #[inline]
    pub const fn max(self, other: Fx) -> Fx {
        if self.0 >= other.0 { self } else { other }
    }

    /// Limits `self` to `lo..=hi`. Total: if `lo > hi` the result is `lo`
    /// (unlike [`Ord::clamp`], which panics).
    #[inline]
    pub const fn clamp(self, lo: Fx, hi: Fx) -> Fx {
        self.min(hi).max(lo)
    }

    /// `self + (to - self) * t`, with one rounding step. `t = 0` gives
    /// `self`, `t = 1` gives `to`; `t` outside `0..=1` extrapolates
    /// (saturating).
    #[inline]
    pub const fn lerp(self, to: Fx, t: Fx) -> Fx {
        let d = (to.0 as i128) - (self.0 as i128); // |d| < 2^64
        let step = round_shr(d * (t.0 as i128), FRAC_BITS); // |d * t| < 2^127
        Fx(sat_i64((self.0 as i128) + step))
    }

    // ---- rounding to whole cells ------------------------------------------------

    /// Rounds toward negative infinity to a whole number of cells.
    #[inline]
    pub const fn floor(self) -> Fx {
        Fx(self.0 & !FRAC_MASK)
    }

    /// Rounds toward positive infinity (saturating at [`Fx::MAX`]).
    #[inline]
    pub const fn ceil(self) -> Fx {
        let f = self.floor();
        if f.0 == self.0 {
            self
        } else {
            f.saturating_add(Fx::ONE)
        }
    }

    /// Rounds to nearest, halves toward positive infinity (saturating).
    #[inline]
    pub const fn round(self) -> Fx {
        let f = self.floor();
        if self.0 - f.0 >= HALF_RAW {
            f.saturating_add(Fx::ONE)
        } else {
            f
        }
    }

    /// Rounds toward zero.
    #[inline]
    pub const fn trunc(self) -> Fx {
        if self.0 >= 0 {
            self.floor()
        } else {
            self.ceil()
        }
    }

    /// `self - self.floor()`, always in `0..1`.
    #[inline]
    pub const fn fract(self) -> Fx {
        Fx(self.0 & FRAC_MASK)
    }

    /// Floor as an integer: the cell index of a coordinate.
    #[inline]
    pub const fn floor_int(self) -> i64 {
        self.0 >> FRAC_BITS
    }

    /// Ceiling as an integer.
    #[inline]
    pub const fn ceil_int(self) -> i64 {
        (self.0 >> FRAC_BITS) + (((self.0 & FRAC_MASK) != 0) as i64)
    }

    /// Nearest integer, halves toward positive infinity.
    #[inline]
    pub const fn round_int(self) -> i64 {
        (self.0 >> FRAC_BITS) + (((self.0 & FRAC_MASK) >= HALF_RAW) as i64)
    }

    /// Integer part, rounded toward zero.
    #[inline]
    pub const fn trunc_int(self) -> i64 {
        if self.0 >= 0 {
            self.floor_int()
        } else {
            self.ceil_int()
        }
    }

    // ---- cells and leptons -----------------------------------------------------

    /// The cell a coordinate lies in: `floor`, as `i32` (saturating; map
    /// coordinates are at most a few hundred).
    #[inline]
    pub const fn cell(self) -> i32 {
        let c = self.0 >> FRAC_BITS;
        if c > i32::MAX as i64 {
            i32::MAX
        } else if c < i32::MIN as i64 {
            i32::MIN
        } else {
            c as i32
        }
    }

    /// Whole leptons, rounded toward negative infinity.
    #[inline]
    pub const fn to_leptons(self) -> i64 {
        self.0 >> LEPTON_SHIFT
    }

    /// Which lepton inside its cell a coordinate lies in, `0..=255`.
    /// Together with [`Fx::cell`]: `cell * 256 + lepton_in_cell` equals
    /// [`Fx::to_leptons`].
    #[inline]
    pub const fn lepton_in_cell(self) -> u8 {
        ((self.0 >> LEPTON_SHIFT) & 0xFF) as u8
    }

    // ---- square root -------------------------------------------------------------

    /// Square root, correctly rounded: the result is the representable value
    /// nearest to the true root (error at most half of [`Fx::EPSILON`]).
    /// Integer only, bit-identical on every platform. Negative input gives
    /// zero.
    #[inline]
    pub const fn sqrt(self) -> Fx {
        if self.0 <= 0 {
            return Fx::ZERO;
        }
        // sqrt(raw / 2^16) * 2^16 = sqrt(raw * 2^16)
        let n = (self.0 as u128) << FRAC_BITS;
        Fx(isqrt_round(n) as i64)
    }
}

// ---- operators ---------------------------------------------------------------------

impl Add for Fx {
    type Output = Fx;
    #[inline]
    fn add(self, rhs: Fx) -> Fx {
        self.saturating_add(rhs)
    }
}

impl Sub for Fx {
    type Output = Fx;
    #[inline]
    fn sub(self, rhs: Fx) -> Fx {
        self.saturating_sub(rhs)
    }
}

impl Mul for Fx {
    type Output = Fx;
    #[inline]
    fn mul(self, rhs: Fx) -> Fx {
        self.saturating_mul(rhs)
    }
}

impl Div for Fx {
    type Output = Fx;
    #[inline]
    fn div(self, rhs: Fx) -> Fx {
        self.saturating_div(rhs)
    }
}

impl Neg for Fx {
    type Output = Fx;
    #[inline]
    fn neg(self) -> Fx {
        self.saturating_neg()
    }
}

impl AddAssign for Fx {
    #[inline]
    fn add_assign(&mut self, rhs: Fx) {
        *self = *self + rhs;
    }
}

impl SubAssign for Fx {
    #[inline]
    fn sub_assign(&mut self, rhs: Fx) {
        *self = *self - rhs;
    }
}

impl MulAssign for Fx {
    #[inline]
    fn mul_assign(&mut self, rhs: Fx) {
        *self = *self * rhs;
    }
}

impl DivAssign for Fx {
    #[inline]
    fn div_assign(&mut self, rhs: Fx) {
        *self = *self / rhs;
    }
}

impl Sum for Fx {
    fn sum<I: Iterator<Item = Fx>>(iter: I) -> Fx {
        iter.fold(Fx::ZERO, |a, b| a + b)
    }
}

impl<'a> Sum<&'a Fx> for Fx {
    fn sum<I: Iterator<Item = &'a Fx>>(iter: I) -> Fx {
        iter.fold(Fx::ZERO, |a, b| a + *b)
    }
}

impl From<i32> for Fx {
    #[inline]
    fn from(n: i32) -> Fx {
        Fx::from_int(n)
    }
}

/// Prints the exact decimal expansion: a binary fraction with 16 bits always
/// terminates, so there is no rounding and no trailing zero (the integration
/// test `fx_display` shows examples).
impl fmt::Display for Fx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mag = self.0.unsigned_abs();
        let mut frac = mag & (FRAC_MASK as u64);
        if self.0 < 0 {
            f.write_str("-")?;
        }
        write!(f, "{}", mag >> FRAC_BITS)?;
        if frac != 0 {
            f.write_str(".")?;
            while frac != 0 {
                frac *= 10;
                write!(f, "{}", frac >> FRAC_BITS)?;
                frac &= FRAC_MASK as u64;
            }
        }
        Ok(())
    }
}

impl fmt::Debug for Fx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fx({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants() {
        assert_eq!(Fx::ONE.raw(), 65536);
        assert_eq!(Fx::HALF.raw(), 32768);
        assert_eq!(Fx::LEPTON.raw(), 256);
        assert_eq!(Fx::LEPTON.mul_int(LEPTONS_PER_CELL), Fx::ONE);
        assert_eq!(Fx::EPSILON.raw(), 1);
        assert_eq!(Fx::MAX.raw(), i64::MAX);
        assert_eq!(Fx::MIN.raw(), i64::MIN);
    }

    #[test]
    fn constants_from_ratio_are_const_and_rounded() {
        const THIRD: Fx = Fx::from_ratio(1, 3);
        const TWO_THIRDS: Fx = Fx::from_ratio(2, 3);
        const NEG_THIRD: Fx = Fx::from_ratio(-1, 3);
        assert_eq!(THIRD.raw(), 21845); // 65536 / 3 = 21845 and 1/3
        assert_eq!(TWO_THIRDS.raw(), 43691); // 43690 and 2/3, rounded up
        assert_eq!(NEG_THIRD.raw(), -21845);
        assert_eq!(Fx::from_ratio(3, 2).raw(), 98304);
        assert_eq!(Fx::from_ratio(5, 1), Fx::from_int(5));
        assert_eq!(Fx::from_ratio(5, -2), Fx::from_ratio(-5, 2));
        assert_eq!(Fx::from_ratio(1, 0), Fx::MAX);
        assert_eq!(Fx::from_ratio(-1, 0), Fx::MIN);
        assert_eq!(Fx::from_ratio(0, 0), Fx::ZERO);
    }

    #[test]
    fn mul_div_rounding() {
        let a = Fx::from_raw(3);
        let half = Fx::HALF;
        // 3 * 1/2 is three halves of a raw unit: a tie, away from zero.
        assert_eq!((a * half).raw(), 2);
        assert_eq!(((-a) * half).raw(), -2);
        assert_eq!((Fx::from_raw(1) * half).raw(), 1);
        assert_eq!((Fx::from_raw(1) * Fx::from_raw(1)).raw(), 0);
        assert_eq!(Fx::from_int(7) / Fx::from_int(2), Fx::from_ratio(7, 2));
        assert_eq!(Fx::from_int(-7) / Fx::from_int(2), Fx::from_ratio(-7, 2));
        assert_eq!(
            Fx::ONE.mul_div(Fx::from_int(3), Fx::from_int(4)).raw(),
            49152
        );
    }

    #[test]
    fn saturation_is_explicit_and_total() {
        assert_eq!(Fx::MAX + Fx::ONE, Fx::MAX);
        assert_eq!(Fx::MIN - Fx::ONE, Fx::MIN);
        assert_eq!(Fx::MAX * Fx::MAX, Fx::MAX);
        assert_eq!(Fx::MIN * Fx::MAX, Fx::MIN);
        assert_eq!(Fx::MIN * Fx::MIN, Fx::MAX);
        assert_eq!(-Fx::MIN, Fx::MAX);
        assert_eq!(Fx::MIN.abs(), Fx::MAX);
        assert_eq!(Fx::MAX / Fx::EPSILON, Fx::MAX);
        assert_eq!(Fx::ONE / Fx::ZERO, Fx::MAX);
        assert_eq!(Fx::NEG_ONE / Fx::ZERO, Fx::MIN);
        assert_eq!(Fx::ZERO / Fx::ZERO, Fx::ZERO);
        assert_eq!(Fx::MAX.checked_add(Fx::EPSILON), None);
        assert_eq!(Fx::MAX.checked_mul(Fx::from_int(2)), None);
        assert_eq!(Fx::ONE.checked_div(Fx::ZERO), None);
        assert_eq!(Fx::MIN.checked_neg(), None);
        assert_eq!(Fx::ONE.checked_div(Fx::from_int(2)), Some(Fx::HALF));
        assert_eq!(Fx::from_i64(i64::MAX), Fx::MAX);
        assert_eq!(Fx::from_i64(i64::MIN), Fx::MIN);
        assert_eq!(Fx::from_leptons(i64::MAX), Fx::MAX);
    }

    #[test]
    fn floor_ceil_round_trunc() {
        let x = Fx::from_ratio(-5, 2); // minus two and a half
        assert_eq!(x.floor(), Fx::from_int(-3));
        assert_eq!(x.ceil(), Fx::from_int(-2));
        assert_eq!(x.round(), Fx::from_int(-2)); // halves toward +inf
        assert_eq!(x.trunc(), Fx::from_int(-2));
        assert_eq!((-x).round(), Fx::from_int(3));
        assert_eq!(x.floor_int(), -3);
        assert_eq!(x.ceil_int(), -2);
        assert_eq!(x.round_int(), -2);
        assert_eq!(x.trunc_int(), -2);
        assert_eq!(x.fract(), Fx::HALF);
        assert_eq!(Fx::from_int(4).ceil(), Fx::from_int(4));
        assert_eq!(Fx::MAX.ceil(), Fx::MAX);
        assert_eq!(Fx::MAX.round(), Fx::MAX);
    }

    #[test]
    fn cells_and_leptons() {
        // Cell 3, lepton 200 (of 256 per cell)
        let p = Fx::from_int(3) + Fx::from_leptons(200) + Fx::from_raw(7);
        assert_eq!(p.cell(), 3);
        assert_eq!(p.lepton_in_cell(), 200);
        assert_eq!(p.to_leptons(), 3 * 256 + 200);
        // Negative coordinates floor consistently.
        let n = Fx::from_leptons(-1);
        assert_eq!(n.cell(), -1);
        assert_eq!(n.lepton_in_cell(), 255);
        assert_eq!(n.to_leptons(), -1);
        assert_eq!(Fx::MAX.cell(), i32::MAX);
        assert_eq!(Fx::MIN.cell(), i32::MIN);
    }

    #[test]
    fn sqrt_known_values() {
        assert_eq!(Fx::ZERO.sqrt(), Fx::ZERO);
        assert_eq!(Fx::from_int(4).sqrt(), Fx::from_int(2));
        assert_eq!(Fx::from_int(256).sqrt(), Fx::from_int(16));
        assert_eq!(Fx::from_ratio(1, 4).sqrt(), Fx::HALF);
        assert_eq!(Fx::from_int(2).sqrt().raw(), 92682);
        assert_eq!(Fx::from_int(-4).sqrt(), Fx::ZERO);
        assert_eq!(Fx::EPSILON.sqrt().raw(), 256); // sqrt(2^-16) = 2^-8
        // sqrt(MAX) is more than 2^23 cells and must not overflow.
        assert!(Fx::MAX.sqrt() > Fx::from_int(1 << 23));
    }

    #[test]
    fn lerp_endpoints_and_extremes() {
        let a = Fx::from_int(10);
        let b = Fx::from_int(20);
        assert_eq!(a.lerp(b, Fx::ZERO), a);
        assert_eq!(a.lerp(b, Fx::ONE), b);
        assert_eq!(a.lerp(b, Fx::HALF), Fx::from_int(15));
        // Largest possible span times the largest t must not overflow.
        assert_eq!(Fx::MIN.lerp(Fx::MAX, Fx::MAX), Fx::MAX);
        assert_eq!(Fx::MAX.lerp(Fx::MIN, Fx::MAX), Fx::MIN);
    }

    #[test]
    fn clamp_is_total() {
        let lo = Fx::from_int(5);
        let hi = Fx::from_int(2);
        assert_eq!(Fx::from_int(3).clamp(lo, hi), lo);
        assert_eq!(Fx::from_int(3).clamp(hi, lo), Fx::from_int(3));
        assert_eq!(Fx::from_int(9).clamp(hi, lo), lo);
        assert_eq!(Fx::from_int(-9).clamp(hi, lo), hi);
    }

    #[test]
    fn sum_saturates() {
        let v = [Fx::MAX, Fx::ONE, Fx::ONE];
        assert_eq!(v.iter().sum::<Fx>(), Fx::MAX);
        assert_eq!(v.iter().copied().sum::<Fx>(), Fx::MAX);
        assert_eq!([Fx::ONE, Fx::HALF].iter().sum::<Fx>(), Fx::from_ratio(3, 2));
    }
}
