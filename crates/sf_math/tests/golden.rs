//! Cross-platform bit-identity proof for `sf_math` (WP0.2 acceptance
//! criterion, PLAN.md section 3 phase 0 and section 6).
//!
//! # How this proves anything
//!
//! Every test here replays a fixed, seeded computation, feeds the *raw bits*
//! of every result into a self-implemented FNV-1a hash
//! ([`sf_math::hash::Fnv1a64`], fixed-width little-endian, no `std::hash`),
//! and compares the final value with a constant committed in this file.
//! The constants were produced once on Linux. CI runs `cargo test` on Linux
//! and on Windows (and `cargo test --release -p sf_math` on both), so **the
//! same golden constants passing on both operating systems, in debug and in
//! release, is the proof that `sf_math` is bit-identical everywhere**. If a
//! platform ever computed a single result differently (a wrapped overflow, a
//! rounding difference, a float sneaking in), its hash would differ and the
//! test would fail there.
//!
//! The headline test is [`mixed_sequence_of_100k_operations_is_bit_identical`].
//! The smaller tests pin the individual parts (RNG, trig tables, facing
//! conversion, fixed-point sweeps) so that a failure points at the culprit.
//!
//! # Changing a golden value
//!
//! A golden value may only change when the *specified behaviour* of sf_math
//! changes on purpose (new rounding rule, new table, different RNG output).
//! That invalidates every saved game and replay, so it is a deliberate,
//! reviewed act, never a fix to make a test pass.

use sf_math::hash::Fnv1a64;
use sf_math::{Facing, Fx, Rng, Vec2};

// ---- helpers --------------------------------------------------------------------

fn opt(h: &mut Fnv1a64, v: Option<Fx>) {
    match v {
        Some(v) => {
            h.write_u8(1);
            h.write_fx(v);
        }
        None => h.write_u8(0),
    }
}

/// Operands of every magnitude, including the awkward ones: tiny, map-sized,
/// log-uniform, full-range, the extremes, and ratios (including `x / 0`).
fn operand(rng: &mut Rng) -> Fx {
    match rng.below(9) {
        0 => Fx::from_raw(rng.range_inclusive_i32(-300, 300) as i64),
        1 | 2 => Fx::from_raw(rng.range_inclusive_i32(-(1 << 24), 1 << 24) as i64),
        3 | 4 => Fx::from_raw((rng.next_u64() as i64) >> rng.below(40)),
        5 => Fx::from_raw(rng.next_u64() as i64),
        6 => {
            const EDGES: [Fx; 9] = [
                Fx::MIN,
                Fx::MAX,
                Fx::ZERO,
                Fx::ONE,
                Fx::NEG_ONE,
                Fx::EPSILON,
                Fx::HALF,
                Fx::from_raw(i64::MIN + 1),
                Fx::from_raw(i64::MAX - 1),
            ];
            EDGES[rng.below(9) as usize]
        }
        7 => Fx::from_ratio(
            rng.range_inclusive_i32(-1000, 1000) as i64,
            rng.range_inclusive_i32(-50, 50) as i64, // includes a zero divisor
        ),
        _ => Fx::from_leptons(rng.range_inclusive_i32(-70_000, 70_000) as i64),
    }
}

fn vector(rng: &mut Rng) -> Vec2 {
    Vec2::new(operand(rng), operand(rng))
}

/// A vector small enough that products stay in range, to exercise the
/// non-saturating paths of rotation and the dot product.
fn map_vector(rng: &mut Rng) -> Vec2 {
    Vec2::new(
        Fx::from_raw(rng.range_inclusive_i32(-(1 << 24), 1 << 24) as i64),
        Fx::from_raw(rng.range_inclusive_i32(-(1 << 24), 1 << 24) as i64),
    )
}

// ---- the headline test --------------------------------------------------------------

const OPS: u32 = 100_000;
const CHECKPOINT_EVERY: u32 = 10_000;

/// FNV-1a hash after every 10000 operations; the last entry covers all
/// 100000. Several checkpoints make a divergence easy to localise.
const GOLDEN_CHECKPOINTS: [u64; 10] = [
    0x310c_1383_18ff_5175,
    0xb35d_5f05_f0ba_f18b,
    0x4985_ba90_4114_2349,
    0x311b_10ea_7af9_bac9,
    0xeff0_f1c5_efa3_55c1,
    0x6821_6780_7090_b150,
    0xaff5_8797_a6f8_bf58,
    0xf7a4_1aaf_28ba_721f,
    0xea33_e950_6c8f_1827,
    0x23c4_b855_bee4_884d,
];

/// 100000 mixed operations from a seeded generator: arithmetic with
/// saturation, division (including by zero), square roots, rounding, lerp,
/// vector math, lengths and normalising, rotation, sine/cosine, the vector
/// to facing conversion, turning, and draws from a second RNG (ranges,
/// shuffles, chances). Every result's raw bits are hashed.
fn mixed_sequence() -> [u64; 10] {
    let mut driver = Rng::new(0x57A1_F407);
    let mut subject = Rng::with_stream(0xC0FF_EE00_DEAD_BEEF, 77);
    let mut h = Fnv1a64::new();
    let mut checkpoints = [0u64; 10];

    for op in 0..OPS {
        let sel = driver.below(26);
        let (a, b, c) = (
            operand(&mut driver),
            operand(&mut driver),
            operand(&mut driver),
        );
        match sel {
            // -- scalar arithmetic
            0 => h.write_fx(a + b),
            1 => h.write_fx(a - b),
            2 => h.write_fx(a * b),
            3 => h.write_fx(a / b),
            4 => h.write_fx(-a),
            5 => h.write_fx(a.abs()),
            6 => {
                opt(&mut h, a.checked_add(b));
                opt(&mut h, a.checked_sub(b));
                opt(&mut h, a.checked_mul(b));
                opt(&mut h, a.checked_div(b));
                opt(&mut h, a.checked_neg());
            }
            7 => {
                h.write_fx(a.mul_div(b, c));
                h.write_fx(a.lerp(b, c));
                h.write_fx(a.mul_int(driver.next_u64() as i64 >> 20));
                h.write_fx(a.div_int(driver.range_inclusive_i32(-9, 9) as i64));
            }
            8 => {
                h.write_fx(a.min(b));
                h.write_fx(a.max(b));
                h.write_fx(a.clamp(b, c));
                h.write_fx(a.signum());
            }
            // -- square root and rounding
            9 => {
                h.write_fx(a.sqrt());
                h.write_fx(a.abs().sqrt());
                h.write_fx(a.square().sqrt());
            }
            10 => {
                h.write_fx(a.floor());
                h.write_fx(a.ceil());
                h.write_fx(a.round());
                h.write_fx(a.trunc());
                h.write_fx(a.fract());
                h.write_i64(a.floor_int());
                h.write_i64(a.ceil_int());
                h.write_i64(a.round_int());
                h.write_i64(a.trunc_int());
            }
            11 => {
                h.write_i32(a.cell());
                h.write_i64(a.to_leptons());
                h.write_u8(a.lepton_in_cell());
            }
            12 => {
                h.write_fx(Fx::from_ratio(a.raw(), b.raw()));
                h.write_fx(Fx::from_i64(a.raw()));
                h.write_fx(Fx::from_leptons(b.raw()));
            }
            // -- vectors
            13 => {
                let (v, w) = (vector(&mut driver), vector(&mut driver));
                h.write_vec2(v + w);
                h.write_vec2(v - w);
                h.write_vec2(-v);
                h.write_vec2(v * a);
                h.write_vec2(v / b);
                h.write_vec2(v.lerp(w, c));
            }
            14 => {
                let (v, w) = (vector(&mut driver), vector(&mut driver));
                h.write_fx(v.dot(w));
                h.write_fx(v.cross(w));
                h.write_fx(v.length_squared());
                h.write_fx(v.length());
                h.write_fx(v.distance(w));
                h.write_fx(v.distance_squared(w));
            }
            15 => {
                let v = vector(&mut driver);
                h.write_vec2(v.normalize_or_zero());
                h.write_vec2(v.with_length(a));
                h.write_vec2(v.clamp_length(b));
            }
            16 => {
                let (v, w) = (map_vector(&mut driver), map_vector(&mut driver));
                h.write_fx(v.dot(w));
                h.write_fx(v.cross(w));
                h.write_fx(v.length());
                h.write_vec2(v.normalize_or_zero());
                h.write_vec2(v.perp());
            }
            // -- angles
            17 => {
                let f = Facing::new(driver.next_u32() as u8);
                let v = map_vector(&mut driver);
                h.write_vec2(v.rotate(f));
                h.write_vec2(vector(&mut driver).rotate(f));
                h.write_vec2(Vec2::from_facing(f, a));
            }
            18 => {
                let f = Facing::new(driver.next_u32() as u8);
                let (s, co) = f.sin_cos();
                h.write_fx(s);
                h.write_fx(co);
                h.write_vec2(f.to_vec2());
                h.write_facing(f.opposite());
            }
            19 => {
                let v = vector(&mut driver);
                match v.facing() {
                    Some(f) => {
                        h.write_u8(1);
                        h.write_facing(f);
                    }
                    None => h.write_u8(0),
                }
                let m = map_vector(&mut driver);
                if let Some(f) = m.facing() {
                    h.write_facing(f);
                }
            }
            20 => {
                let f = Facing::new(driver.next_u32() as u8);
                let t = Facing::new(driver.next_u32() as u8);
                let step = driver.next_u32() as u8;
                h.write_facing(f.turn_toward(t, step));
                h.write_i32(f.signed_diff(t) as i32);
                h.write_u8(f.abs_diff(t));
                h.write_facing(f.turn(step as i8));
            }
            // -- the random generator under test
            21 => {
                h.write_u32(subject.next_u32());
                h.write_u64(subject.next_u64());
                h.write_u32(subject.below(driver.next_u32()));
                h.write_u64(subject.below_u64(driver.next_u64()));
            }
            22 => {
                let (lo, hi) = (driver.next_u32() as i32, driver.next_u32() as i32);
                h.write_i32(subject.range_i32(lo, hi));
                h.write_i32(subject.range_inclusive_i32(lo, hi));
                h.write_u8(subject.chance(driver.below(10), driver.below(12)) as u8);
                h.write_u8(subject.flip() as u8);
            }
            23 => {
                h.write_fx(subject.fx_unit());
                h.write_fx(subject.fx_range(a, b));
                h.write_facing(subject.facing());
            }
            24 => {
                let mut deck = [0u8; 13];
                for (i, card) in deck.iter_mut().enumerate() {
                    *card = i as u8;
                }
                subject.shuffle(&mut deck);
                h.write_bytes(&deck);
                h.write_u8(*subject.choose(&deck).unwrap());
            }
            _ => {
                // A little simulation: a unit walking toward a target,
                // turning at a limited rate, with jitter. Exercises the
                // pieces together the way sf_sim will.
                let mut pos = map_vector(&mut driver);
                let target = map_vector(&mut driver);
                let mut face = Facing::new(driver.next_u32() as u8);
                for _ in 0..8 {
                    if let Some(want) = (target - pos).facing() {
                        face = face.turn_toward(want, 6);
                    }
                    let jitter = Vec2::splat(subject.fx_range(Fx::ZERO, Fx::LEPTON));
                    pos += Vec2::from_facing(face, Fx::from_ratio(3, 4)) + jitter;
                }
                h.write_vec2(pos);
                h.write_facing(face);
            }
        }

        if (op + 1) % CHECKPOINT_EVERY == 0 {
            checkpoints[((op + 1) / CHECKPOINT_EVERY - 1) as usize] = h.finish();
        }
    }
    // The generators' final states are part of the proof as well.
    h.write_bytes(&driver.to_bytes());
    h.write_bytes(&subject.to_bytes());
    checkpoints[9] = h.finish();
    checkpoints
}

/// **The cross-platform proof.** Runs 100000 mixed operations and compares
/// the hash trail with the committed values. Passing on both the Linux and
/// the Windows CI jobs (debug and release) means the results are bit
/// identical on both.
#[test]
fn mixed_sequence_of_100k_operations_is_bit_identical() {
    let got = mixed_sequence();
    assert_eq!(
        got, GOLDEN_CHECKPOINTS,
        "the operation trail diverged; actual checkpoints:\n{got:#018x?}"
    );
}

/// The same computation twice in one process gives the same trail (no hidden
/// state, no address- or time-dependent behaviour).
#[test]
fn the_sequence_is_repeatable_within_a_process() {
    assert_eq!(mixed_sequence(), mixed_sequence());
}

// ---- focused goldens ------------------------------------------------------------------

const GOLDEN_RNG: u64 = 0x7ee1_218c_d7b5_3985;
const GOLDEN_TRIG: u64 = 0x515d_a0c8_eca5_77d1;
const GOLDEN_FACING_GRID: u64 = 0xcb17_600b_7867_77d4;
const GOLDEN_FX_SWEEP: u64 = 0x7639_95ee_e02d_628c;

#[test]
fn rng_streams_are_bit_identical() {
    let mut h = Fnv1a64::new();
    for (seed, stream) in [(0u64, 0u64), (42, 54), (u64::MAX, u64::MAX), (1, 1 << 63)] {
        let mut r = Rng::with_stream(seed, stream);
        for _ in 0..2_000 {
            h.write_u32(r.next_u32());
        }
        for bound in [1u32, 2, 3, 7, 100, 65_537, 1 << 31, (1 << 31) + 5, u32::MAX] {
            h.write_u32(r.below(bound));
        }
        h.write_u64(r.below_u64(10_000_000_000_007));
        h.write_i32(r.range_i32(-1_000_000, 1_000_000));
        h.write_bytes(&r.to_bytes());
    }
    assert_eq!(h.finish(), GOLDEN_RNG, "actual {:#018x}", h.finish());
}

#[test]
fn trig_tables_are_bit_identical() {
    let mut h = Fnv1a64::new();
    for f in 0..=255u8 {
        let face = Facing::new(f);
        h.write_fx(face.sin());
        h.write_fx(face.cos());
    }
    assert_eq!(h.finish(), GOLDEN_TRIG, "actual {:#018x}", h.finish());
}

#[test]
fn facing_conversion_is_bit_identical_on_a_dense_grid() {
    let mut h = Fnv1a64::new();
    for x in -64..=64i64 {
        for y in -64..=64i64 {
            for scale in [1i64, 1000, 1 << 30] {
                let v = Vec2::new(Fx::from_raw(x * scale), Fx::from_raw(y * scale));
                match v.facing() {
                    Some(f) => h.write_facing(f),
                    None => h.write_u8(0xFF),
                }
            }
        }
    }
    assert_eq!(
        h.finish(),
        GOLDEN_FACING_GRID,
        "actual {:#018x}",
        h.finish()
    );
}

#[test]
fn fixed_point_sweeps_are_bit_identical() {
    let mut h = Fnv1a64::new();
    // Square roots across the whole range: every small value, every power of
    // two and its neighbours.
    for raw in 0..20_000i64 {
        h.write_fx(Fx::from_raw(raw).sqrt());
    }
    for bit in 0..63 {
        for d in [-1i64, 0, 1] {
            h.write_fx(Fx::from_raw((1i64 << bit) + d).sqrt());
        }
    }
    // Every pair of awkward operands through every binary operation.
    let edges: Vec<Fx> = [
        i64::MIN,
        i64::MIN + 1,
        -(1 << 40),
        -(1 << 24) - 1,
        -65_537,
        -65_536,
        -65_535,
        -3,
        -2,
        -1,
        0,
        1,
        2,
        3,
        32_767,
        32_768,
        32_769,
        65_535,
        65_536,
        65_537,
        (1 << 24) + 1,
        1 << 40,
        i64::MAX - 1,
        i64::MAX,
    ]
    .iter()
    .map(|&r| Fx::from_raw(r))
    .collect();
    for &a in &edges {
        for &b in &edges {
            h.write_fx(a + b);
            h.write_fx(a - b);
            h.write_fx(a * b);
            h.write_fx(a / b);
            h.write_fx(a.lerp(b, Fx::HALF));
            h.write_fx(a.mul_div(b, Fx::from_int(3)));
            h.write_vec2(Vec2::new(a, b).normalize_or_zero());
            h.write_fx(Vec2::new(a, b).length());
            h.write_fx(Vec2::new(a, b).dot(Vec2::new(b, a)));
        }
    }
    assert_eq!(h.finish(), GOLDEN_FX_SWEEP, "actual {:#018x}", h.finish());
}
