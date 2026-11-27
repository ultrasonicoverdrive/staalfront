//! The pictures of the trial figure for Tijmen's style check (WP11.6i), rendered by the engine:
//! the set v3 rifleman next to the set v5 one, both factions, blue and red player, 45 and 80
//! degrees down (`compare_close.png`), and eight frames of one walk cycle per faction from the
//! side and from 45 degrees (`walk_cycle.png`). The frames come from the instancer's own walk
//! (a unit moved by an eighth of a stride per frame), not from a pose set by hand.
//!
//! Always rendered and checked (the walk frames differ from each other, the stand pose of the v3
//! figure does not move); written only when `SF_SAVE_SHOTS=<dir>` is set. Adapter rules as in the
//! other screenshot tests (software preferred, required on CI).

use std::path::PathBuf;

use glam::camera::rh;
use glam::{Mat4, Vec3};
use sf_math::render_bridge::metres_to_cells;
use sf_math::{Facing, Vec2};
use sf_render::glb::{self, AxisConvention, LoadedModel};
use sf_render::image::{self, Rgba8};
use sf_render::{FrameView, Frustum, GpuOptions, Instancer, Motion, Renderer, Style, UnitId};

const W: u32 = 320;
const H: u32 = 360;

fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn load(path: &str) -> LoadedModel {
    glb::load_glb_file(&assets().join(path), &AxisConvention::ENGINE)
        .unwrap_or_else(|e| panic!("{path}: {e}"))
}

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
    r.set_scene(&[]);
    r
}

/// A camera at `elevation_deg` down and `azimuth_deg` round from `+X` toward `+Z`, `d` metres from
/// `centre`, 30° field of view.
fn view(centre: Vec3, d: f32, elevation_deg: f32, azimuth_deg: f32) -> Mat4 {
    let (se, ce) = elevation_deg.to_radians().sin_cos();
    let (sa, ca) = azimuth_deg.to_radians().sin_cos();
    let eye = centre + Vec3::new(ca * ce, se, sa * ce) * d;
    let up = if elevation_deg > 60.0 {
        -Vec3::X
    } else {
        Vec3::Y
    };
    let proj =
        rh::proj::directx::perspective(30_f32.to_radians(), W as f32 / H as f32, 0.1, d * 4.0);
    proj * rh::view::look_at_mat4(eye, centre, up)
}

fn cells(x: f32, z: f32) -> Vec2 {
    Vec2::new(metres_to_cells(x), metres_to_cells(z))
}

// ---- A 3 x 5 pixel font for the labels -------------------------------------------------------

/// Rows top to bottom, three bits each (bit 2 = left).
fn glyph(c: char) -> [u8; 5] {
    match c {
        'A' => [2, 5, 7, 5, 5],
        'B' => [6, 5, 6, 5, 6],
        'C' => [3, 4, 4, 4, 3],
        'D' => [6, 5, 5, 5, 6],
        'E' => [7, 4, 6, 4, 7],
        'F' => [7, 4, 6, 4, 4],
        'G' => [3, 4, 5, 5, 3],
        'H' => [5, 5, 7, 5, 5],
        'I' => [7, 2, 2, 2, 7],
        'K' => [5, 5, 6, 5, 5],
        'L' => [4, 4, 4, 4, 7],
        'M' => [5, 7, 7, 5, 5],
        'N' => [6, 5, 5, 5, 5],
        'O' => [2, 5, 5, 5, 2],
        'P' => [6, 5, 6, 4, 4],
        'R' => [6, 5, 6, 5, 5],
        'S' => [3, 4, 2, 1, 6],
        'T' => [7, 2, 2, 2, 2],
        'U' => [5, 5, 5, 5, 7],
        'V' => [5, 5, 5, 5, 2],
        'W' => [5, 5, 7, 7, 5],
        'Y' => [5, 5, 2, 2, 2],
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [6, 1, 2, 4, 7],
        '3' => [6, 1, 2, 1, 6],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 6, 1, 6],
        '6' => [3, 4, 7, 5, 7],
        '7' => [7, 1, 2, 2, 2],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 6],
        '/' => [1, 1, 2, 4, 4],
        '.' => [0, 0, 0, 0, 2],
        _ => [0; 5],
    }
}

/// Write `text` (upper case) at `(x, y)` in a tile, 3 pixels per font pixel, white on a dark box.
fn label(tile: &mut Rgba8, text: &str, x: u32, y: u32) {
    let s = 3;
    let (w, h) = (tile.width, tile.height);
    let len = text.chars().count() as u32;
    let mut put = |px: u32, py: u32, c: [u8; 4]| {
        if px < w && py < h {
            let o = ((py * w + px) * 4) as usize;
            tile.pixels[o..o + 4].copy_from_slice(&c);
        }
    };
    for py in y.saturating_sub(2)..y + 5 * s + 2 {
        for px in x.saturating_sub(2)..x + len * 4 * s + 2 {
            put(px, py, [20, 20, 20, 255]);
        }
    }
    for (i, c) in text.chars().enumerate() {
        for (row, bits) in glyph(c).iter().enumerate() {
            for col in 0..3 {
                if bits & (4 >> col) != 0 {
                    for dy in 0..s {
                        for dx in 0..s {
                            let px = x + (i as u32 * 4 + col) * s + dx;
                            let py = y + row as u32 * s + dy;
                            put(px, py, [240, 240, 240, 255]);
                        }
                    }
                }
            }
        }
    }
}

/// Tiles of `W x H` in rows of `cols`.
fn sheet(tiles: &[Rgba8], cols: u32) -> Rgba8 {
    let rows = (tiles.len() as u32).div_ceil(cols);
    let mut px = vec![0u8; (W * cols * H * rows * 4) as usize];
    for (i, t) in tiles.iter().enumerate() {
        let (cx, cy) = (i as u32 % cols, i as u32 / cols);
        for y in 0..H {
            for x in 0..W {
                let o = (((cy * H + y) * W * cols + cx * W + x) * 4) as usize;
                px[o..o + 4].copy_from_slice(&t.pixel(x, y));
            }
        }
    }
    Rgba8::new(W * cols, H * rows, px)
}

fn style() -> Style {
    Style::embedded()
        .with_palette(&[
            ("blue".into(), [40, 90, 230]),
            ("red".into(), [220, 40, 40]),
        ])
        .expect("palette")
}

fn differ(a: &Rgba8, b: &Rgba8) -> usize {
    (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .filter(|&(x, y)| a.pixel(x, y) != b.pixel(x, y))
        .count()
}

#[test]
fn the_trial_figure_pictures_are_rendered_and_written_on_request() {
    let factions = ["meridian", "veyran"];
    let files: Vec<String> = factions
        .iter()
        .flat_map(|f| {
            [
                format!("set_v3/{f}_rifleman.glb"),
                format!("set_v5/{f}_rifleman.glb"),
            ]
        })
        .collect();
    let models: Vec<LoadedModel> = files.iter().map(|f| load(f)).collect();
    let set = glb::model_set(&models, &[60.0, 30.0]).unwrap();
    let mut r = renderer();
    let at = (64.0_f32, 64.0_f32);

    // compare_close: rows = faction x set, columns = player x angle.
    let mut inst = Instancer::new(style(), (128.0, 128.0));
    inst.set_models(set.clone());
    let mut close = Vec::new();
    for (shape, file) in files.iter().enumerate() {
        for player in [0u8, 1] {
            for elevation in [45.0, 80.0] {
                let p = cells(at.0, at.1);
                inst.update_tick([Motion::from_fixed(
                    shape as u8,
                    player,
                    p,
                    p,
                    Facing::new(0),
                    Facing::new(0),
                )
                .with_id(UnitId::from_parts(1, 0))]);
                let vp = view(Vec3::new(at.0 + 0.2, 0.9, at.1), 4.6, elevation, 35.0);
                inst.build_frame(
                    &FrameView::models_only(&Frustum::from_view_proj(vp), 1.0),
                    |_, _| 0.0,
                );
                r.set_view_proj(vp);
                r.upload_units(&inst);
                let mut img = r.render_to_image().expect("render");
                let set_name = if file.starts_with("set_v3") {
                    "V3"
                } else {
                    "V5"
                };
                let faction = file.split('/').nth(1).unwrap().split('_').next().unwrap();
                let who = if player == 0 { "BLUE" } else { "RED" };
                label(
                    &mut img,
                    &format!("{set_name} {} {who} {elevation:.0}", faction.to_uppercase()),
                    6,
                    6,
                );
                close.push(img);
            }
        }
    }

    // walk_cycle: per faction a side row and a 45° row of eight frames, the camera on the unit.
    let stride = sf_render::walk::WalkConfig::embedded().stride_m;
    let mut walk = Vec::new();
    let mut frames_differ = Vec::new();
    for (fi, faction) in factions.iter().enumerate() {
        let shape = (fi * 2 + 1) as u8;
        for (vname, elevation, azimuth) in [("SIDE", 4.0, 90.0), ("45", 45.0, 60.0)] {
            let mut inst = Instancer::new(style(), (128.0, 128.0));
            inst.set_models(set.clone());
            let id = UnitId::from_parts(1, 0);
            let mut x = 20.0_f32;
            let step = |inst: &mut Instancer, from: f32, to: f32, vp: Mat4| {
                inst.update_tick([Motion::from_fixed(
                    shape,
                    0,
                    cells(from, at.1),
                    cells(to, at.1),
                    Facing::new(0),
                    Facing::new(0),
                )
                .with_id(id)]);
                inst.build_frame(
                    &FrameView::models_only(&Frustum::from_view_proj(vp), 1.0),
                    |_, _| 0.0,
                );
            };
            // Walk two full cycles first so the walk is fully blended in, ending at phase 0.
            for _ in 0..16 {
                let to = x + stride / 8.0;
                let vp = view(Vec3::new(to, 0.9, at.1), 4.6, elevation, azimuth);
                step(&mut inst, x, to, vp);
                x = to;
            }
            let mut row = Vec::new();
            for k in 0..8 {
                let to = x + stride / 8.0;
                let vp = view(Vec3::new(to + 0.2, 0.9, at.1), 4.6, elevation, azimuth);
                step(&mut inst, x, to, vp);
                x = to;
                r.set_view_proj(vp);
                r.upload_units(&inst);
                let mut img = r.render_to_image().expect("render");
                label(
                    &mut img,
                    &format!("{} {vname} {}/8", faction.to_uppercase(), k + 1),
                    6,
                    6,
                );
                row.push(img);
            }
            frames_differ.push(differ(&row[1], &row[5]));
            walk.extend(row);
        }
    }
    // Opposite phases (a quarter and three quarters of the cycle) look different: the legs swap.
    for (i, d) in frames_differ.iter().enumerate() {
        assert!(
            *d > 500,
            "walk row {i}: frames 2 and 6 differ in only {d} pixels"
        );
    }

    if let Some(dir) = std::env::var_os("SF_SAVE_SHOTS") {
        let dir = PathBuf::from(dir);
        image::write_png(&dir.join("compare_close.png"), &sheet(&close, 4)).unwrap();
        image::write_png(&dir.join("walk_cycle.png"), &sheet(&walk, 8)).unwrap();
    }
}
