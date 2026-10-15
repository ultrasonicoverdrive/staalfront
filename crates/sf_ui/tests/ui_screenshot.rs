//! Screenshot tests for the UI: a fixed UI scene rendered on the software
//! adapter (Mesa lavapipe / DX12 WARP) on top of the clear color or the real
//! terrain.
//!
//! No golden image: glyph rasterization and the sRGB/blending path differ a
//! little between the two software drivers, so the checks are about content
//! that survives that (ink coverage inside a text box, ordering of button
//! state colors, the minimap's mean color and the field-of-view frame). Set
//! `SF_SAVE_SHOTS=<dir>` to write the images for a look.
//!
//! Adapter rules as in `sf_render`'s screenshot tests: a software adapter is
//! preferred; on CI (`CI` set) a missing one is a failure, locally any
//! adapter is accepted. No adapter at all fails the test.

use sf_data::map::generate::generate;
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8};
use sf_render::{Camera, CameraConfig, GpuOptions, Renderer};
use sf_ui::{
    Anchor, Color, Minimap, MinimapGeometry, Rect, Ui, UiConfig, UiInput, UiRenderer, fov_quad,
};

const W: u32 = 640;
const H: u32 = 360;

fn renderer() -> Renderer {
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
    r
}

fn save(name: &str, img: &Rgba8) {
    if let Some(dir) = std::env::var_os("SF_SAVE_SHOTS") {
        let path = std::path::Path::new(&dir).join(name);
        image::write_png(&path, img).expect("save shot");
    }
}

fn px(img: &Rgba8, x: f32, y: f32) -> [u8; 4] {
    img.pixel(x as u32, y as u32)
}

fn lum(p: [u8; 4]) -> i32 {
    (i32::from(p[0]) * 3 + i32::from(p[1]) * 6 + i32::from(p[2])) / 10
}

fn input(cursor: Option<(f32, f32)>, pressed: bool) -> UiInput {
    let mut i = UiInput {
        cursor,
        ..UiInput::default()
    };
    i.pressed[0] = pressed;
    i.down[0] = pressed;
    i
}

/// Render one UI frame on the clear color.
fn shoot(r: &mut Renderer, ui: &mut Ui) -> Rgba8 {
    let mut ur = UiRenderer::new(&r.gpu().device, r.color_format());
    r.render_to_image_with(|t| ur.render(t, ui))
        .expect("render")
}

/// A panel with one button; returns the panel, the button and the label size.
fn button_scene(
    ui: &mut Ui,
    label: &str,
    enabled: bool,
    cursor: Option<(f32, f32)>,
    pressed: bool,
) -> (Rect, Rect) {
    ui.begin_frame(input(cursor, pressed), (W, H), 1.0);
    let panel = ui.panel(Anchor::Center, (0.0, 0.0), (300.0, 120.0));
    let button = ui.place(panel, Anchor::Center, (0.0, 0.0), (160.0, 36.0));
    ui.button("b", button, label, enabled);
    ui.end_frame();
    (panel, button)
}

/// Color of the button face left of the label.
fn face(img: &Rgba8, button: Rect) -> [u8; 4] {
    px(img, button.x + 14.0, button.y + button.h * 0.5)
}

#[test]
fn button_states_have_distinct_colors() {
    let mut r = renderer();
    let mut ui = Ui::new(UiConfig::default());
    let mut shot = |ui: &mut Ui, enabled, cursor, pressed| {
        let (_, b) = button_scene(ui, "Build", enabled, cursor, pressed);
        let img = shoot(&mut r, ui);
        (face(&img, b), img)
    };
    let centre = Some((W as f32 / 2.0, H as f32 / 2.0));
    let (normal, img) = shot(&mut ui, true, Some((5.0, 5.0)), false);
    save("button_normal.png", &img);
    let (hover, _) = shot(&mut ui, true, centre, false);
    let (pressed, img) = shot(&mut ui, true, centre, true);
    save("button_pressed.png", &img);
    let (disabled, img) = shot(&mut ui, false, centre, false);
    save("button_disabled.png", &img);
    println!("normal {normal:?} hover {hover:?} pressed {pressed:?} disabled {disabled:?}");

    let sum = |p: [u8; 4]| i32::from(p[0]) + i32::from(p[1]) + i32::from(p[2]);
    assert!(
        sum(hover) > sum(normal) + 30,
        "hover is brighter than normal"
    );
    assert!(
        sum(pressed) + 30 < sum(normal),
        "pressed is darker than normal"
    );
    // The normal button is blue; the disabled one is grey (little spread).
    assert!(
        i32::from(normal[2]) > i32::from(normal[0]) + 30,
        "normal is blue-ish"
    );
    let spread = |p: [u8; 4]| {
        i32::from(p.iter().take(3).max().copied().unwrap())
            - i32::from(p.iter().take(3).min().copied().unwrap())
    };
    assert!(
        spread(disabled) < spread(normal) / 2,
        "disabled is desaturated"
    );
    for p in [normal, hover, pressed, disabled] {
        assert_eq!(p[3], 255);
    }
}

#[test]
fn text_inks_its_box_and_nothing_around_it() {
    let mut r = renderer();
    let mut ui = Ui::new(UiConfig::default());
    ui.begin_frame(UiInput::default(), (W, H), 1.0);
    let panel = ui.panel(Anchor::Center, (0.0, 0.0), (300.0, 120.0));
    let text = ui.text_in(
        panel,
        Anchor::TopLeft,
        (24.0, 24.0),
        "Staalfront HQ",
        Color(255, 255, 255, 255),
    );
    ui.end_frame();
    let img = shoot(&mut r, &mut ui);
    save("text.png", &img);

    let bg = lum(px(&img, panel.x + 12.0, panel.y + panel.h - 14.0));
    let mut ink = 0;
    let mut brightest = 0;
    for y in text.y as u32..text.bottom() as u32 {
        for x in text.x as u32..text.right() as u32 {
            let l = lum(img.pixel(x, y));
            ink += usize::from(l > bg + 60);
            brightest = brightest.max(l);
        }
    }
    let area = (text.w * text.h) as usize;
    let ratio = ink as f32 / area as f32;
    println!("text box {text:?}: {ink} ink pixels of {area} ({ratio:.2}), brightest {brightest}");
    assert!((0.08..0.6).contains(&ratio), "ink coverage {ratio}");
    assert!(brightest > 200, "full-coverage pixels reach the text color");

    // Around the box (a margin of 5 px, inside the panel) nothing was drawn.
    let ring = Rect::new(text.x - 5.0, text.y - 5.0, text.w + 10.0, text.h + 10.0);
    let mut stray = 0;
    for y in ring.y as u32..ring.bottom() as u32 {
        for x in ring.x as u32..ring.right() as u32 {
            let inside = text.contains(x as f32, y as f32);
            stray += usize::from(!inside && lum(img.pixel(x, y)) > bg + 30);
        }
    }
    assert_eq!(stray, 0, "text stays inside its measured box");
}

#[test]
fn diacritics_add_ink_above_the_letter() {
    let mut r = renderer();
    let mut ink_of = |s: &str| {
        let mut ui = Ui::new(UiConfig::default());
        ui.begin_frame(UiInput::default(), (W, H), 1.0);
        let panel = ui.panel(Anchor::Center, (0.0, 0.0), (300.0, 120.0));
        ui.text_in(
            panel,
            Anchor::Center,
            (0.0, 0.0),
            s,
            Color(255, 255, 255, 255),
        );
        ui.end_frame();
        let img = shoot(&mut r, &mut ui);
        let bg = lum(px(&img, panel.x + 12.0, panel.y + panel.h - 14.0));
        let mut n = 0;
        for y in panel.y as u32 + 10..panel.bottom() as u32 - 10 {
            for x in panel.x as u32 + 10..panel.right() as u32 - 10 {
                n += usize::from(lum(img.pixel(x, y)) > bg + 60);
            }
        }
        n
    };
    let plain = ink_of("eee");
    let accent = ink_of("\u{e9}\u{e9}\u{e9}");
    let diaeresis = ink_of("\u{eb}\u{eb}\u{eb}");
    let ij = ink_of("\u{ef}\u{ef}\u{ef}");
    println!("ink: e {plain}, acute {accent}, diaeresis {diaeresis}, i-diaeresis {ij}");
    assert!(plain > 50);
    assert!(accent > plain + 10, "the acute adds ink");
    assert!(diaeresis > plain + 10, "the diaeresis adds ink");
    assert!(ij > 30);
}

#[test]
fn nine_slice_panel_stretches_without_distorting_the_corners() {
    let mut r = renderer();
    let mut ui = Ui::new(UiConfig::default());
    ui.begin_frame(UiInput::default(), (W, H), 1.0);
    let big = ui.panel(Anchor::TopLeft, (20.0, 20.0), (400.0, 200.0));
    let small = ui.panel(Anchor::BottomRight, (20.0, 20.0), (60.0, 40.0));
    ui.end_frame();
    let img = shoot(&mut r, &mut ui);
    save("panels.png", &img);

    // The outer corner pixel is cut by the rounded corner: it shows the clear color.
    let clear = px(&img, 2.0, 2.0);
    assert_eq!(
        px(&img, big.x, big.y),
        clear,
        "rounded corner is transparent"
    );
    // The stretched middle is uniform along x at a fixed y.
    let y = big.y + 100.0;
    let a = px(&img, big.x + 40.0, y);
    let b = px(&img, big.x + 360.0, y);
    for c in 0..3 {
        assert!(a[c].abs_diff(b[c]) <= 2, "{a:?} vs {b:?}");
    }
    assert_ne!(a, clear, "the panel covers the clear color");
    // The small panel is made of the same pieces: its middle matches the big one's.
    let m = px(&img, small.x + small.w / 2.0, small.y + small.h / 2.0);
    for c in 0..3 {
        assert!(m[c].abs_diff(a[c]) <= 8, "{m:?} vs {a:?}");
    }
}

#[test]
fn ui_scale_doubles_the_button() {
    let mut r = renderer();
    let mut ui = Ui::new(UiConfig::default());
    let size = |ui: &mut Ui, dpi: f64| {
        ui.begin_frame(UiInput::default(), (W, H), dpi);
        let panel = ui.panel(Anchor::Center, (0.0, 0.0), (300.0, 120.0));
        let button = ui.place(panel, Anchor::Center, (0.0, 0.0), (160.0, 36.0));
        ui.button("b", button, "Scale", true);
        ui.end_frame();
        button
    };
    let one = size(&mut ui, 1.0);
    let two = size(&mut ui, 2.0);
    assert_eq!((one.w, one.h), (160.0, 36.0));
    assert_eq!((two.w, two.h), (320.0, 72.0));
    let img = shoot(&mut r, &mut ui);
    save("scale2.png", &img);
    // The pixels follow: the 2x button face exists where the 1x one would not reach.
    assert_ne!(
        px(&img, two.x + 4.0, two.y + two.h / 2.0),
        px(&img, 2.0, 2.0)
    );
}

/// The terrain, the minimap bottom-right, dots and the field-of-view frame.
struct MinimapScene {
    img: Rgba8,
    frame: Rect,
    geom: MinimapGeometry,
    quad: [(f32, f32); 4],
    minimap: Minimap,
}

fn minimap_scene(with_overlays: bool) -> MinimapScene {
    let mut r = renderer();
    let map = generate(128, 128, 1);
    r.set_terrain(&map);
    let mut cam = Camera::new(
        CameraConfig {
            map_size: DVec2::new(128.0, 128.0),
            ..CameraConfig::default()
        },
        W,
        H,
    );
    cam.set_zoom(0.3);
    cam.set_focus(DVec2::new(64.0, 64.0));
    r.set_view_proj(cam.view_proj());

    let minimap = Minimap::bake(&map, 256);
    let mut ur = UiRenderer::new(&r.gpu().device, r.color_format());
    ur.set_minimap(&r.gpu().device, &r.gpu().queue, minimap.image());

    let mut ui = Ui::new(UiConfig::default());
    ui.begin_frame(UiInput::default(), (W, H), 1.0);
    let frame = ui.place(
        ui.screen(),
        Anchor::BottomRight,
        (10.0, 10.0),
        (170.0, 170.0),
    );
    let quad = fov_quad(&cam, (W as f32, H as f32), (128.0, 128.0));
    let dots = [
        ((30.0, 30.0), Color(255, 0, 0, 255)),
        ((100.0, 90.0), Color(255, 0, 0, 255)),
    ];
    ui.minimap(
        &minimap,
        "minimap",
        frame,
        with_overlays.then_some(quad),
        if with_overlays {
            dots.to_vec()
        } else {
            Vec::new()
        },
    );
    ui.end_frame();
    let img = r
        .render_to_image_with(|t| ur.render(t, &mut ui))
        .expect("render");
    assert!(
        ur.draw_calls() <= 3,
        "frame, minimap image, dots and lines: {} draw calls",
        ur.draw_calls()
    );
    let geom = MinimapGeometry::new(frame.inset(2.0), (128.0, 128.0));
    MinimapScene {
        img,
        frame,
        geom,
        quad,
        minimap,
    }
}

#[test]
fn minimap_shows_the_baked_terrain() {
    let s = minimap_scene(false);
    save("minimap.png", &s.img);
    let c = s.geom.content();
    let (mut sum, mut n) = ([0u64; 3], 0u64);
    for y in (c.y as u32 + 2)..(c.bottom() as u32 - 2) {
        for x in (c.x as u32 + 2)..(c.right() as u32 - 2) {
            let p = s.img.pixel(x, y);
            for i in 0..3 {
                sum[i] += u64::from(p[i]);
            }
            n += 1;
        }
    }
    let shown: Vec<f64> = sum.iter().map(|v| *v as f64 / n as f64).collect();
    let image = s.minimap.image();
    let mut base = [0u64; 3];
    for p in image.rgba.chunks_exact(4) {
        for i in 0..3 {
            base[i] += u64::from(p[i]);
        }
    }
    let count = (image.rgba.len() / 4) as f64;
    for i in 0..3 {
        let want = base[i] as f64 / count;
        println!("channel {i}: shown {:.1}, baked {want:.1}", shown[i]);
        assert!(
            (shown[i] - want).abs() < 12.0,
            "channel {i}: {} vs {want}",
            shown[i]
        );
    }
    // The frame around it is the dark frame color, and the world outside is the terrain scene.
    let edge = px(&s.img, s.frame.x + 1.0, s.frame.y + s.frame.h / 2.0);
    assert!(lum(edge) < 60, "dark minimap frame {edge:?}");
    assert_ne!(px(&s.img, 20.0, 20.0), edge);
}

#[test]
fn minimap_draws_unit_dots_and_the_field_of_view_frame() {
    let s = minimap_scene(true);
    save("minimap_overlays.png", &s.img);
    for (x, z) in [(30.0, 30.0), (100.0, 90.0)] {
        let (sx, sy) = s.geom.world_to_screen(x, z);
        let red = (-1i32..=1)
            .flat_map(|dy| (-1i32..=1).map(move |dx| (dx, dy)))
            .any(|(dx, dy)| {
                let p = s
                    .img
                    .pixel((sx as i32 + dx) as u32, (sy as i32 + dy) as u32);
                p[0] > 200 && p[1] < 80 && p[2] < 80
            });
        assert!(red, "red unit dot at ({x}, {z}) -> ({sx}, {sy})");
    }
    for (x, z) in s.quad {
        assert!(
            x > 1.0 && x < 127.0 && z > 1.0 && z < 127.0,
            "frame not clamped: ({x}, {z})"
        );
    }
    // The midpoint of each side of the frame is a white line.
    for i in 0..4 {
        let (a, b) = (s.quad[i], s.quad[(i + 1) % 4]);
        let (sx, sy) = s.geom.world_to_screen((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let white = (-1i32..=1)
            .flat_map(|dy| (-1i32..=1).map(move |dx| (dx, dy)))
            .any(|(dx, dy)| {
                let p = s
                    .img
                    .pixel((sx as i32 + dx) as u32, (sy as i32 + dy) as u32);
                p[0] > 200 && p[1] > 200 && p[2] > 200
            });
        assert!(white, "side {i} of the field-of-view frame at ({sx}, {sy})");
    }
}
