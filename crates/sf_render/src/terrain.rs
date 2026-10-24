//! Terrain (WP1.7, WP1.8): the CPU side. Chunk meshes with levels of detail
//! built from an `sf_data` map, and the procedural material textures. The GPU
//! side lives in [`crate::renderer`] (`Renderer::set_terrain`), the per-frame
//! level selection and culling in [`crate::lod`], ray picking in
//! [`crate::ground`].
//!
//! * **Chunks:** the map is cut into chunks of [`CHUNK_CELLS`] x
//!   [`CHUNK_CELLS`] cells (smaller at the east/south edge when the map size
//!   is not a multiple). Positions and normals come from the global
//!   heightmap, so vertices on a shared chunk edge are bit-identical in both
//!   chunks at the same level. The cell diagonal alternates by the *global*
//!   cell parity ([`diagonal_nw_se`]), so triangles agree across chunk
//!   borders too. Every chunk carries its [`Aabb`] for culling.
//! * **Levels of detail (WP1.8):** every chunk has up to [`LOD_LEVELS`] meshes,
//!   level `l` using every `2^l`-th vertex of the full-detail grid (steps of 1,
//!   2, 4, 8, 16 and 32 cells; a smaller edge chunk only has the levels whose
//!   step divides its size). Each level stores its **geometric error**: the
//!   largest vertical distance, in metres, between its surface and the
//!   full-detail surface. It is exact, not sampled: both surfaces are
//!   piecewise linear, so the difference peaks at a vertex of their common
//!   refinement, which is a lattice point or the centre of a cell that the
//!   coarse cell's diagonal crosses ([`LodMesh::error`]). Errors never
//!   decrease with the level, so "the coarsest level within a pixel budget"
//!   is well defined. A coarse cell takes the diagonal with the smaller error.
//!   Normals are computed over the level's own step (central differences of
//!   +-step cells), like a mip chain, so far shading is band-limited instead
//!   of aliased.
//! * **Seams: skirts.** Two neighbouring chunks at different levels meet along
//!   polylines that differ by up to the sum of their errors, which would leave a
//!   crack. Each chunk-level therefore hangs a vertical *skirt* below every
//!   edge that has a neighbour, down to the lowest full-detail height on that
//!   edge (minus [`SKIRT_MARGIN`]). A level's surface along an edge is a
//!   polyline between full-detail heights of *that edge*, so it can never dip
//!   below that minimum: whatever the neighbour's level is, the higher of the
//!   two skirts reaches below the lower surface and the gap is closed exactly
//!   (tested for every pair of levels). The skirts face outward, so the one
//!   that hides a gap (the higher chunk's, seen from the lower side) is a front
//!   face and back-face culling stays on. Stitching was the alternative; skirts
//!   win because a chunk's mesh then depends on nothing but itself: level
//!   selection, hysteresis and culling stay per chunk, the index data is built
//!   once, and chunks of equal level can be drawn in one call (see
//!   [`PackedTerrain`]). The price is a few triangles per edge.
//! * **Materials:** one material per *cell*, not per vertex. A map with a
//!   terrain layer (WP2.2) is drawn from the gameplay terrain type of every 4 m
//!   cell ([`crate::terrain_look`]); a map without one from the material of
//!   every 1 m sample (WP1.7). Either way the index map is uploaded as a small
//!   texture and the fragment shader blends the four nearest cell centres, so
//!   the mesh stays one shared vertex grid. Each material is a small procedural
//!   tileable texture in a `texture_2d_array`, sampled with world-space UVs
//!   (one repeat per [`TILE_METERS`]).

use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use sf_data::map::Map;

pub use crate::frustum::Aabb;

/// Cells per chunk side.
pub const CHUNK_CELLS: u32 = 32;
/// Number of levels of detail (steps of 1, 2, 4, 8, 16 and 32 cells).
pub const LOD_LEVELS: usize = 6;
/// The coarsest level: one quad per 32x32-cell chunk.
pub const MAX_LEVEL: u8 = LOD_LEVELS as u8 - 1;
/// How far a skirt reaches below the lowest height of its edge, metres. It
/// also covers sub-pixel rasterisation cracks where two surfaces touch at the
/// same height.
pub const SKIRT_MARGIN: f32 = 0.5;
/// Edge length of a material texture layer in texels.
pub const TEXTURE_SIZE: u32 = 128;
/// World size of one texture repeat in metres.
pub const TILE_METERS: f32 = 4.0;

/// One terrain vertex: position, unit normal and the level of detail of the
/// mesh it belongs to (for the debug tint).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct TerrainVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub lod: u32,
}

/// A side of a chunk. North is `-z`, east is `+x`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    North,
    East,
    South,
    West,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::North, Side::East, Side::South, Side::West];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn opposite(self) -> Side {
        match self {
            Side::North => Side::South,
            Side::East => Side::West,
            Side::South => Side::North,
            Side::West => Side::East,
        }
    }
}

/// How the mesh is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshOptions {
    /// Build skirts under the chunk edges that have a neighbour. Switching
    /// them off is a debug aid: it shows the cracks between chunks of
    /// different levels.
    pub skirts: bool,
}

impl Default for MeshOptions {
    fn default() -> Self {
        Self { skirts: true }
    }
}

/// One level of detail of one chunk.
#[derive(Clone, Debug)]
pub struct LodMesh {
    pub level: u8,
    /// Cells per quad: `1 << level`.
    pub step: u32,
    /// Vertices per side of the surface grid, `(x, z)`.
    pub grid: (u32, u32),
    /// The surface grid (row-major, `z` rows), then the skirt's lower vertices.
    pub vertices: Vec<TerrainVertex>,
    /// Surface triangle list, counter-clockwise seen from above; indices
    /// into `vertices`.
    pub surface: Vec<u32>,
    /// Skirt triangle list (outward facing), indices into `vertices`.
    pub skirt: Vec<u32>,
    /// Height of the skirt's lower edge per [`Side`], `None` without skirt.
    pub skirt_bottom: [Option<f32>; 4],
    /// Largest vertical distance to the full-detail surface in metres (an
    /// upper bound, never decreasing with the level).
    pub error: f32,
}

impl LodMesh {
    pub fn surface_triangles(&self) -> u32 {
        self.surface.len() as u32 / 3
    }

    /// Surface plus skirt.
    pub fn triangles(&self) -> u32 {
        (self.surface.len() + self.skirt.len()) as u32 / 3
    }

    /// The surface grid vertex at grid position `(i, j)`.
    pub fn grid_vertex(&self, i: u32, j: u32) -> &TerrainVertex {
        &self.vertices[(j * self.grid.0 + i) as usize]
    }
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
    /// Box around the full-detail surface (every level lies inside it).
    pub aabb: Aabb,
    /// Level 0 (full detail) first; as many as the chunk's size allows.
    pub levels: Vec<LodMesh>,
}

impl TerrainChunk {
    /// The coarsest level this chunk has.
    pub fn max_level(&self) -> u8 {
        self.levels.len() as u8 - 1
    }

    pub fn level(&self, level: u8) -> &LodMesh {
        &self.levels[usize::from(level)]
    }
}

/// All chunks of a map.
#[derive(Clone, Debug)]
pub struct TerrainMesh {
    pub chunks: Vec<TerrainChunk>,
    /// Chunk grid size.
    pub chunks_x: u32,
    pub chunks_z: u32,
}

impl TerrainMesh {
    /// Index of the chunk at chunk coordinates `(cx, cz)`.
    pub fn chunk_index(&self, cx: u32, cz: u32) -> usize {
        (cz * self.chunks_x + cx) as usize
    }

    /// Triangles (surface plus skirt) of the whole map at one level, counting
    /// a chunk that lacks the level at its own coarsest.
    pub fn triangles_at(&self, level: u8) -> u64 {
        self.chunks
            .iter()
            .map(|c| u64::from(c.level(level.min(c.max_level())).triangles()))
            .sum()
    }
}

/// How a cell is split into triangles. Corner order: `[nw, ne, sw, se]`.
/// `true` = the diagonal runs north-west to south-east.
pub fn diagonal_nw_se(cell_x: u32, cell_z: u32) -> bool {
    (cell_x + cell_z).is_multiple_of(2)
}

/// The two triangles of a cell as indices into `[nw, ne, sw, se]`,
/// counter-clockwise seen from above. The one source of the triangulation:
/// the mesh, the surface height and the ray picking all use it.
pub fn cell_triangles(nw_se: bool) -> [[usize; 3]; 2] {
    if nw_se {
        [[0, 2, 3], [0, 3, 1]]
    } else {
        [[0, 2, 1], [1, 2, 3]]
    }
}

/// Height on the mesh surface inside one cell: `c = [nw, ne, sw, se]` are the
/// corner heights, `(fx, fz)` the position in the cell (`0..=1`, east and
/// south).
pub fn cell_surface(c: [f64; 4], fx: f64, fz: f64, nw_se: bool) -> f64 {
    let [nw, ne, sw, se] = c;
    if nw_se {
        if fx >= fz {
            // triangle (nw, se, ne)
            nw + (ne - nw) * (fx - fz) + (se - nw) * fz
        } else {
            // triangle (nw, sw, se)
            nw + (sw - nw) * (fz - fx) + (se - nw) * fx
        }
    } else if fx + fz <= 1.0 {
        // triangle (nw, sw, ne)
        nw + (ne - nw) * fx + (sw - nw) * fz
    } else {
        // triangle (ne, sw, se)
        ne + (se - ne) * (fx + fz - 1.0) + (sw - ne) * (1.0 - fx)
    }
}

/// Height of the full-detail mesh surface at a world position (clamped to the
/// map). This is the surface the picking sees and units stand on; coarser
/// levels deviate from it by at most their [`LodMesh::error`].
pub fn surface_height(map: &Map, x: f64, z: f64) -> f64 {
    let x = x.clamp(0.0, f64::from(map.width()));
    let z = z.clamp(0.0, f64::from(map.height()));
    let cx = (x.floor() as i64).min(i64::from(map.width()) - 1);
    let cz = (z.floor() as i64).min(i64::from(map.height()) - 1);
    let h = |dx, dz| f64::from(map.vertex_height(cx + dx, cz + dz));
    cell_surface(
        [h(0, 0), h(1, 0), h(0, 1), h(1, 1)],
        x - cx as f64,
        z - cz as f64,
        diagonal_nw_se(cx as u32, cz as u32),
    )
}

/// Unit surface normal at vertex `(vx, vz)` by central differences of the
/// heightmap (one-sided at the map border).
pub fn vertex_normal(map: &Map, vx: i64, vz: i64) -> [f32; 3] {
    vertex_normal_step(map, vx, vz, 1)
}

/// Like [`vertex_normal`], but differences over `+-step` cells: the normal of
/// a level of detail whose vertices are `step` cells apart.
pub fn vertex_normal_step(map: &Map, vx: i64, vz: i64, step: i64) -> [f32; 3] {
    let (w, h) = (i64::from(map.width()), i64::from(map.height()));
    let slope = |a: (i64, i64), b: (i64, i64)| {
        let span = ((b.0 - a.0) + (b.1 - a.1)) as f32;
        (map.vertex_height(b.0, b.1) - map.vertex_height(a.0, a.1)) / span
    };
    let dx = slope(((vx - step).max(0), vz), ((vx + step).min(w), vz));
    let dz = slope((vx, (vz - step).max(0)), (vx, (vz + step).min(h)));
    Vec3::new(-dx, 1.0, -dz).normalize().to_array()
}

/// Build the chunk meshes of a map with skirts.
pub fn build_mesh(map: &Map) -> TerrainMesh {
    build_mesh_with(map, MeshOptions::default())
}

/// Build the chunk meshes of a map.
pub fn build_mesh_with(map: &Map, options: MeshOptions) -> TerrainMesh {
    let chunks_x = map.width().div_ceil(CHUNK_CELLS);
    let chunks_z = map.height().div_ceil(CHUNK_CELLS);
    let mut chunks = Vec::with_capacity((chunks_x * chunks_z) as usize);
    for cz in 0..chunks_z {
        for cx in 0..chunks_x {
            chunks.push(build_chunk(map, (cx, cz), (chunks_x, chunks_z), options));
        }
    }
    TerrainMesh {
        chunks,
        chunks_x,
        chunks_z,
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn build_chunk(
    map: &Map,
    chunk: (u32, u32),
    grid: (u32, u32),
    options: MeshOptions,
) -> TerrainChunk {
    let origin = (chunk.0 * CHUNK_CELLS, chunk.1 * CHUNK_CELLS);
    let cells = (
        CHUNK_CELLS.min(map.width() - origin.0),
        CHUNK_CELLS.min(map.height() - origin.1),
    );
    let height = |x: u32, z: u32| map.vertex_height(i64::from(x), i64::from(z));
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for z in origin.1..=origin.1 + cells.1 {
        for x in origin.0..=origin.0 + cells.0 {
            let y = height(x, z);
            lo = lo.min(y);
            hi = hi.max(y);
        }
    }
    // Which sides touch another chunk, and the lowest height along each side.
    let neighbour = [
        chunk.1 > 0,
        chunk.0 + 1 < grid.0,
        chunk.1 + 1 < grid.1,
        chunk.0 > 0,
    ];
    let along = |a: u32, b: u32, f: &dyn Fn(u32) -> f32| (a..=b).map(f).fold(f32::MAX, f32::min);
    let (x1, z1) = (origin.0 + cells.0, origin.1 + cells.1);
    let side_min = [
        along(origin.0, x1, &|x| height(x, origin.1)),
        along(origin.1, z1, &|z| height(x1, z)),
        along(origin.0, x1, &|x| height(x, z1)),
        along(origin.1, z1, &|z| height(origin.0, z)),
    ];
    let max_level = gcd(cells.0, cells.1)
        .trailing_zeros()
        .min(u32::from(MAX_LEVEL)) as u8;
    let mut levels: Vec<LodMesh> = (0..=max_level)
        .map(|level| {
            build_level(
                map,
                origin,
                cells,
                level,
                &SkirtPlan {
                    enabled: options.skirts,
                    neighbour,
                    side_min,
                },
            )
        })
        .collect();
    // A coarser level never claims a smaller error than a finer one.
    for l in 1..levels.len() {
        levels[l].error = levels[l].error.max(levels[l - 1].error);
    }
    TerrainChunk {
        chunk,
        origin,
        cells,
        aabb: Aabb::new(
            Vec3::new(origin.0 as f32, lo, origin.1 as f32),
            Vec3::new(x1 as f32, hi, z1 as f32),
        ),
        levels,
    }
}

struct SkirtPlan {
    enabled: bool,
    neighbour: [bool; 4],
    side_min: [f32; 4],
}

/// Largest `|full - lod|` over one coarse cell. `full` is the `(s+1)^2` grid
/// of full-detail corner heights of the cell (row-major), `nw_se` the coarse
/// diagonal and `origin_even` the parity of the cell's first full-detail cell
/// (which fixes the diagonal of every full-detail cell inside).
fn coarse_cell_error(full: &[f64], s: usize, nw_se: bool, origin_even: bool) -> f64 {
    let w = s + 1;
    let corners = [full[0], full[s], full[s * w], full[s * w + s]];
    let sf = s as f64;
    let mut error = 0.0_f64;
    for b in 0..=s {
        for a in 0..=s {
            let lod = cell_surface(corners, a as f64 / sf, b as f64 / sf, nw_se);
            error = error.max((full[b * w + a] - lod).abs());
        }
    }
    // The difference of two piecewise linear surfaces peaks at a vertex of
    // their common refinement. Besides the lattice points that is where the
    // coarse diagonal crosses a full-detail diagonal: the cell centre.
    for k in 0..s {
        let (a, b) = if nw_se { (k, k) } else { (k, s - 1 - k) };
        let c = [
            full[b * w + a],
            full[b * w + a + 1],
            full[(b + 1) * w + a],
            full[(b + 1) * w + a + 1],
        ];
        let fine_nw_se = origin_even == (a + b).is_multiple_of(2);
        let fine = cell_surface(c, 0.5, 0.5, fine_nw_se);
        let lod = cell_surface(corners, (a as f64 + 0.5) / sf, (b as f64 + 0.5) / sf, nw_se);
        error = error.max((fine - lod).abs());
    }
    error
}

fn build_level(
    map: &Map,
    origin: (u32, u32),
    cells: (u32, u32),
    level: u8,
    skirts: &SkirtPlan,
) -> LodMesh {
    let step = 1_u32 << level;
    let (cw, ch) = (cells.0 / step, cells.1 / step);
    let (gw, gh) = (cw + 1, ch + 1);
    let mut vertices = Vec::with_capacity((gw * gh) as usize);
    for j in 0..gh {
        for i in 0..gw {
            let (gx, gz) = (origin.0 + i * step, origin.1 + j * step);
            vertices.push(TerrainVertex {
                position: [
                    gx as f32,
                    map.vertex_height(i64::from(gx), i64::from(gz)),
                    gz as f32,
                ],
                normal: vertex_normal_step(map, i64::from(gx), i64::from(gz), i64::from(step)),
                lod: u32::from(level),
            });
        }
    }

    let s = step as usize;
    let mut full = vec![0.0_f64; (s + 1) * (s + 1)];
    let mut surface = Vec::with_capacity((cw * ch * 6) as usize);
    let mut error = 0.0_f64;
    for j in 0..ch {
        for i in 0..cw {
            let (gx0, gz0) = (origin.0 + i * step, origin.1 + j * step);
            let parity = diagonal_nw_se(gx0, gz0);
            let nw_se = if level == 0 {
                parity
            } else {
                for b in 0..=s {
                    for a in 0..=s {
                        full[b * (s + 1) + a] = f64::from(
                            map.vertex_height(i64::from(gx0) + a as i64, i64::from(gz0) + b as i64),
                        );
                    }
                }
                let e_nw_se = coarse_cell_error(&full, s, true, parity);
                let e_ne_sw = coarse_cell_error(&full, s, false, parity);
                let pick = if (e_nw_se - e_ne_sw).abs() < 1e-12 {
                    parity
                } else {
                    e_nw_se < e_ne_sw
                };
                error = error.max(if pick { e_nw_se } else { e_ne_sw });
                pick
            };
            let a = j * gw + i;
            let corner = [a, a + 1, a + gw, a + gw + 1];
            for tri in cell_triangles(nw_se) {
                surface.extend(tri.iter().map(|&c| corner[c]));
            }
        }
    }

    let mut skirt = Vec::new();
    let mut skirt_bottom = [None; 4];
    if skirts.enabled {
        for side in Side::ALL {
            if !skirts.neighbour[side.index()] {
                continue;
            }
            let bottom = skirts.side_min[side.index()] - SKIRT_MARGIN;
            skirt_bottom[side.index()] = Some(bottom);
            // Upper vertices along the side, in increasing x (north, south)
            // or increasing z (west, east).
            let tops: Vec<u32> = match side {
                Side::North => (0..gw).collect(),
                Side::South => (0..gw).map(|i| (gh - 1) * gw + i).collect(),
                Side::West => (0..gh).map(|j| j * gw).collect(),
                Side::East => (0..gh).map(|j| j * gw + gw - 1).collect(),
            };
            let base = vertices.len() as u32;
            for &t in &tops {
                let v = vertices[t as usize];
                vertices.push(TerrainVertex {
                    position: [v.position[0], bottom, v.position[2]],
                    ..v
                });
            }
            for k in 0..tops.len() - 1 {
                let (t0, t1) = (tops[k], tops[k + 1]);
                let (b0, b1) = (base + k as u32, base + k as u32 + 1);
                // Outward facing: north and east are the mirror of south and west.
                if matches!(side, Side::North | Side::East) {
                    skirt.extend_from_slice(&[t0, b1, b0, t0, t1, b1]);
                } else {
                    skirt.extend_from_slice(&[t0, b0, b1, t0, b1, t1]);
                }
            }
        }
    }

    LodMesh {
        level,
        step,
        grid: (gw, gh),
        vertices,
        surface,
        skirt,
        skirt_bottom,
        // Round up, so the f32 stays an upper bound.
        error: if level == 0 {
            0.0
        } else {
            (error as f32).next_up()
        },
    }
}

/// All chunk meshes of a map packed for one vertex buffer and one index
/// buffer. Indices are absolute (`u32`, no base vertex), and the index
/// ranges are laid out level by level, chunk by chunk (`ranges[level][chunk]`
/// are consecutive), so a row of chunks drawn at the same level is one
/// contiguous range: one draw call.
#[derive(Clone, Debug)]
pub struct PackedTerrain {
    pub vertices: Vec<TerrainVertex>,
    pub indices: Vec<u32>,
    /// `ranges[level][chunk]`: the index range (surface, then skirt) of a
    /// chunk at a level; empty if the chunk does not have that level.
    pub ranges: Vec<Vec<Range<u32>>>,
}

/// Pack a [`TerrainMesh`] (see [`PackedTerrain`]).
pub fn pack(mesh: &TerrainMesh) -> PackedTerrain {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut ranges = Vec::with_capacity(LOD_LEVELS);
    for level in 0..LOD_LEVELS {
        let mut per_chunk = Vec::with_capacity(mesh.chunks.len());
        for chunk in &mesh.chunks {
            let start = indices.len() as u32;
            if let Some(lod) = chunk.levels.get(level) {
                let base = vertices.len() as u32;
                vertices.extend_from_slice(&lod.vertices);
                indices.extend(lod.surface.iter().chain(&lod.skirt).map(|&i| base + i));
            }
            per_chunk.push(start..indices.len() as u32);
        }
        ranges.push(per_chunk);
    }
    PackedTerrain {
        vertices,
        indices,
        ranges,
    }
}

// ---------------------------------------------------------------------------
// Procedural material textures
// ---------------------------------------------------------------------------

pub(crate) fn hash(seed: u32, x: u32, y: u32) -> u32 {
    let mut h = seed ^ x.wrapping_mul(0x85eb_ca6b) ^ y.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

/// Tileable value noise in `0..1`: `period` lattice points across the
/// texture, wrapping so opposite edges match.
pub(crate) fn tile_noise(seed: u32, u: f32, v: f32, period: u32) -> f32 {
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

pub(crate) fn name_hash(name: &str) -> u32 {
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
mod tests;
