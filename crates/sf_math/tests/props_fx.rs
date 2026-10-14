//! Property tests for `Fx`. Strategies and the meaning of "safe" are in
//! `common/mod.rs`. Tests run in debug mode by default, so any hidden
//! integer overflow in the library would panic here; the full-range
//! strategies exist to provoke exactly that.

mod common;

use common::*;
use proptest::prelude::*;
use sf_math::Fx;

fn wide(a: Fx) -> i128 {
    a.raw() as i128
}

/// Parses `Display` output back into a raw value, asserting that the
/// expansion is exact and minimal.
fn parse_display(s: &str) -> i128 {
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let (int, frac) = rest.split_once('.').unwrap_or((rest, ""));
    assert!(!frac.ends_with('0'), "trailing zero in {s}");
    assert!(frac.len() <= 16, "too many digits in {s}");
    let mut raw = int.parse::<i128>().unwrap() << 16;
    if !frac.is_empty() {
        let num = frac.parse::<i128>().unwrap() * 65536;
        let den = 10i128.pow(frac.len() as u32);
        assert_eq!(num % den, 0, "inexact fraction in {s}");
        raw += num / den;
    }
    if neg {
        assert!(raw != 0, "negative zero in {s}");
        -raw
    } else {
        raw
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4000))]

    // ---- addition and subtraction -----------------------------------------------

    #[test]
    fn add_and_sub_are_inverse(a in safe_fx(), b in safe_fx()) {
        prop_assert_eq!((a + b) - b, a);
        prop_assert_eq!((a - b) + b, a);
        prop_assert_eq!(a + b, b + a);
        prop_assert_eq!(a - a, Fx::ZERO);
        prop_assert_eq!(a + Fx::ZERO, a);
        prop_assert_eq!(-(-a), a);
        prop_assert_eq!(a - b, a + (-b));
    }

    #[test]
    fn add_is_associative_in_range(a in safe_fx(), b in safe_fx(), c in safe_fx()) {
        prop_assert_eq!((a + b) + c, a + (b + c));
    }

    #[test]
    fn add_and_sub_saturate_exactly_like_i64(a in any_fx(), b in any_fx()) {
        prop_assert_eq!((a + b).raw(), a.raw().saturating_add(b.raw()));
        prop_assert_eq!((a - b).raw(), a.raw().saturating_sub(b.raw()));
        prop_assert_eq!(a.checked_add(b).is_none(), a.raw().checked_add(b.raw()).is_none());
        prop_assert_eq!(a.checked_sub(b).is_none(), a.raw().checked_sub(b.raw()).is_none());
        if let Some(s) = a.checked_add(b) {
            prop_assert_eq!(s, a + b);
        }
    }

    #[test]
    fn neg_and_abs(a in any_fx()) {
        if a == Fx::MIN {
            prop_assert_eq!(-a, Fx::MAX);
            prop_assert_eq!(a.abs(), Fx::MAX);
            prop_assert_eq!(a.checked_neg(), None);
        } else {
            prop_assert_eq!(-a, Fx::from_raw(-a.raw()));
            prop_assert_eq!(a.abs().raw(), a.raw().abs());
            prop_assert_eq!(a.checked_neg(), Some(-a));
        }
        prop_assert!(a.abs() >= Fx::ZERO);
    }

    // ---- multiplication ------------------------------------------------------------

    #[test]
    fn mul_commutes_and_has_identity_over_the_whole_range(a in any_fx(), b in any_fx()) {
        prop_assert_eq!(a * b, b * a);
        prop_assert_eq!(a * Fx::ONE, a);
        prop_assert_eq!(Fx::ONE * a, a);
        prop_assert_eq!(a * Fx::ZERO, Fx::ZERO);
    }

    #[test]
    fn mul_is_correctly_rounded_or_saturates_to_the_right_end(a in any_fx(), b in any_fx()) {
        let exact = wide(a) * wide(b); // the true product, scaled by 2^16
        let r = a * b;
        match a.checked_mul(b) {
            Some(c) => {
                prop_assert_eq!(c, r);
                let err = (wide(r) << 16) - exact;
                prop_assert!(err.abs() <= 1 << 15, "error {} too big", err);
            }
            None => {
                prop_assert!((exact > 0 && r == Fx::MAX) || (exact < 0 && r == Fx::MIN));
            }
        }
    }

    #[test]
    fn mul_is_odd_symmetric_in_range(a in map_fx(), b in map_fx()) {
        prop_assert_eq!((-a) * b, -(a * b));
        prop_assert_eq!(a * (-b), -(a * b));
        prop_assert_eq!((-a) * (-b), a * b);
    }

    #[test]
    fn mul_distributes_up_to_rounding(a in map_fx(), b in map_fx(), c in map_fx()) {
        let l = a * (b + c);
        let r = a * b + a * c;
        prop_assert!((l.raw() - r.raw()).abs() <= 1);
    }

    #[test]
    fn mul_by_whole_numbers_is_exact(a in map_fx(), n in -1000i32..1000) {
        prop_assert_eq!(a * Fx::from_int(n), a.mul_int(n as i64));
        prop_assert_eq!(a.mul_int(n as i64).raw(), a.raw() * n as i64);
    }

    // ---- division -----------------------------------------------------------------

    #[test]
    fn div_is_correctly_rounded_or_saturates_to_the_right_end(a in any_fx(), b in any_fx()) {
        if b.is_zero() {
            return Ok(());
        }
        let num = wide(a) << 16;
        let q = a / b;
        match a.checked_div(b) {
            Some(c) => {
                prop_assert_eq!(c, q);
                let err = wide(q) * wide(b) - num;
                prop_assert!(2 * err.abs() <= wide(b).abs());
            }
            None => {
                let negative = (a.is_negative()) != (b.is_negative());
                prop_assert!((!negative && q == Fx::MAX) || (negative && q == Fx::MIN));
            }
        }
    }

    #[test]
    fn division_by_zero_never_panics(a in any_fx()) {
        let q = a / Fx::ZERO;
        prop_assert_eq!(q, if a.is_positive() { Fx::MAX } else if a.is_negative() { Fx::MIN } else { Fx::ZERO });
        prop_assert_eq!(a.checked_div(Fx::ZERO), None);
        prop_assert_eq!(a.div_int(0), q);
        prop_assert_eq!(a.mul_div(Fx::ONE, Fx::ZERO), q);
    }

    #[test]
    fn dividing_by_the_whole_number_just_multiplied_by_is_exact(a in map_fx(), k in 1i32..200) {
        let k = Fx::from_int(k);
        prop_assert_eq!((a * k) / k, a);
    }

    #[test]
    fn mul_div_is_one_rounding_step(a in map_fx(), n in map_fx(), d in map_fx()) {
        if d.is_zero() {
            return Ok(());
        }
        let r = a.mul_div(n, d);
        let err = wide(r) * wide(d) - wide(a) * wide(n);
        prop_assert!(2 * err.abs() <= wide(d).abs());
    }

    #[test]
    fn from_ratio_is_correctly_rounded(num in -(1i64 << 40)..(1i64 << 40), den in prop_oneof![1i64..(1 << 40), -(1i64 << 40)..-1]) {
        let r = Fx::from_ratio(num, den);
        let err = wide(r) * den as i128 - ((num as i128) << 16);
        prop_assert!(2 * err.abs() <= (den as i128).abs());
        // Scaling both sides changes nothing.
        prop_assert_eq!(Fx::from_ratio(num * 3, den * 3), r);
        prop_assert_eq!(Fx::from_ratio(-num, -den), r);
    }

    #[test]
    fn whole_number_division_matches_from_ratio(n in -100_000i32..100_000, d in 1i32..1000) {
        let q = Fx::from_int(n) / Fx::from_int(d);
        prop_assert_eq!(q, Fx::from_ratio(n as i64, d as i64));
    }

    // ---- ordering helpers ---------------------------------------------------------------

    #[test]
    fn min_max_clamp_laws(a in any_fx(), b in any_fx(), c in any_fx()) {
        prop_assert!(a.min(b) <= a && a.min(b) <= b);
        prop_assert!(a.max(b) >= a && a.max(b) >= b);
        prop_assert!(a.min(b) == a || a.min(b) == b);
        let (lo, hi) = (b.min(c), b.max(c));
        let x = a.clamp(lo, hi);
        prop_assert!(lo <= x && x <= hi);
        prop_assert_eq!(x.clamp(lo, hi), x);
        if lo <= a && a <= hi {
            prop_assert_eq!(x, a);
        }
        // Total even for an inverted range.
        let _ = a.clamp(hi, lo);
    }

    #[test]
    fn lerp_is_bounded_and_hits_the_ends(a in safe_fx(), b in safe_fx(), t in 0i64..=65536) {
        let t = Fx::from_raw(t);
        let x = a.lerp(b, t);
        prop_assert!(x >= a.min(b) && x <= a.max(b));
        prop_assert_eq!(a.lerp(b, Fx::ZERO), a);
        prop_assert_eq!(a.lerp(b, Fx::ONE), b);
    }

    // ---- floor, ceil, round, cells -----------------------------------------------------------------

    #[test]
    fn rounding_family(a in any_fx()) {
        let f = a.floor();
        prop_assert!(f <= a);
        prop_assert_eq!(f.raw() & 0xFFFF, 0);
        prop_assert_eq!(f.floor(), f);
        prop_assert_eq!(a.fract(), a - f);
        prop_assert!(a.fract() >= Fx::ZERO && a.fract() < Fx::ONE);
        prop_assert_eq!(f.raw(), a.floor_int() << 16);
        prop_assert!(a.ceil() >= a);
        prop_assert!(a.ceil_int() >= a.floor_int());
        prop_assert!(a.ceil_int() - a.floor_int() <= 1);
        prop_assert_eq!(a.ceil_int() == a.floor_int(), a.fract().is_zero());
        prop_assert!(a.trunc().abs() <= a.abs());
        prop_assert_eq!(a.trunc_int(), if a.is_negative() { a.ceil_int() } else { a.floor_int() });
        // Round is the nearest whole number (halves up): within half a cell.
        let r = wide(a.round()) - wide(a);
        prop_assert!((-(1 << 15) + 1..=(1 << 15)).contains(&r) || a.round() == Fx::MAX,
            "round error {} for {:?}", r, a);
        if a.round() != Fx::MAX {
            prop_assert_eq!(a.round_int() << 16, a.round().raw());
        }
    }

    #[test]
    fn floor_ceil_round_are_monotonic(a in any_fx(), b in any_fx()) {
        let (lo, hi) = (a.min(b), a.max(b));
        prop_assert!(lo.floor() <= hi.floor());
        prop_assert!(lo.ceil() <= hi.ceil());
        prop_assert!(lo.round() <= hi.round());
        prop_assert!(lo.floor_int() <= hi.floor_int());
        prop_assert!(lo.cell() <= hi.cell());
        prop_assert!(lo.to_leptons() <= hi.to_leptons());
    }

    #[test]
    fn cell_and_lepton_decompose_a_coordinate(a in safe_fx()) {
        prop_assert_eq!(a.cell() as i64, a.floor_int());
        prop_assert_eq!(a.cell() as i64 * 256 + a.lepton_in_cell() as i64, a.to_leptons());
        // The lepton position reassembles to a value no larger than a, and
        // less than one lepton below it.
        let base = Fx::from_int(a.cell()) + Fx::from_leptons(a.lepton_in_cell() as i64);
        prop_assert!(base <= a && a - base < Fx::LEPTON);
        // Leptons are exact.
        let l = a.to_leptons();
        prop_assert_eq!(Fx::from_leptons(l).to_leptons(), l);
    }

    // ---- square root -----------------------------------------------------------------------------

    #[test]
    fn sqrt_is_correctly_rounded(a in any_fx()) {
        let r = a.sqrt();
        if a.raw() <= 0 {
            prop_assert_eq!(r, Fx::ZERO);
        } else {
            let n = (a.raw() as u128) << 16;
            prop_assert!(is_rounded_sqrt(n, r.raw() as u128), "{:?} -> {:?}", a, r);
        }
    }

    #[test]
    fn sqrt_is_monotonic(a in any_fx(), b in any_fx()) {
        let (lo, hi) = (a.min(b), a.max(b));
        prop_assert!(lo.sqrt() <= hi.sqrt());
    }

    #[test]
    fn sqrt_of_a_square_returns_the_magnitude(a in map_fx()) {
        if a.raw().abs() < 1 << 16 {
            return Ok(());
        }
        prop_assert_eq!((a * a).sqrt(), a.abs());
        // Squaring the root lands within the root's own rounding error.
        let root = a.abs().sqrt();
        prop_assert!((root * root - a.abs()).raw().abs() <= root.raw() / 65536 + 1);
    }

    // ---- display and operators -----------------------------------------------------------------------

    #[test]
    fn display_is_exact_and_round_trips(a in any_fx()) {
        let s = format!("{a}");
        prop_assert_eq!(parse_display(&s), wide(a));
        prop_assert_eq!(format!("{a:?}"), format!("Fx({s})"));
    }

    #[test]
    fn ordering_is_raw_ordering(a in any_fx(), b in any_fx()) {
        prop_assert_eq!(a.cmp(&b), a.raw().cmp(&b.raw()));
        prop_assert_eq!(a < b, a.raw() < b.raw());
    }

    #[test]
    fn assign_operators_match_binary_operators(a in any_fx(), b in any_fx()) {
        let mut x = a; x += b; prop_assert_eq!(x, a + b);
        let mut x = a; x -= b; prop_assert_eq!(x, a - b);
        let mut x = a; x *= b; prop_assert_eq!(x, a * b);
        let mut x = a; x /= b; prop_assert_eq!(x, a / b);
    }

    #[test]
    fn sum_matches_a_fold_of_saturating_adds(v in proptest::collection::vec(any_fx(), 0..20)) {
        let folded = v.iter().fold(Fx::ZERO, |acc, x| acc + *x);
        prop_assert_eq!(v.iter().sum::<Fx>(), folded);
        prop_assert_eq!(v.iter().copied().sum::<Fx>(), folded);
    }

    /// Every public operation on arbitrary bit patterns: nothing may panic
    /// (this is the debug-build overflow check) or invoke undefined behaviour.
    #[test]
    fn nothing_panics_for_any_input(a in any_fx(), b in any_fx(), c in any_fx(), n in any::<i64>(), small in any::<i32>()) {
        let _ = (a + b, a - b, a * b, a / b, -a, a.abs(), a.square(), a.signum());
        let _ = (a.saturating_add(b), a.saturating_sub(b), a.saturating_mul(b), a.saturating_div(b));
        let _ = (a.checked_add(b), a.checked_sub(b), a.checked_mul(b), a.checked_div(b), a.checked_neg());
        let _ = (a.mul_int(n), a.div_int(n), a.mul_div(b, c), a.lerp(b, c), a.clamp(b, c), a.min(b), a.max(b));
        let _ = (a.floor(), a.ceil(), a.round(), a.trunc(), a.fract());
        let _ = (a.floor_int(), a.ceil_int(), a.round_int(), a.trunc_int());
        let _ = (a.cell(), a.to_leptons(), a.lepton_in_cell(), a.sqrt());
        let _ = (Fx::from_int(small), Fx::from_i64(n), Fx::from_ratio(n, a.raw()), Fx::from_leptons(n));
        let _ = (format!("{a} {a:?}"), a.is_zero(), a.is_negative(), a.is_positive());
    }
}
