//! Instanced drawing of many small objects (units): interpolation between two
//! sim ticks, bucketing by map tile, frustum culling per tile, one instance
//! list per mesh kind.
//!
//! # Data flow (PLAN.md section 1: sim -> render is one way)
//! * Once per **sim tick** the game hands the [`Instancer`] one [`Motion`] per
//!   entity ([`Instancer::update_tick`]). A `Motion` holds the previous and
//!   current position and facing already converted to `f32` through
//!   `sf_math::render_bridge` (the only fixed-point to float conversion), so
//!   this crate never needs `sf_sim` and the per-frame work is float-only.
//!   The motions are bucketed by map tile (counting sort, O(n)).
//! * Once per **frame** [`Instancer::build`] takes the frustum of the camera
//!   and the interpolation factor `alpha`; tiles whose bounding box is
//!   outside the frustum are skipped, the entities of the visible tiles are
//!   interpolated (`lerp(prev, current, alpha)`, facing along the shortest
//!   arc), put on the ground (display only: the sim stays 2D) and appended
//!   to the instance list of their mesh kind.
//! * The caller uploads the lists with `Renderer::upload_units`: one
//!   instance buffer and one draw call per list.
//!
//! # Levels of detail, icons and picking (WP1.9)
//! [`Instancer::build_frame`] takes a [`FrameView`]: besides the frustum and
//! `alpha` it carries the camera (for the level selection) and the
//! model/icon [`Blend`] of the frame. For every visible unit it
//! * picks the level of its model's chain by projected size, with
//!   hysteresis (the unit's last level is kept in a byte per entity slot),
//!   and appends it to the list of that level's mesh: **one list, one
//!   buffer and one draw call per (mesh kind, level)**, see [`crate::model`];
//! * appends a model instance whose alpha is the model weight of the blend
//!   (the dissolve threshold of the cross-fade) unless the weight is 0, and
//!   an [`IconInstance`] unless it is 1, see [`crate::icons`];
//! * records what it drew (id, the interpolated position it stands at, the
//!   facing, where its instances went) so that [`Instancer::pick`] answers
//!   from exactly the frame on the screen and [`Instancer::highlight`] can
//!   tint the picked unit's instances before they are uploaded.
//!
//! # Tiles
//! The map is divided into square tiles of [`DEFAULT_TILE_SIZE`] m (32 m, the
//! same as a terrain chunk, so one tile's units sit on one chunk). An
//! entity belongs to the tile of its *current* position. The tile's box is
//! not the nominal tile square but the box around the previous and current
//! positions of all its members, grown by the largest shape footprint; since
//! an interpolated position always lies between the two, no member can
//! stick out of its tile's box. Its height range is the terrain relief
//! (`set_height_range`) plus the tallest shape, so units standing on hills
//! are never culled wrongly. Culling therefore never drops a unit that
//! would be (even partly) visible.

use std::f32::consts::TAU;

use glam::{Mat4, Vec3};
use sf_math::render_bridge::vec2_cells_to_metres;
use sf_math::{Facing, Vec2};

use crate::frustum::{Aabb, Frustum};
use crate::icons::{
    Blend, Dominant, ICON_HIGHLIGHT, IconConfig, IconInstance, IconParams, snap_icon_center,
};
use crate::model::{EyeView, MAX_LODS, MAX_PARTS, MeshId, ModelSet, PartAngles, pose_parts};
use crate::pick::{
    Pick, PickVia, UnitId, click_margin, click_reach, icon_hit, project_px, ray_hits_aabb, ray_obb,
};
use crate::scene::{Instance, MeshKind};
use crate::style::Style;

/// Tile edge in metres; equals the terrain chunk size (32 terrain cells of
/// 1 m, which is 8 gameplay cells of 4 m).
pub const DEFAULT_TILE_SIZE: f32 = 32.0;

/// Marks "no level yet" in the per-entity hysteresis state.
const NO_LEVEL: u8 = u8::MAX;

/// Entity slots above this get no hysteresis state (a guard against a huge
/// id allocating gigabytes; real worlds have a few ten thousand slots).
const MAX_LOD_STATE: usize = 1 << 22;

/// One entity's motion over the last sim tick, in render units (metres,
/// facing in 1/256 turns).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    /// Index into the style's shapes (wraps around).
    pub shape: u8,
    /// Index into the style's palette (wraps around): the owner.
    pub player: u8,
    /// The handle [`Instancer::pick`] returns for this unit. Left at
    /// [`UnitId::NONE`], [`Instancer::update_tick`] numbers the unit by its
    /// place in the list.
    pub id: UnitId,
    prev: [f32; 2],
    pos: [f32; 2],
    prev_facing: u8,
    facing: u8,
}

impl Motion {
    /// From the fixed-point state of the previous and current tick. The
    /// positions are in simulation **cells**; this is where they become
    /// metres (`sf_math::CELL_METRES` per cell, decision B12). The sim's `y`
    /// is the render `z`.
    pub fn from_fixed(
        shape: u8,
        player: u8,
        prev_pos: Vec2,
        pos: Vec2,
        prev_facing: Facing,
        facing: Facing,
    ) -> Self {
        Self {
            shape,
            player,
            id: UnitId::NONE,
            prev: vec2_cells_to_metres(prev_pos),
            pos: vec2_cells_to_metres(pos),
            prev_facing: prev_facing.raw(),
            facing: facing.raw(),
        }
    }

    /// The same motion with the handle that picking returns for it (the
    /// entity id, packed with [`UnitId::from_parts`]).
    pub fn with_id(mut self, id: UnitId) -> Self {
        self.id = id;
        self
    }

    /// Position `(x, z)` and facing in steps at fraction `alpha` of the way
    /// from the previous to the current tick (clamped to `0..=1`). The
    /// returned facing is unwrapped (it may leave `0..256`): the previous
    /// facing plus the shortest signed turn times `alpha`, so 255 -> 1
    /// passes through 0, not through 128. `alpha = 0` is exactly the
    /// previous state, `alpha = 1` exactly the current one.
    pub fn at(&self, alpha: f32) -> ([f32; 2], f32) {
        let a = alpha.clamp(0.0, 1.0);
        let lerp = |p: f32, c: f32| p * (1.0 - a) + c * a;
        let pos = [
            lerp(self.prev[0], self.pos[0]),
            lerp(self.prev[1], self.pos[1]),
        ];
        let turn = Facing::new(self.prev_facing).signed_diff(Facing::new(self.facing));
        // Facing steps are small integers (0..=255): exact in f32.
        let steps = f32::from(self.prev_facing) + f32::from(turn) * a;
        (pos, steps)
    }

    /// The tile-box corner points: previous and current position.
    fn extent(&self) -> ([f32; 2], [f32; 2]) {
        (self.prev, self.pos)
    }
}

/// Model matrix of a shape standing at `(x, y, z)` facing `steps`
/// (1/256 turns; 0 = along `+x`, increasing turns towards `+z`), scaled by
/// `size` = `(along the facing, height, across)`.
pub fn model_matrix(x: f32, y: f32, z: f32, steps: f32, size: [f32; 3]) -> [[f32; 4]; 4] {
    let (s, c) = (steps * (TAU / 256.0)).sin_cos();
    [
        [c * size[0], 0.0, s * size[0], 0.0],
        [0.0, size[1], 0.0, 0.0],
        [-s * size[2], 0.0, c * size[2], 0.0],
        [x, y, z, 1.0],
    ]
}

/// What one [`Instancer::build`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InstanceStats {
    /// All entities known to the instancer.
    pub total: u32,
    /// Entities that went into the instance lists (inside visible tiles).
    pub visible: u32,
    /// Non-empty tiles.
    pub tiles_total: u32,
    /// Non-empty tiles that passed the frustum test.
    pub tiles_visible: u32,
    /// Visible entities per [`MeshKind::index`].
    pub per_mesh: [u32; MeshKind::ALL.len()],
    /// All entities per [`MeshKind::index`] (visible or not).
    pub total_per_mesh: [u32; MeshKind::ALL.len()],
    /// Visible units drawn as models, per level of detail (index 0 = finest).
    /// Their sum is `models`; the rest of `visible` is drawn as icons only.
    pub per_lod: [u32; MAX_LODS],
    /// Visible units drawn as models (any level).
    pub models: u32,
    /// Visible units drawn as icons.
    pub icons: u32,
    /// Triangles of the model instances submitted.
    pub model_triangles: u64,
    /// Draw calls of the model lists: one per non-empty (mesh kind, level).
    pub model_draw_calls: u32,
    /// Draw calls of the icons: 1 when there are any.
    pub icon_draw_calls: u32,
    /// Model instances submitted (one per part of a unit drawn as a model;
    /// equal to `models` for single-mesh models).
    pub part_instances: u32,
}

impl InstanceStats {
    /// Draw calls needed for the visible instances: one per non-empty
    /// instance list (a mesh kind at a level of detail) plus one for the icons.
    pub fn draw_calls(&self) -> u32 {
        self.model_draw_calls + self.icon_draw_calls
    }
}

/// What one [`Instancer::build_frame`] needs to know about the frame.
#[derive(Clone, Copy, Debug)]
pub struct FrameView<'a> {
    /// The camera's view frustum (tile culling).
    pub frustum: &'a Frustum,
    /// Interpolation factor between the previous and the current tick.
    pub alpha: f32,
    /// The camera for the level selection and the click volumes; `None` =
    /// every model at its finest level.
    pub eye: Option<EyeView>,
    /// How much of the picture is models and how much is icons.
    pub blend: Blend,
    /// UI scale (window DPI factor times `ui.ron` scale): icons, their
    /// zoom thresholds and the minimum click size scale with it.
    pub ui_scale: f32,
}

impl<'a> FrameView<'a> {
    /// Models only, all at their finest level: the view of WP1.6.
    pub fn models_only(frustum: &'a Frustum, alpha: f32) -> Self {
        Self {
            frustum,
            alpha,
            eye: None,
            blend: Blend::MODELS,
            ui_scale: 1.0,
        }
    }

    /// The full view: levels of detail by `eye`, models and icons by `blend`.
    pub fn new(
        frustum: &'a Frustum,
        alpha: f32,
        eye: EyeView,
        blend: Blend,
        ui_scale: f32,
    ) -> Self {
        Self {
            frustum,
            alpha,
            eye: Some(eye),
            blend,
            ui_scale,
        }
    }
}

/// What a frame was built with, kept for the picking.
#[derive(Clone, Copy, Debug)]
struct FrameState {
    blend: Blend,
    ui_scale: f32,
}

/// Marks a record without a model or icon instance.
const NO_INSTANCE: u32 = u32::MAX;

/// What one visible unit looked like in the last frame.
#[derive(Clone, Copy, Debug)]
struct Drawn {
    id: UnitId,
    /// Where the model stands: interpolated `x`, terrain height, `z`.
    pos: [f32; 3],
    /// Interpolated facing in 1/256 turns.
    steps: f32,
    shape: u8,
    /// The mesh list and the index in it of the unit's model instance (its
    /// first part).
    model_mesh: u16,
    model_index: u32,
    /// The instances of the further parts (`extra_parts` of them).
    extra: [(u16, u32); MAX_PARTS - 1],
    extra_parts: u8,
    /// Index in the icon list.
    icon_index: u32,
}

/// The records of one visible tile.
#[derive(Clone, Copy, Debug)]
struct VisibleTile {
    tile: u32,
    start: u32,
    end: u32,
}

/// Buckets motions by tile, culls tiles against a frustum and builds the
/// per-mesh instance lists. Buffers are reused between frames.
pub struct Instancer {
    style: Style,
    models: ModelSet,
    lod_hysteresis: f32,
    icon_config: IconConfig,
    tile_size: f32,
    tiles_x: usize,
    tiles_z: usize,
    motions: Vec<Motion>,
    /// Entity indices grouped by tile (`tile_start[t]..tile_start[t + 1]`).
    order: Vec<u32>,
    tile_start: Vec<u32>,
    tile_boxes: Vec<Aabb>,
    footprint: f32,
    tallest: f32,
    ground: (f32, f32),
    /// One list per mesh of the model set (a mesh kind at a level of detail).
    lists: Vec<Vec<Instance>>,
    icons: Vec<IconInstance>,
    /// Last level of each entity slot, for the hysteresis.
    lod_state: Vec<u8>,
    /// Entities per mesh kind, counted when the tick is bucketed.
    total_per_mesh: [u32; MeshKind::ALL.len()],
    stats: InstanceStats,
    frame: Option<FrameState>,
    records: Vec<Drawn>,
    visible_tiles: Vec<VisibleTile>,
    /// Where the turrets aim (`x`, `z`), the WP1.10 demo; `None` = straight ahead.
    aim: Option<[f32; 2]>,
}

/// `(footprint, tallest)` of a style and its models: half the diagonal of
/// the widest footprint (covers any rotation) and the tallest shape; a model
/// with parts counts with its reach (the farthest a vertex can get from the
/// unit's origin with the joints turned).
fn style_extent(style: &Style, models: &ModelSet) -> (f32, f32) {
    let footprint = style
        .shapes()
        .iter()
        .map(|s| 0.5 * s.size[0].hypot(s.size[2]))
        .fold(0.0, f32::max);
    let tallest = style.shapes().iter().map(|s| s.size[1]).fold(0.0, f32::max);
    let reach = models
        .models()
        .iter()
        .filter_map(|m| m.bounds())
        .map(|b| b.reach)
        .fold(0.0, f32::max);
    (footprint.max(reach), tallest.max(reach))
}

/// What a unit's model looks like to the culling, the level selection and
/// the picking: a box in the unit's frame, and the scale of its mesh.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ShapeGeom {
    /// `(along the facing, height from the ground, across)` of the click box.
    size: [f32; 3],
    /// Centre of the box's footprint in the unit's frame (along, across).
    offset: [f32; 2],
    /// Diameter for the level selection.
    diameter: f32,
    /// Scale of the mesh in the model matrix: the shape size for the unit
    /// placeholder meshes, 1 for glTF models (real metres).
    scale: [f32; 3],
}

/// The world position of a point `offset` (along, across) in the frame of a
/// unit at `pos` facing `steps`.
fn offset_point(pos: Vec3, steps: f32, offset: [f32; 2]) -> Vec3 {
    if offset == [0.0, 0.0] {
        return pos;
    }
    let (s, c) = (steps * (TAU / 256.0)).sin_cos();
    pos + Vec3::new(
        offset[0] * c - offset[1] * s,
        0.0,
        offset[0] * s + offset[1] * c,
    )
}

/// The turret and barrel angles of the demo: the turret turns towards
/// `aim`, the barrel rises a little with the distance to it.
pub fn aim_angles(x: f32, z: f32, steps: f32, aim: Option<[f32; 2]>) -> PartAngles {
    let Some([ax, az]) = aim else {
        return PartAngles::default();
    };
    let (dx, dz) = (ax - x, az - z);
    if dx == 0.0 && dz == 0.0 {
        return PartAngles::default();
    }
    let facing = steps * (TAU / 256.0);
    PartAngles {
        yaw: dz.atan2(dx) - facing,
        pitch: (dx.hypot(dz) / 400.0).clamp(0.03, 0.35),
    }
}

impl Instancer {
    /// For a map of `map_size` metres (x, z) with the default 32 m tiles.
    pub fn new(style: Style, map_size: (f32, f32)) -> Self {
        Self::with_tile_size(style, map_size, DEFAULT_TILE_SIZE)
    }

    pub fn with_tile_size(style: Style, map_size: (f32, f32), tile_size: f32) -> Self {
        assert!(tile_size > 0.0, "tile size must be positive");
        let tiles_x = (map_size.0 / tile_size).ceil().max(1.0) as usize;
        let tiles_z = (map_size.1 / tile_size).ceil().max(1.0) as usize;
        let models = ModelSet::from_style(&style);
        let (footprint, tallest) = style_extent(&style, &models);
        Self {
            lod_hysteresis: style.lod_settings().hysteresis,
            icon_config: *style.icons(),
            lists: vec![Vec::new(); models.mesh_count()],
            models,
            style,
            tile_size,
            tiles_x,
            tiles_z,
            motions: Vec::new(),
            order: Vec::new(),
            tile_start: vec![0; tiles_x * tiles_z + 1],
            tile_boxes: vec![Aabb::new(Vec3::ZERO, Vec3::ZERO); tiles_x * tiles_z],
            footprint,
            tallest,
            ground: (0.0, 0.0),
            icons: Vec::new(),
            lod_state: Vec::new(),
            total_per_mesh: [0; MeshKind::ALL.len()],
            stats: InstanceStats::default(),
            frame: None,
            records: Vec::new(),
            visible_tiles: Vec::new(),
            aim: None,
        }
    }

    /// The WP1.10 demo of the part hierarchy: every turret turns towards
    /// the point `(x, z)` and its barrel rises a little; `None` = turrets
    /// straight ahead. Takes effect in the next build.
    pub fn set_aim(&mut self, aim: Option<[f32; 2]>) {
        self.aim = aim;
    }

    /// The look of shape `shape` to the culling, the level selection and
    /// the picking: the model's real bounds (finest level) if it has them,
    /// else the style's shape size.
    fn geom(&self, shape: u8) -> ShapeGeom {
        let s = self.style.shape(shape);
        match self.models.model(shape).bounds() {
            Some(b) => ShapeGeom {
                size: [b.max.x - b.min.x, b.max.y.max(0.01), b.max.z - b.min.z],
                offset: [0.5 * (b.min.x + b.max.x), 0.5 * (b.min.z + b.max.z)],
                diameter: b.diameter(),
                scale: [1.0; 3],
            },
            None => ShapeGeom {
                size: s.size,
                offset: [0.0, 0.0],
                diameter: s.bounding_diameter(),
                scale: s.size,
            },
        }
    }

    /// Swap the look (palette, shapes, level-of-detail chains, icon
    /// settings), e.g. after `units.ron` was edited. The entities are
    /// re-bucketed because the culling margins depend on the shape sizes; the
    /// next [`Instancer::build_frame`] uses the new look. The model set
    /// changes, so [`Renderer::upload_units`](crate::Renderer::upload_units)
    /// registers its meshes again.
    pub fn set_style(&mut self, style: Style) {
        self.lod_hysteresis = style.lod_settings().hysteresis;
        self.icon_config = *style.icons();
        self.style = style;
        self.set_models(ModelSet::from_style(&self.style));
    }

    /// Swap the look and the models together (a style whose shapes go with
    /// glTF models, see [`Instancer::set_models`]).
    pub fn set_style_and_models(&mut self, style: Style, models: ModelSet) {
        self.lod_hysteresis = style.lod_settings().hysteresis;
        self.icon_config = *style.icons();
        self.style = style;
        self.set_models(models);
    }

    /// Use another set of models (meshes and chains per shape), for example
    /// with glTF meshes (WP1.10). The set must have a chain for every shape
    /// of the style that is in use (indices wrap around).
    pub fn set_models(&mut self, models: ModelSet) {
        self.lists = vec![Vec::new(); models.mesh_count()];
        self.icons.clear();
        self.lod_state.fill(NO_LEVEL);
        self.records.clear();
        self.visible_tiles.clear();
        self.frame = None;
        self.models = models;
        // The culling margins depend on the model extents.
        (self.footprint, self.tallest) = style_extent(&self.style, &self.models);
        self.rebucket();
    }

    pub fn style(&self) -> &Style {
        &self.style
    }

    /// The models: their chains, and the meshes the renderer needs.
    pub fn models(&self) -> &ModelSet {
        &self.models
    }

    /// The icon settings of the style.
    pub fn icon_config(&self) -> &IconConfig {
        &self.icon_config
    }

    pub fn tile_size(&self) -> f32 {
        self.tile_size
    }

    /// Lowest and highest terrain height (metres): the tile boxes get this
    /// vertical range so relief cannot hide a unit. Entities already known
    /// are re-bucketed, so the order of this call and
    /// [`Instancer::update_tick`] does not matter (found by the relief
    /// culling test: the boxes used to keep the old range until the next
    /// tick).
    pub fn set_height_range(&mut self, min: f32, max: f32) {
        self.ground = (min.min(max), max.max(min));
        if !self.motions.is_empty() {
            self.rebucket();
        }
    }

    pub fn len(&self) -> usize {
        self.motions.len()
    }

    /// The most instances each mesh of the model set can ever get from the
    /// entities now known (index = [`MeshId`]): per entity, the most times its
    /// model uses the mesh in one level (a model whose two parts share a mesh
    /// counts it twice), summed over the entities. A frame never exceeds it, so
    /// a renderer can allocate that much once, at load, instead of growing
    /// the buffers on the frame a crowd first comes into view.
    pub fn mesh_capacity(&self) -> Vec<u32> {
        let chains = self.models.models();
        let mut per_shape = vec![0_u32; chains.len()];
        for m in &self.motions {
            per_shape[usize::from(m.shape) % chains.len()] += 1;
        }
        let mut bound = vec![0_u32; self.models.mesh_count()];
        for (chain, &count) in chains.iter().zip(&per_shape) {
            for (mesh, uses) in chain.peak_mesh_uses() {
                bound[mesh.index()] += count * uses;
            }
        }
        bound
    }

    pub fn is_empty(&self) -> bool {
        self.motions.is_empty()
    }

    /// Replace the entities (call once per sim tick) and bucket them. A
    /// motion without an id is numbered by its place in the list.
    pub fn update_tick(&mut self, motions: impl IntoIterator<Item = Motion>) {
        self.motions.clear();
        self.motions.extend(motions);
        let mut highest = 0;
        for (i, m) in self.motions.iter_mut().enumerate() {
            if m.id == UnitId::NONE {
                m.id = UnitId::from_parts(i as u32, 0);
            }
            highest = highest.max(m.id.index() as usize);
        }
        // One byte of hysteresis state per entity slot, kept between ticks
        // (slots are stable in the simulation, so are the levels).
        let wanted = (highest + 1).min(MAX_LOD_STATE);
        if self.lod_state.len() < wanted && !self.motions.is_empty() {
            self.lod_state.resize(wanted, NO_LEVEL);
        }
        self.rebucket();
    }

    fn tile_of(&self, p: [f32; 2]) -> usize {
        let tx = ((p[0] / self.tile_size).max(0.0) as usize).min(self.tiles_x - 1);
        let tz = ((p[1] / self.tile_size).max(0.0) as usize).min(self.tiles_z - 1);
        tz * self.tiles_x + tx
    }

    fn rebucket(&mut self) {
        let tiles = self.tiles_x * self.tiles_z;
        self.tile_start.clear();
        self.tile_start.resize(tiles + 1, 0);
        self.total_per_mesh = [0; MeshKind::ALL.len()];
        for m in &self.motions {
            let t = self.tile_of(m.pos);
            self.tile_start[t + 1] += 1;
            self.total_per_mesh[self.style.shape(m.shape).mesh.index()] += 1;
        }
        for t in 0..tiles {
            self.tile_start[t + 1] += self.tile_start[t];
        }
        // Fill with a moving cursor per tile.
        let mut cursor: Vec<u32> = self.tile_start[..tiles].to_vec();
        self.order.clear();
        self.order.resize(self.motions.len(), 0);
        let (lo_y, hi_y) = (self.ground.0, self.ground.1 + self.tallest);
        let mut lo = vec![[f32::MAX; 2]; tiles];
        let mut hi = vec![[f32::MIN; 2]; tiles];
        for (i, m) in self.motions.iter().enumerate() {
            let t = self.tile_of(m.pos);
            self.order[cursor[t] as usize] = i as u32;
            cursor[t] += 1;
            let (a, b) = m.extent();
            for p in [a, b] {
                lo[t] = [lo[t][0].min(p[0]), lo[t][1].min(p[1])];
                hi[t] = [hi[t][0].max(p[0]), hi[t][1].max(p[1])];
            }
        }
        for t in 0..tiles {
            if self.tile_start[t] == self.tile_start[t + 1] {
                continue;
            }
            let r = self.footprint;
            self.tile_boxes[t] = Aabb::new(
                Vec3::new(lo[t][0] - r, lo_y, lo[t][1] - r),
                Vec3::new(hi[t][0] + r, hi_y, hi[t][1] + r),
            );
        }
    }

    /// Cull tiles against `frustum` and rebuild the instance lists for the
    /// interpolation factor `alpha`, models only and all at their finest
    /// level (the WP1.6 picture). `ground(x, z)` is the terrain height the
    /// unit stands on (display only). Returns the counters. See
    /// [`Instancer::build_frame`] for levels of detail and icons.
    pub fn build(
        &mut self,
        frustum: &Frustum,
        alpha: f32,
        ground: impl Fn(f32, f32) -> f32,
    ) -> InstanceStats {
        self.build_frame(&FrameView::models_only(frustum, alpha), ground)
    }

    /// Cull tiles, select the level of detail of every visible model, and
    /// rebuild the model lists and the icon list for `view`. `ground(x, z)`
    /// is the terrain height the unit stands on (display only). Returns the
    /// counters.
    pub fn build_frame(
        &mut self,
        view: &FrameView<'_>,
        ground: impl Fn(f32, f32) -> f32,
    ) -> InstanceStats {
        for list in &mut self.lists {
            list.clear();
        }
        self.icons.clear();
        self.records.clear();
        self.visible_tiles.clear();
        let blend = view.blend;
        let (models_on, icons_on) = (blend.models_drawn(), blend.icons_drawn());
        let weight = blend.model();
        let lift = self.icon_config.lift;
        let mut stats = InstanceStats {
            total: self.motions.len() as u32,
            total_per_mesh: self.total_per_mesh,
            ..InstanceStats::default()
        };
        for t in 0..self.tiles_x * self.tiles_z {
            let (start, end) = (self.tile_start[t] as usize, self.tile_start[t + 1] as usize);
            if start == end {
                continue;
            }
            stats.tiles_total += 1;
            if !view.frustum.intersects(&self.tile_boxes[t]) {
                continue;
            }
            stats.tiles_visible += 1;
            let first = self.records.len() as u32;
            for &i in &self.order[start..end] {
                let m = &self.motions[i as usize];
                let ([x, z], steps) = m.at(view.alpha);
                let y = ground(x, z);
                let shape = self.style.shape(m.shape);
                let geom = self.geom(m.shape);
                let [r, g, b] = self.style.linear(m.player);
                let mut drawn = Drawn {
                    id: m.id,
                    pos: [x, y, z],
                    steps,
                    shape: m.shape,
                    model_mesh: 0,
                    model_index: NO_INSTANCE,
                    extra: [(0, NO_INSTANCE); MAX_PARTS - 1],
                    extra_parts: 0,
                    icon_index: NO_INSTANCE,
                };
                if models_on {
                    let chain = self.models.model(m.shape);
                    let level = match view.eye {
                        Some(eye) if chain.len() > 1 => {
                            let middle = offset_point(
                                Vec3::new(x, y + 0.5 * geom.size[1], z),
                                steps,
                                geom.offset,
                            );
                            let px = eye.projected_px(geom.diameter, (middle - eye.eye).length());
                            let slot = m.id.index() as usize;
                            let previous =
                                self.lod_state.get(slot).copied().filter(|&l| l != NO_LEVEL);
                            let level = chain.select(px, previous, self.lod_hysteresis);
                            if let Some(state) = self.lod_state.get_mut(slot) {
                                *state = level;
                            }
                            level
                        }
                        _ => 0,
                    };
                    // The alpha is the model's weight in the cross-fade: the
                    // fragment shader keeps that share of the pixels.
                    let color = [r, g, b, weight];
                    let unit = model_matrix(x, y, z, steps, geom.scale);
                    let parts = chain.parts(level);
                    if parts.is_empty() {
                        let mesh = chain.level(level).mesh;
                        let list = &mut self.lists[mesh.index()];
                        drawn.model_mesh = mesh.0;
                        drawn.model_index = list.len() as u32;
                        list.push(Instance { model: unit, color });
                        stats.part_instances += 1;
                    } else {
                        // Hull, turret, barrel: one instance in each part's list.
                        let mut world = [Mat4::IDENTITY; MAX_PARTS];
                        let angles = aim_angles(x, z, steps, self.aim);
                        pose_parts(Mat4::from_cols_array_2d(&unit), parts, angles, &mut world);
                        for (i, part) in parts.iter().enumerate().take(MAX_PARTS) {
                            // A part kept only as the frame of its children
                            // has nothing to draw.
                            if i > 0 && self.models.mesh_triangles(part.mesh) == 0 {
                                continue;
                            }
                            let list = &mut self.lists[part.mesh.index()];
                            let at = (part.mesh.0, list.len() as u32);
                            list.push(Instance {
                                model: world[i].to_cols_array_2d(),
                                color,
                            });
                            if i == 0 {
                                (drawn.model_mesh, drawn.model_index) = at;
                            } else {
                                drawn.extra[usize::from(drawn.extra_parts)] = at;
                                drawn.extra_parts += 1;
                            }
                            stats.part_instances += 1;
                        }
                    }
                    stats.per_lod[usize::from(level).min(MAX_LODS - 1)] += 1;
                    stats.models += 1;
                }
                if icons_on {
                    drawn.icon_index = self.icons.len() as u32;
                    self.icons.push(IconInstance::new(
                        [x, y + lift, z],
                        shape.icon,
                        [r, g, b],
                        blend,
                    ));
                    stats.icons += 1;
                }
                self.records.push(drawn);
                stats.per_mesh[shape.mesh.index()] += 1;
                stats.visible += 1;
            }
            self.visible_tiles.push(VisibleTile {
                tile: t as u32,
                start: first,
                end: self.records.len() as u32,
            });
        }
        for (mesh, list) in self.lists.iter().enumerate() {
            if !list.is_empty() {
                stats.model_draw_calls += 1;
                stats.model_triangles +=
                    list.len() as u64 * u64::from(self.models.mesh_triangles(MeshId(mesh as u16)));
            }
        }
        stats.icon_draw_calls = u32::from(!self.icons.is_empty());
        self.frame = Some(FrameState {
            blend,
            ui_scale: view.ui_scale,
        });
        self.stats = stats;
        stats
    }

    /// The instances of the last build for one mesh kind at its finest
    /// level, if the style's chains start with the built-in meshes (the
    /// default). Use [`Instancer::mesh_instances`] for any mesh.
    pub fn instances(&self, mesh: MeshKind) -> &[Instance] {
        &self.lists[mesh.index()]
    }

    /// The instances of the last build that use `mesh` (a mesh kind at one
    /// level of detail): one draw call.
    pub fn mesh_instances(&self, mesh: MeshId) -> &[Instance] {
        self.lists.get(mesh.index()).map_or(&[], Vec::as_slice)
    }

    /// The icons of the last build: one draw call.
    pub fn icon_instances(&self) -> &[IconInstance] {
        &self.icons
    }

    /// The icon sizes of the last build in physical pixels (the settings
    /// times the frame's UI scale).
    pub fn icon_params(&self) -> IconParams {
        let scale = self.frame.map_or(1.0, |f| f.ui_scale);
        IconParams {
            size_px: self.icon_config.icon_px(scale),
            outline_px: self.icon_config.outline_px * scale,
        }
    }

    /// The model/icon blend of the last build.
    pub fn blend(&self) -> Blend {
        self.frame.map_or(Blend::MODELS, |f| f.blend)
    }

    /// The counters of the last build.
    pub fn stats(&self) -> InstanceStats {
        self.stats
    }

    /// The unit under the cursor in the frame of the last
    /// [`Instancer::build_frame`], or `None`. Which representation is
    /// tested depends on the blend: the model's bounding box while models
    /// are at least half of the picture, the icon's screen quad beyond. See
    /// [`crate::pick`] for the rules and the tie-breaks.
    pub fn pick(&self, query: &crate::pick::PickQuery) -> Option<Pick> {
        let frame = self.frame?;
        let eye = EyeView::from_view_proj(query.view_proj, query.viewport[1]);
        let ray = query.ray()?;
        let best = match frame.blend.dominant() {
            Dominant::Model => self.pick_model(query, &ray, &eye, frame.ui_scale),
            Dominant::Icon => self.pick_icon(query, &ray, &eye, frame.ui_scale),
        };
        best.map(|c| Pick {
            id: c.id,
            via: c.via,
            distance: c.distance,
            cursor_distance: c.cursor_distance,
            record: c.record,
        })
    }

    fn pick_model(
        &self,
        query: &crate::pick::PickQuery,
        ray: &crate::pick::PickRay,
        eye: &EyeView,
        ui_scale: f32,
    ) -> Option<Candidate> {
        let click_px = self.icon_config.click_min_px * ui_scale;
        let mut best: Option<Candidate> = None;
        for vt in &self.visible_tiles {
            let tile = &self.tile_boxes[vt.tile as usize];
            // The tile box, grown by the most any of its units' click volumes can grow.
            let (centre, half) = ((tile.min + tile.max) * 0.5, (tile.max - tile.min) * 0.5);
            let far = (centre - eye.eye).length() + half.length();
            let reach = Vec3::splat(click_reach(far, eye, click_px));
            if !ray_hits_aabb(ray, tile.min - reach, tile.max + reach) {
                continue;
            }
            for (r, rec) in
                (vt.start..vt.end).zip(&self.records[vt.start as usize..vt.end as usize])
            {
                let geom = self.geom(rec.shape);
                // The box's footprint centre (a model's real bounds need not
                // be centred on the unit's origin).
                let pos = offset_point(Vec3::from(rec.pos), rec.steps, geom.offset);
                let middle = pos + Vec3::new(0.0, 0.5 * geom.size[1], 0.0);
                let distance = (middle - eye.eye).length();
                // The smaller side of the footprint is what has to be wide
                // enough to hit, whatever way the unit faces.
                let footprint = geom.size[0].min(geom.size[2]);
                let margin = click_margin(footprint, distance, eye.focal_px, click_px);
                let Some(t) = ray_obb(ray, pos, rec.steps, geom.size, margin) else {
                    continue;
                };
                // Entered behind the terrain the ray meets first: hidden by the ground.
                if query
                    .terrain_t
                    .is_some_and(|ground_t| t > ground_t + margin + 0.05)
                {
                    continue;
                }
                let candidate = Candidate {
                    id: rec.id,
                    via: PickVia::Model,
                    distance: t,
                    cursor_distance: 0.0,
                    // Models rank by depth only.
                    rank: 0.0,
                    record: r,
                };
                if best.is_none_or(|b| candidate.beats(&b)) {
                    best = Some(candidate);
                }
            }
        }
        let mut best = best?;
        // For the readout: how far the box centre is from the cursor on the screen.
        let rec = &self.records[best.record as usize];
        let geom = self.geom(rec.shape);
        let middle = offset_point(Vec3::from(rec.pos), rec.steps, geom.offset)
            + Vec3::new(0.0, 0.5 * geom.size[1], 0.0);
        best.cursor_distance = project_px(&query.view_proj, middle, query.viewport)
            .map_or(0.0, |(p, _)| {
                (p[0] - query.cursor[0]).hypot(p[1] - query.cursor[1])
            });
        Some(best)
    }

    fn pick_icon(
        &self,
        query: &crate::pick::PickQuery,
        ray: &crate::pick::PickRay,
        eye: &EyeView,
        ui_scale: f32,
    ) -> Option<Candidate> {
        let size = self.icon_config.icon_px(ui_scale);
        let lift = self.icon_config.lift;
        let mut best: Option<Candidate> = None;
        for vt in &self.visible_tiles {
            let tile = &self.tile_boxes[vt.tile as usize];
            // Everything whose icon quad can reach the cursor is within a
            // few pixels' worth of metres of the cursor ray.
            let (centre, half) = ((tile.min + tile.max) * 0.5, (tile.max - tile.min) * 0.5);
            let far = (centre - eye.eye).length() + half.length();
            let reach = Vec3::splat((0.75 * size + 1.0) * far / eye.focal_px.max(1.0) + lift);
            if !ray_hits_aabb(ray, tile.min - reach, tile.max + reach) {
                continue;
            }
            for (r, rec) in
                (vt.start..vt.end).zip(&self.records[vt.start as usize..vt.end as usize])
            {
                let anchor = Vec3::new(rec.pos[0], rec.pos[1] + lift, rec.pos[2]);
                let Some((px, _)) = project_px(&query.view_proj, anchor, query.viewport) else {
                    continue;
                };
                // The quad the shader draws: centre snapped to the pixel grid.
                let centre = [snap_icon_center(px[0], size), snap_icon_center(px[1], size)];
                let Some(cursor_distance) = icon_hit(query.cursor, centre, size) else {
                    continue;
                };
                let candidate = Candidate {
                    id: rec.id,
                    via: PickVia::Icon,
                    distance: (anchor - eye.eye).length(),
                    cursor_distance,
                    rank: cursor_distance,
                    record: r,
                };
                if best.is_none_or(|b| candidate.beats(&b)) {
                    best = Some(candidate);
                }
            }
        }
        best
    }

    /// Mark the picked unit in the instance lists of the last build (before
    /// they are uploaded): its model is tinted towards white, its icon gets
    /// the highlight flag (a white outline). Does nothing if the pick is
    /// from another frame.
    pub fn highlight(&mut self, pick: &Pick) {
        let Some(rec) = self.records.get(pick.record as usize).copied() else {
            return;
        };
        if rec.id != pick.id {
            return;
        }
        let parts = std::iter::once((rec.model_mesh, rec.model_index))
            .chain(rec.extra[..usize::from(rec.extra_parts)].iter().copied());
        for (mesh, index) in parts {
            if index == NO_INSTANCE {
                continue;
            }
            if let Some(instance) = self
                .lists
                .get_mut(usize::from(mesh))
                .and_then(|l| l.get_mut(index as usize))
            {
                for c in &mut instance.color[..3] {
                    *c += (1.0 - *c) * HIGHLIGHT_TINT;
                }
            }
        }
        if rec.icon_index != NO_INSTANCE
            && let Some(icon) = self.icons.get_mut(rec.icon_index as usize)
        {
            icon.flags |= ICON_HIGHLIGHT;
        }
    }
}

/// How far a highlighted model's colour moves towards white (linear light).
const HIGHLIGHT_TINT: f32 = 0.55;

/// A unit that is under the cursor, with what decides between two of them.
#[derive(Clone, Copy, Debug)]
struct Candidate {
    id: UnitId,
    via: PickVia,
    distance: f32,
    cursor_distance: f32,
    /// Icons: the cursor distance, the nearest centre wins.
    rank: f32,
    record: u32,
}

impl Candidate {
    /// Nearest icon centre to the cursor first, then the nearer to the
    /// camera, then the lower id; for models the nearer hit along the ray,
    /// then the lower id. Differences below a hundredth of a pixel or a
    /// tenth of a millimetre count as equal.
    fn beats(&self, other: &Candidate) -> bool {
        if (self.rank - other.rank).abs() > 0.01 {
            return self.rank < other.rank;
        }
        if (self.distance - other.distance).abs() > 1e-4 {
            return self.distance < other.distance;
        }
        self.id < other.id
    }
}

/// Levels of detail, icons, picking and the highlight (WP1.9).
#[cfg(test)]
mod frame_tests;

/// glTF models in the instancer (WP1.10).
#[cfg(test)]
mod glb_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{Camera, CameraConfig, DVec2};
    use crate::ground::{Ground, TerrainGround};
    use sf_math::Fx;

    /// A position given in metres, as the cells the sim would hold.
    fn v2(x: i32, y: i32) -> Vec2 {
        Vec2::new(
            Fx::from_ratio(i64::from(x), 4),
            Fx::from_ratio(i64::from(y), 4),
        )
    }

    fn motion(prev: Vec2, pos: Vec2, pf: u8, f: u8) -> Motion {
        Motion::from_fixed(0, 0, prev, pos, Facing::new(pf), Facing::new(f))
    }

    #[test]
    fn alpha_zero_is_previous_and_one_is_current() {
        let m = motion(v2(10, 20), v2(11, 22), 10, 30);
        assert_eq!(m.at(0.0), ([10.0, 20.0], 10.0));
        assert_eq!(m.at(1.0), ([11.0, 22.0], 30.0));
        let (p, f) = m.at(0.5);
        assert_eq!(p, [10.5, 21.0]);
        assert_eq!(f, 20.0);
        // Out-of-range alpha is clamped.
        assert_eq!(m.at(-3.0), m.at(0.0));
        assert_eq!(m.at(7.0), m.at(1.0));
    }

    #[test]
    fn fixed_point_fractions_convert_exactly() {
        // Half a cell is two metres (B12: a cell is 4 m).
        let half = Fx::from_raw(Fx::ONE.raw() / 2);
        let m = motion(Vec2::new(half, half), Vec2::new(half, half), 0, 0);
        assert_eq!(m.at(0.3).0, [2.0, 2.0]);
    }

    #[test]
    fn facing_wraps_the_short_way() {
        // 255 -> 1 is a turn of +2 through 0.
        let m = motion(v2(0, 0), v2(0, 0), 255, 1);
        let (_, mid) = m.at(0.5);
        assert_eq!(mid.rem_euclid(256.0), 0.0);
        let (_, end) = m.at(1.0);
        assert_eq!(end.rem_euclid(256.0), 1.0);
        // And the other way: 1 -> 255 passes through 0 as well.
        let m = motion(v2(0, 0), v2(0, 0), 1, 255);
        assert_eq!(m.at(0.5).1.rem_euclid(256.0), 0.0);
        // Never takes the long way: 10 -> 200 turns backwards (-66 steps).
        let m = motion(v2(0, 0), v2(0, 0), 10, 200);
        assert!((m.at(0.5).1 - (10.0 - 33.0)).abs() < 1e-4);
    }

    #[test]
    fn model_matrix_points_along_the_facing() {
        let col = |m: [[f32; 4]; 4], i: usize| Vec3::new(m[i][0], m[i][1], m[i][2]);
        // Facing 0 = +x, 64 (a quarter turn) = +z.
        let m0 = model_matrix(1.0, 2.0, 3.0, 0.0, [2.0, 1.0, 1.0]);
        assert!((col(m0, 0) - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-6);
        assert_eq!(m0[3], [1.0, 2.0, 3.0, 1.0]);
        let m64 = model_matrix(0.0, 0.0, 0.0, 64.0, [2.0, 1.0, 1.0]);
        assert!((col(m64, 0) - Vec3::new(0.0, 0.0, 2.0)).length() < 1e-5);
        // A proper rotation: the across axis turns with it.
        assert!((col(m64, 2) - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-5);
    }

    /// A deterministic spread of `n` entities on a `side` x `side` m map.
    fn crowd(n: usize, side: f32) -> Vec<Motion> {
        let per_row = (n as f32).sqrt().ceil() as usize;
        let step = side / per_row as f32;
        (0..n)
            .map(|i| {
                let (cx, cz) = ((i % per_row) as f32 * step, (i / per_row) as f32 * step);
                let (dx, dz) = (((i * 7) % 5) as f32 * 0.1, ((i * 3) % 5) as f32 * 0.1);
                let to_fx = |v: f32| sf_math::render_bridge::metres_to_cells(v);
                Motion::from_fixed(
                    (i % 3) as u8,
                    ((i / 3) % 4) as u8,
                    Vec2::new(to_fx(cx), to_fx(cz)),
                    Vec2::new(to_fx(cx + dx), to_fx(cz + dz)),
                    Facing::new((i % 256) as u8),
                    Facing::new(((i + 5) % 256) as u8),
                )
            })
            .collect()
    }

    fn camera(zoom: f64, focus: (f64, f64), side: f64) -> Camera {
        let cfg = CameraConfig {
            map_size: DVec2::new(side, side),
            ..CameraConfig::default()
        };
        let mut c = Camera::new(cfg, 1280, 720);
        c.set_zoom(zoom);
        c.set_focus(DVec2::new(focus.0, focus.1));
        c
    }

    fn instancer(n: usize, side: f32) -> Instancer {
        let mut inst = Instancer::new(Style::embedded(), (side, side));
        inst.set_height_range(0.0, 6.0);
        inst.update_tick(crowd(n, side));
        inst
    }

    #[test]
    fn zooming_in_culls_most_instances() {
        let mut inst = instancer(10_000, 256.0);
        let far = camera(1.0, (128.0, 128.0), 256.0);
        let near = camera(0.2, (128.0, 128.0), 256.0);
        let all = inst.build(&Frustum::from_view_proj(far.view_proj()), 0.5, |_, _| 0.0);
        assert_eq!(all.total, 10_000);
        assert_eq!(all.visible, 10_000, "whole map in view");
        assert_eq!(all.tiles_visible, all.tiles_total);
        let close = inst.build(&Frustum::from_view_proj(near.view_proj()), 0.5, |_, _| 0.0);
        assert!(close.visible > 0);
        assert!(
            close.visible * 4 < all.visible,
            "zoomed in {} vs {}",
            close.visible,
            all.visible
        );
        assert!(close.tiles_visible < close.tiles_total);
        assert_eq!(close.per_mesh.iter().sum::<u32>(), close.visible);
        // Lists match the counters.
        let listed: usize = MeshKind::ALL.iter().map(|&k| inst.instances(k).len()).sum();
        assert_eq!(listed as u32, close.visible);
    }

    #[test]
    fn culling_never_drops_an_instance_that_is_in_view() {
        let side = 256.0;
        let mut inst = instancer(4_000, side);
        let motions = crowd(4_000, side);
        for (zoom, focus) in [
            (0.2, (60.0, 90.0)),
            (0.35, (200.0, 30.0)),
            (0.1, (10.0, 10.0)),
        ] {
            let cam = camera(zoom, focus, f64::from(side));
            let frustum = Frustum::from_view_proj(cam.view_proj());
            let alpha = 0.37;
            let stats = inst.build(&frustum, alpha, |_, _| 0.0);
            let in_view = motions
                .iter()
                .filter(|m| {
                    let ([x, z], _) = m.at(alpha);
                    frustum.contains_point(Vec3::new(x, 0.5, z))
                })
                .count() as u32;
            assert!(in_view > 0, "test view shows nothing");
            assert!(stats.visible >= in_view, "{} < {in_view}", stats.visible);
        }
    }

    /// A 256 m map with hills up to about 20 m and slopes of the order of 0.5.
    fn hilly_ground() -> std::sync::Arc<TerrainGround> {
        let n = 257;
        let heights = (0..n * n)
            .map(|i| {
                let (x, z) = ((i % n) as f32, (i / n) as f32);
                let h = 10.0
                    + 6.0 * (x * 0.045).sin() * (z * 0.037).cos()
                    + 4.0 * (x * 0.021 + z * 0.033).sin();
                (h / 20.0 * 65535.0) as u16
            })
            .collect();
        let map = sf_data::map::Map::from_parts(
            256,
            256,
            20.0,
            vec!["a".into()],
            heights,
            vec![0; 256 * 256],
        )
        .expect("valid hills");
        std::sync::Arc::new(TerrainGround::new(&map))
    }

    /// Bit pattern of an instance's ground position, to find it again.
    fn key(x: f32, z: f32) -> (u32, u32) {
        (x.to_bits(), z.to_bits())
    }

    /// `(units whose centre, half a metre above the hill they stand on, is
    /// inside the frustum, how many of them are missing from the built
    /// lists)`.
    fn dropped_in_view(
        inst: &mut Instancer,
        ground: &TerrainGround,
        motions: &[Motion],
        frustum: &Frustum,
        alpha: f32,
    ) -> (usize, usize) {
        let height = |x: f32, z: f32| ground.height(f64::from(x), f64::from(z)) as f32;
        inst.build(frustum, alpha, height);
        let built: std::collections::BTreeSet<(u32, u32)> = MeshKind::ALL
            .iter()
            .flat_map(|&k| inst.instances(k))
            .map(|i| key(i.model[3][0], i.model[3][2]))
            .collect();
        let (mut in_view, mut dropped) = (0, 0);
        for m in motions {
            let ([x, z], _) = m.at(alpha);
            if frustum.contains_point(Vec3::new(x, height(x, z) + 0.5, z)) {
                in_view += 1;
                dropped += usize::from(!built.contains(&key(x, z)));
            }
        }
        (in_view, dropped)
    }

    /// The same brute-force check as above, on hills: seen through a camera
    /// that follows the relief, a unit on a 20 m hill is in view from places
    /// where the flat ground below it is not, so the tile boxes must reach up
    /// to the terrain's highest point. A wrong height range (flat) drops
    /// visible units, which shows that the test can fail.
    #[test]
    fn culling_never_drops_an_instance_that_is_in_view_on_relief() {
        let side = 256.0;
        let ground = hilly_ground();
        let motions = crowd(4_000, side);
        let views = [
            (0.2, (60.0, 90.0)),
            (0.35, (200.0, 30.0)),
            (0.1, (10.0, 10.0)),
            (0.45, (128.0, 200.0)),
            (0.0, (30.0, 220.0)),
        ];
        let mut right = instancer(4_000, side);
        right.set_height_range(0.0, 20.5);
        let mut wrong = instancer(4_000, side);
        wrong.set_height_range(0.0, 0.0);
        let (mut total, mut wrongly_dropped) = (0, 0);
        for (zoom, focus) in views {
            let mut cam = camera(zoom, focus, f64::from(side));
            cam.set_ground(ground.clone());
            let frustum = Frustum::from_view_proj(cam.view_proj());
            let (in_view, dropped) = dropped_in_view(&mut right, &ground, &motions, &frustum, 0.37);
            assert!(in_view > 0, "zoom {zoom}: the test view shows nothing");
            assert_eq!(
                dropped, 0,
                "zoom {zoom}: {dropped} of {in_view} visible units dropped"
            );
            total += in_view;
            wrongly_dropped += dropped_in_view(&mut wrong, &ground, &motions, &frustum, 0.37).1;
        }
        println!("{total} units in view; a flat height range would drop {wrongly_dropped}");
        assert!(
            wrongly_dropped > 0,
            "the relief case does not exercise the height range"
        );
    }

    #[test]
    fn built_instances_carry_the_player_colors_and_one_list_per_mesh() {
        let mut inst = instancer(1_200, 128.0);
        let cam = camera(1.0, (64.0, 64.0), 128.0);
        let stats = inst.build(&Frustum::from_view_proj(cam.view_proj()), 0.0, |_, _| 0.0);
        assert_eq!(stats.draw_calls(), 3);
        let style = Style::embedded();
        for kind in [MeshKind::Cube, MeshKind::Cylinder, MeshKind::Cone] {
            assert_eq!(inst.instances(kind).len(), 400);
        }
        assert!(inst.instances(MeshKind::Plane).is_empty());
        for p in 0..4 {
            let lin = style.linear(p);
            assert!(
                inst.instances(MeshKind::Cube)
                    .iter()
                    .any(|i| i.color[..3] == lin),
                "player {p} missing"
            );
        }
    }

    #[test]
    fn instances_stand_on_the_given_ground() {
        let mut inst = instancer(100, 64.0);
        let cam = camera(1.0, (32.0, 32.0), 64.0);
        inst.build(&Frustum::from_view_proj(cam.view_proj()), 0.0, |x, _| {
            x * 0.1
        });
        for i in inst.instances(MeshKind::Cube) {
            assert!((i.model[3][1] - i.model[3][0] * 0.1).abs() < 1e-5);
        }
    }

    #[test]
    fn tile_box_spans_the_relief_plus_the_tallest_shape() {
        let mut inst = Instancer::new(Style::embedded(), (64.0, 64.0));
        inst.set_height_range(-2.0, 20.0);
        inst.update_tick([motion(v2(40, 10), v2(41, 10), 0, 0)]);
        // The unit sits in tile (1, 0); its box covers both positions.
        let b = inst.tile_boxes[1];
        assert!(b.min.x < 40.0 && b.max.x > 41.0);
        assert_eq!(b.min.y, -2.0);
        assert!(b.max.y >= 20.0 + 1.4);
        // Interpolated positions stay inside the box.
        for a in [0.0, 0.25, 1.0] {
            let ([x, z], _) = inst.motions[0].at(a);
            assert!(b.min.x <= x && x <= b.max.x && b.min.z <= z && z <= b.max.z);
        }
    }

    /// A mesh used by two parts of one model gets two instances per entity,
    /// and `mesh_capacity` reserves for both (it used to count the mesh once
    /// per model, follow-up to WP1.14).
    #[test]
    fn mesh_capacity_counts_a_mesh_once_per_use() {
        use crate::mesh::MeshData;
        use crate::model::{Joint, ModelBounds, ModelLods, RigPart};
        let shared = MeshId(crate::model::BUILTIN_MESHES as u16);
        let part = |parent| RigPart {
            mesh: shared,
            parent,
            rest: Mat4::IDENTITY,
            joint: Joint::Fixed,
        };
        let bounds = ModelBounds {
            min: Vec3::splat(-1.0),
            max: Vec3::splat(1.0),
            reach: 2.0,
        };
        // Shape 0: two parts over one mesh at both levels. Shape 1: a plain
        // single-mesh model over the same mesh.
        let twin = ModelLods::with_parts(
            vec![
                vec![part(None), part(Some(0))],
                vec![part(None), part(Some(0))],
            ],
            vec![50.0],
            bounds,
        )
        .unwrap();
        let single = ModelLods::single(shared);
        let models = ModelSet::new(vec![MeshData::cube()], vec![twin, single]).unwrap();
        let mut inst = Instancer::new(Style::embedded(), (64.0, 64.0));
        inst.set_models(models);
        let mut motions = Vec::new();
        for i in 0..5 {
            let mut m = motion(v2(8 + 4 * i, 8), v2(8 + 4 * i, 8), 0, 0);
            m.shape = (i % 2) as u8;
            motions.push(m);
        }
        inst.update_tick(motions);
        // Three entities of shape 0 (2 uses each) and two of shape 1 (1 use).
        let capacity = inst.mesh_capacity();
        assert_eq!(capacity[shared.index()], 3 * 2 + 2);
        // And a frame really draws that many, so the bound is tight.
        let cam = camera(1.0, (32.0, 32.0), 64.0);
        inst.build(&Frustum::from_view_proj(cam.view_proj()), 0.0, |_, _| 0.0);
        assert_eq!(inst.mesh_instances(shared).len(), 8);
    }

    #[test]
    fn totals_per_mesh_count_hidden_entities_too() {
        let mut inst = instancer(3000, 256.0);
        let near = camera(0.2, (128.0, 128.0), 256.0);
        let stats = inst.build(&Frustum::from_view_proj(near.view_proj()), 0.5, |_, _| 0.0);
        assert_eq!(stats.total_per_mesh.iter().sum::<u32>(), 3000);
        assert!(stats.visible < stats.total);
        for k in 0..MeshKind::ALL.len() {
            assert!(stats.per_mesh[k] <= stats.total_per_mesh[k]);
        }
    }

    #[test]
    fn set_style_changes_the_look_and_the_per_mesh_totals() {
        let mut inst = instancer(300, 64.0);
        let cam = camera(1.0, (32.0, 32.0), 64.0);
        let frustum = Frustum::from_view_proj(cam.view_proj());
        let before = inst.build(&frustum, 0.0, |_, _| 0.0);
        assert!(before.total_per_mesh[MeshKind::Cone.index()] > 0);
        // Every shape becomes a cube, taller than any before, in one color.
        let style = Style::from_ron(
            "(palette: [(name: \"a\", srgb: (255, 0, 0))],
              shapes: [(mesh: Cube, size: (1.0, 3.0, 1.0))])",
        )
        .unwrap();
        inst.set_style(style);
        let after = inst.build(&frustum, 0.0, |_, _| 0.0);
        assert_eq!(after.total, before.total);
        assert_eq!(after.total_per_mesh[MeshKind::Cube.index()], 300);
        assert_eq!(after.total_per_mesh[MeshKind::Cone.index()], 0);
        assert_eq!(inst.instances(MeshKind::Cone).len(), 0);
        assert!(
            inst.instances(MeshKind::Cube)
                .iter()
                .all(|i| i.color[0] > 0.9 && i.color[1] == 0.0)
        );
    }
}
