//! Property and statistical tests for `Rng` (PCG32).

mod common;

use common::*;
use proptest::prelude::*;
use sf_math::{Facing, Fx, Rng};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn same_seed_and_stream_give_the_same_sequence(seed in any::<u64>(), stream in any::<u64>()) {
        let mut a = Rng::with_stream(seed, stream);
        let mut b = Rng::with_stream(seed, stream);
        for _ in 0..32 {
            prop_assert_eq!(a.next_u32(), b.next_u32());
        }
        prop_assert_eq!(a, b);
    }

    #[test]
    fn streams_differing_below_the_top_bit_diverge(seed in any::<u64>(), stream in 0u64..(1 << 62)) {
        let mut a = Rng::with_stream(seed, stream);
        let mut b = Rng::with_stream(seed, stream + 1);
        let same = (0..16).all(|_| a.next_u32() == b.next_u32());
        prop_assert!(!same);
    }

    #[test]
    fn below_stays_in_range(seed in any::<u64>(), bound in 1u32..=u32::MAX) {
        let mut r = Rng::new(seed);
        for _ in 0..16 {
            prop_assert!(r.below(bound) < bound);
        }
    }

    #[test]
    fn below_u64_and_usize_stay_in_range(seed in any::<u64>(), bound in 1u64..=u64::MAX) {
        let mut r = Rng::new(seed);
        for _ in 0..8 {
            prop_assert!(r.below_u64(bound) < bound);
        }
        let b = (bound % (1 << 40)) as usize + 1;
        prop_assert!(r.below_usize(b) < b);
    }

    #[test]
    fn integer_ranges_stay_in_range(seed in any::<u64>(), a in any::<i32>(), b in any::<i32>()) {
        let (lo, hi) = (a.min(b), a.max(b));
        let mut r = Rng::new(seed);
        for _ in 0..16 {
            let x = r.range_i32(lo, hi);
            if lo == hi {
                prop_assert_eq!(x, lo);
            } else {
                prop_assert!(lo <= x && x < hi);
            }
            let y = r.range_inclusive_i32(lo, hi);
            prop_assert!(lo <= y && y <= hi);
        }
        // Empty ranges answer with their start.
        prop_assert_eq!(r.range_i32(hi, lo.min(hi)), hi);
    }

    #[test]
    fn fx_ranges_stay_in_range(seed in any::<u64>(), a in any_fx(), b in any_fx()) {
        let (lo, hi) = (a.min(b), a.max(b));
        let mut r = Rng::new(seed);
        for _ in 0..16 {
            let x = r.fx_range(lo, hi);
            if lo == hi {
                prop_assert_eq!(x, lo);
            } else {
                prop_assert!(lo <= x && x < hi);
            }
            let u = r.fx_unit();
            prop_assert!(u >= Fx::ZERO && u < Fx::ONE);
        }
    }

    #[test]
    fn chance_edge_cases_and_facing_range(seed in any::<u64>(), num in any::<u32>(), den in any::<u32>()) {
        let mut r = Rng::new(seed);
        prop_assert!(!r.chance(num, 0));
        prop_assert!(r.chance(den, den.max(1)) || den == 0);
        prop_assert!(!r.chance(0, den.max(1)));
        let _ = r.chance(num, den);
        let _: Facing = r.facing();
        let _ = r.flip();
    }

    #[test]
    fn state_round_trips_after_any_number_of_draws(seed in any::<u64>(), stream in any::<u64>(), n in 0usize..200) {
        let mut a = Rng::with_stream(seed, stream);
        for _ in 0..n {
            a.next_u32();
        }
        let mut b = Rng::from_state(a.state());
        let mut c = Rng::from_bytes(a.to_bytes());
        prop_assert_eq!(&a, &b);
        prop_assert_eq!(&a, &c);
        prop_assert_eq!(a.state().inc & 1, 1);
        for _ in 0..32 {
            let x = a.next_u32();
            prop_assert_eq!(x, b.next_u32());
            prop_assert_eq!(x, c.next_u32());
        }
    }

    #[test]
    fn shuffle_is_a_permutation(seed in any::<u64>(), items in proptest::collection::vec(any::<u16>(), 0..64)) {
        let mut shuffled = items.clone();
        Rng::new(seed).shuffle(&mut shuffled);
        let mut a = items.clone();
        let mut b = shuffled.clone();
        a.sort_unstable();
        b.sort_unstable();
        prop_assert_eq!(a, b);
        // Same seed, same permutation.
        let mut again = items.clone();
        Rng::new(seed).shuffle(&mut again);
        prop_assert_eq!(shuffled, again);
    }

    #[test]
    fn choose_returns_an_element(seed in any::<u64>(), items in proptest::collection::vec(any::<u8>(), 0..32)) {
        let mut r = Rng::new(seed);
        match r.choose(&items) {
            None => prop_assert!(items.is_empty()),
            Some(x) => prop_assert!(items.contains(x)),
        }
    }
}

/// With a fixed seed these are deterministic, not flaky; the bounds are
/// many standard deviations wide.
#[test]
fn small_bounds_are_uniform() {
    let mut r = Rng::new(0x1234_5678);
    let mut counts = [0u32; 6];
    for _ in 0..60_000 {
        counts[r.below(6) as usize] += 1;
    }
    for (face, c) in counts.iter().enumerate() {
        assert!(
            (9_600..=10_400).contains(c),
            "face {face}: {c} of 60000 ({counts:?})"
        );
    }
}

/// A bound of 3 * 2^30 would put 1/2 of a biased generator's output in the
/// first third (2^32 mod bound = 2^30 values wrap around); the unbiased one
/// gives a third.
#[test]
fn large_bounds_show_no_modulo_bias() {
    let bound = 3u32 << 30;
    let third = 1u32 << 30;
    let mut r = Rng::new(0xfeed);
    let n = 60_000i64;
    let low = (0..n).filter(|_| r.below(bound) < third).count() as i64;
    // Expect n/3 = 20000 (a biased draw would give 30000); 3 * |low - n/3|
    // under 1% of n.
    assert!(
        (low * 3 - n).abs() < n / 100 * 3,
        "first-third count {low} of {n}"
    );
}

#[test]
fn bits_and_facings_look_uniform() {
    let mut r = Rng::new(99);
    let mut bit_counts = [0u32; 32];
    let mut facing_counts = [0u32; 256];
    for _ in 0..64_000 {
        let x = r.next_u32();
        for (b, count) in bit_counts.iter_mut().enumerate() {
            *count += (x >> b) & 1;
        }
        facing_counts[r.facing().raw() as usize] += 1;
    }
    for (b, c) in bit_counts.iter().enumerate() {
        assert!((31_000..=33_000).contains(c), "bit {b}: {c}");
    }
    for (f, c) in facing_counts.iter().enumerate() {
        assert!((180..=330).contains(c), "facing {f}: {c}"); // mean 250
    }
    let heads = (0..10_000).filter(|_| r.flip()).count();
    assert!((4_700..=5_300).contains(&heads), "flip: {heads}");
    let hits = (0..30_000).filter(|_| r.chance(1, 3)).count();
    assert!((9_500..=10_500).contains(&hits), "chance: {hits}");
}

#[test]
fn fx_range_covers_the_whole_interval() {
    let mut r = Rng::new(7);
    let (lo, hi) = (Fx::from_int(-3), Fx::from_int(5));
    let (mut min, mut max) = (hi, lo);
    let mut sum = 0i128;
    let n = 20_000;
    for _ in 0..n {
        let x = r.fx_range(lo, hi);
        min = min.min(x);
        max = max.max(x);
        sum += x.raw() as i128;
    }
    // Mean is the midpoint (1 cell) within a fraction of a percent.
    let mean = sum / n;
    assert!((mean - (1 << 16)).abs() < 2_500, "mean {mean}");
    assert!(min < Fx::from_ratio(-299, 100) && max > Fx::from_ratio(499, 100));
}
