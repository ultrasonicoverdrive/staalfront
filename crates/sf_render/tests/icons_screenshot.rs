//! Screenshot tests for the strategic icons and the model/icon cross-fade
//! (WP1.9), through the real renderer on a software adapter.
//!
//! Content checks, no golden image: which shapes and colours are on the
//! screen, how many pixels across an icon is at two zooms and two UI scales,
//! and that the dissolve between a model and its icon is exactly
//! complementary pixel by pixel. Same adapter rules as `screenshot.rs`:
//! software adapter preferred, no adapter = failure, and on CI (`CI` set) a
//! missing software adapter is not masked.

use std::path::PathBuf;
use std::time::Instant;

use glam::Vec3;
use sf_data::map::{Map, generate::generate};
use sf_math::render_bridge::fx_from_f32;
use sf_math::{Facing, Vec2};
use sf_render::camera::DVec2;
use sf_render::icons::Blend;
use sf_render::image::Rgba8;
use sf_render::model::EyeView;
use sf_render::pick::project_px;
use sf_render::{
    Camera, CameraConfig, FrameView, Frustum, GpuOptions, Instancer, Motion, PickQuery, Renderer,
    Style, TerrainGround, UnitId,
};

const W: u32 = 640;
const H: u32 = 360;

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

fn instancer(map: &Map, style: Style) -> Instancer {
    let mut inst = Instancer::new(style, (map.width() as f32, map.height() as f32));
    let heights = (0..=i64::from(map.height()))
        .flat_map(|z| (0..=i64::from(map.width())).map(move |x| (x, z)))
        .map(|(x, z)| map.vertex_height(x, z));
    let (lo, hi) = heights.fold((f32::MAX, f32::MIN), |(lo, hi), h| (lo.min(h), hi.max(h)));
    inst.set_height_range(lo, hi);
    inst
}

fn fx2(x: f32, z: f32) -> Vec2 {
    Vec2::new(fx_from_f32(x), fx_from_f32(z))
}

fn still(shape: u8, player: u8, id: u32, at: (f32, f32)) -> Motion {
    Motion::from_fixed(
        shape,
        player,
        fx2(at.0, at.1),
        fx2(at.0, at.1),
        Facing::new(0),
        Facing::new(0),
    )
    .with_id(UnitId::from_parts(id, 0))
}

/// The blend the game would use at this camera.
fn blend_at(inst: &Instancer, cam: &Camera, ui_scale: f32) -> Blend {
    let eye = EyeView::from_view_proj(cam.view_proj(), H as f32);
    inst.icon_config().blend_for_view(
        eye.focal_px,
        cam.distance() as f32,
        cam.max_distance() as f32,
        ui_scale,
    )
}

/// Build the frame as sf_game does and upload it. `models` / `icons` can
/// switch one representation off to look at the other alone.
fn draw(
    r: &mut Renderer,
    inst: &mut Instancer,
    map: &Map,
    cam: &Camera,
    blend: Blend,
    ui_scale: f32,
    (models, icons): (bool, bool),
) -> Rgba8 {
    let vp = cam.view_proj();
    r.set_view_proj(vp);
    let frustum = Frustum::from_view_proj(vp);
    let eye = EyeView::from_view_proj(vp, H as f32);
    inst.build_frame(
        &FrameView::new(&frustum, 0.0, eye, blend, ui_scale),
        |x, z| map.height_at(x, z),
    );
    r.upload_units(inst);
    if !models {
        for id in 0..inst.models().mesh_count() {
            r.set_mesh_instances(sf_render::MeshId(id as u16), &[]);
        }
    }
    if !icons {
        r.set_icons(&[], inst.icon_params());
    }
    r.render_to_image().expect("render")
}

/// With `SF_DUMP_DIR` set, write the image there as a PNG (looking at what
/// a test saw); otherwise nothing.
fn dump(img: &Rgba8, name: &str) {
    if let Some(dir) = std::env::var_os("SF_DUMP_DIR") {
        let path = PathBuf::from(dir).join(format!("{name}.png"));
        sf_render::image::write_png(&path, img).expect("write the dump");
    }
}

fn matches_color(pixel: [u8; 4], srgb: [u8; 3], tol: u8) -> bool {
    (0..3).all(|i| pixel[i].abs_diff(srgb[i]) <= tol)
}

/// A blob of pixels of one colour: its bounding box and how much of it is filled.
#[derive(Clone, Copy, Debug)]
struct Blob {
    width: u32,
    height: u32,
    pixels: u32,
}

impl Blob {
    fn fill(&self) -> f64 {
        f64::from(self.pixels) / f64::from(self.width * self.height)
    }
}

/// Connected blobs (4-neighbourhood) of at least `min_pixels` pixels that
/// match `srgb`.
fn blobs(img: &Rgba8, srgb: [u8; 3], min_pixels: u32) -> Vec<Blob> {
    let (w, h) = (img.width as usize, img.height as usize);
    let on: Vec<bool> = (0..w * h)
        .map(|i| matches_color(img.pixel((i % w) as u32, (i / w) as u32), srgb, 10))
        .collect();
    let mut seen = vec![false; w * h];
    let mut out = Vec::new();
    for start in 0..w * h {
        if !on[start] || seen[start] {
            continue;
        }
        let mut stack = vec![start];
        seen[start] = true;
        let (mut x0, mut y0, mut x1, mut y1, mut n) = (w, h, 0, 0, 0u32);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            (x0, y0, x1, y1, n) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y), n + 1);
            for (dx, dy) in [(1_i32, 0_i32), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                let j = ny as usize * w + nx as usize;
                if on[j] && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        if n >= min_pixels {
            out.push(Blob {
                width: (x1 - x0 + 1) as u32,
                height: (y1 - y0 + 1) as u32,
                pixels: n,
            });
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Square,
    Circle,
    Triangle,
}

/// What an icon blob is, from how much of its bounding box it fills: a
/// square fills all of it, a circle a bit over three quarters, a triangle
/// about half.
fn classify(blob: &Blob) -> Shape {
    match blob.fill() {
        f if f > 0.92 => Shape::Square,
        f if f > 0.68 => Shape::Circle,
        _ => Shape::Triangle,
    }
}

/// 4 players (columns) by 3 shapes (rows), 8 m apart around the map centre.
fn grid() -> Vec<Motion> {
    let mut out = Vec::new();
    for shape in 0..3u8 {
        for player in 0..4u8 {
            let at = (
                32.0 + (f32::from(player) - 1.5) * 8.0,
                32.0 + (f32::from(shape) - 1.0) * 8.0,
            );
            out.push(still(
                shape,
                player,
                u32::from(shape) * 4 + u32::from(player),
                at,
            ));
        }
    }
    out
}

fn icon_blobs(img: &Rgba8, style: &Style) -> Vec<(u8, Blob)> {
    (0..4u8)
        .flat_map(|p| {
            blobs(img, style.srgb(p), 30)
                .into_iter()
                .map(move |b| (p, b))
        })
        .collect()
}

/// At strategic zoom every unit is an icon: all four player colours and all
/// three shapes are on the screen, each icon is the configured number of
/// pixels across, and it stays that size when the zoom changes (the models
/// shrink with the distance, which is what makes this a test: the control
/// shows the model sizes do change).
#[test]
fn strategic_zoom_shows_icons_in_all_colors_and_shapes_at_a_constant_pixel_size() {
    let map = test_map();
    let style = Style::embedded();
    let mut r = renderer(&map);
    let mut inst = instancer(&map, style.clone());
    inst.update_tick(grid());
    let size = style.icons().size_px;
    let outline = style.icons().outline_px;

    let mut sizes = Vec::new();
    for zoom in [0.85, 1.0] {
        let cam = camera(&map, zoom, (32.0, 32.0));
        let blend = blend_at(&inst, &cam, 1.0);
        assert_eq!(blend, Blend::ICONS, "zoom {zoom} is past the threshold");
        let img = draw(&mut r, &mut inst, &map, &cam, blend, 1.0, (true, true));
        dump(&img, &format!("icons_zoom_{zoom}"));
        let stats = inst.stats();
        assert_eq!(
            (stats.models, stats.icons, stats.icon_draw_calls),
            (0, 12, 1)
        );
        assert_eq!(r.draw_stats().icon_instances, 12);
        assert_eq!(r.draw_stats().icon_draw_calls, 1);
        assert_eq!(r.draw_stats().dynamic_draw_calls, 0, "no model is drawn");

        let found = icon_blobs(&img, &style);
        println!("zoom {zoom}: {} icon blobs", found.len());
        // Twelve icons: one of each shape per player colour.
        assert_eq!(found.len(), 12, "{found:?}");
        for player in 0..4u8 {
            let mut shapes: Vec<_> = found
                .iter()
                .filter(|(p, _)| *p == player)
                .map(|(_, b)| classify(b))
                .collect();
            shapes.sort_by_key(|s| *s as u8);
            assert_eq!(
                shapes,
                [Shape::Square, Shape::Circle, Shape::Triangle],
                "player {player} ({}): {found:?}",
                style.color_name(player)
            );
        }
        // The pure fill (inside the outline, which fades over one pixel) is
        // the icon minus the outline and half a pixel of fade on both sides;
        // a triangle's box is a little smaller still, and the colour match
        // takes off the blended edge pixels.
        let expected = size - 2.0 * (outline + 0.5);
        for (_, b) in &found {
            for side in [b.width, b.height] {
                assert!(
                    (expected - 3.0..=expected + 1.5).contains(&(side as f32)),
                    "icon fill {side} px, want about {expected}: {b:?}"
                );
            }
        }
        sizes.push(
            found
                .iter()
                .map(|(_, b)| f64::from(b.width + b.height) / 2.0)
                .sum::<f64>()
                / found.len() as f64,
        );
    }
    println!("mean icon side at zoom 0.85 / 1.0: {sizes:?}");
    assert!(
        (sizes[0] - sizes[1]).abs() < 0.75,
        "icons must not change size with the zoom: {sizes:?}"
    );

    // Control: the same units as models change size by more than a pixel or
    // two between the same zooms.
    let model_extent = |zoom: f64, r: &mut Renderer, inst: &mut Instancer| {
        let cam = camera(&map, zoom, (32.0, 32.0));
        let base = {
            inst.update_tick([]);
            draw(r, inst, &map, &cam, Blend::MODELS, 1.0, (true, true))
        };
        inst.update_tick(grid());
        let img = draw(r, inst, &map, &cam, Blend::MODELS, 1.0, (true, true));
        let (mut lo, mut hi) = ((u32::MAX, u32::MAX), (0, 0));
        for y in 0..H {
            for x in 0..W {
                let (a, b) = (img.pixel(x, y), base.pixel(x, y));
                if (0..3).any(|i| a[i].abs_diff(b[i]) > 12) {
                    lo = (lo.0.min(x), lo.1.min(y));
                    hi = (hi.0.max(x), hi.1.max(y));
                }
            }
        }
        f64::from(hi.0 - lo.0)
    };
    let (near, far) = (
        model_extent(0.4, &mut r, &mut inst),
        model_extent(1.0, &mut r, &mut inst),
    );
    println!("control: the crowd of models is {near} px wide at zoom 0.4, {far} px at 1.0");
    assert!(
        near > far * 1.5,
        "models must shrink with the zoom for the constant icon size to mean anything"
    );
}

/// The icon scales with the UI scale (DPI), and its click quad with it.
#[test]
fn icons_grow_with_the_ui_scale() {
    let map = test_map();
    let style = Style::embedded();
    let mut r = renderer(&map);
    let mut inst = instancer(&map, style.clone());
    inst.update_tick(grid());
    let cam = camera(&map, 1.0, (32.0, 32.0));
    let mean_side = |scale: f32, r: &mut Renderer, inst: &mut Instancer| {
        let img = draw(r, inst, &map, &cam, Blend::ICONS, scale, (true, true));
        let found = icon_blobs(&img, &style);
        assert_eq!(found.len(), 12, "scale {scale}: {found:?}");
        found
            .iter()
            .map(|(_, b)| f64::from(b.width + b.height) / 2.0)
            .sum::<f64>()
            / found.len() as f64
    };
    let (one, two) = (
        mean_side(1.0, &mut r, &mut inst),
        mean_side(2.0, &mut r, &mut inst),
    );
    println!("icon side at UI scale 1: {one:.1} px, at 2: {two:.1} px");
    assert!(two > one * 1.7 && two < one * 2.3, "{one} vs {two}");
}

fn changed(img: &Rgba8, base: &Rgba8, x: u32, y: u32) -> bool {
    let (a, b) = (img.pixel(x, y), base.pixel(x, y));
    (0..3).any(|i| a[i].abs_diff(b[i]) > 12)
}

/// One big model with its icon in front of it, at a given cross-fade weight.
/// In the middle of the icon (where the model fills the whole screen
/// area under it) a pixel is drawn by the model if the dither threshold is
/// below the weight, and by the icon otherwise: exactly one of them, for
/// every pixel and every weight. Rendered model-only and icon-only and
/// together, on the real renderer and shaders.
#[test]
fn the_cross_fade_is_exactly_complementary_in_every_pixel() {
    let map = test_map();
    let style = Style::from_ron(
        "(palette: [(name: \"red\", srgb: (214, 40, 40))],
          shapes: [(mesh: Cube, size: (6.0, 6.0, 6.0), icon: Square)])",
    )
    .unwrap();
    let mut r = renderer(&map);
    let mut inst = instancer(&map, style);
    let cam = camera(&map, 0.3, (32.0, 32.0));
    inst.update_tick([]);
    let base = draw(
        &mut r,
        &mut inst,
        &map,
        &cam,
        Blend::MODELS,
        1.0,
        (true, true),
    );
    inst.update_tick([still(0, 0, 1, (32.0, 32.0))]);

    // Where the icon is drawn: centred on the lifted anchor, snapped.
    let ground = map.height_at(32.0, 32.0);
    let lift = inst.icon_config().lift;
    let (px, _) = project_px(
        &cam.view_proj(),
        Vec3::new(32.0, ground + lift, 32.0),
        [W as f32, H as f32],
    )
    .unwrap();
    let c = (
        sf_render::icons::snap_icon_center(px[0], 14.0),
        sf_render::icons::snap_icon_center(px[1], 14.0),
    );
    // The inside of the icon, clear of its outline and antialiased edge.
    let (x0, y0) = (c.0 as u32 - 5, c.1 as u32 - 5);
    let inside = || (y0..y0 + 10).flat_map(move |y| (x0..x0 + 10).map(move |x| (x, y)));

    let mut model_share = Vec::new();
    // Besides round weights, weights exactly on a dither threshold
    // `(rank + 0.5) / 64` (exact in f32): there the pixel of that rank belongs
    // to the icon, so a `<=` in scene.wgsl (or a `>` in icon.wgsl) fails here.
    for weight in [
        0.0,
        0.5 / 64.0,
        0.125,
        0.25,
        33.0 / 128.0,
        0.5,
        65.0 / 128.0,
        0.75,
        0.875,
        127.0 / 128.0,
        1.0,
    ] {
        let blend = Blend::from_model_weight(weight);
        let model_only = draw(&mut r, &mut inst, &map, &cam, blend, 1.0, (true, false));
        let icon_only = draw(&mut r, &mut inst, &map, &cam, blend, 1.0, (false, true));
        let both = draw(&mut r, &mut inst, &map, &cam, blend, 1.0, (true, true));
        let (mut models, mut icons) = (0, 0);
        for (x, y) in inside() {
            let (m, i) = (
                changed(&model_only, &base, x, y),
                changed(&icon_only, &base, x, y),
            );
            // The shaders draw exactly the pixels the CPU function says.
            assert_eq!(m, blend.model_at(x, y), "weight {weight}: model at {x},{y}");
            assert_eq!(i, blend.icon_at(x, y), "weight {weight}: icon at {x},{y}");
            assert!(m != i, "weight {weight}: pixel {x},{y} drawn {m}/{i}");
            // Together: the pixel is whichever one drew it, never a mix.
            let expected = if m { &model_only } else { &icon_only };
            let (got, want) = (both.pixel(x, y), expected.pixel(x, y));
            assert!(
                (0..3).all(|k| got[k].abs_diff(want[k]) <= 1),
                "weight {weight}: {x},{y} is {got:?}, one layer alone gives {want:?}"
            );
            models += u32::from(m);
            icons += u32::from(i);
        }
        assert_eq!(models + icons, 100);
        println!("weight {weight}: {models} model pixels, {icons} icon pixels of 100");
        model_share.push(f64::from(models) / 100.0);
        assert!(
            (f64::from(models) / 100.0 - f64::from(weight)).abs() < 0.07,
            "the model keeps a share of the pixels equal to its weight"
        );
    }
    assert!(
        model_share.windows(2).all(|w| w[0] <= w[1]),
        "{model_share:?}"
    );
}

/// The hovered unit's icon gets a white outline, and the pick that selected
/// it finds it where it was drawn.
#[test]
fn a_highlighted_icon_has_a_white_outline_and_is_picked_where_it_is_drawn() {
    let map = test_map();
    let style = Style::embedded();
    let mut r = renderer(&map);
    let mut inst = instancer(&map, style.clone());
    let cam = camera(&map, 1.0, (32.0, 32.0));
    inst.update_tick([still(0, 1, 5, (32.0, 32.0))]);
    let vp = cam.view_proj();
    r.set_view_proj(vp);
    let frustum = Frustum::from_view_proj(vp);
    let eye = EyeView::from_view_proj(vp, H as f32);
    let ground = |x: f32, z: f32| map.height_at(x, z);
    inst.build_frame(
        &FrameView::new(&frustum, 0.0, eye, Blend::ICONS, 1.0),
        ground,
    );
    r.upload_units(&inst);
    let plain = r.render_to_image().unwrap();

    // Pick at the icon's drawn position and highlight what was found.
    let ground_y = map.height_at(32.0, 32.0);
    let (px, _) = project_px(
        &vp,
        Vec3::new(32.0, ground_y + inst.icon_config().lift, 32.0),
        [W as f32, H as f32],
    )
    .unwrap();
    let pick = inst
        .pick(&PickQuery::new(px, vp, [W as f32, H as f32]))
        .expect("the icon is under the cursor");
    assert_eq!(pick.id, UnitId::from_parts(5, 0));
    inst.highlight(&pick);
    r.upload_units(&inst);
    let lit = r.render_to_image().unwrap();

    let white = |img: &Rgba8| {
        (0..H)
            .flat_map(|y| (0..W).map(move |x| (x, y)))
            .filter(|&(x, y)| matches_color(img.pixel(x, y), [255, 255, 255], 12))
            .count()
    };
    println!(
        "white pixels: {} plain, {} highlighted",
        white(&plain),
        white(&lit)
    );
    assert!(white(&plain) < 5);
    assert!(white(&lit) > 25, "the white outline of a 14 px icon");
}

/// The numbers for the PR: draw calls and triangles with 10,000 units at
/// close, mid and strategic zoom, and the CPU time of the level selection and
/// the icon build. Printed; the assertions pin the structure, not the time.
#[test]
fn ten_thousand_units_at_close_mid_and_strategic_zoom() {
    let map = generate(256, 256, 1);
    let style = Style::embedded();
    let mut r = Renderer::headless(1920, 1080, &GpuOptions::software())
        .or_else(|e| {
            if std::env::var_os("CI").is_some() {
                return Err(e);
            }
            Renderer::headless(
                1920,
                1080,
                &GpuOptions {
                    backends: sf_render::wgpu::Backends::PRIMARY,
                    ..GpuOptions::default()
                },
            )
        })
        .unwrap_or_else(|e| panic!("NO GPU ADAPTER AVAILABLE: {e}"));
    r.set_scene(&[]);
    r.set_terrain(&map);
    let mut inst = instancer(&map, style);
    // 10,000 units on a jittered grid over the whole map.
    let per_row = 100usize;
    let step = 256.0 / per_row as f32;
    let motions: Vec<_> = (0..10_000usize)
        .map(|i| {
            let jitter = |k: usize| ((i * k) % 7) as f32 / 7.0 * step * 0.8;
            let at = (
                (i % per_row) as f32 * step + jitter(5),
                (i / per_row) as f32 * step + jitter(3),
            );
            still((i % 3) as u8, ((i / 3) % 4) as u8, i as u32, at)
        })
        .collect();
    inst.update_tick(motions);
    println!(
        "zoom   | visible | models (L0/L1/L2) | icons | draw calls (units+icons) | unit triangles | build ms (min of 20)"
    );
    let mut rows = Vec::new();
    for (name, zoom, focus) in [
        ("close", 0.0, (128.0, 128.0)),
        ("mid", 0.5, (128.0, 128.0)),
        ("strategic", 1.0, (128.0, 128.0)),
    ] {
        let mut cam = Camera::new(
            CameraConfig {
                map_size: DVec2::new(256.0, 256.0),
                ..CameraConfig::default()
            },
            1920,
            1080,
        );
        cam.set_zoom(zoom);
        cam.set_focus(DVec2::new(focus.0, focus.1));
        let vp = cam.view_proj();
        r.set_view_proj(vp);
        let frustum = Frustum::from_view_proj(vp);
        let eye = EyeView::from_view_proj(vp, 1080.0);
        let blend = inst.icon_config().blend_for_view(
            eye.focal_px,
            cam.distance() as f32,
            cam.max_distance() as f32,
            1.0,
        );
        // The picture before WP1.9 for comparison: every unit a level-0
        // model, whatever the zoom.
        let mut base_ms = f64::MAX;
        let mut base = Default::default();
        for _ in 0..20 {
            let started = Instant::now();
            base = inst.build(&frustum, 0.5, |x, z| map.height_at(x, z));
            base_ms = base_ms.min(started.elapsed().as_secs_f64() * 1e3);
        }
        let mut best = f64::MAX;
        let mut stats = Default::default();
        for _ in 0..20 {
            let started = Instant::now();
            stats = inst.build_frame(&FrameView::new(&frustum, 0.5, eye, blend, 1.0), |x, z| {
                map.height_at(x, z)
            });
            best = best.min(started.elapsed().as_secs_f64() * 1e3);
        }
        r.upload_units(&inst);
        r.render_to_image().expect("render");
        let draw = r.draw_stats();
        println!(
            "{name:9} | {:7} | {:6} ({}/{}/{}) | {:5} | {:2} + {} | {:14} | {best:.3}",
            stats.visible,
            stats.models,
            stats.per_lod[0],
            stats.per_lod[1],
            stats.per_lod[2],
            stats.icons,
            draw.dynamic_draw_calls,
            draw.icon_draw_calls,
            draw.dynamic_triangles,
        );
        // Picking at the middle of the screen (with the terrain ray, as the game does).
        let ground = TerrainGround::new(&map);
        let mut pick_ms = f64::MAX;
        for _ in 0..20 {
            let started = Instant::now();
            let query = PickQuery::new([960.0, 540.0], vp, [1920.0, 1080.0]).with_ground(&ground);
            let _ = inst.pick(&query);
            pick_ms = pick_ms.min(started.elapsed().as_secs_f64() * 1e3);
        }
        println!(
            "          |         | before WP1.9, level-0 models only: {} draw calls, {} triangles, {base_ms:.3} ms; pick incl. terrain ray {pick_ms:.3} ms",
            base.model_draw_calls, base.model_triangles,
        );
        assert!(stats.model_triangles <= base.model_triangles);
        // What was counted at the draw calls is what the instancer built.
        assert_eq!(draw.dynamic_draw_calls, stats.model_draw_calls);
        assert_eq!(draw.icon_draw_calls, stats.icon_draw_calls);
        assert_eq!(u64::from(draw.dynamic_triangles), stats.model_triangles);
        assert_eq!(draw.dynamic_instances, stats.models);
        assert_eq!(draw.icon_instances, stats.icons);
        rows.push((name, blend, stats, draw));
    }
    let (close, mid, strategic) = (&rows[0], &rows[1], &rows[2]);
    // Close: only models, at most seven draws (1 + 3 + 3 lists), the finest levels.
    assert_eq!(close.1, Blend::MODELS);
    assert_eq!(close.2.icons, 0);
    assert!(close.3.dynamic_draw_calls <= 7);
    assert!(close.2.per_lod[0] > 0);
    // Strategic: icons only, one draw call, not a single model triangle.
    assert_eq!(strategic.1, Blend::ICONS);
    assert_eq!(strategic.3.dynamic_draw_calls, 0);
    assert_eq!(strategic.3.dynamic_triangles, 0);
    assert_eq!(strategic.3.icon_draw_calls, 1);
    assert_eq!(strategic.2.icons, 10_000);
    // Mid zoom: still models, and the coarser levels are in use.
    assert!(mid.2.models > 0);
    assert!(
        mid.2.per_lod[1] + mid.2.per_lod[2] > 0,
        "{:?}",
        mid.2.per_lod
    );
}
