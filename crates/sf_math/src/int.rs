//! Exact integer helpers shared by the fixed-point types.
//!
//! Everything here works on `i128`/`u128` so that products of two `i64` raw
//! values never overflow, and every function is `const` so tables can be
//! built at compile time. All rounding is "to nearest, ties away from zero",
//! which makes the operations odd-symmetric (`f(-x) == -f(x)`).

/// Clamps a wide value into `i64`.
#[inline]
pub(crate) const fn sat_i64(v: i128) -> i64 {
    if v > i64::MAX as i128 {
        i64::MAX
    } else if v < i64::MIN as i128 {
        i64::MIN
    } else {
        v as i64
    }
}

/// `round(n / 2^bits)` with ties away from zero. `bits` must be in `1..=100`
/// and `|n| + 2^(bits-1)` must fit in `i128`.
#[inline]
pub(crate) const fn round_shr(n: i128, bits: u32) -> i128 {
    let half = 1i128 << (bits - 1);
    if n >= 0 {
        (n + half) >> bits
    } else {
        -((-n + half) >> bits)
    }
}

/// [`round_shr`] followed by [`sat_i64`], total for every `i128` `n`
/// (`bits` in `1..=62`): anything of magnitude `2^126` or more is far outside
/// the `i64` range after the shift, so it saturates without rounding.
#[inline]
pub(crate) const fn sat_round_shr(n: i128, bits: u32) -> i64 {
    const LIMIT: i128 = 1 << 126;
    if n >= LIMIT {
        i64::MAX
    } else if n <= -LIMIT {
        i64::MIN
    } else {
        sat_i64(round_shr(n, bits))
    }
}

/// `round(n / d)` with ties away from zero. `d` must not be zero and
/// `n` must not be `i128::MIN` (every caller stays far below that).
#[inline]
pub(crate) const fn div_round(n: i128, d: i128) -> i128 {
    let q = n / d;
    let r = n % d;
    // |r| < |d|, so `2 * |r|` fits in u128.
    if r.unsigned_abs() * 2 >= d.unsigned_abs() {
        if (n < 0) != (d < 0) { q - 1 } else { q + 1 }
    } else {
        q
    }
}

/// Integer square root rounded to nearest (never a tie: `n` would have to
/// be `r*r + r + 1/4`). `|result - sqrt(n)| < 1/2`.
#[inline]
pub(crate) const fn isqrt_round(n: u128) -> u128 {
    let r = n.isqrt();
    // sqrt(n) > r + 1/2  <=>  n >= r*r + r + 1
    if n - r * r > r { r + 1 } else { r }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_shr_ties_away_from_zero() {
        assert_eq!(round_shr(3, 1), 2); // 3/2 -> 2
        assert_eq!(round_shr(-3, 1), -2); // -3/2 -> -2
        assert_eq!(round_shr(1, 1), 1); // 1/2 -> 1
        assert_eq!(round_shr(-1, 1), -1);
        assert_eq!(round_shr(5, 2), 1); // 5/4 -> 1
        assert_eq!(round_shr(-5, 2), -1);
        assert_eq!(round_shr(0, 16), 0);
    }

    #[test]
    fn div_round_cases() {
        assert_eq!(div_round(7, 2), 4);
        assert_eq!(div_round(-7, 2), -4);
        assert_eq!(div_round(7, -2), -4);
        assert_eq!(div_round(-7, -2), 4);
        assert_eq!(div_round(5, 3), 2);
        assert_eq!(div_round(4, 3), 1);
        assert_eq!(div_round(0, 5), 0);
        assert_eq!(div_round(6, 3), 2);
    }

    #[test]
    fn isqrt_round_cases() {
        assert_eq!(isqrt_round(0), 0);
        assert_eq!(isqrt_round(1), 1);
        assert_eq!(isqrt_round(2), 1); // sqrt(2) is below 3/2
        assert_eq!(isqrt_round(3), 2); // sqrt(3) is above 3/2
        assert_eq!(isqrt_round(6), 2); // sqrt(6) is below 5/2
        assert_eq!(isqrt_round(7), 3); // sqrt(7) is above 5/2
        assert_eq!(isqrt_round(u128::MAX), 1u128 << 64); // 2^64 - 2^-65
    }
}
