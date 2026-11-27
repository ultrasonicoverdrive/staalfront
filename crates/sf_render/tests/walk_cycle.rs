//! The walk cycle through the instancer (WP11.6i): the set v5 trial rifleman's leg instances
//! swing with the distance the unit travels and settle back when it stops, the set v3 rifleman
//! (nodes not at the joints) is drawn as before, and a tank's turret is posed exactly as without
//! the walk.

use std::path::PathBuf;

use glam::{Mat4, Vec3};
use sf_math::render_bridge::metres_to_cells;
use sf_math::{Facing, Vec2};
use sf_render::glb::{self, AxisConvention, LoadedModel};
use sf_render::model::{MAX_PARTS, PartAngles, pose_parts};
use sf_render::{FrameView, Frustum, Instancer, ModelSet, Motion, Style, UnitId};

fn load(path: &str) -> LoadedModel {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets")
        .join(path);
    glb::load_glb_file(&p, &AxisConvention::ENGINE).unwrap()
}

/// Shapes: 0 the set v5 rifleman, 1 the set v3 rifleman, 2 the set v5 light tank.
fn set() -> ModelSet {
    let models = [
        load("set_v5/meridian_rifleman.glb"),
        load("set_v3/meridian_rifleman.glb"),
        load("set_v5/meridian_light_tank.glb"),
    ];
    glb::model_set(&models, &[60.0, 30.0]).unwrap()
}

fn frustum() -> Frustum {
    let vp =
        glam::camera::rh::proj::directx::orthographic(-100.0, 100.0, -100.0, 100.0, -100.0, 100.0)
            * glam::camera::rh::view::look_at_mat4(
                Vec3::new(50.0, 50.0, 50.0),
                Vec3::new(50.0, 0.0, 50.0),
                -Vec3::Z,
            );
    Frustum::from_view_proj(vp)
}

fn cells(x: f32) -> Vec2 {
    Vec2::new(metres_to_cells(x), metres_to_cells(50.0))
}

/// The world matrix of part `part` of shape `shape` at LOD0 in the last build.
fn part_matrix(inst: &Instancer, set: &ModelSet, shape: u8, part: usize) -> Mat4 {
    let mesh = set.model(shape).parts(0)[part].mesh;
    Mat4::from_cols_array_2d(&inst.mesh_instances(mesh)[0].model)
}

/// The left foot of the trial rifleman along `+X` relative to the unit (0.9 m below the hip).
fn foot_ahead(inst: &Instancer, set: &ModelSet, unit_x: f32) -> f32 {
    part_matrix(inst, set, 0, 0)
        .transform_point3(Vec3::new(0.0, -0.9, 0.0))
        .x
        - unit_x
}

#[test]
fn the_legs_swing_with_the_distance_and_settle_when_the_unit_stops() {
    let set = set();
    let mut inst = Instancer::new(Style::embedded(), (128.0, 128.0));
    inst.set_models(set.clone());
    let f = frustum();
    let id = UnitId::from_parts(1, 0);
    let walk = |inst: &mut Instancer, from: f32, to: f32, steps: u32| {
        inst.update_tick([Motion::from_fixed(
            0,
            0,
            cells(from),
            cells(to),
            Facing::new(0),
            Facing::new(0),
        )
        .with_id(id)]);
        for k in 0..=steps {
            let a = k as f32 / steps as f32;
            inst.build_frame(&FrameView::models_only(&f, a), |_, _| 0.0);
        }
    };
    // Standing: the leg hangs straight.
    walk(&mut inst, 40.0, 40.0, 4);
    assert!(foot_ahead(&inst, &set, 40.0).abs() < 1e-3);
    // A quarter of a stride (1.46 m per cycle): the left leg is at its most forward.
    let stride = sf_render::walk::WalkConfig::embedded().stride_m;
    walk(&mut inst, 40.0, 40.0 + stride * 0.25, 30);
    let ahead = foot_ahead(&inst, &set, 40.0 + stride * 0.25);
    // 0.9 m * sin(24°) = 0.366 m, times the blend weight (fully in after 15 frames of 1/60 s).
    assert!((ahead - 0.366).abs() < 0.01, "{ahead}");
    // The same quarter stride in 4 frames or in 60: the same phase (distance, not time).
    let mut slow = Instancer::new(Style::embedded(), (128.0, 128.0));
    slow.set_models(set.clone());
    walk(&mut slow, 40.0, 40.0, 4);
    walk(&mut slow, 40.0, 40.0 + stride * 0.25, 60);
    let ahead_slow = foot_ahead(&slow, &set, 40.0 + stride * 0.25);
    assert!((ahead_slow - ahead).abs() < 1e-3, "{ahead_slow} {ahead}");
    // Stopped: back in the stand pose after the blend time.
    let x = 40.0 + stride * 0.25;
    walk(&mut inst, x, x, 30);
    assert!(foot_ahead(&inst, &set, x).abs() < 1e-3);
}

#[test]
fn a_figure_without_joint_origins_and_a_turret_are_drawn_as_before() {
    let set = set();
    let mut inst = Instancer::new(Style::embedded(), (128.0, 128.0));
    inst.set_models(set.clone());
    let f = frustum();
    let v3 = UnitId::from_parts(1, 0);
    let tank = UnitId::from_parts(2, 0);
    inst.update_tick([
        Motion::from_fixed(
            1,
            0,
            cells(30.0),
            cells(31.0),
            Facing::new(0),
            Facing::new(0),
        )
        .with_id(v3),
        Motion::from_fixed(
            2,
            0,
            cells(60.0),
            cells(61.0),
            Facing::new(0),
            Facing::new(0),
        )
        .with_turret(Facing::new(0), Facing::new(64))
        .with_id(tank),
    ]);
    for k in 0..=10 {
        let a = k as f32 / 10.0;
        inst.build_frame(&FrameView::models_only(&f, a), |_, _| 0.0);
        // The set v3 figure is one body at the unit's matrix.
        assert!(!set.model(1).walks());
        assert_eq!(set.model(1).parts(0).len(), 1);
        let body = part_matrix(&inst, &set, 1, 0);
        let unit = sf_render::instancing::model_matrix(30.0 + a, 0.0, 50.0, 0.0, [1.0; 3]);
        let rest = set.model(1).parts(0)[0].rest;
        assert!(body.abs_diff_eq(Mat4::from_cols_array_2d(&unit) * rest, 1e-5));
        // The tank: every part where pose_parts puts it with the turret's yaw and no walk.
        let unit = Mat4::from_cols_array_2d(&sf_render::instancing::model_matrix(
            60.0 + a,
            0.0,
            50.0,
            0.0,
            [1.0; 3],
        ));
        let parts = set.model(2).parts(0);
        let mut want = [Mat4::IDENTITY; MAX_PARTS];
        let yaw = 64.0 * a * std::f32::consts::TAU / 256.0;
        pose_parts(unit, parts, PartAngles::new(yaw, 0.0), &mut want);
        for (i, w) in want.iter().enumerate().take(parts.len()) {
            assert!(
                part_matrix(&inst, &set, 2, i).abs_diff_eq(*w, 1e-4),
                "part {i}"
            );
        }
    }
}
