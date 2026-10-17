//! Screenshot tests for the cutscene renderer (PLAN.md section 5 rule 5): the
//! example briefing rendered offscreen on the software adapter (Mesa lavapipe
//! on Linux, DX12 WARP on Windows).
//!
//! No golden images: glyph rasterisation and blending differ a little between
//! the two software drivers, so the checks are about content that survives
//! that: the frame is not empty, the subtitle box is dark with white text
//! pixels inside it, the fade darkens the picture, the map markers are red on
//! the parchment, a PNG part shows its own pixels, a missing PNG shows the
//! magenta placeholder, and bars appear when the window has another shape.
//!
//! Set `SF_SAVE_SHOTS=<dir>` to write the images (the ones in
//! `docs/screenshots/wp12.2` were made that way).
//!
//! Adapter rules as in `sf_render`'s screenshot tests: a software adapter is
//! preferred; on CI (`CI` set) a missing one is a failure, locally any adapter
//! is accepted. No adapter at all fails the test.

use std::path::{Path, PathBuf};

use sf_cutscene::render::{CutsceneRenderer, FrameLayout, Rect};
use sf_cutscene::{Scene, SceneState, load_file, load_str};
use sf_render::image::{self, Rgba8};
use sf_render::{GpuOptions, Renderer};

fn repo_assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn example() -> Scene {
    load_file(
        &repo_assets().join("scenes/example_bridge.ron"),
        Some(&repo_assets()),
    )
    .unwrap_or_else(|e| panic!("{e}"))
    .scene
}

fn renderer(w: u32, h: u32) -> Renderer {
    let r = Renderer::headless(w, h, &GpuOptions::software())
        .or_else(|software_err| {
            if std::env::var_os("CI").is_some() {
                return Err(software_err);
            }
            Renderer::headless(
                w,
                h,
                &GpuOptions {
                    backends: sf_render::wgpu::Backends::PRIMARY,
                    ..GpuOptions::default()
                },
            )
        })
        .unwrap_or_else(|e| panic!("NO GPU ADAPTER AVAILABLE: {e}"));
    println!("adapter: {}", r.adapter_summary());
    r
}

fn save(name: &str, img: &Rgba8) {
    if let Some(dir) = std::env::var_os("SF_SAVE_SHOTS") {
        image::write_png(&Path::new(&dir).join(name), img).expect("save shot");
    }
}

/// Render `scene` at `t_ms` and return the image with the layout it used.
fn shoot(
    r: &mut Renderer,
    cutscenes: &mut CutsceneRenderer,
    scene: &Scene,
    t_ms: i32,
) -> (Rgba8, FrameLayout) {
    shoot_state(r, cutscenes, scene, &scene.sample(t_ms))
}

fn shoot_state(
    r: &mut Renderer,
    cutscenes: &mut CutsceneRenderer,
    scene: &Scene,
    state: &SceneState<'_>,
) -> (Rgba8, FrameLayout) {
    let img = r
        .render_to_image_with(|t| cutscenes.render(t, scene, state, None))
        .expect("render");
    (img, cutscenes.layout().expect("layout").clone())
}

/// The map table is (217, 201, 160), luminance 201; white text is 255.
const TEXT_LUM: u32 = 235;

/// A subtitle is really drawn inside `r`: the dark box covers more than half
/// of it (the dimmest picture behind it is far brighter than the box) and it
/// holds at least `min_text` text-white pixels, which no picture content in
/// this scene reaches (the brightest, the table, is below `TEXT_LUM`).
fn has_subtitle(img: &Rgba8, r: &Rect, min_text: usize) -> bool {
    let area = (r.w * r.h) as usize;
    let dark = count_in(img, r, |p| lum(p) < 110);
    let text = count_in(img, r, |p| lum(p) > TEXT_LUM);
    dark * 2 > area && text >= min_text
}

fn lum(p: [u8; 4]) -> u32 {
    (u32::from(p[0]) * 3 + u32::from(p[1]) * 6 + u32::from(p[2])) / 10
}

fn mean_lum(img: &Rgba8) -> f64 {
    let sum: u64 = img
        .pixels
        .chunks_exact(4)
        .map(|p| u64::from(lum([p[0], p[1], p[2], p[3]])))
        .sum();
    sum as f64 / (img.pixels.len() / 4) as f64
}

fn distinct_colours(img: &Rgba8) -> usize {
    let mut v: Vec<[u8; 4]> = img
        .pixels
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect();
    v.sort_unstable();
    v.dedup();
    v.len()
}

/// Pixels inside `r` for which `f` holds.
fn count_in(img: &Rgba8, r: &Rect, f: impl Fn([u8; 4]) -> bool) -> usize {
    let (x0, y0) = (r.x.max(0.0) as u32, r.y.max(0.0) as u32);
    let (x1, y1) = (
        (r.right().ceil() as u32).min(img.width),
        (r.bottom().ceil() as u32).min(img.height),
    );
    let mut n = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            if f(img.pixel(x, y)) {
                n += 1;
            }
        }
    }
    n
}

fn near(p: [u8; 4], q: [u8; 3], tol: i32) -> bool {
    (0..3).all(|i| (i32::from(p[i]) - i32::from(q[i])).abs() <= tol)
}

#[test]
fn example_briefing_at_three_moments() {
    let (w, h) = (1280, 720);
    let scene = example();
    let mut r = renderer(w, h);
    let gpu = r.gpu();
    let mut cs = CutsceneRenderer::new(&gpu.device, &gpu.queue, r.color_format(), repo_assets());

    // 1. Mid-dialogue: 4 s, the major's first line.
    let (talk, layout) = shoot(&mut r, &mut cs, &scene, 4000);
    save("mid_dialogue.png", &talk);
    assert_eq!((talk.width, talk.height), (w, h));
    assert!(distinct_colours(&talk) > 20, "not an empty frame");
    // The wall (#6b7a99) is behind everything, the major's torso (#3a5f8a)
    // stands left of the middle.
    assert!(count_in(&talk, &layout.viewport, |p| near(p, [0x6b, 0x7a, 0x99], 6)) > 20_000);
    assert!(count_in(&talk, &layout.viewport, |p| near(p, [0x3a, 0x5f, 0x8a], 6)) > 2_000);
    let sub = layout.subtitle.as_ref().expect("a line is up at 4 s");
    assert_eq!(sub.name, "Major Halversen");
    assert!(sub.font_px >= 22.0, "readable at 720p: {} px", sub.font_px);
    let bright = count_in(&talk, &sub.rect, |p| lum(p) > 200);
    let dark = count_in(&talk, &sub.rect, |p| lum(p) < 70);
    let area = (sub.rect.w * sub.rect.h) as usize;
    assert!(bright > 400, "text pixels in the subtitle box: {bright}");
    assert!(
        dark * 2 > area,
        "the box is a dark background: {dark} of {area}"
    );
    // Both the name (warm) and the line (white) are there.
    assert!(
        count_in(&talk, &sub.rect, |p| p[0] > 200
            && p[1] > 160
            && p[2] < 170
            && p[0] > p[2] + 60)
            > 30,
        "the speaker name is in colour"
    );
    // Above the box the picture is untouched by text: the wall is still plain.
    let strip = Rect {
        x: sub.rect.x,
        y: sub.rect.y - 30.0,
        w: sub.rect.w,
        h: 20.0,
    };
    assert_eq!(count_in(&talk, &strip, |p| lum(p) > 200), 0);

    // 2. During the fade: 31.2 s, half way to black, nobody speaks.
    let (fade, layout) = shoot(&mut r, &mut cs, &scene, 31_200);
    save("fade.png", &fade);
    assert!(layout.subtitle.is_none());
    assert!(distinct_colours(&fade) > 5, "the picture is still there");
    assert!(
        mean_lum(&fade) < mean_lum(&talk) * 0.8,
        "darker than the unfaded frame: {} vs {}",
        mean_lum(&fade),
        mean_lum(&talk)
    );
    assert!(mean_lum(&fade) > 5.0, "but not black yet");

    // 3. The map overlay: 10 s.
    let (map, layout) = shoot(&mut r, &mut cs, &scene, 10_000);
    save("map_overlay.png", &map);
    let v = layout.viewport;
    assert!(
        count_in(&map, &v, |p| near(p, [0xd9, 0xc9, 0xa0], 6)) > 150_000,
        "the map table fills the picture"
    );
    assert!(
        count_in(&map, &v, |p| near(p, [226, 72, 58], 12)) > 150,
        "pins"
    );
    assert!(
        count_in(&map, &v, |p| near(p, [200, 52, 40], 12)) > 300,
        "arrows"
    );
    let sub = layout.subtitle.expect("the second line is up at 10 s");
    assert!(
        has_subtitle(&map, &sub.rect, 400),
        "a dark box with text on the map too"
    );

    // The frames differ.
    assert_ne!(talk.pixels, map.pixels);
    assert_ne!(talk.pixels, fade.pixels);
}

#[test]
fn the_subtitle_is_readable_at_1080p() {
    let (w, h) = (1920, 1080);
    let scene = example();
    let mut r = renderer(w, h);
    let gpu = r.gpu();
    let mut cs = CutsceneRenderer::new(&gpu.device, &gpu.queue, r.color_format(), repo_assets());
    let (img, layout) = shoot(&mut r, &mut cs, &scene, 21_000);
    save("mid_dialogue_1080.png", &img);
    let sub = layout.subtitle.expect("the order is being given at 21 s");
    assert!(sub.font_px >= 34.0, "{} px", sub.font_px);
    assert!(sub.lines.len() >= 2);
    assert!(has_subtitle(&img, &sub.rect, 1500));
    // The box stays inside the picture and above its lower edge.
    assert!(sub.rect.bottom() < layout.viewport.bottom());
    assert!(sub.rect.x >= 0.0 && sub.rect.right() <= w as f32);
}

#[test]
fn another_window_shape_gets_bars_and_a_clipped_picture() {
    let (w, h) = (800, 800);
    let scene = example();
    let mut r = renderer(w, h);
    let gpu = r.gpu();
    let mut cs = CutsceneRenderer::new(&gpu.device, &gpu.queue, r.color_format(), repo_assets());
    // At 1.5 s the major stands in the tent; the room's 2400-unit wall is
    // wider than the stage and must not spill into the bars.
    let (img, layout) = shoot(&mut r, &mut cs, &scene, 1500);
    save("letterbox.png", &img);
    let v = layout.viewport;
    assert_eq!(v.w, 800.0);
    assert!(v.y > 100.0);
    assert_eq!(img.pixel(400, 10), [0, 0, 0, 255], "bar above");
    assert_eq!(img.pixel(400, 790), [0, 0, 0, 255], "bar below");
    assert!(
        near(img.pixel(400, (v.y + 20.0) as u32), [0x6b, 0x7a, 0x99], 8),
        "picture inside"
    );
}

/// The subtitle checks must be able to fail: the same frames without the
/// subtitle do not pass them (on the map the table alone is bright enough to
/// have fooled a plain brightness check).
#[test]
fn the_subtitle_checks_fail_when_no_subtitle_is_drawn() {
    let scene = example();
    let mut r = renderer(1280, 720);
    let gpu = r.gpu();
    let mut cs = CutsceneRenderer::new(&gpu.device, &gpu.queue, r.color_format(), repo_assets());
    for t in [4000, 10_000] {
        let (with, layout) = shoot(&mut r, &mut cs, &scene, t);
        let rect = layout.subtitle.expect("a line is up").rect;
        assert!(
            has_subtitle(&with, &rect, 400),
            "{t} ms: the check passes with a subtitle"
        );
        let mut state = scene.sample(t);
        state.subtitle = None;
        let (without, layout) = shoot_state(&mut r, &mut cs, &scene, &state);
        assert!(layout.subtitle.is_none());
        assert!(
            !has_subtitle(&without, &rect, 400),
            "{t} ms: the check fails without one"
        );
        assert_eq!(
            count_in(&without, &rect, |p| lum(p) > TEXT_LUM),
            0,
            "no text pixels"
        );
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sf_cutscene_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("art")).expect("temp dir");
    dir
}

#[test]
fn png_parts_show_their_pixels_and_a_missing_png_shows_a_placeholder() {
    let dir = temp_dir("png");
    // 100 x 60: left half pure red, right half pure blue.
    let (pw, ph) = (100u32, 60u32);
    let mut pixels = Vec::new();
    for _y in 0..ph {
        for x in 0..pw {
            pixels.extend_from_slice(&if x < pw / 2 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            });
        }
    }
    image::write_png(&dir.join("art/split.png"), &Rgba8::new(pw, ph, pixels)).unwrap();

    let scene = load_str(
        r##"
Scene(
    name: "png",
    stage: (width: 640, height: 360),
    layers: [
        (name: "bg", art: Rect(w: 700.0, h: 400.0, color: "#204020"), z: -5),
        (name: "split", art: Image("art/split.png"), pos: (-200.0, 0.0), z: -4),
        (name: "gone", art: Image("art/gone.png"), pos: (200.0, 0.0), z: -3),
    ],
)"##,
        None,
    )
    .expect("scene")
    .scene;

    let mut r = renderer(640, 360);
    let gpu = r.gpu();
    let mut cs = CutsceneRenderer::new(&gpu.device, &gpu.queue, r.color_format(), &dir);
    // Loading up front tells which pictures will be placeholders before any
    // frame is drawn (the viewer prints this list).
    cs.preload_pictures(&scene);
    assert_eq!(cs.failed_pictures().len(), 1);
    let (img, _) = shoot(&mut r, &mut cs, &scene, 0);
    save("png_parts.png", &img);
    // Stage unit = pixel here. The picture spans x 70..170 (centre 120), y 150..210.
    assert!(
        near(img.pixel(95, 180), [255, 0, 0], 4),
        "red half: {:?}",
        img.pixel(95, 180)
    );
    assert!(
        near(img.pixel(145, 180), [0, 0, 255], 4),
        "blue half: {:?}",
        img.pixel(145, 180)
    );
    assert!(
        near(img.pixel(120, 120), [0x20, 0x40, 0x20], 4),
        "background around it"
    );
    // The missing file: a magenta box of 128 x 128 around (520, 180).
    let p = img.pixel(520, 180);
    assert!(
        p[0] > 150 && p[2] > 150 && p[1] < 100,
        "magenta placeholder: {p:?}"
    );
    assert_eq!(cs.failed_pictures().len(), 1);
    assert_eq!(cs.failed_pictures()[0].0, "art/gone.png");
    let _ = std::fs::remove_dir_all(&dir);
}
