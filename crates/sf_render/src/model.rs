//! Models and their level-of-detail chains (WP1.9).
//!
//! # The chain
//! A **model** is what a unit shape looks like in the world. It has a chain
//! of `N` level-of-detail meshes (`N <= MAX_LODS`), finest first. Level `i`
//! (for `i < N - 1`) is used while the model's *projected size* is at least
//! `switch_px[i]` pixels; the last level takes everything smaller. The
//! projected size is the model's bounding diameter ([`Shape::bounding_diameter`](crate::style::Shape::bounding_diameter))
//! times the focal length over the distance to the eye, in physical pixels.
//! It is a property of the picture, so it does not move with the UI scale.
//!
//! # Hysteresis
//! [`ModelLods::select`] takes the level the same unit had last frame. A unit
//! goes to a coarser level only when it is clearly below the switch size
//! (`switch * (1 - h)`) and back to a finer one only when it is clearly above
//! it (`switch * (1 + h)`), so a unit hovering at a switch size does not
//! flicker. Without a previous level the level is the plain threshold
//! ([`ModelLods::ideal`]), which is monotone: the farther away, the coarser or
//! equal. That is the case for a unit that has never been drawn, and for every
//! unit after [`Instancer::set_models`](crate::instancing::Instancer::set_models).
//!
//! A unit that is culled (its tile is outside the view) or not drawn as a
//! model (icons only) **keeps** the level it had when it was last drawn; it is
//! not reset. That is harmless: whatever the previous level, `select` returns
//! a level within the hysteresis band of the plain threshold (test
//! `the_chosen_level_stays_within_the_band_of_the_ideal`), so a stale level
//! only decides *which* of the at most two levels allowed inside a band a
//! returning unit gets, never a level from far outside it.
//!
//! # Meshes and draw calls
//! Meshes live in one table, addressed by [`MeshId`]: the five built-in
//! placeholder kinds come first (so a [`MeshKind`] is its own id), the lower
//! detail variants and, in WP1.10, glTF meshes come after
//! ([`ModelSet::extra_meshes`]). The instancer keeps **one instance list per
//! mesh id** and the renderer one buffer and one draw call per list, so
//! each (mesh kind, level) pair stays a single draw.
//!
//! # Parts (WP1.10, glTF models)
//! A level can be made of **parts** ([`RigPart`]): a hull, a turret on the
//! hull and a barrel on the turret, each its own mesh with its rest
//! transform relative to its parent part. Every part is one mesh id, so a
//! unit at a level adds one instance to the list of each of its parts, and a
//! (part, level) pair stays one draw call. The turret turns about its local
//! `+Y` ([`Joint::Yaw`]) and the barrel tilts about its local `+Z`
//! ([`Joint::Pitch`]); [`pose_parts`] computes the matrices per instance.
//! A model with parts also carries its real bounds ([`ModelBounds`], at the
//! finest level), which the culling, the level selection and the picking
//! use instead of the placeholder shape size. See [`crate::glb`] for the
//! loader.

use std::sync::atomic::{AtomicU64, Ordering};

use glam::{DMat4, DVec4, Mat4, Vec3};

use crate::mesh::{MeshData, SEGMENTS};
use crate::scene::MeshKind;
use crate::style::{LodSpec, Style};

/// Most levels a chain can have (the stats keep a counter per level).
pub const MAX_LODS: usize = 4;

/// Most parts a level can have (hull, turret, barrel and one spare).
pub const MAX_PARTS: usize = 4;

/// How a part moves relative to its parent, driven per instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Joint {
    /// Rigidly attached (the hull, any other part).
    Fixed,
    /// Turns about its local `+Y` (a turret). Positive angles turn the
    /// part's front (`+X`) towards `+Z`, the same sense as the unit facing.
    Yaw,
    /// Tilts about its local `+Z` (a barrel). Positive angles lift the
    /// part's front (`+X`) towards `+Y`.
    Pitch,
}

/// One part of a level: its mesh, its parent (an earlier part, `None` = the
/// unit itself) and its rest transform in the parent's frame, in engine
/// axes (metres, `+Y` up, front `+X`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RigPart {
    pub mesh: MeshId,
    pub parent: Option<u8>,
    pub rest: Mat4,
    pub joint: Joint,
}

/// A named point on a model that is not drawn (WP5.6b): the end of a barrel (`muzzle`, `muzzle_l`), a
/// place for smoke or fire (`fx_smoke`). It sits in the frame of a part of the level, so it follows the
/// turret's yaw and the barrel's pitch like the part does.
#[derive(Clone, Debug, PartialEq)]
pub struct RigAnchor {
    /// The node name without its `_LOD<n>` suffix.
    pub name: String,
    /// The part whose frame it is in (an index in the level's parts); `None` = the unit itself.
    pub parent: Option<u8>,
    /// From the anchor's frame to its parent's, in engine axes (metres).
    pub local: Mat4,
}

/// The world position of the `n`-th anchor of a level whose name starts with `prefix`, for a unit drawn
/// with matrix `unit` and joint `angles`: the same `pose_parts` the draw uses, then `parent * local`.
/// `n` counts the matching anchors in the file's order and wraps around (a burst alternates between
/// `muzzle_l` and `muzzle_r`). `None` when no anchor matches.
pub fn anchor_position(
    unit: Mat4,
    parts: &[RigPart],
    anchors: &[RigAnchor],
    angles: PartAngles,
    prefix: &str,
    n: usize,
) -> Option<Vec3> {
    let count = anchors
        .iter()
        .filter(|a| a.name.starts_with(prefix))
        .count();
    if count == 0 {
        return None;
    }
    let anchor = anchors
        .iter()
        .filter(|a| a.name.starts_with(prefix))
        .nth(n % count)?;
    let mut world = [Mat4::IDENTITY; MAX_PARTS];
    pose_parts(unit, parts, angles, &mut world);
    let parent = match anchor.parent {
        Some(p) => *world.get(usize::from(p))?,
        None => unit,
    };
    Some((parent * anchor.local).transform_point3(Vec3::ZERO))
}

/// The real extent of a model with parts, in the unit's frame (front `+X`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelBounds {
    /// Box around the finest level in its rest pose.
    pub min: Vec3,
    pub max: Vec3,
    /// No vertex of any level gets farther than this from the unit's origin,
    /// whatever the joint angles (culling margins).
    pub reach: f32,
}

impl ModelBounds {
    /// Diameter of the sphere around the box (what the level selection
    /// measures, like `Shape::bounding_diameter`).
    pub fn diameter(&self) -> f32 {
        (self.max - self.min).length()
    }
}

/// The per-instance joint angles of a pose, radians.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PartAngles {
    pub yaw: f32,
    pub pitch: f32,
}

/// The world matrices of the parts of a level: `unit` is the unit's model
/// matrix (position and facing), each part is `parent * rest * joint`.
/// Writes `parts.len()` matrices into `out` (at most [`MAX_PARTS`]).
pub fn pose_parts(unit: Mat4, parts: &[RigPart], angles: PartAngles, out: &mut [Mat4; MAX_PARTS]) {
    for (i, part) in parts.iter().enumerate().take(MAX_PARTS) {
        let joint = match part.joint {
            Joint::Fixed => Mat4::IDENTITY,
            // A positive yaw takes +X to +Z: a negative turn about +Y.
            Joint::Yaw => Mat4::from_rotation_y(-angles.yaw),
            Joint::Pitch => Mat4::from_rotation_z(angles.pitch),
        };
        let parent = part
            .parent
            .map_or(unit, |p| out[usize::from(p).min(i.saturating_sub(1))]);
        out[i] = parent * part.rest * joint;
    }
}

/// How many meshes exist without registering any: one per [`MeshKind`].
pub const BUILTIN_MESHES: usize = MeshKind::ALL.len();

/// What the level selection and the click volumes need to know about the
/// camera: where the eye is and how many pixels a radian covers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EyeView {
    pub eye: Vec3,
    /// Pixels per radian at the screen centre: `viewport_height / (2 tan(fov_y / 2))`.
    pub focal_px: f32,
}

/// Distances below this (metres) count as this, so an eye inside a model
/// cannot divide by zero.
const MIN_EYE_DISTANCE: f32 = 0.5;

impl EyeView {
    pub fn new(eye: Vec3, viewport_height_px: f32, fov_y: f32) -> Self {
        Self {
            eye,
            focal_px: 0.5 * viewport_height_px / (0.5 * fov_y).tan(),
        }
    }

    /// Recover the eye and the focal length from a perspective
    /// view-projection (wgpu clip space, rigid view): the eye is the point
    /// that maps to clip `(0, 0, z, 0)`, and the xyz part of the second row
    /// has the length `1 / tan(fov_y / 2)`. The renderer and the picking can
    /// then work with whatever matrix the frame was drawn with.
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

    /// Size in physical pixels of something `size` metres across at `distance`.
    pub fn projected_px(&self, size: f32, distance: f32) -> f32 {
        size * self.focal_px / distance.max(MIN_EYE_DISTANCE)
    }
}

/// An entry of the renderer's mesh table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MeshId(pub u16);

impl MeshId {
    /// The built-in mesh of a placeholder kind.
    pub fn of(kind: MeshKind) -> Self {
        Self(kind.index() as u16)
    }

    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// One level of a chain: the mesh to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LodLevel {
    pub mesh: MeshId,
}

/// Why a chain was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelError {
    NoLevels,
    TooManyLevels,
    /// There must be one switch size fewer than there are levels.
    SwitchCount,
    /// Switch sizes must be positive, finite and strictly falling.
    SwitchOrder,
    /// A level names a mesh that is not in the table.
    UnknownMesh,
    /// A level has no parts, more than [`MAX_PARTS`], or a part whose parent
    /// is not an earlier part.
    Parts,
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoLevels => "a model needs at least one level",
            Self::TooManyLevels => "a model has more than MAX_LODS levels",
            Self::SwitchCount => "a chain needs one switch size fewer than it has levels",
            Self::SwitchOrder => "switch sizes must be positive and strictly falling",
            Self::UnknownMesh => "a level names a mesh that does not exist",
            Self::Parts => "a level needs 1 to MAX_PARTS parts, each after its parent",
        })
    }
}

impl std::error::Error for ModelError {}

/// The level-of-detail chain of one model.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelLods {
    levels: Vec<LodLevel>,
    /// `switch_px[i]`: level `i` is used while the projected size is at least this.
    switch_px: Vec<f32>,
    /// The parts of each level; empty = the level is its single mesh.
    parts: Vec<Vec<RigPart>>,
    /// The anchors of each level (WP5.6b), parallel to `parts`; empty = none.
    anchors: Vec<Vec<RigAnchor>>,
    bounds: Option<ModelBounds>,
}

impl ModelLods {
    /// A chain from its levels (finest first) and the `levels - 1` switch
    /// sizes between them in pixels.
    pub fn new(levels: Vec<LodLevel>, switch_px: Vec<f32>) -> Result<Self, ModelError> {
        if levels.is_empty() {
            return Err(ModelError::NoLevels);
        }
        if levels.len() > MAX_LODS {
            return Err(ModelError::TooManyLevels);
        }
        if switch_px.len() + 1 != levels.len() {
            return Err(ModelError::SwitchCount);
        }
        let ordered = switch_px.iter().all(|&px| px.is_finite() && px > 0.0)
            && switch_px.windows(2).all(|w| w[0] > w[1]);
        if !ordered {
            return Err(ModelError::SwitchOrder);
        }
        Ok(Self {
            levels,
            switch_px,
            parts: Vec::new(),
            anchors: Vec::new(),
            bounds: None,
        })
    }

    /// A chain whose levels are made of parts (glTF models): the first part
    /// of a level is its root (the hull). `bounds` is the real extent.
    pub fn with_parts(
        parts: Vec<Vec<RigPart>>,
        switch_px: Vec<f32>,
        bounds: ModelBounds,
    ) -> Result<Self, ModelError> {
        for level in &parts {
            let ok = !level.is_empty()
                && level.len() <= MAX_PARTS
                && level.first().is_some_and(|p| p.parent.is_none())
                && level
                    .iter()
                    .enumerate()
                    .all(|(i, p)| p.parent.is_none_or(|q| usize::from(q) < i));
            if !ok {
                return Err(ModelError::Parts);
            }
        }
        let levels = parts.iter().map(|l| LodLevel { mesh: l[0].mesh }).collect();
        let mut chain = Self::new(levels, switch_px)?;
        chain.parts = parts;
        chain.bounds = Some(bounds);
        Ok(chain)
    }

    /// A model with one level, drawn at every size.
    pub fn single(mesh: MeshId) -> Self {
        Self {
            levels: vec![LodLevel { mesh }],
            switch_px: Vec::new(),
            parts: Vec::new(),
            anchors: Vec::new(),
            bounds: None,
        }
    }

    /// The same chain with the anchors of each level (one list per level, in the order of the levels).
    /// Lists beyond the levels are ignored.
    pub fn with_anchors(mut self, mut anchors: Vec<Vec<RigAnchor>>) -> Self {
        anchors.truncate(self.levels.len());
        self.anchors = anchors;
        self
    }

    /// The anchors of a level (clamped like [`ModelLods::level`]); empty when the model has none.
    pub fn anchors(&self, level: u8) -> &[RigAnchor] {
        if self.anchors.is_empty() {
            return &[];
        }
        &self.anchors[usize::from(level).min(self.anchors.len() - 1)]
    }

    /// How many anchors of the finest level have a name that starts with `prefix`.
    pub fn anchor_count(&self, prefix: &str) -> usize {
        self.anchors(0)
            .iter()
            .filter(|a| a.name.starts_with(prefix))
            .count()
    }

    /// The parts of a level (clamped like [`ModelLods::level`]); empty for a
    /// single-mesh level.
    pub fn parts(&self, level: u8) -> &[RigPart] {
        if self.parts.is_empty() {
            return &[];
        }
        &self.parts[usize::from(level).min(self.parts.len() - 1)]
    }

    /// The real extent of a model with parts; `None` = the placeholder shape
    /// size applies.
    pub fn bounds(&self) -> Option<&ModelBounds> {
        self.bounds.as_ref()
    }

    /// The same chain with every extra mesh (id `BUILTIN_MESHES` and up)
    /// moved up by `by` places in the table; the built-in kinds stay.
    fn shifted(&self, by: usize) -> Self {
        let shift = |m: MeshId| {
            if m.index() < BUILTIN_MESHES {
                m
            } else {
                MeshId((m.index() + by) as u16)
            }
        };
        let mut chain = self.clone();
        for level in &mut chain.levels {
            level.mesh = shift(level.mesh);
        }
        for part in chain.parts.iter_mut().flatten() {
            part.mesh = shift(part.mesh);
        }
        chain
    }

    /// Every mesh the chain draws (levels and parts).
    pub fn meshes(&self) -> impl Iterator<Item = MeshId> + '_ {
        self.levels
            .iter()
            .map(|l| l.mesh)
            .chain(self.parts.iter().flatten().map(|p| p.mesh))
    }

    /// The most instances one entity of this model puts into each mesh's
    /// list in a frame: per use, so a model whose two parts share a mesh
    /// counts it twice. The levels are alternatives (one is drawn per
    /// entity), so the largest level decides. Meshes that are never drawn
    /// are left out.
    pub fn peak_mesh_uses(&self) -> Vec<(MeshId, u32)> {
        let mut peak: Vec<(MeshId, u32)> = Vec::new();
        for level in 0..self.levels.len() {
            let level = level as u8;
            let parts = self.parts(level);
            let meshes: Vec<MeshId> = if parts.is_empty() {
                vec![self.level(level).mesh]
            } else {
                parts.iter().take(MAX_PARTS).map(|p| p.mesh).collect()
            };
            for &mesh in &meshes {
                let uses = meshes.iter().filter(|&&m| m == mesh).count() as u32;
                match peak.iter_mut().find(|(m, _)| *m == mesh) {
                    Some((_, best)) => *best = (*best).max(uses),
                    None => peak.push((mesh, uses)),
                }
            }
        }
        peak
    }

    pub fn len(&self) -> usize {
        self.levels.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn levels(&self) -> &[LodLevel] {
        &self.levels
    }

    pub fn level(&self, level: u8) -> LodLevel {
        self.levels[usize::from(level).min(self.levels.len() - 1)]
    }

    pub fn switch_px(&self) -> &[f32] {
        &self.switch_px
    }

    /// The level for a projected size without any history: the finest level
    /// whose switch size is reached, or the last one.
    pub fn ideal(&self, px: f32) -> u8 {
        self.switch_px.iter().take_while(|&&s| px < s).count() as u8
    }

    /// The level for a projected size `px`, given the level the unit had in
    /// the previous frame (`None` = it was not drawn). `hysteresis` is the
    /// relative margin (`0..1`) around each switch size.
    pub fn select(&self, px: f32, previous: Option<u8>, hysteresis: f32) -> u8 {
        let last = self.levels.len() - 1;
        let Some(previous) = previous.filter(|&p| usize::from(p) <= last) else {
            return self.ideal(px);
        };
        let h = if hysteresis.is_nan() {
            0.0
        } else {
            hysteresis.clamp(0.0, 0.95)
        };
        let mut level = usize::from(previous);
        // Coarser: only when clearly below the switch size of this level.
        while level < last && px < self.switch_px[level] * (1.0 - h) {
            level += 1;
        }
        // Finer: only when clearly above the switch size of the level above.
        while level > 0 && px >= self.switch_px[level - 1] * (1.0 + h) {
            level -= 1;
        }
        level as u8
    }
}

/// All models of a style plus the meshes they need beyond the built-in kinds.
#[derive(Clone, Debug)]
pub struct ModelSet {
    revision: u64,
    extra: Vec<MeshData>,
    /// Triangles of every mesh, built-in kinds first.
    triangles: Vec<u32>,
    models: Vec<ModelLods>,
}

static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

impl ModelSet {
    /// A set from extra meshes (ids `BUILTIN_MESHES..`) and one chain per
    /// shape. This is the entry point for WP1.10: load the glTF meshes, push
    /// them as `extra`, build the chains over their ids.
    pub fn new(extra: Vec<MeshData>, models: Vec<ModelLods>) -> Result<Self, ModelError> {
        let mesh_count = BUILTIN_MESHES + extra.len();
        if models.is_empty() {
            return Err(ModelError::NoLevels);
        }
        if models
            .iter()
            .any(|m| m.meshes().any(|mesh| mesh.index() >= mesh_count))
        {
            return Err(ModelError::UnknownMesh);
        }
        let builtin = MeshKind::ALL.map(|k| builtin_mesh(k).triangle_count());
        let triangles = builtin
            .into_iter()
            .chain(extra.iter().map(MeshData::triangle_count))
            .collect();
        Ok(Self {
            revision: NEXT_REVISION.fetch_add(1, Ordering::Relaxed),
            extra,
            triangles,
            models,
        })
    }

    /// The chains described by a style: round shapes get meshes with fewer
    /// facets for their lower levels, other shapes keep one level.
    pub fn from_style(style: &Style) -> Self {
        let mut extra: Vec<(MeshKind, u32, MeshData)> = Vec::new();
        let mut models = Vec::new();
        for (i, shape) in style.shapes().iter().enumerate() {
            let specs = style.lod_specs(i as u8);
            let default = [LodSpec {
                segments: None,
                min_px: 0.0,
            }];
            let specs = if specs.is_empty() {
                &default[..]
            } else {
                specs
            };
            let mut levels = Vec::new();
            for spec in specs {
                let segments = spec.segments.unwrap_or(SEGMENTS as u32);
                let id = match shape.mesh {
                    MeshKind::Cylinder | MeshKind::Cone if segments != SEGMENTS as u32 => {
                        let found = extra
                            .iter()
                            .position(|(k, n, _)| *k == shape.mesh && *n == segments);
                        let at = found.unwrap_or_else(|| {
                            extra.push((
                                shape.mesh,
                                segments,
                                round_mesh(shape.mesh, segments as usize),
                            ));
                            extra.len() - 1
                        });
                        MeshId((BUILTIN_MESHES + at) as u16)
                    }
                    kind => MeshId::of(kind),
                };
                levels.push(LodLevel { mesh: id });
            }
            let switch = specs[..specs.len() - 1].iter().map(|s| s.min_px).collect();
            models.push(ModelLods::new(levels, switch).expect("the style validated its chains"));
        }
        let extra = extra.into_iter().map(|(_, _, m)| m).collect();
        Self::new(extra, models).expect("the chains only name meshes that were registered")
    }

    /// This set followed by `other`: the shapes of `other` come after the
    /// shapes of `self` (shape `self.models().len() + i` is `other`'s shape
    /// `i`) and its extra meshes after ours. The demo draws placeholder shapes
    /// and glTF models in one scene this way. `None` if the mesh table would
    /// outgrow a [`MeshId`].
    pub fn concat(&self, other: &ModelSet) -> Option<ModelSet> {
        if BUILTIN_MESHES + self.extra.len() + other.extra.len() > usize::from(u16::MAX) {
            return None;
        }
        let extra = self.extra.iter().chain(&other.extra).cloned().collect();
        let models = self
            .models
            .iter()
            .cloned()
            .chain(other.models.iter().map(|m| m.shifted(self.extra.len())))
            .collect();
        ModelSet::new(extra, models).ok()
    }

    /// A set with the same meshes whose shape `i` is the chain `chains[i]` of
    /// this one: chains can be left out, repeated or reordered. A model that
    /// two roles draw with different icons is one chain listed twice (one shape
    /// per icon, the meshes uploaded and drawn once). `None` if an index is out
    /// of range or the list is empty.
    pub fn with_chains(&self, chains: &[usize]) -> Option<ModelSet> {
        let models = chains
            .iter()
            .map(|&i| self.models.get(i).cloned())
            .collect::<Option<Vec<_>>>()?;
        ModelSet::new(self.extra.clone(), models).ok()
    }

    /// Changes whenever a different set is built; the renderer compares it to
    /// know when to upload the extra meshes again.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The meshes beyond the built-in kinds; mesh `BUILTIN_MESHES + i` is `extra_meshes()[i]`.
    pub fn extra_meshes(&self) -> &[MeshData] {
        &self.extra
    }

    /// Built-in kinds plus extra meshes.
    pub fn mesh_count(&self) -> usize {
        self.triangles.len()
    }

    pub fn mesh_triangles(&self, mesh: MeshId) -> u32 {
        self.triangles.get(mesh.index()).copied().unwrap_or(0)
    }

    /// The chain of a shape (index wraps around as in `Style::shape`).
    pub fn model(&self, shape: u8) -> &ModelLods {
        &self.models[usize::from(shape) % self.models.len()]
    }

    pub fn models(&self) -> &[ModelLods] {
        &self.models
    }
}

fn builtin_mesh(kind: MeshKind) -> MeshData {
    match kind {
        MeshKind::Plane => MeshData::plane(),
        MeshKind::Cube => MeshData::cube(),
        MeshKind::Pyramid => MeshData::pyramid(),
        MeshKind::Cylinder => MeshData::cylinder(),
        MeshKind::Cone => MeshData::cone(),
    }
}

fn round_mesh(kind: MeshKind, segments: usize) -> MeshData {
    match kind {
        MeshKind::Cone => MeshData::cone_with(segments),
        _ => MeshData::cylinder_with(segments),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain3() -> ModelLods {
        let levels = (0..3)
            .map(|i| LodLevel { mesh: MeshId(i) })
            .collect::<Vec<_>>();
        ModelLods::new(levels, vec![60.0, 25.0]).unwrap()
    }

    #[test]
    fn the_eye_and_focal_length_come_back_out_of_the_view_projection() {
        use crate::camera::{Camera, CameraConfig, DVec2};
        for (w, h, zoom) in [(1280_u32, 720_u32, 0.0), (1280, 720, 0.6), (800, 1000, 1.0)] {
            let cfg = CameraConfig {
                map_size: DVec2::new(256.0, 256.0),
                ..CameraConfig::default()
            };
            let mut cam = Camera::new(cfg, w, h);
            cam.set_zoom(zoom);
            cam.set_focus(DVec2::new(100.0, 90.0));
            let view = EyeView::from_view_proj(cam.view_proj(), h as f32);
            let eye = cam.eye();
            let at = Vec3::new(eye.x as f32, eye.y as f32, eye.z as f32);
            assert!(
                (view.eye - at).length() < 0.05,
                "zoom {zoom}: {} vs {at}",
                view.eye
            );
            let focal = 0.5 * h as f32 / (0.5 * cfg.fov_y as f32).tan();
            assert!(
                (view.focal_px - focal).abs() < focal * 1e-3,
                "{} vs {focal}",
                view.focal_px
            );
            let direct = EyeView::new(at, h as f32, cfg.fov_y as f32);
            assert!((direct.focal_px - view.focal_px).abs() < focal * 1e-3);
            // A 2 m unit at the focus is `2 * focal / distance` pixels.
            let px = view.projected_px(2.0, cam.distance() as f32);
            assert!((px - 2.0 * focal / cam.distance() as f32).abs() < 0.05);
        }
    }

    #[test]
    fn chains_are_validated() {
        let l = |n: u16| (0..n).map(|i| LodLevel { mesh: MeshId(i) }).collect();
        assert!(ModelLods::new(l(3), vec![60.0, 25.0]).is_ok());
        assert_eq!(ModelLods::new(vec![], vec![]), Err(ModelError::NoLevels));
        assert_eq!(
            ModelLods::new(l(5), vec![5.0, 4.0, 3.0, 2.0]),
            Err(ModelError::TooManyLevels)
        );
        assert_eq!(
            ModelLods::new(l(3), vec![60.0]),
            Err(ModelError::SwitchCount)
        );
        for bad in [
            vec![25.0, 60.0],
            vec![60.0, 60.0],
            vec![60.0, 0.0],
            vec![f32::NAN, 5.0],
        ] {
            assert_eq!(ModelLods::new(l(3), bad), Err(ModelError::SwitchOrder));
        }
        assert_eq!(ModelLods::single(MeshId(1)).len(), 1);
        assert_eq!(ModelLods::single(MeshId(1)).ideal(0.0), 0);
    }

    #[test]
    fn the_ideal_level_is_monotone_in_the_size() {
        let c = chain3();
        // Boundaries: level 0 from 60 px up, level 1 from 25 px, else level 2.
        assert_eq!(c.ideal(1000.0), 0);
        assert_eq!(c.ideal(60.0), 0);
        assert_eq!(c.ideal(59.99), 1);
        assert_eq!(c.ideal(25.0), 1);
        assert_eq!(c.ideal(24.99), 2);
        assert_eq!(c.ideal(0.0), 2);
        // Over a sweep of sizes the level never gets finer as the unit shrinks.
        let mut last = 0;
        for i in 0..=2000 {
            let px = 200.0 * (1.0 - i as f32 / 2000.0);
            let level = c.ideal(px);
            assert!(
                level >= last,
                "level fell from {last} to {level} at {px} px"
            );
            last = level;
        }
        assert_eq!(last, 2);
    }

    /// Distance, not size, is what the player moves: size = k / distance, so
    /// monotone in the size is monotone in the distance.
    #[test]
    fn selection_by_distance_is_monotone_with_and_without_history() {
        let c = chain3();
        let k = 2.0 * 1300.0; // diameter * focal length
        for h in [0.0, 0.15, 0.5] {
            let (mut level, mut last) = (None, 0);
            for step in 0..4000 {
                let distance = 5.0 + step as f32 * 0.1; // 5 m .. 405 m away
                let l = c.select(k / distance, level, h);
                assert!(l >= last, "h={h}: fell from {last} to {l} at {distance} m");
                (level, last) = (Some(l), l);
            }
            assert_eq!(last, 2, "h={h}: the farthest unit uses the last level");
            // The same going back in: never coarser as it comes closer.
            for step in (0..4000).rev() {
                let distance = 5.0 + step as f32 * 0.1;
                let l = c.select(k / distance, level, h);
                assert!(l <= last, "h={h}: rose from {last} to {l} at {distance} m");
                (level, last) = (Some(l), l);
            }
            assert_eq!(last, 0, "h={h}: the nearest unit uses the finest level");
        }
    }

    #[test]
    fn without_history_or_hysteresis_selection_is_the_plain_threshold() {
        let c = chain3();
        for px in [0.0, 10.0, 24.99, 25.0, 40.0, 59.99, 60.0, 500.0] {
            assert_eq!(c.select(px, None, 0.3), c.ideal(px));
            for prev in 0..3 {
                assert_eq!(
                    c.select(px, Some(prev), 0.0),
                    c.ideal(px),
                    "{px} from {prev}"
                );
            }
            // A stale level beyond the chain is ignored.
            assert_eq!(c.select(px, Some(9), 0.3), c.ideal(px));
        }
    }

    #[test]
    fn hysteresis_holds_a_level_inside_the_band_and_moves_outside_it() {
        let c = chain3();
        let h = 0.2; // bands: 48..72 around 60, 20..30 around 25
        // Level 0 holds down to 48 px, level 1 holds from 72 px down.
        assert_eq!(c.select(55.0, Some(0), h), 0);
        assert_eq!(c.select(48.01, Some(0), h), 0);
        assert_eq!(c.select(47.99, Some(0), h), 1);
        assert_eq!(c.select(65.0, Some(1), h), 1);
        assert_eq!(c.select(71.99, Some(1), h), 1);
        assert_eq!(c.select(72.01, Some(1), h), 0);
        // The second boundary the same way.
        assert_eq!(c.select(22.0, Some(1), h), 1);
        assert_eq!(c.select(19.99, Some(1), h), 2);
        assert_eq!(c.select(29.99, Some(2), h), 2);
        assert_eq!(c.select(30.01, Some(2), h), 1);
        // A big jump skips levels in one step, in both directions.
        assert_eq!(c.select(1.0, Some(0), h), 2);
        assert_eq!(c.select(500.0, Some(2), h), 0);
    }

    /// A unit wobbling around a switch size, as a camera shaking by a few
    /// percent does: without hysteresis the level flips constantly, with it
    /// the level never changes.
    #[test]
    fn a_wobble_around_a_switch_size_flickers_without_hysteresis_only() {
        let c = chain3();
        let changes = |h: f32| {
            let mut level = Some(c.ideal(60.0));
            let mut changes = 0;
            for i in 0..1000 {
                let px = 60.0 * (1.0 + 0.04 * (i as f32 * 1.7).sin());
                let l = c.select(px, level, h);
                changes += usize::from(Some(l) != level);
                level = Some(l);
            }
            changes
        };
        let (without, with) = (changes(0.0), changes(0.15));
        println!("level changes in 1000 wobbling frames: {without} without, {with} with");
        assert!(without > 100, "the control must flicker: {without}");
        assert_eq!(with, 0);
    }

    /// Whatever the history, the chosen level lies between the threshold
    /// levels of the size one band down and one band up: a level is only ever
    /// held while the size is within `h` (relative) of its switch size.
    #[test]
    fn the_chosen_level_stays_within_the_band_of_the_ideal() {
        let c = chain3();
        let h = 0.15;
        let mut level = None;
        let mut state = 12345_u32;
        for _ in 0..20_000 {
            // A random walk of the size, sometimes with a big jump.
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let r = (state >> 8) as f32 / 16_777_216.0;
            let px = 2.0 + r * 120.0;
            let l = c.select(px, level, h);
            assert!(
                l >= c.ideal(px / (1.0 - h)) && l <= c.ideal(px / (1.0 + h)),
                "{px} px chose {l}"
            );
            level = Some(l);
        }
    }

    #[test]
    fn model_set_from_the_embedded_style_has_lower_poly_round_levels() {
        let style = Style::embedded();
        let set = ModelSet::from_style(&style);
        assert_eq!(set.models().len(), style.shapes().len());
        // The cube: one level, the built-in cube.
        let cube = set.model(0);
        assert_eq!(cube.len(), 1);
        assert_eq!(cube.level(0).mesh, MeshId::of(MeshKind::Cube));
        for shape in [1u8, 2] {
            let m = set.model(shape);
            assert!(m.len() >= 3, "shape {shape}: {} levels", m.len());
            // Level 0 is the full-detail built-in mesh, the rest are extra
            // meshes with strictly fewer triangles.
            let kind = style.shape(shape).mesh;
            assert_eq!(m.level(0).mesh, MeshId::of(kind));
            let tris: Vec<u32> = m
                .levels()
                .iter()
                .map(|l| set.mesh_triangles(l.mesh))
                .collect();
            assert!(tris.windows(2).all(|w| w[0] > w[1]), "{tris:?}");
            for l in &m.levels()[1..] {
                assert!(l.mesh.index() >= BUILTIN_MESHES);
            }
        }
        assert_eq!(set.mesh_count(), BUILTIN_MESHES + set.extra_meshes().len());
        // The meshes the ids name exist and have the triangles that were counted.
        for (i, mesh) in set.extra_meshes().iter().enumerate() {
            assert_eq!(
                set.mesh_triangles(MeshId((BUILTIN_MESHES + i) as u16)),
                mesh.triangle_count()
            );
        }
    }

    #[test]
    fn identical_levels_share_a_mesh_and_revisions_differ() {
        let style = Style::from_ron(
            "(palette: [(name: \"a\", srgb: (1, 2, 3))],
              shapes: [
                (mesh: Cylinder, size: (1.0, 1.0, 1.0),
                 lods: [(min_px: 40.0), (segments: Some(8), min_px: 0.0)]),
                (mesh: Cylinder, size: (2.0, 1.0, 2.0),
                 lods: [(min_px: 40.0), (segments: Some(8), min_px: 0.0)]),
                (mesh: Cone, size: (1.0, 1.0, 1.0),
                 lods: [(min_px: 40.0), (segments: Some(8), min_px: 0.0)]),
              ])",
        )
        .unwrap();
        let (a, b) = (ModelSet::from_style(&style), ModelSet::from_style(&style));
        assert_ne!(a.revision(), b.revision());
        // Two cylinders share their 8-facet mesh; the cone has its own.
        assert_eq!(a.model(0).level(1).mesh, a.model(1).level(1).mesh);
        assert_ne!(a.model(0).level(1).mesh, a.model(2).level(1).mesh);
        assert_eq!(a.extra_meshes().len(), 2);
    }

    #[test]
    fn a_set_refuses_chains_over_missing_meshes() {
        let chain = ModelLods::single(MeshId(BUILTIN_MESHES as u16));
        assert!(matches!(
            ModelSet::new(vec![], vec![chain.clone()]),
            Err(ModelError::UnknownMesh)
        ));
        assert!(ModelSet::new(vec![MeshData::cube()], vec![chain]).is_ok());
        assert!(matches!(
            ModelSet::new(vec![], vec![]),
            Err(ModelError::NoLevels)
        ));
    }

    #[test]
    fn concat_appends_shapes_and_moves_their_meshes_behind_ours() {
        let a = ModelSet::new(vec![MeshData::cube()], vec![ModelLods::single(MeshId(5))]).unwrap();
        let b = ModelSet::new(
            vec![MeshData::cone(), MeshData::cylinder()],
            vec![
                ModelLods::single(MeshId(6)),
                ModelLods::single(MeshId::of(MeshKind::Cube)),
            ],
        )
        .unwrap();
        let both = a.concat(&b).unwrap();
        assert_eq!(both.models().len(), 3);
        assert_eq!(both.extra_meshes().len(), 3);
        assert_eq!(both.model(0).level(0).mesh, MeshId(5));
        // b's second extra mesh (id 6) now sits behind a's one extra mesh: id 7.
        assert_eq!(both.model(1).level(0).mesh, MeshId(7));
        // A built-in kind keeps its id.
        assert_eq!(both.model(2).level(0).mesh, MeshId::of(MeshKind::Cube));
        assert_eq!(both.mesh_count(), BUILTIN_MESHES + 3);
    }
}
