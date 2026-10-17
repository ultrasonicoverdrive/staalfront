//! Procedural test map generator: bit-exact for a given size and seed on every
//! platform. The noise hash comes from `sf_math` ([`Fnv1a64`] chained into
//! [`Rng`]) and everything else is `sf_math` fixed point ([`Fx`]): no float,
//! no `sin`/`exp`/`sqrt` of the platform. A golden test (`tests/golden.rs`)
//! pins a hash of a generated 64 x 64 cell and a 256 x 256 cell map, and CI
//! runs it on Linux and Windows, so the simulation can read terrain from
//! generated maps (WP2.1).
//!
//! The landscape: gentle noise hills, a few round hills, a winding valley
//! with a riverbed, and a flat plateau. Materials are engine placeholders:
//! `grass`, `dirt`, `rock`, `sand`, `dark_ground`.
//!
//! Replacements for the former float functions: the river's `sin` is the
//! `sf_math` sine table of [`Facing`] (256 steps per turn) interpolated
//! linearly; a hill's `exp(-d2)` is `(1 - d2 / 8)^8` (the same peak and
//! nearly the same width, reaching zero at 2.83 hill radii, so the hill's
//! reach is exact instead of a cut-off); the plateau's `sqrt` is
//! [`Fx::sqrt`].

use sf_math::hash::Fnv1a64;
use sf_math::{Facing, Fx, Rng};

use super::Map;

/// Material names, in index order.
pub const MATERIALS: [&str; 5] = ["grass", "dirt", "rock", "sand", "dark_ground"];
const GRASS: u8 = 0;
const DIRT: u8 = 1;
const ROCK: u8 = 2;
const SAND: u8 = 3;
const DARK: u8 = 4;

/// Metres of height at a heightmap value of 65535 (the header value; the
/// generator itself computes with [`HEIGHT_SCALE_M`]).
pub const HEIGHT_SCALE: f32 = 8.0;
const HEIGHT_SCALE_M: u32 = 8;
/// Highest generated terrain as a fraction of the scale (keeps the relief
/// below the eye height of the closest camera zoom): 3/4.
const MAX_RELIEF_NUM: i64 = 3;
const MAX_RELIEF_DEN: i64 = 4;

/// A height sample in millimetres (rounded down), for the material rules.
fn millimetres(raw: u16) -> u32 {
    u32::from(raw) * (HEIGHT_SCALE_M * 1000) / 65535
}

/// A fraction of a whole, exactly: `num / den` as fixed point.
fn ratio(num: i64, den: i64) -> Fx {
    Fx::from_ratio(num, den)
}

/// A stateless 32-bit hash of a lattice point, from `sf_math` only: the
/// FNV-1a hash of `(seed, x, z)` seeds a PCG32 generator, and the hash is its
/// first output. (FNV alone mixes the last bytes too weakly for noise, and a
/// PCG seeded straight with a small integer gives nearly equal first outputs
/// for neighbouring seeds; chaining the two avoids both.)
fn hash(seed: u32, x: i32, z: i32) -> u32 {
    let mut key = Fnv1a64::new();
    key.write_u32(seed);
    key.write_i32(x);
    key.write_i32(z);
    Rng::new(key.finish()).next_u32()
}

/// A lattice value in `0..1` (16 bits).
fn unit(seed: u32, x: i32, z: i32) -> Fx {
    Fx::from_raw(i64::from(hash(seed, x, z) >> 16))
}

fn smooth(t: Fx) -> Fx {
    t * t * (Fx::from_int(3) - t.mul_int(2))
}

/// Value noise in `0..1`; one lattice point per unit.
fn noise(seed: u32, x: Fx, z: Fx) -> Fx {
    let (fx, fz) = (smooth(x.fract()), smooth(z.fract()));
    let (ix, iz) = (x.floor_int() as i32, z.floor_int() as i32);
    let a = unit(seed, ix, iz);
    let b = unit(seed, ix + 1, iz);
    let c = unit(seed, ix, iz + 1);
    let d = unit(seed, ix + 1, iz + 1);
    a.lerp(b, fx).lerp(c.lerp(d, fx), fz)
}

fn fbm(seed: u32, x: Fx, z: Fx, octaves: u32) -> Fx {
    let (mut sum, mut amp, mut norm) = (Fx::ZERO, Fx::ONE, Fx::ZERO);
    for o in 0..octaves {
        let freq = 1_i64 << o;
        sum += amp * noise(seed.wrapping_add(o * 101), x.mul_int(freq), z.mul_int(freq));
        norm += amp;
        amp = amp.div_int(2);
    }
    sum / norm
}

/// Sine of an angle given in turns (any value; only the fraction counts):
/// the 256-step table of [`Facing`], interpolated linearly.
fn sin_turns(turns: Fx) -> Fx {
    let raw = turns.fract().raw();
    let step = (raw >> 8) as u8;
    let between = Fx::from_raw((raw & 0xff) << 8);
    Facing::new(step)
        .sin()
        .lerp(Facing::new(step.wrapping_add(1)).sin(), between)
}

/// Size in metres the landscape features (hills, valley width, noise) scale
/// with: the shorter map side, but capped so big maps get more hills of the
/// same steepness instead of the same hills stretched flat.
fn feature_scale(w: u32, h: u32) -> u32 {
    w.min(h).min(96)
}

/// A round hill: `amp * (1 - d2 / 8)^8` with `d2` the squared distance in
/// radii, zero from `d2 = 8` (2.83 radii) on. The curve is the limit form of
/// the Gaussian `exp(-d2)` the generator used before, cut to a finite reach.
#[derive(Clone, Copy)]
struct Hill {
    x: Fx,
    z: Fx,
    radius: Fx,
    amp: Fx,
}

impl Hill {
    /// How far the hill reaches in every direction (a bounding square of the
    /// circle `d2 < 8`; the height outside it is zero by definition).
    fn reach(&self) -> Fx {
        self.radius.mul_int(3)
    }

    /// The height this hill adds at a point (zero outside its reach).
    fn height(&self, x: Fx, z: Fx) -> Fx {
        let (dx, dz) = (x - self.x, z - self.z);
        let reach = self.reach();
        if dx.abs() >= reach || dz.abs() >= reach {
            return Fx::ZERO;
        }
        let d2 = (dx.square() + dz.square()) / self.radius.square();
        if d2 >= Fx::from_int(8) {
            return Fx::ZERO;
        }
        let u = Fx::ONE - d2.div_int(8);
        let u2 = u.square();
        let u4 = u2.square();
        self.amp * u4.square()
    }
}

struct Landscape {
    seed: u32,
    /// Map size in metres.
    w: Fx,
    h: Fx,
    /// [`feature_scale`] in metres.
    scale: u32,
    hills: Vec<Hill>,
    /// Phase of the river's sine, in turns.
    river_phase: Fx,
}

impl Landscape {
    fn new(width: u32, height: u32, seed: u32) -> Self {
        let f = feature_scale(width, height);
        let (f2, area) = (
            u64::from(f) * u64::from(f),
            u64::from(width) * u64::from(height),
        );
        // Five hills per feature-scale square, rounded to nearest.
        let count = (10 * area + f2) / (2 * f2);
        let hills = (0..count as u32)
            .map(|i| {
                let s = seed.wrapping_add(7000 + i);
                Hill {
                    x: unit(s, 1, 0).mul_int(i64::from(width)),
                    z: unit(s, 2, 0).mul_int(i64::from(height)),
                    radius: (ratio(1, 20) + unit(s, 3, 0).div_int(20)).mul_int(i64::from(f)),
                    amp: ratio(45, 100) + ratio(40, 100) * unit(s, 4, 0),
                }
            })
            .collect();
        Self {
            seed,
            w: Fx::from_i64(i64::from(width)),
            h: Fx::from_i64(i64::from(height)),
            scale: f,
            hills,
            river_phase: unit(seed, 99, 99),
        }
    }

    /// The lattice step of the noise, in metres: 0.12 of the scale (12/100).
    fn noise_step(&self) -> Fx {
        ratio(i64::from(self.scale) * 12, 100)
    }

    /// Distance (m) from a world point to the river centre line, measured
    /// along z (good enough for a valley).
    fn river_distance(&self, x: Fx, z: Fx) -> Fx {
        let turns = x / self.w * ratio(6, 5) + self.river_phase;
        let centre = self.h.div_int(2) + sin_turns(turns) * self.h * ratio(16, 100);
        (z - centre).abs()
    }

    /// Height as a fraction of the scale, `0..=MAX_RELIEF`, over all hills
    /// (the reference the row shortcut is tested against).
    #[cfg(test)]
    fn height(&self, x: Fx, z: Fx) -> Fx {
        self.height_among(x, z, &self.hills)
    }

    /// The hills that can change a height on the row `z`: those whose reach
    /// includes it. Leaving the others out gives the same bits (they add
    /// exactly zero) and makes the 1,024 m demo map much faster to generate.
    fn hills_near_row(&self, z: Fx) -> Vec<Hill> {
        self.hills
            .iter()
            .copied()
            .filter(|hill| (z - hill.z).abs() < hill.reach())
            .collect()
    }

    fn height_among(&self, x: Fx, z: Fx, hills: &[Hill]) -> Fx {
        let scale = Fx::from_i64(i64::from(self.scale));
        let step = self.noise_step();
        let mut v = ratio(1, 5) + ratio(3, 5) * fbm(self.seed, x / step, z / step, 4);
        for hill in hills {
            v += hill.height(x, z);
        }
        // Valley with a flat riverbed.
        let d = self.river_distance(x, z);
        let valley = Fx::ONE - smooth((d / (scale / Fx::from_int(10))).min(Fx::ONE));
        v = v * (Fx::ONE - ratio(4, 5) * valley) + ratio(3, 100) * valley;
        // Plateau in the north-west quarter.
        let (px, pz) = (x / self.w - ratio(1, 5), z / self.h - ratio(9, 50));
        let dist = (px.square() + pz.square()).sqrt();
        let plateau = Fx::ONE - smooth((dist / ratio(14, 100)).min(Fx::ONE));
        v = v * (Fx::ONE - plateau) + ratio(17, 50) * plateau;
        v.clamp(Fx::ZERO, Fx::ONE)
            .mul_int(MAX_RELIEF_NUM)
            .div_int(MAX_RELIEF_DEN)
    }
}

/// A height fraction of the scale (`0..=1`) as a heightmap value, rounded to
/// nearest.
fn height_value(fraction: Fx) -> u16 {
    let scaled = i128::from(fraction.raw()) * 65535;
    ((scaled + (1 << 15)) >> 16).clamp(0, 65535) as u16
}

/// Generate a map of `width` x `height` gameplay cells (B12): four terrain
/// cells of 1 m along every cell side, so 256 x 256 cells is the 1,024 m map
/// of the engine demo.
pub fn generate_cells(width: u32, height: u32, seed: u32) -> Map {
    generate(
        width * sf_math::CELL_METRES,
        height * sf_math::CELL_METRES,
        seed,
    )
}

/// Generate a `width` x `height` map of 1 m terrain cells from a seed.
pub fn generate(width: u32, height: u32, seed: u32) -> Map {
    let land = Landscape::new(width, height, seed);
    let (vw, vh) = (width as usize + 1, height as usize + 1);
    let mut heights = Vec::with_capacity(vw * vh);
    for vz in 0..vh {
        let z = Fx::from_i64(vz as i64);
        let near = land.hills_near_row(z);
        for vx in 0..vw {
            let f = land.height_among(Fx::from_i64(vx as i64), z, &near);
            heights.push(height_value(f));
        }
    }
    let mm = |x: usize, z: usize| millimetres(heights[z * vw + x]);
    let mut cells = Vec::with_capacity(width as usize * height as usize);
    let step = land.noise_step();
    let half = Fx::HALF;
    for cz in 0..height as usize {
        for cx in 0..width as usize {
            let corners = [
                mm(cx, cz),
                mm(cx + 1, cz),
                mm(cx, cz + 1),
                mm(cx + 1, cz + 1),
            ];
            let sum: u32 = corners.iter().sum(); // four times the mean
            let low = corners.iter().copied().min().unwrap_or(0);
            let high = corners.iter().copied().max().unwrap_or(0);
            let slope = high - low; // millimetres per cell
            let (x, z) = (
                Fx::from_i64(cx as i64) + half,
                Fx::from_i64(cz as i64) + half,
            );
            let river = land.river_distance(x, z);
            let n = fbm(seed.wrapping_add(555), x / step, z / step, 3);
            let river_edge = ratio(i64::from(land.scale) * 45, 1000) + (n - half).mul_int(3);
            let material = if river < river_edge {
                SAND
            } else if slope > 550 || sum > 4 * 4400 {
                ROCK
            } else if n > ratio(31, 50) {
                DIRT
            } else if n < ratio(17, 50) && sum < 4 * 3000 {
                DARK
            } else {
                GRASS
            };
            cells.push(material);
        }
    }
    Map::from_parts(
        width,
        height,
        HEIGHT_SCALE,
        MATERIALS.iter().map(|m| (*m).to_string()).collect(),
        heights,
        cells,
    )
    .expect("the generator produces a valid map")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Leaving the far hills out of a row must not change a single height.
    #[test]
    fn the_hill_shortcut_gives_the_same_bits_as_summing_all_hills() {
        let (w, h) = (300_u32, 260_u32);
        let land = Landscape::new(w, h, 5);
        for vz in (0..=h).step_by(3) {
            let z = Fx::from_i64(i64::from(vz));
            let near = land.hills_near_row(z);
            for vx in (0..=w).step_by(2) {
                let x = Fx::from_i64(i64::from(vx));
                assert_eq!(
                    land.height_among(x, z, &near),
                    land.height(x, z),
                    "at ({vx}, {vz})"
                );
            }
        }
    }

    /// The lattice hash must look random: uniform, and not correlated with
    /// its neighbours in x, in z or in the seed (a weak hash shows up as
    /// stripes in the value noise).
    #[test]
    fn the_lattice_hash_is_uniform_and_uncorrelated() {
        let n = 64_i32;
        let value = |seed: u32, x: i32, z: i32| unit(seed, x, z).raw() as f64 / 65536.0;
        let mut sum = 0.0;
        let mut buckets = [0_u32; 16];
        for z in 0..n {
            for x in 0..n {
                let v = value(1, x, z);
                sum += v;
                buckets[(v * 16.0) as usize] += 1;
            }
        }
        let count = f64::from(n * n);
        assert!((sum / count - 0.5).abs() < 0.02, "mean {}", sum / count);
        for (i, b) in buckets.iter().enumerate() {
            assert!(
                (f64::from(*b) - count / 16.0).abs() < 60.0,
                "bucket {i}: {b}"
            );
        }
        // Pearson correlation with the neighbour in each direction.
        let correlation = |other: &dyn Fn(i32, i32) -> f64| {
            let (mut sa, mut sb, mut saa, mut sbb, mut sab) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for z in 0..n {
                for x in 0..n {
                    let (a, b) = (value(1, x, z), other(x, z));
                    sa += a;
                    sb += b;
                    saa += a * a;
                    sbb += b * b;
                    sab += a * b;
                }
            }
            let cov = sab / count - sa / count * sb / count;
            cov / ((saa / count - (sa / count).powi(2)) * (sbb / count - (sb / count).powi(2)))
                .sqrt()
        };
        for (name, r) in [
            ("x + 1", correlation(&|x, z| value(1, x + 1, z))),
            ("z + 1", correlation(&|x, z| value(1, x, z + 1))),
            ("seed + 1", correlation(&|x, z| value(2, x, z))),
            ("swapped", correlation(&|x, z| value(1, z, x))),
        ] {
            assert!(r.abs() < 0.06, "{name}: correlation {r}");
        }
    }

    /// The river's sine is the table sine, interpolated: exact at the table
    /// steps and never further than a hair off in between.
    #[test]
    fn the_interpolated_sine_matches_the_table_and_stays_close() {
        for step in 0..=255_u8 {
            let turns = Fx::from_raw(i64::from(step) << 8);
            assert_eq!(sin_turns(turns), Facing::new(step).sin());
        }
        // Whole turns do not matter, negative turns wrap.
        assert_eq!(sin_turns(ratio(1, 4)), Fx::ONE);
        assert_eq!(sin_turns(ratio(5, 4)), Fx::ONE);
        assert_eq!(sin_turns(ratio(-3, 4)), Fx::ONE);
        // Between two steps the error is below 1e-4 (the float reference
        // lives in this test only).
        for raw in (0..65536_i64).step_by(7) {
            let exact = (raw as f64 / 65536.0 * std::f64::consts::TAU).sin();
            let got = sin_turns(Fx::from_raw(raw)).raw() as f64 / 65536.0;
            assert!(
                (got - exact).abs() < 1.0e-4,
                "turns {raw}/65536: {got} vs {exact}"
            );
        }
    }

    /// The hill peaks at its amplitude, falls off like the Gaussian it
    /// replaces and is exactly zero from 2.83 radii on.
    #[test]
    fn a_hill_peaks_at_its_amplitude_and_follows_the_gaussian_it_replaces() {
        let hill = Hill {
            x: Fx::from_int(10),
            z: Fx::from_int(10),
            radius: Fx::from_int(5),
            amp: ratio(1, 2),
        };
        assert_eq!(hill.height(hill.x, hill.z), hill.amp);
        let mut last = hill.amp;
        for dx in 1..8 {
            let h = hill.height(Fx::from_int(10 + dx), hill.z);
            assert!(h < last, "dx {dx}: {h} !< {last}");
            last = h;
            // The float reference lives in this test only.
            let d2 = f64::from(dx * dx) / 25.0;
            let gauss = 0.5 * (-d2).exp();
            let got = h.raw() as f64 / 65536.0;
            assert!((got - gauss).abs() < 0.04, "dx {dx}: {got} vs {gauss}");
        }
        assert_eq!(hill.height(Fx::from_int(24), hill.z), Fx::ZERO);
        assert_eq!(hill.height(Fx::from_int(25), hill.z), Fx::ZERO);
        // The corner of the bounding square is outside the circle (d2 = 8).
        assert_eq!(hill.height(Fx::from_int(20), Fx::from_int(20)), Fx::ZERO);
        assert!(hill.height(Fx::from_int(17), Fx::from_int(17)) > Fx::ZERO);
    }

    /// The generator is integer and fixed point only: no float type outside
    /// the one header constant, and no platform maths function.
    #[test]
    fn the_generator_source_has_no_floats() {
        let source = include_str!("generate.rs");
        // Line endings may be CRLF (a Windows checkout), so split on the
        // line start of the test module only.
        let production = source
            .split("\nmod tests {")
            .next()
            .expect("the file has a production part");
        assert!(
            production.len() < source.len() - 1000,
            "the test module was found and cut off"
        );
        for (n, line) in production.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if code.contains("pub const HEIGHT_SCALE: f32 = 8.0;") {
                continue;
            }
            for needle in ["f32", "f64", ".exp(", ".powi(", ".powf(", ".ln(", ".hypot("] {
                assert!(
                    !code.contains(needle),
                    "line {}: `{needle}` in {code}",
                    n + 1
                );
            }
            let float_literal = code
                .as_bytes()
                .windows(3)
                .any(|w| w[0].is_ascii_digit() && w[1] == b'.' && w[2].is_ascii_digit());
            assert!(!float_literal, "line {}: a float literal in {code}", n + 1);
        }
    }
}
