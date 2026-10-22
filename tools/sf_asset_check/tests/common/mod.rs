//! Tiny `.glb` fixtures for the rule tests, built with the engine's own test
//! writer (`sf_render::glb::writer`). A fixture is a model with three LODs;
//! each LOD is a tree of parts; a part is a node with boxes of one material.
//! A box is 12 triangles; `reps` repeats a part's boxes to reach a count.

#![allow(dead_code)]

use sf_render::glb::writer::{self, Node, Prim, Scene};

pub type V3 = [f32; 3];

#[derive(Clone, Debug)]
pub struct Part {
    pub name: String,
    pub t: V3,
    pub mat: usize,
    pub boxes: Vec<(V3, V3)>,
    pub reps: usize,
    /// Write zero-length normals (as a generator does for flat triangles).
    pub zero_normals: bool,
    pub children: Vec<Part>,
}

impl Part {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            t: [0.0; 3],
            mat: 0,
            boxes: Vec::new(),
            reps: 1,
            zero_normals: false,
            children: Vec::new(),
        }
    }

    pub fn zero_normals(mut self) -> Self {
        self.zero_normals = true;
        self
    }

    pub fn at(mut self, t: V3) -> Self {
        self.t = t;
        self
    }

    pub fn mat(mut self, mat: usize) -> Self {
        self.mat = mat;
        self
    }

    pub fn boxed(mut self, lo: V3, hi: V3) -> Self {
        self.boxes.push((lo, hi));
        self
    }

    pub fn reps(mut self, n: usize) -> Self {
        self.reps = n;
        self
    }

    pub fn child(mut self, c: Part) -> Self {
        self.children.push(c);
        self
    }

    fn find_mut(&mut self, name: &str) -> Option<&mut Part> {
        if self.name == name {
            return Some(self);
        }
        self.children.iter_mut().find_map(|c| c.find_mut(name))
    }

    fn remove(&mut self, name: &str) -> bool {
        let before = self.children.len();
        self.children.retain(|c| c.name != name);
        self.children.len() != before || self.children.iter_mut().any(|c| c.remove(name))
    }
}

#[derive(Clone, Debug, Default)]
pub struct Lod {
    pub origin: V3,
    pub parts: Vec<Part>,
}

#[derive(Clone, Debug)]
pub struct Model {
    pub stem: String,
    pub materials: Vec<(String, [f32; 3])>,
    pub lods: Vec<Lod>,
    pub extra_json: String,
}

impl Model {
    /// The `.glb` bytes.
    pub fn bytes(&self) -> Vec<u8> {
        let mut nodes = Vec::new();
        let mut roots = Vec::new();
        for (n, lod) in self.lods.iter().enumerate() {
            let at = nodes.len();
            nodes.push(Node {
                name: format!("{}_LOD{n}", self.stem),
                translation: lod.origin,
                ..Node::default()
            });
            for part in &lod.parts {
                let child = push_part(&mut nodes, part);
                nodes[at].children.push(child);
            }
            roots.push(at);
        }
        writer::write(&Scene {
            materials: self.materials.clone(),
            nodes,
            roots,
            version: "2.0".into(),
            extra_json: self.extra_json.clone(),
        })
    }

    pub fn part_mut(&mut self, lod: usize, name: &str) -> &mut Part {
        self.lods[lod]
            .parts
            .iter_mut()
            .find_map(|p| p.find_mut(name))
            .unwrap_or_else(|| panic!("no part {name} in LOD{lod}"))
    }

    pub fn remove_part(&mut self, lod: usize, name: &str) {
        let parts = &mut self.lods[lod].parts;
        let before = parts.len();
        parts.retain(|p| p.name != name);
        if parts.len() == before {
            assert!(
                parts.iter_mut().any(|p| p.remove(name)),
                "no part {name} in LOD{lod}"
            );
        }
    }

    /// Add a part at the top level of every LOD.
    pub fn add_everywhere(&mut self, part: &Part) {
        for lod in &mut self.lods {
            lod.parts.push(part.clone());
        }
    }

    pub fn with_stem(mut self, stem: &str) -> Self {
        self.stem = stem.into();
        self
    }
}

fn push_part(nodes: &mut Vec<Node>, part: &Part) -> usize {
    let at = nodes.len();
    nodes.push(Node {
        name: part.name.clone(),
        translation: part.t,
        ..Node::default()
    });
    if !part.boxes.is_empty() {
        let mut prim = Prim {
            material: part.mat,
            ..Prim::default()
        };
        for _ in 0..part.reps.max(1) {
            for (lo, hi) in &part.boxes {
                for face in writer::box_faces(*lo, *hi, [part.mat; 6]) {
                    let base = prim.positions.len() as u32;
                    prim.positions.extend(face.positions);
                    prim.normals.extend(face.normals);
                    prim.indices.extend(face.indices.iter().map(|i| i + base));
                }
            }
        }
        if part.zero_normals {
            prim.normals.iter_mut().for_each(|n| *n = [0.0; 3]);
        }
        nodes[at].prim = Some(prim);
    }
    for child in &part.children {
        let c = push_part(nodes, child);
        nodes[at].children.push(c);
    }
    at
}

/// Grey cloth (material 0) and the team colour (material 1).
pub fn cloth_and_team() -> Vec<(String, [f32; 3])> {
    vec![("cloth".into(), [0.3; 3]), ("team".into(), [0.9; 3])]
}

/// A figure 0.6 x 1.75 x 0.4 m standing on the ground, front +X, with
/// `share` of its top-down view in the `team` material (a slab on the head).
/// LOD0 and LOD1 have 72 triangles, LOD2 24.
pub fn infantry(stem: &str, share: f32) -> Model {
    let torso = || Part::new("torso").boxed([-0.3, 0.0, -0.2], [0.3, 1.7, 0.2]);
    let head = Part::new("head")
        .mat(1)
        .boxed([-0.3, 1.7, -0.2], [-0.3 + 0.6 * share, 1.75, 0.2]);
    let limb = |name: &str, x: f32, z: f32| {
        Part::new(name).boxed([x - 0.05, 0.0, z - 0.05], [x + 0.05, 0.5, z + 0.05])
    };
    let full = vec![
        torso(),
        head.clone(),
        limb("arm_l", 0.0, -0.1),
        limb("arm_r", 0.0, 0.1),
        limb("leg_l", 0.1, -0.1),
        limb("leg_r", 0.1, 0.1),
    ];
    let simple = vec![
        torso(),
        head,
        Part::new("arm_l"),
        Part::new("arm_r"),
        Part::new("leg_l"),
        Part::new("leg_r"),
    ];
    Model {
        stem: stem.into(),
        materials: cloth_and_team(),
        lods: vec![
            Lod {
                origin: [0.0; 3],
                parts: full.clone(),
            },
            Lod {
                origin: [0.0; 3],
                parts: full,
            },
            Lod {
                origin: [0.0; 3],
                parts: simple,
            },
        ],
        extra_json: String::new(),
    }
}

/// A neutral wreck 4 x 1 x 2 m: `hull` -> `turret` -> `barrel`, an empty
/// `fx_smoke_1` and `fx_fire_1`. One material, no team.
pub fn wreck(stem: &str) -> Model {
    let parts = || {
        vec![
            Part::new("hull")
                .boxed([-2.0, 0.0, -1.0], [2.0, 1.0, 1.0])
                .child(
                    Part::new("turret")
                        .at([0.0, 1.0, 0.0])
                        .boxed([-0.5, 0.0, -0.5], [0.5, 0.3, 0.5])
                        .child(
                            // An empty node with the mesh below it, so the
                            // engine loader keeps the barrel as a part.
                            Part::new("barrel").at([0.5, 0.15, 0.0]).child(
                                Part::new("barrel_geo")
                                    .boxed([0.0, -0.05, -0.05], [1.0, 0.05, 0.05]),
                            ),
                        ),
                ),
            Part::new("fx_smoke_1").at([0.0, 1.5, 0.0]),
            Part::new("fx_fire_1").at([0.0, 0.8, 0.0]),
        ]
    };
    Model {
        stem: stem.into(),
        materials: vec![("burnt".into(), [0.2; 3])],
        lods: (0..3)
            .map(|_| Lod {
                origin: [0.0; 3],
                parts: parts(),
            })
            .collect(),
        extra_json: String::new(),
    }
}

/// A neutral tree-like decor object 2 x 6 x 2 m with one material.
pub fn decor(stem: &str) -> Model {
    let part = || Part::new("trunk").boxed([-1.0, 0.0, -1.0], [1.0, 6.0, 1.0]);
    Model {
        stem: stem.into(),
        materials: vec![("bark".into(), [0.3; 3])],
        lods: (0..3)
            .map(|_| Lod {
                origin: [0.0; 3],
                parts: vec![part()],
            })
            .collect(),
        extra_json: String::new(),
    }
}

/// A faction light tank 4 x 1 x 2 m: hull, empty turret and barrel nodes
/// under it, and a team plate on the hull covering `share` of the top view.
pub fn tank(stem: &str, share: f32) -> Model {
    let parts = || {
        vec![
            Part::new("hull")
                .boxed([-2.0, 0.0, -1.0], [2.0, 1.0, 1.0])
                .child(Part::new("turret").child(Part::new("barrel"))),
            Part::new("plate")
                .mat(1)
                .boxed([-2.0, 1.0, -1.0], [-2.0 + 4.0 * share, 1.05, 1.0]),
        ]
    };
    Model {
        stem: stem.into(),
        materials: cloth_and_team(),
        lods: (0..3)
            .map(|_| Lod {
                origin: [0.0; 3],
                parts: parts(),
            })
            .collect(),
        extra_json: String::new(),
    }
}

/// The same `.glb` with its JSON chunk rewritten by `f` (to add what the
/// writer cannot: an emissive factor, a texture).
pub fn patch_json(glb: &[u8], f: impl FnOnce(String) -> String) -> Vec<u8> {
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let json = String::from_utf8(glb[20..20 + json_len].to_vec()).unwrap();
    let mut json = f(json).into_bytes();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let rest = &glb[20 + json_len..];
    let total = 12 + 8 + json.len() + rest.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(rest);
    out
}

/// The same `.glb` with one scene per LOD root, named `LOD0`, `LOD1`, ...,
/// the way the exporter of art set v4 writes it (the nodes stay as they are).
pub fn scene_per_lod(glb: &[u8]) -> Vec<u8> {
    patch_json(glb, |json| {
        let head = "\"scenes\":[{\"nodes\":[";
        let start = json.find(head).expect("the writer's single scene");
        let from = start + head.len();
        let end = from + json[from..].find("]}]").expect("end of the scene");
        let scenes: Vec<String> = json[from..end]
            .split(',')
            .enumerate()
            .map(|(n, root)| format!("{{\"name\":\"LOD{n}\",\"nodes\":[{root}]}}"))
            .collect();
        format!(
            "{}\"scenes\":[{}]{}",
            &json[..start],
            scenes.join(","),
            &json[end + 3..]
        )
    })
}

/// A second figure in the same file: a `_2` copy of the six figure nodes in
/// every LOD.
pub fn add_second_figure(m: &mut Model) {
    for lod in &mut m.lods {
        let copies: Vec<Part> = lod
            .parts
            .iter()
            .filter(|p| {
                ["torso", "head", "arm_l", "arm_r", "leg_l", "leg_r"].contains(&p.name.as_str())
            })
            .map(|p| Part {
                name: format!("{}_2", p.name),
                ..p.clone()
            })
            .collect();
        lod.parts.extend(copies);
    }
}
