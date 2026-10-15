//! Screenshot tests for the terrain level of detail, the seams and the
//! picking (WP1.8): the 256x256 generated map at whole-map zoom with golden
//! image and content checks, chunk seams between different levels (with a
//! probe that fails when the skirts are switched off), the level tint,
//! the counters the renderer reports, and picking checked on actual pixels.
//!
//! Same adapter rules as `screenshot.rs`: software adapter preferred, no
//! adapter = failure; on CI (`CI` set) a missing software adapter is not
//! masked. Regenerate the golden with
//! `SF_UPDATE_GOLDEN=1 cargo test -p sf_render --test terrain_lod_screenshot`.

use std::path::PathBuf;
use std::sync::Arc;

use glam::{DVec4, Mat4, Vec3};
use sf_data::map::Map;
use sf_data::map::generate::generate;
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8, Tolerance, compare};
use sf_render::lod::LOD_TINT;
use sf_render::scene::{Lighting, MeshKind, SceneObject};
use sf_render::terrain::LOD_LEVELS;
use sf_render::{
    Camera, CameraConfig, DebugView, GpuOptions, Ground, LodConfig, Renderer, TerrainConfig,
    TerrainGround,
};

const W: u32 = 320;
const H: u32 = 180;

/// See `screenshot.rs`: 12/255 per channel, at most 2 % of the pixels.
const TOLERANCE: Tolerance = Tolerance {
    channel: 12,
    max_ratio: 0.02,
};

fn renderer(width: u32, height: u32) -> Renderer {
    let mut r = Renderer::headless(width, height, &GpuOptions::software())
        .or_else(|software_err| {
            if std::env::var_os("CI").is_some() {
                return Err(software_err);
            }
            Renderer::headless(
                width,
                height,
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

fn camera(map: &Map, size: (u32, u32), zoom: f64, focus: (f64, f64)) -> Camera {
    let cfg = CameraConfig {
        map_size: DVec2::new(f64::from(map.width()), f64::from(map.height())),
        ..CameraConfig::default()
    };
    let mut c = Camera::new(cfg, size.0, size.1);
    c.set_ground(Arc::new(TerrainGround::new(map)));
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

/// Pixel of the clear colour (linear (0.08, 0.14, 0.24) is about (79, 104, 138) in sRGB).
fn is_sky(p: [u8; 4]) -> bool {
    p[2] > p[1] && p[1] > p[0] && p[2] > 120 && p[0] < 100
}

fn sky_pixels(img: &Rgba8) -> usize {
    (0..img.height)
        .flat_map(|y| (0..img.width).map(move |x| (x, y)))
        .filter(|&(x, y)| is_sky(img.pixel(x, y)))
        .count()
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

fn luminance(p: [u8; 4]) -> f32 {
    0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2])
}

/// Screen pixel of a world point, `None` if behind the camera or outside the frame.
fn project(vp: &Mat4, size: (u32, u32), p: Vec3) -> Option<(u32, u32)> {
    let m = glam::DMat4::from_cols_array(&vp.to_cols_array().map(f64::from));
    let clip = m * DVec4::new(f64::from(p.x), f64::from(p.y), f64::from(p.z), 1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let (nx, ny) = (clip.x / clip.w, clip.y / clip.w);
    if nx.abs() >= 1.0 || ny.abs() >= 1.0 {
        return None;
    }
    Some((
        ((nx * 0.5 + 0.5) * f64::from(size.0)) as u32,
        ((0.5 - ny * 0.5) * f64::from(size.1)) as u32,
    ))
}

// ---------------------------------------------------------------------------

/// Whole-map zoom on the 256x256 generated map: far fewer triangles than at
/// full detail, a handful of draw calls, and a picture that is practically
/// the full-detail one.
#[test]
fn whole_map_with_relief_uses_far_fewer_triangles_and_looks_the_same() {
    let map = generate(256, 256, 1);
    let cam = camera(&map, (W, H), 1.0, (128.0, 128.0));

    let mut r = renderer(W, H);
    r.set_terrain(&map);
    assert_eq!(r.terrain_chunk_count(), 64);
    let lod = shot(&mut r, &cam);
    let stats = r.draw_stats();
    let tstats = r.terrain_stats();
    println!("with LOD: {stats:?} {tstats:?}");
    check_golden("terrain_lod_far_320.png", &lod);

    // The counters the renderer reports.
    assert_eq!(stats.terrain_chunks_total, 64);
    assert_eq!(stats.terrain_chunks_visible, 64);
    assert_eq!(stats.terrain_triangles, tstats.triangles);
    assert_eq!(stats.terrain_draw_calls, tstats.draw_calls);
    assert!(
        stats.terrain_draw_calls >= 1 && stats.terrain_draw_calls <= 300,
        "{} terrain draw calls",
        stats.terrain_draw_calls
    );

    // The same view at full detail.
    r.set_terrain_lod(LodConfig::full_detail());
    let full = shot(&mut r, &cam);
    let full_stats = r.terrain_stats();
    println!("full detail: {full_stats:?}");
    assert_eq!(full_stats.per_level[0], 64);
    assert!(
        tstats.triangles * 4 < full_stats.triangles,
        "LOD {} triangles vs full detail {}",
        tstats.triangles,
        full_stats.triangles
    );

    // Practically the same picture: LOD changes few pixels noticeably.
    let diff = compare(&lod, &full, 24).expect("same size");
    println!(
        "LOD vs full detail: {:.4} of the pixels differ by more than 24/255",
        diff.ratio()
    );
    assert!(diff.ratio() < 0.05, "{:.4}", diff.ratio());
    // And the picture is a real terrain picture.
    assert!(distinct_colors(&lod) > 60, "{}", distinct_colors(&lod));
    let sky_delta = sky_pixels(&lod).abs_diff(sky_pixels(&full));
    assert!(
        sky_delta < (W * H / 200) as usize,
        "the silhouette moved: {sky_delta} pixels"
    );
    // Relief is visible: lit and shaded slopes.
    let mut l: Vec<f32> = (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .map(|(x, y)| lod.pixel(x, y))
        .filter(|p| !is_sky(*p))
        .map(luminance)
        .collect();
    l.sort_by(f32::total_cmp);
    let (lo, hi) = (l[l.len() / 20], l[l.len() * 19 / 20]);
    assert!(hi > lo * 1.4, "flat-looking frame: p5 {lo:.1}, p95 {hi:.1}");
}

/// Triangle counts close and far, for the report.
#[test]
fn triangles_close_and_far() {
    let map = generate(256, 256, 1);
    let mut r = renderer(1920, 1080);
    r.set_terrain(&map);
    for (name, zoom) in [("close", 0.0), ("mid", 0.5), ("whole map", 1.0)] {
        let cam = camera(&map, (1920, 1080), zoom, (128.0, 128.0));
        r.set_view_proj(cam.view_proj());
        r.set_terrain_lod(LodConfig::default());
        r.render_to_image().expect("render");
        let lod = r.terrain_stats();
        r.set_terrain_lod(LodConfig::full_detail());
        r.render_to_image().expect("render");
        let full = r.terrain_stats();
        println!(
            "1080p {name}: {} triangles, {} chunks, {} draw calls with LOD; {} triangles at full detail",
            lod.triangles, lod.chunks_visible, lod.draw_calls, full.triangles
        );
        assert!(lod.triangles <= full.triangles);
    }
}

// --- seams ---

/// Two chunks on a plateau 6 m high. Along their shared edge (x = 32) every
/// second vertex is a pit down to 0 m. Full detail has the pits on both
/// sides; a coarser east chunk drops them and has a flat edge at 6 m, so
/// between the two there is a slit as deep as the pits.
///
/// A ray from the camera that crosses the edge from the west at a height
/// between the pit and the plateau passes into the ground under the east
/// plateau and, going down, leaves it nowhere: it sees the clear colour.
/// That is what makes cracks visible as sky-coloured pixels (on most terrain
/// a ray through a crack hits some other surface from below instead).
fn pit_seam_map() -> Map {
    let (w, h) = (64_u32, 32_u32);
    let mut heights = vec![65535_u16; ((w + 1) * (h + 1)) as usize];
    for z in (1..=h).step_by(2) {
        heights[(z * (w + 1) + 32) as usize] = 0;
    }
    Map::from_parts(
        w,
        h,
        6.0,
        vec!["grass".into()],
        heights,
        vec![0; (w * h) as usize],
    )
    .expect("valid map")
}

/// Neighbouring chunks at different levels show no hole between them with
/// the skirts; the same scene without skirts does (the probe), so the check
/// can fail.
#[test]
fn no_cracks_between_chunks_of_different_levels_and_the_probe_finds_them_without_skirts() {
    let (w, h) = (640, 360);
    let map = pit_seam_map();
    // The camera is west of the seam and looks north, so the rays on the
    // right half of the picture cross the seam from west to east.
    let cam = camera(&map, (w, h), 0.4, (24.0, 20.0));

    let render = |seams: bool, levels: Vec<u8>| {
        let mut r = renderer(w, h);
        r.set_terrain_with(
            &map,
            &TerrainConfig {
                seams,
                lod: LodConfig::default(),
            },
        );
        r.set_terrain_levels(Some(levels));
        let img = shot(&mut r, &cam);
        (img, r.terrain_stats())
    };

    // Reference: both chunks at full detail have no crack; whatever sky there
    // is belongs to the border of the map.
    let (reference, _) = render(true, vec![0, 0]);
    let baseline = sky_pixels(&reference);
    let (with_skirts, stats) = render(true, vec![0, 1]);
    assert_eq!(
        (stats.per_level[0], stats.per_level[1]),
        (1, 1),
        "{stats:?}"
    );
    let (without_skirts, _) = render(false, vec![0, 1]);
    let (holes, cracks) = (sky_pixels(&with_skirts), sky_pixels(&without_skirts));
    println!("sky pixels: {baseline} at full detail, {holes} with skirts, {cracks} without");
    assert!(
        cracks > baseline + 30,
        "the probe found no cracks without skirts: {cracks} vs {baseline}"
    );
    assert!(
        holes <= baseline + 2,
        "cracks between chunks of different levels: {holes} sky pixels vs {baseline}"
    );
    // The skirts change little else: elsewhere the pictures are the same.
    let diff = compare(&with_skirts, &without_skirts, 12).expect("same size");
    assert!(diff.ratio() < 0.02, "{:.4}", diff.ratio());
}

// --- level tint ---

/// With the tint on, each chunk shows the colour of its level. A flat map
/// (so the lighting is the same everywhere) of six chunks, one per level;
/// the tinted pixel must be the untinted one blended with the level's
/// colour, as `terrain.wgsl` does.
#[test]
fn level_tint_colours_chunks_by_level() {
    let (w, h) = (640, 160);
    let n = 193 * 33;
    let map = Map::from_parts(
        192,
        32,
        4.0,
        vec!["grass".into()],
        vec![20_000; n],
        vec![0; 192 * 32],
    )
    .expect("flat map");
    let cam = camera(&map, (w, h), 1.0, (96.0, 16.0));
    let mut r = renderer(w, h);
    r.set_terrain(&map);
    r.set_terrain_levels(Some((0..LOD_LEVELS as u8).collect()));
    let plain = shot(&mut r, &cam);
    let stats = r.terrain_stats();
    assert_eq!(stats.per_level, [1; LOD_LEVELS]);
    r.set_debug_view(DebugView {
        lod_tint: true,
        grid_alpha: 0.0,
    });
    let tinted = shot(&mut r, &cam);

    let light = Lighting::default();
    let to_light = (-light.direction).normalize();
    let lit = light.ambient + light.color * to_light.y.max(0.0);
    let to_linear = |c: u8| {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let to_srgb = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let v = if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        v * 255.0
    };
    let vp = cam.view_proj();
    let ground = TerrainGround::new(&map);
    let mut checked = [0_u32; LOD_LEVELS];
    for (level, count) in checked.iter_mut().enumerate() {
        // Sample a few points around the middle of the chunk.
        for (dx, dz) in [
            (-6.0, -5.0),
            (0.0, 0.0),
            (6.0, 5.0),
            (-4.0, 7.0),
            (5.0, -8.0),
        ] {
            let (x, z) = (32.0 * level as f32 + 16.0 + dx, 16.0 + dz);
            let y = ground.height(f64::from(x), f64::from(z)) as f32;
            let Some((px, py)) = project(&vp, (w, h), Vec3::new(x, y, z)) else {
                continue;
            };
            let (before, after) = (plain.pixel(px, py), tinted.pixel(px, py));
            for ch in 0..3 {
                let expect = 0.4 * to_linear(before[ch]) + 0.6 * LOD_TINT[level][ch] * lit[ch];
                let expect = to_srgb(expect);
                assert!(
                    (f32::from(after[ch]) - expect).abs() < 10.0,
                    "level {level} at ({x}, {z}) channel {ch}: {} vs expected {expect:.0} (untinted {})",
                    after[ch],
                    before[ch]
                );
            }
            *count += 1;
        }
        assert!(
            *count >= 3,
            "level {level}: only {count} sample points on screen"
        );
    }
    // The six colours are really different from one another.
    let mid = |level: usize| {
        let y = ground.height(0.0, 0.0) as f32;
        let (px, py) =
            project(&vp, (w, h), Vec3::new(32.0 * level as f32 + 16.0, y, 16.0)).unwrap();
        tinted.pixel(px, py)
    };
    for a in 0..LOD_LEVELS {
        for b in a + 1..LOD_LEVELS {
            let d: i32 = (0..3)
                .map(|c| (i32::from(mid(a)[c]) - i32::from(mid(b)[c])).abs())
                .sum();
            assert!(
                d > 60,
                "levels {a} and {b} look alike: {:?} {:?}",
                mid(a),
                mid(b)
            );
        }
    }
}

// --- picking on actual pixels ---

/// Where the old pick (the ray through the cursor against the plane `y = 0`)
/// lands for the same camera pose.
fn plane_pick(cam: &Camera, size: (u32, u32), px: DVec2) -> DVec2 {
    let vp = glam::DMat4::from_cols_array(&cam.view_proj().to_cols_array().map(f64::from));
    let inv = vp.inverse();
    let ndc = DVec2::new(
        2.0 * px.x / f64::from(size.0) - 1.0,
        1.0 - 2.0 * px.y / f64::from(size.1),
    );
    let near = inv * DVec4::new(ndc.x, ndc.y, 0.0, 1.0);
    let far = inv * DVec4::new(ndc.x, ndc.y, 1.0, 1.0);
    let (a, b) = (near.truncate() / near.w, far.truncate() / far.w);
    let t = -a.y / (b.y - a.y);
    DVec2::new(a.x + (b.x - a.x) * t, a.z + (b.z - a.z) * t)
}

/// A small bright cube standing on the picked point shows under the cursor
/// when the pick is a pick on the terrain, and does not when the point
/// comes from the flat plane `y = 0` (the control: what the camera did
/// before WP1.8).
#[test]
fn a_marker_at_the_picked_ground_point_shows_under_the_cursor() {
    let map = generate(256, 256, 1);
    let size = (W, H);
    let mut cam = camera(&map, size, 0.25, (128.0, 128.0));
    cam.set_focus(DVec2::new(60.0, 70.0));
    // The marker's sunlit top is bright, its shaded sides darker, but red and
    // blue always beat green by far; no terrain colour does.
    let magenta = |p: [u8; 4]| {
        let g2 = u16::from(p[1]) * 2;
        g2 < u16::from(p[0]) && g2 < u16::from(p[2]) && p[0] > 90 && p[2] > 100
    };
    let cursors = [
        (160.0, 90.0),
        (80.0, 60.0),
        (240.0, 120.0),
        (120.0, 140.0),
        (200.0, 40.0),
        (40.0, 100.0),
    ];
    let marker_shows = |at: DVec2, px: DVec2| {
        let y = cam.ground().height(at.x, at.y);
        let marker = SceneObject {
            mesh: MeshKind::Cube,
            transform: Mat4::from_translation(Vec3::new(at.x as f32, y as f32, at.y as f32))
                * Mat4::from_scale(Vec3::splat(1.6)),
            color: [0.9, 0.02, 0.9],
        };
        let mut r = renderer(W, H);
        r.set_scene(&[marker]);
        r.set_terrain(&map);
        r.set_terrain_lod(LodConfig::full_detail());
        let img = shot(&mut r, &cam);
        // The 3x3 pixels around the cursor.
        (0..3)
            .flat_map(|dy| (0..3).map(move |dx| (dx, dy)))
            .any(|(dx, dy)| magenta(img.pixel(px.x as u32 + dx - 1, px.y as u32 + dy - 1)))
    };

    let (mut on_terrain, mut on_plane, mut off_by) = (0, 0, 0.0_f64);
    for (cx, cy) in cursors {
        let px = DVec2::new(cx, cy);
        let picked = cam.ground_point(px).expect("ground under the cursor");
        let plane = plane_pick(&cam, size, px);
        off_by = f64::max(off_by, (picked - plane).length());
        on_terrain += u32::from(marker_shows(picked, px));
        on_plane += u32::from(marker_shows(plane, px));
    }
    println!(
        "marker under the cursor: {on_terrain}/6 at the terrain pick, {on_plane}/6 at the \
         flat-plane pick (up to {off_by:.1} m apart)"
    );
    assert_eq!(on_terrain, 6);
    assert!(
        on_plane <= 3,
        "the flat-plane control hits as well: {on_plane}/6"
    );
}
