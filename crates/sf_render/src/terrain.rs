//! Terrain (WP1.7): the CPU side. Chunk meshes built from an `sf_data` map
//! and the procedural material textures. The GPU side lives in
//! [`crate::renderer`] (`Renderer::set_terrain`).
//!
//! * **Chunks:** the map is cut into chunks of [`CHUNK_CELLS`] x
//!   [`CHUNK_CELLS`] cells (smaller at the east/south edge when the map size
//!   is not a multiple). A chunk has `(cells_x+1) * (cells_z+1)` vertices on
//!   the regular 1 m grid and two triangles per cell. Positions and normals
//!   are computed from the global heightmap, so the vertices on a shared
//!   chunk edge are bit-identical in both chunks: no cracks at full detail.
//!   The cell diagonal alternates by the *global* cell parity, so triangles
//!   agree across chunk borders too.
//! * **LOD (WP1.8):** the regular grid lets a coarser level reuse every
//!   2nd/4th vertex of the same vertex buffer with a different index
//!   buffer; every chunk carries its [`Aabb`] for culling and screen-error
//!   estimates. Nothing of that exists yet: all chunks draw at full detail.
//! * **Materials:** one material per *cell*, not per vertex. The material
//!   index map is uploaded as an `R8Uint` texture and the fragment shader
//!   blends the four nearest cell centres, so the mesh stays one shared
//!   vertex grid. Each material is a small procedural tileable texture in a
//!   `texture_2d_array`, sampled with world-space UVs (one repeat per
//!   [`TILE_METERS`]).

use glam::Vec3;
use sf_data::map::Map;

use crate::mesh::Vertex;

/// Cells per chunk side.
pub const CHUNK_CELLS: u32 = 32;
/// Edge length of a material texture layer in texels.
pub const TEXTURE_SIZE: u32 = 128;
/// World size of one texture repeat in metres.
pub const TILE_METERS: f32 = 4.0;

/// Axis-aligned bounding box in world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

/// One 32x32-cell piece of the terrain.
#[derive(Clone, Debug)]
pub struct TerrainChunk {
    /// Chunk coordinates (`x` east, `z` south).
    pub chunk: (u32, u32),
    /// First cell of the chunk.
    pub origin: (u32, u32),
    /// Size in cells (32, or less at the east/south map edge).
    pub cells: (u32, u32),
    pub aabb: Aabb,
    /// Row-major vertices, `(cells.0 + 1) * (cells.1 + 1)`.
    pub vertices: Vec<Vertex>,
    /// Full-detail triangle list (counter-clockwise seen from above).
    pub indices: Vec<u16>,
}

/// All chunks of a map.
#[derive(Clone, Debug)]
pub struct TerrainMesh {
    pub chunks: Vec<TerrainChunk>,
    /// Chunk grid size.
    pub chunks_x: u32,
    pub chunks_z: u32,
}

/// Unit surface normal at vertex `(vx, vz)` by central differences of the
/// heightmap (one-sided at the map border).
pub fn vertex_normal(map: &Map, vx: i64, vz: i64) -> [f32; 3] {
    let (w, h) = (i64::from(map.width()), i64::from(map.height()));
    let slope = |a: (i64, i64), b: (i64, i64)| {
        let span = ((b.0 - a.0) + (b.1 - a.1)) as f32;
        (map.vertex_height(b.0, b.1) - map.vertex_height(a.0, a.1)) / span
    };
    let dx = slope(((vx - 1).max(0), vz), ((vx + 1).min(w), vz));
    let dz = slope((vx, (vz - 1).max(0)), (vx, (vz + 1).min(h)));
    Vec3::new(-dx, 1.0, -dz).normalize().to_array()
}

/// Build the chunk meshes of a map.
pub fn build_mesh(map: &Map) -> TerrainMesh {
    let chunks_x = map.width().div_ceil(CHUNK_CELLS);
    let chunks_z = map.height().div_ceil(CHUNK_CELLS);
    let mut chunks = Vec::with_capacity((chunks_x * chunks_z) as usize);
    for cz in 0..chunks_z {
        for cx in 0..chunks_x {
            chunks.push(build_chunk(map, (cx, cz)));
        }
    }
    TerrainMesh {
        chunks,
        chunks_x,
        chunks_z,
    }
}

fn build_chunk(map: &Map, chunk: (u32, u32)) -> TerrainChunk {
    let origin = (chunk.0 * CHUNK_CELLS, chunk.1 * CHUNK_CELLS);
    let cells = (
        CHUNK_CELLS.min(map.width() - origin.0),
        CHUNK_CELLS.min(map.height() - origin.1),
    );
    let (vw, vh) = (cells.0 + 1, cells.1 + 1);
    let mut vertices = Vec::with_capacity((vw * vh) as usize);
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for z in 0..vh {
        for x in 0..vw {
            let (gx, gz) = (i64::from(origin.0 + x), i64::from(origin.1 + z));
            let y = map.vertex_height(gx, gz);
            lo = lo.min(y);
            hi = hi.max(y);
            vertices.push(Vertex {
                position: [gx as f32, y, gz as f32],
                normal: vertex_normal(map, gx, gz),
            });
        }
    }
    let mut indices = Vec::with_capacity((cells.0 * cells.1 * 6) as usize);
    for z in 0..cells.1 {
        for x in 0..cells.0 {
            let a = (z * vw + x) as u16; // north-west corner
            let b = a + 1; // north-east
            let c = a + vw as u16; // south-west
            let d = c + 1; // south-east
            if (origin.0 + x + origin.1 + z).is_multiple_of(2) {
                indices.extend_from_slice(&[a, c, d, a, d, b]);
            } else {
                indices.extend_from_slice(&[a, c, b, b, c, d]);
            }
        }
    }
    TerrainChunk {
        chunk,
        origin,
        cells,
        aabb: Aabb {
            min: Vec3::new(origin.0 as f32, lo, origin.1 as f32),
            max: Vec3::new((origin.0 + cells.0) as f32, hi, (origin.1 + cells.1) as f32),
        },
        vertices,
        indices,
    }
}

// ---------------------------------------------------------------------------
// Procedural material textures
// ---------------------------------------------------------------------------

fn hash(seed: u32, x: u32, y: u32) -> u32 {
    let mut h = seed ^ x.wrapping_mul(0x85eb_ca6b) ^ y.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

/// Tileable value noise in `0..1`: `period` lattice points across the
/// texture, wrapping so opposite edges match.
fn tile_noise(seed: u32, u: f32, v: f32, period: u32) -> f32 {
    let (x, y) = (u * period as f32, v * period as f32);
    let (x0, y0) = (x.floor(), y.floor());
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (fx, fy) = (s(x - x0), s(y - y0));
    let at = |ix: i64, iy: i64| {
        let p = i64::from(period);
        (hash(seed, ix.rem_euclid(p) as u32, iy.rem_euclid(p) as u32) >> 8) as f32 / 16_777_216.0
    };
    let (ix, iy) = (x0 as i64, y0 as i64);
    let (a, b, c, d) = (
        at(ix, iy),
        at(ix + 1, iy),
        at(ix, iy + 1),
        at(ix + 1, iy + 1),
    );
    (a + (b - a) * fx) * (1.0 - fy) + (c + (d - c) * fx) * fy
}

fn name_hash(name: &str) -> u32 {
    name.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// Base color (sRGB), contrast of the pattern and whether the pattern is
/// ridged (cracks) for a material name. Names that are not known get a
/// color derived from the name, so any map renders.
fn material_style(name: &str) -> ([u8; 3], f32, bool) {
    match name {
        "grass" => ([74, 128, 52], 0.30, false),
        "dirt" => ([128, 94, 62], 0.28, false),
        "rock" => ([122, 120, 116], 0.34, true),
        "sand" => ([204, 182, 128], 0.14, false),
        "dark_ground" => ([62, 54, 48], 0.26, false),
        other => {
            let h = name_hash(other);
            (
                [
                    64 + (h & 0x7f) as u8,
                    64 + ((h >> 8) & 0x7f) as u8,
                    64 + ((h >> 16) & 0x7f) as u8,
                ],
                0.25,
                false,
            )
        }
    }
}

/// RGBA8 sRGB pixels of one tileable material texture,
/// [`TEXTURE_SIZE`] x [`TEXTURE_SIZE`], mip level 0.
pub fn material_texture(name: &str) -> Vec<u8> {
    let (base, contrast, ridged) = material_style(name);
    let seed = name_hash(name);
    let n = TEXTURE_SIZE;
    let mut out = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let (u, v) = (x as f32 / n as f32, y as f32 / n as f32);
            let coarse = tile_noise(seed, u, v, 4);
            let medium = tile_noise(seed ^ 1, u, v, 16);
            let fine = tile_noise(seed ^ 2, u, v, 64);
            let mut t = 0.45 * coarse + 0.35 * medium + 0.20 * fine;
            if ridged {
                let r = 1.0 - (2.0 * tile_noise(seed ^ 3, u, v, 8) - 1.0).abs();
                t = 0.6 * t + 0.4 * r * r;
            }
            let k = 1.0 + (t - 0.5) * 2.0 * contrast;
            for c in base {
                out.push((f32::from(c) * k).clamp(0.0, 255.0).round() as u8);
            }
            out.push(255);
        }
    }
    out
}

/// Box-filtered mip chain of an RGBA8 square texture, level 0 first, down to
/// 1x1.
pub fn mip_chain(level0: Vec<u8>, size: u32) -> Vec<Vec<u8>> {
    let mut levels = vec![level0];
    let mut s = size;
    while s > 1 {
        let prev = levels.last().expect("non-empty");
        let half = s / 2;
        let mut next = Vec::with_capacity((half * half * 4) as usize);
        for y in 0..half {
            for x in 0..half {
                for ch in 0..4 {
                    let at = |dx: u32, dy: u32| {
                        u32::from(prev[(((2 * y + dy) * s + 2 * x + dx) * 4 + ch) as usize])
                    };
                    next.push(((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4) as u8);
                }
            }
        }
        levels.push(next);
        s = half;
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_data::map::generate::generate;

    fn check_chunk_basics(map: &Map, mesh: &TerrainMesh) {
        for c in &mesh.chunks {
            assert_eq!(
                c.vertices.len(),
                ((c.cells.0 + 1) * (c.cells.1 + 1)) as usize
            );
            assert_eq!(c.indices.len(), (c.cells.0 * c.cells.1 * 6) as usize);
            assert!(c.indices.iter().all(|&i| usize::from(i) < c.vertices.len()));
            for v in &c.vertices {
                let len = Vec3::from(v.normal).length();
                assert!((len - 1.0).abs() < 1e-5, "normal length {len}");
                assert!(v.normal[1] > 0.0);
                assert!(
                    v.position[1] >= c.aabb.min.y && v.position[1] <= c.aabb.max.y,
                    "vertex outside the chunk AABB"
                );
            }
            // Every triangle faces up (counter-clockwise from above).
            for t in c.indices.chunks(3) {
                let p = |i: u16| Vec3::from(c.vertices[usize::from(i)].position);
                let n = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
                assert!(n.y > 0.0, "downward triangle in chunk {:?}", c.chunk);
            }
        }
        let cells: u32 = mesh.chunks.iter().map(|c| c.cells.0 * c.cells.1).sum();
        assert_eq!(cells, map.width() * map.height());
    }

    #[test]
    fn a_64x64_map_makes_four_chunks_of_33x33_vertices() {
        let map = generate(64, 64, 1);
        let mesh = build_mesh(&map);
        assert_eq!((mesh.chunks_x, mesh.chunks_z), (2, 2));
        assert_eq!(mesh.chunks.len(), 4);
        for c in &mesh.chunks {
            assert_eq!(c.cells, (32, 32));
            assert_eq!(c.vertices.len(), 33 * 33);
            assert_eq!(c.indices.len(), 32 * 32 * 6);
        }
        check_chunk_basics(&map, &mesh);
    }

    #[test]
    fn edge_chunks_of_odd_sized_maps_are_smaller() {
        let map = generate(70, 40, 2);
        let mesh = build_mesh(&map);
        assert_eq!((mesh.chunks_x, mesh.chunks_z), (3, 2));
        let last = mesh.chunks.last().unwrap();
        assert_eq!(last.cells, (6, 8));
        check_chunk_basics(&map, &mesh);
    }

    /// Shared border vertices are identical in neighbouring chunks (position
    /// and normal, bit for bit), and every vertex is the map's own height.
    #[test]
    fn chunk_borders_have_no_cracks() {
        let map = generate(96, 64, 5);
        let mesh = build_mesh(&map);
        let chunk = |cx, cz| &mesh.chunks[(cz * mesh.chunks_x + cx) as usize];
        let vert =
            |c: &TerrainChunk, x: u32, z: u32| c.vertices[(z * (c.cells.0 + 1) + x) as usize];
        let mut compared = 0;
        for cz in 0..mesh.chunks_z {
            for cx in 0..mesh.chunks_x {
                let c = chunk(cx, cz);
                if cx + 1 < mesh.chunks_x {
                    let e = chunk(cx + 1, cz);
                    for z in 0..=c.cells.1 {
                        assert_eq!(vert(c, c.cells.0, z), vert(e, 0, z));
                        compared += 1;
                    }
                }
                if cz + 1 < mesh.chunks_z {
                    let s = chunk(cx, cz + 1);
                    for x in 0..=c.cells.0 {
                        assert_eq!(vert(c, x, c.cells.1), vert(s, x, 0));
                        compared += 1;
                    }
                }
                for v in &c.vertices {
                    let (x, z) = (v.position[0] as i64, v.position[2] as i64);
                    assert_eq!(v.position[1], map.vertex_height(x, z));
                }
            }
        }
        assert!(compared > 100);
    }

    #[test]
    fn normals_of_a_flat_and_a_sloped_map() {
        let flat =
            Map::from_parts(4, 4, 5.0, vec!["a".into()], vec![1000; 25], vec![0; 16]).unwrap();
        for vz in 0..=4 {
            for vx in 0..=4 {
                let n = vertex_normal(&flat, vx, vz);
                assert!((n[1] - 1.0).abs() < 1e-6, "{n:?}");
            }
        }
        // Height rising 1 m per metre to the east: normal tilts to the west.
        let heights: Vec<u16> = (0..25).map(|i| ((i % 5) * 13107) as u16).collect();
        let ramp = Map::from_parts(4, 4, 5.0, vec!["a".into()], heights, vec![0; 16]).unwrap();
        for vx in 0..=4 {
            let n = vertex_normal(&ramp, vx, 2);
            assert!(
                (n[0] + std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3,
                "{n:?}"
            );
            assert!(
                (n[1] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3,
                "{n:?}"
            );
            assert!(n[2].abs() < 1e-6);
        }
    }

    #[test]
    fn textures_tile_and_differ() {
        let names = [
            "grass",
            "dirt",
            "rock",
            "sand",
            "dark_ground",
            "unknown_thing",
        ];
        let textures: Vec<Vec<u8>> = names.iter().map(|n| material_texture(n)).collect();
        for (i, t) in textures.iter().enumerate() {
            assert_eq!(t.len(), (TEXTURE_SIZE * TEXTURE_SIZE * 4) as usize);
            let distinct: std::collections::BTreeSet<_> =
                t.chunks(4).map(|p| (p[0], p[1], p[2])).collect();
            assert!(distinct.len() > 50, "{} is flat", names[i]);
            for other in &textures[..i] {
                assert_ne!(t, other);
            }
        }
        // Tileable: the step across the wrap equals typical neighbour steps.
        let t = &textures[0];
        let n = TEXTURE_SIZE as usize;
        let px = |x: usize, y: usize| i32::from(t[(y * n + x) * 4 + 1]);
        let mean_step: i32 = (0..n).map(|y| (px(1, y) - px(0, y)).abs()).sum::<i32>() / n as i32;
        let wrap: i32 = (0..n).map(|y| (px(0, y) - px(n - 1, y)).abs()).sum::<i32>() / n as i32;
        assert!(wrap <= mean_step * 2 + 2, "wrap {wrap} vs step {mean_step}");
    }

    #[test]
    fn mip_chain_reaches_one_texel() {
        let levels = mip_chain(material_texture("grass"), TEXTURE_SIZE);
        assert_eq!(levels.len(), 8);
        assert_eq!(levels.last().unwrap().len(), 4);
        assert_eq!(levels[3].len(), 16 * 16 * 4);
    }
}
