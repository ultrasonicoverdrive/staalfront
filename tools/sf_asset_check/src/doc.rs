//! What the glTF document says, beyond what the engine loader keeps: the node
//! tree of every LOD root (including empty `fx_*` nodes, which the loader
//! prunes), the materials and the features the engine refuses. The LOD roots
//! are found by the loader's own `lod_roots`, so the checker and the engine
//! cannot disagree on a file that has one scene per level.

use std::collections::BTreeSet;

use sf_render::glb::{LodLayout, lod_roots, split_lod_suffix};

/// Deepest node tree this module follows (the loader refuses deeper ones too).
const MAX_DEPTH: usize = 32;

/// One node, its name without a `_LOD<n>` suffix.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeInfo {
    pub name: String,
    pub has_mesh: bool,
    pub children: Vec<NodeInfo>,
}

impl NodeInfo {
    /// All paths below this node (`hull`, `hull/turret`, ...), depth first.
    fn paths(&self, prefix: &str, out: &mut Vec<String>) {
        for child in &self.children {
            let path = if prefix.is_empty() {
                child.name.clone()
            } else {
                format!("{prefix}/{}", child.name)
            };
            out.push(path.clone());
            child.paths(&path, out);
        }
    }

    /// The first node called `name` below this one, with its parent's name
    /// (`None` = a direct child of the root).
    pub fn find(&self, name: &str) -> Option<(&NodeInfo, Option<&str>)> {
        fn walk<'a>(
            node: &'a NodeInfo,
            parent: Option<&'a str>,
            name: &str,
        ) -> Option<(&'a NodeInfo, Option<&'a str>)> {
            for child in &node.children {
                if child.name == name {
                    return Some((child, parent));
                }
                if let Some(found) = walk(child, Some(&child.name), name) {
                    return Some(found);
                }
            }
            None
        }
        walk(self, None, name)
    }

    /// Every node below this one, depth first.
    pub fn descendants(&self) -> Vec<&NodeInfo> {
        let mut out = Vec::new();
        fn walk<'a>(node: &'a NodeInfo, out: &mut Vec<&'a NodeInfo>) {
            for child in &node.children {
                out.push(child);
                walk(child, out);
            }
        }
        walk(self, &mut out);
        out
    }
}

/// A root of the default scene: one level of detail.
#[derive(Clone, Debug, PartialEq)]
pub struct LodRoot {
    /// The name as written (`veyran_scout_LOD0`).
    pub raw_name: String,
    /// The name without the `_LOD<n>` suffix.
    pub base: String,
    pub lod: Option<u32>,
    /// Translation, rotation (quaternion) and scale of the root node.
    pub transform: ([f32; 3], [f32; 4], [f32; 3]),
    /// The root's children (the root itself is `base`).
    pub tree: NodeInfo,
}

impl LodRoot {
    /// The sorted paths of all nodes below the root: the "node structure".
    pub fn structure(&self) -> Vec<String> {
        let mut paths = Vec::new();
        self.tree.paths("", &mut paths);
        paths.sort();
        paths
    }
}

/// What a material says that matters here.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialInfo {
    pub name: Option<String>,
    /// A non-zero emissive factor, which the engine's colour-only loader ignores.
    pub emissive: bool,
}

/// The inspected document.
#[derive(Clone, Debug, PartialEq)]
pub struct DocInfo {
    /// Whether the LOD roots share the default scene or have a scene each.
    /// Both are accepted and read the same; the brief prefers one scene.
    pub layout: LodLayout,
    pub roots: Vec<LodRoot>,
    pub materials: Vec<MaterialInfo>,
    /// A primitive has no material (the default material would be a seventh).
    pub unmaterialed_primitive: bool,
    pub textures: usize,
    pub images: usize,
}

impl DocInfo {
    /// Whether a material is called `team`.
    pub fn has_team_material(&self) -> bool {
        self.materials
            .iter()
            .any(|m| m.name.as_deref() == Some("team"))
    }
}

/// Read the structure of a `.glb`.
pub fn inspect(bytes: &[u8]) -> Result<DocInfo, String> {
    let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| format!("not a valid glTF: {e}"))?;
    let doc = &gltf.document;
    // The same reading as the engine loader: the default scene, or one scene
    // per LOD named LOD0, LOD1, ... (see `docs/conventions.md` section 12).
    let (layout, scene_roots) = lod_roots(doc).map_err(|e| e.to_string())?;
    let mut roots = Vec::new();
    for node in scene_roots {
        let raw_name = node.name().unwrap_or("").to_owned();
        let (base, lod) = split_lod_suffix(&raw_name);
        let (base, lod) = (base.to_owned(), lod);
        let mut seen = BTreeSet::new();
        let tree = node_info(&node, 0, &mut seen)?;
        roots.push(LodRoot {
            raw_name,
            base,
            lod,
            transform: {
                let (t, r, s) = node.transform().decomposed();
                (t, r, s)
            },
            tree,
        });
    }
    let materials = doc
        .materials()
        .map(|m| MaterialInfo {
            name: m.name().map(str::to_owned),
            emissive: m.emissive_factor().iter().any(|&c| c != 0.0),
        })
        .collect();
    let unmaterialed_primitive = doc
        .meshes()
        .any(|m| m.primitives().any(|p| p.material().index().is_none()));
    Ok(DocInfo {
        layout,
        roots,
        materials,
        unmaterialed_primitive,
        textures: doc.textures().count(),
        images: doc.images().count(),
    })
}

fn node_info(
    node: &gltf::Node<'_>,
    depth: usize,
    seen: &mut BTreeSet<usize>,
) -> Result<NodeInfo, String> {
    if depth > MAX_DEPTH {
        return Err(format!("node tree deeper than {MAX_DEPTH} levels"));
    }
    if !seen.insert(node.index()) {
        return Err(format!(
            "node {} appears twice in one LOD root (a cycle or a shared node)",
            node.index()
        ));
    }
    let raw = node.name().unwrap_or("");
    let mut children = Vec::new();
    for child in node.children() {
        children.push(node_info(&child, depth + 1, seen)?);
    }
    Ok(NodeInfo {
        name: split_lod_suffix(raw).0.to_owned(),
        has_mesh: node.mesh().is_some(),
        children,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, has_mesh: bool, children: Vec<NodeInfo>) -> NodeInfo {
        NodeInfo {
            name: name.into(),
            has_mesh,
            children,
        }
    }

    #[test]
    fn paths_and_find_follow_the_hierarchy() {
        let tree = node(
            "root",
            false,
            vec![
                node("hull", true, vec![node("turret", true, vec![])]),
                node("fx_smoke_1", false, vec![]),
            ],
        );
        let root = LodRoot {
            raw_name: "x_LOD0".into(),
            base: "x".into(),
            lod: Some(0),
            transform: ([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3]),
            tree,
        };
        assert_eq!(
            root.structure(),
            ["fx_smoke_1", "hull", "hull/turret"].map(String::from)
        );
        let (turret, parent) = root.tree.find("turret").unwrap();
        assert!(turret.has_mesh);
        assert_eq!(parent, Some("hull"));
        let (_, parent) = root.tree.find("hull").unwrap();
        assert_eq!(parent, None);
        assert!(root.tree.find("barrel").is_none());
        assert_eq!(root.tree.descendants().len(), 3);
    }

    #[test]
    fn garbage_is_not_a_glb() {
        assert!(inspect(b"not a model").is_err());
        assert!(inspect(&[]).is_err());
    }
}
