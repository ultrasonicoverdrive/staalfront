//! The loader's `COLOR_0` rule (WP11.6a) on hand-built files: every vertex colour is exactly the
//! material's base colour factor times `COLOR_0` (both linear, alpha 0), for a float `VEC3` and a
//! normalised `u8` `VEC4` accessor; the `team` material ignores `COLOR_0`; a primitive without it
//! keeps the factor; a `COLOR_0` of another length than `POSITION` is refused.

use sf_render::glb::writer::{self, Colors, Node, Prim, Scene};
use sf_render::glb::{AxisConvention, GlbError, LoadedModel, load_glb};
use sf_render::mesh::Vertex;

const STEEL: [f32; 3] = [0.30, 0.45, 0.60];
const TEAM_FACTOR: [f32; 3] = [0.9, 0.1, 0.2];
const F32_SHADE: [[f32; 3]; 4] = [
    [1.0, 1.0, 1.0],
    [0.5, 0.25, 0.75],
    [0.1, 0.9, 0.333],
    [0.0, 0.62, 1.0],
];
const U8_SHADE: [[u8; 4]; 4] = [
    [255, 255, 255, 255],
    [128, 64, 192, 7],
    [1, 254, 85, 0],
    [0, 77, 200, 255],
];

fn quad(material: usize, colors: Option<Colors>) -> Prim {
    let mut p = writer::box_faces([0.0; 3], [1.0; 3], [material; 6])
        .into_iter()
        .next()
        .unwrap();
    p.colors = colors;
    p
}

fn scene(prims: Vec<(&str, Prim)>) -> Scene {
    let mut nodes = vec![Node {
        name: "m_LOD0".into(),
        ..Node::default()
    }];
    // A node without a mesh is a part; its mesh leaf merges into it.
    for (name, prim) in prims {
        let at = nodes.len();
        nodes[0].children.push(at);
        nodes.push(Node {
            name: format!("{name}_LOD0"),
            children: vec![at + 1],
            ..Node::default()
        });
        nodes.push(Node {
            name: format!("{name}_mesh_LOD0"),
            prim: Some(prim),
            ..Node::default()
        });
    }
    Scene {
        materials: vec![("steel".into(), STEEL), ("team".into(), TEAM_FACTOR)],
        nodes,
        roots: vec![0],
        version: "2.0".into(),
        extra_json: String::new(),
    }
}

fn load(s: &Scene) -> Result<LoadedModel, GlbError> {
    load_glb(&writer::write(s), &AxisConvention::ENGINE)
}

fn colours(m: &LoadedModel, part: &str) -> Vec<[f32; 4]> {
    let p = m.levels[0]
        .parts
        .iter()
        .find(|p| p.name == part)
        .unwrap_or_else(|| panic!("no part {part}"));
    p.mesh.vertices.iter().map(|v| v.color).collect()
}

#[test]
fn every_vertex_colour_is_the_factor_times_color_0() {
    let m = load(&scene(vec![
        ("float", quad(0, Some(Colors::F32(F32_SHADE.to_vec())))),
        ("byte", quad(0, Some(Colors::U8(U8_SHADE.to_vec())))),
        ("team", quad(1, Some(Colors::F32(F32_SHADE.to_vec())))),
        ("plain", quad(0, None)),
    ]))
    .unwrap();

    let float = colours(&m, "float");
    assert_eq!(float.len(), 4);
    for (got, s) in float.iter().zip(F32_SHADE) {
        let want = [STEEL[0] * s[0], STEEL[1] * s[1], STEEL[2] * s[2], 0.0];
        assert_eq!(*got, want);
    }

    let byte = colours(&m, "byte");
    assert_eq!(byte.len(), 4);
    for (got, s) in byte.iter().zip(U8_SHADE) {
        // The glTF crate normalises a u8 as `v * (1 / 255)`.
        let n = |k: usize| s[k] as f32 * 255.0_f32.recip();
        let want = [STEEL[0] * n(0), STEEL[1] * n(1), STEEL[2] * n(2), 0.0];
        assert_eq!(*got, want, "alpha of COLOR_0 is dropped");
    }

    let team = colours(&m, "team");
    assert_eq!(team.len(), 4);
    assert!(team.iter().all(|c| *c == Vertex::TEAM), "{team:?}");

    let plain = colours(&m, "plain");
    assert_eq!(plain.len(), 4);
    assert!(
        plain
            .iter()
            .all(|c| *c == [STEEL[0], STEEL[1], STEEL[2], 0.0])
    );
}

#[test]
fn a_color_0_of_another_length_than_position_is_invalid() {
    let short = F32_SHADE[..3].to_vec();
    match load(&scene(vec![("float", quad(0, Some(Colors::F32(short))))])) {
        Err(GlbError::Invalid(e)) => assert!(e.contains("COLOR_0"), "{e}"),
        other => panic!("expected Invalid, got {other:?}"),
    }
}
