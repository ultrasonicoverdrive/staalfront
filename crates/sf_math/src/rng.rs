//! [`Rng`]: PCG32, the deterministic random number generator of the game.
//!
//! # Why PCG32
//!
//! PCG-XSH-RR 64/32 (O'Neill, 2014) is tiny (two `u64`), fast, statistically
//! strong for games, has independent *streams*, and is defined purely by
//! 64-bit wrapping multiplication, addition, shifts and a rotation, so it is
//! bit-identical on every platform and in debug and release. It is
//! implemented here, in full, instead of pulling in a dependency, so the
//! output sequence can never change underneath a replay: the first outputs of
//! seed 42 / stream 54 are pinned to the reference implementation's published
//! values in the unit tests.
//!
//! # Contract
//!
//! * Seeding is always explicit; there is no global or time-based seeding.
//! * Range helpers are free of modulo bias: they use rejection sampling
//!   (the number of draws consumed is deterministic given the state, but
//!   not constant).
//! * The state is two `u64` ([`RngState`], or 16 bytes via [`Rng::to_bytes`])
//!   for saves and replays. `Rng` is `Clone` but deliberately not `Copy`, so
//!   a generator is never duplicated by accident.

use crate::facing::Facing;
use crate::fx::Fx;

const MULTIPLIER: u64 = 6_364_136_223_846_793_005;

/// The stream [`Rng::new`] uses (the reference implementation's default
/// increment). Only its low 63 bits matter: the generator stores
/// `(stream << 1) | 1`.
pub const DEFAULT_STREAM: u64 = 0xda3e_39cb_94b9_5bdb;

/// A snapshot of an [`Rng`], for saving and restoring.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct RngState {
    /// The 64-bit LCG state.
    pub state: u64,
    /// The stream increment (always odd for a live generator).
    pub inc: u64,
}

/// PCG32 (XSH RR, 64-bit state), with the helpers a game needs.
///
/// ```
/// use sf_math::Rng;
/// let mut a = Rng::new(7);
/// let mut b = Rng::new(7);
/// assert_eq!(a.next_u32(), b.next_u32());
/// assert!(a.below(10) < 10);
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rng {
    state: u64,
    inc: u64,
}

impl Rng {
    /// A generator on the [`DEFAULT_STREAM`].
    #[inline]
    pub const fn new(seed: u64) -> Rng {
        Rng::with_stream(seed, DEFAULT_STREAM)
    }

    /// A generator with an explicit stream. Different streams (or seeds)
    /// give unrelated sequences, so each consumer (map generation, combat,
    /// the AI of each player) can own a stream. This is the reference
    /// `pcg32_srandom_r(seed, stream)`.
    #[inline]
    pub const fn with_stream(seed: u64, stream: u64) -> Rng {
        let mut rng = Rng {
            state: 0,
            inc: (stream << 1) | 1,
        };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(seed);
        rng.next_u32();
        rng
    }

    /// Restores a generator from a snapshot. The increment is forced odd (a
    /// snapshot of a live generator already is).
    #[inline]
    pub const fn from_state(s: RngState) -> Rng {
        Rng {
            state: s.state,
            inc: s.inc | 1,
        }
    }

    /// A snapshot of the current state.
    #[inline]
    pub const fn state(&self) -> RngState {
        RngState {
            state: self.state,
            inc: self.inc,
        }
    }

    /// The state as 16 bytes: `state` then `inc`, both little-endian.
    #[inline]
    pub const fn to_bytes(&self) -> [u8; 16] {
        let s = self.state.to_le_bytes();
        let i = self.inc.to_le_bytes();
        let mut out = [0u8; 16];
        let mut k = 0;
        while k < 8 {
            out[k] = s[k];
            out[8 + k] = i[k];
            k += 1;
        }
        out
    }

    /// Restores a generator from [`Rng::to_bytes`] output.
    #[inline]
    pub const fn from_bytes(b: [u8; 16]) -> Rng {
        let mut s = [0u8; 8];
        let mut i = [0u8; 8];
        let mut k = 0;
        while k < 8 {
            s[k] = b[k];
            i[k] = b[8 + k];
            k += 1;
        }
        Rng::from_state(RngState {
            state: u64::from_le_bytes(s),
            inc: u64::from_le_bytes(i),
        })
    }

    // ---- raw output --------------------------------------------------------------

    /// The next 32 random bits (`pcg32_random_r`).
    #[inline]
    pub const fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(MULTIPLIER).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// The next 64 random bits: two draws, the first one is the high half.
    #[inline]
    pub const fn next_u64(&mut self) -> u64 {
        let hi = self.next_u32() as u64;
        let lo = self.next_u32() as u64;
        (hi << 32) | lo
    }

    // ---- unbiased ranges ------------------------------------------------------------

    /// A uniform value in `0..bound` (`0` if `bound == 0`), without modulo
    /// bias.
    #[inline]
    pub const fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        // Reject the first `2^32 mod bound` values so the rest divide evenly.
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let r = self.next_u32();
            if r >= threshold {
                return r % bound;
            }
        }
    }

    /// A uniform value in `0..bound` (`0` if `bound == 0`), without modulo
    /// bias, for 64-bit bounds.
    #[inline]
    pub const fn below_u64(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let r = self.next_u64();
            if r >= threshold {
                return r % bound;
            }
        }
    }

    /// A uniform value in `0..bound` as `usize` (`0` if `bound == 0`); the
    /// result depends only on the value of `bound`, not on the pointer width.
    #[inline]
    pub const fn below_usize(&mut self, bound: usize) -> usize {
        if bound <= u32::MAX as usize {
            self.below(bound as u32) as usize
        } else {
            self.below_u64(bound as u64) as usize
        }
    }

    /// A uniform integer in the half-open range `lo..hi`; `lo` if the range
    /// is empty (`hi <= lo`).
    #[inline]
    pub const fn range_i32(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        // hi - lo <= 2^32 - 1 fits u32.
        let span = (hi as i64 - lo as i64) as u32;
        (lo as i64 + self.below(span) as i64) as i32
    }

    /// A uniform integer in the closed range `lo..=hi`; `lo` if `hi < lo`.
    #[inline]
    pub const fn range_inclusive_i32(&mut self, lo: i32, hi: i32) -> i32 {
        if hi < lo {
            return lo;
        }
        let span = hi as i64 - lo as i64 + 1; // 1..=2^32
        if span > u32::MAX as i64 {
            // Every i32 is allowed: any 32 random bits will do.
            return self.next_u32() as i32;
        }
        (lo as i64 + self.below(span as u32) as i64) as i32
    }

    /// `true` with probability `num / den` exactly (`false` if `den == 0`,
    /// `true` if `num >= den`).
    #[inline]
    pub const fn chance(&mut self, num: u32, den: u32) -> bool {
        if den == 0 {
            return false;
        }
        if num >= den {
            return true;
        }
        self.below(den) < num
    }

    /// A fair coin.
    #[inline]
    pub const fn flip(&mut self) -> bool {
        self.next_u32() >> 31 == 1
    }

    // ---- game types ---------------------------------------------------------------------

    /// A uniform [`Fx`] in `0..1`, with 16 bits of resolution (every
    /// multiple of 1/65536 is equally likely).
    #[inline]
    pub const fn fx_unit(&mut self) -> Fx {
        Fx::from_raw((self.next_u32() >> 16) as i64)
    }

    /// A uniform [`Fx`] in `lo..hi` (every representable value equally
    /// likely); `lo` if the range is empty.
    #[inline]
    pub const fn fx_range(&mut self, lo: Fx, hi: Fx) -> Fx {
        if hi.raw() <= lo.raw() {
            return lo;
        }
        // hi - lo < 2^64 fits u64.
        let span = (hi.raw() as i128 - lo.raw() as i128) as u64;
        let off = self.below_u64(span) as i128;
        Fx::from_raw((lo.raw() as i128 + off) as i64)
    }

    /// A uniformly random [`Facing`].
    #[inline]
    pub const fn facing(&mut self) -> Facing {
        Facing::new((self.next_u32() >> 24) as u8)
    }

    // ---- slices ---------------------------------------------------------------------------

    /// A uniformly chosen element, or `None` for an empty slice.
    #[inline]
    pub fn choose<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            Some(&items[self.below_usize(items.len())])
        }
    }

    /// Fisher-Yates shuffle: every permutation is equally likely, and the
    /// result is the same on every platform for the same state.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        let mut i = items.len();
        while i > 1 {
            let j = self.below_usize(i);
            i -= 1;
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference implementation's demo program: seed 42, stream 54 (the
    /// published first outputs).
    const REFERENCE: [u32; 6] = [
        0xa15c_02b7,
        0x7b47_f409,
        0xba1d_3330,
        0x83d2_f293,
        0xbfa4_784b,
        0xcbed_606e,
    ];

    #[test]
    fn matches_the_reference_implementation() {
        let mut rng = Rng::with_stream(42, 54);
        for expected in REFERENCE {
            assert_eq!(rng.next_u32(), expected);
        }
    }

    #[test]
    fn same_seed_same_sequence_different_seed_or_stream_differs() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(1);
        let mut c = Rng::new(2);
        let mut d = Rng::with_stream(1, 99);
        let (mut same, mut other_seed, mut other_stream) = (true, false, false);
        for _ in 0..16 {
            let x = a.next_u32();
            same &= x == b.next_u32();
            other_seed |= x != c.next_u32();
            other_stream |= x != d.next_u32();
        }
        assert!(same && other_seed && other_stream);
    }

    #[test]
    fn state_round_trips() {
        let mut a = Rng::new(0xdead_beef);
        for _ in 0..100 {
            a.next_u32();
        }
        let mut b = Rng::from_state(a.state());
        let mut c = Rng::from_bytes(a.to_bytes());
        assert_eq!(a, b);
        assert_eq!(a, c);
        for _ in 0..100 {
            let x = a.next_u32();
            assert_eq!(x, b.next_u32());
            assert_eq!(x, c.next_u32());
        }
        // An even increment in a snapshot is repaired.
        let fixed = Rng::from_state(RngState { state: 5, inc: 8 });
        assert_eq!(fixed.state().inc, 9);
    }

    #[test]
    fn bytes_are_little_endian() {
        let rng = Rng::from_state(RngState {
            state: 0x0102_0304_0506_0708,
            inc: 0x1112_1314_1516_1719,
        });
        assert_eq!(
            rng.to_bytes(),
            [
                8, 7, 6, 5, 4, 3, 2, 1, 0x19, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x11
            ]
        );
    }

    #[test]
    fn next_u64_is_two_draws_high_first() {
        let mut a = Rng::new(5);
        let mut b = a.clone();
        let x = a.next_u64();
        let hi = b.next_u32() as u64;
        let lo = b.next_u32() as u64;
        assert_eq!(x, hi << 32 | lo);
    }

    #[test]
    fn degenerate_ranges_are_total() {
        let mut r = Rng::new(3);
        assert_eq!(r.below(0), 0);
        assert_eq!(r.below_u64(0), 0);
        assert_eq!(r.below_usize(0), 0);
        assert_eq!(r.below(1), 0);
        assert_eq!(r.range_i32(5, 5), 5);
        assert_eq!(r.range_i32(9, 2), 9);
        assert_eq!(r.range_inclusive_i32(5, 5), 5);
        assert_eq!(r.range_inclusive_i32(9, 2), 9);
        assert!(!r.chance(1, 0));
        assert!(r.chance(5, 5));
        assert!(r.chance(9, 5));
        assert!(!r.chance(0, 5));
        assert_eq!(r.fx_range(Fx::ONE, Fx::ONE), Fx::ONE);
        assert_eq!(r.fx_range(Fx::ONE, Fx::ZERO), Fx::ONE);
        let none: &[u8] = &[];
        assert_eq!(r.choose(none), None);
        // The whole i32 range works in both flavours.
        r.range_inclusive_i32(i32::MIN, i32::MAX);
        let v = r.range_i32(i32::MIN, i32::MAX);
        assert!(v < i32::MAX);
        // The whole Fx range works.
        r.fx_range(Fx::MIN, Fx::MAX);
    }

    #[test]
    fn shuffle_and_choose_are_deterministic_permutations() {
        let mut a = [0u8; 10];
        for (i, v) in a.iter_mut().enumerate() {
            *v = i as u8;
        }
        let mut b = a;
        Rng::new(11).shuffle(&mut a);
        Rng::new(11).shuffle(&mut b);
        assert_eq!(a, b);
        let mut sorted = a;
        sorted.sort_unstable();
        assert_eq!(sorted, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_ne!(a, sorted); // a fixed seed that does move something
        let mut one = [7u8];
        Rng::new(1).shuffle(&mut one);
        assert_eq!(one, [7]);
        let item = Rng::new(2).choose(&a).copied();
        assert!(item.is_some());
    }

    /// A PCG32 state whose next output is exactly `target`. The output is
    /// `rotr(((s >> 18) ^ s) >> 27, s >> 59)`: with the top five bits of `s`
    /// clear the rotation is zero, bits 27 to 58 of `(s >> 18) ^ s` are the
    /// output, and each of those bits can be solved from the top down because
    /// bit `p + 18` is either known already or above bit 58 (zero). Bits 0 to
    /// 26 do not matter and are taken from `free_low`.
    fn state_with_output(target: u32, free_low: u64) -> u64 {
        let mut s = free_low & ((1 << 27) - 1);
        for p in (27..=58u32).rev() {
            let want = u64::from((target >> (p - 27)) & 1);
            let above = if p + 18 <= 58 { (s >> (p + 18)) & 1 } else { 0 };
            s |= (want ^ above) << p;
        }
        // The construction itself is checked, so a wrong helper cannot make
        // the tests below pass or fail by accident.
        let probe = Rng::from_state(RngState { state: s, inc: 1 }).next_u32();
        assert_eq!(probe, target);
        s
    }

    /// A generator whose first `next_u32` is `target`.
    fn rng_whose_first_draw_is(target: u32) -> Rng {
        Rng::from_state(RngState {
            state: state_with_output(target, 0),
            inc: 1,
        })
    }

    /// `rng` after exactly `n` raw draws.
    fn after_draws(rng: &Rng, n: u32) -> Rng {
        let mut r = rng.clone();
        for _ in 0..n {
            r.next_u32();
        }
        r
    }

    #[test]
    fn below_accepts_the_threshold_itself_and_rejects_the_value_under_it() {
        // `threshold = 2^32 mod bound`. The values 0..threshold are rejected;
        // `threshold` itself is the first accepted one, so `r >= threshold`
        // is right and `r > threshold` would bias the result. The generator
        // state afterwards tells how many draws were consumed.
        for (bound, threshold) in [
            (3u32, 1u32),
            (5, 1),
            (6, 4),
            (7, 4),
            (10, 6),
            (0x8000_0001, 0x7FFF_FFFF),
        ] {
            assert_eq!(bound.wrapping_neg() % bound, threshold, "bound {bound}");

            let mut rng = rng_whose_first_draw_is(threshold);
            let one_draw = after_draws(&rng, 1);
            assert_eq!(rng.below(bound), threshold % bound, "bound {bound}");
            assert_eq!(rng, one_draw, "bound {bound}: accepted on the first draw");

            let mut rng = rng_whose_first_draw_is(threshold - 1);
            let one_draw = after_draws(&rng, 1);
            rng.below(bound);
            assert_ne!(rng, one_draw, "bound {bound}: rejected, drew again");
        }
        // A power of two has threshold zero: nothing is ever rejected.
        let mut rng = rng_whose_first_draw_is(0);
        let one_draw = after_draws(&rng, 1);
        assert_eq!(rng.below(8), 0);
        assert_eq!(rng, one_draw);
    }

    #[test]
    fn below_u64_accepts_the_threshold_itself_and_rejects_the_value_under_it() {
        // A state pair (s0, s1) with s1 = s0 * MULTIPLIER + inc, where s0
        // outputs `hi` and s1 outputs `lo`: so the first `next_u64` is
        // `hi << 32 | lo`. The increment must be odd; s0 even and s1 odd
        // make `s1 - s0 * MULTIPLIER` odd.
        let rng_whose_first_u64_is = |hi: u32, lo: u32| {
            let s0 = state_with_output(hi, 0);
            let s1 = state_with_output(lo, 1);
            let inc = s1.wrapping_sub(s0.wrapping_mul(MULTIPLIER));
            assert_eq!(inc & 1, 1);
            let rng = Rng::from_state(RngState { state: s0, inc });
            assert_eq!(rng.clone().next_u64(), u64::from(hi) << 32 | u64::from(lo));
            rng
        };
        // The thresholds 2^64 mod 3 (one) and 2^64 mod 6 (four).
        for (bound, threshold) in [(3u64, 1u32), (6, 4)] {
            assert_eq!(bound.wrapping_neg() % bound, u64::from(threshold));

            let mut rng = rng_whose_first_u64_is(0, threshold);
            let two_draws = after_draws(&rng, 2);
            assert_eq!(
                rng.below_u64(bound),
                u64::from(threshold) % bound,
                "bound {bound}"
            );
            assert_eq!(rng, two_draws, "bound {bound}: accepted on the first pair");

            let mut rng = rng_whose_first_u64_is(0, threshold - 1);
            let two_draws = after_draws(&rng, 2);
            rng.below_u64(bound);
            assert_ne!(rng, two_draws, "bound {bound}: rejected, drew again");
        }
    }

    #[test]
    fn unbiased_bound_arithmetic() {
        // For every bound the accepted draw count is a multiple of the bound.
        for bound in [
            1u32,
            2,
            3,
            5,
            6,
            7,
            10,
            100,
            1000,
            65_535,
            1 << 31,
            (1 << 31) + 1,
            u32::MAX,
        ] {
            let threshold = bound.wrapping_neg() % bound;
            let accepted = (1u64 << 32) - threshold as u64;
            assert_eq!(accepted % bound as u64, 0, "bound {bound}");
        }
    }
}
