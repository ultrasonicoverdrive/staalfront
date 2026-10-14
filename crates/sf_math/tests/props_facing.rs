//! Property tests for `Facing`: wrap-around turning, the trig tables and the
//! vector-to-facing conversion.

mod common;

use common::*;
use proptest::prelude::*;
use sf_math::{Facing, Fx, Vec2};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4000))]

    #[test]
    fn turning_is_modular_arithmetic(f in any_facing(), a in any::<i8>(), b in any::<i8>()) {
        prop_assert_eq!(f.turn(a).turn(b), f.turn((a as i16 + b as i16) as i8));
        prop_assert_eq!(f.turn(0), f);
        prop_assert_eq!(f.opposite().opposite(), f);
        prop_assert_eq!(f.opposite(), f.turn(127).turn(1));
        prop_assert_eq!(f.turn(a).raw(), f.raw().wrapping_add(a as u8));
    }

    #[test]
    fn signed_diff_is_the_shortest_signed_turn(f in any_facing(), g in any_facing()) {
        let d = f.signed_diff(g);
        prop_assert_eq!(f.turn(d), g);
        prop_assert_eq!(f.abs_diff(g), f.signed_diff(g).unsigned_abs().min(128));
        prop_assert_eq!(f.abs_diff(g), g.abs_diff(f));
        prop_assert!(f.abs_diff(g) <= 128);
        // Antisymmetric except across a half turn, where both read -128.
        if f.abs_diff(g) != 128 {
            prop_assert_eq!(g.signed_diff(f), -d);
        } else {
            prop_assert_eq!((d, g.signed_diff(f)), (i8::MIN, i8::MIN));
        }
        prop_assert_eq!(f.abs_diff(f), 0);
    }

    #[test]
    fn turn_toward_never_overshoots_and_makes_progress(f in any_facing(), t in any_facing(), step in any::<u8>()) {
        let r = f.turn_toward(t, step);
        // Moves at most `step`, never away, never past the target.
        prop_assert!(f.abs_diff(r) <= step.min(128));
        prop_assert!(r.abs_diff(t) <= f.abs_diff(t));
        if f.abs_diff(t) <= step {
            prop_assert_eq!(r, t);
        } else {
            prop_assert_eq!(f.abs_diff(r), step);
            prop_assert_eq!(r.abs_diff(t), f.abs_diff(t) - step);
        }
        prop_assert_eq!(f.turn_toward(t, 0), f);
        prop_assert_eq!(f.turn_toward(f, step), f);
    }

    #[test]
    fn repeated_turning_converges(f in any_facing(), t in any_facing(), step in 1u8..=255) {
        let mut cur = f;
        let rounds = 128u32.div_ceil(step as u32) + 1;
        for _ in 0..rounds {
            cur = cur.turn_toward(t, step);
        }
        prop_assert_eq!(cur, t);
    }

    #[test]
    fn sin_and_cos_obey_the_symmetries_exactly(f in any_facing()) {
        let (s, c) = f.sin_cos();
        prop_assert_eq!(s, f.sin());
        prop_assert_eq!(c, f.cos());
        // Quarter-turn shifts.
        prop_assert_eq!(f.turn(64).sin(), c);
        prop_assert_eq!(f.turn(64).cos(), -s);
        prop_assert_eq!(f.opposite().sin(), -s);
        prop_assert_eq!(f.opposite().cos(), -c);
        // Mirroring.
        let m = Facing::new(f.raw().wrapping_neg());
        prop_assert_eq!(m.sin(), -s);
        prop_assert_eq!(m.cos(), c);
        prop_assert_eq!(Facing::new(64u8.wrapping_sub(f.raw())).sin(), c);
        prop_assert_eq!(Facing::new(64u8.wrapping_sub(f.raw())).cos(), s);
    }

    #[test]
    fn the_unit_vector_has_unit_length(f in any_facing()) {
        let v = f.to_vec2();
        prop_assert_eq!(v, Vec2::new(f.cos(), f.sin()));
        let len = v.length().raw();
        // Each component is within half a unit of the true value.
        prop_assert!((len - 65536).abs() <= 1, "len {}", len);
        let sq = v.x.raw() * v.x.raw() + v.y.raw() * v.y.raw();
        prop_assert!((sq - (1 << 32)).abs() <= 1 << 17);
    }

    #[test]
    fn every_nonzero_vector_has_a_facing_and_reflection_is_a_half_turn(x in any_fx(), y in any_fx()) {
        let f = Facing::from_xy(x, y);
        prop_assert_eq!(f.is_none(), x.is_zero() && y.is_zero());
        prop_assert_eq!(f, Vec2::new(x, y).facing());
        prop_assert_eq!(f, Facing::from_vec2(Vec2::new(x, y)));
        // Reflection through the origin is a half turn.
        if let Some(f) = f
            && x != Fx::MIN
            && y != Fx::MIN
        {
            prop_assert_eq!(Facing::from_xy(-x, -y), Some(f.opposite()));
        }
    }

    #[test]
    fn symmetric_vectors_give_symmetric_facings(x in 1i64..(1 << 40), y in 1i64..(1 << 40)) {
        let (x, y) = (Fx::from_raw(x), Fx::from_raw(y));
        // First quadrant vector p; its mirror images.
        let f = Facing::from_xy(x, y).unwrap().raw();
        prop_assert!(f <= 64);
        // Swapping x and y reflects across the diagonal: f -> 64 - f, up to
        // the tie rule on the exact bisector (round toward the larger facing).
        let g = Facing::from_xy(y, x).unwrap().raw();
        prop_assert!((f as i16 + g as i16 - 64).abs() <= 1, "{} {}", f, g);
        // The mirror across the x axis negates the facing.
        let m = Facing::from_xy(x, -y).unwrap().raw();
        prop_assert!((f as i16 + m as i16 - 256).abs() <= 1 || f == 0 && m == 0, "{} {}", f, m);
        // Rotating by a quarter turn adds 64 exactly (the reduction is exact).
        prop_assert_eq!(Facing::from_xy(-y, x).unwrap().raw(), f.wrapping_add(64));
        prop_assert_eq!(Facing::from_xy(-x, -y).unwrap().raw(), f.wrapping_add(128));
        prop_assert_eq!(Facing::from_xy(y, -x).unwrap().raw(), f.wrapping_add(192));
    }

    #[test]
    fn neighbouring_facings_stay_distinct_at_any_radius(r in 256i64..(1 << 30), a in any_facing()) {
        // Unit vectors at radius r (>= 256 raw units resolves each step)
        // classify back to themselves and to their neighbour.
        let v = Vec2::from_facing(a, Fx::from_raw(r));
        let w = Vec2::from_facing(a.turn(1), Fx::from_raw(r));
        prop_assert_eq!(v.facing(), Some(a));
        prop_assert_eq!(w.facing(), Some(a.turn(1)));
    }
}

/// The exhaustive checks: only 256 facings, so test them all.
#[test]
fn every_facing_round_trips_through_its_vector() {
    for f in 0..=255u8 {
        let face = Facing::new(f);
        assert_eq!(Facing::from_vec2(face.to_vec2()), Some(face), "facing {f}");
    }
}

#[test]
fn table_is_a_unit_circle_within_rounding() {
    for f in 0..=255u8 {
        let (s, c) = Facing::new(f).sin_cos();
        let (s, c) = (s.raw(), c.raw());
        // s^2 + c^2 = 2^32, each of s and c off by at most half a unit.
        let err = s * s + c * c - (1 << 32);
        assert!(err.abs() <= 1 << 17, "facing {f}: err {err}");
        assert!(s.abs() <= 65536 && c.abs() <= 65536);
    }
}

#[test]
fn table_is_strictly_increasing_on_each_quadrant() {
    for f in 0..64u8 {
        assert!(Facing::new(f).sin() < Facing::new(f + 1).sin());
        assert!(Facing::new(f).cos() > Facing::new(f + 1).cos());
    }
}

/// Bisector exactness: the direction exactly between two neighbouring
/// facings, built from the table, classifies to the larger one; just below
/// it classifies to the smaller.
#[test]
fn halfway_directions_round_up_and_just_below_rounds_down() {
    for f in 0..=255u8 {
        let a = Facing::new(f).to_vec2();
        let b = Facing::new(f.wrapping_add(1)).to_vec2();
        // Scale up so the sum has plenty of resolution.
        let k = 1_000_000;
        let mid = (a + b).mul_int(k);
        assert_eq!(
            mid.facing(),
            Some(Facing::new(f.wrapping_add(1))),
            "above {f}"
        );
        // Nudge toward the smaller facing by an angle far below a step but
        // above the table error: add a small multiple of a.
        let below = (a.mul_int(3) + b.mul_int(2)).mul_int(k);
        assert_eq!(below.facing(), Some(Facing::new(f)), "below {f}");
        let above = (a.mul_int(2) + b.mul_int(3)).mul_int(k);
        assert_eq!(
            above.facing(),
            Some(Facing::new(f.wrapping_add(1))),
            "above {f}"
        );
    }
}
