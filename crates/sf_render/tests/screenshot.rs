//! Screenshot tests: render the built-in test scene offscreen and compare it
//! with a committed golden PNG.
//!
//! These tests need a GPU adapter. A software adapter is preferred (Mesa
//! lavapipe on Linux, DX12 WARP on Windows) so results are comparable
//! between machines. When no adapter exists the tests FAIL; they never skip.
//!
//! Regenerate the golden after an intended visual change with
//! `SF_UPDATE_GOLDEN=1 cargo test -p sf_render --test screenshot`.

use std::path::PathBuf;

use sf_render::image::{self, Rgba8, Tolerance, compare};
use sf_render::{GpuOptions, Renderer};

const SIZE: u32 = 256;

/// Software rasterizers (lavapipe vs. WARP) differ slightly in edge coverage
/// and rounding. A pixel only counts as different when a channel is off by
/// more than 12/255; up to 2 % of the pixels (triangle edges) may differ.
/// A black frame, swapped channels or a missing object miss this by far
/// (see `tolerance_rejects_broken_images`).
const TOLERANCE: Tolerance = Tolerance {
    channel: 12,
    max_ratio: 0.02,
};

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/test_scene_256.png")
}

/// Headless renderer on a software adapter if there is one, otherwise on any
/// adapter. No adapter at all is a hard failure.
fn renderer(width: u32, height: u32) -> Renderer {
    let r = Renderer::headless(width, height, &GpuOptions::software())
        .or_else(|software_err| {
            eprintln!("no software adapter ({software_err}); trying any adapter");
            Renderer::headless(
                width,
                height,
                &GpuOptions {
                    backends: sf_render::wgpu::Backends::PRIMARY,
                    ..GpuOptions::default()
                },
            )
        })
        .unwrap_or_else(|e| {
            panic!(
                "NO GPU ADAPTER AVAILABLE: {e}. On Linux install mesa-vulkan-drivers \
                 (lavapipe); on Windows DX12 WARP is built in."
            )
        });
    println!("adapter: {}", r.adapter_summary());
    r
}

fn scene_image() -> Rgba8 {
    renderer(SIZE, SIZE)
        .render_to_image()
        .expect("render test scene")
}

/// Dominant channel of a pixel as `'r'`, `'g'` or `'b'`.
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

#[test]
fn test_scene_matches_golden() {
    let actual = scene_image();
    let path = golden_path();
    if std::env::var_os("SF_UPDATE_GOLDEN").is_some_and(|v| v == "1") {
        image::write_png(&path, &actual).expect("write golden");
        println!("golden updated: {}", path.display());
        return;
    }
    let golden = image::read_png(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read golden {} ({e}); create it with SF_UPDATE_GOLDEN=1",
            path.display()
        )
    });
    let diff = compare(&actual, &golden, TOLERANCE.channel)
        .expect("golden has a different size than the render");
    println!(
        "golden diff: {} of {} pixels differ by more than {} (ratio {:.4}, max channel diff {})",
        diff.differing,
        diff.total,
        TOLERANCE.channel,
        diff.ratio(),
        diff.max_channel
    );
    if !diff.within(TOLERANCE) {
        let out = std::env::temp_dir().join("sf_render_actual.png");
        image::write_png(&out, &actual).ok();
        panic!(
            "render differs from golden: ratio {:.4} > {} (actual written to {})",
            diff.ratio(),
            TOLERANCE.max_ratio,
            out.display()
        );
    }
}

/// Independent of the golden: the image must show what the scene contains.
#[test]
fn test_scene_has_expected_content() {
    let img = scene_image();

    // Not a uniform (e.g. black or cleared-only) frame.
    let first = img.pixel(0, 0);
    let distinct = (0..SIZE)
        .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
        .filter(|&(x, y)| img.pixel(x, y) != first)
        .count();
    assert!(
        distinct > (SIZE * SIZE) as usize / 4,
        "image is nearly uniform ({distinct} pixels differ from the corner)"
    );

    // Sky (top-left corner) is blue-ish, ground (bottom-left) green, and the
    // red cube, blue cube and yellow pyramid show up with their hues.
    assert_eq!(hue(img.pixel(2, 2)), 'b', "sky {:?}", img.pixel(2, 2));
    assert_eq!(
        hue(img.pixel(8, 248)),
        'g',
        "ground {:?}",
        img.pixel(8, 248)
    );
    let red_cube = img.pixel(RED_CUBE.0, RED_CUBE.1);
    assert_eq!(hue(red_cube), 'r', "red cube {red_cube:?}");
    assert!(
        red_cube[0] > 100 && red_cube[1] < 60 && red_cube[2] < 60,
        "{red_cube:?}"
    );
    let blue_cube = img.pixel(BLUE_CUBE.0, BLUE_CUBE.1);
    assert_eq!(hue(blue_cube), 'b', "blue cube {blue_cube:?}");
    assert!(blue_cube[2] > 100 && blue_cube[0] < 60, "{blue_cube:?}");
    let pyramid = img.pixel(PYRAMID.0, PYRAMID.1);
    assert!(
        pyramid[0] > 150 && pyramid[1] > 120 && pyramid[2] < 100,
        "pyramid should be yellow, got {pyramid:?}"
    );

    // Lighting: the lit top of a cube is brighter than its shaded side face.
    let lum = |p: [u8; 4]| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]);
    let top = img.pixel(RED_CUBE_TOP.0, RED_CUBE_TOP.1);
    assert!(
        lum(top) > lum(red_cube),
        "top {top:?} should be brighter than side {red_cube:?}"
    );
}

// Sample points, read off the golden (256x256).
const RED_CUBE: (u32, u32) = (68, 135); // front face, in shade
const RED_CUBE_TOP: (u32, u32) = (65, 108); // top face, lit
const BLUE_CUBE: (u32, u32) = (144, 108);
const PYRAMID: (u32, u32) = (183, 145);

/// Proves the tolerance is tight enough to catch a broken render.
#[test]
fn tolerance_rejects_broken_images() {
    let golden = image::read_png(&golden_path()).expect("golden (see SF_UPDATE_GOLDEN)");

    // Identical passes.
    let same = compare(&golden, &golden, TOLERANCE.channel).unwrap();
    assert!(same.within(TOLERANCE));

    // A black frame.
    let black = Rgba8::new(
        golden.width,
        golden.height,
        [0, 0, 0, 255].repeat((golden.width * golden.height) as usize),
    );
    let d = compare(&golden, &black, TOLERANCE.channel).unwrap();
    assert!(!d.within(TOLERANCE), "black frame accepted: {d:?}");

    // Red and blue swapped (wrong color order).
    let mut swapped = golden.clone();
    for p in swapped.pixels.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    let d = compare(&golden, &swapped, TOLERANCE.channel).unwrap();
    assert!(!d.within(TOLERANCE), "swapped channels accepted: {d:?}");

    // Slightly different (edge-level noise on ~1 % of pixels) still passes.
    let mut noisy = golden.clone();
    for p in noisy.pixels.chunks_exact_mut(4).step_by(100) {
        p[0] = p[0].saturating_add(40);
    }
    let d = compare(&golden, &noisy, TOLERANCE.channel).unwrap();
    assert!(d.within(TOLERANCE), "small edge noise rejected: {d:?}");

    // The clear color alone (no objects) fails.
    let sky = Rgba8::new(
        golden.width,
        golden.height,
        golden
            .pixel(2, 2)
            .repeat((golden.width * golden.height) as usize),
    );
    let d = compare(&golden, &sky, TOLERANCE.channel).unwrap();
    assert!(!d.within(TOLERANCE), "empty scene accepted: {d:?}");
}

#[test]
fn non_square_and_resize() {
    let mut r = renderer(160, 90);
    let a = r.render_to_image().unwrap();
    assert_eq!((a.width, a.height), (160, 90));
    r.resize(96, 128);
    let b = r.render_to_image().unwrap();
    assert_eq!((b.width, b.height), (96, 128));
    assert_ne!(b.pixel(48, 64), b.pixel(1, 1));
}

/// A broken shader on hot-reload is rejected and the old pipeline keeps
/// rendering; a valid edit takes effect.
#[test]
fn shader_reload_keeps_old_pipeline_on_error() {
    let mut r = renderer(64, 64);
    let before = r.render_to_image().unwrap();

    let broken = "this is not wgsl {{{";
    let err = r.reload_shader(broken).expect_err("broken shader accepted");
    assert!(!err.is_empty());
    assert_eq!(
        r.render_to_image().unwrap(),
        before,
        "render changed after rejected reload"
    );

    // Valid WGSL that does not match the pipeline (missing fs_main) is also rejected.
    let wrong = "@vertex fn vs_main() -> @builtin(position) vec4<f32> { return vec4<f32>(0.0); }";
    assert!(r.reload_shader(wrong).is_err());
    assert_eq!(r.render_to_image().unwrap(), before);

    // A valid edit (solid magenta) is applied.
    let magenta = sf_render::EMBEDDED_SHADER.replace(
        "return vec4<f32>(in.color * lit, 1.0);",
        "return vec4<f32>(1.0, 0.0, 1.0, 1.0);",
    );
    assert_ne!(magenta, sf_render::EMBEDDED_SHADER);
    r.reload_shader(&magenta).expect("valid shader rejected");
    let after = r.render_to_image().unwrap();
    assert_ne!(after, before);
    let p = after.pixel(32, 56);
    assert!(p[0] > 200 && p[1] < 50 && p[2] > 200, "{p:?}");
}

/// The file watcher (debug builds only) picks up edits, survives a broken
/// file and recovers when it is fixed.
#[cfg(debug_assertions)]
#[test]
fn shader_file_watcher_hot_reload() {
    use std::time::Duration;

    let dir = std::env::temp_dir().join(format!("sf_render_hot_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("scene.wgsl");
    std::fs::write(&file, sf_render::EMBEDDED_SHADER).unwrap();

    let mut r = renderer(64, 64);
    r.watch_shader_file(file.clone(), Duration::ZERO);
    let original = r.render_to_image().unwrap();
    assert!(r.poll_shader_reload().is_none(), "no change yet");

    // mtime resolution can be coarse; make sure the edit gets a new stamp.
    let write = |text: &str| {
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(&file, text).unwrap();
    };

    write("broken {{{");
    assert!(matches!(r.poll_shader_reload(), Some(Err(_))));
    assert_eq!(r.render_to_image().unwrap(), original);

    let magenta = sf_render::EMBEDDED_SHADER.replace(
        "return vec4<f32>(in.color * lit, 1.0);",
        "return vec4<f32>(1.0, 0.0, 1.0, 1.0);",
    );
    write(&magenta);
    assert!(matches!(r.poll_shader_reload(), Some(Ok(()))));
    assert_ne!(r.render_to_image().unwrap(), original);

    write(sf_render::EMBEDDED_SHADER);
    assert!(matches!(r.poll_shader_reload(), Some(Ok(()))));
    assert_eq!(r.render_to_image().unwrap(), original);

    std::fs::remove_dir_all(&dir).ok();
}
