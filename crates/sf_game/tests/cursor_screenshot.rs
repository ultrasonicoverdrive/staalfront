//! Screenshot test for the software cursors (WP3.5), through the real
//! renderer on a software adapter (Mesa lavapipe / DX12 WARP): every cursor of
//! the set drawn by `sf_ui` over a generated terrain, at UI scale 1 and 2.
//!
//! No golden image: the two software drivers differ a little in blending. The
//! checks state what has to be true instead: the cursor changes the picture at
//! its mouse position, its hotspot is the mouse position (the 1:1 picture sits
//! on the pixels the sprite says), **its dark outline is dark wherever it lies**
//! (on grass, sand, rock and water alike), it grows with the UI scale, and all
//! the cursors of a frame are one UI draw call. Set `SF_SAVE_SHOTS=<dir>` to
//! write the images for a look.
//!
//! Adapter rules as in `sf_render`'s screenshot tests: a software adapter is
//! preferred; on CI (`CI` set) a missing one is a failure, locally any adapter
//! is accepted. No adapter at all fails the test.

use std::path::PathBuf;
use std::sync::Arc;

use sf_data::map::{Map, generate::generate_cells};
use sf_game::cursor::{CursorArt, CursorKind};
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8};
use sf_render::{Camera, CameraConfig, GpuOptions, Renderer, TerrainGround};
use sf_ui::{Ui, UiConfig, UiInput, UiRenderer};

const W: u32 = 960;
const H: u32 = 540;
const CELLS: u32 = 64;
/// Cursors per row of the contact sheet, and the spacing in pixels.
const PER_ROW: usize = 8;
const SPACING: (f32, f32) = (112.0, 150.0);
const ORIGIN: (f32, f32) = (80.0, 90.0);

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
    println!("adapter: {}", r.adapter_summary());
    r.set_scene(&[]);
    r.set_terrain(map);
    r
}

fn save(name: &str, img: &Rgba8) {
    if let Some(dir) = std::env::var_os("SF_SAVE_SHOTS") {
        image::write_png(&PathBuf::from(dir).join(name), img).expect("save shot");
    }
}

fn cursor_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/ui/cursors")
}

/// The terrain from a camera that sees the river, the grass, the rock and
/// the sand of the generated map.
struct Scene {
    r: Renderer,
    ui: Ui,
    art: CursorArt,
    ur: UiRenderer,
    /// The sprites' pixels by kind, for the exact checks.
    sprites: Vec<Rgba8>,
    /// The terrain alone.
    base: Rgba8,
}

impl Scene {
    fn new() -> Self {
        let map = generate_cells(CELLS, CELLS, 1);
        let ground = Arc::new(TerrainGround::new(&map));
        let mut r = renderer(&map);
        let cfg = CameraConfig {
            map_size: DVec2::new(f64::from(map.width()), f64::from(map.height())),
            ..CameraConfig::default()
        };
        let mut cam = Camera::new(cfg, W, H);
        cam.set_ground(ground);
        cam.set_zoom(0.55);
        cam.set_focus(DVec2::new(
            f64::from(map.width()) / 2.0,
            f64::from(map.height()) / 2.0,
        ));
        r.set_view_proj(cam.view_proj());
        let mut ui = Ui::new(UiConfig::default());
        let set = sf_ui::CursorSet::load(&cursor_dir(), ui.atlas_mut()).expect("the set loads");
        let art = CursorArt::new(set).expect("every kind has a picture");
        let ur = UiRenderer::new(&r.gpu().device, r.color_format());
        let sprites = CursorKind::ALL
            .iter()
            .map(|k| image::read_png(&cursor_dir().join(format!("{}.png", k.name()))).expect("png"))
            .collect();
        let mut scene = Scene {
            r,
            ui,
            art,
            ur,
            sprites,
            base: Rgba8::new(1, 1, vec![0; 4]),
        };
        scene.base = scene.shoot(&[], 1.0).0;
        scene
    }

    /// One frame with the cursors `(kind, mouse position)` drawn last, at DPI
    /// factor `scale`. Returns the picture and the UI draw calls.
    fn shoot(&mut self, cursors: &[(CursorKind, (f32, f32))], scale: f64) -> (Rgba8, u32) {
        self.ui.begin_frame(UiInput::default(), (W, H), scale);
        self.ui.end_frame();
        for &(kind, at) in cursors {
            self.ui
                .draw_cursor(self.art.set(), self.art.sprite_of(kind), at);
        }
        let ui = &mut self.ui;
        let ur = &mut self.ur;
        let img = self
            .r
            .render_to_image_with(|t| ur.render(t, ui))
            .expect("render");
        (img, self.ur.draw_calls())
    }
}

/// Where the contact sheet puts cursor number `i`.
fn spot(i: usize) -> (f32, f32) {
    (
        ORIGIN.0 + SPACING.0 * (i % PER_ROW) as f32,
        ORIGIN.1 + SPACING.1 * (i / PER_ROW) as f32,
    )
}

fn sheet() -> Vec<(CursorKind, (f32, f32))> {
    CursorKind::ALL
        .iter()
        .enumerate()
        .map(|(i, &k)| (k, spot(i)))
        .collect()
}

fn near(a: [u8; 4], b: [u8; 4], tolerance: i32) -> bool {
    (0..3).all(|i| (i32::from(a[i]) - i32::from(b[i])).abs() <= tolerance)
}

/// Pixels that differ visibly between the two pictures inside the box of
/// `half` pixels round `at`: their count and bounding box.
fn changed(a: &Rgba8, b: &Rgba8, at: (f32, f32), half: i32) -> (usize, [i32; 4]) {
    let (cx, cy) = (at.0.round() as i32, at.1.round() as i32);
    let (mut n, mut bb) = (0, [i32::MAX, i32::MAX, i32::MIN, i32::MIN]);
    for y in (cy - half).max(0)..(cy + half).min(H as i32) {
        for x in (cx - half).max(0)..(cx + half).min(W as i32) {
            let (p, q) = (a.pixel(x as u32, y as u32), b.pixel(x as u32, y as u32));
            let d: i32 = (0..3)
                .map(|i| (i32::from(p[i]) - i32::from(q[i])).abs())
                .sum();
            if d > 24 {
                n += 1;
                bb = [bb[0].min(x), bb[1].min(y), bb[2].max(x), bb[3].max(y)];
            }
        }
    }
    (n, bb)
}

/// Every cursor over the terrain: it changes the picture at its mouse
/// position, it sits exactly where its sprite says (the hotspot is the mouse),
/// and its dark outline is dark on whatever ground it lies, so it can be read
/// on grass, sand, rock and water alike.
#[test]
fn every_cursor_is_drawn_at_its_hotspot_with_a_dark_outline_on_any_ground() {
    let mut scene = Scene::new();
    let (img, calls) = scene.shoot(&sheet(), 1.0);
    save("cursors_over_terrain.png", &img);
    save("terrain_without_cursors.png", &scene.base);
    assert_eq!(calls, 1, "all cursors of a frame are one UI draw call");

    let hotspots: Vec<(f32, f32)> = (0..CursorKind::ALL.len())
        .map(|i| {
            let sprite = scene
                .art
                .set()
                .sprite(scene.art.sprite_of(CursorKind::ALL[i]));
            sprite.expect("sprite").hotspot
        })
        .collect();
    let mut backgrounds = Vec::new();
    for (i, kind) in CursorKind::ALL.into_iter().enumerate() {
        let at = spot(i);
        let sprite = &scene.sprites[i];
        let opaque = sprite.pixels.chunks(4).filter(|p| p[3] > 200).count();
        let (n, _) = changed(&scene.base, &img, at, 24);
        assert!(
            n * 2 >= opaque,
            "{}: only {n} pixels changed, the picture has {opaque} opaque texels",
            kind.name()
        );
        // The picture is 1:1 at scale 1, with its hotspot texel on the mouse:
        // the texel (tx, ty) is the pixel (mouse - hotspot + t). Every texel
        // of the dark outline that is opaque must show in the frame.
        let (hx, hy) = (hotspots[i].0 as i32, hotspots[i].1 as i32);
        let (mut checked, mut wrong) = (0, 0);
        for ty in 0..sprite.height as i32 {
            for tx in 0..sprite.width as i32 {
                let t = sprite.pixel(tx as u32, ty as u32);
                if t[3] < 250 {
                    continue;
                }
                let x = at.0.round() as i32 - hx + tx;
                let y = at.1.round() as i32 - hy + ty;
                if t[0] < 40 && t[1] < 40 && t[2] < 40 {
                    checked += 1;
                    let got = img.pixel(x as u32, y as u32);
                    if !near(got, t, 14) {
                        wrong += 1;
                    }
                }
            }
        }
        assert!(
            checked > 40,
            "{}: only {checked} outline texels",
            kind.name()
        );
        assert!(
            wrong * 20 <= checked,
            "{}: {wrong} of {checked} outline pixels are not the dark outline",
            kind.name()
        );
        backgrounds.push(scene.base.pixel(at.0 as u32, at.1 as u32));
    }
    // The sheet lies over different ground: dark and light, green and sand.
    let luma = |p: [u8; 4]| (u32::from(p[0]) * 3 + u32::from(p[1]) * 6 + u32::from(p[2])) / 10;
    let (lo, hi) = backgrounds
        .iter()
        .map(|&p| luma(p))
        .fold((u32::MAX, 0), |(lo, hi), l| (lo.min(l), hi.max(l)));
    println!("background luma under the cursors: {lo} to {hi}");
    assert!(
        hi > lo + 20,
        "the test terrain is one flat colour ({lo}..{hi})"
    );
}

/// The cursor grows with the UI scale and stays on its hotspot.
#[test]
fn the_cursor_grows_with_the_ui_scale_and_keeps_its_hotspot() {
    let mut scene = Scene::new();
    let at = (480.0, 270.0);
    let moves = [(CursorKind::Move, at)];
    let (one, _) = scene.shoot(&moves, 1.0);
    let (two, _) = scene.shoot(&moves, 2.0);
    save("cursor_scale_1.png", &one);
    save("cursor_scale_2.png", &two);
    let (n1, b1) = changed(&scene.base, &one, at, 64);
    let (n2, b2) = changed(&scene.base, &two, at, 64);
    let (w1, w2) = (b1[2] - b1[0] + 1, b2[2] - b2[0] + 1);
    assert!(n1 > 100 && n2 > n1 * 3, "pixels {n1} -> {n2}");
    assert!(
        (w2 as f32 / w1 as f32 - 2.0).abs() < 0.2,
        "width {w1} -> {w2} px"
    );
    // The middle of the cross stays under the mouse at both sizes.
    for (b, name) in [(b1, "scale 1"), (b2, "scale 2")] {
        let mid = ((b[0] + b[2]) as f32 / 2.0, (b[1] + b[3]) as f32 / 2.0);
        assert!(
            (mid.0 - at.0).abs() <= 2.0 && (mid.1 - at.1).abs() <= 2.0,
            "{name}: the cursor is centred on {mid:?}, the mouse is at {at:?}"
        );
    }
}

/// With the mouse in a corner the cursor is cut off by the window, not moved;
/// out of the window or with no cursor drawn the picture is the terrain alone.
#[test]
fn a_cursor_at_the_window_edge_is_clipped_and_no_cursor_is_no_change() {
    let mut scene = Scene::new();
    let (corner, calls) = scene.shoot(&[(CursorKind::ScrollNw, (0.0, 0.0))], 1.0);
    assert_eq!(calls, 1);
    // Only the lower right quarter of the arrow is inside the window.
    let (n, bb) = changed(&scene.base, &corner, (0.0, 0.0), 40);
    assert!(n > 20, "{n} pixels");
    assert!(bb[0] <= 1 && bb[1] <= 1, "{bb:?}");
    // A cursor fully outside the window draws nothing and does not fail.
    let (outside, _) = scene.shoot(&[(CursorKind::Move, (-500.0, -500.0))], 1.0);
    assert_eq!(changed(&scene.base, &outside, (0.0, 0.0), 400).0, 0);
    // Nothing drawn: no UI draw call at all, the bench scene's case.
    let (plain, calls) = scene.shoot(&[], 1.0);
    assert_eq!(calls, 0, "no cursor, no UI pass");
    assert_eq!(changed(&scene.base, &plain, (480.0, 270.0), 400).0, 0);
}
