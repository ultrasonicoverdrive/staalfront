//! Terrain level of detail and culling (WP1.8): the configuration, the
//! per-frame level selection and the draw list. CPU only (no wgpu), so it is
//! tested without a GPU; [`crate::renderer`] feeds the result to the GPU.
//!
//! # Error metric
//! Every chunk-level has a geometric error in metres ([`crate::terrain::LodMesh::error`]):
//! the largest vertical distance to the full-detail surface. For a camera at
//! `eye` with a focal length of `focal_px` pixels (half the viewport height
//! over `tan(fov_y / 2)`), an error `e` at distance `d` moves a point on the
//! screen by at most `e * focal_px / d` pixels, whatever the viewing
//! direction. The planner uses the distance to the *nearest point of the
//! chunk's box*, so the estimate is conservative: it never undersells the
//! error of a chunk. A chunk takes the **coarsest level whose projected error
//! is at most [`LodConfig::error_px`]**; level 0 is always allowed.
//!
//! # Hysteresis
//! A chunk keeps its level between frames. It goes finer as soon as its
//! level's error exceeds the budget (so the error bound always holds), but it
//! goes coarser only when the coarser level would fit into
//! `error_px * (1 - hysteresis)`. A camera that wobbles around a switching
//! distance therefore does not make a chunk flicker between two levels.
//!
//! # Culling and draw calls
//! A chunk whose box is outside the view frustum ([`Frustum`], the same code
//! the instancing uses) is not drawn. The visible chunks of the same level
//! have consecutive index ranges in the packed index buffer
//! ([`crate::terrain::PackedTerrain`]), so chunks that are neighbours in the
//! chunk list and share a level become **one** draw call. At whole-map zoom
//! on a 256x256 map that is a handful of draws instead of 64.

use glam::{DMat4, DVec4, Mat4, Vec3};
use serde::Deserialize;
use std::fmt;
use std::ops::Range;

use crate::frustum::{Aabb, Frustum};
use crate::terrain::{LOD_LEVELS, TerrainMesh};

/// The shipped terrain settings, embedded at build time (like the unit style).
pub const EMBEDDED_TERRAIN_CONFIG: &str = include_str!("../../../assets/config/terrain.ron");

/// Marks a chunk that is not drawn in [`Selection::level`].
pub const NOT_DRAWN: u8 = u8::MAX;

/// The debug tint of each level (linear light), as in `terrain.wgsl`:
/// red, orange, yellow, green, blue, purple. The tests render with the tint
/// on and check the pixels against these.
pub const LOD_TINT: [[f32; 3]; LOD_LEVELS] = [
    [0.80, 0.05, 0.05],
    [1.00, 0.40, 0.02],
    [0.90, 0.85, 0.05],
    [0.05, 0.60, 0.10],
    [0.05, 0.25, 1.00],
    [0.55, 0.10, 0.90],
];

/// Level of detail settings (`assets/config/terrain.ron`).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LodConfig {
    /// Largest allowed projected geometric error in pixels.
    pub error_px: f32,
    /// A chunk goes to a coarser level only if that level's error fits in
    /// `error_px * (1 - hysteresis)`. `0` = no hysteresis.
    pub hysteresis: f32,
    /// Debug: draw every chunk at this level (clamped to what it has).
    pub force_level: Option<u8>,
    /// Debug: tint chunks by level (see `terrain.wgsl` for the colours).
    pub tint_levels: bool,
}

impl Default for LodConfig {
    fn default() -> Self {
        Self {
            error_px: 2.0,
            hysteresis: 0.25,
            force_level: None,
            tint_levels: false,
        }
    }
}

impl LodConfig {
    /// Every chunk at full detail (the WP1.7 look).
    pub fn full_detail() -> Self {
        Self {
            force_level: Some(0),
            ..Self::default()
        }
    }
}

/// Terrain settings (`assets/config/terrain.ron`).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerrainConfig {
    pub lod: LodConfig,
    /// Skirts under the chunk edges (see [`crate::terrain`]). `false` is a
    /// debug aid that shows the cracks between chunks of different levels.
    pub seams: bool,
}

impl Default for TerrainConfig {
    fn default() -> Self {
        Self {
            lod: LodConfig::default(),
            seams: true,
        }
    }
}

/// Why a terrain config was refused.
#[derive(Debug)]
pub enum TerrainConfigError {
    Parse(String),
    Invalid(&'static str),
}

impl fmt::Display for TerrainConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "cannot parse terrain config: {e}"),
            Self::Invalid(e) => write!(f, "invalid terrain config: {e}"),
        }
    }
}

impl std::error::Error for TerrainConfigError {}

impl TerrainConfig {
    /// The config embedded in the binary.
    pub fn embedded() -> Self {
        Self::from_ron(EMBEDDED_TERRAIN_CONFIG).expect("embedded terrain.ron is valid")
    }

    pub fn from_ron(text: &str) -> Result<Self, TerrainConfigError> {
        let config: Self =
            ron::from_str(text).map_err(|e| TerrainConfigError::Parse(e.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), TerrainConfigError> {
        let lod = &self.lod;
        if !lod.error_px.is_finite() || lod.error_px <= 0.0 {
            return Err(TerrainConfigError::Invalid(
                "lod.error_px must be a finite number above 0",
            ));
        }
        if !(0.0..=0.9).contains(&lod.hysteresis) {
            return Err(TerrainConfigError::Invalid(
                "lod.hysteresis must be between 0 and 0.9",
            ));
        }
        if lod
            .force_level
            .is_some_and(|l| usize::from(l) >= LOD_LEVELS)
        {
            return Err(TerrainConfigError::Invalid(
                "lod.force_level must be a level the terrain has (0 to 5)",
            ));
        }
        Ok(())
    }
}

/// What the level selection needs to know about the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodView {
    pub eye: Vec3,
    /// Pixels per radian at the screen centre: `viewport_height / (2 tan(fov_y / 2))`.
    pub focal_px: f32,
}

impl LodView {
    pub fn new(eye: Vec3, viewport_height_px: f32, fov_y: f32) -> Self {
        Self {
            eye,
            focal_px: 0.5 * viewport_height_px / (0.5 * fov_y).tan(),
        }
    }

    /// Recover the eye and the focal length from a perspective
    /// view-projection (wgpu clip space, rigid view): the eye is the point
    /// that maps to clip `(0, 0, z, 0)`, and the xyz part of the second row
    /// has the length `1 / tan(fov_y / 2)`. This lets the renderer work with
    /// whatever matrix it was given (the game camera or the placeholder one).
    pub fn from_view_proj(view_proj: Mat4, viewport_height_px: f32) -> Self {
        let m = DMat4::from_cols_array(&view_proj.to_cols_array().map(f64::from));
        let eye = m.inverse() * DVec4::new(0.0, 0.0, 1.0, 0.0);
        let eye = if eye.w.abs() > 1e-30 {
            (eye.truncate() / eye.w).as_vec3()
        } else {
            Vec3::ZERO
        };
        let row1 = Vec3::new(view_proj.x_axis.y, view_proj.y_axis.y, view_proj.z_axis.y);
        Self {
            eye,
            focal_px: 0.5 * viewport_height_px * row1.length(),
        }
    }

    /// Screen-space size in pixels of `error` metres at `distance` metres.
    pub fn pixels(&self, error: f32, distance: f32) -> f32 {
        error * self.focal_px / distance.max(MIN_DISTANCE)
    }
}

/// Distances below this (metres) count as this, so a camera inside a chunk's
/// box does not divide by zero.
const MIN_DISTANCE: f32 = 0.5;

/// Distance from `p` to the nearest point of `b`.
pub fn distance_to_aabb(p: Vec3, b: &Aabb) -> f32 {
    let d = (b.min - p).max(p - b.max).max(Vec3::ZERO);
    d.length()
}

/// Counters of the last frame's terrain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerrainStats {
    pub chunks_total: u32,
    /// Chunks that passed the frustum test (and are drawn).
    pub chunks_visible: u32,
    /// Visible chunks per level.
    pub per_level: [u32; LOD_LEVELS],
    /// Triangles drawn, surface and skirts.
    pub triangles: u32,
    pub draw_calls: u32,
}

/// The result of one selection.
#[derive(Clone, Debug, Default)]
pub struct Selection {
    /// Level per chunk, [`NOT_DRAWN`] if culled.
    pub level: Vec<u8>,
    /// Index ranges to draw, in draw order.
    pub draws: Vec<Range<u32>>,
    pub stats: TerrainStats,
}

/// Static data of one chunk the selection needs.
#[derive(Clone, Debug)]
struct ChunkLod {
    aabb: Aabb,
    /// Geometric error per level (metres); levels the chunk lacks are absent.
    errors: Vec<f32>,
    /// Triangles per level, surface and skirt.
    triangles: Vec<u32>,
}

/// Level selection, culling and draw list for a terrain. Holds the hysteresis
/// state; one per terrain.
#[derive(Clone, Debug)]
pub struct LodPlanner {
    chunks: Vec<ChunkLod>,
    /// `ranges[level][chunk]` of the packed index buffer.
    ranges: Vec<Vec<Range<u32>>>,
    current: Vec<u8>,
    selection: Selection,
}

impl LodPlanner {
    pub fn new(mesh: &TerrainMesh, ranges: Vec<Vec<Range<u32>>>) -> Self {
        let chunks: Vec<ChunkLod> = mesh
            .chunks
            .iter()
            .map(|c| ChunkLod {
                aabb: c.aabb,
                errors: c.levels.iter().map(|l| l.error).collect(),
                triangles: c.levels.iter().map(|l| l.triangles()).collect(),
            })
            .collect();
        let n = chunks.len();
        Self {
            chunks,
            ranges,
            current: vec![NOT_DRAWN; n],
            selection: Selection {
                level: vec![NOT_DRAWN; n],
                ..Selection::default()
            },
        }
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn chunk_aabb(&self, chunk: usize) -> &Aabb {
        &self.chunks[chunk].aabb
    }

    /// The coarsest level of a chunk.
    pub fn chunk_max_level(&self, chunk: usize) -> u8 {
        self.chunks[chunk].errors.len() as u8 - 1
    }

    /// Geometric error of a chunk at a level, metres.
    pub fn chunk_error(&self, chunk: usize, level: u8) -> f32 {
        self.chunks[chunk].errors[usize::from(level)]
    }

    /// The last selection.
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// The coarsest level of `chunk` whose projected error is at most
    /// `budget_px` (0 if even level 1 is too coarse).
    pub fn coarsest_within(&self, chunk: usize, view: &LodView, budget_px: f32) -> u8 {
        let c = &self.chunks[chunk];
        let distance = distance_to_aabb(view.eye, &c.aabb);
        let mut best = 0;
        for (level, &error) in c.errors.iter().enumerate().skip(1) {
            if view.pixels(error, distance) <= budget_px {
                best = level as u8;
            } else {
                break;
            }
        }
        best
    }

    /// Cull against `frustum`, choose a level per visible chunk and build the
    /// draw list. `levels` overrides the choice per chunk (tests and debug
    /// panels); `None` selects by the projected error.
    pub fn select(
        &mut self,
        view: &LodView,
        frustum: &Frustum,
        config: &LodConfig,
        levels: Option<&[u8]>,
    ) -> &Selection {
        let n = self.chunks.len();
        let mut stats = TerrainStats {
            chunks_total: n as u32,
            ..TerrainStats::default()
        };
        self.selection.level.resize(n, NOT_DRAWN);
        for id in 0..n {
            if !frustum.intersects(&self.chunks[id].aabb) {
                self.selection.level[id] = NOT_DRAWN;
                continue;
            }
            let max = self.chunk_max_level(id);
            let level = match (levels.and_then(|l| l.get(id)), config.force_level) {
                (Some(&forced), _) | (None, Some(forced)) => forced.min(max),
                (None, None) => {
                    let budget = config.error_px;
                    let current = self.current[id];
                    let target = self.coarsest_within(id, view, budget);
                    if current != NOT_DRAWN && current <= target {
                        // Still within the budget: coarsen only with margin.
                        let tight =
                            self.coarsest_within(id, view, budget * (1.0 - config.hysteresis));
                        current.max(tight)
                    } else {
                        target
                    }
                }
            };
            self.current[id] = level;
            self.selection.level[id] = level;
            stats.chunks_visible += 1;
            stats.per_level[usize::from(level)] += 1;
            stats.triangles += self.chunks[id].triangles[usize::from(level)];
        }

        // Draw list: per level, merge chunks with adjacent index ranges.
        self.selection.draws.clear();
        for (level, ranges) in self.ranges.iter().enumerate() {
            let mut run: Option<Range<u32>> = None;
            for (id, range) in ranges.iter().enumerate() {
                if usize::from(self.selection.level[id]) != level {
                    continue;
                }
                match &mut run {
                    Some(r) if r.end == range.start => r.end = range.end,
                    _ => {
                        if let Some(done) = run.take() {
                            self.selection.draws.push(done);
                        }
                        run = Some(range.clone());
                    }
                }
            }
            if let Some(done) = run {
                self.selection.draws.push(done);
            }
        }
        stats.draw_calls = self.selection.draws.len() as u32;
        self.selection.stats = stats;
        &self.selection
    }
}

#[cfg(test)]
mod tests;
