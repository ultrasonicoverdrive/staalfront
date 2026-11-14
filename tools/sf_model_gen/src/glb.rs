//! The GLB writer: one scene whose roots are the levels `<stem>_LOD0..2` (layout 1 of
//! docs/conventions.md §12), a node per part or anchor with a translation in metres, one mesh per
//! part with one primitive per material, flat triangles (three corners each) with `POSITION`,
//! `NORMAL` and `COLOR_0` (the baked shade, [`crate::shade`]), `u16` or `u32` indices, and
//! base-colour-only materials. No textures, no images, no samplers (the loader refuses them).

use std::fmt::Write as _;

use crate::kit::{Mat, Model, Node, Tri};
use crate::shade::{bake, unit_normal};

/// sRGB 0..=255 to the linear factor glTF stores (a cubic fit of the sRGB curve, basic operations
/// only, so the same bytes everywhere; within 0.4 % of the exact curve).
pub fn linear(c: u8) -> f32 {
    let s = f32::from(c) / 255.0;
    s * (s * (s * 0.305_306 + 0.682_171) + 0.012_523)
}

struct Prim {
    mesh_node: usize,
    material: Mat,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 3]>,
}

struct Flat {
    name: String,
    at: [i32; 3],
    children: Vec<usize>,
    mesh: Option<usize>,
}

fn metres(mm: i32) -> f32 {
    mm as f32 / 1000.0
}

/// Flatten a level in preorder; returns the index of `node`.
fn flatten(node: &Node, nodes: &mut Vec<Flat>, meshes: &mut Vec<Vec<Tri>>) -> usize {
    let at = nodes.len();
    nodes.push(Flat {
        name: node.name.clone(),
        at: node.at,
        children: Vec::new(),
        mesh: None,
    });
    if !node.mesh.is_empty() {
        nodes[at].mesh = Some(meshes.len());
        meshes.push(node.mesh.tris.clone());
    }
    for c in &node.children {
        let i = flatten(c, nodes, meshes);
        nodes[at].children.push(i);
    }
    at
}

/// The baked shade of every node's triangles of one level, keyed like the flattened meshes.
fn shades(level: &Node) -> Vec<Vec<[f32; 3]>> {
    // Bake on the whole level in model space (parts occlude each other), then hand each node
    // its slice, in the same preorder as `flatten`.
    let world = level.world_tris();
    let baked = bake(&world);
    let mut out = Vec::new();
    let mut at = 0;
    fn walk(n: &Node, baked: &[[f32; 3]], at: &mut usize, out: &mut Vec<Vec<[f32; 3]>>) {
        if !n.mesh.is_empty() {
            let len = n.mesh.len() * 3;
            out.push(baked[*at..*at + len].to_vec());
            *at += len;
        }
        for c in &n.children {
            walk(c, baked, at, out);
        }
    }
    walk(level, &baked, &mut at, &mut out);
    out
}

/// Write the model as GLB bytes.
pub fn write(model: &Model) -> Vec<u8> {
    let mut nodes = Vec::new();
    let mut meshes = Vec::new();
    let mut shade = Vec::new();
    let mut roots = Vec::new();
    for level in &model.lods {
        roots.push(flatten(level, &mut nodes, &mut meshes));
        shade.extend(shades(level));
    }
    // Primitives: per mesh, per material in material order.
    let mut prims: Vec<Prim> = Vec::new();
    for (mi, tris) in meshes.iter().enumerate() {
        let mut mats: Vec<Mat> = tris.iter().map(|t| t.mat).collect();
        mats.sort();
        mats.dedup();
        for mat in mats {
            let mut p = Prim {
                mesh_node: mi,
                material: mat,
                positions: Vec::new(),
                normals: Vec::new(),
                colors: Vec::new(),
            };
            for (ti, t) in tris.iter().enumerate().filter(|(_, t)| t.mat == mat) {
                let n = unit_normal(t);
                for (c, corner) in t.v.iter().enumerate() {
                    p.positions.push(corner.map(metres));
                    p.normals.push(n);
                    p.colors.push(shade[mi][ti * 3 + c]);
                }
            }
            prims.push(p);
        }
    }
    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut view = |bin: &mut Vec<u8>, bytes: &[u8], target: u32| -> usize {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        views.push(format!(
            "{{\"buffer\":0,\"byteOffset\":{},\"byteLength\":{},\"target\":{target}}}",
            bin.len(),
            bytes.len()
        ));
        bin.extend_from_slice(bytes);
        views.len() - 1
    };
    let mut mesh_prims: Vec<Vec<String>> = vec![Vec::new(); meshes.len()];
    for p in &prims {
        let count = p.positions.len();
        let mut attrs = Vec::new();
        for (data, name) in [
            (&p.positions, "POSITION"),
            (&p.normals, "NORMAL"),
            (&p.colors, "COLOR_0"),
        ] {
            let bytes: Vec<u8> = data
                .iter()
                .flat_map(|v| v.iter().flat_map(|f| f.to_le_bytes()))
                .collect();
            let bv = view(&mut bin, &bytes, 34962);
            let bounds = if name == "POSITION" {
                let lo = (0..3).map(|i| data.iter().map(|v| v[i]).fold(f32::INFINITY, f32::min));
                let hi =
                    (0..3).map(|i| data.iter().map(|v| v[i]).fold(f32::NEG_INFINITY, f32::max));
                format!(",\"min\":[{}],\"max\":[{}]", list(lo), list(hi))
            } else {
                String::new()
            };
            accessors.push(format!(
                "{{\"bufferView\":{bv},\"componentType\":5126,\"count\":{count},\"type\":\"VEC3\"{bounds}}}"
            ));
            attrs.push(format!("\"{name}\":{}", accessors.len() - 1));
        }
        let (bytes, kind): (Vec<u8>, u32) = if count <= usize::from(u16::MAX) {
            ((0..count as u16).flat_map(u16::to_le_bytes).collect(), 5123)
        } else {
            ((0..count as u32).flat_map(u32::to_le_bytes).collect(), 5125)
        };
        let bv = view(&mut bin, &bytes, 34963);
        accessors.push(format!(
            "{{\"bufferView\":{bv},\"componentType\":{kind},\"count\":{count},\"type\":\"SCALAR\"}}"
        ));
        mesh_prims[p.mesh_node].push(format!(
            "{{\"attributes\":{{{}}},\"indices\":{},\"material\":{}}}",
            attrs.join(","),
            accessors.len() - 1,
            p.material.0
        ));
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let node_json: Vec<String> = nodes
        .iter()
        .map(|n| {
            let mut s = format!("{{\"name\":\"{}\"", n.name);
            if n.at != [0, 0, 0] {
                let _ = write!(s, ",\"translation\":[{}]", list(n.at.map(metres)));
            }
            if let Some(m) = n.mesh {
                let _ = write!(s, ",\"mesh\":{m}");
            }
            if !n.children.is_empty() {
                let _ = write!(s, ",\"children\":[{}]", list(n.children.iter()));
            }
            s.push('}');
            s
        })
        .collect();
    let mesh_json: Vec<String> = mesh_prims
        .iter()
        .map(|p| format!("{{\"primitives\":[{}]}}", p.join(",")))
        .collect();
    let mat_json: Vec<String> = model
        .materials
        .iter()
        .map(|m| {
            format!(
                "{{\"name\":\"{}\",\"pbrMetallicRoughness\":{{\"baseColorFactor\":[{},1],\"metallicFactor\":0,\"roughnessFactor\":1}}}}",
                m.name,
                list(m.srgb.map(linear))
            )
        })
        .collect();
    let json = format!(
        "{{\"asset\":{{\"version\":\"2.0\",\"generator\":\"sf_model_gen\"}},\"scene\":0,\"scenes\":[{{\"nodes\":[{}]}}],\"nodes\":[{}],\"meshes\":[{}],\"materials\":[{}],\"accessors\":[{}],\"bufferViews\":[{}],\"buffers\":[{{\"byteLength\":{}}}]}}",
        list(roots.iter()),
        node_json.join(","),
        mesh_json.join(","),
        mat_json.join(","),
        accessors.join(","),
        views.join(","),
        bin.len()
    );
    let mut json = json.into_bytes();
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

fn list<T: std::fmt::Display>(v: impl IntoIterator<Item = T>) -> String {
    v.into_iter()
        .map(|x| x.to_string())
        .collect::<Vec<_>>()
        .join(",")
}
