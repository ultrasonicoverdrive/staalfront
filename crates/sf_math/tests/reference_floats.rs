//! Cross-checks of the integer-only implementation against `f64`.
//!
//! This is the one place where floats are used to *judge* sf_math: they live
//! here in `tests/` (outside the scanned `src/`), never feed back into the
//! library and never into the golden hashes (`f64::sin` itself may differ in
//! the last bit between platforms; every assertion below has a margin that
//! makes that irrelevant).
#![allow(
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::cast_precision_loss
)]

use core::f64::consts::TAU;
use sf_math::render_bridge::{
    METRES_PER_CELL, cells_to_metres, fx_from_f32, fx_to_f32, metres_to_cells,
    vec2_cells_to_metres, vec2_from_f32, vec2_metres_to_cells, vec2_to_f32,
};
use sf_math::{CELL_METRES, Facing, Fx, Rng, Vec2};

fn to_f64(v: Fx) -> f64 {
    v.raw() as f64 / 65536.0
}

#[test]
fn trig_tables_equal_the_correctly_rounded_f64_values() {
    for f in 0..=255u8 {
        let angle = f as f64 * TAU / 256.0;
        let want_sin = (angle.sin() * 65536.0).round() as i64;
        let want_cos = (angle.cos() * 65536.0).round() as i64;
        let face = Facing::new(f);
        assert_eq!(face.sin().raw(), want_sin, "sin of facing {f}");
        assert_eq!(face.cos().raw(), want_cos, "cos of facing {f}");
    }
}

#[test]
fn facing_agrees_with_atan2_away_from_the_halfway_boundaries() {
    let check = |x: i64, y: i64| {
        let Some(got) = Facing::from_xy(Fx::from_raw(x), Fx::from_raw(y)) else {
            assert!(x == 0 && y == 0);
            return;
        };
        // Angle in steps, in [0, 256).
        let t = (y as f64).atan2(x as f64) / TAU * 256.0;
        let t = t.rem_euclid(256.0);
        let nearest = (t + 0.5).floor() as i64 % 256;
        let distance_to_boundary = ((t - t.floor()) - 0.5).abs();
        let diff = (got.raw() as i64 - nearest).rem_euclid(256);
        if distance_to_boundary > 0.01 {
            assert_eq!(diff, 0, "({x}, {y}): t = {t}, got {}", got.raw());
        } else {
            assert!(diff == 0 || diff == 1 || diff == 255, "({x}, {y}): t = {t}");
        }
    };
    // Every small vector around the origin ...
    for x in -100..=100 {
        for y in -100..=100 {
            check(x, y);
        }
    }
    // ... and random ones at every magnitude up to the largest.
    let mut rng = Rng::new(0xa7a2);
    for _ in 0..50_000 {
        let bits = rng.below(63) + 1;
        let mask = (1u64 << bits) - 1;
        let x = (rng.next_u64() & mask) as i64 * if rng.flip() { 1 } else { -1 };
        let y = (rng.next_u64() & mask) as i64 * if rng.flip() { 1 } else { -1 };
        check(x, y);
    }
}

#[test]
fn sqrt_and_length_match_f64_to_half_a_unit() {
    let mut rng = Rng::new(0x5171);
    for _ in 0..50_000 {
        let bits = rng.below(63) + 1;
        let raw = (rng.next_u64() & ((1u64 << bits) - 1)) as i64;
        let want = ((raw as f64) * 65536.0).sqrt();
        let got = Fx::from_raw(raw).sqrt().raw() as f64;
        // f64 has 53 bits: its own error is far below the 0.5 allowed.
        assert!(
            (got - want).abs() <= 0.5 + 1e-3,
            "sqrt of raw {raw}: {got} vs {want}"
        );
    }
    for _ in 0..50_000 {
        let bits = rng.below(62) + 1;
        let mask = (1u64 << bits) - 1;
        let centre = (mask / 2) as i64;
        let v = Vec2::new(
            Fx::from_raw((rng.next_u64() & mask) as i64 - centre),
            Fx::from_raw((rng.next_u64() & mask) as i64 - centre),
        );
        let want = (v.x.raw() as f64).hypot(v.y.raw() as f64);
        let got = v.length().raw() as f64;
        assert!(
            (got - want).abs() <= 0.5 + want * 1e-15,
            "{v:?}: {got} vs {want}"
        );
    }
}

#[test]
fn rotation_matches_f64_to_table_precision() {
    let mut rng = Rng::new(0x707);
    for _ in 0..20_000 {
        let v = Vec2::new(
            Fx::from_raw(rng.range_i32(-(1 << 24), 1 << 24) as i64),
            Fx::from_raw(rng.range_i32(-(1 << 24), 1 << 24) as i64),
        );
        let f = rng.facing();
        let r = v.rotate(f);
        let a = f.raw() as f64 * TAU / 256.0;
        let (x, y) = (to_f64(v.x), to_f64(v.y));
        let want = (x * a.cos() - y * a.sin(), x * a.sin() + y * a.cos());
        let len = x.hypot(y);
        // Table error per entry is at most 2^-17, plus one output rounding.
        let tol = len * 2.0 * 7.7e-6 + 2.0 / 65536.0;
        assert!((to_f64(r.x) - want.0).abs() <= tol, "{v:?} by {f:?}");
        assert!((to_f64(r.y) - want.1).abs() <= tol, "{v:?} by {f:?}");
    }
}

#[test]
fn constants_from_ratio_match_f64() {
    let mut rng = Rng::new(0xca7);
    for _ in 0..20_000 {
        let num = rng.range_i32(-1_000_000, 1_000_000) as i64;
        let den = rng.range_i32(1, 100_000) as i64;
        let got = Fx::from_ratio(num, den).raw() as f64;
        let want = num as f64 / den as f64 * 65536.0;
        assert!((got - want).abs() <= 0.5 + 1e-6, "{num}/{den}");
    }
}

// ---- the render bridge --------------------------------------------------------

#[test]
fn bridge_is_exact_for_everything_on_a_map() {
    // All multiples of a lepton on a 256-cell map, and a spread of raw
    // values up to 2^24 (the largest coordinate), survive a round trip.
    for l in (-256 * 256..=256 * 256).step_by(7) {
        let v = Fx::from_leptons(l);
        assert_eq!(fx_to_f32(v) * 256.0, l as f32);
        assert_eq!(fx_from_f32(fx_to_f32(v)), v);
    }
    let mut rng = Rng::new(0xb41d9e);
    for _ in 0..50_000 {
        let v = Fx::from_raw(rng.range_i32(-(1 << 24), (1 << 24) + 1) as i64);
        assert_eq!(fx_from_f32(fx_to_f32(v)), v);
    }
    let p = Vec2::new(Fx::from_ratio(1, 2), Fx::from_ratio(-5, 4));
    assert_eq!(vec2_to_f32(p), [0.5, -1.25]);
    assert_eq!(vec2_from_f32([0.5, -1.25]), p);
}

#[test]
fn bridge_rounds_to_nearest_and_saturates() {
    assert_eq!(fx_from_f32(0.0), Fx::ZERO);
    assert_eq!(fx_from_f32(1.0), Fx::ONE);
    assert_eq!(fx_from_f32(-1.0), Fx::NEG_ONE);
    // Half a raw unit rounds away from zero.
    assert_eq!(fx_from_f32(0.5 / 65536.0).raw(), 1);
    assert_eq!(fx_from_f32(-0.5 / 65536.0).raw(), -1);
    assert_eq!(fx_from_f32(0.49 / 65536.0).raw(), 0);
    assert_eq!(fx_from_f32(0.51 / 65536.0).raw(), 1);
    assert_eq!(fx_from_f32(1.0 / 3.0).raw(), 21845);
    // A value just below one half of a unit must not round up.
    assert_eq!(fx_from_f32(0.499_999_97 / 65536.0).raw(), 0);
    // Non-finite and huge inputs.
    assert_eq!(fx_from_f32(f32::NAN), Fx::ZERO);
    assert_eq!(fx_from_f32(f32::INFINITY), Fx::MAX);
    assert_eq!(fx_from_f32(f32::NEG_INFINITY), Fx::MIN);
    assert_eq!(fx_from_f32(1e30), Fx::MAX);
    assert_eq!(fx_from_f32(-1e30), Fx::MIN);
    assert_eq!(fx_from_f32(f32::MAX), Fx::MAX);
}

// ---- the cell size (B12) -------------------------------------------------------

#[test]
fn a_cell_is_four_metres_and_the_scale_is_exact() {
    assert_eq!(CELL_METRES, 4);
    assert_eq!(METRES_PER_CELL, 4.0);
    // The whole 256-cell map is 1,024 m, and every position on it converts to
    // metres and back without a bit lost.
    assert_eq!(cells_to_metres(Fx::from_int(256)), 1024.0);
    assert_eq!(
        vec2_cells_to_metres(Vec2::from_ints(1, 2)),
        [4.0, 8.0],
        "(1, 2) cells"
    );
    let mut rng = Rng::new(0x4_c311);
    for _ in 0..50_000 {
        let v = Fx::from_raw(rng.range_i32(-(1 << 24), (1 << 24) + 1) as i64);
        assert_eq!(cells_to_metres(v), fx_to_f32(v) * 4.0);
        assert_eq!(metres_to_cells(cells_to_metres(v)), v);
    }
    let p = Vec2::new(Fx::from_ratio(1, 2), Fx::from_ratio(-5, 4));
    assert_eq!(vec2_cells_to_metres(p), [2.0, -5.0]);
    assert_eq!(vec2_metres_to_cells([2.0, -5.0]), p);
}
