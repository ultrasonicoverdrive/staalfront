//! Fixed-point numbers, vectors, angles and the deterministic RNG.
//!
//! `sf_math` is the numeric foundation of Staalfront. The simulation
//! (`sf_sim`) computes with nothing else, so that a tick produces
//! bit-identical state on every machine, in debug and release, which replays,
//! saves and lockstep multiplayer depend on (PLAN.md decision B3). The
//! renderer shares the crate and converts at the edge via `render_bridge`
//! (behind the `render` cargo feature, see below).
//!
//! # What is in here
//!
//! | Item | Purpose |
//! |---|---|
//! | [`Fx`] | the fixed-point scalar (`Q47.16` in an `i64`) |
//! | [`Vec2`] | 2D vector of `Fx`: lengths, normalising, rotation |
//! | [`Facing`] | one of 256 directions; exact sin/cos, `atan2` replacement, turning |
//! | [`Rng`] | PCG32 with unbiased range helpers and serialisable state |
//! | [`hash::Fnv1a64`] | explicit little-endian FNV-1a for state checksums |
//! | [`CELL_METRES`] | the scale between a cell and the metres of the presentation (4) |
//! | [`TICKS_PER_SECOND`], [`TICK_MILLIS`] | the 20 Hz tick (B3) |
//! | `render_bridge` | the only float code: `Fx`/`Vec2` to and from single-precision floats; exists only with the `render` feature |
//!
//! # The number format
//!
//! [`Fx`] is a binary fixed-point number: an `i64` holding `value * 2^16`
//! (`Q47.16`). **The unit is one cell** (PLAN.md, the Coordinates section).
//!
//! | | |
//! |---|---|
//! | Resolution | 1/65536 cell. One *lepton* (1/256 cell, [`LEPTONS_PER_CELL`] per cell) is exactly 256 raw units, so leptons are exact and there are 256 sub-steps inside each of them |
//! | Range | `-2^47 ..= 2^47 - 1/65536` cells, about plus or minus 140 trillion |
//! | Largest map | 256 cells is a raw value of 2^24, so a map coordinate uses 25 of the 63 magnitude bits |
//!
//! Sizing: a squared distance across the whole 256 x 256 map is below
//! 2^17 cells squared (2^33 raw), a speed of 10 cells/s is 32768 raw units per
//! 20 Hz tick, and the product of two coordinates is below 2^16 cells squared.
//! All of that sits about 30 bits below the saturation point, so ordinary game
//! values never get near the limits. Every multiplication and division goes
//! through an exact 128-bit intermediate, so the only rounding is the final
//! one.
//!
//! # Overflow, division and rounding policy
//!
//! This is the contract the rest of the engine can rely on.
//!
//! * **Every operation is total.** `+ - * / neg abs`, the vector operations and
//!   the helpers never panic, never wrap and never invoke undefined behaviour,
//!   for any input bits. They are written with explicit saturating arithmetic
//!   (not `+`/`-`/`*` on the raw integers), so debug builds (overflow checks on)
//!   and release builds (off) compute exactly the same bits. A test runs the
//!   golden sequence in both profiles in CI.
//! * **Overflow saturates** to [`Fx::MAX`] / [`Fx::MIN`] (`-MIN` is `MAX`).
//!   `checked_*` variants return `None` instead where a caller wants to detect
//!   it. Saturation is an escape hatch, not a feature: simulation code should
//!   stay far from the limits (see the sizing above).
//! * **Division by zero does not panic**: `x / 0` is `MAX` for positive `x`,
//!   `MIN` for negative `x`, and `0 / 0` is zero. A panic in a lockstep
//!   simulation would crash every peer on bad input; a defined result keeps
//!   all peers identical. [`Fx::checked_div`] reports it.
//! * **Rounding is to nearest, ties away from zero** for multiplication,
//!   division, ratios, rotation and normalising. This makes them odd-symmetric
//!   (`(-a) * b == -(a * b)`) so mirrored situations behave mirrored. Square
//!   roots and lengths are correctly rounded (error at most half a raw unit).
//!   `floor`, `ceil`, `round`, `trunc` are documented individually.
//! * **No floating point** anywhere in the arithmetic. Constants are built
//!   with [`Fx::from_ratio`], [`Fx::from_int`] or [`Fx::from_leptons`] (all
//!   `const fn`).
//!
//! # Angles
//!
//! A [`Facing`] is a `u8`: a full turn in 256 steps, wrapping naturally. Facing
//! `0` is `+x` and facings increase toward `+y` (the `atan2(y, x)` convention).
//! The sine table is generated at compile time with integer arithmetic only
//! (see the `trig` module source) from the half-angle identities, so there are
//! no magic numbers to trust; quadrant symmetries hold exactly. The vector to
//! facing conversion ([`Facing::from_xy`]) uses exact integer cross products
//! against the bisectors of neighbouring table directions, so it is exact for
//! any vector magnitude.
//!
//! # Determinism and how it is enforced
//!
//! * `#![no_std]`: no std hash collections, no clock, no I/O and no threads
//!   are even reachable. Iteration-order hazards cannot arise here.
//! * `scripts/check-sim-determinism.sh` scans this crate like `sf_sim`: no
//!   float type tokens, no float literals, no float-conversion helpers. The
//!   only file exempt is `render_bridge.rs`, which exists to convert at the
//!   rendering boundary; the script forbids `sf_sim` from naming it. Test code
//!   that needs floats (comparing the tables with the standard library's
//!   floating-point sine, say) lives in `tests/`, outside the scanned `src/`.
//! * `clippy::float_arithmetic`, `float_cmp` and `cast_precision_loss` are
//!   denied for the whole crate except `render_bridge`.
//! * `render_bridge` is compiled only with the `render` cargo feature, which
//!   `sf_sim` never enables. Building `sf_sim` on its own (`cargo build -p
//!   sf_sim`, `cargo test -p sf_sim`) therefore leaves the module out, so a
//!   use of it is a compile error. Cargo unifies features across one build, so
//!   a whole-workspace build that also contains a renderer has the module
//!   present; the lint script and the isolated `-p sf_sim` CI step cover that
//!   case.
//! * Every integer width is explicit (`i64`, `i128`, `u32`, ...) and `usize`
//!   never enters arithmetic, so 32-bit and 64-bit targets agree.
//! * `tests/golden.rs` replays a fixed pseudo-random sequence of 100000 mixed
//!   operations, hashes every result bit and compares against a committed
//!   value. CI runs it on Linux and Windows (and in release mode), so the
//!   same value passing on both is the proof of bit-identical behaviour.
//!
//! # Example
//!
//! ```
//! use sf_math::{Facing, Fx, Rng, Vec2};
//!
//! // A unit at the centre of cell (10, 20), moving 3/4 cell per tick along
//! // a heading found from a target, with a tiny random jitter.
//! let mut rng = Rng::new(2026);
//! let pos = Vec2::new(Fx::from_int(10) + Fx::HALF, Fx::from_int(20) + Fx::HALF);
//! let target = Vec2::from_ints(14, 23);
//! let heading = (target - pos).facing().unwrap();
//! let step = Vec2::from_facing(heading, Fx::from_ratio(3, 4));
//! let jitter = Vec2::splat(rng.fx_range(Fx::ZERO, Fx::LEPTON));
//! let next = pos + step + jitter;
//! assert_eq!(next.cell().0, 11);
//! assert!(next.distance(target) < pos.distance(target));
//! ```

#![no_std]

#[cfg(test)]
extern crate std;

pub mod facing;
pub mod fx;
pub mod hash;
mod int;
#[cfg(feature = "render")]
pub mod render_bridge;
pub mod rng;
mod trig;
pub mod vec2;

pub use facing::Facing;
pub use fx::{FRAC_BITS, Fx, LEPTONS_PER_CELL};
pub use rng::{Rng, RngState};
pub use vec2::Vec2;

/// Metres in one cell: the scale between the simulation and everything that
/// is drawn, heard or measured (PLAN.md decision B12).
///
/// Simulation positions are in cells ([`Fx::ONE`] is one cell). The terrain,
/// the camera, the models and the sounds are in metres, with four terrain
/// samples along every cell side. This is the one number that connects the
/// two; the presentation applies it at the boundary
/// (`render_bridge::cells_to_metres`), the simulation never reads it, so the
/// checksums do not depend on it. It is an integer so that map sizes can be
/// counted exactly (`Map::size_in_cells` in `sf_data`); the renderer's single-precision
/// copy is `render_bridge::METRES_PER_CELL`.
pub const CELL_METRES: u32 = 4;

/// Simulation steps per second (PLAN.md decision B3). It lives here, next to
/// the number format, because the simulation and the unit definitions in
/// `sf_data` (durations in ticks, speed per tick) need the same 20 and neither
/// depends on the other. `sf_sim` re-exports it. (`sf_engine` has its own copy,
/// `SIM_HZ`, as it depends on neither.)
pub const TICKS_PER_SECOND: u32 = 20;

/// Milliseconds of game time per tick: exactly `1000 / TICKS_PER_SECOND`.
pub const TICK_MILLIS: u32 = 1000 / TICKS_PER_SECOND;
