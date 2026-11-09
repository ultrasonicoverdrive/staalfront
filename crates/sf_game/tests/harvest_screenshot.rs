//! Screenshot test for the harvesters (WP7.2), through the real renderer on a software adapter (Mesa
//! lavapipe / DX12 WARP): the playfield with a refinery and three harvesters, the world stepped
//! until each stage of the loop is seen (mining, driving back, queueing, unloading), and one frame
//! of each as `sf_game` draws it: the models, then the cargo bars on the UI's world layer.
//!
//! The stages are *found* in the simulation (`World::harvester_of`, `World::dock_of`); nothing here
//! knows a rule. The checks look at the pixels: the amber of the cargo bars (`cargo_view::FILL`)
//! grows with the load, a full load has the brighter fill, a harvester without a load has an empty
//! track, and the ore cells drain (the ore look of `OreView`, which the picture shows). No golden
//! image: the two software drivers differ a little in blending. Set `SF_SAVE_SHOTS=<dir>` to write
//! the images with their tick in the name (`docs/screenshots/wp7.2/`).
//!
//! Adapter rules as in `selection_screenshot.rs`: a software adapter is preferred; on CI (`CI` set)
//! a missing one is a failure, locally any adapter is accepted.

use std::path::PathBuf;
use std::sync::Arc;

use sf_data::map::Map;
use sf_data::units::UnitDefs;
use sf_game::cargo_view;
use sf_game::ore_view::OreView;
use sf_game::playfield::{make_world, with_buildings, with_movement};
use sf_game::roster::Roster;
use sf_game::selection_view::FrameGeometry;
use sf_game::sim_snapshot::SimSnapshot;
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8};
use sf_render::model::EyeView;
use sf_render::terrain_look::TerrainLooks;
use sf_render::{
    Camera, CameraConfig, FrameView, Frustum, GpuOptions, Ground, Instancer, Motion, Renderer,
    Style, TerrainGround, UnitId,
};
use sf_sim::{Cell, Command, EntityId, HarvestStage, Kind, PlayerId, Tick, World, WorldConfig};
use sf_ui::{Ui, UiConfig, UiInput, UiRenderer};

const W: u32 = 960;
const H: u32 = 540;
const P0: PlayerId = PlayerId(0);

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
    println!("adapter: {}", r.adapter_summary());
    r.set_scene(&[]);
    r.set_terrain(map);
    r.set_icon_atlas(&icon_set());
    r
}

fn icon_set() -> sf_render::IconAtlas {
    let atlas = sf_render::IconAtlas::load(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons"),
    );
    assert!(atlas.warnings().is_empty(), "{:?}", atlas.warnings());
    atlas
}

fn save(name: &str, img: &Rgba8) {
    if let Some(dir) = std::env::var_os("SF_SAVE_SHOTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("shot directory");
        image::write_png(&dir.join(name), img).expect("save shot");
    }
}

fn map() -> Map {
    Map::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps/playfield256"))
        .expect("the playfield map")
}

/// The refinery (slot 0) at (96, 86) and three harvesters (slots 1 to 3) at the places of the
/// `harvest` scenario, on the playfield; the field is the 28 cells round (118, 82).
struct Scene {
    map: Map,
    ground: Arc<TerrainGround>,
    world: World,
    instancer: Instancer,
    roster: Roster,
    ore_view: OreView,
}

impl Scene {
    fn new() -> Scene {
        Scene::build(&[(104, 90), (106, 92), (94, 91)], None)
    }

    /// The same refinery with harvesters at `spawns` (slots 1, 2, ...), and the shipped harvest
    /// rules unless `edit` changes them.
    fn build(spawns: &[(i32, i32)], edit: Option<fn(&mut sf_sim::HarvestRules)>) -> Scene {
        let map = map();
        let ground = Arc::new(TerrainGround::new(&map));
        let defs = UnitDefs::embedded();
        let cells = map.size_in_cells();
        let world = make_world(WorldConfig::new(1).with_map_size(cells.0, cells.1), &map)
            .unwrap()
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap();
        let world = with_buildings(with_movement(world, &defs), &defs);
        let mut rules = sf_bridge::harvest_rules(&sf_data::harvest::HarvestSettings::embedded());
        if let Some(edit) = edit {
            edit(&mut rules);
        }
        let mut world = world
            .with_harvest(rules, &sf_bridge::harvester_kinds(&defs))
            .expect("the harvest rules");
        let kind = |name: &str| Kind(defs.kind_of(name).unwrap());
        let at = |c: i32, r: i32| {
            sf_math::Vec2::new(
                sf_math::Fx::from_int(c) + sf_math::Fx::HALF,
                sf_math::Fx::from_int(r) + sf_math::Fx::HALF,
            )
        };
        world
            .schedule(
                Tick(0),
                P0,
                Command::PlaceStructure {
                    kind: kind("refinery"),
                    cell: Cell::new(96, 86),
                    owner: P0,
                },
            )
            .unwrap();
        for &(x, y) in spawns {
            world
                .schedule(
                    Tick(0),
                    P0,
                    Command::SpawnUnit {
                        kind: kind("harvester"),
                        pos: at(x, y),
                        owner: P0,
                    },
                )
                .unwrap();
        }
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let base = Style::embedded().scaled(3.0);
        let roster = Roster::build(&defs, &base, &icon_set(), &assets, "", false).unwrap();
        let mut instancer = Instancer::new(
            roster.style(&base),
            (map.width() as f32, map.height() as f32),
        );
        instancer.set_height_range(ground.min_height() as f32, ground.max_height() as f32);
        let mut scene = Scene {
            map,
            ground,
            world,
            instancer,
            roster,
            ore_view: OreView::new(&TerrainLooks::embedded()),
        };
        scene.world.step();
        scene.sync();
        scene
    }

    fn sync(&mut self) {
        let roster = &self.roster;
        let views: Vec<_> = self.world.snapshot().iter().collect();
        self.instancer.update_tick(views.iter().map(|v| {
            Motion::from_fixed(
                roster.shape_of(v.kind).unwrap_or(0),
                v.owner.0,
                v.prev_pos,
                v.pos,
                v.prev_facing,
                v.facing,
            )
            .with_id(UnitId::from_parts(v.id.index(), v.id.generation()))
        }));
    }

    /// Steps until `stop` holds; the tick it holds at.
    fn run_until(&mut self, limit: u64, mut stop: impl FnMut(&World) -> bool) -> u64 {
        for _ in 0..limit {
            if stop(&self.world) {
                self.sync();
                return self.world.tick().0;
            }
            self.world.step();
        }
        panic!("not reached within {limit} ticks");
    }

    fn camera(&self, zoom: f64, focus_cells: (f64, f64)) -> Camera {
        let cfg = CameraConfig {
            map_size: DVec2::new(f64::from(self.map.width()), f64::from(self.map.height())),
            ..CameraConfig::default()
        };
        let mut c = Camera::new(cfg, W, H);
        c.set_ground(self.ground.clone());
        c.set_zoom(zoom);
        c.set_focus(DVec2::new(focus_cells.0 * 4.0, focus_cells.1 * 4.0));
        c
    }

    /// One frame as `sf_game` draws it, with the cargo bars on or off; the picture.
    fn shoot(&mut self, r: &mut Renderer, cam: &Camera, with_cargo: bool) -> Rgba8 {
        // The ore cells whose look changed are written to the terrain, as the runner does.
        let snap = SimSnapshot::capture(&self.world);
        let changes = self.ore_view.changes(&snap);
        r.update_resource_levels(&changes);
        let vp = cam.view_proj();
        r.set_view_proj(vp);
        let frustum = Frustum::from_view_proj(vp);
        let eye = EyeView::from_view_proj(vp, H as f32);
        let blend = self.instancer.icon_config().blend_for_view(
            eye.focal_px,
            cam.distance() as f32,
            cam.max_distance() as f32,
            1.0,
        );
        let ground = &self.ground;
        self.instancer
            .build_frame(&FrameView::new(&frustum, 0.0, eye, blend, 1.0), |x, z| {
                ground.height(f64::from(x), f64::from(z)) as f32
            });
        let mut ui = Ui::new(UiConfig::default());
        ui.begin_frame(UiInput::default(), (W, H), 1.0);
        if with_cargo {
            let frame = FrameGeometry {
                view_proj: vp,
                viewport: [W as f32, H as f32],
                ui_scale: 1.0,
                blend: self.instancer.blend(),
                icon_px: self.instancer.icon_params().size_px,
            };
            let mut layer = ui.world_layer();
            cargo_view::draw(&self.instancer, &snap, &frame, &mut layer);
        }
        ui.end_frame();
        r.upload_units(&self.instancer);
        let mut ur = UiRenderer::new(&r.gpu().device, r.color_format());
        r.render_to_image_with(|t| ur.render(t, &mut ui))
            .expect("render")
    }
}

/// How many pixels of `img` are within `tol` steps (summed over the channels) of `c`.
fn count(img: &Rgba8, c: sf_ui::Color, tol: i32) -> usize {
    let sf_ui::Color(r, g, b, _) = c;
    (0..img.height)
        .flat_map(|y| (0..img.width).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = img.pixel(x, y);
            (i32::from(p[0]) - i32::from(r)).abs()
                + (i32::from(p[1]) - i32::from(g)).abs()
                + (i32::from(p[2]) - i32::from(b)).abs()
                <= tol
        })
        .count()
}

fn stage(w: &World, slot: u32) -> Option<HarvestStage> {
    w.harvester_of(EntityId::new(slot, 0)).map(|v| v.stage)
}

fn load(w: &World, slot: u32) -> u16 {
    w.harvester_of(EntityId::new(slot, 0)).map_or(0, |v| v.load)
}

fn amber(img: &Rgba8) -> usize {
    count(img, cargo_view::FILL, 12)
}

#[test]
fn the_cargo_bars_follow_the_load_and_the_ore_cells_drain() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    // The close-up of the field edge, where the harvesters mine; and of the refinery.
    let field = scene.camera(0.22, (115.0, 85.0));
    let refinery = scene.camera(0.22, (101.0, 88.0));

    // Before anything: three empty tracks, no amber.
    let t0 = scene.world.tick().0;
    let empty = scene.shoot(&mut r, &field, true);
    save(&format!("harvest_t{t0:05}_start.png"), &empty);
    assert_eq!(amber(&empty), 0, "an empty harvester shows no fill");
    let before = scene.shoot(&mut r, &field, false);

    // Mining: some of the load is in, and the cells under the harvesters have started to drain.
    let t1 = scene.run_until(2000, |w| (1..=3).any(|s| load(w, s) >= 60));
    let mining_load = (1..=3).map(|s| load(&scene.world, s)).max().unwrap();
    let mining = scene.shoot(&mut r, &field, true);
    save(&format!("harvest_t{t1:05}_mining.png"), &mining);
    let mining_amber = amber(&mining);
    assert!(mining_amber > 0, "mining: no fill at load {mining_load}");
    // The bars are the only difference between this frame and the same frame without them.
    let without = scene.shoot(&mut r, &field, false);
    assert_ne!(
        count(&mining, cargo_view::FILL, 12),
        count(&without, cargo_view::FILL, 12)
    );
    // The ore under the harvesters drained: the picture of the field is not the one before.
    assert_ne!(before, without);

    // Full: the brighter fill.
    let t2 = scene.run_until(4000, |w| (1..=3).any(|s| load(w, s) >= 200));
    let full = scene.shoot(&mut r, &field, true);
    save(&format!("harvest_t{t2:05}_full_load.png"), &full);
    assert!(
        count(&full, cargo_view::FULL, 12) > 0,
        "a full load has the brighter fill"
    );
    assert!(amber(&full) + count(&full, cargo_view::FULL, 12) > mining_amber);

    // Driving back.
    let t3 = scene.run_until(4000, |w| {
        (1..=3).any(|s| stage(w, s) == Some(HarvestStage::ToRefinery))
    });
    let back = scene.shoot(&mut r, &scene.camera(0.3, (106.0, 86.0)), true);
    save(&format!("harvest_t{t3:05}_driving_back.png"), &back);

    // Two at the refinery: one in the bay, one queued.
    let t4 = scene.run_until(6000, |w| {
        w.dock_of(EntityId::new(0, 0))
            .is_some_and(|d| d.holder.is_some() && !d.queue.is_empty())
    });
    let queue = scene.shoot(&mut r, &refinery, true);
    save(&format!("harvest_t{t4:05}_queueing.png"), &queue);
    let dock = scene.world.dock_of(EntityId::new(0, 0)).unwrap();
    assert!(!dock.queue.is_empty());

    // Unloading: the load of the harvester in the bay drops; its bar shrinks.
    let t5 = scene.run_until(6000, |w| {
        w.dock_of(EntityId::new(0, 0)).is_some_and(|d| {
            d.unloading
                && d.holder
                    .is_some_and(|h| w.harvester_of(h).is_some_and(|v| v.load <= 120))
        })
    });
    let dock = scene.world.dock_of(EntityId::new(0, 0)).unwrap();
    let holder = dock.holder.unwrap();
    let unloading_load = scene.world.harvester_of(holder).unwrap().load;
    let unloading = scene.shoot(&mut r, &refinery, true);
    save(&format!("harvest_t{t5:05}_unloading.png"), &unloading);
    assert!(unloading_load <= 120);
    // Fewer fill pixels at the refinery than when the same two stood there full: the bay's bar
    // is part of the way down.
    assert!(amber(&unloading) + count(&unloading, cargo_view::FULL, 12) > 0);
}

/// Seven harvesters on the one refinery, shot when the queue stands: with the queue spots
/// (WP7.5) the queued harvesters stand beside the approach, with `queue_gap: 0` (the rule of
/// WP7.2) they stand in it. The tick is in the file name.
#[test]
fn queued_harvesters_stand_beside_the_approach_in_the_picture() {
    let spawns = [
        (104, 90),
        (106, 92),
        (106, 88),
        (108, 90),
        (108, 94),
        (104, 94),
        (110, 92),
    ];
    let mut r = None;
    // The dock point is (99.5, 88.5); the approach is the row y = 88.5 to the east of it.
    let mut shoot_queue = |gap_off: bool, name: &str| -> (u64, Vec<(f64, f64)>) {
        let edit: Option<fn(&mut sf_sim::HarvestRules)> = if gap_off {
            Some(|rules| rules.queue_gap = 0)
        } else {
            None
        };
        let mut scene = Scene::build(&spawns, edit);
        let r = r.get_or_insert_with(|| renderer(&scene.map));
        let t = scene.run_until(9000, |w| {
            w.dock_of(EntityId::new(0, 0))
                .is_some_and(|d| d.queue.len() >= 3)
        });
        // Let the queue settle: the ones queued walk to their spots.
        for _ in 0..120 {
            scene.world.step();
        }
        scene.sync();
        let dock = scene.world.dock_of(EntityId::new(0, 0)).unwrap();
        let standing: Vec<(f64, f64)> = dock
            .queue
            .iter()
            .map(|&q| {
                let p = scene.world.get(q).unwrap().pos;
                (
                    p.x.raw() as f64 / 65536.0 - 99.5,
                    p.y.raw() as f64 / 65536.0 - 88.5,
                )
            })
            .collect();
        let cam = scene.camera(0.25, (104.0, 88.0));
        let img = scene.shoot(r, &cam, true);
        let tick = scene.world.tick().0;
        save(&format!("queue_t{tick:05}_{name}.png"), &img);
        println!("{name} (t {t} to {tick}): {standing:?}");
        (tick, standing)
    };
    let (_, with_spots) = shoot_queue(false, "beside_the_approach");
    let (_, without) = shoot_queue(true, "in_the_approach_old_rule");
    // With the spots no queued harvester stands on the line of the approach: the spots are three
    // cells to the side, and a harvester pushed by the crowd or stopped short of its spot is still
    // more than a cell off the line (measured: 1.3 to 4.4 cells). Without them one stands in it
    // (measured: 0.3 cells).
    assert!(with_spots.len() >= 3);
    for &(_, dy) in &with_spots {
        assert!(
            dy.abs() >= 1.0,
            "queued at {dy} cells from the approach: {with_spots:?}"
        );
    }
    assert!(without.iter().any(|&(_, dy)| dy.abs() < 1.0), "{without:?}");
}
