//! The ore on screen (WP7.1): a field of the 256 by 256 cell playfield drawn full, part
//! emptied, and after a long regrowth, through the same path the game uses (the world made by
//! `playfield::make_world`, the levels from `OreView`, the renderer's `update_resource_levels`
//! and the minimap's `paint_cells`).
//!
//! The checks compare the layers the index holds and the colours of the rendered picture, so no
//! golden images are needed. With `SF_SHOT_DIR` set the pictures are written there (that is how
//! `docs/screenshots/wp7.1` was made). Same adapter rules as the sf_render screenshot tests: a
//! software adapter is preferred, no adapter is a failure.

use std::path::Path;

use sf_data::map::Map;
use sf_game::ore_view::OreView;
use sf_game::playfield::make_world;
use sf_game::sim_snapshot::SimSnapshot;
use sf_render::camera::DVec2;
use sf_render::glam::{DVec4, Vec3};
use sf_render::image::{Rgba8, write_png};
use sf_render::terrain_look::TerrainLooks;
use sf_render::{Camera, CameraConfig, GpuOptions, Renderer};
use sf_sim::{Cell, Command, PlayerId, World, WorldConfig};
use sf_ui::Minimap;

const W: u32 = 640;
const H: u32 = 360;

/// The ore cells of the first field of the playfield (around (118, 82)), by row.
fn field(world: &World) -> Vec<Cell> {
    world
        .ore()
        .unwrap()
        .cells()
        .map(|(c, _)| c)
        .filter(|c| (110..125).contains(&c.x) && (76..86).contains(&c.y))
        .collect()
}

fn map() -> Map {
    Map::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps/playfield256"))
        .expect("the playfield map")
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

fn camera(map: &Map) -> Camera {
    let cfg = CameraConfig {
        map_size: DVec2::new(f64::from(map.width()), f64::from(map.height())),
        ..CameraConfig::default()
    };
    let mut c = Camera::new(cfg, W, H);
    c.set_zoom(0.24);
    // The middle of the field, in metres.
    c.set_focus(DVec2::new(118.5 * 4.0, 83.0 * 4.0));
    c
}

/// Screen pixel of the middle of a cell at ground height `y`.
fn project(c: &Camera, cell: Cell, y: f32) -> (u32, u32) {
    let p = Vec3::new(cell.x as f32 * 4.0 + 2.0, y, cell.y as f32 * 4.0 + 2.0);
    let clip = c.view_proj_f64() * DVec4::new(f64::from(p.x), f64::from(p.y), f64::from(p.z), 1.0);
    let (nx, ny) = (clip.x / clip.w, clip.y / clip.w);
    (
        ((nx * 0.5 + 0.5) * f64::from(W)).round() as u32,
        ((0.5 - ny * 0.5) * f64::from(H)).round() as u32,
    )
}

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

struct Scene {
    world: World,
    view: OreView,
    renderer: Renderer,
    minimap: Minimap,
    map: Map,
    camera: Camera,
}

impl Scene {
    fn new() -> Scene {
        let map = map();
        let cells = map.size_in_cells();
        let world = make_world(WorldConfig::new(1).with_map_size(cells.0, cells.1), &map).unwrap();
        let mut scene = Scene {
            renderer: renderer(&map),
            view: OreView::new(&TerrainLooks::embedded()),
            minimap: Minimap::bake(&map, 256),
            camera: camera(&map),
            world,
            map,
        };
        scene.show();
        scene
    }

    /// What the runner does for a new snapshot.
    fn show(&mut self) {
        let snap = SimSnapshot::capture(&self.world);
        let changes = self.view.changes(&snap);
        self.renderer.update_resource_levels(&changes);
        let colors: Vec<_> = changes
            .iter()
            .filter_map(|&(x, y, l)| Some((x, y, self.view.minimap_color(l)?)))
            .collect();
        self.minimap.paint_cells(&self.map, &colors);
    }

    fn step(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.world.step();
        }
        self.show();
    }

    fn take(&mut self, cell: Cell, amount: u16) {
        self.world
            .schedule(
                self.world.tick(),
                PlayerId(0),
                Command::TakeOre { cell, amount },
            )
            .unwrap();
    }

    fn picture(&mut self) -> Rgba8 {
        self.renderer.set_view_proj(self.camera.view_proj());
        self.renderer.render_to_image().expect("render")
    }

    fn layer_of(&self, cell: Cell) -> u8 {
        self.renderer.terrain_cell_texel(cell.x, cell.y).unwrap()[0]
    }
}

fn save(name: &str, img: &Rgba8, minimap: &Minimap) {
    if let Some(dir) = std::env::var_os("SF_SHOT_DIR") {
        let dir = Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        write_png(&dir.join(format!("{name}.png")), img).unwrap();
        let m = minimap.image();
        write_png(
            &dir.join(format!("{name}-minimap.png")),
            &Rgba8::new(m.width, m.height, m.rgba.clone()),
        )
        .unwrap();
    }
}

#[test]
fn a_field_is_drawn_full_part_emptied_and_regrown() {
    let mut scene = Scene::new();
    let field = field(&scene.world);
    assert_eq!(field.len(), 28);
    let looks = TerrainLooks::embedded();
    let first = {
        // The index holds the layer of the first level after the types' own.
        let top = scene.layer_of(field[0]);
        top - 3
    };
    let [empty, low, mid, full] = [first, first + 1, first + 2, first + 3];

    // 1. Tick 0: every cell holds 120 of 160 (75 percent): the top level.
    assert!(field.iter().all(|&c| scene.layer_of(c) == full));
    let ground = 0.0_f32; // the playfield is flat
    let full_pic = scene.picture();
    save("tick-00000-full", &full_pic, &scene.minimap);

    // 2. The west half of the field is emptied, the east half loses 70 (50 left, 31 percent
    //    of 160: the low level; a few cells lose 40 instead and keep 80, 50 percent: mid).
    let (west, east): (Vec<Cell>, Vec<Cell>) = field.iter().partition(|c| c.x <= 117);
    for (i, &c) in west.iter().enumerate() {
        scene.take(c, 500);
        let _ = i;
    }
    for (i, &c) in east.iter().enumerate() {
        scene.take(c, if i % 3 == 0 { 40 } else { 70 });
    }
    scene.step(1);
    for &c in &west {
        // A cell whose turn came in this very tick holds 1 after the take: not empty, but the
        // lowest level above empty. Both are dark ground, one fleck share apart.
        assert!(scene.layer_of(c) <= low, "cell {c}");
    }
    let tick = scene.world.tick().0;
    let half_pic = scene.picture();
    save(
        &format!("tick-{tick:05}-half-depleted"),
        &half_pic,
        &scene.minimap,
    );
    let probe_west = *west.iter().find(|c| scene.world.ore_at(**c) == 0).unwrap();
    let probe_east = *east
        .iter()
        .find(|c| scene.layer_of(**c) == low)
        .expect("an east cell at the low level");
    assert_eq!(scene.layer_of(probe_west), empty);
    let _ = mid;
    let l_empty = lum(patch(
        &half_pic,
        project(&scene.camera, probe_west, ground),
        2,
    ));
    let l_full_before = lum(patch(
        &full_pic,
        project(&scene.camera, probe_west, ground),
        2,
    ));
    assert!(
        l_empty < l_full_before - 6.0,
        "an emptied cell is darker than it was: {l_empty} against {l_full_before}"
    );
    // The minimap shows it too: the pixel of an emptied cell is the level 0 colour (under the
    // relief shade, at most 18 percent).
    let px = scene.minimap.image();
    let at_px = (probe_west.y * px.width + probe_west.x) as usize * 4;
    let seen = [px.rgba[at_px], px.rgba[at_px + 1], px.rgba[at_px + 2]];
    let want = scene.view.minimap_color(0).unwrap();
    for c in 0..3 {
        let w = f32::from(want[c]);
        assert!(
            (f32::from(seen[c]) - w).abs() <= w * 0.19 + 1.0,
            "minimap {seen:?} vs {want:?}"
        );
    }
    let _ = probe_east;

    // 3. Sixteen thousand ticks later (13 minutes of game time) the emptied cells have grown
    //    back: 80 units each, 50 percent of the maximum: the mid level, lighter than empty.
    scene.step(16_000);
    let tick = scene.world.tick().0;
    let grown_pic = scene.picture();
    save(
        &format!("tick-{tick:05}-regrown"),
        &grown_pic,
        &scene.minimap,
    );
    let amount = scene.world.ore_at(probe_west);
    assert!((79..=81).contains(&amount), "regrown to {amount}");
    assert_eq!(scene.layer_of(probe_west), mid);
    let l_grown = lum(patch(
        &grown_pic,
        project(&scene.camera, probe_west, ground),
        2,
    ));
    assert!(
        l_grown > l_empty + 4.0,
        "a regrown cell is lighter than an emptied one: {l_grown} against {l_empty}"
    );
    let _ = (looks, full);
}
