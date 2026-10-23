//! Screenshot tests for the terrain types (WP2.2): the ground is drawn from the
//! gameplay terrain type of every 4 m cell, with soft borders and level water.
//!
//! The maps are built here (flat strips of cells), so the checks do not depend
//! on golden images; the committed pictures of the real maps are in
//! `docs/screenshots/wp2.2`. Same adapter rules as `screenshot.rs`: software
//! adapter preferred, no adapter = failure; on CI (`CI` set) a missing software
//! adapter is not masked.

use glam::{DVec4, Vec3};
use sf_data::map::Map;
use sf_data::terrain::{TerrainLayer, TerrainTypes};
use sf_render::camera::DVec2;
use sf_render::image::Rgba8;
use sf_render::{Camera, CameraConfig, GpuOptions, Renderer};

const W: u32 = 320;
const H: u32 = 180;
const CELL: f32 = 4.0;

const GRASS: u8 = 0;
const ROAD: u8 = 1;
const WATER: u8 = 2;
const ROCK: u8 = 3;
const ORE: u8 = 4;
const BRIDGE: u8 = 5;

/// `cw` x `ch` gameplay cells, `ids` row-major. `slope` is the height drop in
/// metres per metre towards +x (0 = flat). The material map says "sand" or
/// "grass" as `material` asks, to prove that it does not matter.
fn map(ids: &[u8], cw: u32, ch: u32, slope: f32, material: u8) -> Map {
    let (w, h) = (cw * 4, ch * 4);
    let scale = 64.0;
    let top = slope * w as f32;
    assert!(top <= scale);
    let mut heights = Vec::new();
    for _z in 0..=h {
        for x in 0..=w {
            let drop = slope * x as f32;
            heights.push((((top - drop) / scale) * 65535.0).round() as u16);
        }
    }
    let m = Map::from_parts(
        w,
        h,
        scale,
        vec!["grass".into(), "sand".into()],
        heights,
        vec![material; (w * h) as usize],
    )
    .expect("map");
    m.with_terrain(
        TerrainLayer::new(cw, ch, ids.to_vec()).expect("layer"),
        &TerrainTypes::embedded(),
    )
    .expect("terrain")
}

/// One column per type, `rows` rows tall.
fn strips(rows: u32) -> Map {
    let ids: Vec<u8> = (0..rows).flat_map(|_| 0..6).collect();
    map(&ids, 6, rows, 0.0, 1)
}

fn renderer(map: &Map) -> Renderer {
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
    r.set_terrain(map);
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

fn shot(map: &Map, c: &Camera) -> Rgba8 {
    let mut r = renderer(map);
    r.set_view_proj(c.view_proj());
    r.render_to_image().expect("render")
}

/// Screen pixel of a world point.
fn project(c: &Camera, p: Vec3) -> (u32, u32) {
    let clip = c.view_proj_f64() * DVec4::new(f64::from(p.x), f64::from(p.y), f64::from(p.z), 1.0);
    assert!(clip.w > 0.0);
    let (nx, ny) = (clip.x / clip.w, clip.y / clip.w);
    assert!(
        nx.abs() < 0.95 && ny.abs() < 0.95,
        "({nx}, {ny}) is off screen"
    );
    (
        ((nx * 0.5 + 0.5) * f64::from(W)).round() as u32,
        ((0.5 - ny * 0.5) * f64::from(H)).round() as u32,
    )
}

/// Mean colour of the `(2r+1)`-square patch around a pixel.
fn patch(img: &Rgba8, (x, y): (u32, u32), r: u32) -> [f32; 3] {
    let mut sum = [0.0_f32; 3];
    let mut n = 0.0;
    for yy in y.saturating_sub(r)..=(y + r).min(img.height - 1) {
        for xx in x.saturating_sub(r)..=(x + r).min(img.width - 1) {
            let p = img.pixel(xx, yy);
            for c in 0..3 {
                sum[c] += f32::from(p[c]);
            }
            n += 1.0;
        }
    }
    sum.map(|s| s / n)
}

fn lum(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// World point (on flat ground at height 0) of a gameplay cell's middle.
fn cell_middle(cx: u32, cz: u32) -> Vec3 {
    Vec3::new((cx as f32 + 0.5) * CELL, 0.0, (cz as f32 + 0.5) * CELL)
}

#[test]
fn every_type_is_drawn_in_its_own_look() {
    let m = strips(3);
    let c = camera(&m, 1.0, (12.0, 6.0));
    let img = shot(&m, &c);
    let at = |t: u8| patch(&img, project(&c, cell_middle(u32::from(t), 1)), 3);
    let (grass, road, water, rock, ore, bridge) = (
        at(GRASS),
        at(ROAD),
        at(WATER),
        at(ROCK),
        at(ORE),
        at(BRIDGE),
    );
    println!(
        "grass {grass:?} road {road:?} water {water:?} rock {rock:?} ore {ore:?} bridge {bridge:?}"
    );
    assert!(grass[1] > grass[0] && grass[1] > grass[2], "grass is green");
    assert!(water[2] > water[0] + 40.0, "water is blue");
    assert!(
        ore[0] > ore[2] + 25.0 && bridge[0] > bridge[2] + 25.0,
        "ore and wood are warm"
    );
    assert!(
        lum(road) > lum(rock) && road[0] > road[2] + 40.0,
        "the road is light tan packed earth, apart from the grey rock"
    );
    // Every pair reads apart.
    let all = [
        ("grass", grass),
        ("road", road),
        ("water", water),
        ("rock", rock),
        ("ore", ore),
        ("bridge", bridge),
    ];
    for (i, (a, ca)) in all.iter().enumerate() {
        for (b, cb) in &all[..i] {
            assert!(
                dist(*ca, *cb) > 25.0,
                "{a} and {b} look alike: {ca:?} {cb:?}"
            );
        }
    }
}

#[test]
fn the_picture_follows_the_terrain_types_not_the_material_map() {
    // The same terrain layer on a map whose material map says sand and one
    // whose says grass: identical pictures.
    let ids: Vec<u8> = (0..3).flat_map(|_| 0..6).collect();
    let (a, b) = (map(&ids, 6, 3, 0.0, 0), map(&ids, 6, 3, 0.0, 1));
    let c = camera(&a, 0.12, (12.0, 6.0));
    let (ia, ib) = (shot(&a, &c), shot(&b, &c));
    assert_eq!(ia.pixels, ib.pixels, "the material map changed the picture");
    // And a different terrain layer does change it.
    let other: Vec<u8> = (0..3).flat_map(|_| [3, 3, 3, 3, 3, 3]).collect();
    let ic = shot(&map(&other, 6, 3, 0.0, 0), &c);
    assert_ne!(ia.pixels, ic.pixels);
}

#[test]
fn a_border_is_a_soft_ramp_not_a_step() {
    // Grass | road at x = 4 m (column 0 | 1), ground level, seen from close.
    let m = strips(3);
    let c = camera(&m, 0.05, (4.0, 6.0));
    let img = shot(&m, &c);
    let z = 6.0;
    let at = |x: f32| patch(&img, project(&c, Vec3::new(x, 0.0, z)), 1);
    let (grass, road) = (at(1.2), at(6.8));
    let span = dist(grass, road);
    assert!(span > 40.0, "grass and road should differ: {span}");
    // Fraction of the way from the grass to the road along the way across.
    let t = |x: f32| {
        let p = at(x);
        let axis = [road[0] - grass[0], road[1] - grass[1], road[2] - grass[2]];
        let d = [p[0] - grass[0], p[1] - grass[1], p[2] - grass[2]];
        (d[0] * axis[0] + d[1] * axis[1] + d[2] * axis[2]) / (span * span)
    };
    let profile: Vec<f32> = (0..=16).map(|i| t(2.0 + i as f32 * 0.25)).collect();
    println!("profile {profile:.2?}");
    assert!(
        profile[0] < 0.25,
        "pure grass before the border: {}",
        profile[0]
    );
    assert!(
        profile[16] > 0.75,
        "pure road after the border: {}",
        profile[16]
    );
    // Soft: several samples in between (a step would have none), the middle is
    // between the two, and it never swings back by much.
    let between = profile.iter().filter(|&&p| p > 0.2 && p < 0.8).count();
    assert!(between >= 3, "hard border: {between} samples in between");
    assert!((0.25..0.75).contains(&profile[8]), "middle {}", profile[8]);
    for w in profile.windows(2) {
        assert!(w[1] > w[0] - 0.35, "the ramp swings back: {profile:.2?}");
    }
}

#[test]
fn a_staircase_of_cells_does_not_show_a_chessboard() {
    // A diagonal road of cells on grass. What this proves: the border is not
    // hard, the point where two road cells and two grass cells meet diagonally
    // is a blend of both colours, not one of them. It does not prove the
    // wobble (a straight soft ramp passes as well); the wobble is only seen in
    // the pictures of docs/screenshots/wp2.2.
    let ids = [
        1, 0, 0, 0, //
        0, 1, 0, 0, //
        0, 0, 1, 0, //
        0, 0, 0, 1,
    ];
    let m = map(&ids, 4, 4, 0.0, 0);
    let c = camera(&m, 0.07, (8.0, 8.0));
    let img = shot(&m, &c);
    let road = patch(&img, project(&c, cell_middle(1, 1)), 2);
    let grass = patch(&img, project(&c, cell_middle(3, 0)), 2);
    // The point shared by cells (0,0), (1,0), (0,1), (1,1): road, grass, grass,
    // road. A bilinear blend puts it halfway.
    let corner = patch(&img, project(&c, Vec3::new(4.0, 0.0, 4.0)), 1);
    let axis = dist(road, grass);
    assert!(axis > 40.0);
    let to_road = dist(corner, road);
    let to_grass = dist(corner, grass);
    println!("corner: {to_road:.0} from road, {to_grass:.0} from grass, axis {axis:.0}");
    assert!(
        to_road > axis * 0.2 && to_grass > axis * 0.2,
        "the corner is one of the two colours: {to_road:.0}/{to_grass:.0}"
    );
}

#[test]
fn water_and_a_bridge_are_level_where_the_ground_slopes() {
    // The ground drops 0.8 m per metre towards +x, which turns it away from
    // the sun: grass and rock get much darker than on flat ground, water and
    // the bridge deck keep their light.
    let all = |t: u8| vec![t; 16];
    let measure = |t: u8, slope: f32| {
        let m = map(&all(t), 4, 4, slope, 1);
        let c = camera(&m, 0.2, (8.0, 8.0));
        let img = shot(&m, &c);
        lum(patch(&img, (W / 2, H / 2), 24))
    };
    for (name, t, level) in [
        ("grass", GRASS, false),
        ("rock", ROCK, false),
        ("water", WATER, true),
        ("bridge", BRIDGE, true),
    ] {
        let (flat, sloped) = (measure(t, 0.0), measure(t, 0.8));
        let ratio = sloped / flat;
        println!("{name}: flat {flat:.1}, sloped {sloped:.1}, ratio {ratio:.2}");
        if level {
            assert!(
                (0.96..1.04).contains(&ratio),
                "{name} follows the slope: {ratio:.2}"
            );
        } else {
            assert!(ratio < 0.9, "{name} ignores the slope: {ratio:.2}");
        }
    }
}

#[test]
fn a_shore_has_a_light_line_and_open_water_does_not() {
    // Water in the middle column of three, grass either side.
    let ids: Vec<u8> = (0..3).flat_map(|_| [GRASS, WATER, GRASS]).collect();
    let m = map(&ids, 3, 3, 0.0, 1);
    let c = camera(&m, 0.05, (6.0, 6.0));
    let img = shot(&m, &c);
    let at = |x: f32| lum(patch(&img, project(&c, Vec3::new(x, 0.0, 6.0)), 1));
    let (grass, open, shore) = (at(1.5), at(6.0), at(4.0));
    println!("grass {grass:.1}, open water {open:.1}, shore {shore:.1}");
    // The border of two ground colours would lie between them; the light line
    // makes it brighter than both.
    assert!(
        shore > grass.max(open) + 8.0,
        "no light line on the shore: {shore:.1} (grass {grass:.1}, water {open:.1})"
    );
}
