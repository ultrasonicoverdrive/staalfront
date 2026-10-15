//! Procedural test map generator: deterministic for a given size and seed on
//! one platform (integer hashing, simple float math). Used to produce the
//! committed test map and the 256x256 map `sf_game` shows; it is a tool, not
//! simulation code, so floats are fine here.
//!
//! The landscape: gentle noise hills, a few round hills, a winding valley
//! with a riverbed, and a flat plateau. Materials are engine placeholders:
//! `grass`, `dirt`, `rock`, `sand`, `dark_ground`.

use super::Map;

/// Material names, in index order.
pub const MATERIALS: [&str; 5] = ["grass", "dirt", "rock", "sand", "dark_ground"];
const GRASS: u8 = 0;
const DIRT: u8 = 1;
const ROCK: u8 = 2;
const SAND: u8 = 3;
const DARK: u8 = 4;

/// Metres of height at a heightmap value of 65535.
pub const HEIGHT_SCALE: f32 = 8.0;
/// Highest generated terrain as a fraction of the scale (keeps the relief
/// below the eye height of the closest camera zoom).
const MAX_RELIEF: f32 = 0.75;

fn hash(seed: u32, x: i32, z: i32) -> u32 {
    let mut h = seed ^ (x as u32).wrapping_mul(0x85eb_ca6b) ^ (z as u32).wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

fn unit(seed: u32, x: i32, z: i32) -> f32 {
    (hash(seed, x, z) >> 8) as f32 / 16_777_216.0
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Value noise in `0..1`; one lattice point per unit.
fn noise(seed: u32, x: f32, z: f32) -> f32 {
    let (x0, z0) = (x.floor(), z.floor());
    let (fx, fz) = (smooth(x - x0), smooth(z - z0));
    let (ix, iz) = (x0 as i32, z0 as i32);
    let a = unit(seed, ix, iz);
    let b = unit(seed, ix + 1, iz);
    let c = unit(seed, ix, iz + 1);
    let d = unit(seed, ix + 1, iz + 1);
    (a + (b - a) * fx) * (1.0 - fz) + (c + (d - c) * fx) * fz
}

fn fbm(seed: u32, x: f32, z: f32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves {
        sum += amp * noise(seed.wrapping_add(o * 101), x * freq, z * freq);
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

/// Size in metres the landscape features (hills, valley width, noise) scale
/// with: the shorter map side, but capped so big maps get more hills of the
/// same steepness instead of the same hills stretched flat.
fn feature_scale(w: f32, h: f32) -> f32 {
    w.min(h).min(96.0)
}

struct Landscape {
    seed: u32,
    w: f32,
    h: f32,
    hills: Vec<(f32, f32, f32, f32)>,
    river_phase: f32,
}

impl Landscape {
    fn new(width: u32, height: u32, seed: u32) -> Self {
        let (w, h) = (width as f32, height as f32);
        let f = feature_scale(w, h);
        let count = (5.0 * w * h / (f * f)).round() as u32;
        let hills = (0..count)
            .map(|i| {
                let s = seed.wrapping_add(7000 + i);
                (
                    unit(s, 1, 0) * w,
                    unit(s, 2, 0) * h,
                    (0.05 + 0.05 * unit(s, 3, 0)) * f,
                    0.45 + 0.40 * unit(s, 4, 0),
                )
            })
            .collect();
        Self {
            seed,
            w,
            h,
            hills,
            river_phase: unit(seed, 99, 99) * std::f32::consts::TAU,
        }
    }

    /// Distance (m) from a world point to the river centre line, measured
    /// along z (good enough for a valley).
    fn river_distance(&self, x: f32, z: f32) -> f32 {
        let centre = self.h * 0.5
            + (x / self.w * std::f32::consts::TAU * 1.2 + self.river_phase).sin() * self.h * 0.16;
        (z - centre).abs()
    }

    /// Height as a fraction of the scale, `0..=MAX_RELIEF`.
    fn height(&self, x: f32, z: f32) -> f32 {
        let scale = feature_scale(self.w, self.h);
        let mut v = 0.20 + 0.60 * fbm(self.seed, x / (scale * 0.12), z / (scale * 0.12), 4);
        for &(hx, hz, radius, amp) in &self.hills {
            let d2 = ((x - hx).powi(2) + (z - hz).powi(2)) / (radius * radius);
            v += amp * (-d2).exp();
        }
        // Valley with a flat riverbed.
        let d = self.river_distance(x, z);
        let valley = 1.0 - smooth((d / (scale * 0.10)).min(1.0));
        v = v * (1.0 - 0.8 * valley) + 0.03 * valley;
        // Plateau in the north-west quarter.
        let (px, pz) = (x / self.w - 0.2, z / self.h - 0.18);
        let plateau = 1.0 - smooth(((px * px + pz * pz).sqrt() / 0.14).min(1.0));
        v = v * (1.0 - plateau) + 0.34 * plateau;
        v.clamp(0.0, 1.0) * MAX_RELIEF
    }
}

/// Generate a `width` x `height` map from a seed.
pub fn generate(width: u32, height: u32, seed: u32) -> Map {
    let land = Landscape::new(width, height, seed);
    let (vw, vh) = (width as usize + 1, height as usize + 1);
    let mut heights = Vec::with_capacity(vw * vh);
    for vz in 0..vh {
        for vx in 0..vw {
            let f = land.height(vx as f32, vz as f32);
            heights.push((f * 65535.0).round() as u16);
        }
    }
    let at = |x: usize, z: usize| f32::from(heights[z * vw + x]) / 65535.0 * HEIGHT_SCALE;
    let mut cells = Vec::with_capacity(width as usize * height as usize);
    let scale = feature_scale(land.w, land.h);
    for cz in 0..height as usize {
        for cx in 0..width as usize {
            let corners = [
                at(cx, cz),
                at(cx + 1, cz),
                at(cx, cz + 1),
                at(cx + 1, cz + 1),
            ];
            let mean = corners.iter().sum::<f32>() / 4.0;
            let low = corners.iter().copied().fold(f32::MAX, f32::min);
            let high = corners.iter().copied().fold(f32::MIN, f32::max);
            let slope = high - low; // metres per cell
            let (x, z) = (cx as f32 + 0.5, cz as f32 + 0.5);
            let river = land.river_distance(x, z);
            let n = fbm(
                seed.wrapping_add(555),
                x / (scale * 0.12),
                z / (scale * 0.12),
                3,
            );
            let material = if river < scale * 0.045 + 3.0 * (n - 0.5) {
                SAND
            } else if slope > 0.55 || mean > 4.4 {
                ROCK
            } else if n > 0.62 {
                DIRT
            } else if n < 0.34 && mean < 3.0 {
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
