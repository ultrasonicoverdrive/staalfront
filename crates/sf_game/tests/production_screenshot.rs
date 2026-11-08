//! Screenshot test for production (WP6.5), through the real renderer on a software adapter (Mesa
//! lavapipe / DX12 WARP): the playfield with a construction centre, a barracks and a vehicle
//! factory that build from queues, as `sf_game` draws them: the models, the progress bars
//! (`production_view::draw_bars`), the rally flags (`draw_rally_flags`) and the queue readout of
//! the selected factory (`panel_lines` + `draw_panel`, the functions the HUD calls).
//!
//! The states are *found* in the simulation (`World::production_of`). The checks look at the
//! pixels: the colours of `production_look.ron` appear exactly when the factory builds or waits.
//! No golden image. Set `SF_SAVE_SHOTS=<dir>` to write the images with their tick in the name
//! (`docs/screenshots/wp6.5/`).

use std::path::PathBuf;
use std::sync::Arc;

use sf_data::map::Map;
use sf_data::units::UnitDefs;
use sf_game::placement_view::GhostFrame;
use sf_game::playfield::{
    make_world, with_buildings, with_harvest, with_money, with_movement, with_production,
    with_storage,
};
use sf_game::production_view;
use sf_game::roster::Roster;
use sf_game::sim_snapshot::SimSnapshot;
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8};
use sf_render::model::EyeView;
use sf_render::{
    Camera, CameraConfig, FrameView, Frustum, GpuOptions, Ground, Instancer, Motion, Renderer,
    Style, TerrainGround, UnitId,
};
use sf_sim::{Cell, Command, EntityId, FactoryStatus, Kind, PlayerId, Tick, World, WorldConfig};
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

/// A construction centre (84, 74), a barracks (84, 80) and a vehicle factory (90, 74) of player 0
/// on the playfield, with the shipped production and money rules.
struct Scene {
    map: Map,
    ground: Arc<TerrainGround>,
    world: World,
    instancer: Instancer,
    roster: Roster,
    kinds: Vec<(String, Kind)>,
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
        let world = with_buildings(with_movement(world, &defs), &defs);
        let world = with_storage(with_harvest(world, &defs), &defs);
        let mut world = with_production(with_money(world), &defs);
        let kind = |name: &str| Kind(defs.kind_of(name).unwrap());
        for (name, cell) in [
            ("construction_center", (84, 74)),
            ("barracks", (84, 80)),
            ("vehicle_factory", (90, 74)),
            ("refinery", (96, 86)),
        ] {
            world
                .schedule(
                    Tick(0),
                    P0,
                    Command::PlaceStructure {
                        kind: kind(name),
                        cell: Cell::new(cell.0, cell.1),
                        owner: P0,
                    },
                )
                .unwrap();
        }
        let kinds = [
            "rifleman",
            "light_tank",
            "harvester",
            "barracks",
            "vehicle_factory",
        ]
        .iter()
        .map(|n| ((*n).to_owned(), kind(n)))
        .collect();
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
            kinds,
        };
        scene.world.step();
        scene.sync();
        scene
    }

    fn kind(&self, name: &str) -> Kind {
        self.kinds.iter().find(|(n, _)| n == name).unwrap().1
    }

    fn factory(&self, kind: &str) -> EntityId {
        let k = self.kind(kind);
        self.world
            .snapshot()
            .iter()
            .find(|v| v.kind == k)
            .expect("the factory stands")
            .id
    }

    fn command(&mut self, command: Command) {
        let tick = Tick(self.world.tick().0 + 1);
        self.world.schedule(tick, P0, command).unwrap();
        self.world.step();
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

    /// One frame as `sf_game` draws it: the models, the bars, the flags and the readout of
    /// `selected`.
    fn shoot(&mut self, r: &mut Renderer, cam: &Camera, selected: EntityId) -> Rgba8 {
        let snap = SimSnapshot::capture(&self.world);
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
        let look = production_view::shipped_look();
        {
            let frame = GhostFrame {
                view_proj: vp,
                viewport: [W as f32, H as f32],
                ui_scale: 1.0,
            };
            let mut layer = ui.world_layer();
            let height = |x: f32, z: f32| ground.height(f64::from(x), f64::from(z)) as f32;
            production_view::draw_bars(&mut layer, &frame, &height, &snap, look);
            production_view::draw_rally_flags(
                &mut layer,
                &frame,
                &height,
                &snap,
                Some(selected),
                look,
            );
        }
        // The readout, by the functions the HUD calls.
        let kinds = &self.kinds;
        let name = |k: Kind| {
            kinds
                .iter()
                .find(|(_, x)| *x == k)
                .map_or_else(|| format!("kind {}", k.0), |(n, _)| n.clone())
        };
        let function = snap
            .menu()
            .factory_function(snap.get(selected).unwrap().kind)
            .unwrap();
        let menu = snap
            .menu()
            .units_for(function, sf_data::units::Faction::Meridian.bit());
        let next = production_view::menu_pick(&menu, 0)
            .and_then(|k| snap.menu().price_of(k).map(|(c, t)| (name(k), c, t)));
        let lines = production_view::panel_lines(
            &name(snap.get(selected).unwrap().kind),
            snap.factory_of(selected),
            next.as_ref().map(|(n, c, t)| (n.as_str(), *c, *t)),
            &name,
        );
        production_view::draw_panel(&mut ui, &lines, look);
        ui.end_frame();
        r.upload_units(&self.instancer);
        let mut ur = UiRenderer::new(&r.gpu().device, r.color_format());
        r.render_to_image_with(|t| ur.render(t, &mut ui))
            .expect("render")
    }
}

/// How many pixels of `img` inside `area` (x0, y0, x1, y1) are within `tol` steps (summed over the
/// channels) of `c`.
fn count_in(img: &Rgba8, area: (u32, u32, u32, u32), c: (u8, u8, u8, u8), tol: i32) -> usize {
    let (r, g, b, _) = c;
    (area.1..area.3.min(img.height))
        .flat_map(|y| (area.0..area.2.min(img.width)).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = img.pixel(x, y);
            (i32::from(p[0]) - i32::from(r)).abs()
                + (i32::from(p[1]) - i32::from(g)).abs()
                + (i32::from(p[2]) - i32::from(b)).abs()
                <= tol
        })
        .count()
}

/// Pixels of colour `c` (within `tol`) in the whole picture except the readout panel on the left.
fn world_count(img: &Rgba8, c: (u8, u8, u8, u8), tol: i32) -> usize {
    count_in(img, (380, 0, W, H), c, tol)
}

/// Pixels of colour `c` in the readout panel (left 380 columns).
fn panel_count(img: &Rgba8, c: (u8, u8, u8, u8), tol: i32) -> usize {
    count_in(img, (0, 90, 380, 330), c, tol)
}

#[test]
fn the_bars_the_flags_and_the_readout_follow_the_queues() {
    let look = *production_view::shipped_look();
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let cam = scene.camera(0.34, (89.0, 86.0));
    let barracks = scene.factory("barracks");
    let workshop = scene.factory("vehicle_factory");
    let (rifle, tank) = (scene.kind("rifleman"), scene.kind("light_tank"));
    let credits = scene.world.credits(P0);
    assert!(credits >= 1000, "the shipped starting credits: {credits}");

    // Two queues of their own and a rally point for the barracks.
    for kind in [rifle, rifle] {
        scene.command(Command::QueueUnit {
            factory: barracks,
            kind,
        });
    }
    for _ in 0..2 {
        scene.command(Command::QueueUnit {
            factory: workshop,
            kind: tank,
        });
    }
    scene.command(Command::SetRally {
        factory: barracks,
        cell: Some(Cell::new(92, 90)),
    });

    // Building: bars in the fill colour, the flag of the selected factory, the readout lines.
    let t0 = scene.run_until(400, |w| {
        w.production_of(workshop)
            .is_some_and(|v| v.front_permille >= 100)
            && w.production_of(barracks)
                .is_some_and(|v| v.status == FactoryStatus::Building && v.queue.len() >= 2)
    });
    let building = scene.shoot(&mut r, &cam, barracks);
    save(&format!("production_t{t0:05}_building.png"), &building);
    assert!(world_count(&building, look.bar.fill, 20) > 0, "the bars");
    assert_eq!(world_count(&building, look.waiting, 20), 0, "none waits");
    assert!(
        world_count(&building, look.rally.flag_selected, 20) > 0,
        "the selected factory's flag"
    );
    assert!(panel_count(&building, look.accent, 40) > 50, "front line");
    assert_eq!(panel_count(&building, look.warn, 40), 0);

    // 1000 credits: two riflemen (200) and a tank (700) leave 100 for the second tank, which
    // stops part way; the workshop waits for credits, its bar turns and its line goes to warning.
    let t1 = scene.run_until(2000, |w| {
        w.production_of(workshop)
            .is_some_and(|v| v.status == FactoryStatus::WaitingForMoney && v.front_permille > 0)
    });
    let waiting = scene.shoot(&mut r, &cam, workshop);
    save(&format!("production_t{t1:05}_waiting.png"), &waiting);
    assert!(world_count(&waiting, look.waiting, 20) > 0, "waiting bar");
    assert!(panel_count(&waiting, look.warn, 40) > 50, "warning line");
    assert_eq!(
        world_count(&waiting, look.rally.flag_selected, 20),
        0,
        "the workshop has no flag; the barracks' is no longer selected"
    );
    assert!(world_count(&waiting, look.rally.flag, 20) > 0, "other flag");

    // A rifleman came out and walks to the rally flag.
    let t2 = scene.run_until(2000, |w| {
        w.snapshot()
            .iter()
            .any(|v| v.kind == rifle && v.pos.y.cell() >= 88)
    });
    let walking = scene.shoot(&mut r, &cam, barracks);
    save(&format!("production_t{t2:05}_rally.png"), &walking);
    assert!(world_count(&walking, look.rally.flag_selected, 20) > 0);
}
