//! Baked per-vertex shading, written as glTF `COLOR_0` (which the loader multiplies into the
//! material colour; the team material ignores it, `sf_render::glb`).
//!
//! The renderer lights with one directional light and an ambient term and has no shadows, so the
//! depth a model needs is baked in here, per corner of every (flat) triangle:
//!
//! - **ambient occlusion**: [`AO_RAYS`] rays over the hemisphere of the face; the share that hits
//!   other geometry within [`AO_REACH_MM`] darkens the corner by up to [`AO_STRENGTH`] (creases,
//!   the underside of an overhang, the gap between hull and tracks);
//! - **grime**: up to [`GRIME`] darker towards the ground, fading out at [`GRIME_HEIGHT_MM`];
//! - **downward faces** up to [`DOWN`] darker;
//! - **facet variation**: each triangle ± [`VARIATION`] from a fixed hash of its index, so large
//!   flat areas do not look like one sheet;
//! - the triangle's own [`crate::kit::Tone`] (panel lines, recesses).
//!
//! The result is rounded to 1/255, so tiny float differences cannot reach the file. Only basic
//! IEEE operations are used (no `sin`, `pow`), so every platform computes the same bytes.

use crate::kit::Tri;

/// Rays per corner: the normal itself and two rings of eight around it.
pub const AO_RAYS: usize = 17;
/// How far a ray looks for an occluder, in millimetres.
pub const AO_REACH_MM: f32 = 1400.0;
/// The darkest an entirely occluded corner gets (share of the colour taken away).
pub const AO_STRENGTH: f32 = 0.6;
/// Grime at the ground, fading to none at [`GRIME_HEIGHT_MM`].
pub const GRIME: f32 = 0.22;
pub const GRIME_HEIGHT_MM: f32 = 1100.0;
/// A face pointing straight down loses this share.
pub const DOWN: f32 = 0.3;
/// Facet variation, either way.
pub const VARIATION: f32 = 0.035;

type V = [f32; 3];

fn v(p: [i32; 3]) -> V {
    [p[0] as f32, p[1] as f32, p[2] as f32]
}
fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V, s: f32) -> V {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: V) -> V {
    let l = dot(a, a).sqrt();
    if l > 0.0 { mul(a, 1.0 / l) } else { a }
}

/// The unit normal of a triangle, from its integer corners.
pub fn unit_normal(t: &Tri) -> V {
    let n = t.normal();
    // i64 to f64 to f32: exact enough and the same everywhere.
    let f = [n[0] as f64, n[1] as f64, n[2] as f64];
    let l = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt();
    [(f[0] / l) as f32, (f[1] / l) as f32, (f[2] / l) as f32]
}

/// Directions of the rays in the frame (tangent, bitangent, normal): `(t, b, n)` components.
/// Ring one at 30° from the normal, ring two at 65°; the values are fixed decimals.
const RAY_DIRS: [V; AO_RAYS] = {
    // cos/sin of 0, 45, ..., 315 degrees.
    const C: [f32; 8] = [1.0, 0.7071, 0.0, -0.7071, -1.0, -0.7071, 0.0, 0.7071];
    const S: [f32; 8] = [0.0, 0.7071, 1.0, 0.7071, 0.0, -0.7071, -1.0, -0.7071];
    let mut d = [[0.0, 0.0, 1.0]; AO_RAYS];
    let mut i = 0;
    while i < 8 {
        // 30 degrees: sin 0.5, cos 0.866; 65 degrees: sin 0.9063, cos 0.4226. Ring two is turned
        // by half a step so the rings interleave.
        d[1 + i] = [0.5 * C[i], 0.5 * S[i], 0.866];
        let j = (i + 1) % 8;
        d[9 + i] = [
            0.9063 * (C[i] + C[j]) * 0.5412,
            0.9063 * (S[i] + S[j]) * 0.5412,
            0.4226,
        ];
        i += 1;
    }
    d
};

struct Occluder {
    a: V,
    e1: V,
    e2: V,
    centre: V,
    radius: f32,
}

/// Whether the ray `o + t d` (`0 < t < reach`) hits the triangle (Möller–Trumbore).
fn hits(o: V, d: V, tri: &Occluder) -> bool {
    let to_centre = sub(tri.centre, o);
    let along = dot(to_centre, d);
    if along < -tri.radius || along > AO_REACH_MM + tri.radius {
        return false;
    }
    let off = sub(to_centre, mul(d, along));
    if dot(off, off) > tri.radius * tri.radius {
        return false;
    }
    let p = cross(d, tri.e2);
    let det = dot(tri.e1, p);
    if det.abs() < 1e-6 {
        return false;
    }
    let inv = 1.0 / det;
    let s = sub(o, tri.a);
    let u = dot(s, p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = cross(s, tri.e1);
    let w = dot(d, q) * inv;
    if w < 0.0 || u + w > 1.0 {
        return false;
    }
    let t = dot(tri.e2, q) * inv;
    t > 1.0 && t < AO_REACH_MM
}

/// The shade of every corner of `tris` (all the triangles of one level, in model space), in the
/// order of the triangles, three per triangle. 1.0 is the plain material colour.
pub fn bake(tris: &[Tri]) -> Vec<[f32; 3]> {
    let occ: Vec<Occluder> = tris
        .iter()
        .map(|t| {
            let [a, b, c] = t.v.map(v);
            let centre = mul(add(add(a, b), c), 1.0 / 3.0);
            let radius = [a, b, c]
                .iter()
                .map(|p| {
                    let d = sub(*p, centre);
                    dot(d, d)
                })
                .fold(0.0_f32, f32::max)
                .sqrt();
            Occluder {
                a,
                e1: sub(b, a),
                e2: sub(c, a),
                centre,
                radius,
            }
        })
        .collect();
    let mut out = Vec::with_capacity(tris.len() * 3);
    for (i, t) in tris.iter().enumerate() {
        let n = unit_normal(t);
        // A tangent frame from the normal (any perpendicular will do; it is fixed per face).
        let helper = if n[1].abs() < 0.9 {
            [0.0, 1.0, 0.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let tan = norm(cross(helper, n));
        let bit = cross(n, tan);
        let dirs: Vec<V> = RAY_DIRS
            .iter()
            .map(|r| norm(add(add(mul(tan, r[0]), mul(bit, r[1])), mul(n, r[2]))))
            .collect();
        let centre = occ[i].centre;
        let variation = 1.0 + VARIATION * (hash(i as u32) as f32 / 500.0 - 1.0);
        let tone = f32::from(t.tone) / 1000.0;
        for corner in t.v.map(v) {
            // Start a little inside the face and off it, so the ray does not hit its own edges.
            let o = add(add(corner, mul(sub(centre, corner), 0.12)), mul(n, 8.0));
            let blocked = dirs
                .iter()
                .filter(|&&d| {
                    occ.iter()
                        .enumerate()
                        .any(|(j, tri)| j != i && hits(o, d, tri))
                })
                .count();
            let ao = 1.0 - AO_STRENGTH * blocked as f32 / AO_RAYS as f32;
            let height = (corner[1] / GRIME_HEIGHT_MM).clamp(0.0, 1.0);
            let grime = 1.0 - GRIME * (1.0 - height);
            let down = 1.0 - DOWN * (-n[1]).max(0.0);
            let s = (ao * grime * down * variation * tone).clamp(0.0, 1.0);
            let q = (s * 255.0).round() / 255.0;
            out.push([q, q, q]);
        }
    }
    out
}

/// 0..=999 from a fixed integer hash.
fn hash(i: u32) -> u32 {
    let mut x = i.wrapping_mul(0x9e37_79b9) ^ 0x5bd1_e995;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    x % 1000
}
