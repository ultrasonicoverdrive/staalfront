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
//! the instancing uses) is not drawn. The index ranges of one level are
//! consecutive in chunk order in the packed index buffer
//! ([`crate::terrain::PackedTerrain`]), so visible chunks with consecutive
//! ids and the same level become **one** draw call.
//!
//! On hilly ground neighbouring chunks want different levels and the runs
//! get short (a 1024x1024 map of hills at whole-map zoom: 459 draw calls
//! when every chunk takes its coarsest level). So after the selection the
//! planner may give a chunk a **finer** level than its budget needs when
//! that joins draw calls: it minimises `triangles + draw_call_cost * draw
//! calls` exactly (dynamic programming over the chunks in order, six states
//! each, `merge_levels`). A finer level only lowers the error, so the pixel
//! budget still holds; the price is triangles (the same map: 15 draw calls,
//! 58k instead of 28k triangles). `draw_call_cost: 0` switches it off. The
//! 256x256 map at whole-map zoom needs 18 draw calls.

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
    /// What one more draw call is worth, in triangles. The planner may draw
    /// a chunk at a *finer* level than the budget needs (never coarser) when
    /// that joins draw calls and costs fewer extra triangles than this. `0`
    /// = every chunk at its coarsest allowed level, as many draw calls as it
    /// takes.
    pub draw_call_cost: f32,
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
            draw_call_cost: DEFAULT_DRAW_CALL_COST,
            force_level: None,
            tint_levels: false,
        }
    }
}

/// Default for [`LodConfig::draw_call_cost`].
pub const DEFAULT_DRAW_CALL_COST: f32 = 300.0;

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
        if !lod.draw_call_cost.is_finite() || lod.draw_call_cost < 0.0 {
            return Err(TerrainConfigError::Invalid(
                "lod.draw_call_cost must be a finite number of triangles, 0 or more",
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
    scratch: MergeScratch,
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
            scratch: MergeScratch::default(),
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
        coarsest_within(&self.chunks[chunk], view, budget_px)
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
        self.selection.level.resize(n, NOT_DRAWN);

        // 1. Cull, and choose each visible chunk's level.
        let scratch = &mut self.scratch;
        scratch.ids.clear();
        let mut automatic = true;
        for id in 0..n {
            if !frustum.intersects(&self.chunks[id].aabb) {
                self.selection.level[id] = NOT_DRAWN;
                continue;
            }
            let max = self.chunks[id].errors.len() as u8 - 1;
            let forced = levels
                .and_then(|l| l.get(id))
                .copied()
                .or(config.force_level);
            let level = if let Some(forced) = forced {
                automatic = false;
                forced.min(max)
            } else {
                let budget = config.error_px;
                let current = self.current[id];
                let target = coarsest_within(&self.chunks[id], view, budget);
                if current != NOT_DRAWN && current <= target {
                    // Still within the budget: coarsen only with margin.
                    let tight =
                        coarsest_within(&self.chunks[id], view, budget * (1.0 - config.hysteresis));
                    current.max(tight)
                } else {
                    target
                }
            };
            self.selection.level[id] = level;
            scratch.ids.push(id as u32);
        }

        // 2. Join draw calls: where it pays, a chunk takes a finer level
        // than its budget needs, so that neighbours share one.
        if automatic && config.draw_call_cost > 0.0 {
            let chunks = &self.chunks;
            let selection = &mut self.selection;
            scratch.targets.clear();
            scratch
                .targets
                .extend(scratch.ids.iter().map(|&id| selection.level[id as usize]));
            let ids = &scratch.ids;
            merge_levels(
                ids,
                &scratch.targets,
                |k, level| chunks[ids[k] as usize].triangles[usize::from(level)] as f32,
                config.draw_call_cost,
                &mut scratch.dp,
                &mut scratch.back,
                &mut scratch.chosen,
            );
            for (&id, &level) in scratch.ids.iter().zip(&scratch.chosen) {
                selection.level[id as usize] = level;
            }
        }

        // 3. Counters, and the draw list: per level, chunks with adjacent
        // index ranges become one range.
        let mut stats = TerrainStats {
            chunks_total: n as u32,
            ..TerrainStats::default()
        };
        for &id in &scratch.ids {
            let id = id as usize;
            let level = self.selection.level[id];
            self.current[id] = level;
            stats.chunks_visible += 1;
            stats.per_level[usize::from(level)] += 1;
            stats.triangles += self.chunks[id].triangles[usize::from(level)];
        }
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

/// The coarsest level of a chunk whose projected error is at most
/// `budget_px` (0 if even level 1 is too coarse).
fn coarsest_within(c: &ChunkLod, view: &LodView, budget_px: f32) -> u8 {
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

/// Working memory of [`merge_levels`], kept between frames.
#[derive(Clone, Debug, Default)]
struct MergeScratch {
    /// Ids of the visible chunks, ascending.
    ids: Vec<u32>,
    targets: Vec<u8>,
    chosen: Vec<u8>,
    dp: Vec<f32>,
    back: Vec<u8>,
}

/// Pick a level for every visible chunk, at most its `target` (the error
/// budget), so that triangles plus `cost` per draw call is smallest. `ids`
/// are the ascending ids of the visible chunks. A draw call is a run of
/// chunks with consecutive ids and the same level (their index ranges touch),
/// so the cost counts the runs. `triangles(k, level)` is the triangle count
/// of the `k`-th visible chunk at a level. Exact dynamic programming over the
/// chunks in order, at most six states each.
fn merge_levels(
    ids: &[u32],
    target: &[u8],
    triangles: impl Fn(usize, u8) -> f32,
    cost: f32,
    dp: &mut Vec<f32>,
    back: &mut Vec<u8>,
    out: &mut Vec<u8>,
) {
    const L: usize = LOD_LEVELS;
    let n = ids.len();
    out.clear();
    if n == 0 {
        return;
    }
    dp.clear();
    dp.resize(n * L, f32::INFINITY);
    back.clear();
    back.resize(n * L, 0);
    for k in 0..n {
        for level in 0..=usize::from(target[k]) {
            let own = triangles(k, level as u8);
            if k == 0 {
                dp[level] = own + cost;
                continue;
            }
            // From the previous chunk at any level; a new run unless it has
            // the same level and the next id.
            let adjacent = ids[k - 1] + 1 == ids[k];
            let (mut best, mut from) = (f32::INFINITY, 0_u8);
            for prev in 0..=usize::from(target[k - 1]) {
                let new_run = !(adjacent && prev == level);
                let total = dp[(k - 1) * L + prev] + if new_run { cost } else { 0.0 };
                if total < best {
                    (best, from) = (total, prev as u8);
                }
            }
            dp[k * L + level] = best + own;
            back[k * L + level] = from;
        }
    }
    // Walk back from the cheapest last level.
    let last = (0..=usize::from(target[n - 1]))
        .min_by(|&a, &b| dp[(n - 1) * L + a].total_cmp(&dp[(n - 1) * L + b]))
        .unwrap_or(0);
    out.resize(n, 0);
    let mut level = last as u8;
    for k in (0..n).rev() {
        out[k] = level;
        level = back[k * L + usize::from(level)];
    }
}

#[cfg(test)]
mod tests;
