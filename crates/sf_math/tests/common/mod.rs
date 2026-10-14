//! Shared proptest strategies. Each test binary uses a subset.
#![allow(dead_code)]

use proptest::prelude::*;
use sf_math::{Facing, Fx, Vec2};

/// Interesting edge values first, then every magnitude class: the full
/// `i64` range (mostly enormous, so saturation paths run), the large safe
/// range, map-sized values and tiny values.
pub fn any_fx() -> impl Strategy<Value = Fx> {
    prop_oneof![
        8 => any::<i64>().prop_map(Fx::from_raw),
        4 => safe_fx(),
        2 => map_fx(),
        1 => (-300i64..300).prop_map(Fx::from_raw),
        2 => prop_oneof![
            Just(Fx::MIN),
            Just(Fx::MAX),
            Just(Fx::ZERO),
            Just(Fx::ONE),
            Just(Fx::NEG_ONE),
            Just(Fx::EPSILON),
            Just(Fx::from_raw(-1)),
            Just(Fx::from_raw(i64::MIN + 1)),
            Just(Fx::from_raw(i64::MAX - 1)),
            Just(Fx::from_raw(1 << 32)),
        ],
    ]
}

/// `|v| < 2^24` cells: sums and differences never saturate.
pub fn safe_fx() -> impl Strategy<Value = Fx> {
    (-(1i64 << 40)..(1i64 << 40)).prop_map(Fx::from_raw)
}

/// `|v| <= 256` cells, the size of the largest map: products never saturate.
pub fn map_fx() -> impl Strategy<Value = Fx> {
    (-(1i64 << 24)..=(1i64 << 24)).prop_map(Fx::from_raw)
}

pub fn any_vec() -> impl Strategy<Value = Vec2> {
    (any_fx(), any_fx()).prop_map(|(x, y)| Vec2::new(x, y))
}

pub fn safe_vec() -> impl Strategy<Value = Vec2> {
    (safe_fx(), safe_fx()).prop_map(|(x, y)| Vec2::new(x, y))
}

pub fn map_vec() -> impl Strategy<Value = Vec2> {
    (map_fx(), map_fx()).prop_map(|(x, y)| Vec2::new(x, y))
}

pub fn any_facing() -> impl Strategy<Value = Facing> {
    any::<u8>().prop_map(Facing::new)
}

/// `x^2 + y^2` of the raw components, exact.
pub fn norm_sq(v: Vec2) -> u128 {
    let x = v.x.raw().unsigned_abs() as u128;
    let y = v.y.raw().unsigned_abs() as u128;
    x * x + y * y
}

/// The exact Euclidean length in raw units, rounded to nearest, without the
/// `i64` saturation of `Vec2::length` (up to about 2^64).
pub fn exact_len(v: Vec2) -> u128 {
    let n = norm_sq(v);
    let r = n.isqrt();
    if n - r * r > r { r + 1 } else { r }
}

/// Checks `r` is the integer square root of `n` rounded to nearest:
/// `-r < n - r^2 <= r`.
pub fn is_rounded_sqrt(n: u128, r: u128) -> bool {
    let sq = r * r;
    if n >= sq { n - sq <= r } else { sq - n < r }
}
