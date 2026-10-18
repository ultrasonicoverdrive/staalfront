//! glTF models in the instancer (WP1.10): one list per part per level, the
//! real bounds for culling and picking, the turret demo and the highlight.
//! CPU only.

use std::path::PathBuf;
use std::time::Instant;

use super::*;
use crate::camera::{Camera, CameraConfig, DVec2};
use crate::glb::{self, LoadedPack};
use crate::pick::PickQuery;
use sf_math::render_bridge::metres_to_cells;

const W: f32 = 1280.0;
const H: f32 = 720.0;

fn pack() -> LoadedPack {
    let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let config = std::fs::read_to_string(assets.join(glb::MODELS_CONFIG)).unwrap();
    glb::load_pack(&config, "testpack", &assets).unwrap()
}

fn unit(shape: u8, player: u8, id: u32, at: (f32, f32), facing: u8) -> Motion {
    let p = Vec2::new(metres_to_cells(at.0), metres_to_cells(at.1));
    Motion::from_fixed(
        shape,
        player,
        p,
        p,
        Facing::new(facing),
        Facing::new(facing),
    )
    .with_id(UnitId::from_parts(id, 0))
}

fn with_pack(motions: &[Motion]) -> Instancer {
    let pack = pack();
    let style = pack.style(&Style::embedded()).unwrap();
    let mut inst = Instancer::new(Style::embedded(), (256.0, 256.0));
    inst.set_style_and_models(style, pack.set);
    inst.set_height_range(0.0, 30.0);
    inst.update_tick(motions.iter().copied());
    inst
}

fn camera(zoom: f64, focus: (f64, f64)) -> Camera {
    let cfg = CameraConfig {
        map_size: DVec2::new(256.0, 256.0),
        ..CameraConfig::default()
    };
    let mut c = Camera::new(cfg, W as u32, H as u32);
    c.set_zoom(zoom);
    c.set_focus(DVec2::new(focus.0, focus.1));
    c
}

fn build(inst: &mut Instancer, cam: &Camera, blend: Blend) -> InstanceStats {
    let vp = cam.view_proj();
    let frustum = Frustum::from_view_proj(vp);
    let eye = EyeView::from_view_proj(vp, H);
    inst.build_frame(&FrameView::new(&frustum, 1.0, eye, blend, 1.0), |_, _| 0.0)
}

#[test]
fn fourteen_models_are_one_draw_per_part_per_level() {
    // All 14 kinds, every level: units of every kind near and far.
    let motions: Vec<Motion> = (0..14 * 30)
        .map(|i| {
            let kind = (i % 14) as u8;
            let row = (i / 14) as f32;
            unit(
                kind,
                (i % 4) as u8,
                i as u32,
                (8.0 + kind as f32 * 17.0, 8.0 + row * 8.0),
                0,
            )
        })
        .collect();
    let mut inst = with_pack(&motions);
    // Finest level everywhere: one draw per part of each model.
    let frustum = Frustum::from_view_proj(camera(1.0, (128.0, 128.0)).view_proj());
    let s = inst.build_frame(&FrameView::models_only(&frustum, 1.0), |_, _| 0.0);
    assert_eq!(s.models, motions.len() as u32);
    assert!(s.part_instances >= s.models && s.part_instances <= 3 * s.models);
    // 7 models with hull, turret and barrel + 7 with a hull only.
    assert_eq!(s.model_draw_calls, 7 * 3 + 7);
    assert_eq!(s.part_instances, 30 * (7 * 3 + 7));
    // Triangles: every instance's part mesh counted.
    let lod0: u64 = inst
        .models()
        .models()
        .iter()
        .map(|c| {
            c.parts(0)
                .iter()
                .map(|p| u64::from(inst.models().mesh_triangles(p.mesh)))
                .sum::<u64>()
        })
        .sum();
    assert_eq!(s.model_triangles, lod0 * 30);
    // A low camera over the whole crowd uses all three levels: at most one
    // draw per (model, level, part) with geometry = 83, every list one draw.
    let mut cam = camera(0.45, (128.0, 128.0));
    cam.set_focus(DVec2::new(120.0, 30.0));
    let s = build(&mut inst, &cam, Blend::MODELS);
    let used = (0..inst.models().mesh_count())
        .filter(|&m| !inst.mesh_instances(MeshId(m as u16)).is_empty())
        .count() as u32;
    assert_eq!(s.model_draw_calls, used);
    assert!(s.model_draw_calls <= 83);
    assert!(s.per_lod[0] > 0 && s.per_lod[1] + s.per_lod[2] > 0, "{s:?}");
    assert!(s.part_instances >= s.models && s.part_instances <= 3 * s.models);
}

#[test]
fn picking_uses_the_real_bounds_of_the_model() {
    // The vehicle factory (kind 12) is about 14 m long, 11 m wide: a click
    // 6 m in front of its centre hits it, 12 m in front does not.
    let at = (128.0, 128.0);
    let mut inst = with_pack(&[unit(12, 1, 7, at, 0)]);
    let cam = camera(0.3, (128.0, 128.0));
    build(&mut inst, &cam, Blend::MODELS);
    let vp = cam.view_proj();
    let query = |dx: f32| {
        let (px, _) =
            crate::pick::project_px(&vp, Vec3::new(at.0 + dx, 0.0, at.1), [W, H]).unwrap();
        PickQuery::new(px, vp, [W, H])
    };
    let hit = inst.pick(&query(6.0)).expect("inside the factory's box");
    assert_eq!(hit.id, UnitId::from_parts(7, 0));
    assert_eq!(hit.via, PickVia::Model);
    assert!(inst.pick(&query(12.0)).is_none(), "outside the box");
    // A placeholder 1.2 m cube at the same spot would not be hit at 6 m.
    let mut cube = Instancer::new(Style::embedded(), (256.0, 256.0));
    cube.update_tick([unit(0, 1, 7, at, 0)]);
    build(&mut cube, &cam, Blend::MODELS);
    assert!(cube.pick(&query(6.0)).is_none());
}

#[test]
fn turrets_turn_towards_the_aim_point_and_highlight_tints_every_part() {
    let at = (100.0, 100.0);
    // A battle tank facing +X; the aim point straight to its right (+Z).
    let mut inst = with_pack(&[unit(3, 0, 1, at, 0)]);
    inst.set_aim(Some([at.0, at.1 + 50.0]));
    let frustum = Frustum::from_view_proj(camera(0.0, (100.0, 100.0)).view_proj());
    inst.build_frame(&FrameView::models_only(&frustum, 1.0), |_, _| 0.0);
    let chain = inst.models().model(3).clone();
    let parts = chain.parts(0);
    let hull = inst.mesh_instances(parts[0].mesh)[0];
    let turret = inst.mesh_instances(parts[1].mesh)[0];
    let barrel = inst.mesh_instances(parts[2].mesh)[0];
    let x_axis = |m: [[f32; 4]; 4]| Vec3::new(m[0][0], m[0][1], m[0][2]);
    assert!(
        (x_axis(hull.model) - Vec3::X).length() < 1e-5,
        "hull faces +X"
    );
    assert!(
        (x_axis(turret.model) - Vec3::Z).length() < 1e-4,
        "turret faces the aim"
    );
    // The barrel points at the aim and is raised a little.
    let b = x_axis(barrel.model);
    assert!(b.z > 0.9 && b.y > 0.0 && b.x.abs() < 1e-4, "{b}");
    // Highlight: every part of the hovered unit is tinted, not only the hull.
    let cam = camera(0.0, (100.0, 100.0));
    build(&mut inst, &cam, Blend::MODELS);
    let vp = cam.view_proj();
    let (px, _) = crate::pick::project_px(&vp, Vec3::new(at.0, 1.0, at.1), [W, H]).unwrap();
    let pick = inst.pick(&PickQuery::new(px, vp, [W, H])).unwrap();
    let before: Vec<[f32; 4]> = parts
        .iter()
        .map(|p| inst.mesh_instances(p.mesh)[0].color)
        .collect();
    inst.highlight(&pick);
    for (p, old) in parts.iter().zip(before) {
        let new = inst.mesh_instances(p.mesh)[0].color;
        assert!(new[0] > old[0] && new[3] == old[3], "{old:?} -> {new:?}");
    }
}

/// A tank whose turret the simulation turns (WP4.5): the `turret` part's yaw joint is the
/// turret's facing in the world minus the hull's, interpolated between the ticks, and the
/// demo aim point does not touch it.
#[test]
fn a_simulated_turret_drives_the_yaw_joint_interpolated_between_ticks() {
    let at = (100.0, 100.0);
    let turret_axes = |hull: (u8, u8), turret: (u8, u8), alpha: f32| {
        let p = Vec2::new(metres_to_cells(at.0), metres_to_cells(at.1));
        let motion = Motion::from_fixed(3, 0, p, p, Facing::new(hull.0), Facing::new(hull.1))
            .with_turret(Facing::new(turret.0), Facing::new(turret.1))
            .with_id(UnitId::from_parts(1, 0));
        let mut inst = with_pack(&[motion]);
        // An aim point that would send the turret the other way, were it listened to.
        inst.set_aim(Some([at.0 - 50.0, at.1]));
        let frustum = Frustum::from_view_proj(camera(0.0, (100.0, 100.0)).view_proj());
        inst.build_frame(&FrameView::models_only(&frustum, alpha), |_, _| 0.0);
        let chain = inst.models().model(3).clone();
        let parts = chain.parts(0);
        let x_axis = |m: [[f32; 4]; 4]| Vec3::new(m[0][0], m[0][1], m[0][2]);
        (
            x_axis(inst.mesh_instances(parts[0].mesh)[0].model),
            x_axis(inst.mesh_instances(parts[1].mesh)[0].model),
        )
    };
    let near = |a: Vec3, b: Vec3| (a - b).length() < 1e-4;
    // Hull facing +X, turret a quarter turn round to +Z: at the end of the tick.
    let (hull, turret) = turret_axes((0, 0), (0, 64), 1.0);
    assert!(
        near(hull, Vec3::X) && near(turret, Vec3::Z),
        "{hull} {turret}"
    );
    // At the start of the tick it is still on the hull, and half way it is half way.
    let (_, turret) = turret_axes((0, 0), (0, 64), 0.0);
    assert!(near(turret, Vec3::X), "{turret}");
    let (_, turret) = turret_axes((0, 0), (0, 64), 0.5);
    let half = std::f32::consts::FRAC_1_SQRT_2;
    assert!(near(turret, Vec3::new(half, 0.0, half)), "{turret}");
    // The turret's facing is the world's, not the hull's: the hull turns a quarter
    // while the turret stays on +X, and the part turns back against the hull.
    let (hull, turret) = turret_axes((0, 64), (0, 0), 1.0);
    assert!(
        near(hull, Vec3::Z) && near(turret, Vec3::X),
        "{hull} {turret}"
    );
    // The shortest way round the wrap: 250 to 6 passes 0 half way, not 128.
    let (_, turret) = turret_axes((0, 0), (250, 6), 0.5);
    assert!(near(turret, Vec3::X), "{turret}");
}

/// The WP1.10 numbers: 5,000 test-pack units at close, mid and strategic
/// zoom. Run with `cargo test --release -p sf_render testpack_5000 -- --nocapture`.
#[test]
fn testpack_5000_units_report() {
    let mut rng = 0x5742_0110_u32;
    let mut next = || {
        rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (rng >> 8) as f32 / 16_777_216.0
    };
    let motions: Vec<Motion> = (0..5000)
        .map(|i| {
            let (x, z) = (next() * 256.0, next() * 256.0);
            unit(
                (i % 14) as u8,
                ((i / 14) % 4) as u8,
                i,
                (x, z),
                (i * 37 % 256) as u8,
            )
        })
        .collect();
    let mut inst = with_pack(&motions);
    inst.set_aim(Some([128.0, 128.0]));
    let icons = *inst.icon_config();
    for (name, zoom) in [("close", 0.15), ("mid", 0.5), ("strategic", 1.0)] {
        let cam = camera(zoom, (128.0, 128.0));
        let vp = cam.view_proj();
        let eye = EyeView::from_view_proj(vp, H);
        let blend = icons.blend_for_view(
            eye.focal_px,
            cam.distance() as f32,
            cam.max_distance() as f32,
            1.0,
        );
        let frustum = Frustum::from_view_proj(vp);
        build(&mut inst, &cam, blend);
        let rounds = 20;
        let started = Instant::now();
        let mut s = InstanceStats::default();
        for _ in 0..rounds {
            s = inst.build_frame(&FrameView::new(&frustum, 0.5, eye, blend, 1.0), |_, _| 0.0);
        }
        let ms = started.elapsed().as_secs_f64() * 1000.0 / f64::from(rounds);
        println!(
            "testpack 5000 {name}: visible {} models {} icons {} per_lod {:?} triangles {} draw calls {} (models {} + icons {}) build {ms:.2} ms",
            s.visible,
            s.models,
            s.icons,
            &s.per_lod[..3],
            s.model_triangles,
            s.draw_calls(),
            s.model_draw_calls,
            s.icon_draw_calls,
        );
        assert!(s.model_draw_calls <= 83);
        assert!(s.part_instances >= s.models && s.part_instances <= 3 * s.models);
    }
}
