//! Screenshot tests for glTF models (WP1.10).
//!
//! 1. **Orientation**: the orientation test model (front red, top green,
//!    right blue, the other sides grey; written by `sf_render::glb::writer` in
//!    the glTF spec's own axes, front `+Z`, and converted by the loader)
//!    stands at facing 0 and is rendered from four sides and from above. From
//!    `+X` the front is visible, from `+Z` the right side, from above the top,
//!    from `-Z` a grey side: front `+X`, up `+Y` and the right side at `+Z`
//!    (right-handed, no mirror). Turned to facing 64 (a quarter turn towards
//!    `+Z`) the front faces the `+Z` camera.
//! 2. **Test pack at close zoom**: a few test-pack models on the test map,
//!    with a golden image and content checks that do not depend on it: the
//!    player colours are present (the `team` material) and the turrets are
//!    visible (taking the turret parts out changes the picture).
//!
//! Same adapter rules as `screenshot.rs`: software adapter preferred, no
//! adapter = failure, and on CI (`CI` set) a missing software adapter is not
//! masked. Regenerate the golden with
//! `SF_UPDATE_GOLDEN=1 cargo test -p sf_render --test glb_screenshot`.

use std::path::PathBuf;

use glam::camera::rh;
use glam::{Mat4, Vec3, Vec4};
use sf_data::map::Map;
use sf_math::render_bridge::fx_from_f32;
use sf_math::{Facing, Vec2};
use sf_render::camera::DVec2;
use sf_render::glb::{self, AxisConvention, writer};
use sf_render::image::{self, Rgba8, Tolerance, compare};
use sf_render::model::{Joint, MeshId};
use sf_render::{
    Blend, Camera, CameraConfig, EyeView, FrameView, Frustum, GpuOptions, Instancer, Motion,
    Renderer, Style, UnitId,
};

const TOLERANCE: Tolerance = Tolerance {
    channel: 16,
    max_ratio: 0.04,
};

fn renderer(w: u32, h: u32) -> Renderer {
    let mut r = Renderer::headless(w, h, &GpuOptions::software())
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
    r.set_scene(&[]);
    r
}

fn unit(shape: u8, player: u8, id: u32, at: (f32, f32), facing: u8) -> Motion {
    let p = Vec2::new(fx_from_f32(at.0), fx_from_f32(at.1));
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

/// Hue class of a pixel: which channel clearly dominates, or grey.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hue {
    Red,
    Green,
    Blue,
    Grey,
    Other,
}

fn hue(p: [u8; 4]) -> Hue {
    let [r, g, b] = [p[0], p[1], p[2]].map(f32::from);
    let max = r.max(g).max(b).max(1.0);
    let min = r.min(g).min(b);
    if (max - min) / max < 0.12 {
        return Hue::Grey;
    }
    if r > 2.0 * g && r > 2.0 * b {
        Hue::Red
    } else if g > 2.0 * r && g > 2.0 * b {
        Hue::Green
    } else if b > 2.0 * r && b > 1.5 * g {
        Hue::Blue
    } else {
        Hue::Other
    }
}

const OW: u32 = 160;
const OH: u32 = 120;

fn look(eye: Vec3, at: Vec3, up: Vec3) -> Mat4 {
    let proj =
        rh::proj::directx::perspective(40_f32.to_radians(), OW as f32 / OH as f32, 0.1, 100.0);
    proj * rh::view::look_at_mat4(eye, at, up)
}

fn pixel_of(vp: Mat4, p: Vec3) -> (u32, u32) {
    let c = vp * Vec4::new(p.x, p.y, p.z, 1.0);
    let (x, y) = (c.x / c.w, c.y / c.w);
    (
        ((x * 0.5 + 0.5) * OW as f32) as u32,
        ((0.5 - y * 0.5) * OH as f32) as u32,
    )
}

/// Majority hue in a 5x5 window around the pixel of world point `p`.
fn hue_at(img: &Rgba8, vp: Mat4, p: Vec3) -> Hue {
    let (cx, cy) = pixel_of(vp, p);
    let mut counts = [0; 5];
    for y in cy - 2..=cy + 2 {
        for x in cx - 2..=cx + 2 {
            let i = match hue(img.pixel(x, y)) {
                Hue::Red => 0,
                Hue::Green => 1,
                Hue::Blue => 2,
                Hue::Grey => 3,
                Hue::Other => 4,
            };
            counts[i] += 1;
        }
    }
    let best = (0..5).max_by_key(|&i| counts[i]).unwrap();
    [Hue::Red, Hue::Green, Hue::Blue, Hue::Grey, Hue::Other][best]
}

#[test]
fn the_orientation_model_shows_front_top_and_right_where_the_engine_expects() {
    // Written in the glTF spec's axes (front +Z, right -X): the loader's
    // conversion has to put the front on +X.
    let bytes = writer::write(&writer::orientation(&AxisConvention::GLTF));
    let model = glb::load_glb(&bytes, &AxisConvention::GLTF).expect("loads");
    let set = glb::model_set(std::slice::from_ref(&model), &[]).unwrap();
    // Player 2 is yellow: not one of the face colours.
    let mut inst = Instancer::new(Style::embedded(), (64.0, 64.0));
    inst.set_models(set);
    let mut r = renderer(OW, OH);
    let mut shot = |inst: &mut Instancer, facing: u8, vp: Mat4| {
        inst.update_tick([unit(0, 2, 1, (32.0, 32.0), facing)]);
        let frustum = Frustum::from_view_proj(vp);
        inst.build_frame(&FrameView::models_only(&frustum, 1.0), |_, _| 0.0);
        r.set_view_proj(vp);
        r.upload_units(inst);
        r.render_to_image().expect("render")
    };
    let c = Vec3::new(32.0, 0.0, 32.0);
    let mid = c + Vec3::new(0.0, 0.5, 0.0);
    // From +X: the front (red).
    let vp = look(c + Vec3::new(7.0, 0.5, 0.0), mid, Vec3::Y);
    let img = shot(&mut inst, 0, vp);
    assert_eq!(
        hue_at(&img, vp, c + Vec3::new(1.0, 0.5, 0.0)),
        Hue::Red,
        "front at +X"
    );
    // From +Z: the right side (blue).
    let vp = look(c + Vec3::new(0.0, 0.5, 7.0), mid, Vec3::Y);
    let img = shot(&mut inst, 0, vp);
    assert_eq!(
        hue_at(&img, vp, c + Vec3::new(-0.5, 0.5, 0.5)),
        Hue::Blue,
        "right at +Z"
    );
    // From -Z: the left side, grey (not blue: no mirror).
    let vp = look(c + Vec3::new(0.0, 0.5, -7.0), mid, Vec3::Y);
    let img = shot(&mut inst, 0, vp);
    assert_eq!(
        hue_at(&img, vp, c + Vec3::new(-0.5, 0.5, -0.5)),
        Hue::Grey,
        "left at -Z"
    );
    // From above (screen up = +X): the top (green) beside the turret.
    let vp = look(c + Vec3::new(0.0, 9.0, 0.0), c, Vec3::X);
    let img = shot(&mut inst, 0, vp);
    assert_eq!(
        hue_at(&img, vp, c + Vec3::new(-0.7, 1.0, 0.0)),
        Hue::Green,
        "top at +Y"
    );
    // The front is at the top of that picture and the right side on the
    // right: +X up, +Z right, seen from above.
    let (front, right) = (
        pixel_of(vp, c + Vec3::new(1.0, 1.0, 0.0)),
        pixel_of(vp, c + Vec3::new(0.0, 1.0, 0.5)),
    );
    assert!(front.1 < OH / 2 && right.0 > OW / 2, "{front:?} {right:?}");
    // The turret on top takes the player colour (yellow): team material.
    let (tx, ty) = pixel_of(vp, c + Vec3::new(0.0, 1.4, 0.0));
    let t = img.pixel(tx, ty);
    assert!(
        t[0] > 120 && t[1] > 90 && t[2] < t[1] / 2,
        "turret in player colour: {t:?}"
    );
    // The node matrices are converted too: the barrel pivot sits 0.3 m
    // forward of the turret (+X in engine axes, +Z in the file), so its
    // dark tip passes x = 1.0 at turret height. Seen close up from +Z, the
    // centre pixel is the barrel; with an unconverted pivot the barrel
    // would end at x = 0.8 and this pixel would be background.
    let tip = c + Vec3::new(1.0, 1.2, 0.0);
    let vp = look(tip + Vec3::new(0.0, 0.0, 3.0), tip, Vec3::Y);
    let img = shot(&mut inst, 0, vp);
    let (bx, by) = pixel_of(vp, tip);
    let p = img.pixel(bx, by);
    assert!(
        p[0] < 40 && p[1] < 40 && p[2] < 40,
        "dark barrel tip at x = 1.0 seen from +Z: {p:?}"
    );
    // Facing 64 (a quarter turn towards +Z): now the +Z camera sees the front.
    let vp = look(c + Vec3::new(0.0, 0.5, 7.0), mid, Vec3::Y);
    let img = shot(&mut inst, 64, vp);
    assert_eq!(
        hue_at(&img, vp, c + Vec3::new(0.0, 0.5, 1.0)),
        Hue::Red,
        "front turned to +Z"
    );
    image::write_png(&std::env::temp_dir().join("glb_orientation.png"), &img).ok();
}

fn test_map() -> Map {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps/test64");
    Map::load(&dir).expect("committed test map loads")
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

/// Pixels whose hue is one of the palette's player colours.
fn player_pixels(style: &Style, img: &Rgba8, player: u8) -> usize {
    let q = style.srgb(player).map(f32::from);
    let qmax = q[0].max(q[1]).max(q[2]);
    let mut n = 0;
    for y in 0..img.height {
        for x in 0..img.width {
            let p = img.pixel(x, y);
            let c = [p[0], p[1], p[2]].map(f32::from);
            let max = c[0].max(c[1]).max(c[2]);
            if max < 30.0 {
                continue;
            }
            let d: f32 = (0..3)
                .map(|k| (c[k] / max - q[k] / qmax).powi(2))
                .sum::<f32>()
                .sqrt();
            n += usize::from(d < 0.12);
        }
    }
    n
}

const W: u32 = 320;
const H: u32 = 180;

#[test]
fn test_pack_models_at_close_zoom() {
    let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let config = std::fs::read_to_string(assets.join(glb::MODELS_CONFIG)).unwrap();
    let pack = glb::load_pack(&config, "testpack", &assets).unwrap();
    let style = pack.style(&Style::embedded()).unwrap();
    let map = test_map();
    let mut inst = Instancer::new(Style::embedded(), (64.0, 64.0));
    inst.set_style_and_models(style.clone(), pack.set.clone());
    inst.set_height_range(-2.0, 10.0);
    // Five vehicles in a row in red and blue, turrets aimed at a point
    // behind the camera's right.
    let kinds = [0u8, 3, 4, 6, 7];
    let motions: Vec<Motion> = kinds
        .iter()
        .enumerate()
        .map(|(i, &k)| {
            unit(
                k,
                (i % 2) as u8,
                i as u32,
                (
                    20.0 + i as f32 * 6.0,
                    32.0 + if i % 2 == 0 { 0.0 } else { 3.0 },
                ),
                (i * 16) as u8,
            )
        })
        .collect();
    inst.update_tick(motions);
    inst.set_aim(Some([45.0, 60.0]));
    let cfg = CameraConfig {
        map_size: DVec2::new(64.0, 64.0),
        ..CameraConfig::default()
    };
    let mut cam = Camera::new(cfg, W, H);
    cam.set_zoom(0.22);
    cam.set_focus(DVec2::new(32.0, 33.0));
    let mut r = renderer(W, H);
    r.set_terrain(&map);
    let vp = cam.view_proj();
    let frustum = Frustum::from_view_proj(vp);
    let eye = EyeView::from_view_proj(vp, H as f32);
    let ground = |x: f32, z: f32| map.height_at(x, z);
    let stats = inst.build_frame(
        &FrameView::new(&frustum, 1.0, eye, Blend::MODELS, 1.0),
        ground,
    );
    assert_eq!(stats.models, 5);
    assert!(
        stats.per_lod[0] >= 3,
        "close zoom: finest level {:?}",
        stats.per_lod
    );
    // The scout is one part (its turret and barrel nodes are empty), the
    // four tanks three each: every part of every model is its own mesh.
    assert_eq!(stats.model_draw_calls, 13);
    r.set_view_proj(vp);
    r.upload_units(&inst);
    let img = r.render_to_image().expect("render");
    let draws = r.draw_stats();
    assert_eq!(draws.dynamic_draw_calls, 13);
    assert_eq!(draws.dynamic_instances, 13);
    image::write_png(&std::env::temp_dir().join("glb_testpack_close.png"), &img).ok();

    // Player colours: the team material of the red and the blue units.
    let (red, blue) = (
        player_pixels(&style, &img, 0),
        player_pixels(&style, &img, 1),
    );
    println!("player pixels: red {red}, blue {blue}");
    assert!(red > 150 && blue > 150, "red {red}, blue {blue}");

    // Turrets visible: drop the turret instances and the picture changes.
    let mut changed = 0;
    {
        let turrets: Vec<MeshId> = inst
            .models()
            .models()
            .iter()
            .filter_map(|chain| chain.parts(0).iter().find(|p| p.joint == Joint::Yaw))
            .map(|p| p.mesh)
            .collect();
        assert_eq!(turrets.len(), 7, "seven test-pack models have a turret");
        for turret in turrets {
            r.set_mesh_instances(turret, &[]);
        }
        let without = r.render_to_image().expect("render");
        for y in 0..H {
            for x in 0..W {
                let (a, b) = (img.pixel(x, y), without.pixel(x, y));
                changed += usize::from((0..3).any(|i| a[i].abs_diff(b[i]) > 12));
            }
        }
    }
    println!("turret pixels: {changed}");
    assert!(changed > 200, "turrets cover {changed} pixels");

    check_golden("glb_testpack_close_320.png", &img);
}
