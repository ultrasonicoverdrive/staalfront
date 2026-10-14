//! Property tests for `Vec2`: vector algebra, exact lengths, normalising and
//! rotation. See `common/mod.rs` for the strategies.

mod common;

use common::*;
use proptest::prelude::*;
use sf_math::{Facing, Fx, Vec2};

fn raw_len(v: Vec2) -> i128 {
    v.length().raw() as i128
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4000))]

    // ---- algebra --------------------------------------------------------------------

    #[test]
    fn add_sub_neg_laws(a in safe_vec(), b in safe_vec()) {
        prop_assert_eq!((a + b) - b, a);
        prop_assert_eq!(a + b, b + a);
        prop_assert_eq!(a - a, Vec2::ZERO);
        prop_assert_eq!(-(-a), a);
        prop_assert_eq!(a - b, a + (-b));
        let mut c = a;
        c += b;
        prop_assert_eq!(c, a + b);
        c -= b;
        prop_assert_eq!(c, a);
    }

    #[test]
    fn scaling_laws(a in map_vec(), k in map_fx(), n in -100i64..100) {
        prop_assert_eq!(a * k, k * a);
        prop_assert_eq!(a * Fx::ONE, a);
        prop_assert_eq!(a * Fx::ZERO, Vec2::ZERO);
        prop_assert_eq!(a.mul_int(n), a * Fx::from_int(n as i32));
        prop_assert_eq!(a.scale(k).x, a.x * k);
        if !k.is_zero() && k.abs() >= Fx::ONE {
            // Dividing back by a factor of at least one is exact to a unit.
            let back = (a * k) / k;
            prop_assert!((back.x - a.x).raw().abs() <= 1);
            prop_assert!((back.y - a.y).raw().abs() <= 1);
        }
    }

    #[test]
    fn dot_and_cross_algebra(a in map_vec(), b in map_vec()) {
        prop_assert_eq!(a.dot(b), b.dot(a));
        prop_assert_eq!(a.cross(b), -b.cross(a));
        prop_assert_eq!(a.cross(a), Fx::ZERO);
        prop_assert_eq!(a.dot(a), a.length_squared());
        prop_assert_eq!(a.dot(a.perp()), Fx::ZERO);
        prop_assert_eq!(a.perp().perp(), -a);
        // Lagrange's identity, up to the rounding of each factor: a factor
        // that is off by half a unit shifts its square by about its own size.
        let lhs = a.dot(b).square() + a.cross(b).square();
        let rhs = a.length_squared() * b.length_squared();
        let scale = a.dot(b).raw().abs() + a.cross(b).raw().abs()
            + a.length_squared().raw() + b.length_squared().raw();
        let tolerance = scale / 32_768 + 16;
        prop_assert!((lhs.raw() - rhs.raw()).abs() <= tolerance, "{:?} vs {:?}", lhs, rhs);
    }

    #[test]
    fn dot_is_exact_to_one_rounding_or_saturates(a in any_vec(), b in any_vec()) {
        let p = a.x.raw() as i128 * b.x.raw() as i128;
        let q = a.y.raw() as i128 * b.y.raw() as i128;
        // The sum overflows i128 only for MIN*MIN twice; that corner
        // saturates inside the library and is covered by a unit test.
        let Some(exact) = p.checked_add(q) else { return Ok(()); };
        let r = a.dot(b).raw() as i128;
        let limit = 1i128 << 79; // |exact| / 2^16 reaches 2^63 here
        if exact.abs() <= limit - (1 << 16) {
            prop_assert!(((r << 16) - exact).abs() <= 1 << 15);
        } else if exact.abs() >= limit + (1 << 16) {
            prop_assert_eq!(r, if exact > 0 { i64::MAX as i128 } else { i64::MIN as i128 });
        }
    }

    // ---- lengths -------------------------------------------------------------------------

    #[test]
    fn length_is_the_correctly_rounded_euclidean_length(v in safe_vec()) {
        let l = raw_len(v) as u128;
        prop_assert!(is_rounded_sqrt(norm_sq(v), l));
        let (ax, ay) = (v.x.abs().raw() as i128, v.y.abs().raw() as i128);
        prop_assert!(raw_len(v) >= ax.max(ay));
        prop_assert!(raw_len(v) <= ax + ay);
        prop_assert_eq!(v.length().is_zero(), v.is_zero());
    }

    #[test]
    fn length_never_panics_and_dominates_the_components(v in any_vec()) {
        let l = v.length();
        prop_assert!(l >= v.x.abs() && l >= v.y.abs());
        prop_assert!(l >= Fx::ZERO);
        prop_assert_eq!((-v).length(), l);
        prop_assert_eq!(v.perp().length(), l);
    }

    #[test]
    fn distance_is_a_metric_up_to_rounding(a in safe_vec(), b in safe_vec(), c in safe_vec()) {
        prop_assert_eq!(a.distance(b), b.distance(a));
        prop_assert_eq!(a.distance(a), Fx::ZERO);
        prop_assert_eq!(a.distance(b), (a - b).length());
        prop_assert_eq!(a.distance_squared(b), (a - b).length_squared());
        let (ab, bc, ac) = (a.distance(b).raw(), b.distance(c).raw(), a.distance(c).raw());
        // Each distance is rounded by at most half a unit.
        prop_assert!(ac <= ab + bc + 1);
    }

    // ---- normalising -------------------------------------------------------------------------

    #[test]
    fn normalize_gives_a_unit_vector_in_the_same_direction(v in any_vec()) {
        let n = v.normalize_or_zero();
        if v.is_zero() {
            prop_assert_eq!(n, Vec2::ZERO);
        } else {
            let len = raw_len(n);
            prop_assert!((len - 65536).abs() <= 2, "length {} for {:?} -> {:?}", len, v, n);
            // Components keep their signs (or round to zero).
            prop_assert!(n.x.raw() == 0 || (n.x.raw() < 0) == (v.x.raw() < 0));
            prop_assert!(n.y.raw() == 0 || (n.y.raw() < 0) == (v.y.raw() < 0));
            // Parallel to v: the cross product of v with n is at most the
            // rounding error (half a unit per component) times |v|'s span.
            let cross = v.x.raw() as i128 * n.y.raw() as i128 - v.y.raw() as i128 * n.x.raw() as i128;
            let span = v.x.raw().unsigned_abs() as i128 + v.y.raw().unsigned_abs() as i128;
            prop_assert!(cross.abs() <= span / 2 + 2, "cross {} span {}", cross, span);
        }
    }

    #[test]
    fn normalize_ignores_integer_scale(v in safe_vec(), k in 1i64..=8) {
        if v.is_zero() {
            return Ok(());
        }
        let a = v.normalize_or_zero();
        let b = v.mul_int(k).normalize_or_zero();
        // Both are roundings of the same true unit vector.
        prop_assert!((a.x - b.x).raw().abs() <= 1 && (a.y - b.y).raw().abs() <= 1);
    }

    #[test]
    fn with_length_hits_the_requested_length(v in any_vec(), len in any_fx()) {
        let r = v.with_length(len);
        if v.is_zero() {
            prop_assert_eq!(r, Vec2::ZERO);
        } else if !len.is_negative() {
            // Per-component error is at most 1.5 units; huge lengths add a
            // little (the divisor's own 2^-63 relative error).
            prop_assert!((raw_len(r) - len.raw() as i128).abs() <= 4, "{:?} {:?} -> {:?}", v, len, r);
        }
    }

    #[test]
    fn with_negative_length_flips_direction(v in map_vec(), len in (1i64..(1 << 24)).prop_map(Fx::from_raw)) {
        if v.is_zero() {
            return Ok(());
        }
        let a = v.with_length(len);
        let b = v.with_length(-len);
        prop_assert_eq!(a, -b);
    }

    #[test]
    fn clamp_length_shortens_only_when_needed(v in map_vec(), max in map_fx()) {
        let r = v.clamp_length(max);
        let m = max.raw().max(0) as u128;
        if norm_sq(v) <= m * m {
            prop_assert_eq!(r, v);
        } else {
            prop_assert!((raw_len(r) - m as i128).abs() <= 4);
        }
    }

    // ---- directions -----------------------------------------------------------------------------

    #[test]
    fn facing_of_a_vector_is_the_nearest_table_direction(v in any_vec()) {
        match v.facing() {
            None => prop_assert!(v.is_zero()),
            Some(f) => {
                prop_assert!(!v.is_zero());
                // Within half a step (plus table precision): the sine of the
                // angle to the chosen direction is below 1/80 (half a step is
                // 1/81.5 in radians).
                let u = f.to_vec2();
                let cross = v.x.raw() as i128 * u.y.raw() as i128 - v.y.raw() as i128 * u.x.raw() as i128;
                let dot = v.x.raw() as i128 * u.x.raw() as i128 + v.y.raw() as i128 * u.y.raw() as i128;
                // |cross| = |v| |u| sin(angle) and |u| = 65536 (the length is
                // rounded, hence the +1).
                let bound = (exact_len(v) as i128 + 1) * 65536 / 80;
                prop_assert!(dot > 0);
                prop_assert!(cross.abs() <= bound, "cross {} bound {}", cross, bound);
            }
        }
    }

    #[test]
    fn facing_ignores_integer_scale(v in safe_vec(), k in 1i64..=8) {
        if v.is_zero() {
            return Ok(());
        }
        prop_assert_eq!(v.facing(), v.mul_int(k).facing());
    }

    #[test]
    fn facing_of_a_unit_vector_round_trips_at_any_length(f in any_facing(), len in 256i64..(1 << 40)) {
        // Below about 100 raw units the vector itself is too coarse to resolve
        // a single step (component rounding), so start at 256.
        let v = Vec2::from_facing(f, Fx::from_raw(len));
        prop_assert_eq!(v.facing(), Some(f));
    }

    #[test]
    fn rotating_by_zero_and_quarter_turns_is_exact(v in any_vec()) {
        prop_assert_eq!(v.rotate(Facing::PLUS_X), v);
        prop_assert_eq!(v.rotate(Facing::PLUS_Y), Vec2::new(v.y.saturating_neg(), v.x));
        prop_assert_eq!(v.rotate(Facing::MINUS_X), -v);
        prop_assert_eq!(v.rotate(Facing::MINUS_Y), Vec2::new(v.y, v.x.saturating_neg()));
    }

    #[test]
    fn rotation_preserves_length_to_table_precision(v in safe_vec(), f in any_facing()) {
        let r = v.rotate(f);
        let (a, b) = (raw_len(v), raw_len(r));
        // Table entries are off by at most half a unit of 1/65536: a relative
        // error below 2^-16 per component, plus the output rounding.
        prop_assert!((a - b).abs() <= a / 30_000 + 2, "{} vs {}", a, b);
    }

    #[test]
    fn rotation_composes_up_to_table_precision(v in map_vec(), f in any_facing(), g in any_facing()) {
        let two_steps = v.rotate(f).rotate(g);
        let one_step = v.rotate(Facing::new(f.raw().wrapping_add(g.raw())));
        let tolerance = raw_len(v) / 15_000 + 3;
        prop_assert!((two_steps.x - one_step.x).raw().abs() as i128 <= tolerance);
        prop_assert!((two_steps.y - one_step.y).raw().abs() as i128 <= tolerance);
        // And turning back returns (almost) the start.
        let back = v.rotate(f).rotate(Facing::new(f.raw().wrapping_neg()));
        prop_assert!((back.x - v.x).raw().abs() as i128 <= tolerance);
        prop_assert!((back.y - v.y).raw().abs() as i128 <= tolerance);
    }

    // ---- misc -----------------------------------------------------------------------------------------

    #[test]
    fn component_wise_helpers(a in any_vec(), b in any_vec(), c in any_vec()) {
        prop_assert!(a.min(b).x <= a.x && a.min(b).y <= b.y);
        prop_assert!(a.max(b).x >= a.x && a.max(b).y >= b.y);
        let r = a.clamp(b.min(c), b.max(c));
        prop_assert!(r.x >= b.min(c).x && r.x <= b.max(c).x);
        prop_assert!(r.y >= b.min(c).y && r.y <= b.max(c).y);
        prop_assert_eq!(a.abs().x, a.x.abs());
        prop_assert_eq!(a.cell(), (a.x.cell(), a.y.cell()));
    }

    #[test]
    fn nothing_panics_for_any_input(a in any_vec(), b in any_vec(), k in any_fx(), n in any::<i64>(), f in any_facing()) {
        let _ = (a + b, a - b, -a, a * k, k * a, a / k, a.scale(k), a.mul_int(n), a.div_int(n));
        let _ = (a.dot(b), a.cross(b), a.length_squared(), a.length(), a.distance(b), a.distance_squared(b));
        let _ = (a.normalize_or_zero(), a.with_length(k), a.clamp_length(k), a.perp(), a.abs());
        let _ = (a.lerp(b, k), a.facing(), a.rotate(f), a.cell(), a.min(b), a.max(b), a.clamp(b, a));
        let _ = (Vec2::from_facing(f, k), format!("{a:?}"));
        let mut c = a;
        c += b;
        c -= b;
        c *= k;
        c /= k;
    }
}
