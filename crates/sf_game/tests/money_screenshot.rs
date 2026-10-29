//! Screenshot test for the money (WP7.4), through the real renderer on a software adapter (Mesa
//! lavapipe / DX12 WARP): the playfield with a construction centre, a refinery and two harvesters,
//! and a power plant that is built with 150 starting credits. One frame per state, as `sf_game`
//! draws it: the models, the construction bar (`construction_view`, which turns to the paused look
//! while the structure waits for money) and the credits readout (`money_view::draw_panel`, the
//! function the HUD calls) on the UI.
//!
//! The states are *found* in the simulation (`World::credits`, `World::stalled`). The checks look at
//! the pixels: the colours of `money_look.ron` and `build_stages.ron` appear exactly when the
//! structure waits or runs, and the readout turns to its warning colour. No golden image. Set
//! `SF_SAVE_SHOTS=<dir>` to write the images with their tick in the name
//! (`docs/screenshots/wp7.4/`).

use std::path::PathBuf;
use std::sync::Arc;

use sf_data::map::Map;
use sf_data::units::UnitDefs;
use sf_game::construction_view;
use sf_game::money_view::{self, MoneyHud};
use sf_game::ore_view::OreView;
use sf_game::placement_view::GhostFrame;
use sf_game::playfield::{make_world, with_buildings, with_harvest, with_movement};
use sf_game::roster::Roster;
use sf_game::sim_snapshot::SimSnapshot;
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8};
use sf_render::model::EyeView;
use sf_render::terrain_look::TerrainLooks;
use sf_render::{
    Camera, CameraConfig, FrameView, Frustum, GpuOptions, Ground, Instancer, Motion, Renderer,
    Style, TerrainGround, UnitId,
};
use sf_sim::{Cell, Command, Kind, MoneyRules, PlayerId, Tick, World, WorldConfig};
use sf_ui::{Ui, UiConfig, UiInput, UiRenderer};

const W: u32 = 960;
const H: u32 = 540;
const P0: PlayerId = PlayerId(0);
/// Credits player 0 starts with: half of what the power plant costs.
const START: u64 = 150;

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

/// The construction centre (slot 0), the refinery (slot 1) and two harvesters (slots 2, 3) at the
/// places of the `money` scenario, on the playfield; a power plant is built at (90, 74).
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
        let kind = |name: &str| Kind(defs.kind_of(name).unwrap());
        let mut world = world
            .with_storage(&[(kind("refinery"), 400)])
            .unwrap()
            .with_money(MoneyRules { credits_per_ore: 1 }, &[(P0, START)])
            .unwrap();
        let at = |c: i32, r: i32| {
            sf_math::Vec2::new(
                sf_math::Fx::from_int(c) + sf_math::Fx::HALF,
                sf_math::Fx::from_int(r) + sf_math::Fx::HALF,
            )
        };
        for (name, cell) in [("construction_center", (84, 74)), ("refinery", (96, 86))] {
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
        for (x, y) in [(104, 90), (106, 92)] {
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
        world
            .schedule(
                Tick(1),
                P0,
                Command::BuildStructure {
                    kind: kind("power_plant"),
                    cell: Cell::new(90, 74),
                },
            )
            .unwrap();
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

    /// One frame as `sf_game` draws it: the models, the construction bars and the credits readout.
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
            construction_view::draw_bars(
                &mut layer,
                &frame,
                &height,
                &snap,
                &self.roster.stages().bar,
                &money_view::shipped_look().paused,
            );
        }
        // The readout, by the function the HUD calls, for the local player.
        let money = MoneyHud {
            credits: snap.credits_of(P0).expect("a world with money"),
            waiting: snap.waits_for_money(P0),
        };
        money_view::draw_panel(&mut ui, &money, money_view::shipped_look());
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

/// The picture without the corner of the readout (whose warning colour is the pause colour).
fn count(img: &Rgba8, c: (u8, u8, u8, u8), tol: i32) -> usize {
    count_in(img, (0, 90, W, H), c, tol) + count_in(img, (0, 0, W - 270, 90), c, tol)
}

/// The corner of the credits readout and its strip: right of x = W - 240, above y = 90.
const CORNER: (u32, u32, u32, u32) = (W - 270, 0, W, 90);

#[test]
fn the_bar_pauses_when_the_money_is_gone_and_the_readout_follows_the_credits() {
    let money = money_view::shipped_look();
    let normal_fill = Scene::new().roster.stages().bar.fill;
    let paused_fill = money.paused.fill;
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let cam = scene.camera(0.25, (91.0, 78.0));

    // Running: credits left, the bar in its own colours, the readout in the normal colour.
    let t0 = scene.run_until(400, |w| {
        w.credits(P0) <= START - 50 && w.stalled().is_empty()
    });
    assert!(
        scene.world.credits(P0) >= 100,
        "still enough for the normal colour"
    );
    let running = scene.shoot(&mut r, &cam);
    save(&format!("money_t{t0:05}_building.png"), &running);
    assert!(count(&running, normal_fill, 20) > 0, "the bar is drawn");
    assert_eq!(
        count(&running, paused_fill, 20),
        0,
        "no pause mark while it is paid"
    );
    assert!(
        count_in(&running, CORNER, money.text, 40) > 20,
        "the readout, normal colour"
    );

    // Out of money: the paused bar with its strokes, the readout in the warning colour and the strip.
    let t1 = scene.run_until(400, |w| !w.stalled().is_empty());
    assert_eq!(scene.world.credits(P0), 0);
    let paused = scene.shoot(&mut r, &cam);
    save(&format!("money_t{t1:05}_paused.png"), &paused);
    assert!(
        count(&paused, paused_fill, 20) > 0,
        "the paused bar and strokes"
    );
    assert_eq!(count(&paused, normal_fill, 20), 0, "no running bar");
    assert!(
        count_in(&paused, CORNER, money.low, 40) > 100,
        "the warning colour, with the strip"
    );
    assert_eq!(count_in(&paused, CORNER, money.text, 20), 0);
    // The strip is a second panel under the readout: more pixels in the corner than without it.
    assert!(count_in(&paused, CORNER, money.low, 40) > count_in(&running, CORNER, money.low, 40),);

    // Ore arrives: the structure goes on, the bar has its own colours again.
    let t2 = scene.run_until(3000, |w| {
        w.ore_delivered(P0) >= 20 && w.stalled().is_empty()
    });
    let resumed = scene.shoot(&mut r, &cam);
    save(&format!("money_t{t2:05}_resumed.png"), &resumed);
    assert!(count(&resumed, normal_fill, 20) > 0, "the bar runs again");
    assert_eq!(count(&resumed, paused_fill, 20), 0);
}
