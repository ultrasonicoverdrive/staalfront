//! Screenshot tests for the game camera: the 256 m world scene rendered
//! through the real [`Camera`] at close, middle and whole-map zoom, golden
//! images plus content checks that do not depend on the goldens, and an
//! end-to-end check of zooming toward the cursor on actual pixels.
//!
//! Same adapter rules as `screenshot.rs`: software adapter preferred, no
//! adapter = failure. Regenerate goldens with
//! `SF_UPDATE_GOLDEN=1 cargo test -p sf_render --test camera_screenshot`.

use std::path::PathBuf;

use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8, Tolerance, compare};
use sf_render::scene::{WORLD_MARKER, world_scene};
use sf_render::{Camera, CameraConfig, CameraInput, GpuOptions, Renderer};

const W: u32 = 320;
const H: u32 = 180;

/// See `screenshot.rs`: 12/255 per channel, at most 2 % of the pixels.
const TOLERANCE: Tolerance = Tolerance {
    channel: 12,
    max_ratio: 0.02,
};

fn renderer() -> Renderer {
    let mut r = Renderer::headless(W, H, &GpuOptions::software())
        .or_else(|software_err| {
            // On CI a missing software adapter must not be masked.
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
    r.set_scene(&world_scene(256));
    r
}

fn shot(r: &mut Renderer, camera: &Camera) -> Rgba8 {
    r.set_view_proj(camera.view_proj());
    r.render_to_image().expect("render")
}

fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

fn check_golden(name: &str, actual: &Rgba8) {
    let path = golden(name);
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

fn camera_at(zoom: f64, focus: (f64, f64)) -> Camera {
    let mut c = Camera::new(CameraConfig::default(), W, H);
    c.set_zoom(zoom);
    c.set_focus(DVec2::new(focus.0, focus.1));
    c
}

/// Screen pixel of a ground point.
fn project(c: &Camera, x: f64, z: f64) -> (u32, u32) {
    let clip = c.view_proj_f64() * glam::DVec4::new(x, 0.0, z, 1.0);
    let (nx, ny) = (clip.x / clip.w, clip.y / clip.w);
    (
        ((nx * 0.5 + 0.5) * f64::from(W)).round() as u32,
        ((0.5 - ny * 0.5) * f64::from(H)).round() as u32,
    )
}

fn hue(p: [u8; 4]) -> char {
    let [r, g, b, _] = p;
    if r >= g && r >= b {
        'r'
    } else if g >= b {
        'g'
    } else {
        'b'
    }
}

fn is_magenta(p: [u8; 4]) -> bool {
    let [r, g, b, _] = p;
    r > 80 && b > 80 && f32::from(g) < 0.6 * f32::from(r.min(b))
}

fn is_red(p: [u8; 4]) -> bool {
    p[0] > 100 && u32::from(p[0]) > 2 * u32::from(p[1]) && u32::from(p[0]) > 2 * u32::from(p[2])
}

fn distinct_colors(img: &Rgba8) -> usize {
    let mut seen = std::collections::BTreeSet::new();
    for y in 0..img.height {
        for x in 0..img.width {
            let p = img.pixel(x, y);
            seen.insert([p[0] >> 4, p[1] >> 4, p[2] >> 4]);
        }
    }
    seen.len()
}

#[test]
fn close_zoom() {
    let img = shot(&mut renderer(), &camera_at(0.0, (128.0, 128.0)));
    check_golden("camera_close_320.png", &img);
    // 12 m away at 45 degrees: the red tank block fills the middle...
    let mid = img.pixel(W / 2, H / 2);
    assert!(is_red(mid), "{mid:?}");
    // ...no sky at this pitch: the ground reaches the top of the frame...
    assert_eq!(hue(img.pixel(W / 2, 2)), 'g', "{:?}", img.pixel(W / 2, 2));
    // ...and the block is big (a few metres across a ~17 m wide view).
    let red = (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .filter(|&(x, y)| is_red(img.pixel(x, y)))
        .count();
    assert!(red > 2_000, "red block covers {red} px");
}

#[test]
fn middle_zoom() {
    let img = shot(&mut renderer(), &camera_at(0.5, (128.0, 128.0)));
    check_golden("camera_mid_320.png", &img);
    let mid = img.pixel(W / 2, H / 2);
    assert!(is_red(mid), "{mid:?}");
    assert!(
        distinct_colors(&img) > 12,
        "grid of colored pillars expected"
    );
}

#[test]
fn whole_map_zoom() {
    let c = camera_at(1.0, (128.0, 128.0));
    let img = shot(&mut renderer(), &c);
    check_golden("camera_far_320.png", &img);
    // The 16:9 window is wider than the square map: sky-blue clear color at
    // both side edges, ground (green) just inside each map corner.
    assert_eq!(hue(img.pixel(1, H / 2)), 'b');
    assert_eq!(hue(img.pixel(W - 2, H / 2)), 'b');
    for (x, z) in [(3.0, 3.0), (253.0, 3.0), (3.0, 253.0), (253.0, 253.0)] {
        let (px, py) = project(&c, x, z);
        assert!(
            px < W && py < H,
            "corner {x},{z} outside the frame: {px},{py}"
        );
        let p = img.pixel(px, py);
        assert_eq!(hue(p), 'g', "corner {x},{z}: {p:?}");
    }
    assert!(distinct_colors(&img) > 12);
}

/// Zooming in with the wheel toward a cursor over the magenta marker keeps
/// the marker under the cursor in the rendered image, from the whole map down
/// to the closest zoom. The same sweep about the screen centre loses it,
/// which proves the check can fail.
#[test]
fn wheel_zoom_keeps_the_marker_under_the_cursor() {
    let mut r = renderer();
    let mut c = camera_at(1.0, (128.0, 128.0));
    let (px, py) = project(&c, f64::from(WORLD_MARKER.0), f64::from(WORLD_MARKER.1));
    let cursor = DVec2::new(f64::from(px), f64::from(py));
    let mut control = c.clone();
    let p = shot(&mut r, &c).pixel(px, py);
    assert!(
        is_magenta(p),
        "marker not under the cursor to begin with: {p:?}"
    );

    let step = |cam: &mut Camera, at: Option<DVec2>, notches: f64| {
        cam.update(
            1.0 / 60.0,
            &CameraInput {
                zoom_notches: notches,
                cursor: at,
                ..CameraInput::default()
            },
        );
        for _ in 0..120 {
            cam.update(
                1.0 / 60.0,
                &CameraInput {
                    cursor: at,
                    ..CameraInput::default()
                },
            );
        }
    };

    let mut last = None;
    for stage in 0..4 {
        step(&mut c, Some(cursor), 6.0);
        let img = shot(&mut r, &c);
        let p = img.pixel(px, py);
        assert!(is_magenta(p), "stage {stage} (zoom {:.2}): {p:?}", c.zoom());
        last = Some(img);
    }
    assert_eq!(c.zoom(), 0.0, "ended at the closest zoom");
    check_golden("camera_cursor_zoom_320.png", &last.unwrap());

    // Control: the same zoom about the screen centre loses the marker.
    for _ in 0..4 {
        step(&mut control, None, 6.0);
    }
    let p = shot(&mut r, &control).pixel(px, py);
    assert!(!is_magenta(p), "control still shows the marker: {p:?}");
}
