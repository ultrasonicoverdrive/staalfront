//! Screenshot test for the ore silo (WP7.3b), through the real renderer on a software adapter
//! (Mesa lavapipe / DX12 WARP): the playfield with a refinery and three harvesters, as in
//! `storage_screenshot.rs`, with storage of 400 for the refinery and 200 for the silo (so that the
//! store fills in a test's time; the shipped numbers are 2000 and 1500). One frame per state, as
//! `sf_game` draws it: the models, then the storage bars (`storage_view`) on the UI's world layer.
//!
//! The states are *found* in the simulation (`World::ore_stored`, `World::storage_capacity`). The
//! checks look at the pixels: the refinery's store is full in its own colour, a silo beside it
//! takes the fill back to part (400 of 600) and draws a bar of its own, and when the silo is full
//! too both bars are in the full colour. No golden image. Set `SF_SAVE_SHOTS=<dir>` to write the
//! images with their tick in the name (`docs/screenshots/wp7.3b/`).

use std::path::PathBuf;
use std::sync::Arc;

use sf_data::map::Map;
use sf_data::units::UnitDefs;
use sf_game::ore_view::OreView;
use sf_game::placement_view::GhostFrame;
use sf_game::playfield::{make_world, with_buildings, with_harvest, with_movement};
use sf_game::roster::Roster;
use sf_game::sim_snapshot::SimSnapshot;
use sf_game::storage_view;
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8};
use sf_render::model::EyeView;
use sf_render::terrain_look::TerrainLooks;
use sf_render::{
    Camera, CameraConfig, FrameView, Frustum, GpuOptions, Ground, Instancer, Motion, Renderer,
    Style, TerrainGround, UnitId,
};
use sf_sim::{Cell, Command, Kind, PlayerId, Tick, World, WorldConfig};
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
        let map = map();
        let ground = Arc::new(TerrainGround::new(&map));
        let defs = UnitDefs::embedded();
        let cells = map.size_in_cells();
        let world = make_world(WorldConfig::new(1).with_map_size(cells.0, cells.1), &map)
            .unwrap()
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap();
        let world = with_harvest(with_buildings(with_movement(world, &defs), &defs), &defs);
        let refinery = Kind(defs.kind_of("refinery").unwrap());
        let silo = Kind(defs.kind_of("ore_silo").unwrap());
        let mut world = world.with_storage(&[(refinery, 400), (silo, 200)]).unwrap();
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
        for (x, y) in [(104, 90), (106, 92), (94, 91)] {
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

    /// One frame as `sf_game` draws it, with the storage bars; the picture.
    fn shoot(&mut self, r: &mut Renderer, cam: &Camera) -> Rgba8 {
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
        {
            let frame = GhostFrame {
                view_proj: vp,
                viewport: [W as f32, H as f32],
                ui_scale: 1.0,
            };
            let mut layer = ui.world_layer();
            let height = |x: f32, z: f32| ground.height(f64::from(x), f64::from(z)) as f32;
            storage_view::draw_bars(
                &mut layer,
                &frame,
                &height,
                &snap,
                storage_view::shipped_look(),
            );
        }
        ui.end_frame();
        r.upload_units(&self.instancer);
        let mut ur = UiRenderer::new(&r.gpu().device, r.color_format());
        r.render_to_image_with(|t| ur.render(t, &mut ui))
            .expect("render")
    }
}

/// How many pixels of `img` are within `tol` steps (summed over the channels) of `c`.
fn count(img: &Rgba8, c: (u8, u8, u8, u8), tol: i32) -> usize {
    let (r, g, b, _) = c;
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

#[test]
fn a_silo_beside_the_refinery_takes_the_fill_back_to_part_and_has_a_bar_of_its_own() {
    let look = storage_view::shipped_look();
    let (fill, full) = (look.bar.fill, look.full);
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let cam = scene.camera(0.4, (98.0, 90.0));

    // The refinery alone, 400 of 400: its bar in the full colour.
    let t0 = scene.run_until(6000, |w| w.ore_stored(P0) >= 400);
    let one = scene.shoot(&mut r, &cam);
    save(&format!("silo_t{t0:05}_refinery_full.png"), &one);
    let one_full = count(&one, full, 20);
    assert!(one_full > 0, "a full store shows the full colour");

    // A silo beside it: 400 of 600. Both bars are part filled, none full.
    let now = scene.world.tick().0;
    scene
        .world
        .schedule(
            Tick(now),
            P0,
            Command::PlaceStructure {
                kind: Kind(UnitDefs::embedded().kind_of("ore_silo").unwrap()),
                cell: Cell::new(96, 82),
                owner: P0,
            },
        )
        .unwrap();
    let t1 = scene.run_until(10, |w| w.storage_capacity(P0) == 600);
    let two = scene.shoot(&mut r, &cam);
    save(&format!("silo_t{t1:05}_silo_built.png"), &two);
    assert_eq!(count(&two, full, 20), 0);
    assert!(count(&two, fill, 20) > 0, "two bars of two thirds");

    // The harvester in the bay unloads into the room the silo made; at 600 both bars are full.
    let t2 = scene.run_until(6000, |w| w.ore_stored(P0) >= 600);
    let three = scene.shoot(&mut r, &cam);
    save(&format!("silo_t{t2:05}_both_full.png"), &three);
    assert!(
        count(&three, full, 20) > one_full,
        "the silo's bar is full too: {} against {one_full}",
        count(&three, full, 20)
    );
}
