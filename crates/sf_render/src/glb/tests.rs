//! Loader tests (WP1.10): suffixes, axis conversion and orientation, the
//! per-part merge, the team colour, the part chain and clear errors.

use std::path::PathBuf;

use glam::{Mat4, Vec3};

use super::writer::{self, DARK, FRONT_RED, RIGHT_BLUE, Scene, TOP_GREEN};
use super::*;
use crate::model::{BUILTIN_MESHES, Joint, MAX_PARTS, PartAngles, pose_parts};

fn pack_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/testpack_v1/models")
}

fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-5
}

#[test]
fn lod_suffixes_are_stripped() {
    assert_eq!(split_lod_suffix("hull_LOD0"), ("hull", Some(0)));
    assert_eq!(
        split_lod_suffix("gevechtstank_LOD12"),
        ("gevechtstank", Some(12))
    );
    assert_eq!(strip_lod_suffix("hull_steel_LOD2"), "hull_steel");
    // Not a suffix: kept as is.
    for name in [
        "hull",
        "hull_LOD",
        "hull_LODx",
        "LOD0",
        "hull_lod0",
        "a_LOD0b",
    ] {
        assert_eq!(strip_lod_suffix(name), name, "{name}");
    }
}

#[test]
fn the_conversion_is_a_rotation_with_the_source_front_on_plus_x() {
    // The engine's own axes: identity.
    assert_eq!(AxisConvention::ENGINE.matrix(), Some(Mat4::IDENTITY));
    // glTF spec axes: front +Z -> +X, up stays, right (-X in glTF) -> +Z.
    let c = AxisConvention::GLTF.rotation().unwrap();
    assert!(close(c * Vec3::Z, Vec3::X));
    assert!(close(c * Vec3::Y, Vec3::Y));
    assert!(close(c * Vec3::NEG_X, Vec3::Z));
    assert!((c.determinant() - 1.0).abs() < 1e-6, "no mirror");
    // Every perpendicular front/up pair gives a proper rotation that maps
    // front to +X, up to +Y and front x up (the right side) to +Z.
    let axes = [
        Axis::PlusX,
        Axis::MinusX,
        Axis::PlusY,
        Axis::MinusY,
        Axis::PlusZ,
        Axis::MinusZ,
    ];
    let mut valid = 0;
    for front in axes {
        for up in axes {
            let conv = AxisConvention {
                front,
                up,
                metres_per_unit: 0.01,
            };
            let Some(c) = conv.rotation() else {
                assert_eq!(front.vector().dot(up.vector()).abs(), 1.0);
                continue;
            };
            valid += 1;
            let (f, u) = (front.vector(), up.vector());
            assert!(close(c * f, Vec3::X));
            assert!(close(c * u, Vec3::Y));
            assert!(close(c * f.cross(u), Vec3::Z));
            assert!((c.determinant() - 1.0).abs() < 1e-6);
            // Centimetres: 100 source units are one metre.
            let m = conv.matrix().unwrap();
            assert!(close(m.transform_point3(f * 100.0), Vec3::X));
        }
    }
    assert_eq!(valid, 24);
    let bad = AxisConvention {
        metres_per_unit: 0.0,
        ..AxisConvention::ENGINE
    };
    assert!(bad.matrix().is_none());
}

/// The material colour (rgb) of the faces whose normal points along `dir`.
fn face_colors(mesh: &MeshData, dir: Vec3) -> Vec<[f32; 4]> {
    mesh.vertices
        .iter()
        .filter(|v| Vec3::from(v.normal).dot(dir) > 0.99)
        .map(|v| v.color)
        .collect()
}

fn rgb0(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 0.0]
}

/// Load the orientation model written in `axes` and check that front, top
/// and right come out at +X, +Y and +Z, the turret on top and the barrel
/// pointing forward, whatever axes it was authored in.
fn check_orientation(axes: AxisConvention) {
    let bytes = writer::write(&writer::orientation(&axes));
    let model = load_glb(&bytes, &axes).expect("orientation model loads");
    assert_eq!(model.name, "orient");
    assert_eq!(model.levels.len(), 1);
    let parts = &model.levels[0].parts;
    let names: Vec<&str> = parts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["hull", "turret", "barrel"], "{axes:?}");
    let hull = &parts[0].mesh;
    // Front (+X) red, top (+Y) green, right (+Z) blue.
    for (dir, color) in [
        (Vec3::X, FRONT_RED),
        (Vec3::Y, TOP_GREEN),
        (Vec3::Z, RIGHT_BLUE),
    ] {
        let found = face_colors(hull, dir);
        assert_eq!(found.len(), 4, "{axes:?}: one face along {dir}");
        assert!(found.iter().all(|c| *c == rgb0(color)), "{axes:?} {dir}");
    }
    // The hull box in metres, standing on the ground, long along +X.
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for v in &hull.vertices {
        lo = lo.min(Vec3::from(v.position));
        hi = hi.max(Vec3::from(v.position));
    }
    assert!(close(lo, Vec3::new(-1.0, 0.0, -0.5)), "{axes:?}: {lo}");
    assert!(close(hi, Vec3::new(1.0, 1.0, 0.5)), "{axes:?}: {hi}");
    // Winding: every triangle's face normal agrees with the stored normal.
    for part in parts {
        let m = &part.mesh;
        for tri in m.indices.chunks(3) {
            let p = |i: u32| Vec3::from(m.vertices[i as usize].position);
            let face = (p(tri[1]) - p(tri[0])).cross(p(tri[2]) - p(tri[0]));
            let n = Vec3::from(m.vertices[tri[0] as usize].normal);
            assert!(face.normalize().dot(n) > 0.99, "{axes:?}: winding");
        }
    }
    // Node translations converted: turret on top, barrel forward of it.
    assert!(close(
        parts[1].rest.w_axis.truncate(),
        Vec3::new(0.0, 1.0, 0.0)
    ));
    assert!(close(
        parts[2].rest.w_axis.truncate(),
        Vec3::new(0.3, 0.2, 0.0)
    ));
    assert!(close(Mat3::from_mat4(parts[1].rest).col(0), Vec3::X));
    // The barrel's far end is along +X of the turret.
    let tip = parts[2]
        .mesh
        .vertices
        .iter()
        .map(|v| v.position[0])
        .fold(f32::MIN, f32::max);
    assert!((tip - 0.8).abs() < 1e-5);
}

#[test]
fn orientation_survives_the_conversion_from_any_source_axes() {
    check_orientation(AxisConvention::ENGINE);
    check_orientation(AxisConvention::GLTF);
    check_orientation(AxisConvention {
        front: Axis::MinusZ,
        up: Axis::PlusY,
        metres_per_unit: 1.0,
    });
    // Z-up, front -Y, in centimetres.
    check_orientation(AxisConvention {
        front: Axis::MinusY,
        up: Axis::PlusZ,
        metres_per_unit: 0.01,
    });
}

/// Loading glTF-axes content as if it were engine axes gets it wrong: the
/// conversion is what puts the front at +X, not a coincidence of the model.
#[test]
fn the_wrong_convention_puts_the_front_elsewhere() {
    let bytes = writer::write(&writer::orientation(&AxisConvention::GLTF));
    let model = load_glb(&bytes, &AxisConvention::ENGINE).unwrap();
    let hull = &model.levels[0].parts[0].mesh;
    assert!(
        face_colors(hull, Vec3::X)
            .iter()
            .all(|c| *c != rgb0(FRONT_RED))
    );
    assert!(
        face_colors(hull, Vec3::Z)
            .iter()
            .all(|c| *c == rgb0(FRONT_RED))
    );
}

#[test]
fn every_test_pack_model_merges_to_one_mesh_per_part_per_level() {
    let mut files: Vec<PathBuf> = std::fs::read_dir(pack_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "glb"))
        .collect();
    files.sort();
    assert_eq!(files.len(), 14);
    let mut parts_total = 0;
    for path in &files {
        let model = load_glb_file(path, &AxisConvention::ENGINE)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let stem = path.file_stem().unwrap().to_str().unwrap();
        assert_eq!(model.name, stem);
        assert_eq!(model.levels.len(), 3, "{stem}");
        let mut triangles = Vec::new();
        for level in &model.levels {
            // hull -> turret -> barrel; parts without geometry (the empty
            // turret and barrel nodes of the scout, the trucks and the
            // buildings) are dropped. Every part left has triangles.
            let names: Vec<&str> = level.parts.iter().map(|p| p.name.as_str()).collect();
            let chain = ["hull", "turret", "barrel"];
            assert_eq!(names, chain[..names.len()], "{stem}");
            let joints = [Joint::Fixed, Joint::Yaw, Joint::Pitch];
            for (i, part) in level.parts.iter().enumerate() {
                assert_eq!(part.parent, i.checked_sub(1), "{stem}");
                assert_eq!(part.joint, joints[i], "{stem}");
                assert!(part.mesh.triangle_count() > 0, "{stem} {}", part.name);
            }
            parts_total += level.parts.len();
            triangles.push(
                level
                    .parts
                    .iter()
                    .map(|p| p.mesh.triangle_count())
                    .sum::<u32>(),
            );
            // Ground at Y = 0: nothing below it.
            let hull = &level.parts[0].mesh;
            assert!(hull.vertices.iter().all(|v| v.position[1] >= -1e-4));
        }
        // The pack's manifest: 48..156 triangles per level, finest first.
        assert!(
            triangles.windows(2).all(|w| w[0] >= w[1]),
            "{stem}: {triangles:?}"
        );
        assert!(triangles.iter().all(|t| (48..=156).contains(t)), "{stem}");
    }
    // 7 models with hull, turret and barrel, 7 with a hull only; the rocket
    // truck's coarsest level has no barrel: 7*9 + 7*3 - 1.
    assert_eq!(parts_total, 83);
}

#[test]
fn the_team_material_takes_the_player_colour_and_the_others_keep_theirs() {
    let model = load_glb_file(
        &pack_dir().join("gevechtstank.glb"),
        &AxisConvention::ENGINE,
    )
    .unwrap();
    let level = &model.levels[0];
    let hull = &level.parts[0].mesh;
    let team = hull
        .vertices
        .iter()
        .filter(|v| v.color == Vertex::TEAM)
        .count();
    let own = hull.vertices.iter().filter(|v| v.color[3] == 0.0).count();
    assert!(team > 0, "the hull has team-coloured faces");
    assert!(own > 0, "and material-coloured ones");
    assert_eq!(team + own, hull.vertices.len());
    // `steel` keeps its base colour factor (0.29, 0.34, 0.38).
    assert!(
        hull.vertices
            .iter()
            .any(|v| v.color == [0.29, 0.34, 0.38, 0.0])
    );
    // The barrel is all `dark`.
    let barrel = &level.parts[2].mesh;
    assert!(
        barrel
            .vertices
            .iter()
            .all(|v| v.color == [0.09, 0.12, 0.15, 0.0])
    );
    // The orientation model's turret is all team colour.
    let bytes = writer::write(&writer::orientation(&AxisConvention::ENGINE));
    let m = load_glb(&bytes, &AxisConvention::ENGINE).unwrap();
    let parts = &m.levels[0].parts;
    assert!(
        parts[1]
            .mesh
            .vertices
            .iter()
            .all(|v| v.color == Vertex::TEAM)
    );
    assert!(parts[2].mesh.vertices.iter().all(|v| v.color == rgb0(DARK)));
}

#[test]
fn the_turret_and_barrel_transforms_chain() {
    let bytes = writer::write(&writer::orientation(&AxisConvention::ENGINE));
    let model = load_glb(&bytes, &AxisConvention::ENGINE).unwrap();
    let set = model_set(std::slice::from_ref(&model), &[]).unwrap();
    let parts = set.model(0).parts(0);
    assert_eq!(parts.len(), 3);
    let mut world = [Mat4::IDENTITY; MAX_PARTS];
    let tip = Vec3::new(0.8, 0.0, 0.0); // barrel end in the barrel's frame
    let at = |world: &[Mat4; MAX_PARTS]| world[2].transform_point3(tip);
    // Rest pose: turret pivot (0, 1, 0), barrel pivot 0.3 forward and 0.2 up.
    pose_parts(Mat4::IDENTITY, parts, PartAngles::default(), &mut world);
    assert!(
        close(at(&world), Vec3::new(1.1, 1.2, 0.0)),
        "{}",
        at(&world)
    );
    // Turret yaw a quarter turn: the barrel swings from +X to +Z (the same
    // sense as the unit facing), the hull does not move.
    let quarter = std::f32::consts::FRAC_PI_2;
    let yaw = PartAngles {
        yaw: quarter,
        pitch: 0.0,
    };
    pose_parts(Mat4::IDENTITY, parts, yaw, &mut world);
    assert!(
        close(at(&world), Vec3::new(0.0, 1.2, 1.1)),
        "{}",
        at(&world)
    );
    assert_eq!(world[0], Mat4::IDENTITY);
    // Barrel pitch lifts the muzzle about the barrel pivot, turret stays.
    let pitch = PartAngles {
        yaw: 0.0,
        pitch: quarter,
    };
    pose_parts(Mat4::IDENTITY, parts, pitch, &mut world);
    assert!(
        close(at(&world), Vec3::new(0.3, 2.0, 0.0)),
        "{}",
        at(&world)
    );
    assert!(close(world[1].w_axis.truncate(), Vec3::new(0.0, 1.0, 0.0)));
    // The unit's own matrix applies to all parts: facing a quarter turn
    // (towards +Z) at (10, 0, 20).
    let unit = Mat4::from_cols_array_2d(&crate::instancing::model_matrix(
        10.0, 0.0, 20.0, 64.0, [1.0; 3],
    ));
    pose_parts(unit, parts, PartAngles::default(), &mut world);
    assert!(
        close(at(&world), Vec3::new(10.0, 1.2, 21.1)),
        "{}",
        at(&world)
    );
    // Turret yaw and facing add up: facing +Z, yaw a quarter more -> -X.
    pose_parts(unit, parts, yaw, &mut world);
    assert!(
        close(at(&world), Vec3::new(8.9, 1.2, 20.0)),
        "{}",
        at(&world)
    );
}

/// A tank whose turret and barrel are plain mesh nodes: `hull` (one mesh
/// child) -> `turret` (mesh, carrying the barrel when `turret_has_barrel`) ->
/// `barrel` (a childless mesh node). Names get the `_LOD0` suffix.
fn mesh_turret_scene(turret_has_barrel: bool) -> Scene {
    let quad = || {
        writer::box_faces([0.0; 3], [1.0; 3], [0; 6])
            .into_iter()
            .next()
            .unwrap()
    };
    let node = |name: &str, translation: [f32; 3], prim: Option<writer::Prim>| writer::Node {
        name: name.into(),
        translation,
        prim,
        ..writer::Node::default()
    };
    let mut nodes = vec![
        node("tank_LOD0", [0.0; 3], None),
        node("hull_LOD0", [0.0; 3], None),
        node("hull_plate_LOD0", [0.0; 3], Some(quad())),
        node("turret_LOD0", [0.0, 1.0, 0.0], Some(quad())),
        node("barrel_LOD0", [0.3, 0.2, 0.0], Some(quad())),
    ];
    nodes[0].children = vec![1];
    nodes[1].children = vec![2, 3];
    if turret_has_barrel {
        nodes[3].children = vec![4];
    } else {
        // The barrel beside the turret, under the hull.
        nodes[1].children.push(4);
    }
    Scene {
        materials: vec![("steel".into(), writer::STEEL)],
        nodes,
        roots: vec![0],
        version: "2.0".into(),
        extra_json: String::new(),
    }
}

#[test]
fn a_childless_mesh_turret_or_barrel_is_a_part_with_its_joint() {
    let axes = AxisConvention::ENGINE;
    // hull -> turret -> barrel, the barrel a childless mesh node.
    let model = load_glb(&writer::write(&mesh_turret_scene(true)), &axes).unwrap();
    let parts = &model.levels[0].parts;
    let names: Vec<&str> = parts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["hull", "turret", "barrel"]);
    assert_eq!(parts[0].joint, Joint::Fixed);
    assert_eq!(parts[1].joint, Joint::Yaw);
    assert_eq!(parts[2].joint, Joint::Pitch);
    assert_eq!(parts[1].parent, Some(0));
    assert_eq!(parts[2].parent, Some(1), "the barrel hangs on the turret");
    // Each keeps its own mesh: one face of two triangles.
    for part in parts {
        assert_eq!(part.mesh.indices.len(), 6, "{}", part.name);
    }
    // The barrel tilts about its own pivot, the turret yaws it along.
    let set = model_set(std::slice::from_ref(&model), &[]).unwrap();
    let rig = set.model(0).parts(0);
    let mut world = [Mat4::IDENTITY; MAX_PARTS];
    let quarter = std::f32::consts::FRAC_PI_2;
    let tip = Vec3::new(0.8, 0.0, 0.0);
    pose_parts(
        Mat4::IDENTITY,
        rig,
        PartAngles {
            yaw: 0.0,
            pitch: quarter,
        },
        &mut world,
    );
    assert!(close(
        world[2].transform_point3(tip),
        Vec3::new(0.3, 2.0, 0.0)
    ));
    pose_parts(
        Mat4::IDENTITY,
        rig,
        PartAngles {
            yaw: quarter,
            pitch: 0.0,
        },
        &mut world,
    );
    assert!(close(
        world[2].transform_point3(tip),
        Vec3::new(0.0, 1.2, 1.1)
    ));

    // A childless mesh turret (no barrel below it) is a yawing part too, and
    // the hull keeps only its own plate.
    let model = load_glb(&writer::write(&mesh_turret_scene(false)), &axes).unwrap();
    let parts = &model.levels[0].parts;
    let names: Vec<&str> = parts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["hull", "turret", "barrel"]);
    assert_eq!(parts[1].joint, Joint::Yaw);
    assert_eq!(parts[2].joint, Joint::Pitch);
    assert_eq!(parts[2].parent, Some(0));
    assert_eq!(parts[0].mesh.indices.len(), 6);
}

#[test]
fn bounds_cover_the_finest_level_and_the_reach_any_pose() {
    let bytes = writer::write(&writer::orientation(&AxisConvention::ENGINE));
    let model = load_glb(&bytes, &AxisConvention::ENGINE).unwrap();
    let b = bounds_of(&model);
    assert!(close(b.min, Vec3::new(-1.0, 0.0, -0.5)), "{}", b.min);
    // Barrel tip 1.1 forward; turret top at 1.4.
    assert!(close(b.max, Vec3::new(1.1, 1.4, 0.5)), "{}", b.max);
    let set = model_set(std::slice::from_ref(&model), &[]).unwrap();
    let parts = set.model(0).parts(0);
    let mut world = [Mat4::IDENTITY; MAX_PARTS];
    for i in 0..64 {
        let a = i as f32 * 0.3;
        let angles = PartAngles {
            yaw: a,
            pitch: a * 0.7,
        };
        pose_parts(Mat4::IDENTITY, parts, angles, &mut world);
        for (p, part) in model.levels[0].parts.iter().enumerate() {
            for v in &part.mesh.vertices {
                let w = world[p].transform_point3(Vec3::from(v.position));
                assert!(w.length() <= b.reach + 1e-4, "{w} beyond {}", b.reach);
            }
        }
    }
}

#[test]
fn the_test_pack_is_one_model_set_with_one_mesh_per_part_per_level() {
    let config = std::fs::read_to_string(assets().join(MODELS_CONFIG)).unwrap();
    let pack = load_pack(&config, "testpack", &assets()).expect("test pack loads");
    assert_eq!(pack.models.len(), 14);
    // At most 14 models x 3 levels x 3 parts (126); the parts with geometry
    // are 83, each one mesh, so one draw call. Without the merge per part
    // the pack has 191 mesh nodes (up to 7 per unit per level).
    assert_eq!(pack.set.extra_meshes().len(), 83);
    assert_eq!(pack.set.mesh_count(), BUILTIN_MESHES + 83);
    for chain in pack.set.models() {
        assert_eq!(chain.len(), 3);
        assert_eq!(chain.switch_px(), &[60.0, 30.0]);
        let b = chain.bounds().unwrap();
        assert!(b.min.y >= 0.0 && b.max.y > 0.5);
    }
    let style = pack.style(&crate::Style::embedded()).unwrap();
    assert_eq!(style.shape_count(), 14);
    assert_eq!(style.shape(0).icon, crate::IconShape::Triangle.into());
    assert_eq!(style.shape(13).icon, crate::IconShape::Square.into());
    // The scout is about 4 m long (manifest `rompmaat_m`), the factory 14 m.
    assert!((style.shape(0).size[0] - 4.0).abs() < 0.6);
    assert!((style.shape(12).size[0] - 14.0).abs() < 0.6);
    // Unknown pack: a clear error.
    let e = load_pack(&config, "nope", &assets()).err().unwrap();
    assert!(e.to_string().contains("testpack"), "{e}");
}

#[test]
fn a_scaled_model_is_the_same_model_in_other_units() {
    let config = std::fs::read_to_string(assets().join(MODELS_CONFIG)).unwrap();
    let pack = load_pack(&config, "testpack", &assets()).unwrap();
    // The tank has a hull, a turret that turns and a barrel that tilts.
    let tank = &pack.models[2];
    let big = tank.scaled(2.0);
    assert_eq!(big.levels.len(), tank.levels.len());
    let (a, b) = (bounds_of(tank), bounds_of(&big));
    assert!(
        close(b.min, a.min * 2.0) && close(b.max, a.max * 2.0),
        "{a:?} {b:?}"
    );
    assert!((b.reach - 2.0 * a.reach).abs() < 1e-3);
    // Rotations are untouched, only the translations scale.
    for (p, q) in tank.levels[0].parts.iter().zip(&big.levels[0].parts) {
        assert_eq!(p.joint, q.joint);
        assert_eq!(p.parent, q.parent);
        assert!(close(
            q.rest.w_axis.truncate(),
            p.rest.w_axis.truncate() * 2.0
        ));
        assert!(close(q.rest.x_axis.truncate(), p.rest.x_axis.truncate()));
        assert_eq!(p.mesh.indices, q.mesh.indices);
    }
    assert_eq!(tank.scaled(1.0), *tank);
}

#[test]
fn one_model_loads_by_its_path_and_can_be_fitted_to_a_box() {
    let config = std::fs::read_to_string(assets().join(MODELS_CONFIG)).unwrap();
    let path = "testpack_v1/models/raffinaderij.glb";
    let icon = crate::IconId::from(crate::IconShape::Square);
    let plain = load_pack_model(&config, path, icon, &assets(), None).unwrap();
    assert_eq!((plain.models.len(), plain.icons.len()), (1, 1));
    assert_eq!(plain.icons[0], icon);
    assert_eq!(plain.set.models().len(), 1);
    // The levels switch where the pack says.
    assert_eq!(plain.set.models()[0].switch_px(), &[60.0, 30.0]);
    let b = bounds_of(&plain.models[0]);
    let (x, z) = (b.max.x - b.min.x, b.max.z - b.min.z);
    // Fitted to a box twice as wide as long: the tighter side sets the scale.
    let fitted = load_pack_model(
        &config,
        path,
        icon,
        &assets(),
        Some((2.0 * x, 2.0 * z * 0.5)),
    )
    .unwrap();
    let f = bounds_of(&fitted.models[0]);
    let (fx, fz) = (f.max.x - f.min.x, f.max.z - f.min.z);
    assert!(
        (fz - z).abs() < 1e-3 && (fx - x).abs() < 1e-3,
        "scale 1: {fx} x {fz}"
    );
    let fitted = load_pack_model(&config, path, icon, &assets(), Some((2.0 * x, 4.0 * z))).unwrap();
    let f = bounds_of(&fitted.models[0]);
    assert!(((f.max.x - f.min.x) - 2.0 * x).abs() < 1e-3);
    // Never more than four times as big or a quarter of the size.
    let tiny = load_pack_model(&config, path, icon, &assets(), Some((0.001, 0.001))).unwrap();
    let t = bounds_of(&tiny.models[0]);
    assert!(((t.max.x - t.min.x) - 0.25 * x).abs() < 1e-3);

    // A path outside every pack directory, a missing file and a broken
    // models.ron are errors, not panics.
    let e = load_pack_model(&config, "elsewhere/x.glb", icon, &assets(), None)
        .err()
        .unwrap();
    assert!(e.to_string().contains("no model pack"), "{e}");
    let e = load_pack_model(
        &config,
        "testpack_v1/models/nope.glb",
        icon,
        &assets(),
        None,
    )
    .err()
    .unwrap();
    assert!(matches!(e, GlbError::Io(_)), "{e}");
    assert!(load_pack_model("(", path, icon, &assets(), None).is_err());
    // The directory must match whole path parts.
    let e = load_pack_model(&config, "testpack_v1x/models/a.glb", icon, &assets(), None)
        .err()
        .unwrap();
    assert!(e.to_string().contains("no model pack"), "{e}");
}

fn minimal(version: &str, extra: &str) -> Vec<u8> {
    let mut scene: Scene = writer::orientation(&AxisConvention::ENGINE);
    scene.version = version.into();
    scene.extra_json = extra.into();
    writer::write(&scene)
}

fn load_err(bytes: &[u8]) -> GlbError {
    load_glb(bytes, &AxisConvention::ENGINE).expect_err("must be refused")
}

#[test]
fn bad_files_give_clear_errors_not_panics() {
    let missing = load_glb_file(
        &pack_dir().join("does_not_exist.glb"),
        &AxisConvention::ENGINE,
    );
    let e = missing.expect_err("missing file");
    assert!(matches!(e, GlbError::Io(_)), "{e}");
    assert!(e.to_string().contains("does_not_exist.glb"), "{e}");

    // glTF 1.0 in the asset, and a GLB container version 1.
    let e = load_err(&minimal("1.0", ""));
    assert!(matches!(e, GlbError::Version(_)), "{e}");
    assert!(e.to_string().contains("1.0"), "{e}");
    let mut v1 = minimal("2.0", "");
    v1[4] = 1;
    assert!(matches!(load_err(&v1), GlbError::Version(_)));

    // Not glTF at all, cut short, empty.
    for bytes in [
        &b"hello world, not a model"[..],
        &minimal("2.0", "")[..40],
        &[],
    ] {
        let e = load_err(bytes);
        assert!(matches!(e, GlbError::Parse(_)), "{e}");
    }

    // Unsupported features.
    for (extra, what) in [
        (
            ",\"extensionsUsed\":[\"KHR_draco_mesh_compression\"],\"extensionsRequired\":[\"KHR_draco_mesh_compression\"]",
            "draco",
        ),
        (",\"skins\":[{\"joints\":[1]}]", "skin"),
        (
            ",\"animations\":[{\"channels\":[],\"samplers\":[]}]",
            "animation",
        ),
        (
            ",\"images\":[{\"uri\":\"a.png\"}],\"textures\":[{\"source\":0}]",
            "texture",
        ),
    ] {
        let e = load_err(&minimal("2.0", extra));
        assert!(
            matches!(e, GlbError::Unsupported(_) | GlbError::Parse(_)),
            "{what}: {e}"
        );
        assert!(e.to_string().to_lowercase().contains(what), "{what}: {e}");
    }

    // Points instead of triangles.
    let mut scene = writer::orientation(&AxisConvention::ENGINE);
    for node in &mut scene.nodes {
        if let Some(p) = &mut node.prim {
            p.mode = Some(0);
        }
    }
    let e = load_err(&writer::write(&scene));
    assert!(matches!(e, GlbError::Unsupported(_)), "{e}");
    assert!(e.to_string().contains("triangle"), "{e}");

    // An index past the end of the vertices.
    let mut scene = writer::orientation(&AxisConvention::ENGINE);
    scene.nodes[2].prim.as_mut().unwrap().indices[0] = 99;
    let e = load_err(&writer::write(&scene));
    assert!(
        matches!(e, GlbError::Invalid(_) | GlbError::Parse(_)),
        "{e}"
    );

    // A scene with nothing in it.
    let scene = Scene {
        version: "2.0".into(),
        ..Scene::default()
    };
    assert!(matches!(
        load_err(&writer::write(&scene)),
        GlbError::Invalid(_) | GlbError::Parse(_)
    ));

    bad_hierarchies_and_accessors();
}

/// Node graphs that would recurse forever or blow the stack, mirrored and
/// non-uniformly scaled parts, and an accessor past its buffer.
fn bad_hierarchies_and_accessors() {
    let base = || writer::orientation(&AxisConvention::ENGINE);
    let (hull, turret, barrel) = (1, 8, 15);
    assert_eq!(base().nodes[turret].name, "turret_LOD0");
    assert_eq!(base().nodes[barrel].name, "barrel_LOD0");
    let clear = |scene: &Scene, what: &str| {
        let e = load_err(&writer::write(scene));
        assert!(
            matches!(e, GlbError::Invalid(_) | GlbError::Parse(_)),
            "{what}: {e}"
        );
        e
    };

    // The three shapes the "appears twice" check refuses: a node that is its
    // own child, a cycle and a diamond (one node below two parents). Each
    // must fail for that reason and name the node, not just fail.
    let twice = |e: &GlbError, node: &str, what: &str| {
        assert!(matches!(e, GlbError::Invalid(_)), "{what}: {e}");
        let text = e.to_string();
        assert!(
            text.contains("appears twice in the hierarchy") && text.contains(node),
            "{what}: {text}"
        );
    };
    // A node that is its own child.
    let mut scene = base();
    scene.nodes[hull].children.push(hull);
    let e = clear(&scene, "self child");
    twice(&e, "hull", "self child");
    // A cycle: the barrel has the hull as a child.
    let mut scene = base();
    scene.nodes[barrel].children.push(hull);
    let e = clear(&scene, "cycle");
    twice(&e, "hull", "cycle");
    // A diamond: one extra node is a child of the turret and of the barrel
    // (which is itself a child of the turret), so it is reached twice.
    let mut scene = base();
    let shared = scene.nodes.len();
    scene.nodes.push(writer::Node {
        name: "shared_LOD0".into(),
        ..writer::Node::default()
    });
    scene.nodes[turret].children.push(shared);
    scene.nodes[barrel].children.push(shared);
    let e = clear(&scene, "diamond");
    twice(&e, "shared", "diamond");
    // The same node under two siblings of the hull is a diamond as well.
    let mut scene = base();
    let (left, right, shared) = (
        scene.nodes.len(),
        scene.nodes.len() + 1,
        scene.nodes.len() + 2,
    );
    for (name, children) in [
        ("left_LOD0", vec![shared]),
        ("right_LOD0", vec![shared]),
        ("shared_LOD0", vec![]),
    ] {
        scene.nodes.push(writer::Node {
            name: name.into(),
            children,
            ..writer::Node::default()
        });
    }
    scene.nodes[hull].children.extend([left, right]);
    let e = clear(&scene, "diamond below the hull");
    twice(&e, "shared", "diamond below the hull");
    // The same shape without sharing loads: two children, each with its own
    // child. (An empty node is pruned; the point is that it is not refused.)
    let mut scene = base();
    let first = scene.nodes.len();
    for (name, children) in [
        ("left_LOD0", vec![first + 2]),
        ("right_LOD0", vec![first + 3]),
        ("left_leaf_LOD0", vec![]),
        ("right_leaf_LOD0", vec![]),
    ] {
        scene.nodes.push(writer::Node {
            name: name.into(),
            children,
            ..writer::Node::default()
        });
    }
    scene.nodes[hull].children.extend([first, first + 1]);
    assert!(load_glb(&writer::write(&scene), &AxisConvention::ENGINE).is_ok());
    // A chain 200,000 nodes deep under the barrel.
    let mut scene = base();
    let first = scene.nodes.len();
    let deep = 200_000;
    for i in 0..deep {
        scene.nodes.push(writer::Node {
            name: format!("link{i}"),
            children: if i + 1 < deep {
                vec![first + i + 1]
            } else {
                vec![]
            },
            ..writer::Node::default()
        });
    }
    scene.nodes[barrel].children.push(first);
    let e = clear(&scene, "deep chain");
    assert!(e.to_string().contains("deeper"), "{e}");

    // A mirrored turret and a turret stretched along one axis.
    for (scale, what) in [
        ([-1.0, 1.0, 1.0], "mirror"),
        ([1.0, 2.0, 1.0], "non-uniform"),
    ] {
        let mut scene = base();
        scene.nodes[turret].scale = Some(scale);
        let e = load_err(&writer::write(&scene));
        assert!(matches!(e, GlbError::Unsupported(_)), "{what}: {e}");
        assert!(
            e.to_string().contains("turret") && e.to_string().contains(what),
            "{e}"
        );
    }
    // A uniform scale is fine.
    let mut scene = base();
    scene.nodes[turret].scale = Some([2.0, 2.0, 2.0]);
    assert!(load_glb(&writer::write(&scene), &AxisConvention::ENGINE).is_ok());

    // A raw `matrix` that shears: columns of equal length, a positive
    // determinant and still not at right angles. The turret sits at (0, 1, 0).
    let turret_matrix = |columns: [[f32; 3]; 3]| {
        let [x, y, z] = columns;
        [
            x[0], x[1], x[2], 0.0, y[0], y[1], y[2], 0.0, z[0], z[1], z[2], 0.0, 0.0, 1.0, 0.0, 1.0,
        ]
    };
    let with_matrix = |columns: [[f32; 3]; 3]| {
        let mut scene = base();
        scene.nodes[turret].matrix = Some(turret_matrix(columns));
        writer::write(&scene)
    };
    let skew = |cosine: f32| {
        // The Y column leans away from Y by an angle whose cosine to the X
        // column is `cosine`; its length stays 1.
        [
            [1.0, 0.0, 0.0],
            [cosine, (1.0 - cosine * cosine).sqrt(), 0.0],
            [0.0, 0.0, 1.0],
        ]
    };
    for (cosine, what) in [(0.5, "a 60 degree shear"), (0.01, "a 0.6 degree shear")] {
        let columns = skew(cosine);
        let det = columns[0][0] * columns[1][1] - columns[0][1] * columns[1][0];
        assert!(det > 0.0, "{what}: a positive determinant");
        let e = load_err(&with_matrix(columns));
        assert!(matches!(e, GlbError::Unsupported(_)), "{what}: {e}");
        let text = e.to_string();
        assert!(
            text.contains("turret") && text.contains("skewed") && text.contains("columns 0 and 1"),
            "{what}: {text}"
        );
    }
    // Shears between the other pairs of columns are refused as well.
    let e = load_err(&with_matrix([
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.3, 0.0, (1.0f32 - 0.09).sqrt()],
    ]));
    assert!(e.to_string().contains("columns 0 and 2"), "{e}");
    let e = load_err(&with_matrix([
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.3, (1.0f32 - 0.09).sqrt()],
    ]));
    assert!(e.to_string().contains("columns 1 and 2"), "{e}");
    // Inside the tolerance (cosine 1e-4, 0.006 degrees) the matrix passes, and
    // so does a clean rotation with a uniform scale written as a matrix: a
    // quarter turn about Y, doubled.
    assert!(load_glb(&with_matrix(skew(1e-4)), &AxisConvention::ENGINE).is_ok());
    let quarter_turn = [[0.0, 0.0, -2.0], [0.0, 2.0, 0.0], [2.0, 0.0, 0.0]];
    assert!(load_glb(&with_matrix(quarter_turn), &AxisConvention::ENGINE).is_ok());
    // And the plain identity matrix.
    let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    assert!(load_glb(&with_matrix(identity), &AxisConvention::ENGINE).is_ok());

    // An accessor that claims more vertices than its buffer view holds:
    // same byte length, so the GLB chunks stay valid.
    let mut bytes = writer::write(&base());
    let at = bytes
        .windows(9)
        .position(|w| w == b"\"count\":4")
        .expect("a 4-vertex accessor");
    bytes[at + 8] = b'9';
    let e = load_err(&bytes);
    assert!(!e.to_string().contains("has no POSITION"), "{e}");
    assert!(
        e.to_string().contains("out of range") || matches!(e, GlbError::Parse(_)),
        "{e}"
    );
}

#[test]
fn levels_must_be_numbered_without_gaps() {
    let mut scene = writer::orientation(&AxisConvention::ENGINE);
    scene.nodes[0].name = "orient_LOD1".into();
    let e = load_err(&writer::write(&scene));
    assert!(e.to_string().contains("without gaps"), "{e}");
    // A single root without a suffix is a one-level model.
    scene.nodes[0].name = "orient".into();
    let m = load_glb(&writer::write(&scene), &AxisConvention::ENGINE).unwrap();
    assert_eq!(m.levels.len(), 1);
}

/// A model `m` with three levels in one scene: per level a root `m_LOD<n>`
/// with one mesh child `hull`, the level getting smaller with `n`. The roots
/// are the nodes 0, 2 and 4.
fn three_level_scene() -> Scene {
    let node = |name: String, size: f32, children: Vec<usize>, mesh: bool| writer::Node {
        name,
        children,
        prim: mesh.then(|| {
            writer::box_faces([0.0; 3], [size; 3], [0; 6])
                .into_iter()
                .next()
                .unwrap()
        }),
        ..writer::Node::default()
    };
    let mut nodes = Vec::new();
    for n in 0..3 {
        nodes.push(node(format!("m_LOD{n}"), 1.0, vec![nodes.len() + 1], false));
        nodes.push(node("hull".into(), 1.0 - 0.1 * n as f32, vec![], true));
    }
    Scene {
        materials: vec![("steel".into(), writer::STEEL)],
        nodes,
        roots: vec![0, 2, 4],
        version: "2.0".into(),
        extra_json: String::new(),
    }
}

/// `glb` with the value of its `scenes` member replaced by `scenes_json`
/// (the writer puts `"scenes":[...]` directly before `"nodes"`).
fn with_scenes(glb: &[u8], scenes_json: &str) -> Vec<u8> {
    let len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let json = std::str::from_utf8(&glb[20..20 + len]).unwrap();
    let start = json.find("\"scenes\":").unwrap() + "\"scenes\":".len();
    let end = json.find("],\"nodes\":[").unwrap() + 1;
    let mut json = format!("{}{scenes_json}{}", &json[..start], &json[end..]).into_bytes();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let rest = &glb[20 + len..];
    let mut out = Vec::new();
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&((20 + json.len() + rest.len()) as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(rest);
    out
}

const PER_LOD: &str =
    r#"[{"name":"LOD0","nodes":[0]},{"name":"LOD1","nodes":[2]},{"name":"LOD2","nodes":[4]}]"#;

#[test]
fn a_scene_per_lod_loads_as_the_same_model_as_one_scene() {
    let axes = AxisConvention::ENGINE;
    let one = writer::write(&three_level_scene());
    let per_lod = with_scenes(&one, PER_LOD);
    assert_ne!(one, per_lod);
    let (a, b) = (
        load_glb(&one, &axes).unwrap(),
        load_glb(&per_lod, &axes).unwrap(),
    );
    assert_eq!(a.levels.len(), 3);
    assert_eq!(a, b, "the layout of the scenes changes nothing");

    // The order of the scenes in the file does not matter.
    let shuffled = with_scenes(
        &one,
        r#"[{"name":"LOD2","nodes":[4]},{"name":"LOD0","nodes":[0]},{"name":"LOD1","nodes":[2]}]"#,
    );
    assert_eq!(load_glb(&shuffled, &axes).unwrap(), a);
}

#[test]
fn lod_roots_names_the_layout_and_the_roots_in_level_order() {
    let names = |bytes: &[u8]| {
        let gltf = gltf::Gltf::from_slice(bytes).unwrap();
        let (layout, roots) = lod_roots(&gltf.document).unwrap();
        let names: Vec<String> = roots
            .iter()
            .map(|r| r.name().unwrap_or("").to_owned())
            .collect();
        (layout, names)
    };
    let one = writer::write(&three_level_scene());
    let expected = ["m_LOD0", "m_LOD1", "m_LOD2"].map(String::from).to_vec();
    assert_eq!(names(&one), (LodLayout::OneScene, expected.clone()));
    let per_lod = with_scenes(&one, PER_LOD);
    assert_eq!(names(&per_lod), (LodLayout::ScenePerLod, expected));
}

#[test]
fn a_file_with_very_many_lod_scenes_is_refused_quickly() {
    // 80,000 scenes named LOD<n> once took seconds in the duplicate check.
    let one = writer::write(&three_level_scene());
    let scenes: Vec<String> = (0..80_000)
        .map(|n| format!(r#"{{"name":"LOD{n}","nodes":[0]}}"#))
        .collect();
    let hostile = with_scenes(&one, &format!("[{}]", scenes.join(",")));
    let start = std::time::Instant::now();
    let e = load_glb(&hostile, &AxisConvention::ENGINE).unwrap_err();
    let took = start.elapsed();
    assert!(e.to_string().contains("levels of detail"), "{e}");
    assert!(took < std::time::Duration::from_secs(2), "took {took:?}");
}

#[test]
fn only_clean_scene_per_lod_files_are_accepted() {
    let axes = AxisConvention::ENGINE;
    let one = writer::write(&three_level_scene());
    let refused = |scenes: &str, needle: &str| {
        let e = load_glb(&with_scenes(&one, scenes), &axes).expect_err(scenes);
        assert!(e.to_string().contains(needle), "{scenes}: {e}");
    };
    // A scene with two roots, or none.
    refused(
        r#"[{"name":"LOD0","nodes":[0,2]},{"name":"LOD1","nodes":[4]}]"#,
        "exactly one root",
    );
    refused(
        r#"[{"name":"LOD0","nodes":[0]},{"name":"LOD1","nodes":[]}]"#,
        "exactly one root",
    );
    // The number of the scene and the number of its root disagree.
    refused(
        r#"[{"name":"LOD0","nodes":[0]},{"name":"LOD1","nodes":[4]},{"name":"LOD2","nodes":[2]}]"#,
        "must be named <model>_LOD1",
    );
    // Two scenes with one number: either the root has the wrong number for
    // the second scene, or it is the same root twice.
    refused(
        r#"[{"name":"LOD0","nodes":[0]},{"name":"LOD0","nodes":[2]}]"#,
        "must be named <model>_LOD0",
    );
    refused(
        r#"[{"name":"LOD0","nodes":[0]},{"name":"LOD0","nodes":[0]}]"#,
        "two scenes are named LOD0",
    );
    // A gap in the numbers is the same error as in one scene.
    refused(
        r#"[{"name":"LOD0","nodes":[0]},{"name":"LOD2","nodes":[4]}]"#,
        "without gaps",
    );
    // A model name that differs between the levels is the same error too.
    let mut other = three_level_scene();
    other.nodes[2].name = "n_LOD1".into();
    let e = load_glb(&with_scenes(&writer::write(&other), PER_LOD), &axes).unwrap_err();
    assert!(e.to_string().contains("different models"), "{e}");

    // Scenes that are not (all) named LOD<n> are not the layout: the default
    // scene is read as always. Here scene 0 is a one-level model.
    for scenes in [
        r#"[{"nodes":[0]},{"nodes":[2]}]"#,
        r#"[{"name":"LOD0","nodes":[0]},{"name":"extra","nodes":[2]}]"#,
        r#"[{"name":"lod0","nodes":[0]},{"name":"lod1","nodes":[2]}]"#,
        r#"[{"name":"LOD","nodes":[0]},{"name":"LODx","nodes":[2]}]"#,
        r#"[{"name":"LOD0","nodes":[0]}]"#,
    ] {
        let model = load_glb(&with_scenes(&one, scenes), &axes).unwrap();
        assert_eq!(model.levels.len(), 1, "{scenes}");
    }
}
