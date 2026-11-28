//! The cost of the walk cycle on the CPU frame build (WP11.6i fix round), measured, not pinned:
//! 200 riflemen moving across a 128 m map, `Instancer::build_frame` timed over 300 frames, for
//! the set v5 rifleman (six parts, walking), the set v3 rifleman (one body, no walk) and the set
//! v5 light tank (hull, turret, barrel), in alternating rounds. Run with
//! `cargo test --release -p sf_render --test walk_bench -- --ignored --nocapture`.

use std::path::PathBuf;
use std::time::Instant;

use glam::Vec3;
use sf_math::render_bridge::metres_to_cells;
use sf_math::{Facing, Vec2};
use sf_render::glb::{self, AxisConvention};
use sf_render::{FrameView, Frustum, Instancer, Motion, Style, UnitId};

const UNITS: u32 = 200;
const FRAMES: u32 = 300;

fn load(path: &str) -> glb::LoadedModel {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets")
        .join(path);
    glb::load_glb_file(&p, &AxisConvention::ENGINE).unwrap()
}

/// Microseconds per frame (median of `FRAMES`) and part instances per frame, for shape `shape`.
fn run(set: &sf_render::ModelSet, shape: u8) -> (f64, u32) {
    let mut inst = Instancer::new(Style::embedded(), (128.0, 128.0));
    inst.set_models(set.clone());
    let vp = glam::camera::rh::proj::directx::orthographic(-70.0, 70.0, -70.0, 70.0, -200.0, 200.0)
        * glam::camera::rh::view::look_at_mat4(
            Vec3::new(64.0, 100.0, 64.0),
            Vec3::new(64.0, 0.0, 64.0),
            -Vec3::Z,
        );
    let f = Frustum::from_view_proj(vp);
    let at = |i: u32, t: u32| {
        let x = 8.0 + (i % 20) as f32 * 5.5 + t as f32 * 0.14;
        let z = 8.0 + (i / 20) as f32 * 11.0;
        Vec2::new(metres_to_cells(x), metres_to_cells(z))
    };
    let mut times = Vec::new();
    let mut parts = 0;
    for tick in 0..FRAMES / 3 {
        inst.update_tick((0..UNITS).map(|i| {
            Motion::from_fixed(
                shape,
                0,
                at(i, tick),
                at(i, tick + 1),
                Facing::new(0),
                Facing::new(0),
            )
            .with_id(UnitId::from_parts(i, 0))
        }));
        for k in 0..3 {
            let t = Instant::now();
            let s = inst.build_frame(&FrameView::models_only(&f, k as f32 / 3.0), |_, _| 0.0);
            times.push(t.elapsed().as_secs_f64() * 1e6);
            parts = s.part_instances;
        }
    }
    times.sort_by(f64::total_cmp);
    (times[times.len() / 2], parts)
}

#[test]
#[ignore = "a measurement: run in release with --ignored --nocapture"]
fn the_walk_cost_on_the_frame_build() {
    let set = glb::model_set(
        &[
            load("set_v5/meridian_rifleman.glb"),
            load("set_v3/meridian_rifleman.glb"),
            load("set_v5/meridian_light_tank.glb"),
        ],
        &[60.0, 30.0],
    )
    .unwrap();
    assert!(set.model(0).walks() && !set.model(1).walks());
    let names = [
        "v5 rifleman (walks)",
        "v3 rifleman (one body)",
        "v5 light tank",
    ];
    let mut all = vec![Vec::new(); 3];
    for _round in 0..5 {
        for (shape, list) in all.iter_mut().enumerate() {
            list.push(run(&set, shape as u8));
        }
    }
    for (name, list) in names.iter().zip(&all) {
        let us: Vec<String> = list.iter().map(|r| format!("{:.1}", r.0)).collect();
        println!(
            "{name}: {} part instances, median µs per frame per round: {}",
            list[0].1,
            us.join(" ")
        );
    }
}
