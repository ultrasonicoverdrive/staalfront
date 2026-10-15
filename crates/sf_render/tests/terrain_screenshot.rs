//! Screenshot tests for the terrain (WP1.7): the committed 64x64 test map
//! (`assets/maps/test64`) rendered through the real camera, golden images plus
//! content checks that do not depend on the goldens (colour variety, every
//! material shows its colour, sun-facing slopes brighter than shaded ones).
//!
//! Same adapter rules as `screenshot.rs`: software adapter preferred, no
//! adapter = failure; on CI (`CI` set) a missing software adapter is not
//! masked. Regenerate goldens with
//! `SF_UPDATE_GOLDEN=1 cargo test -p sf_render --test terrain_screenshot`.

use std::path::PathBuf;

use glam::{DVec4, Vec3};
use sf_data::map::Map;
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8, Tolerance, compare};
use sf_render::scene::{Lighting, world_markers};
use sf_render::terrain::vertex_normal;
use sf_render::{Camera, CameraConfig, GpuOptions, Renderer};

const W: u32 = 320;
const H: u32 = 180;

/// See `screenshot.rs`: 12/255 per channel, at most 2 % of the pixels.
const TOLERANCE: Tolerance = Tolerance {
    channel: 12,
    max_ratio: 0.02,
};

fn test_map() -> Map {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps/test64");
    Map::load(&dir).expect("committed test map loads")
}

fn renderer(map: &Map, markers: bool) -> Renderer {
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
    let objects = if markers {
        world_markers(map.width(), |x, z| map.height_at(x, z))
    } else {
        Vec::new()
    };
    r.set_scene(&objects);
    r.set_terrain(map);
    assert_eq!(r.terrain_chunk_count(), 4);
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

fn shot(r: &mut Renderer, camera: &Camera) -> Rgba8 {
    r.set_view_proj(camera.view_proj());
    r.render_to_image().expect("render")
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

/// Screen pixel of a world point, `None` if it is outside the frame (with a
/// margin) or behind the camera.
fn project(c: &Camera, p: Vec3) -> Option<(u32, u32)> {
    let clip = c.view_proj_f64() * DVec4::new(f64::from(p.x), f64::from(p.y), f64::from(p.z), 1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let (nx, ny) = (clip.x / clip.w, clip.y / clip.w);
    if nx.abs() > 0.9 || ny.abs() > 0.9 {
        return None;
    }
    Some((
        ((nx * 0.5 + 0.5) * f64::from(W)).round() as u32,
        ((0.5 - ny * 0.5) * f64::from(H)).round() as u32,
    ))
}

fn luminance(p: [u8; 4]) -> f32 {
    0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2])
}

fn distinct_colors(img: &Rgba8) -> usize {
    let mut seen = std::collections::BTreeSet::new();
    for y in 0..img.height {
        for x in 0..img.width {
            let p = img.pixel(x, y);
            seen.insert([p[0] >> 3, p[1] >> 3, p[2] >> 3]);
        }
    }
    seen.len()
}

fn sky_pixels(img: &Rgba8) -> usize {
    // Linear clear colour (0.08, 0.14, 0.24) is about (79, 104, 138) in sRGB.
    (0..img.height)
        .flat_map(|y| (0..img.width).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = img.pixel(x, y);
            p[2] > p[1] && p[1] > p[0] && p[2] > 120 && p[0] < 100
        })
        .count()
}

fn lit_fraction(map: &Map, vx: i64, vz: i64) -> f32 {
    let to_light = (-Lighting::default().direction).normalize();
    Vec3::from(vertex_normal(map, vx, vz)).dot(to_light)
}

fn is_flat(map: &Map, vx: i64, vz: i64) -> bool {
    vertex_normal(map, vx, vz)[1] > 0.95
}

#[test]
fn whole_map_shows_relief_and_all_materials() {
    let map = test_map();
    let c = camera(&map, 1.0, (32.0, 32.0));
    let img = shot(&mut renderer(&map, false), &c);
    check_golden("terrain_far_320.png", &img);
    assert!(distinct_colors(&img) > 60, "{}", distinct_colors(&img));

    // Interior cells (all four neighbours share the material) show their
    // material's colour at the cell centre.
    let mut checked = [0usize; 5];
    for cz in 1..i64::from(map.height()) - 1 {
        for cx in 1..i64::from(map.width()) - 1 {
            let m = map.material_at(cx, cz);
            let same = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .all(|&(dx, dz)| map.material_at(cx + dx, cz + dz) == m);
            let flat = is_flat(&map, cx, cz) && is_flat(&map, cx + 1, cz + 1);
            if !same || !flat {
                continue;
            }
            let h = map.height_at(cx as f32 + 0.5, cz as f32 + 0.5);
            let Some((px, py)) = project(&c, Vec3::new(cx as f32 + 0.5, h, cz as f32 + 0.5)) else {
                continue;
            };
            let p = img.pixel(px, py);
            let (r, g, b) = (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
            let ok = match m {
                0 => g > r && g > b,                          // grass: green
                1 => r > g && g > b && r - b > 25,            // dirt: brown
                2 => (r - b).abs() < 28 && g > 60,            // rock: grey
                3 => r > 130 && r - b > 40,                   // sand: pale yellow
                4 => r < 110 && g < 100 && b < 100 && r >= b, // dark ground
                _ => unreachable!(),
            };
            assert!(ok, "material {m} at cell ({cx},{cz}) looks like {p:?}");
            checked[usize::from(m)] += 1;
        }
    }
    println!("interior flat cells checked per material: {checked:?}");
    assert!(
        checked.iter().filter(|&&n| n >= 2).count() >= 4,
        "too few materials verified: {checked:?}"
    );
}

/// Mean luminance of the most sun-facing tenth and the most shaded tenth of
/// the visible vertices of `map` (ranked by its normals), read from `img`
/// (which may show a different map), plus the spread of the facing values.
fn slope_contrast(map: &Map, img: &Rgba8, c: &Camera) -> (f32, f32, f32) {
    let mut seen = Vec::new();
    for vz in 0..=i64::from(map.height()) {
        for vx in 0..=i64::from(map.width()) {
            let y = map.vertex_height(vx, vz);
            if let Some((px, py)) = project(c, Vec3::new(vx as f32, y, vz as f32)) {
                seen.push((lit_fraction(map, vx, vz), luminance(img.pixel(px, py))));
            }
        }
    }
    seen.sort_by(|a, b| a.0.total_cmp(&b.0));
    let n = seen.len() / 10;
    assert!(n > 50, "{n}");
    let mean = |v: &[(f32, f32)]| v.iter().map(|p| p.1).sum::<f32>() / v.len() as f32;
    let (shaded, lit) = (&seen[..n], &seen[seen.len() - n..]);
    (mean(lit), mean(shaded), lit[0].0 - shaded[n - 1].0)
}

/// The same map with the same materials but flat ground.
fn flattened(map: &Map) -> Map {
    let flat = vec![10_000; map.heights().len()];
    Map::from_parts(
        map.width(),
        map.height(),
        map.height_scale(),
        map.materials().to_vec(),
        flat,
        map.cells().to_vec(),
    )
    .expect("flat copy")
}

#[test]
fn sun_facing_slopes_are_brighter_than_shaded_ones() {
    let map = test_map();
    let c = camera(&map, 0.55, (32.0, 32.0));
    let img = shot(&mut renderer(&map, false), &c);
    let (lit, shaded, spread) = slope_contrast(&map, &img, &c);
    println!("relief: lit {lit:.1}, shaded {shaded:.1}, facing spread {spread:.2}");
    assert!(spread > 0.4, "the test map has no real slopes: {spread}");
    assert!(lit > shaded * 1.25, "lit {lit:.1} vs shaded {shaded:.1}");

    // Control: the same materials on flat ground must NOT show that contrast
    // (measured with the relief map's ranking), proving the check can fail.
    let flat = flattened(&map);
    let flat_img = shot(&mut renderer(&flat, false), &c);
    let (lit, shaded, _) = slope_contrast(&map, &flat_img, &c);
    println!("flat control: lit {lit:.1}, shaded {shaded:.1}");
    assert!(
        lit < shaded * 1.12,
        "flat map shows slope shading: {lit:.1} vs {shaded:.1}"
    );
}

/// Low-angle shot (zoomed in, pitch about 45 degrees) on the steepest part of
/// the test map: the shading must clearly separate sun-facing from shaded
/// slopes, in one material (grass-only render of that area is not needed:
/// the contrast is measured against the flat render of the same view).
#[test]
fn relief_close_up_shows_lit_and_shaded_slopes() {
    let map = test_map();
    // Steepest 9x9 window of the map, found from the heightmap normals.
    let (mut best, mut focus) = (-1.0_f32, (32.0, 32.0));
    for vz in 6..=i64::from(map.height()) - 6 {
        for vx in 6..=i64::from(map.width()) - 6 {
            let mut steep = 0.0;
            for dz in -4..=4 {
                for dx in -4..=4 {
                    steep += 1.0 - vertex_normal(&map, vx + dx, vz + dz)[1];
                }
            }
            if steep > best {
                best = steep;
                focus = (vx as f64, vz as f64);
            }
        }
    }
    println!("steepest area around {focus:?}");
    let c = camera(&map, 0.1, focus);
    let img = shot(&mut renderer(&map, false), &c);
    check_golden("terrain_relief_320.png", &img);
    let flat_img = shot(&mut renderer(&flattened(&map), false), &c);
    // Per-pixel difference to the flat render of the same view: the relief
    // (lighting and perspective) changes a large part of the frame.
    let diff = compare(&img, &flat_img, 30).expect("same size");
    println!("relief vs flat: ratio {:.3}", diff.ratio());
    assert!(
        diff.ratio() > 0.3,
        "relief barely changes the picture: {:.3}",
        diff.ratio()
    );
    // Luminance spread across the frame (lit slope vs shaded slope).
    let l: Vec<f32> = (0..img.height)
        .flat_map(|y| (0..img.width).map(move |x| (x, y)))
        .map(|(x, y)| luminance(img.pixel(x, y)))
        .collect();
    let (lo, hi) = (percentile(&l, 0.05), percentile(&l, 0.95));
    println!("luminance p5 {lo:.1}, p95 {hi:.1}");
    assert!(hi > lo * 1.5, "flat-looking frame: p5 {lo:.1}, p95 {hi:.1}");
}

fn percentile(values: &[f32], q: f32) -> f32 {
    let mut v = values.to_vec();
    v.sort_by(f32::total_cmp);
    v[((v.len() - 1) as f32 * q) as usize]
}

#[test]
fn close_zoom_with_markers_standing_on_the_terrain() {
    let map = test_map();
    let c = camera(&map, 0.05, (32.0, 32.0));
    let img = shot(&mut renderer(&map, true), &c);
    check_golden("terrain_close_320.png", &img);
    assert!(distinct_colors(&img) > 60, "{}", distinct_colors(&img));
    // Terrain fills the whole frame close up: no sky.
    assert_eq!(sky_pixels(&img), 0, "sky visible in a close-up");
}

/// Chunk seams: the border between the four chunks of the map is a straight
/// line at x = 32 / z = 32. Rendering must have no clear-colour pixels along
/// it (a crack would show the sky or the far terrain through).
#[test]
fn no_cracks_along_the_chunk_borders() {
    let map = test_map();
    let c = camera(&map, 0.45, (32.0, 32.0));
    let img = shot(&mut renderer(&map, false), &c);
    let mut probed = 0;
    for t in 0..64 {
        let t = f64::from(t) + 0.25;
        for (x, z) in [(32.0, t), (t, 32.0)] {
            let p = Vec3::new(x as f32, map.height_at(x as f32, z as f32), z as f32);
            if let Some((px, py)) = project(&c, p) {
                for dx in 0..=1 {
                    let pix = img.pixel((px + dx).min(W - 1), py);
                    assert!(
                        !(pix[2] > pix[1] && pix[1] > pix[0] && pix[2] > 120 && pix[0] < 100),
                        "sky-coloured pixel {pix:?} on the seam at ({x},{z})"
                    );
                }
                probed += 1;
            }
        }
    }
    assert!(probed > 30, "{probed}");
}
