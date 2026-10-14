//! Sine and cosine tables for the 256 [`Facing`](crate::Facing) directions,
//! generated at compile time with integer arithmetic only.
//!
//! # Generation (no floats, no committed magic numbers)
//!
//! The table holds `sin(k * 2pi/256)` for `k = 0..=64` (one quadrant, both
//! end points) as `Q16` raw values, i.e. `round(sin * 65536)`. It is built in
//! `Q62` fixed point:
//!
//! * Start from the angle pi/2 (cosine 0, sine 1). Pi itself is never needed.
//! * Halve the angle six times with the half-angle identities
//!   `cos(a/2) = sqrt((1 + cos a) / 2)` and `sin(a/2) = sin a / (2 cos(a/2))`,
//!   using the exact integer square root. This yields the angle of exactly one
//!   facing step, a 256th of a turn.
//! * Rotate by that step 64 times (angle addition) and record the sine.
//!
//! The accumulated error is below `2^-45`, far under the `2^-17` rounding of
//! the final table, so every entry is the correctly rounded value except
//! possibly when the true value is within `2^-45` of a rounding boundary.
//! The integration test `reference_floats` compares every entry with an
//! independent floating-point computation and finds them equal.
//!
//! The other three quadrants and the cosine follow from the symmetries
//! `cos(k) = sin(64 - k)`, `sin(64 + k) = cos(k)` and so on, so those
//! identities hold *exactly* in the table.

use crate::int::{div_round, round_shr};

/// Fractional bits of the generation arithmetic.
const P: u32 = 62;
const ONE_P: i128 = 1 << P;

/// Number of facings in a quarter turn.
const QUARTER: usize = 64;

/// `sin(k * 2pi/256)` for `k = 0..=64`, as `Q16` raw values.
pub(crate) const SIN_QUADRANT: [i32; QUARTER + 1] = build_quadrant();

const fn build_quadrant() -> [i32; QUARTER + 1] {
    // Half-angle chain from a quarter turn down to one facing step.
    let mut c: i128 = 0; // cos(pi/2)
    let mut s: i128 = ONE_P; // sin(pi/2)
    let mut i = 0;
    while i < 6 {
        // cos(a/2) = sqrt((1 + cos a) / 2); the argument stays below 2^63
        let half = (ONE_P + c) / 2;
        let c2 = ((half as u128) << P).isqrt() as i128;
        // sin(a/2) = sin(a) / (2 cos(a/2)); no cancellation, unlike
        // sqrt((1 - cos a) / 2).
        let s2 = div_round(s << P, 2 * c2);
        c = c2;
        s = s2;
        i += 1;
    }
    let (c1, s1) = (c, s); // cos and sin of one facing step

    // Rotate by one step at a time: (c, s) <- (c, s) rotated by (c1, s1).
    let mut out = [0i32; QUARTER + 1];
    let mut ck: i128 = ONE_P;
    let mut sk: i128 = 0;
    let mut k = 0;
    while k <= QUARTER {
        out[k] = round_shr(sk, P - 16) as i32;
        let nc = round_shr(ck * c1 - sk * s1, P);
        let ns = round_shr(sk * c1 + ck * s1, P);
        ck = nc;
        sk = ns;
        k += 1;
    }
    out
}

// Compile-time sanity: exact end points and strict monotonicity.
const _: () = {
    assert!(SIN_QUADRANT[0] == 0);
    assert!(SIN_QUADRANT[QUARTER] == 1 << 16);
    let mut k = 0;
    while k < QUARTER {
        assert!(SIN_QUADRANT[k] < SIN_QUADRANT[k + 1]);
        k += 1;
    }
};

/// `sin(facing)` and `cos(facing)` as `Q16` raw values, by quadrant symmetry.
#[inline]
pub(crate) const fn sin_cos_raw(facing: u8) -> (i64, i64) {
    let k = (facing & 63) as usize;
    let a = SIN_QUADRANT[k] as i64; // sin(k)
    let b = SIN_QUADRANT[QUARTER - k] as i64; // cos(k)
    match facing >> 6 {
        0 => (a, b),
        1 => (b, -a),
        2 => (-a, -b),
        _ => (-b, a),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_entries() {
        assert_eq!(SIN_QUADRANT[0], 0);
        assert_eq!(SIN_QUADRANT[64], 65536);
        // sin of 45 degrees is sqrt(1/2), 46341 / 65536 once rounded.
        assert_eq!(SIN_QUADRANT[32], 46341);
        // One step is just under pi/128 radians (a tiny cubic term below it).
        assert_eq!(SIN_QUADRANT[1], 1608);
    }

    #[test]
    fn symmetry_is_exact() {
        for f in 0..=255u8 {
            let (s, c) = sin_cos_raw(f);
            let (s90, c90) = sin_cos_raw(f.wrapping_add(64));
            let (s180, c180) = sin_cos_raw(f.wrapping_add(128));
            assert_eq!((s90, c90), (c, -s));
            assert_eq!((s180, c180), (-s, -c));
            let (sn, cn) = sin_cos_raw(f.wrapping_neg());
            assert_eq!((sn, cn), (-s, c));
        }
    }
}
