//! WP1.11 tests: the draw-call counters, the egui overlay layer and the
//! terrain debug views (LOD tint, cell grid).
//!
//! Content checks only, no goldens: the egui text is rasterised by the
//! adapter's driver and fonts may differ between lavapipe and WARP, so the
//! tests look at *where* and *how much* changed, not at exact pixels.
//! Adapter rules as in `screenshot.rs`: software adapter preferred; on CI
//! (`CI` set) a missing one is a failure.

use std::path::PathBuf;

use glam::{Mat4, Vec3};
use sf_data::map::Map;
use sf_render::camera::DVec2;
use sf_render::image::{Rgba8, compare};
use sf_render::scene::{Instance, MeshKind, SceneObject};
use sf_render::{Camera, CameraConfig, DebugView, GpuOptions, OverlayFrame, Renderer};

const W: u32 = 320;
const H: u32 = 180;

fn renderer() -> Renderer {
    Renderer::headless(W, H, &GpuOptions::software())
        .or_else(|software_err| {
            if std::env::var_os("CI").is_some() {
                return Err(software_err);
            }
            Renderer::headless(
                W,
                H,
                &GpuOptions {
                    backends: sf_render::wgpu::Backends::PRIMARY,
                    ..GpuOptions::default()
                },
            )
        })
        .unwrap_or_else(|e| panic!("NO GPU ADAPTER AVAILABLE: {e}"))
}

fn test_map() -> Map {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps/test64");
    Map::load(&dir).expect("committed test map loads")
}

fn object(mesh: MeshKind, x: f32) -> SceneObject {
    SceneObject {
        mesh,
        transform: Mat4::from_translation(Vec3::new(x, 0.0, 0.0)),
        color: [0.5, 0.5, 0.5],
    }
}

fn instances(n: usize) -> Vec<Instance> {
    (0..n)
        .map(|i| object(MeshKind::Cube, i as f32).instance())
        .collect()
}

#[test]
fn draw_calls_are_counted_where_they_are_recorded() {
    let mut r = renderer();
    assert_eq!(r.draw_stats().total_draw_calls(), 0, "nothing rendered yet");

    // Static scene: one draw call per mesh kind used.
    r.set_scene(&[
        object(MeshKind::Cube, 0.0),
        object(MeshKind::Cube, 2.0),
        object(MeshKind::Pyramid, 4.0),
    ]);
    r.render_to_image().unwrap();
    let s = r.draw_stats();
    assert_eq!((s.scene_draw_calls, s.scene_instances), (2, 3));
    assert_eq!(s.total_draw_calls(), 2);

    // One kind fewer: one draw call fewer.
    r.set_scene(&[object(MeshKind::Cube, 0.0), object(MeshKind::Cube, 2.0)]);
    r.render_to_image().unwrap();
    let s = r.draw_stats();
    assert_eq!((s.scene_draw_calls, s.scene_instances), (1, 2));

    // Unit instances: one call per non-empty kind.
    r.set_instances(MeshKind::Cube, &instances(5));
    r.set_instances(MeshKind::Cone, &instances(3));
    r.render_to_image().unwrap();
    let s = r.draw_stats();
    assert_eq!((s.dynamic_draw_calls, s.dynamic_instances), (2, 8));
    assert_eq!(s.total_draw_calls(), 3);

    // Hiding a kind drops the counter with the draw call.
    r.set_instances(MeshKind::Cone, &[]);
    r.render_to_image().unwrap();
    let s = r.draw_stats();
    assert_eq!((s.dynamic_draw_calls, s.dynamic_instances), (1, 5));
    assert_eq!(s.total_draw_calls(), 2);

    // Terrain: one draw call per run of visible chunks with the same level
    // of detail (WP1.8), so never more than the chunks drawn. The chunk and
    // triangle counters travel with the draw counters.
    r.set_terrain(&test_map());
    r.render_to_image().unwrap();
    let s = r.draw_stats();
    assert_eq!(s.terrain_chunks_total, 4);
    assert!((1..=4).contains(&s.terrain_chunks_visible), "{s:?}");
    assert!(
        s.terrain_draw_calls >= 1 && s.terrain_draw_calls <= s.terrain_chunks_visible,
        "{s:?}"
    );
    assert!(s.terrain_triangles > 0);
    assert_eq!(
        s.terrain_chunks_per_level.iter().sum::<u32>(),
        s.terrain_chunks_visible,
        "the level histogram counts every drawn chunk once"
    );
    assert_eq!(s.total_draw_calls(), 2 + s.terrain_draw_calls);
    r.clear_terrain();
    r.render_to_image().unwrap();
    assert_eq!(r.draw_stats().terrain_draw_calls, 0);
}

/// The panels of the test: one egui window at the top left.
fn overlay_frame(ctx: &egui::Context) -> OverlayFrame {
    // `time` matters: a new egui window fades in over a fraction of a
    // second, so the second pass is a second later.
    let input = |time: f64| egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(W as f32, H as f32),
        )),
        time: Some(time),
        ..Default::default()
    };
    let ui = |ui: &mut egui::Ui| {
        egui::Window::new("panel")
            .anchor(egui::Align2::LEFT_TOP, [8.0, 8.0])
            .resizable(false)
            .fixed_size([140.0, 60.0])
            .show(ui.ctx(), |ui| {
                ui.label("frame time");
                ui.label("draw calls");
                ui.add(egui::Slider::new(&mut 0.5_f32, 0.0..=1.0));
            });
    };
    // Windows are laid out invisibly on their first pass. That pass also
    // carries the font atlas upload, which must reach the painter.
    let warm_up = ctx.run_ui(input(0.0), ui);
    let out = ctx.run_ui(input(1.0), ui);
    let mut textures_delta = warm_up.textures_delta;
    textures_delta.append(out.textures_delta);
    OverlayFrame {
        jobs: ctx.tessellate(out.shapes, out.pixels_per_point),
        textures_delta,
        pixels_per_point: out.pixels_per_point,
    }
}

/// Bounding box `(x0, y0, x1, y1)` and count of the pixels that differ.
fn changed(a: &Rgba8, b: &Rgba8) -> (Option<(u32, u32, u32, u32)>, usize) {
    let (mut bbox, mut n) = (None::<(u32, u32, u32, u32)>, 0);
    for y in 0..a.height {
        for x in 0..a.width {
            let (p, q) = (a.pixel(x, y), b.pixel(x, y));
            if p.iter().zip(&q).any(|(u, v)| u.abs_diff(*v) > 3) {
                n += 1;
                bbox = Some(bbox.map_or((x, y, x, y), |(x0, y0, x1, y1)| {
                    (x0.min(x), y0.min(y), x1.max(x), y1.max(y))
                }));
            }
        }
    }
    (bbox, n)
}

#[test]
fn the_overlay_paints_on_top_and_clearing_it_restores_the_scene() {
    let mut r = renderer();
    r.set_terrain(&test_map());
    let map = test_map();
    let camera = Camera::new(
        CameraConfig {
            map_size: DVec2::new(f64::from(map.width()), f64::from(map.height())),
            ..CameraConfig::default()
        },
        W,
        H,
    );
    r.set_view_proj(camera.view_proj());
    let base = r.render_to_image().unwrap();

    let ctx = egui::Context::default();
    r.set_overlay(overlay_frame(&ctx));
    let with = r.render_to_image().unwrap();
    let (bbox, n) = changed(&base, &with);
    let (x0, y0, x1, y1) = bbox.expect("the panel changed some pixels");
    println!("overlay changed {n} pixels in {x0},{y0}..{x1},{y1}");
    // A window with a few lines and a slider covers well over a thousand
    // pixels; it stays in the corner it is anchored to.
    assert!(n > 1500, "panel too small: {n} pixels");
    assert!(x0 >= 4 && y0 >= 4, "starts at the anchor: {x0},{y0}");
    assert!(
        x1 < W * 3 / 5 && y1 < H * 4 / 5,
        "stays in the corner: {x1},{y1}"
    );
    // The window body is an opaque dark fill: most pixels inside the box
    // are far darker than the lit terrain behind.
    let dark = (y0..=y1)
        .flat_map(|y| (x0..=x1).map(move |x| (x, y)))
        .filter(|&(x, y)| with.pixel(x, y)[..3].iter().all(|&c| c < 80))
        .count();
    let area = ((x1 - x0 + 1) * (y1 - y0 + 1)) as usize;
    assert!(dark * 2 > area, "{dark} dark of {area} pixels");

    // The overlay is not part of the draw-call counters.
    assert_eq!(r.draw_stats().dynamic_draw_calls, 0);

    // Off again: the scene is exactly what it was.
    r.clear_overlay();
    let after = r.render_to_image().unwrap();
    assert_eq!(changed(&base, &after).1, 0);
}

fn terrain_shot(r: &mut Renderer, zoom: f64, view: DebugView) -> Rgba8 {
    let map = test_map();
    let mut camera = Camera::new(
        CameraConfig {
            map_size: DVec2::new(f64::from(map.width()), f64::from(map.height())),
            ..CameraConfig::default()
        },
        W,
        H,
    );
    camera.set_zoom(zoom);
    camera.set_focus(DVec2::new(32.0, 32.0));
    r.set_view_proj(camera.view_proj());
    r.set_debug_view(view);
    r.render_to_image().unwrap()
}

#[test]
fn grid_and_lod_tint_change_only_the_terrain_and_switch_off_cleanly() {
    let mut r = renderer();
    r.set_terrain(&test_map());
    let off = terrain_shot(&mut r, 0.0, DebugView::default());

    let grid = DebugView {
        lod_tint: false,
        grid_alpha: 1.0,
    };
    let with_grid = terrain_shot(&mut r, 0.0, grid);
    let (_, n_grid) = changed(&off, &with_grid);
    let total = (W * H) as usize;
    println!("grid changed {n_grid} of {total}");
    // Zoomed in, 1 m lines darken a clear share of the ground, not all of it.
    assert!(n_grid * 100 > total * 2, "grid not visible: {n_grid}");
    assert!(
        n_grid * 100 < total * 60,
        "grid covers everything: {n_grid}"
    );

    let tint = DebugView {
        lod_tint: true,
        grid_alpha: 0.0,
    };
    let tinted = terrain_shot(&mut r, 0.0, tint);
    let (_, n_tint) = changed(&off, &tinted);
    println!("tint changed {n_tint} of {total}");
    assert!(n_tint * 100 > total * 40, "tint not visible: {n_tint}");

    // Switched off, the picture is bit-for-bit the original.
    let again = terrain_shot(&mut r, 0.0, DebugView::default());
    let diff = compare(&off, &again, 0).unwrap();
    assert_eq!(diff.differing, 0);

    // Fully zoomed out the 1 m lines are about a pixel apart: the shader
    // fades them (and the chunk borders stay) instead of turning the whole
    // ground into moire.
    let far_off = terrain_shot(&mut r, 1.0, DebugView::default());
    let far_grid = terrain_shot(&mut r, 1.0, grid);
    let (_, n_far) = changed(&far_off, &far_grid);
    println!("far grid changed {n_far} of {total}");
    assert!(n_far * 100 < total * 30, "far grid is noise: {n_far}");
}

#[test]
fn grid_opacity_fades_with_the_camera_distance() {
    let a = DebugView::grid_alpha_for_distance;
    assert_eq!(a(12.0), 1.0, "closest zoom");
    assert_eq!(a(40.0), 1.0);
    assert_eq!(a(340.0), 0.0, "whole map in view");
    let mut last = 1.0;
    for d in (40..=140).step_by(5) {
        let v = a(f64::from(d));
        assert!(v <= last, "fades monotonically at {d} m");
        last = v;
    }
    assert!(a(90.0) > 0.0 && a(90.0) < 1.0);
}
