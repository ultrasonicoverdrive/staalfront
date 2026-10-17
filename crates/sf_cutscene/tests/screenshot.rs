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

use sf_cutscene::preview::{Applied, Preview, Source};
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

/// WP12.6: the live preview's text over the picture, drawn for real: the red
/// error box with the `file:line:col` message, the amber list of pictures that
/// did not load, and the writer's HUD. The same frame without panels has none
/// of those pixels in that corner.
#[test]
fn the_error_overlay_and_the_writer_hud_are_drawn_over_the_picture() {
    use sf_cutscene::render::Hud;

    let (w, h) = (1280, 720);
    let scene = example();
    let text = std::fs::read_to_string(repo_assets().join("scenes/example_bridge.ron")).unwrap();
    let source = Source::Scene(PathBuf::from("scenes/f1_m01.ron"));
    let first = source.load_text(&text, &repo_assets());
    let mut preview = Preview::new(source.clone(), repo_assets(), first, true);
    preview.player_mut().seek(4000);
    let broken = text.replacen("pos:", "pos: (", 1);
    assert_eq!(
        preview.apply(source.load_text(&broken, &repo_assets())),
        Applied::Rejected
    );
    let pictures = vec![(
        "art/halversen/head.png".to_string(),
        "file not found".to_string(),
    )];
    let panels = preview.panels(&pictures);
    assert_eq!(panels.len(), 3, "errors, pictures, HUD");

    let mut r = renderer(w, h);
    let gpu = r.gpu();
    let mut cs = CutsceneRenderer::new(&gpu.device, &gpu.queue, r.color_format(), repo_assets());
    let state = scene.sample(4000);
    let mut frame = |panels: Vec<sf_cutscene::render::HudPanel>| {
        let hud = Hud {
            progress: 0.1,
            panels,
            ..Hud::default()
        };
        r.render_to_image_with(|t| cs.render(t, &scene, &state, Some(&hud)))
            .expect("render")
    };
    let with = frame(panels);
    let without = frame(Vec::new());
    save("overlay_error.png", &with);

    let corner = Rect {
        x: 0.0,
        y: 0.0,
        w: 700.0,
        h: 330.0,
    };
    // The error box: dark red, with light-red text (255, 160, 150) on it.
    // Dark and reddish with a little blue in it (the placeholder figures'
    // dark red mouths have none), (63, 34, 44) on lavapipe.
    let red_box = |p: [u8; 4]| {
        let (r, g, b) = (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
        r > g + 15 && b > g + 3 && r > b + 8 && lum(p) < 100
    };
    let red_text =
        |p: [u8; 4]| p[0] > 235 && (130..190).contains(&p[1]) && p[2] > 120 && p[2] < 190;
    assert!(count_in(&with, &corner, red_box) > 5_000);
    assert!(count_in(&with, &corner, red_text) > 100);
    assert_eq!(count_in(&without, &corner, red_box), 0);
    assert_eq!(count_in(&without, &corner, red_text), 0);
    // The amber list and the white HUD text.
    let amber = |p: [u8; 4]| p[0] > 235 && (190..225).contains(&p[1]) && p[2] > 70 && p[2] < 130;
    assert!(count_in(&with, &corner, amber) > 100, "amber picture list");
    assert!(
        count_in(&with, &corner, |p| lum(p) > TEXT_LUM) > 100,
        "HUD text"
    );
    assert_eq!(count_in(&without, &corner, amber), 0);
}

/// WP12.6: after `forget_pictures` a picture that changed on disk is read
/// again; before it, the old pixels stay.
#[test]
fn a_picture_changed_on_disk_shows_after_forget_pictures() {
    let dir = temp_dir("reload");
    let write = |rgba: [u8; 4]| {
        let pixels = rgba.repeat(20 * 20);
        image::write_png(&dir.join("art/p.png"), &Rgba8::new(20, 20, pixels)).unwrap();
    };
    write([255, 0, 0, 255]);
    let scene = load_str(
        r##"Scene(name: "p", stage: (width: 640, height: 360), layers: [
            (name: "bg", art: Rect(w: 700.0, h: 400.0, color: "#204020"), z: -5),
            (name: "p", art: Image("art/p.png"), z: -4),
            (name: "gone", art: Image("art/later.png"), pos: (200.0, 0.0), z: -3),
        ])"##,
        None,
    )
    .expect("scene")
    .scene;
    let mut r = renderer(640, 360);
    let gpu = r.gpu();
    let mut cs = CutsceneRenderer::new(&gpu.device, &gpu.queue, r.color_format(), &dir);
    cs.preload_pictures(&scene);
    let (img, _) = shoot(&mut r, &mut cs, &scene, 0);
    assert!(near(img.pixel(320, 180), [255, 0, 0], 4));
    assert_eq!(cs.failed_pictures().len(), 1, "art/later.png is missing");

    write([0, 0, 255, 255]);
    image::write_png(
        &dir.join("art/later.png"),
        &Rgba8::new(20, 20, [0, 255, 0, 255].repeat(400)),
    )
    .unwrap();
    let (img, _) = shoot(&mut r, &mut cs, &scene, 0);
    assert!(
        near(img.pixel(320, 180), [255, 0, 0], 4),
        "cached: still red"
    );

    cs.forget_pictures();
    assert!(cs.failed_pictures().is_empty());
    cs.preload_pictures(&scene);
    let (img, _) = shoot(&mut r, &mut cs, &scene, 0);
    assert!(
        near(img.pixel(320, 180), [0, 0, 255], 4),
        "read again: blue"
    );
    assert!(near(img.pixel(520, 180), [0, 255, 0], 4), "the new picture");
    assert!(cs.failed_pictures().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// WP12.5: the demo scene (mission 1 briefing) at four moments. The PNGs in
// `docs/screenshots/wp12.5` were made with `SF_SAVE_SHOTS`.

fn demo() -> Scene {
    load_file(
        &repo_assets().join("scenes/f1_m01_briefing.ron"),
        Some(&repo_assets()),
    )
    .unwrap_or_else(|e| panic!("{e}"))
    .scene
}

/// Pixels of the placeholder rigs and of the map, as the scene paints them.
const ELIAN_JACKET: [u8; 3] = [0x66, 0x70, 0x46];
const MIRA_JACKET: [u8; 3] = [0x52, 0x70, 0x8a];
const TENT_CANVAS: [u8; 3] = [0xa3, 0x9a, 0x74];
const MAP_SHEET: [u8; 3] = [0xe2, 0xd4, 0xad];
/// What each figure is made of (jacket, sleeve, skin and, for Mira, hair).
const ELIAN_COLOURS: &[[u8; 3]] = &[ELIAN_JACKET, [0x59, 0x62, 0x3a], [0xe0, 0xb4, 0x8f]];
const MIRA_COLOURS: &[[u8; 3]] = &[
    MIRA_JACKET,
    [0x48, 0x62, 0x79],
    [0xc8, 0x94, 0x6c],
    [0x2a, 0x24, 0x20],
];

/// Pixels of a colour in the three-pixel strips along the left, right and top
/// edge of the picture: a figure that the frame cuts off shows up there.
fn on_edge(img: &Rgba8, v: &Rect, rgb: [u8; 3]) -> usize {
    let strips = [
        Rect {
            x: v.x,
            y: v.y,
            w: 3.0,
            h: v.h,
        },
        Rect {
            x: v.right() - 3.0,
            y: v.y,
            w: 3.0,
            h: v.h,
        },
        Rect {
            x: v.x,
            y: v.y,
            w: v.w,
            h: 3.0,
        },
    ];
    strips
        .iter()
        .map(|s| count_in(img, s, |p| near(p, rgb, 6)))
        .sum()
}

/// Horizontal centre of the pixels of a colour inside `r`, and how many there are.
fn centre_x(img: &Rgba8, r: &Rect, rgb: [u8; 3]) -> (f32, usize) {
    let (mut sum, mut n) = (0.0_f64, 0_usize);
    let (x0, y0) = (r.x.max(0.0) as u32, r.y.max(0.0) as u32);
    let (x1, y1) = (
        (r.right().ceil() as u32).min(img.width),
        (r.bottom().ceil() as u32).min(img.height),
    );
    for y in y0..y1 {
        for x in x0..x1 {
            if near(img.pixel(x, y), rgb, 6) {
                sum += f64::from(x);
                n += 1;
            }
        }
    }
    ((sum / n.max(1) as f64) as f32, n)
}

#[test]
fn demo_briefing_at_four_moments() {
    let (w, h) = (1280, 720);
    let scene = demo();
    let mut r = renderer(w, h);
    let gpu = r.gpu();
    let mut cs = CutsceneRenderer::new(&gpu.device, &gpu.queue, r.color_format(), repo_assets());

    // 1. The first line: Elian leans on the table, Mira stands across from him.
    let t = 3500;
    let (first, layout) = shoot(&mut r, &mut cs, &scene, t);
    save("1_first_line.png", &first);
    check_moment(&mut r, &mut cs, &scene, t, &first, &layout);
    let sub = layout.subtitle.as_ref().expect("Elian speaks at 3.5 s");
    assert_eq!(sub.name, "Captain Elian");
    assert!(
        sub.lines[0].0.starts_with("The relay went dark"),
        "{:?}",
        sub.lines
    );
    let v = layout.viewport;
    assert!(
        count_in(&first, &v, |p| near(p, TENT_CANVAS, 6)) > 60_000,
        "the tent"
    );
    assert!(count_in(&first, &v, |p| near(p, ELIAN_JACKET, 6)) > 6_000);
    assert!(count_in(&first, &v, |p| near(p, MIRA_JACKET, 6)) > 6_000);

    // 2. The map shot: the map fills the picture, the characters are gone from
    // it, the pins and the arrow of the overlay are on it.
    let t = 19_000;
    let (map, layout) = shoot(&mut r, &mut cs, &scene, t);
    save("2_map.png", &map);
    check_moment(&mut r, &mut cs, &scene, t, &map, &layout);
    let sub = layout.subtitle.as_ref().expect("Mira speaks over the map");
    assert_eq!(sub.name, "Mira");
    let v = layout.viewport;
    assert!(
        count_in(&map, &v, |p| near(p, MAP_SHEET, 6)) > 300_000,
        "the sheet"
    );
    assert_eq!(
        count_in(&map, &v, |p| near(p, ELIAN_JACKET, 6)),
        0,
        "no Elian"
    );
    assert_eq!(
        count_in(&map, &v, |p| near(p, MIRA_JACKET, 6)),
        0,
        "no Mira"
    );
    assert!(
        count_in(&map, &v, |p| near(p, [0xa4, 0xad, 0x74], 6)) > 5_000,
        "farmland"
    );
    assert!(
        count_in(&map, &v, |p| near(p, [0x9b, 0x84, 0x66], 6)) > 10_000,
        "ore ridge"
    );
    assert!(
        count_in(&map, &v, |p| near(p, [0x8e, 0x4a, 0x4a], 6)) > 150,
        "hostile camp"
    );
    assert!(
        count_in(&map, &v, |p| near(p, [226, 72, 58], 12)) > 150,
        "pins"
    );
    assert!(
        count_in(&map, &v, |p| near(p, [200, 52, 40], 12)) > 300,
        "arrow"
    );

    // 3. Mid-scene two-shot: Elian points at the map while he gives the next
    // order; the camera is wide, both figures are whole and on their own side.
    let t = 41_000;
    let (two, layout) = shoot(&mut r, &mut cs, &scene, t);
    save("3_two_shot.png", &two);
    check_moment(&mut r, &mut cs, &scene, t, &two, &layout);
    let sub = layout.subtitle.as_ref().expect("Elian speaks at 41 s");
    assert_eq!(sub.name, "Captain Elian");
    let v = layout.viewport;
    let (ex, en) = centre_x(&two, &v, ELIAN_JACKET);
    let (mx, mn) = centre_x(&two, &v, MIRA_JACKET);
    assert!(
        en > 10_000 && mn > 10_000,
        "both in the picture: {en}, {mn}"
    );
    let mid = v.x + v.w / 2.0;
    assert!(
        ex < mid - 150.0 && mx > mid + 150.0,
        "Elian left, Mira right: {ex}, {mx}"
    );
    assert!(mx - ex > 300.0);
    for (who, colours) in [("Elian", ELIAN_COLOURS), ("Mira", MIRA_COLOURS)] {
        for &c in colours {
            assert_eq!(on_edge(&two, &v, c), 0, "{who} is cut off by the frame");
        }
    }

    // 4. The fade-out: half way to black, the figures walking out, nobody speaking.
    let t = 61_900;
    let (fade, layout) = shoot(&mut r, &mut cs, &scene, t);
    save("4_fade_out.png", &fade);
    check_moment(&mut r, &mut cs, &scene, t, &fade, &layout);
    assert!(layout.subtitle.is_none(), "the last line ended at 60.7 s");
    assert!(
        mean_lum(&fade) < mean_lum(&first) * 0.8 && mean_lum(&fade) > 5.0,
        "half way to black: {} against {}",
        mean_lum(&fade),
        mean_lum(&first)
    );

    // Four different pictures.
    for (a, b) in [
        (&first, &map),
        (&first, &two),
        (&first, &fade),
        (&map, &two),
    ] {
        assert_ne!(a.pixels, b.pixels);
    }
}

/// The push-in on Mira's doubt is a real close-up: Elian is out of the picture
/// and Mira is whole, with nothing of her cut off at the left, right or top
/// edge, from the moment the camera has settled until the end of her line.
#[test]
fn demo_close_up_on_mira_cuts_nobody_off() {
    let scene = demo();
    let mut r = renderer(1280, 720);
    let gpu = r.gpu();
    let mut cs = CutsceneRenderer::new(&gpu.device, &gpu.queue, r.color_format(), repo_assets());
    for (i, t) in [47_200, 50_000, 54_500].into_iter().enumerate() {
        let (img, layout) = shoot(&mut r, &mut cs, &scene, t);
        if i == 1 {
            save("5_close_up.png", &img);
        }
        check_moment(&mut r, &mut cs, &scene, t, &img, &layout);
        assert_eq!(layout.subtitle.as_ref().expect("Mira speaks").name, "Mira");
        let v = layout.viewport;
        for &c in ELIAN_COLOURS {
            // A stray blended pixel may match a colour; a figure is thousands.
            let n = count_in(&img, &v, |p| near(p, c, 6));
            assert!(n < 100, "{t} ms: Elian is in the picture ({n} px of {c:?})");
        }
        assert!(
            count_in(&img, &v, |p| near(p, [0xc8, 0x94, 0x6c], 6)) > 8_000,
            "{t} ms: Mira's face fills the picture"
        );
        for &c in MIRA_COLOURS {
            // A cut-off figure is hundreds of pixels along the edge.
            let n = on_edge(&img, &v, c);
            assert!(
                n < 60,
                "{t} ms: Mira is cut off by the frame ({n} px of {c:?})"
            );
        }
    }
}

/// The checks every moment must pass. The frame is not empty. Where a line is
/// being spoken the subtitle is really drawn (a dark box with text-white pixels
/// that no picture content reaches), and the very same check fails on the same
/// frame without the subtitle: it can fail. Where nobody speaks, there is no
/// box and no text-white pixel where a subtitle would be.
fn check_moment(
    r: &mut Renderer,
    cs: &mut CutsceneRenderer,
    scene: &Scene,
    t_ms: i32,
    img: &Rgba8,
    layout: &FrameLayout,
) {
    assert!(distinct_colours(img) > 20, "{t_ms} ms: not an empty frame");
    assert!(mean_lum(img) > 5.0, "{t_ms} ms: not black");
    let state = scene.sample(t_ms);
    match (&state.subtitle, &layout.subtitle) {
        (Some(line), Some(sub)) => {
            assert!(
                has_subtitle(img, &sub.rect, 400),
                "{t_ms} ms: no subtitle text pixels for {:?}",
                line.text
            );
            let mut bare = scene.sample(t_ms);
            bare.subtitle = None;
            let (without, l2) = shoot_state(r, cs, scene, &bare);
            assert!(l2.subtitle.is_none());
            assert!(
                !has_subtitle(&without, &sub.rect, 400),
                "{t_ms} ms: the check also passes without a subtitle"
            );
            assert_eq!(
                count_in(&without, &sub.rect, |p| lum(p) > TEXT_LUM),
                0,
                "{t_ms} ms: picture content as bright as text"
            );
        }
        (None, None) => {
            // Where the box of the nearest line would sit, nothing is bright.
            let near_line = scene.sample(60_000);
            assert!(near_line.subtitle.is_some(), "a line is up at 60 s");
            let (probe, probe_layout) = shoot(r, cs, scene, 60_000);
            let rect = probe_layout.subtitle.expect("layout of the line").rect;
            assert!(has_subtitle(&probe, &rect, 400));
            assert!(
                !has_subtitle(img, &rect, 400),
                "{t_ms} ms: a ghost subtitle"
            );
            assert_eq!(count_in(img, &rect, |p| lum(p) > TEXT_LUM), 0, "{t_ms} ms");
        }
        _ => panic!("{t_ms} ms: the sampler and the renderer disagree about the subtitle"),
    }
}
