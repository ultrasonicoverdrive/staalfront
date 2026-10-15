use super::*;
use crate::camera::{Camera, CameraConfig, DVec2};
use crate::ground::{Ground, TerrainGround};
use crate::terrain::{MAX_LEVEL, build_mesh, pack};
use sf_data::map::Map;
use sf_data::map::generate::generate;
use std::sync::Arc;

const W: u32 = 1920;
const H: u32 = 1080;

fn planner_for(map: &Map) -> LodPlanner {
    let mesh = build_mesh(map);
    let packed = pack(&mesh);
    LodPlanner::new(&mesh, packed.ranges)
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

fn view_of(cam: &Camera) -> (LodView, Frustum) {
    let vp = cam.view_proj();
    (
        LodView::from_view_proj(vp, H as f32),
        Frustum::from_view_proj(vp),
    )
}

/// A config with the pure error-budget selection: no joining of draw calls.
fn config(error_px: f32, hysteresis: f32) -> LodConfig {
    LodConfig {
        error_px,
        hysteresis,
        draw_call_cost: 0.0,
        ..LodConfig::default()
    }
}

// --- config ---

#[test]
fn the_shipped_config_is_valid_and_has_the_documented_defaults() {
    let c = TerrainConfig::embedded();
    assert_eq!(c, TerrainConfig::default());
    assert_eq!(c.lod.error_px, 2.0);
    assert_eq!(c.lod.hysteresis, 0.25);
    assert_eq!(c.lod.draw_call_cost, DEFAULT_DRAW_CALL_COST);
    assert_eq!(c.lod.force_level, None);
    assert!(c.seams);
    // Partial files fall back to the defaults.
    let c = TerrainConfig::from_ron("(lod: (error_px: 4.0))").unwrap();
    assert_eq!((c.lod.error_px, c.lod.hysteresis), (4.0, 0.25));
    let c = TerrainConfig::from_ron("(lod: (force_level: Some(3)), seams: false)").unwrap();
    assert_eq!(c.lod.force_level, Some(3));
    assert!(!c.seams);
}

#[test]
fn bad_configs_are_refused() {
    for bad in [
        "(lod: (error_px: 0.0))",
        "(lod: (error_px: -1.0))",
        "(lod: (hysteresis: 1.0))",
        "(lod: (hysteresis: -0.1))",
        "(lod: (draw_call_cost: -1.0))",
        "(lod: (force_level: Some(6)))",
        "(lod: (error_pixels: 2.0))",
        "(lod: (tint_levels: true))", // the tint is the debug view's switch
        "(seams: maybe)",
        "not ron",
    ] {
        assert!(TerrainConfig::from_ron(bad).is_err(), "{bad}");
    }
}

// --- the view ---

#[test]
fn the_view_is_recovered_from_the_view_projection() {
    let map = generate(256, 256, 1);
    for zoom in [0.0, 0.3, 0.7, 1.0] {
        for focus in [(128.0, 128.0), (40.0, 200.0)] {
            let cam = camera(&map, zoom, focus);
            let view = LodView::from_view_proj(cam.view_proj(), H as f32);
            let eye = cam.eye();
            let err = (view.eye.as_dvec3() - eye).length();
            assert!(
                err < 2e-3 * cam.distance().max(10.0),
                "zoom {zoom}: eye off by {err} m"
            );
            let expect = LodView::new(eye.as_vec3(), H as f32, cam.config().fov_y as f32);
            assert!((view.focal_px / expect.focal_px - 1.0).abs() < 1e-4);
        }
    }
}

#[test]
fn pixels_scale_with_error_and_inverse_distance() {
    let v = LodView::new(Vec3::ZERO, 1080.0, 45_f32.to_radians());
    assert!((v.focal_px - 1303.7).abs() < 0.5);
    assert!((v.pixels(1.0, 100.0) - 13.037).abs() < 0.01);
    assert!((v.pixels(2.0, 100.0) / v.pixels(1.0, 100.0) - 2.0).abs() < 1e-6);
    assert!((v.pixels(1.0, 50.0) / v.pixels(1.0, 100.0) - 2.0).abs() < 1e-6);
    // Never divides by zero.
    assert!(v.pixels(1.0, 0.0).is_finite());
}

#[test]
fn distance_to_a_box_is_zero_inside() {
    let b = Aabb::new(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));
    assert_eq!(distance_to_aabb(Vec3::ONE, &b), 0.0);
    assert!((distance_to_aabb(Vec3::new(5.0, 1.0, 1.0), &b) - 3.0).abs() < 1e-6);
    assert!((distance_to_aabb(Vec3::new(-3.0, -4.0, 1.0), &b) - 5.0).abs() < 1e-6);
}

// --- selection ---

/// Moving away from a chunk never makes its level finer.
#[test]
fn the_level_is_monotone_in_distance() {
    let map = generate(128, 128, 2);
    let p = planner_for(&map);
    for chunk in 0..p.chunk_count() {
        let aabb = *p.chunk_aabb(chunk);
        let centre = (aabb.min + aabb.max) * 0.5;
        let mut last = 0;
        let mut distance = 1.0_f32;
        while distance < 5000.0 {
            let view = LodView::new(centre + Vec3::new(0.0, distance, 0.0), 1080.0, 0.785);
            let level = p.coarsest_within(chunk, &view, 2.0);
            assert!(
                level >= last,
                "chunk {chunk}: level {level} after {last} at {distance} m"
            );
            last = level;
            distance *= 1.05;
        }
        assert_eq!(
            last,
            p.chunk_max_level(chunk),
            "far away every chunk is coarsest"
        );
    }
}

#[test]
fn close_up_chunks_are_full_detail_and_far_ones_are_coarse() {
    let map = generate(256, 256, 1);
    let mut p = planner_for(&map);
    let cfg = config(2.0, 0.25);
    let near = camera(&map, 0.0, (128.0, 128.0));
    let (view, fr) = view_of(&near);
    let stats = p.select(&view, &fr, &cfg, None).stats;
    println!("zoom 0: {stats:?}");
    assert!(stats.chunks_visible < 20);
    assert!(
        stats.per_level[0] >= 1,
        "the chunk under the camera is full detail"
    );
    let far = camera(&map, 1.0, (128.0, 128.0));
    let (view, fr) = view_of(&far);
    let stats = p.select(&view, &fr, &cfg, None).stats;
    println!("zoom 1: {stats:?}");
    assert_eq!(stats.chunks_visible, 64);
    assert!(
        stats.per_level[0] == 0,
        "whole map at 2 px: nothing needs full detail"
    );
}

/// The bound holds: every drawn chunk has a projected error within the
/// budget (or is at full detail), through a long random camera path with
/// hysteresis on.
#[test]
fn the_projected_error_never_exceeds_the_budget() {
    let map = generate(256, 256, 4);
    let mut p = planner_for(&map);
    let cfg = config(2.0, 0.25);
    let mut cam = camera(&map, 0.5, (128.0, 128.0));
    let mut seed = 99_u64;
    let mut rnd = move || {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        (seed >> 33) as f64 / (1_u64 << 31) as f64
    };
    let mut seen = [0_u32; LOD_LEVELS];
    for _frame in 0..400 {
        cam.set_zoom(rnd());
        cam.set_focus(DVec2::new(rnd() * 256.0, rnd() * 256.0));
        let (view, fr) = view_of(&cam);
        let sel = p.select(&view, &fr, &cfg, None).clone();
        for (id, &level) in sel.level.iter().enumerate() {
            if level == NOT_DRAWN {
                continue;
            }
            seen[usize::from(level)] += 1;
            let d = distance_to_aabb(view.eye, p.chunk_aabb(id));
            let px = view.pixels(p.chunk_error(id, level), d);
            assert!(
                level == 0 || px <= cfg.error_px * 1.0001,
                "chunk {id} level {level}: {px} px at {d} m"
            );
        }
    }
    println!("levels drawn over the path: {seen:?}");
    assert!(seen.iter().filter(|&&n| n > 0).count() >= 3);
}

/// Without hysteresis the selection is the coarsest level within budget.
#[test]
fn without_hysteresis_the_coarsest_level_within_budget_is_chosen() {
    let map = generate(256, 256, 4);
    let mut p = planner_for(&map);
    let cfg = config(1.5, 0.0);
    for zoom in [0.1, 0.4, 0.8, 1.0] {
        let cam = camera(&map, zoom, (100.0, 150.0));
        let (view, fr) = view_of(&cam);
        let sel = p.select(&view, &fr, &cfg, None).clone();
        for (id, &level) in sel.level.iter().enumerate() {
            if level == NOT_DRAWN {
                continue;
            }
            let d = distance_to_aabb(view.eye, p.chunk_aabb(id));
            assert_eq!(level, p.coarsest_within(id, &view, 1.5));
            if level < p.chunk_max_level(id) {
                let next = view.pixels(p.chunk_error(id, level + 1), d);
                assert!(
                    next > 1.5,
                    "chunk {id}: level {} would fit ({next} px)",
                    level + 1
                );
            }
        }
    }
}

/// A camera that shivers around a switching distance flickers without
/// hysteresis and holds still with it.
#[test]
fn hysteresis_stops_flicker() {
    let map = generate(128, 128, 5);
    // Any frustum that contains chunk 0 will do: the wobbling is in the view.
    let (_, frustum) = view_of(&camera(&map, 1.0, (64.0, 64.0)));
    let count_changes = |hysteresis: f32| {
        let mut p = planner_for(&map);
        let cfg = config(2.0, hysteresis);
        let aabb = *p.chunk_aabb(0);
        let centre = (aabb.min + aabb.max) * 0.5;
        let at = |d: f32| LodView::new(centre + Vec3::new(0.0, d, 0.0), 1080.0, 0.785);
        // Find the distance where level 2 first fits the budget.
        let mut d = 1.0_f32;
        while p.coarsest_within(0, &at(d), 2.0) < 2 {
            d *= 1.001;
        }
        let switch = d;
        let mut changes = 0;
        let mut last = None;
        for i in 0..200 {
            // A wobble of +-3 % around the switching distance.
            let wobble = 1.0 + 0.03 * ((i % 2) as f32 * 2.0 - 1.0);
            let level = p.select(&at(switch * wobble), &frustum, &cfg, None).level[0];
            assert_ne!(level, NOT_DRAWN);
            if last.is_some_and(|l| l != level) {
                changes += 1;
            }
            last = Some(level);
        }
        changes
    };
    let without = count_changes(0.0);
    let with = count_changes(0.25);
    println!("level changes in 200 wobbling frames: {without} without hysteresis, {with} with");
    assert!(without >= 100, "{without}");
    assert!(with <= 1, "{with}");
}

// --- culling and draw list ---

/// Every point of the terrain surface that is inside the frustum belongs
/// to a chunk that is drawn.
#[test]
fn culling_never_drops_a_chunk_that_shows_terrain() {
    let map = generate(256, 256, 7);
    let ground = TerrainGround::new(&map);
    let mut p = planner_for(&map);
    let cfg = config(2.0, 0.25);
    let mut culled_somewhere = false;
    for (zoom, focus) in [
        (0.0, (40.0, 40.0)),
        (0.2, (200.0, 60.0)),
        (0.5, (128.0, 128.0)),
        (0.8, (30.0, 220.0)),
        (1.0, (128.0, 128.0)),
    ] {
        let mut cam = camera(&map, zoom, focus);
        cam.set_ground(Arc::new(ground.clone()));
        let (view, fr) = view_of(&cam);
        let sel = p.select(&view, &fr, &cfg, None).clone();
        culled_somewhere |= sel.stats.chunks_visible < sel.stats.chunks_total;
        let mut checked = 0;
        for ix in 0..256 {
            for iz in 0..256 {
                let (x, z) = (ix as f32 + 0.5, iz as f32 + 0.5);
                let y = ground.height(f64::from(x), f64::from(z)) as f32;
                if fr.contains_point(Vec3::new(x, y, z)) {
                    let chunk = (iz / 32) * 8 + ix / 32;
                    assert_ne!(sel.level[chunk], NOT_DRAWN, "({x}, {z}) at zoom {zoom}");
                    checked += 1;
                }
            }
        }
        assert!(checked > 100, "zoom {zoom}");
    }
    assert!(culled_somewhere, "the test views never cull anything");
}

#[test]
fn zooming_in_culls_most_chunks() {
    let map = generate(256, 256, 1);
    let mut p = planner_for(&map);
    let cfg = config(2.0, 0.25);
    let far = camera(&map, 1.0, (128.0, 128.0));
    let (view, fr) = view_of(&far);
    let all = p.select(&view, &fr, &cfg, None).stats;
    assert_eq!((all.chunks_total, all.chunks_visible), (64, 64));
    let near = camera(&map, 0.15, (100.0, 100.0));
    let (view, fr) = view_of(&near);
    let close = p.select(&view, &fr, &cfg, None).stats;
    assert!(close.chunks_visible >= 1 && close.chunks_visible * 4 < all.chunks_visible);
    assert!(close.triangles < all.triangles, "{close:?} vs {all:?}");
}

#[test]
fn the_draw_list_covers_exactly_the_drawn_chunks() {
    let map = generate(256, 256, 1);
    let mesh = build_mesh(&map);
    let packed = pack(&mesh);
    let ranges = packed.ranges.clone();
    let mut p = LodPlanner::new(&mesh, packed.ranges);
    let cfg = config(2.0, 0.25);
    for (zoom, focus) in [
        (0.3, (100.0, 90.0)),
        (0.7, (128.0, 128.0)),
        (1.0, (128.0, 128.0)),
    ] {
        let cam = camera(&map, zoom, focus);
        let (view, fr) = view_of(&cam);
        let sel = p.select(&view, &fr, &cfg, None).clone();
        // Index positions covered by the draws.
        let mut covered = vec![false; packed.indices.len()];
        for r in &sel.draws {
            for i in r.clone() {
                assert!(!covered[i as usize], "drawn twice");
                covered[i as usize] = true;
            }
        }
        let mut expect_triangles = 0;
        for (level, per_chunk) in ranges.iter().enumerate() {
            for (id, r) in per_chunk.iter().enumerate() {
                let drawn = usize::from(sel.level[id]) == level;
                assert!(
                    r.clone().all(|i| covered[i as usize] == drawn),
                    "chunk {id} level {level}: drawn {drawn}"
                );
                if drawn {
                    expect_triangles += r.len() as u32 / 3;
                }
            }
        }
        assert_eq!(sel.stats.triangles, expect_triangles);
        assert_eq!(sel.stats.draw_calls as usize, sel.draws.len());
        assert!(sel.stats.draw_calls <= sel.stats.chunks_visible);
        assert_eq!(
            sel.stats.per_level.iter().sum::<u32>(),
            sel.stats.chunks_visible
        );
    }
}

#[test]
fn chunks_of_one_level_become_one_draw_call() {
    let map = generate(256, 256, 1);
    let mut p = planner_for(&map);
    let cam = camera(&map, 1.0, (128.0, 128.0));
    let (view, fr) = view_of(&cam);
    for level in 0..=MAX_LEVEL {
        let cfg = LodConfig {
            force_level: Some(level),
            ..LodConfig::default()
        };
        let stats = p.select(&view, &fr, &cfg, None).stats;
        assert_eq!(stats.chunks_visible, 64);
        assert_eq!(stats.draw_calls, 1, "level {level}");
        assert_eq!(stats.per_level[usize::from(level)], 64);
    }
    // Culled chunks in the middle split the run.
    let cam = camera(&map, 0.2, (128.0, 128.0));
    let (view, fr) = view_of(&cam);
    let cfg = LodConfig {
        force_level: Some(0),
        ..LodConfig::default()
    };
    let s = p.select(&view, &fr, &cfg, None).stats;
    assert!(s.chunks_visible < 64 && s.draw_calls >= 1);
}

#[test]
fn explicit_levels_override_the_selection_and_clamp_to_the_chunk() {
    let map = generate(70, 40, 2); // 3 x 2 chunks, small ones at the edge
    let mut p = planner_for(&map);
    let cam = camera(&map, 1.0, (35.0, 20.0));
    let (view, fr) = view_of(&cam);
    let want = vec![5, 5, 5, 5, 5, 5];
    let sel = p
        .select(&view, &fr, &config(2.0, 0.25), Some(&want))
        .clone();
    for (id, &level) in sel.level.iter().enumerate() {
        assert_eq!(level, 5.min(p.chunk_max_level(id)), "chunk {id}");
    }
}

// --- scale ---

/// A terrain with cheap, smooth relief: the mesh build of the largest
/// map the format allows stays affordable in tests.
fn synthetic(side: u32) -> Map {
    let n = side as usize + 1;
    let heights = (0..n * n)
        .map(|i| {
            let (x, z) = ((i % n) as f32, (i / n) as f32);
            let h = 0.5
                + 0.25 * (x * 0.021).sin()
                + 0.2 * (z * 0.017 + x * 0.005).cos()
                + 0.05 * (x * 0.3).sin() * (z * 0.23).cos();
            (h.clamp(0.0, 1.0) * 65535.0) as u16
        })
        .collect();
    Map::from_parts(
        side,
        side,
        12.0,
        vec!["a".into()],
        heights,
        vec![0; (side * side) as usize],
    )
    .unwrap()
}

/// A bigger map with the generated hills: the 256 m map mirrored over and
/// over (continuous at the seams), so the relief is as steep as the real
/// thing and building it stays cheap (the generator itself is quadratic in
/// the map size).
fn mirrored_hills(side: u32) -> Map {
    let base = generate(256, 256, 1);
    let fold = |v: u32, size: u32| {
        let u = v % (2 * size);
        if u > size { 2 * size - u } else { u }
    };
    let heights = (0..=side)
        .flat_map(|z| (0..=side).map(move |x| (x, z)))
        .map(|(x, z)| base.raw_height(i64::from(fold(x, 256)), i64::from(fold(z, 256))))
        .collect();
    let cells = (0..side)
        .flat_map(|z| (0..side).map(move |x| (x, z)))
        .map(|(x, z)| {
            base.material_at(
                i64::from(fold(x, 256).min(255)),
                i64::from(fold(z, 256).min(255)),
            )
        })
        .collect();
    Map::from_parts(
        side,
        side,
        base.height_scale(),
        base.materials().to_vec(),
        heights,
        cells,
    )
    .expect("valid map")
}

/// Whole-map zoom: the requirement (<= 300 terrain draw calls on a
/// 256x256 map), and the same for the largest map the format allows. Joining
/// draw calls (`draw_call_cost`) is what keeps the 1024 hills below 300; the
/// numbers with and without it are printed for the report.
#[test]
fn whole_map_zoom_needs_few_draw_calls() {
    let cfg = LodConfig::default();
    let pure = config(cfg.error_px, cfg.hysteresis);
    // The generated hills at 256 and, mirrored, at 1024 (sixteen times the
    // hills of the same steepness), and a smooth 1024 map for the other extreme.
    for (side, smooth) in [(256_u32, false), (1024, false), (1024, true)] {
        let map = if smooth {
            synthetic(side)
        } else if side == 256 {
            generate(side, side, 1)
        } else {
            mirrored_hills(side)
        };
        let started = std::time::Instant::now();
        let mut p = planner_for(&map);
        let built = started.elapsed();
        let cam = camera(&map, 1.0, (f64::from(side) / 2.0, f64::from(side) / 2.0));
        let (view, fr) = view_of(&cam);

        let mut fresh = p.clone();
        let unjoined = p.select(&view, &fr, &pure, None).clone();
        let started = std::time::Instant::now();
        let joined = fresh.select(&view, &fr, &cfg, None).clone();
        let first = started.elapsed();
        let started = std::time::Instant::now();
        for _ in 0..50 {
            std::hint::black_box(fresh.select(&view, &fr, &cfg, None));
        }
        let each = started.elapsed() / 50;
        let (u, j) = (unjoined.stats, joined.stats);
        println!(
            "{side}x{side}{} whole map: {}/{} chunks drawn. Strict budget: {} draw calls, {} triangles, \
             levels {:?}. Joined: {} draw calls, {} triangles, levels {:?}. \
             Mesh build {built:?}, select+cull {first:?} first, {each:?} each",
            if smooth { " (smooth)" } else { "" },
            j.chunks_visible,
            j.chunks_total,
            u.draw_calls,
            u.triangles,
            u.per_level,
            j.draw_calls,
            j.triangles,
            j.per_level,
        );
        assert_eq!(j.chunks_visible, j.chunks_total, "the whole map is in view");
        assert!(j.draw_calls <= 300, "{} draw calls", j.draw_calls);
        assert!(j.draw_calls <= u.draw_calls);
        // Joining only ever makes chunks finer, and costs less than it saves.
        assert!(j.triangles >= u.triangles);
        let saved = f64::from(u.draw_calls - j.draw_calls);
        let extra = f64::from(j.triangles - u.triangles);
        assert!(extra <= saved * f64::from(cfg.draw_call_cost) + 1.0);
        for (id, (&a, &b)) in unjoined.level.iter().zip(&joined.level).enumerate() {
            assert!(b <= a, "chunk {id} went coarser: {a} -> {b}");
        }
        if side == 256 {
            assert_eq!(j.chunks_total, 64);
        }
    }
}

/// The joining is an exact optimisation: it finds the cheapest choice of
/// levels (triangles plus a price per run), checked against trying every
/// choice on small random cases.
#[test]
fn joining_draw_calls_finds_the_cheapest_levels() {
    let mut seed = 5_u64;
    let mut rnd = move |n: u64| {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (seed >> 33) % n
    };
    let run_cost = |ids: &[u32], levels: &[u8], tri: &dyn Fn(usize, u8) -> f32, cost: f32| {
        let mut total = 0.0;
        for k in 0..ids.len() {
            total += tri(k, levels[k]);
            if k == 0 || ids[k - 1] + 1 != ids[k] || levels[k - 1] != levels[k] {
                total += cost;
            }
        }
        total
    };
    let (mut dp, mut back, mut out) = (Vec::new(), Vec::new(), Vec::new());
    let mut joined_cases = 0;
    for case in 0..400 {
        let n = 1 + rnd(6) as usize;
        // Ascending ids, mostly consecutive.
        let mut ids = Vec::new();
        let mut id = rnd(10) as u32;
        for _ in 0..n {
            ids.push(id);
            id += 1 + u32::from(rnd(4) == 0);
        }
        let target: Vec<u8> = (0..n).map(|_| rnd(4) as u8).collect();
        // Triangles per chunk and level: falling steeply with the level.
        let base: Vec<f32> = (0..n).map(|_| 8.0 + rnd(200) as f32).collect();
        let tri = |k: usize, level: u8| base[k] / 4_f32.powi(i32::from(level));
        let cost = rnd(120) as f32;
        merge_levels(&ids, &target, tri, cost, &mut dp, &mut back, &mut out);
        assert_eq!(out.len(), n);
        assert!(
            out.iter().zip(&target).all(|(l, t)| l <= t),
            "case {case}: {out:?} exceeds {target:?}"
        );
        let got = run_cost(&ids, &out, &tri, cost);
        // Every assignment with level <= target.
        let mut best = f32::INFINITY;
        let mut levels = vec![0_u8; n];
        loop {
            best = best.min(run_cost(&ids, &levels, &tri, cost));
            let mut k = 0;
            while k < n {
                if levels[k] < target[k] {
                    levels[k] += 1;
                    break;
                }
                levels[k] = 0;
                k += 1;
            }
            if k == n {
                break;
            }
        }
        assert!(
            (got - best).abs() < 1e-3,
            "case {case}: ids {ids:?} targets {target:?} cost {cost}: chose {out:?} for {got}, best {best}"
        );
        joined_cases += usize::from(out != target);
    }
    // The cases are not all trivial: joining does change levels.
    assert!(joined_cases > 50, "{joined_cases}");
    // No price, no joining.
    let ids = [0, 1, 2, 3];
    merge_levels(
        &ids,
        &[3, 1, 3, 2],
        |_, l| 100.0 / 4_f32.powi(l.into()),
        0.0,
        &mut dp,
        &mut back,
        &mut out,
    );
    assert_eq!(out, [3, 1, 3, 2]);
    // A huge price joins everything that touches, down to the finest target.
    merge_levels(
        &ids,
        &[3, 1, 3, 2],
        |_, l| 100.0 / 4_f32.powi(l.into()),
        1.0e5,
        &mut dp,
        &mut back,
        &mut out,
    );
    assert_eq!(out, [1, 1, 1, 1]);
    // Chunks that do not touch cannot share a draw call, whatever the price.
    merge_levels(
        &[0, 5],
        &[3, 1],
        |_, l| 100.0 / 4_f32.powi(l.into()),
        1.0e5,
        &mut dp,
        &mut back,
        &mut out,
    );
    assert_eq!(out, [3, 1]);
}

#[test]
fn triangle_counts_close_and_far() {
    let map = generate(256, 256, 1);
    let mut p = planner_for(&map);
    let cfg = LodConfig::default();
    let full = LodConfig::full_detail();
    let mut report = Vec::new();
    for (name, zoom) in [("close", 0.0), ("mid", 0.5), ("whole map", 1.0)] {
        let cam = camera(&map, zoom, (128.0, 128.0));
        let (view, fr) = view_of(&cam);
        let lod = p.select(&view, &fr, &cfg, None).stats;
        let none = p.select(&view, &fr, &full, None).stats;
        report.push(format!(
            "{name}: {} triangles with LOD, {} at full detail ({} chunks, {} draws)",
            lod.triangles, none.triangles, lod.chunks_visible, lod.draw_calls
        ));
        assert!(lod.triangles <= none.triangles);
    }
    println!("{}", report.join("\n"));
}
