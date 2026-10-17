//! Screenshot tests for instancing (WP1.6): placeholder shapes, player
//! colors, facing, interpolation between two ticks, tile culling and the
//! 10,000-instance stress scenes, through the real camera.
//!
//! Content checks (what is visible, which colors, where) do not depend on a
//! golden; the 10k scene also has a golden image. Same adapter rules as
//! `screenshot.rs`: software adapter preferred, no adapter = failure, and
//! on CI (`CI` set) a missing software adapter is not masked. Regenerate the
//! golden with `SF_UPDATE_GOLDEN=1 cargo test -p sf_render --test instancing_screenshot`.

use std::path::PathBuf;

use glam::{DVec4, Vec3};
use sf_data::map::{Map, generate::generate};
use sf_math::render_bridge::metres_to_cells;
use sf_math::{Facing, Vec2};
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8, Tolerance, compare};
use sf_render::scene::MeshKind;
use sf_render::{
    Camera, CameraConfig, Frustum, GpuOptions, InstanceStats, Instancer, Motion, Renderer, Style,
};

const W: u32 = 320;
const H: u32 = 180;

/// The 10k scene has thousands of small shapes, so a bigger share of pixels
/// sits on edges where WARP and lavapipe may round differently than for the
/// terrain goldens (12/255 per channel, 2 %).
const TOLERANCE: Tolerance = Tolerance {
    channel: 16,
    max_ratio: 0.04,
};

fn test_map() -> Map {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps/test64");
    Map::load(&dir).expect("committed test map loads")
}

fn renderer(map: &Map) -> Renderer {
    let mut r = Renderer::headless(W, H, &GpuOptions::software())
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
        .unwrap_or_else(|e| panic!("NO GPU ADAPTER AVAILABLE: {e}"));
    r.set_scene(&[]);
    r.set_terrain(map);
    r
}

fn camera(map: &Map, zoom: f64, focus: (f64, f64)) -> Camera {
    let cfg = CameraConfig {
        map_size: DVec2::new(f64::from(map.width()), f64::from(map.height())),
        ..CameraConfig::default()
    };
    let mut c = Camera::new(cfg, W, H);
    c.set_zoom(zoom);
    c.set_focus(DVec2::new(focus.0, focus.1));
    c
}

fn instancer(map: &Map) -> Instancer {
    let mut inst = Instancer::new(Style::embedded(), (map.width() as f32, map.height() as f32));
    let heights = (0..=i64::from(map.height()))
        .flat_map(|z| (0..=i64::from(map.width())).map(move |x| (x, z)))
        .map(|(x, z)| map.vertex_height(x, z));
    let (lo, hi) = heights.fold((f32::MAX, f32::MIN), |(lo, hi), h| (lo.min(h), hi.max(h)));
    inst.set_height_range(lo, hi);
    inst
}

fn fx2(x: f32, z: f32) -> Vec2 {
    Vec2::new(metres_to_cells(x), metres_to_cells(z))
}

/// Cull, interpolate, upload and render one frame.
fn shot(
    r: &mut Renderer,
    inst: &mut Instancer,
    map: &Map,
    cam: &Camera,
    alpha: f32,
) -> (Rgba8, InstanceStats) {
    r.set_view_proj(cam.view_proj());
    let stats = inst.build(&Frustum::from_view_proj(cam.view_proj()), alpha, |x, z| {
        map.height_at(x, z)
    });
    for kind in MeshKind::ALL {
        r.set_instances(kind, inst.instances(kind));
    }
    (r.render_to_image().expect("render"), stats)
}

/// Pixels that differ from `base` (units only; the terrain is identical).
fn mask(img: &Rgba8, base: &Rgba8) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for y in 0..img.height {
        for x in 0..img.width {
            let (a, b) = (img.pixel(x, y), base.pixel(x, y));
            if (0..3).any(|i| a[i].abs_diff(b[i]) > 12) {
                out.push((x, y));
            }
        }
    }
    out
}

/// The palette index whose hue (color divided by its brightest channel)
/// is closest to `pixel`'s, or `None` if none is close: shading scales the
/// brightness, not the hue.
fn palette_match(style: &Style, pixel: [u8; 4]) -> Option<u8> {
    let chroma = |c: [u8; 3]| {
        let max = f32::from(*c.iter().max().unwrap()).max(1.0);
        c.map(|v| f32::from(v) / max)
    };
    let p = chroma([pixel[0], pixel[1], pixel[2]]);
    (0..style.color_count() as u8)
        .map(|i| {
            let q = chroma(style.srgb(i));
            let d = (0..3).map(|k| (p[k] - q[k]).powi(2)).sum::<f32>().sqrt();
            (i, d)
        })
        .filter(|&(_, d)| d < 0.3)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

fn colors_in(style: &Style, img: &Rgba8, px: &[(u32, u32)]) -> Vec<usize> {
    let mut counts = vec![0; style.color_count()];
    for &(x, y) in px {
        if let Some(i) = palette_match(style, img.pixel(x, y)) {
            counts[usize::from(i)] += 1;
        }
    }
    counts
}

fn centroid(px: &[(u32, u32)]) -> (f64, f64) {
    let n = px.len() as f64;
    (
        px.iter().map(|p| f64::from(p.0)).sum::<f64>() / n,
        px.iter().map(|p| f64::from(p.1)).sum::<f64>() / n,
    )
}

fn project(c: &Camera, p: Vec3) -> (f64, f64) {
    let clip = c.view_proj_f64() * DVec4::new(f64::from(p.x), f64::from(p.y), f64::from(p.z), 1.0);
    (
        (clip.x / clip.w * 0.5 + 0.5) * f64::from(W),
        (0.5 - clip.y / clip.w * 0.5) * f64::from(H),
    )
}

fn check_golden(name: &str, actual: &Rgba8) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("SF_UPDATE_GOLDEN").is_some_and(|v| v == "1") {
        image::write_png(&path, actual).expect("write golden");
        return;
    }
    let expected = image::read_png(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {} ({e}); use SF_UPDATE_GOLDEN=1",
            path.display()
        )
    });
    let diff = compare(actual, &expected, TOLERANCE.channel).expect("same size");
    println!(
        "{name}: ratio {:.4}, max channel {}",
        diff.ratio(),
        diff.max_channel
    );
    if !diff.within(TOLERANCE) {
        let out = std::env::temp_dir().join(format!("actual_{name}"));
        image::write_png(&out, actual).ok();
        panic!(
            "{name} differs from golden: ratio {:.4} (actual in {})",
            diff.ratio(),
            out.display()
        );
    }
}

/// One unit of `shape` (style index) and `player` at the focus point.
fn single(shape: u8, player: u8, facing: u8) -> Motion {
    let at = fx2(32.0, 32.0);
    Motion::from_fixed(
        shape,
        player,
        at,
        at,
        Facing::new(facing),
        Facing::new(facing),
    )
}

#[test]
fn every_shape_and_player_color_renders_distinctly() {
    let map = test_map();
    let style = Style::embedded();
    let mut r = renderer(&map);
    let mut inst = instancer(&map);
    let cam = camera(&map, 0.05, (32.0, 32.0));
    inst.update_tick([]);
    let (base, stats) = shot(&mut r, &mut inst, &map, &cam, 0.0);
    assert_eq!(stats.visible, 0);

    let mut areas = [[0usize; 4]; 3];
    let mut masks = Vec::new();
    for shape in 0..3u8 {
        for player in 0..4u8 {
            inst.update_tick([single(shape, player, 0)]);
            let (img, stats) = shot(&mut r, &mut inst, &map, &cam, 0.0);
            assert_eq!(stats.visible, 1);
            assert_eq!(stats.draw_calls(), 1);
            let px = mask(&img, &base);
            let counts = colors_in(&style, &img, &px);
            let own = counts[usize::from(player)];
            println!(
                "shape {shape} ({:?}) player {player} ({}): {} px, own color {own}",
                style.shape(shape).mesh,
                style.color_name(player),
                px.len()
            );
            assert!(px.len() > 100, "unit is nearly invisible: {}", px.len());
            // Nearly all of the unit's pixels have its player's hue.
            assert!(
                own * 10 > px.len() * 9,
                "player {player}: {own} of {} px in its color ({counts:?})",
                px.len()
            );
            areas[usize::from(shape)][usize::from(player)] = px.len();
            if player == 0 {
                masks.push(px);
            }
        }
    }
    // The shapes differ: cube > cylinder > cone in silhouette area (same
    // footprint, the cylinder is rounder, the cone tapers), each by > 10 %.
    let total = |s: usize| areas[s].iter().sum::<usize>() as f64;
    println!("areas per shape: {:?}", [total(0), total(1), total(2)]);
    assert!(total(0) > total(1) * 1.1, "cube vs cylinder");
    assert!(total(1) > total(2) * 1.1, "cylinder vs cone");
    // And the silhouettes are different pictures, not just different sizes.
    for (a, b) in [(0, 1), (1, 2), (0, 2)] {
        let sa: std::collections::BTreeSet<_> = masks[a].iter().collect();
        let overlap = masks[b].iter().filter(|p| sa.contains(p)).count();
        let union = sa.len() + masks[b].len() - overlap;
        assert!(
            (overlap as f64) < union as f64 * 0.9,
            "shapes {a} and {b} look alike"
        );
    }
}

#[test]
fn facing_turns_the_shape() {
    let map = test_map();
    let mut r = renderer(&map);
    let mut inst = instancer(&map);
    let cam = camera(&map, 0.05, (32.0, 32.0));
    inst.update_tick([]);
    let (base, _) = shot(&mut r, &mut inst, &map, &cam, 0.0);
    let mut masks = Vec::new();
    for facing in [0u8, 32, 64] {
        inst.update_tick([single(0, 1, facing)]);
        let (img, _) = shot(&mut r, &mut inst, &map, &cam, 0.0);
        masks.push(mask(&img, &base));
    }
    // A quarter turn of a square footprint looks the same; an eighth turn does not.
    let set = |m: &Vec<(u32, u32)>| m.iter().copied().collect::<std::collections::BTreeSet<_>>();
    let (a, b, c) = (set(&masks[0]), set(&masks[1]), set(&masks[2]));
    let diff = |x: &std::collections::BTreeSet<(u32, u32)>,
                y: &std::collections::BTreeSet<(u32, u32)>| {
        x.symmetric_difference(y).count()
    };
    println!("facing diffs: 0/32 {}, 0/64 {}", diff(&a, &b), diff(&a, &c));
    assert!(diff(&a, &b) > 80, "an eighth turn changes the outline");
    assert!(
        diff(&a, &c) < diff(&a, &b) / 2,
        "a quarter turn of a cube looks alike"
    );
}

#[test]
fn interpolation_moves_the_unit_between_the_two_ticks() {
    let map = test_map();
    let mut r = renderer(&map);
    let mut inst = instancer(&map);
    let cam = camera(&map, 0.05, (32.0, 32.0));
    inst.update_tick([]);
    let (base, _) = shot(&mut r, &mut inst, &map, &cam, 0.0);
    // Walks 4 m east during the tick; same facing.
    let facing = Facing::new(0);
    inst.update_tick([Motion::from_fixed(
        0,
        2,
        fx2(30.0, 32.0),
        fx2(34.0, 32.0),
        facing,
        facing,
    )]);
    let mut xs = Vec::new();
    for alpha in [0.0, 0.5, 1.0] {
        let (img, stats) = shot(&mut r, &mut inst, &map, &cam, alpha);
        assert_eq!(stats.visible, 1);
        let px = mask(&img, &base);
        assert!(px.len() > 100);
        xs.push(centroid(&px).0);
    }
    println!("centroid x at alpha 0 / 0.5 / 1: {xs:?}");
    // On screen the 4 m walk is a few dozen pixels; alpha 0.5 is the middle.
    let on_ground = |x: f32| Vec3::new(x, map.height_at(x, 32.0) + 0.5, 32.0);
    let a0 = project(&cam, on_ground(30.0)).0;
    let a1 = project(&cam, on_ground(34.0)).0;
    assert!((xs[0] - a0).abs() < 3.0, "alpha 0 at {} not {a0}", xs[0]);
    assert!((xs[2] - a1).abs() < 3.0, "alpha 1 at {} not {a1}", xs[2]);
    assert!(a1 - a0 > 30.0);
    assert!(
        (xs[1] - (xs[0] + xs[2]) / 2.0).abs() < 1.5,
        "alpha 0.5 is not halfway: {xs:?}"
    );
}

/// 10,000 units spread over the map on a jittered grid; shape and color
/// are spread like `sf_game` does (shape = i mod 3, player = (i / 3) mod 4).
fn crowd(n: usize, side: f32) -> Vec<Motion> {
    let per_row = (n as f32).sqrt().ceil() as usize;
    let step = side / per_row as f32;
    (0..n)
        .map(|i| {
            let jitter = |k: usize| ((i * k) % 7) as f32 / 7.0 * step * 0.8;
            let (x, z) = (
                (i % per_row) as f32 * step + jitter(5),
                (i / per_row) as f32 * step + jitter(3),
            );
            Motion::from_fixed(
                (i % 3) as u8,
                ((i / 3) % 4) as u8,
                fx2(x, z),
                fx2(x + 0.1, z + 0.1),
                Facing::new((i % 256) as u8),
                Facing::new(((i + 4) % 256) as u8),
            )
        })
        .collect()
}

fn assert_all_colors(style: &Style, img: &Rgba8, base: &Rgba8, min_each: usize) {
    let px = mask(img, base);
    let counts = colors_in(style, img, &px);
    println!("unit pixels {}, per color {counts:?}", px.len());
    for (i, &n) in counts.iter().enumerate() {
        assert!(
            n >= min_each,
            "player color {} ({}) shows {n} px, want {min_each}",
            i,
            style.color_name(i as u8)
        );
    }
}

/// Stress scene 1: 10,000 instances on the 256 m map (generated, seed 1),
/// camera at mid zoom, so culling removes part of them. Golden + content.
#[test]
fn ten_thousand_instances_on_the_big_map_are_culled_and_drawn() {
    let map = generate(256, 256, 1);
    let style = Style::embedded();
    let mut r = renderer(&map);
    let mut inst = instancer(&map);
    inst.update_tick([]);
    let cam = camera(&map, 0.45, (128.0, 128.0));
    let (base, _) = shot(&mut r, &mut inst, &map, &cam, 0.5);

    inst.update_tick(crowd(10_000, 256.0));
    let (img, stats) = shot(&mut r, &mut inst, &map, &cam, 0.5);
    println!("{stats:?}, {:?}", r.draw_stats());
    check_golden("instancing_10k_320.png", &img);

    assert_eq!(stats.total, 10_000);
    assert!(stats.visible > 500, "only {} visible", stats.visible);
    assert!(
        stats.visible < stats.total / 2,
        "culling kept {} of {}",
        stats.visible,
        stats.total
    );
    assert!(stats.tiles_visible < stats.tiles_total);
    // One draw call per mesh kind, one buffer each.
    assert_eq!(stats.draw_calls(), 3);
    assert_eq!(r.draw_stats().dynamic_draw_calls, 3);
    assert_eq!(r.draw_stats().dynamic_instances, stats.visible);
    // All three shapes were drawn, in nearly equal numbers.
    for kind in [MeshKind::Cube, MeshKind::Cylinder, MeshKind::Cone] {
        let n = stats.per_mesh[kind.index()];
        assert!(n * 4 > stats.visible, "{kind:?}: {n} of {}", stats.visible);
    }
    assert_all_colors(&style, &img, &base, 200);
}

/// Stress scene 2: all 10,000 instances in view at once (64 m map), heavy overlap.
#[test]
fn ten_thousand_instances_all_in_view() {
    let map = test_map();
    let style = Style::embedded();
    let mut r = renderer(&map);
    let mut inst = instancer(&map);
    inst.update_tick([]);
    let cam = camera(&map, 1.0, (32.0, 32.0));
    let (base, _) = shot(&mut r, &mut inst, &map, &cam, 0.0);
    inst.update_tick(crowd(10_000, 64.0));
    let (img, stats) = shot(&mut r, &mut inst, &map, &cam, 0.0);
    assert_eq!(stats.total, 10_000);
    assert_eq!(stats.visible, 10_000, "the whole map is in view");
    assert_eq!(stats.draw_calls(), 3);
    assert_all_colors(&style, &img, &base, 1000);
}
