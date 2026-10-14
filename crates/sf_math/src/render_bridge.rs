//! The one place where `sf_math` touches floating point: converting to and
//! from `f32` at the boundary with rendering and input.
//!
//! Rendering uses `glam` and floats (PLAN.md section 1.0); the simulation uses
//! fixed point only. These helpers are for the *presentation side*:
//!
//! * [`fx_to_f32`] / [`vec2_to_f32`] turn a simulation value into something to
//!   draw. Every in-map coordinate (raw value up to 2^24, i.e. 256 cells)
//!   converts exactly.
//! * [`fx_from_f32`] / [`vec2_from_f32`] turn a rendered or picked position
//!   (a mouse ray hit, say) into a simulation value *before* it is put into a
//!   command. The conversion is platform independent too (IEEE multiply by a
//!   power of two, then a saturating cast), but nothing in the simulation may
//!   call it; the result must travel through the command stream.
//!
//! Enforcement: this file is the single exception in `sf_math` to the float
//! bans. `scripts/check-sim-determinism.sh` allows floats here and nowhere
//! else in `crates/sf_math/src`, and forbids any mention of this module in
//! `crates/sf_sim/src`; clippy's float lints are switched off for this module
//! only (below).
#![allow(clippy::float_arithmetic, clippy::cast_precision_loss)]

use crate::fx::{FRAC_BITS, Fx};
use crate::vec2::Vec2;

const SCALE: f32 = (1u32 << FRAC_BITS) as f32;

/// An [`Fx`] as `f32`, for rendering only. Exact while the magnitude is below
/// 256 cells (the largest map), rounded to 24 significant bits beyond that.
#[inline]
pub fn fx_to_f32(v: Fx) -> f32 {
    v.raw() as f32 / SCALE
}

/// A [`Vec2`] as `[x, y]` in `f32`, for rendering only.
#[inline]
pub fn vec2_to_f32(v: Vec2) -> [f32; 2] {
    [fx_to_f32(v.x), fx_to_f32(v.y)]
}

/// An `f32` as the nearest [`Fx`] (halves away from zero). NaN gives zero and
/// out-of-range values saturate. For input handling only, never for
/// simulation code.
#[inline]
pub fn fx_from_f32(v: f32) -> Fx {
    let scaled = v * SCALE; // exact: a power of two, up to overflow to inf
    // `as` truncates toward zero, saturates, and maps NaN to 0 in Rust.
    let whole = scaled as i64;
    // Exact: below 2^24 both operands are small integers or nearby values,
    // above it every f32 is already a whole number (difference 0).
    let diff = scaled - whole as f32;
    let nearest = if diff >= 0.5 {
        whole.saturating_add(1)
    } else if diff <= -0.5 {
        whole.saturating_sub(1)
    } else {
        whole
    };
    Fx::from_raw(nearest)
}

/// A `[x, y]` pair of `f32` as the nearest [`Vec2`] (see [`fx_from_f32`]).
#[inline]
pub fn vec2_from_f32(v: [f32; 2]) -> Vec2 {
    Vec2::new(fx_from_f32(v[0]), fx_from_f32(v[1]))
}
