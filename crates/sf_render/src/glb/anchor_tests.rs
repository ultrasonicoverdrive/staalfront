//! Anchor tests (WP5.6b): `muzzle*` and `fx_*` nodes are kept by the loader, follow the pruning of empty
//! parts, the axis conversion and the joints, and the old models (test pack, set v3) have none.

use std::path::PathBuf;

use glam::{Mat4, Vec3};

use super::writer::{self, Scene};
use super::*;
use crate::model::PartAngles;

fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-5
}

fn translation_of(m: Mat4) -> Vec3 {
    m.w_axis.truncate()
}

fn quad() -> writer::Prim {
    writer::box_faces([0.0; 3], [1.0; 3], [0; 6])
        .into_iter()
        .next()
        .unwrap()
}

/// A tank with an `fx_smoke` on the hull, a `barrel` with no geometry of its own (so it is pruned) and a
/// `muzzle` below that barrel.
fn anchor_scene() -> Scene {
    let node = |name: &str, translation: [f32; 3], prim: Option<writer::Prim>| writer::Node {
        name: name.into(),
        translation,
        prim,
        ..writer::Node::default()
    };
    let mut nodes = vec![
        node("tank_LOD0", [0.0; 3], None),
        node("hull_LOD0", [0.0; 3], Some(quad())),
        node("turret_LOD0", [0.0, 1.0, 0.0], Some(quad())),
        node("barrel_LOD0", [1.0, 0.2, 0.0], None),
        node("muzzle_LOD0", [2.0, 0.0, 0.0], None),
        node("fx_smoke_LOD0", [-1.0, 0.5, 0.0], None),
    ];
    nodes[0].children = vec![1];
    nodes[1].children = vec![2, 5];
    nodes[2].children = vec![3];
    nodes[3].children = vec![4];
    Scene {
        materials: vec![("steel".into(), writer::STEEL)],
        nodes,
        roots: vec![0],
        version: "2.0".into(),
        extra_json: String::new(),
    }
}

/// Anchors are kept before the empty parts are pruned; an anchor whose part is pruned moves up to the
/// nearest part that stays, with the pruned part's frame folded into its matrix.
#[test]
fn an_anchor_under_a_pruned_part_moves_to_its_nearest_kept_ancestor() {
    let model = load_glb(&writer::write(&anchor_scene()), &AxisConvention::ENGINE).unwrap();
    let level = &model.levels[0];
    let names: Vec<&str> = level.parts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        ["hull", "turret"],
        "the empty barrel is pruned, anchors are not parts"
    );
    let by_name = |n: &str| level.anchors.iter().find(|a| a.name == n).unwrap();
    // Straight under the hull: kept as it was.
    let smoke = by_name("fx_smoke");
    assert_eq!(smoke.parent, Some(0));
    assert!(close(
        translation_of(smoke.local),
        Vec3::new(-1.0, 0.5, 0.0)
    ));
    // Under the pruned barrel (1, 0.2, 0 from the turret): now on the turret, 3 forward and 0.2 up.
    let muzzle = by_name("muzzle");
    assert_eq!(muzzle.parent, Some(1));
    assert!(
        close(translation_of(muzzle.local), Vec3::new(3.0, 0.2, 0.0)),
        "{}",
        translation_of(muzzle.local)
    );
    // The order is the file's.
    let order: Vec<&str> = level.anchors.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(order, ["muzzle", "fx_smoke"]);
}

/// The loader's conversion applies to anchors the way it applies to parts: a model authored in
/// centimetres with the front on +Z gets its anchor in metres with the front on +X, and `scaled` moves
/// anchors with the model.
#[test]
fn anchors_follow_the_axis_and_unit_conversion_and_scaling() {
    let source = AxisConvention {
        front: Axis::PlusZ,
        up: Axis::PlusY,
        metres_per_unit: 0.01,
    };
    // The same scene with every translation turned into the source axes (front +Z) and centimetres.
    let mut scene = anchor_scene();
    for n in &mut scene.nodes {
        let [x, y, z] = n.translation;
        // The engine's front +X is source +Z; the engine's right +Z is source -X.
        n.translation = [-z * 100.0, y * 100.0, x * 100.0];
    }
    let model = load_glb(&writer::write(&scene), &source).unwrap();
    let muzzle = model.levels[0]
        .anchors
        .iter()
        .find(|a| a.name == "muzzle")
        .unwrap();
    assert!(
        close(translation_of(muzzle.local), Vec3::new(3.0, 0.2, 0.0)),
        "{}",
        translation_of(muzzle.local)
    );
    let big = model.scaled(2.0);
    let a = &big.levels[0].anchors[0];
    assert!(close(
        translation_of(a.local),
        translation_of(model.levels[0].anchors[0].local) * 2.0
    ));
    assert!(close(a.local.x_axis.truncate(), Vec3::X), "rotation kept");
}

/// A node that is called muzzle but has geometry is a part like any other, and no anchor.
#[test]
fn a_muzzle_named_mesh_is_geometry_not_an_anchor() {
    let mut scene = anchor_scene();
    scene.nodes[4].prim = Some(quad());
    let model = load_glb(&writer::write(&scene), &AxisConvention::ENGINE).unwrap();
    let level = &model.levels[0];
    assert!(level.anchors.iter().all(|a| a.name != "muzzle"));
    // The mesh leaf is merged into the `barrel` frame it hangs on, which now has geometry and stays.
    let names: Vec<&str> = level.parts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["hull", "turret", "barrel"]);
    assert_eq!(level.parts[2].mesh.indices.len(), 6);
    assert_eq!(level.anchors.len(), 1, "only the fx_smoke is left");
}

fn heavy_tank() -> LoadedModel {
    load_glb_file(
        &assets().join("set_v4/veyran_heavy_tank.glb"),
        &AxisConvention::ENGINE,
    )
    .unwrap()
}

/// The heavy tank of set v4 has two muzzles on its barrel at every level (the file: `muzzle_l` at
/// (2.808, 0, -0.456) and `muzzle_r` at (2.808, 0, 0.456) from the barrel pivot).
#[test]
fn the_veyran_heavy_tank_has_two_muzzle_anchors() {
    let model = heavy_tank();
    assert_eq!(model.levels.len(), 3);
    for (i, level) in model.levels.iter().enumerate() {
        let names: Vec<&str> = level.anchors.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["muzzle_l", "muzzle_r"], "level {i}");
        let parts: Vec<&str> = level.parts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(parts, ["hull", "turret", "barrel"], "level {i}");
        for (a, z) in level.anchors.iter().zip([-0.456, 0.456]) {
            assert_eq!(a.parent, Some(2), "on the barrel");
            assert!(
                close(translation_of(a.local), Vec3::new(2.808, 0.0, z)),
                "{} {}",
                a.name,
                translation_of(a.local)
            );
        }
    }
    let set = model_set(std::slice::from_ref(&model), &DEFAULT_SWITCH_PX).unwrap();
    assert_eq!(set.model(0).anchor_count("muzzle"), 2);
    assert_eq!(set.model(0).anchor_count("fx_"), 0);
    assert_eq!(set.model(0).anchors(2).len(), 2, "every level carries them");
}

/// Turning the turret 64 steps (a quarter turn) swings the muzzle 90 degrees about the turret's pivot.
#[test]
fn a_turret_turned_64_steps_rotates_its_anchor_90_degrees_about_the_pivot() {
    let model = heavy_tank();
    let set = model_set(std::slice::from_ref(&model), &DEFAULT_SWITCH_PX).unwrap();
    let chain = set.model(0);
    let at = |yaw: f32, n: usize| {
        crate::model::anchor_position(
            Mat4::IDENTITY,
            chain.parts(0),
            chain.anchors(0),
            PartAngles { yaw, pitch: 0.0 },
            "muzzle",
            n,
        )
        .unwrap()
    };
    let steps_64 = 64.0 * std::f32::consts::TAU / 256.0;
    // The rest pose, pinned against the numbers of the file: the turret pivot (0.624, 1.334, 0), the barrel
    // pivot (0.78, 0.435, 0) from it, the muzzle (2.808, 0, -0.456) from that.
    assert!(
        close(at(0.0, 0), Vec3::new(4.212, 1.769, -0.456)),
        "{}",
        at(0.0, 0)
    );
    assert!(close(at(0.0, 1), Vec3::new(4.212, 1.769, 0.456)));
    let pivot = Vec3::new(0.624, 1.334, 0.0);
    for n in 0..2 {
        let (before, after) = (at(0.0, n) - pivot, at(steps_64, n) - pivot);
        // The same height and the same distance from the pivot ...
        assert!((before.y - after.y).abs() < 1e-5);
        assert!((before.length() - after.length()).abs() < 1e-5);
        // ... a right angle between the two horizontal arms ...
        let (b, a) = (before.with_y(0.0), after.with_y(0.0));
        assert!(b.dot(a).abs() < 1e-4, "{} apart", b.dot(a));
        // ... turned towards +Z, the sense of the facing (+X goes to +Z).
        assert!(close(a, Vec3::new(-b.z, 0.0, b.x)), "{a} from {b}");
    }
    // `n` counts the matching anchors round and round.
    assert!(close(at(0.0, 2), at(0.0, 0)) && close(at(0.0, 3), at(0.0, 1)));
    // No anchor of that name: nothing.
    assert!(
        crate::model::anchor_position(
            Mat4::IDENTITY,
            chain.parts(0),
            chain.anchors(0),
            PartAngles::default(),
            "fx_",
            0
        )
        .is_none()
    );
}

/// No model of the test pack or of set v3 has a muzzle anchor, so the game falls back to the
/// bounding-box rule of WP5.6a for them. (The test pack has no anchors at all; set v3 has `fx_` ones on
/// its neutral wrecks only, which the muzzle does not look at.)
#[test]
fn the_test_pack_and_set_v3_have_no_muzzle_anchors_and_fall_back() {
    let (mut testpack, mut v3, mut v3_with_fx) = (0, 0, 0);
    for (dir, count) in [
        (assets().join("testpack_v1/models"), &mut testpack),
        (assets().join("set_v3"), &mut v3),
    ] {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "glb") {
                continue;
            }
            let model = load_glb_file(&path, &AxisConvention::ENGINE).unwrap();
            let set = model_set(std::slice::from_ref(&model), &DEFAULT_SWITCH_PX).unwrap();
            assert_eq!(set.model(0).anchor_count("muzzle"), 0, "{}", path.display());
            let testpack_file = dir.ends_with("models");
            if testpack_file {
                assert!(set.model(0).anchors(0).is_empty(), "{}", path.display());
            } else {
                v3_with_fx += usize::from(set.model(0).anchor_count("fx_") > 0);
            }
            *count += 1;
        }
    }
    eprintln!("testpack {testpack} files, set v3 {v3} files ({v3_with_fx} with fx_)");
    assert_eq!((testpack, v3, v3_with_fx), (14, 30, 4));
}

/// How many files of set v4 carry muzzle anchors and how many fx anchors, and that every anchor sits in
/// the frame of a part that exists.
#[test]
fn set_v4_anchor_counts_are_these_numbers() {
    let (mut files, mut with_muzzle, mut with_fx, mut anchors) = (0, 0, 0, 0);
    for entry in std::fs::read_dir(assets().join("set_v4")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "glb") {
            continue;
        }
        let model = load_glb_file(&path, &AxisConvention::ENGINE).unwrap();
        files += 1;
        let level = &model.levels[0];
        with_muzzle += usize::from(level.anchors.iter().any(|a| a.name.starts_with("muzzle")));
        with_fx += usize::from(level.anchors.iter().any(|a| a.name.starts_with("fx_")));
        anchors += level.anchors.len();
        for a in &level.anchors {
            assert!(a.parent.is_none_or(|p| p < level.parts.len()));
            assert!(a.local.is_finite());
        }
    }
    eprintln!(
        "set v4: {files} files, {with_muzzle} with muzzle, {with_fx} with fx_, {anchors} anchors"
    );
    assert_eq!((files, with_muzzle, with_fx, anchors), (194, 44, 119, 283));
}
