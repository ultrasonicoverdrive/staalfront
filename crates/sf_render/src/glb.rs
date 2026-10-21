//! glTF 2.0 model loading (WP1.10): binary `.glb` files with meshes,
//! base-colour materials and a node hierarchy of parts, converted once to the
//! engine's axes and merged to one mesh per part per level of detail.
//!
//! # Engine convention (`docs/conventions.md`)
//! Metres, `+Y` up, the front of a model along `+X`, the ground at `Y = 0`,
//! right-handed (so a model's right side is `+Z`). Facing 0 points along `+X`
//! and turns towards `+Z` (`instancing::model_matrix`); the sim's 2D `(x, y)`
//! is the render `(x, z)`.
//!
//! # Axis and unit conversion, once, here
//! glTF is right-handed with `+Y` up; the spec puts the front along `+Z`,
//! but asset packs differ (the test pack uses `+X`). An [`AxisConvention`]
//! says where the source has its front and up and how many metres one unit
//! is. The loader turns that into one similarity `S = s * C`, where the rows
//! of the rotation `C` are the source's front, up and right (`front x up`):
//! a source vector `v` becomes `(v . front, v . up, v . right) * s`. Vertices
//! get `S`, normals `C`, and every node matrix `M` becomes `S M S^-1` (its
//! rotation is conjugated, its translation scaled). Because the source is
//! right-handed and `C` is built as front/up/front x up, `det C = +1`: no
//! mirroring, the winding stays counter-clockwise. The renderer and the
//! shaders never correct axes.
//!
//! # Levels, parts and materials
//! Each root node of the scene is one level of detail, named `<model>_LOD<n>`
//! (a single root without a suffix is a one-level model). A file may instead
//! have one scene per level, named `LOD0`, `LOD1`, ... ([`lod_roots`]); that
//! is read as the same model. Below a root, a node
//! without a mesh, or with children, is a **part** (`hull_LOD0`, `turret_LOD0`,
//! `barrel_LOD0`); a mesh node without children (`hull_steel_LOD0`, ...) is
//! merged into the nearest part above it with its transform relative to that
//! part. The exception is a node named `turret` or `barrel` (after the LOD
//! suffix): it is always a part, even as a childless mesh node, because it
//! has to turn or tilt on its own. So a part split per material becomes **one** vertex/index buffer
//! with the material as a per-vertex colour ([`crate::mesh::Vertex`]): the
//! material named `team` gets team weight 1 (the player colour), every other
//! material its base colour factor. `_LOD<n>` suffixes are stripped from part
//! names. A part named `turret` turns about its local `+Y`, one named
//! `barrel` tilts about its local `+Z` ([`Joint`]).
//!
//! # Not supported (a clear [`GlbError`], never a panic)
//! External or embedded-URI buffers (only the GLB `BIN` chunk), textures (the
//! materials are colours only), skins, morph targets, animation clips,
//! required extensions, and primitives other than triangle lists. Missing
//! normals are an error too (the test pack has them everywhere).

use std::fmt;
use std::path::Path;

use glam::{Mat3, Mat4, Vec3};
use serde::Deserialize;

use crate::mesh::{MeshData, Vertex};
use crate::model::{
    BUILTIN_MESHES, Joint, MAX_LODS, MAX_PARTS, MeshId, ModelBounds, ModelError, ModelLods,
    ModelSet, PartAngles, RigPart, pose_parts,
};

/// A signed coordinate axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Axis {
    PlusX,
    MinusX,
    PlusY,
    MinusY,
    PlusZ,
    MinusZ,
}

impl Axis {
    pub fn vector(self) -> Vec3 {
        match self {
            Self::PlusX => Vec3::X,
            Self::MinusX => Vec3::NEG_X,
            Self::PlusY => Vec3::Y,
            Self::MinusY => Vec3::NEG_Y,
            Self::PlusZ => Vec3::Z,
            Self::MinusZ => Vec3::NEG_Z,
        }
    }
}

/// Where a source asset has its front and up, and its unit length.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisConvention {
    pub front: Axis,
    pub up: Axis,
    /// Metres per source unit (glTF says 1).
    #[serde(default = "one")]
    pub metres_per_unit: f32,
}

fn one() -> f32 {
    1.0
}

impl AxisConvention {
    /// The glTF spec: front `+Z`, up `+Y`, metres.
    pub const GLTF: Self = Self {
        front: Axis::PlusZ,
        up: Axis::PlusY,
        metres_per_unit: 1.0,
    };
    /// Already the engine's axes: front `+X`, up `+Y`, metres (the test pack).
    pub const ENGINE: Self = Self {
        front: Axis::PlusX,
        up: Axis::PlusY,
        metres_per_unit: 1.0,
    };

    /// The rotation part `C` of the conversion: rows front, up, right.
    /// `None` if front and up are not perpendicular or the scale is not
    /// positive.
    pub fn rotation(&self) -> Option<Mat3> {
        let (f, u) = (self.front.vector(), self.up.vector());
        if f.dot(u) != 0.0 || !(self.metres_per_unit.is_finite() && self.metres_per_unit > 0.0) {
            return None;
        }
        let r = f.cross(u);
        // Rows f, u, r: the transpose of the matrix with those columns.
        Some(Mat3::from_cols(f, u, r).transpose())
    }

    /// The full conversion `S = s * C` as an affine matrix, if valid.
    pub fn matrix(&self) -> Option<Mat4> {
        self.rotation()
            .map(|c| Mat4::from_mat3(c) * Mat4::from_scale(Vec3::splat(self.metres_per_unit)))
    }
}

/// Why a model could not be loaded.
#[derive(Debug)]
pub enum GlbError {
    /// The file could not be read.
    Io(String),
    /// Not a valid glTF 2.0 binary (the parser's message).
    Parse(String),
    /// The asset says another glTF version.
    Version(String),
    /// A feature this loader does not handle.
    Unsupported(String),
    /// Well-formed glTF, but not a model this engine can use.
    Invalid(String),
}

impl fmt::Display for GlbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read model: {e}"),
            Self::Parse(e) => write!(f, "not a valid glTF 2.0 binary: {e}"),
            Self::Version(v) => write!(f, "unsupported glTF version {v} (need 2.x)"),
            Self::Unsupported(what) => write!(f, "unsupported glTF feature: {what}"),
            Self::Invalid(e) => write!(f, "invalid model: {e}"),
        }
    }
}

impl std::error::Error for GlbError {}

/// One part of a loaded level, in engine axes.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedPart {
    /// The node name without its `_LOD<n>` suffix (`hull`, `turret`, ...).
    pub name: String,
    /// Index of the parent part in the level; `None` = the unit itself.
    pub parent: Option<usize>,
    /// Transform from this part's frame to its parent's (or the unit's).
    pub rest: Mat4,
    pub joint: Joint,
    /// All mesh nodes of the part merged, in the part's frame.
    pub mesh: MeshData,
}

/// One level of detail: its parts, parents before children.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedLevel {
    pub parts: Vec<LoadedPart>,
}

/// A loaded model: its levels, finest first.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedModel {
    /// The root name without its `_LOD<n>` suffix.
    pub name: String,
    pub levels: Vec<LoadedLevel>,
}

impl LoadedModel {
    /// The same model `factor` times as big in every direction: vertex
    /// positions and the translations of the part transforms scale, rotations
    /// and normals do not. A uniform scale keeps a turret's yaw and a barrel's
    /// pitch exactly as they were.
    pub fn scaled(&self, factor: f32) -> LoadedModel {
        let mut model = self.clone();
        for level in &mut model.levels {
            for part in &mut level.parts {
                part.rest.w_axis =
                    (part.rest.w_axis.truncate() * factor).extend(part.rest.w_axis.w);
                for v in &mut part.mesh.vertices {
                    v.position = v.position.map(|c| c * factor);
                }
            }
        }
        model
    }
}

/// `name` without a trailing `_LOD<digits>`, and the number if there was one.
pub fn split_lod_suffix(name: &str) -> (&str, Option<u32>) {
    if let Some(at) = name.rfind("_LOD") {
        let digits = &name[at + 4..];
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            return (&name[..at], digits.parse().ok());
        }
    }
    (name, None)
}

/// `name` without a trailing `_LOD<digits>`.
pub fn strip_lod_suffix(name: &str) -> &str {
    split_lod_suffix(name).0
}

/// How a file stores its levels of detail (`docs/conventions.md` section 12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LodLayout {
    /// The default scene holds every LOD root. The layout the art brief
    /// prefers.
    OneScene,
    /// One scene per level, named `LOD0`, `LOD1`, ..., each with one root
    /// node `<model>_LOD<n>`. The same model as [`LodLayout::OneScene`]; the
    /// exporter of art set v4 writes it this way.
    ScenePerLod,
}

/// The level number of a scene named exactly `LOD<digits>`.
fn scene_lod(name: Option<&str>) -> Option<u32> {
    let digits = name?.strip_prefix("LOD")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The LOD root nodes of a document and the layout they were found in.
///
/// - **Scene per LOD:** the document has two or more scenes and every scene
///   is named `LOD<n>`. Each such scene holds exactly one root, named
///   `<model>_LOD<n>` with the same number, and the roots come back in level
///   order. This is read exactly like one scene with all the roots.
/// - **Otherwise:** the roots of the default scene (the first scene if the
///   file names none), in file order.
///
/// A file that mixes scenes named `LOD<n>` with other scenes is not the
/// scene-per-LOD layout; its default scene is read as before.
pub fn lod_roots(doc: &gltf::Document) -> Result<(LodLayout, Vec<gltf::Node<'_>>), GlbError> {
    let scenes: Vec<gltf::Scene<'_>> = doc.scenes().collect();
    let numbers: Vec<Option<u32>> = scenes.iter().map(|s| scene_lod(s.name())).collect();
    if scenes.len() >= 2 && numbers.iter().all(Option::is_some) {
        let mut found: Vec<(u32, gltf::Node<'_>)> = Vec::new();
        for (scene, n) in scenes.iter().zip(numbers.iter().flatten().copied()) {
            let mut nodes = scene.nodes();
            let (Some(root), None) = (nodes.next(), nodes.next()) else {
                return Err(GlbError::Invalid(format!(
                    "scene LOD{n} must hold exactly one root node"
                )));
            };
            let raw = root.name().unwrap_or("");
            if split_lod_suffix(raw).1 != Some(n) {
                return Err(GlbError::Invalid(format!(
                    "scene LOD{n} holds the root {raw:?}; it must be named <model>_LOD{n}"
                )));
            }
            if found.iter().any(|(m, _)| *m == n) {
                return Err(GlbError::Invalid(format!("two scenes are named LOD{n}")));
            }
            found.push((n, root));
        }
        found.sort_by_key(|(n, _)| *n);
        return Ok((
            LodLayout::ScenePerLod,
            found.into_iter().map(|(_, node)| node).collect(),
        ));
    }
    let scene = doc
        .default_scene()
        .or_else(|| scenes.first().cloned())
        .ok_or_else(|| GlbError::Invalid("the file has no scene".into()))?;
    Ok((LodLayout::OneScene, scene.nodes().collect()))
}

/// A part name that carries a joint (`turret` yaws, `barrel` pitches), after
/// the LOD suffix is stripped. Such a node is always a part of its own.
fn is_joint_name(name: &str) -> bool {
    matches!(name, "turret" | "barrel")
}

/// Read and load a `.glb` file.
pub fn load_glb_file(path: &Path, axes: &AxisConvention) -> Result<LoadedModel, GlbError> {
    let bytes =
        std::fs::read(path).map_err(|e| GlbError::Io(format!("{}: {e}", path.display())))?;
    load_glb(&bytes, axes)
}

/// Load a model from the bytes of a `.glb` file.
pub fn load_glb(bytes: &[u8], axes: &AxisConvention) -> Result<LoadedModel, GlbError> {
    let convert = axes.matrix().ok_or_else(|| {
        GlbError::Invalid("axis convention: front and up must be perpendicular, scale > 0".into())
    })?;
    // The GLB header carries a container version of its own.
    if bytes.len() >= 8 && &bytes[0..4] == b"glTF" {
        let v = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if v != 2 {
            return Err(GlbError::Version(format!("{v} (GLB container)")));
        }
    }
    let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| GlbError::Parse(e.to_string()))?;
    let doc = &gltf.document;
    let json = doc.as_json();
    if !json.asset.version.starts_with("2.") {
        return Err(GlbError::Version(json.asset.version.clone()));
    }
    if let Some(min) = &json.asset.min_version
        && !min.starts_with("2.")
    {
        return Err(GlbError::Version(min.clone()));
    }
    if let Some(ext) = doc.extensions_required().next() {
        return Err(GlbError::Unsupported(format!("required extension {ext}")));
    }
    if doc.skins().next().is_some() {
        return Err(GlbError::Unsupported("skins (skinned animation)".into()));
    }
    if doc.animations().next().is_some() {
        return Err(GlbError::Unsupported("animation clips".into()));
    }
    if doc.textures().next().is_some() || doc.images().next().is_some() {
        return Err(GlbError::Unsupported(
            "textures (materials are colours only)".into(),
        ));
    }
    for buffer in doc.buffers() {
        if let gltf::buffer::Source::Uri(_) = buffer.source() {
            return Err(GlbError::Unsupported(
                "buffers outside the GLB BIN chunk".into(),
            ));
        }
    }
    let blob = gltf.blob.as_deref();

    let (_, scene_roots) = lod_roots(doc)?;
    let mut roots: Vec<(u32, gltf::Node<'_>)> = Vec::new();
    let mut model_name: Option<String> = None;
    for node in scene_roots {
        let name = node.name().unwrap_or("");
        let (base, lod) = split_lod_suffix(name);
        roots.push((lod.unwrap_or(0), node));
        match &model_name {
            None => model_name = Some(base.to_owned()),
            Some(n) if n != base => {
                return Err(GlbError::Invalid(format!(
                    "scene roots name different models: {n} and {base}"
                )));
            }
            Some(_) => {}
        }
    }
    roots.sort_by_key(|(lod, _)| *lod);
    if roots.is_empty() {
        return Err(GlbError::Invalid("the scene has no root nodes".into()));
    }
    if roots.len() > MAX_LODS {
        return Err(GlbError::Invalid(format!(
            "{} levels of detail (at most {MAX_LODS})",
            roots.len()
        )));
    }
    if roots
        .iter()
        .enumerate()
        .any(|(i, (lod, _))| *lod as usize != i)
    {
        return Err(GlbError::Invalid(
            "root nodes must be <model>_LOD0, _LOD1, ... without gaps".into(),
        ));
    }

    let mut levels = Vec::new();
    for (_, root) in &roots {
        let mut level = Level::default();
        level.enter(root, 0)?;
        let root_matrix = node_matrix(root);
        if root.mesh().is_some() {
            // The root itself has geometry: it is the first part.
            let at = level.add_part(root, None, root_matrix)?;
            for child in root.children() {
                level.visit(&child, Some(at), node_matrix(&child), blob, 1)?;
            }
            level.merge_node(root, at, Mat4::IDENTITY, blob)?;
        } else {
            for child in root.children() {
                level.visit(&child, None, root_matrix * node_matrix(&child), blob, 1)?;
            }
        }
        level.prune();
        if level.parts.is_empty() {
            return Err(GlbError::Invalid(format!(
                "level {} has no geometry",
                root.name().unwrap_or("?")
            )));
        }
        if level.parts.len() > MAX_PARTS {
            return Err(GlbError::Invalid(format!(
                "{} parts in one level (at most {MAX_PARTS})",
                level.parts.len()
            )));
        }
        levels.push(level.convert(convert));
    }
    Ok(LoadedModel {
        name: model_name.unwrap_or_default(),
        levels,
    })
}

/// Why an attribute could not be read: absent, or its accessor runs past
/// the data it points into (the reader returns nothing in both cases).
fn missing(prim: &gltf::Primitive<'_>, semantic: gltf::Semantic, name: &str) -> GlbError {
    match prim.get(&semantic) {
        None if name == "NORMAL" => GlbError::Unsupported("primitives without NORMAL".into()),
        None => GlbError::Invalid(format!("a primitive has no {name}")),
        Some(_) => GlbError::Invalid(format!(
            "{name} accessor out of range (past the end of its buffer, or the buffer is missing)"
        )),
    }
}

fn node_matrix(node: &gltf::Node<'_>) -> Mat4 {
    Mat4::from_cols_array_2d(&node.transform().matrix())
}

/// A level while it is being collected, in source axes.
#[derive(Default)]
struct Level {
    parts: Vec<LoadedPart>,
    /// Nodes already met in this level: a node that is its own child, a
    /// cycle or a node used twice is refused instead of recursing forever.
    seen: std::collections::BTreeSet<usize>,
}

/// Deepest node hierarchy below a level root (the test pack needs 4).
pub const MAX_NODE_DEPTH: usize = 32;

impl Level {
    /// Mark `node` as visited at `depth`; an error for a cycle, a node used
    /// twice or a hierarchy deeper than [`MAX_NODE_DEPTH`].
    fn enter(&mut self, node: &gltf::Node<'_>, depth: usize) -> Result<(), GlbError> {
        if depth > MAX_NODE_DEPTH {
            return Err(GlbError::Invalid(format!(
                "node hierarchy deeper than {MAX_NODE_DEPTH} levels"
            )));
        }
        if !self.seen.insert(node.index()) {
            return Err(GlbError::Invalid(format!(
                "node {} ({}) appears twice in the hierarchy (a cycle or a shared node)",
                node.index(),
                node.name().unwrap_or("unnamed")
            )));
        }
        Ok(())
    }

    /// Drop the parts without geometry below them (the test pack's scout and
    /// buildings have an empty `turret` and `barrel` node): they would be
    /// draw calls of nothing. An empty part that carries a part with
    /// geometry stays, as its frame and joint.
    fn prune(&mut self) {
        let n = self.parts.len();
        let mut keep = vec![false; n];
        // Parents come before their children, so walk backwards.
        for i in (0..n).rev() {
            let carries = (i + 1..n).any(|j| keep[j] && self.parts[j].parent == Some(i));
            keep[i] = !self.parts[i].mesh.indices.is_empty() || carries;
        }
        let mut new_index = vec![None; n];
        let mut kept = Vec::new();
        for (i, part) in std::mem::take(&mut self.parts).into_iter().enumerate() {
            if keep[i] {
                new_index[i] = Some(kept.len());
                kept.push(LoadedPart {
                    parent: part.parent.and_then(|p| new_index[p]),
                    ..part
                });
            }
        }
        self.parts = kept;
    }

    fn add_part(
        &mut self,
        node: &gltf::Node<'_>,
        parent: Option<usize>,
        rest: Mat4,
    ) -> Result<usize, GlbError> {
        if !rest.is_finite() {
            return Err(GlbError::Invalid("a node matrix is not finite".into()));
        }
        let name = strip_lod_suffix(node.name().unwrap_or("part")).to_owned();
        // A part's frame goes to the GPU as an instance matrix, and the
        // shader turns normals with it (no inverse transpose) and culls back
        // faces: a mirror or a non-uniform scale would light it wrongly or
        // turn it inside out.
        let linear = Mat3::from_mat4(rest);
        let lengths = [0, 1, 2].map(|i| linear.col(i).length());
        let (lo, hi) = (
            lengths.iter().copied().fold(f32::MAX, f32::min),
            lengths.iter().copied().fold(0.0, f32::max),
        );
        if linear.determinant() <= 0.0 {
            return Err(GlbError::Unsupported(format!(
                "part {name}: a mirrored (or degenerate) node transform"
            )));
        }
        if hi - lo > 1e-3 * hi {
            return Err(GlbError::Unsupported(format!(
                "part {name}: a non-uniform scale in the node transform"
            )));
        }
        // A node given as a raw `matrix` can also shear: columns of equal
        // length that are not at right angles (positive determinant, uniform
        // lengths, and still skewed lighting). The tolerance is the same
        // 1e-3 as for the lengths, here on the cosine of the angle between
        // two columns (0.06 degrees off a right angle).
        for (a, b) in [(0, 1), (0, 2), (1, 2)] {
            let cosine = linear.col(a).dot(linear.col(b)) / (lengths[a] * lengths[b]);
            if cosine.abs() > 1e-3 {
                return Err(GlbError::Unsupported(format!(
                    "part {name}: a skewed (non-orthogonal) node transform \
                     (columns {a} and {b} are not at right angles)"
                )));
            }
        }
        let joint = match name.as_str() {
            "turret" => Joint::Yaw,
            "barrel" => Joint::Pitch,
            _ => Joint::Fixed,
        };
        self.parts.push(LoadedPart {
            name,
            parent,
            rest,
            joint,
            mesh: MeshData::default(),
        });
        Ok(self.parts.len() - 1)
    }

    /// `to_parent`: from this node's frame to the frame of `parent` (or the unit's).
    fn visit(
        &mut self,
        node: &gltf::Node<'_>,
        parent: Option<usize>,
        to_parent: Mat4,
        blob: Option<&[u8]>,
        depth: usize,
    ) -> Result<(), GlbError> {
        self.enter(node, depth)?;
        // A node without a mesh or with children is a part (a frame). So is
        // a turret or barrel, even a childless mesh node: it has to turn or
        // tilt on its own, and folding it into its parent would lose that.
        let is_part = node.mesh().is_none()
            || node.children().next().is_some()
            || is_joint_name(strip_lod_suffix(node.name().unwrap_or("")));
        if is_part {
            let at = self.add_part(node, parent, to_parent)?;
            self.merge_node(node, at, Mat4::IDENTITY, blob)?;
            for child in node.children() {
                self.visit(&child, Some(at), node_matrix(&child), blob, depth + 1)?;
            }
            return Ok(());
        }
        // A mesh leaf: merged into its part. Directly under the root it
        // gets a part of its own (a model that is one mesh per material).
        let at = match parent {
            Some(p) => p,
            None => match self.parts.iter().position(|p| p.name == "body") {
                Some(p) => p,
                None => {
                    self.parts.push(LoadedPart {
                        name: "body".into(),
                        parent: None,
                        rest: Mat4::IDENTITY,
                        joint: Joint::Fixed,
                        mesh: MeshData::default(),
                    });
                    self.parts.len() - 1
                }
            },
        };
        self.merge_node(node, at, to_parent, blob)
    }

    /// Append the mesh of `node` (if any) to part `at`, transformed by `m`.
    fn merge_node(
        &mut self,
        node: &gltf::Node<'_>,
        at: usize,
        m: Mat4,
        blob: Option<&[u8]>,
    ) -> Result<(), GlbError> {
        let Some(mesh) = node.mesh() else {
            return Ok(());
        };
        if !m.is_finite() || m.determinant().abs() < 1e-12 {
            return Err(GlbError::Invalid(
                "a mesh node matrix is singular or not finite".into(),
            ));
        }
        let normal_m = Mat3::from_mat4(m).inverse().transpose();
        let flip = m.determinant() < 0.0;
        let out = &mut self.parts[at].mesh;
        for prim in mesh.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                return Err(GlbError::Unsupported(format!(
                    "primitive mode {:?} (triangle lists only)",
                    prim.mode()
                )));
            }
            if prim.morph_targets().next().is_some() {
                return Err(GlbError::Unsupported("morph targets".into()));
            }
            let material = prim.material();
            let pbr = material.pbr_metallic_roughness();
            let [r, g, b, _] = pbr.base_color_factor();
            let team = material.name() == Some("team");
            let color = if team { Vertex::TEAM } else { [r, g, b, 0.0] };
            let reader = prim.reader(|buffer| match buffer.source() {
                gltf::buffer::Source::Bin => blob,
                gltf::buffer::Source::Uri(_) => None,
            });
            let positions: Vec<[f32; 3]> = reader
                .read_positions()
                .ok_or_else(|| missing(&prim, gltf::Semantic::Positions, "POSITION"))?
                .collect();
            let normals: Vec<[f32; 3]> = reader
                .read_normals()
                .ok_or_else(|| missing(&prim, gltf::Semantic::Normals, "NORMAL"))?
                .collect();
            if normals.len() != positions.len() {
                return Err(GlbError::Invalid(
                    "NORMAL and POSITION counts differ".into(),
                ));
            }
            let indices: Vec<u32> = match reader.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };
            if !indices.len().is_multiple_of(3)
                || indices.iter().any(|&i| i as usize >= positions.len())
            {
                return Err(GlbError::Invalid(
                    "index out of range or not triangles".into(),
                ));
            }
            let base = out.vertices.len() as u32;
            for (p, n) in positions.iter().zip(&normals) {
                let p = m.transform_point3(Vec3::from(*p));
                let n = (normal_m * Vec3::from(*n)).normalize_or_zero();
                if !p.is_finite() {
                    return Err(GlbError::Invalid("a vertex is not finite".into()));
                }
                out.vertices.push(Vertex {
                    position: p.to_array(),
                    normal: n.to_array(),
                    color,
                });
            }
            for tri in indices.chunks_exact(3) {
                let (a, b, c) = (tri[0] + base, tri[1] + base, tri[2] + base);
                if flip {
                    out.indices.extend_from_slice(&[a, c, b]);
                } else {
                    out.indices.extend_from_slice(&[a, b, c]);
                }
            }
        }
        Ok(())
    }

    /// To engine axes: `S M S^-1` for every part transform, `S` for the
    /// vertices, the rotation for the normals.
    fn convert(self, s: Mat4) -> LoadedLevel {
        let s_inv = s.inverse();
        let rot = Mat3::from_mat4(s);
        let scale = rot.col(0).length();
        let rot = rot * (1.0 / scale);
        LoadedLevel {
            parts: self
                .parts
                .into_iter()
                .map(|mut p| {
                    p.rest = s * p.rest * s_inv;
                    for v in &mut p.mesh.vertices {
                        v.position = s.transform_point3(Vec3::from(v.position)).to_array();
                        v.normal = (rot * Vec3::from(v.normal)).to_array();
                    }
                    p
                })
                .collect(),
        }
    }
}

/// Default switch sizes between the levels of a glTF chain, in pixels (the
/// test pack's `screenPixelMin`, the same as `units.ron`).
pub const DEFAULT_SWITCH_PX: [f32; MAX_LODS - 1] = [60.0, 30.0, 15.0];

/// A set of loaded models as a [`ModelSet`]: model `i` is chain `i`, every
/// (part, level) its own mesh id, so one draw call each. `switch_px` are the
/// switch sizes (at least `levels - 1` of them).
pub fn model_set(models: &[LoadedModel], switch_px: &[f32]) -> Result<ModelSet, ModelError> {
    let mut extra = Vec::new();
    let mut chains = Vec::new();
    for model in models {
        let mut levels = Vec::new();
        for level in &model.levels {
            let mut parts = Vec::new();
            for part in &level.parts {
                let mesh = MeshId(
                    u16::try_from(BUILTIN_MESHES + extra.len())
                        .map_err(|_| ModelError::UnknownMesh)?,
                );
                extra.push(part.mesh.clone());
                parts.push(RigPart {
                    mesh,
                    parent: part.parent.map(|p| p as u8),
                    rest: part.rest,
                    joint: part.joint,
                });
            }
            levels.push(parts);
        }
        let n = levels.len();
        if switch_px.len() + 1 < n {
            return Err(ModelError::SwitchCount);
        }
        let bounds = bounds_of(model);
        chains.push(ModelLods::with_parts(
            levels,
            switch_px[..n - 1].to_vec(),
            bounds,
        )?);
    }
    ModelSet::new(extra, chains)
}

/// The rest-pose box of the finest level and the farthest any vertex of any
/// level can get from the origin with the joints turned.
pub fn bounds_of(model: &LoadedModel) -> ModelBounds {
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    let mut reach: f32 = 0.0;
    for (li, level) in model.levels.iter().enumerate() {
        let rig: Vec<RigPart> = level
            .parts
            .iter()
            .map(|p| RigPart {
                mesh: MeshId(0),
                parent: p.parent.map(|q| q as u8),
                rest: p.rest,
                joint: p.joint,
            })
            .collect();
        let mut world = [Mat4::IDENTITY; MAX_PARTS];
        pose_parts(Mat4::IDENTITY, &rig, PartAngles::default(), &mut world);
        // Upper bound of a part's origin distance under any joint angles.
        let mut origin_reach = [0.0_f32; MAX_PARTS];
        for (i, part) in level.parts.iter().enumerate().take(MAX_PARTS) {
            let up = part.parent.map_or(0.0, |p| origin_reach[p]);
            origin_reach[i] = up + part.rest.w_axis.truncate().length();
            let linear = Mat3::from_mat4(part.rest);
            let stretch = linear
                .col(0)
                .length()
                .max(linear.col(1).length())
                .max(linear.col(2).length());
            for v in &part.mesh.vertices {
                let p = Vec3::from(v.position);
                reach = reach.max(origin_reach[i] + stretch * p.length());
                if li == 0 {
                    let w = world[i].transform_point3(p);
                    min = min.min(w);
                    max = max.max(w);
                }
            }
        }
    }
    if min.x > max.x {
        (min, max) = (Vec3::ZERO, Vec3::ZERO);
    }
    ModelBounds { min, max, reach }
}

/// A minimal glTF 2.0 binary writer for test fixtures (the orientation
/// model, broken files): nodes with a name, a translation and at most one
/// mesh of one triangle-list primitive with positions, normals and a
/// material colour. Not a general exporter.
pub mod writer {
    /// A mesh primitive: corners, normals, triangle indices and a material.
    #[derive(Clone, Debug, Default)]
    pub struct Prim {
        pub positions: Vec<[f32; 3]>,
        pub normals: Vec<[f32; 3]>,
        pub indices: Vec<u32>,
        /// Index into [`Scene::materials`].
        pub material: usize,
        /// glTF primitive mode; `None` = triangles (tests write others).
        pub mode: Option<u32>,
    }

    /// A node: name, translation, optional mesh primitive, children.
    #[derive(Clone, Debug, Default)]
    pub struct Node {
        pub name: String,
        pub translation: [f32; 3],
        /// Written as `scale` when set (tests: mirrors, non-uniform scale).
        pub scale: Option<[f32; 3]>,
        /// Written as a raw `matrix` (16 numbers, column-major as in glTF)
        /// instead of `translation` and `scale` when set (tests: a shear).
        pub matrix: Option<[f32; 16]>,
        pub prim: Option<Prim>,
        pub children: Vec<usize>,
    }

    /// The whole file: materials `(name, linear rgb)`, nodes, root nodes.
    #[derive(Clone, Debug, Default)]
    pub struct Scene {
        pub materials: Vec<(String, [f32; 3])>,
        pub nodes: Vec<Node>,
        pub roots: Vec<usize>,
        /// Written as `asset.version` (normally `"2.0"`).
        pub version: String,
        /// Extra top-level JSON members, e.g. `"skins":[...]` (tests).
        pub extra_json: String,
    }

    fn list<T: std::fmt::Display>(v: impl IntoIterator<Item = T>) -> String {
        v.into_iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }

    fn f(v: f32) -> String {
        // Debug keeps a decimal point and round-trips exactly.
        format!("{v:?}")
    }

    /// The `.glb` bytes of `scene`.
    pub fn write(scene: &Scene) -> Vec<u8> {
        let mut bin: Vec<u8> = Vec::new();
        let (mut views, mut accessors, mut meshes) = (Vec::new(), Vec::new(), Vec::new());
        let mut nodes_json = Vec::new();
        for node in &scene.nodes {
            let mut members = vec![format!("\"name\":\"{}\"", node.name)];
            if let Some(m) = node.matrix {
                members.push(format!("\"matrix\":[{}]", list(m.map(f))));
            } else {
                members.push(format!(
                    "\"translation\":[{},{},{}]",
                    f(node.translation[0]),
                    f(node.translation[1]),
                    f(node.translation[2])
                ));
                if let Some(s) = node.scale {
                    members.push(format!("\"scale\":[{},{},{}]", f(s[0]), f(s[1]), f(s[2])));
                }
            }
            if !node.children.is_empty() {
                members.push(format!("\"children\":[{}]", list(&node.children)));
            }
            if let Some(p) = &node.prim {
                let mut attr = Vec::new();
                for (data, kind) in [(&p.positions, "POSITION"), (&p.normals, "NORMAL")] {
                    let offset = bin.len();
                    for v in data.iter() {
                        for c in v {
                            bin.extend_from_slice(&c.to_le_bytes());
                        }
                    }
                    views.push(format!(
                        "{{\"buffer\":0,\"byteOffset\":{offset},\"byteLength\":{}}}",
                        data.len() * 12
                    ));
                    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                    for v in data.iter() {
                        for k in 0..3 {
                            lo[k] = lo[k].min(v[k]);
                            hi[k] = hi[k].max(v[k]);
                        }
                    }
                    let bounds = if kind == "POSITION" {
                        format!(
                            ",\"min\":[{}],\"max\":[{}]",
                            list(lo.map(f)),
                            list(hi.map(f))
                        )
                    } else {
                        String::new()
                    };
                    accessors.push(format!(
                        "{{\"bufferView\":{},\"componentType\":5126,\"count\":{},\"type\":\"VEC3\"{bounds}}}",
                        views.len() - 1,
                        data.len()
                    ));
                    attr.push(format!("\"{kind}\":{}", accessors.len() - 1));
                }
                let offset = bin.len();
                for i in &p.indices {
                    bin.extend_from_slice(&i.to_le_bytes());
                }
                views.push(format!(
                    "{{\"buffer\":0,\"byteOffset\":{offset},\"byteLength\":{}}}",
                    p.indices.len() * 4
                ));
                accessors.push(format!(
                    "{{\"bufferView\":{},\"componentType\":5125,\"count\":{},\"type\":\"SCALAR\"}}",
                    views.len() - 1,
                    p.indices.len()
                ));
                let mode = p.mode.map_or(String::new(), |m| format!(",\"mode\":{m}"));
                meshes.push(format!(
                    "{{\"primitives\":[{{\"attributes\":{{{}}},\"indices\":{},\"material\":{}{mode}}}]}}",
                    attr.join(","),
                    accessors.len() - 1,
                    p.material
                ));
                members.push(format!("\"mesh\":{}", meshes.len() - 1));
            }
            nodes_json.push(format!("{{{}}}", members.join(",")));
        }
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let materials = scene
            .materials
            .iter()
            .map(|(name, c)| {
                format!(
                    "{{\"name\":\"{name}\",\"pbrMetallicRoughness\":{{\"baseColorFactor\":[{},{},{},1.0]}}}}",
                    f(c[0]),
                    f(c[1]),
                    f(c[2])
                )
            })
            .collect::<Vec<_>>();
        let mut json = format!(
            "{{\"asset\":{{\"version\":\"{}\"}},\"scene\":0,\"scenes\":[{{\"nodes\":[{}]}}],\"nodes\":[{}],\"meshes\":[{}],\"materials\":[{}],\"accessors\":[{}],\"bufferViews\":[{}],\"buffers\":[{{\"byteLength\":{}}}]{}}}",
            scene.version,
            list(&scene.roots),
            nodes_json.join(","),
            meshes.join(","),
            materials.join(","),
            accessors.join(","),
            views.join(","),
            bin.len(),
            scene.extra_json
        )
        .into_bytes();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let total = 12 + 8 + json.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&json);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }

    /// Material colours of the [`orientation`] model.
    pub const FRONT_RED: [f32; 3] = [0.80, 0.04, 0.04];
    pub const TOP_GREEN: [f32; 3] = [0.04, 0.70, 0.04];
    pub const RIGHT_BLUE: [f32; 3] = [0.04, 0.08, 0.85];
    pub const STEEL: [f32; 3] = [0.30, 0.30, 0.30];
    pub const DARK: [f32; 3] = [0.05, 0.05, 0.05];

    /// The orientation test model, written in the source axes `axes`: in
    /// engine axes a hull box `x -1..1, y 0..1, z -0.5..0.5` whose front
    /// (`+X`) face is red, top (`+Y`) green and right (`+Z`) blue, the other
    /// three steel grey, each face its own mesh node (so the loader has to
    /// merge them); a team-coloured turret box on top at `(0, 1, 0)` and a
    /// dark barrel along the front at `(0.3, 0.2, 0)` from the turret. One
    /// level, `orient_LOD0`. Loading it with the same `axes` must give back
    /// exactly that.
    pub fn orientation(axes: &super::AxisConvention) -> Scene {
        use glam::Vec3;
        let c = axes.rotation().expect("a valid convention");
        let s = axes.metres_per_unit;
        // Engine -> source: the inverse of `s * C`.
        let to_source = |v: Vec3| (c.transpose() * v) / s;
        let source_box = |lo: [f32; 3], hi: [f32; 3]| {
            let (a, b) = (to_source(Vec3::from(lo)), to_source(Vec3::from(hi)));
            (a.min(b).to_array(), a.max(b).to_array())
        };
        let materials = vec![
            ("front".to_owned(), FRONT_RED),
            ("top".to_owned(), TOP_GREEN),
            ("right".to_owned(), RIGHT_BLUE),
            ("steel".to_owned(), STEEL),
            ("team".to_owned(), [0.9, 0.9, 0.9]),
            ("dark".to_owned(), DARK),
        ];
        // Material of a box face by where its normal points in engine axes.
        let face_material = |normal: [f32; 3]| {
            let n = c * Vec3::from(normal);
            if n.x > 0.5 {
                0
            } else if n.y > 0.5 {
                1
            } else if n.z > 0.5 {
                2
            } else {
                3
            }
        };
        let mut nodes = vec![
            Node {
                name: "orient_LOD0".into(),
                children: vec![1],
                ..Node::default()
            },
            Node {
                name: "hull_LOD0".into(),
                ..Node::default()
            },
        ];
        let (lo, hi) = source_box([-1.0, 0.0, -0.5], [1.0, 1.0, 0.5]);
        for (i, mut prim) in box_faces(lo, hi, [0; 6]).into_iter().enumerate() {
            prim.material = face_material(prim.normals[0]);
            nodes.push(Node {
                name: format!("hull_face{i}_LOD0"),
                prim: Some(prim),
                ..Node::default()
            });
            let last = nodes.len() - 1;
            nodes[1].children.push(last);
        }
        let turret = nodes.len();
        nodes.push(Node {
            name: "turret_LOD0".into(),
            translation: to_source(Vec3::new(0.0, 1.0, 0.0)).to_array(),
            ..Node::default()
        });
        nodes[1].children.push(turret);
        let (lo, hi) = source_box([-0.3, 0.0, -0.3], [0.3, 0.4, 0.3]);
        for (i, prim) in box_faces(lo, hi, [4; 6]).into_iter().enumerate() {
            nodes.push(Node {
                name: format!("turret_team{i}_LOD0"),
                prim: Some(prim),
                ..Node::default()
            });
            let last = nodes.len() - 1;
            nodes[turret].children.push(last);
        }
        let barrel = nodes.len();
        nodes.push(Node {
            name: "barrel_LOD0".into(),
            translation: to_source(Vec3::new(0.3, 0.2, 0.0)).to_array(),
            ..Node::default()
        });
        nodes[turret].children.push(barrel);
        let (lo, hi) = source_box([0.0, -0.05, -0.05], [0.8, 0.05, 0.05]);
        for (i, prim) in box_faces(lo, hi, [5; 6]).into_iter().enumerate() {
            nodes.push(Node {
                name: format!("barrel_dark{i}_LOD0"),
                prim: Some(prim),
                ..Node::default()
            });
            let last = nodes.len() - 1;
            nodes[barrel].children.push(last);
        }
        Scene {
            materials,
            nodes,
            roots: vec![0],
            version: "2.0".into(),
            extra_json: String::new(),
        }
    }

    /// Flat-shaded box `lo..hi` with one material per face, in the order
    /// `+X, -X, +Y, -Y, +Z, -Z` (counter-clockwise from outside).
    pub fn box_faces(lo: [f32; 3], hi: [f32; 3], materials: [usize; 6]) -> Vec<Prim> {
        let c = |x: bool, y: bool, z: bool| {
            [
                if x { hi[0] } else { lo[0] },
                if y { hi[1] } else { lo[1] },
                if z { hi[2] } else { lo[2] },
            ]
        };
        let faces: [([[f32; 3]; 4], [f32; 3]); 6] = [
            (
                [
                    c(true, false, false),
                    c(true, true, false),
                    c(true, true, true),
                    c(true, false, true),
                ],
                [1.0, 0.0, 0.0],
            ),
            (
                [
                    c(false, false, false),
                    c(false, false, true),
                    c(false, true, true),
                    c(false, true, false),
                ],
                [-1.0, 0.0, 0.0],
            ),
            (
                [
                    c(false, true, false),
                    c(false, true, true),
                    c(true, true, true),
                    c(true, true, false),
                ],
                [0.0, 1.0, 0.0],
            ),
            (
                [
                    c(false, false, false),
                    c(true, false, false),
                    c(true, false, true),
                    c(false, false, true),
                ],
                [0.0, -1.0, 0.0],
            ),
            (
                [
                    c(false, false, true),
                    c(true, false, true),
                    c(true, true, true),
                    c(false, true, true),
                ],
                [0.0, 0.0, 1.0],
            ),
            (
                [
                    c(false, false, false),
                    c(false, true, false),
                    c(true, true, false),
                    c(true, false, false),
                ],
                [0.0, 0.0, -1.0],
            ),
        ];
        faces
            .iter()
            .zip(materials)
            .map(|((corners, n), material)| Prim {
                positions: corners.to_vec(),
                normals: vec![*n; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
                material,
                mode: None,
            })
            .collect()
    }
}

/// The models of a pack as listed in `assets/config/models.ron`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackFile {
    pub packs: std::collections::BTreeMap<String, PackSpec>,
}

/// One pack: a directory under `assets/`, its axes, switch sizes and models.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackSpec {
    pub dir: String,
    pub axes: AxisConvention,
    /// Switch sizes between the levels in pixels (one fewer than the levels).
    pub lod_px: Vec<f32>,
    /// Model `i` is the look of unit kind `i` (wrapping around).
    pub models: Vec<PackModel>,
}

/// One model of a pack: the `.glb` file and the strategic icon of its kind.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackModel {
    pub file: String,
    pub icon: crate::icons::IconShape,
}

/// A pack loaded: the models in kind order with their icons.
pub struct LoadedPack {
    /// The models as loaded, each once.
    pub models: Vec<LoadedModel>,
    /// The icon of each *shape* of [`LoadedPack::set`]; the same length as
    /// `set.models()`. For a pack from `models.ron` that is one per model; a
    /// roster (`sf_game`) may list a model under several icons (the same
    /// meshes, one shape per icon).
    pub icons: Vec<crate::icons::IconId>,
    pub set: ModelSet,
}

impl LoadedPack {
    /// One shape per model: its real size (finest level) and the icon of its
    /// kind.
    fn shapes(&self) -> Vec<crate::style::Shape> {
        self.set
            .models()
            .iter()
            .zip(&self.icons)
            .map(|(chain, &icon)| {
                let b = chain.bounds().copied().unwrap_or(ModelBounds {
                    min: Vec3::ZERO,
                    max: Vec3::ONE,
                    reach: 1.0,
                });
                let size = (b.max - b.min).max(Vec3::splat(0.01));
                crate::style::Shape {
                    mesh: crate::scene::MeshKind::Cube,
                    size: [size.x, b.max.y.max(0.01), size.z],
                    icon,
                }
            })
            .collect()
    }

    /// `base` (palette, icon and level settings) with one shape per model:
    /// its real size (finest level) and the icon of its kind.
    pub fn style(&self, base: &crate::Style) -> Result<crate::Style, crate::style::StyleError> {
        base.with_shapes(self.shapes())
    }

    /// The placeholder shapes of `base` followed by one shape per model:
    /// shape `base.shape_count() + i` is model `i`. Goes with the set
    /// `ModelSet::from_style(base).concat(&pack.set)`; this is how the demo
    /// draws cubes, cylinders, cones and glTF models in one scene.
    pub fn style_after(
        &self,
        base: &crate::Style,
    ) -> Result<crate::Style, crate::style::StyleError> {
        let mut shapes = base.shapes().to_vec();
        shapes.extend(self.shapes());
        base.with_shapes(shapes)
    }
}

/// The path of `models.ron` under the assets root.
pub const MODELS_CONFIG: &str = "config/models.ron";

/// Load pack `name` from the text of `models.ron`, with the `.glb` paths
/// relative to `assets`.
pub fn load_pack(config: &str, name: &str, assets: &Path) -> Result<LoadedPack, GlbError> {
    let file: PackFile =
        ron::from_str(config).map_err(|e| GlbError::Invalid(format!("models.ron: {e}")))?;
    let spec = file.packs.get(name).ok_or_else(|| {
        GlbError::Invalid(format!(
            "no model pack {name:?} (known: {})",
            file.packs.keys().cloned().collect::<Vec<_>>().join(", ")
        ))
    })?;
    if spec.models.is_empty() || spec.models.len() > 255 {
        return Err(GlbError::Invalid("a pack needs 1 to 255 models".into()));
    }
    let dir = assets.join(&spec.dir);
    let mut models = Vec::new();
    for m in &spec.models {
        let path = dir.join(&m.file);
        let model = load_glb_file(&path, &spec.axes).map_err(|e| match e {
            GlbError::Io(e) => GlbError::Io(e),
            other => GlbError::Invalid(format!("{}: {other}", path.display())),
        })?;
        models.push(model);
    }
    let set = model_set(&models, &spec.lod_px)
        .map_err(|e| GlbError::Invalid(format!("pack {name}: {e}")))?;
    Ok(LoadedPack {
        icons: spec.models.iter().map(|m| m.icon.into()).collect(),
        models,
        set,
    })
}

/// Load one `.glb` file by its path under `assets/` (WP3.2): the model of a
/// unit definition. The file must lie in the directory (`dir`) of a pack in
/// `models.ron`, which says how to read it (axes) and where its levels of
/// detail switch; `icon` is the strategic icon the model's shape gets (an atlas
/// layer, see [`crate::icon_atlas::IconAtlas`]). The
/// result is a pack of one model, so several of them can be joined with
/// [`ModelSet::concat`] without the models having to be one pack's list.
///
/// With `fit` set to `(along x, along z)` metres the model is scaled, the
/// same in every direction, until its finest level just fits a box of that
/// size on the ground (it may grow or shrink, by no more than a factor of 4
/// either way): a building then stands on its footprint of 4 m cells.
pub fn load_pack_model(
    config: &str,
    path: &str,
    icon: crate::icons::IconId,
    assets: &Path,
    fit: Option<(f32, f32)>,
) -> Result<LoadedPack, GlbError> {
    let file: PackFile =
        ron::from_str(config).map_err(|e| GlbError::Invalid(format!("models.ron: {e}")))?;
    let (name, spec) = file
        .packs
        .iter()
        .find(|(_, spec)| {
            path.strip_prefix(spec.dir.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
        })
        .ok_or_else(|| {
            GlbError::Invalid(format!(
                "{path}: no model pack in models.ron has this directory"
            ))
        })?;
    let file = assets.join(path);
    let model = load_glb_file(&file, &spec.axes).map_err(|e| match e {
        GlbError::Io(e) => GlbError::Io(e),
        other => GlbError::Invalid(format!("{}: {other}", file.display())),
    })?;
    let model = match fit {
        Some((x, z)) => {
            let b = bounds_of(&model);
            let (have_x, have_z) = (b.max.x - b.min.x, b.max.z - b.min.z);
            if have_x > 0.0 && have_z > 0.0 && x > 0.0 && z > 0.0 {
                model.scaled((x / have_x).min(z / have_z).clamp(0.25, 4.0))
            } else {
                model
            }
        }
        None => model,
    };
    let set = model_set(std::slice::from_ref(&model), &spec.lod_px)
        .map_err(|e| GlbError::Invalid(format!("pack {name}: {e}")))?;
    Ok(LoadedPack {
        models: vec![model],
        icons: vec![icon],
        set,
    })
}

#[cfg(test)]
mod tests;
