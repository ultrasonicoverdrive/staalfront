//! Screenshot test for power (WP6.4), through the real renderer on a software adapter (Mesa lavapipe /
//! DX12 WARP): the playfield with a construction centre, a power plant, a barracks and a refinery, as
//! `sf_game` draws it, with the sidebar on the right edge made by the functions the HUD calls
//! (`sidebar::power_reading`, `sidebar::model`, `sidebar_view::frame`).
//!
//! Three states are *found* in the simulation: enough power (the bar half full in the normal colour, a
//! rifleman advancing in the normal bar colour), a shortage (a vehicle factory and a radar tip the
//! bar over: the bar full in the low colour, the rifleman's bar in the low-power colour with the words
//! "low power", and a power plant going up slowly) and a damaged plant (four tanks shoot it: it makes
//! less, the bar shows the smaller number). The checks look at the pixels. No golden image. Set
//! `SF_SAVE_SHOTS=<dir>` to write the images with their tick in the name (`docs/screenshots/wp6.4/`).

use std::path::PathBuf;
use std::sync::Arc;

use sf_data::map::Map;
use sf_data::money_look::MoneyLook;
use sf_data::sidebar_look::SidebarLook;
use sf_data::units::{Faction, UnitDefs};
use sf_game::playfield::{
    make_world, with_buildings, with_combat, with_harvest, with_money, with_movement, with_power,
    with_production, with_storage,
};
use sf_game::roster::Roster;
use sf_game::sidebar::{self, Catalog, Situation, Tab};
use sf_game::sidebar_layout;
use sf_game::sidebar_view::{self, SidebarState};
use sf_game::sim_snapshot::SimSnapshot;
use sf_math::Vec2;
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8};
use sf_render::model::EyeView;
use sf_render::{
    Camera, CameraConfig, FrameView, Frustum, GpuOptions, Ground, Instancer, Motion, Renderer,
    Style, TerrainGround, UnitId,
};
use sf_sim::{
    Cell, Command, EntityId, FactoryStatus, Kind, Order, PlayerId, Tick, World, WorldConfig,
};
use sf_ui::{Rect, Ui, UiConfig, UiInput, UiRenderer};

const W: u32 = 1280;
const H: u32 = 720;
const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn icon_set() -> sf_render::IconAtlas {
    let atlas = sf_render::IconAtlas::load(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons"),
    );
    assert!(atlas.warnings().is_empty(), "{:?}", atlas.warnings());
    atlas
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
    println!("adapter: {}", r.adapter_summary());
    r.set_scene(&[]);
    r.set_terrain(map);
    r.set_icon_atlas(&icon_set());
    r
}

fn save(name: &str, img: &Rgba8) {
    if let Some(dir) = std::env::var_os("SF_SAVE_SHOTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("shot directory");
        image::write_png(&dir.join(name), img).expect("save shot");
    }
}

struct Scene {
    map: Map,
    ground: Arc<TerrainGround>,
    world: World,
    instancer: Instancer,
    roster: Roster,
    defs: UnitDefs,
    catalog: Catalog,
    look: SidebarLook,
}

impl Scene {
    fn new() -> Scene {
        let map = Map::load(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps/playfield256"),
        )
        .expect("the playfield map");
        let ground = Arc::new(TerrainGround::new(&map));
        let defs = UnitDefs::embedded();
        let cells = map.size_in_cells();
        let world = make_world(WorldConfig::new(1).with_map_size(cells.0, cells.1), &map)
            .unwrap()
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap();
        let world = with_buildings(with_movement(world, &defs), &defs);
        let world = with_storage(with_harvest(world, &defs), &defs);
        let world = with_power(with_production(with_money(world), &defs));
        let mut world = with_combat(world, &defs);
        let kind = |name: &str| Kind(defs.kind_of(name).unwrap());
        for (name, cell) in [
            ("construction_center", (84, 74)),
            ("power_plant", (78, 74)),
            ("barracks", (84, 80)),
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
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let base = Style::embedded().scaled(3.0);
        let roster = Roster::build(&defs, &base, &icon_set(), &assets, "", false).unwrap();
        let mut instancer = Instancer::new(
            roster.style(&base),
            (map.width() as f32, map.height() as f32),
        );
        instancer.set_height_range(ground.min_height() as f32, ground.max_height() as f32);
        let look = SidebarLook::embedded();
        let catalog = Catalog::build(&defs, Faction::Meridian, &look);
        let mut scene = Scene {
            map,
            ground,
            world,
            instancer,
            roster,
            defs,
            catalog,
            look,
        };
        scene.world.step();
        scene.sync();
        scene
    }

    fn kind(&self, name: &str) -> Kind {
        Kind(self.defs.kind_of(name).unwrap())
    }

    fn id_of(&self, name: &str) -> EntityId {
        let k = self.kind(name);
        self.world
            .snapshot()
            .iter()
            .find(|v| v.kind == k)
            .expect("the structure stands")
            .id
    }

    fn command(&mut self, command: Command) {
        let tick = Tick(self.world.tick().0 + 1);
        self.world.schedule(tick, P0, command).unwrap();
        self.world.step();
    }

    /// A cell near the construction centre where a power plant may go up (the answer of
    /// `World::preview_placement`, the function `BuildStructure` runs).
    fn free_cell_for(&self, name: &str) -> Cell {
        let kind = self.kind(name);
        for dy in 0..24u32 {
            for dx in 0..24u32 {
                let cell = Cell::new(70 + dx, 62 + dy);
                if self.world.preview_placement(kind, cell, P0).verdict.is_ok() {
                    return cell;
                }
            }
        }
        panic!("no room for a {name}");
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

    fn camera(&self) -> Camera {
        let cfg = CameraConfig {
            map_size: DVec2::new(f64::from(self.map.width()), f64::from(self.map.height())),
            ..CameraConfig::default()
        };
        let mut c = Camera::new(cfg, W, H);
        c.set_ground(self.ground.clone());
        c.set_zoom(0.34);
        c.set_focus(DVec2::new(86.0 * 4.0, 80.0 * 4.0));
        c
    }

    /// One frame as `sf_game` draws it: the world and the sidebar showing `tab`.
    fn shoot(&mut self, r: &mut Renderer, cam: &Camera, tab: Tab) -> Rgba8 {
        let snap = SimSnapshot::capture(&self.world);
        let vp = cam.view_proj();
        r.set_view_proj(vp);
        let frustum = Frustum::from_view_proj(vp);
        let eye = EyeView::from_view_proj(vp, H as f32);
        let blend = sf_render::CameraSettings::embedded()
            .zoom
            .blend(cam.zoom() as f32);
        let ground = &self.ground;
        self.instancer
            .build_frame(&FrameView::new(&frustum, 0.0, eye, blend, 1.0), |x, z| {
                ground.height(f64::from(x), f64::from(z)) as f32
            });
        let mut ui = Ui::new(UiConfig::default());
        let input = UiInput::default();
        ui.begin_frame(input, (W, H), 1.0);
        let sit = Situation::read(&snap, P0, &self.catalog, &[]);
        let power = sidebar::power_reading(&snap, P0);
        let model = sidebar::model(&self.catalog, &sit, tab, None, power, &self.look);
        let money = MoneyLook::embedded();
        let setup = sidebar_view::Setup {
            look: &self.look,
            money: &money,
            edge_band_px: 4.0,
            minimap: true,
        };
        let mut state = SidebarState { tab, scroll: 0 };
        sidebar_view::frame(&mut ui, &input, &mut state, &model, &setup);
        ui.end_frame();
        r.upload_units(&self.instancer);
        let mut ur = UiRenderer::new(&r.gpu().device, r.color_format());
        r.render_to_image_with(|t| ur.render(t, &mut ui))
            .expect("render")
    }
}

fn column() -> Rect {
    sidebar_layout::layout(
        Rect::new(0.0, 0.0, W as f32, H as f32),
        1.0,
        &SidebarLook::embedded(),
        4.0,
        true,
    )
    .column
}

/// How many pixels of `img` inside `area` are within `tol` steps (summed over the channels) of `c`.
fn count_in(img: &Rgba8, area: Rect, c: (u8, u8, u8, u8), tol: i32) -> usize {
    let (x0, y0) = (area.x as u32, area.y as u32);
    let (x1, y1) = (area.right() as u32, area.bottom() as u32);
    (y0..y1.min(img.height))
        .flat_map(|y| (x0..x1.min(img.width)).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = img.pixel(x, y);
            (i32::from(p[0]) - i32::from(c.0)).abs()
                + (i32::from(p[1]) - i32::from(c.1)).abs()
                + (i32::from(p[2]) - i32::from(c.2)).abs()
                <= tol
        })
        .count()
}

/// The strip along the bottom edge of the item in slot `n` (where its progress bar is).
fn bar_strip(n: usize) -> Rect {
    let look = SidebarLook::embedded();
    let layout = sidebar_layout::layout(
        Rect::new(0.0, 0.0, W as f32, H as f32),
        1.0,
        &look,
        4.0,
        true,
    );
    let slot = layout.slot(n);
    let h = f32::from(look.bar_height_px);
    Rect::new(slot.x, slot.bottom() - h, slot.w, h)
}

fn opaque(c: (u8, u8, u8)) -> (u8, u8, u8, u8) {
    (c.0, c.1, c.2, 255)
}

fn power_bar() -> Rect {
    sidebar_layout::layout(
        Rect::new(0.0, 0.0, W as f32, H as f32),
        1.0,
        &SidebarLook::embedded(),
        4.0,
        true,
    )
    .power
}

fn area(r: Rect) -> usize {
    (r.w * r.h) as usize
}

impl Scene {
    /// Sends `command` as `player` for the tick about to run, and runs it.
    fn command_as(&mut self, player: PlayerId, command: Command) {
        let tick = self.world.tick();
        self.world.schedule(tick, player, command).unwrap();
        self.world.step();
    }

    fn place(&mut self, name: &str, cell: (u32, u32)) {
        let kind = self.kind(name);
        self.command_as(
            P0,
            Command::PlaceStructure {
                kind,
                cell: Cell::new(cell.0, cell.1),
                owner: P0,
            },
        );
    }

    fn reading(&self) -> sidebar::PowerReading {
        sidebar::power_reading(&SimSnapshot::capture(&self.world), P0).expect("a world with power")
    }
}

#[test]
fn power_is_fine_then_short_and_the_sidebar_shows_it() {
    let look = SidebarLook::embedded();
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let cam = scene.camera();
    let bar = power_bar();
    let (barracks, rifle) = (scene.id_of("barracks"), scene.kind("rifleman"));

    // 1. Enough power: a plant (+100) against a refinery (-30) and a barracks (-20).
    for _ in 0..2 {
        scene.command(Command::QueueUnit {
            factory: barracks,
            kind: rifle,
        });
    }
    let t0 = scene.run_until(400, |w| {
        w.production_of(barracks)
            .is_some_and(|v| v.status == FactoryStatus::Building && v.front_permille >= 300)
    });
    let reading = scene.reading();
    assert_eq!((reading.made, reading.used), (100, 50));
    let fine = scene.shoot(&mut r, &cam, Tab::Infantry);
    save(&format!("power_t{t0:05}_fine_infantry.png"), &fine);
    // The bar is half full, in the normal colour; the rifleman's bar is the normal blue.
    let filled = count_in(&fine, bar, look.colors.power_fill, 6);
    assert!(
        filled > area(bar) * 4 / 10 && filled < area(bar) * 6 / 10,
        "half of the bar: {filled} of {}",
        area(bar)
    );
    assert_eq!(count_in(&fine, bar, look.colors.power_low, 6), 0);
    assert!(count_in(&fine, bar_strip(0), look.colors.bar_fill, 60) > 10);
    assert_eq!(count_in(&fine, bar_strip(0), look.colors.bar_low, 30), 0);

    // 2. A vehicle factory (-40) and a radar (-50): 140 used of 100 made, speed 714.
    scene.place("vehicle_factory", (90, 74));
    scene.place("radar", (72, 80));
    let t1 = scene.run_until(100, |w| {
        w.power_of(P0).is_some_and(|t| t.used == 140)
            && w.production_of(barracks)
                .is_some_and(|v| v.status == FactoryStatus::Building && v.front_permille >= 450)
    });
    let reading = scene.reading();
    assert_eq!((reading.made, reading.used), (100, 140));
    assert_eq!(scene.world.production_speed_permille(P0), 714);
    let short = scene.shoot(&mut r, &cam, Tab::Infantry);
    save(&format!("power_t{t1:05}_shortage_infantry.png"), &short);
    // The bar is full and in the low colour; the rifleman advances in the low-power colour.
    assert!(count_in(&short, bar, look.colors.power_low, 6) > area(bar) * 6 / 10);
    assert_eq!(count_in(&short, bar, look.colors.power_fill, 6), 0);
    assert!(count_in(&short, bar_strip(0), look.colors.bar_low, 40) > 10);
    assert_eq!(count_in(&short, bar_strip(0), look.colors.bar_fill, 20), 0);
    // The words are there: the item says "low power" (the model, which the HUD draws from).
    let snap = SimSnapshot::capture(&scene.world);
    let sit = Situation::read(&snap, P0, &scene.catalog, &[]);
    let items = sidebar::items_for(&scene.catalog, &sit, Tab::Infantry);
    assert!(items[0].low_power && items[0].waiting.is_none());

    // 3. A power plant ordered in the shortage goes up at the same slow pace.
    let cell = scene.free_cell_for("power_plant");
    scene.command(Command::BuildStructure {
        kind: scene.kind("power_plant"),
        cell,
    });
    let t2 = scene.run_until(400, |w| w.constructions().any(|(_, c)| c.permille() >= 100));
    let building = scene.shoot(&mut r, &cam, Tab::Structures);
    save(
        &format!("power_t{t2:05}_shortage_structures.png"),
        &building,
    );
    let plant = scene
        .catalog
        .entry(scene.kind("power_plant"))
        .unwrap()
        .clone();
    assert!(count_in(&building, column(), opaque(plant.picture.color), 6) > 400);
    assert!(count_in(&building, bar_strip(0), look.colors.bar_low, 40) > 5);
    assert_eq!(
        count_in(&building, bar_strip(0), look.colors.bar_fill, 20),
        0
    );
}

#[test]
fn a_damaged_plant_makes_less_and_the_bar_shows_it() {
    let look = SidebarLook::embedded();
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let cam = scene.camera();
    let bar = power_bar();
    let (barracks, rifle) = (scene.id_of("barracks"), scene.kind("rifleman"));
    let plant = scene.id_of("power_plant");
    scene.command(Command::QueueUnit {
        factory: barracks,
        kind: rifle,
    });
    scene.world.step();
    scene.world.step();
    let before = scene.reading();
    assert_eq!((before.made, before.used), (100, 50));

    // Four Veyran light tanks come to shoot the plant.
    let tank = scene.kind("light_tank");
    let tanks_of = |w: &World| -> Vec<EntityId> {
        w.snapshot()
            .iter()
            .filter(|v| v.kind == tank && v.owner == P1)
            .map(|v| v.id)
            .collect()
    };
    // A spawn that lands on a cell a tank cannot stand on is ignored: try cells in the open ground north-west
    // of the base until four stand there.
    for (x, y) in (60..76)
        .step_by(2)
        .flat_map(|x| (60..72).step_by(2).map(move |y| (x, y)))
    {
        if tanks_of(&scene.world).len() == 4 {
            break;
        }
        scene.command_as(
            P1,
            Command::SpawnUnit {
                kind: tank,
                pos: Vec2::from_ints(x, y) + Vec2::new(sf_math::Fx::HALF, sf_math::Fx::HALF),
                owner: P1,
            },
        );
    }
    let tanks = tanks_of(&scene.world);
    assert_eq!(tanks.len(), 4);
    scene.command_as(
        P1,
        Command::Order {
            ids: tanks,
            order: Order::Attack(plant),
            queued: false,
        },
    );
    // Until it is down to about a third: it makes 100 * hp / 600.
    let t = scene.run_until(1500, |w| w.hp_of(plant).is_some_and(|hp| hp <= 220));
    scene.world.step();
    let hp = scene.world.hp_of(plant).expect("it still stands");
    let reading = scene.reading();
    assert_eq!(u64::from(reading.made), 100 * u64::from(hp) / 600);
    assert!(reading.made < 50, "the plant no longer covers the base");
    assert_eq!(reading.used, 50);
    let speed = scene.world.production_speed_permille(P0);
    assert_eq!(
        u64::from(speed),
        (u64::from(reading.made) * 1000 / 50).max(250)
    );
    assert!(speed < 1000);
    let shot = scene.shoot(&mut r, &cam, Tab::Infantry);
    save(&format!("power_t{t:05}_damaged_plant.png"), &shot);
    assert!(count_in(&shot, bar, look.colors.power_low, 6) > area(bar) * 6 / 10);
    assert_eq!(count_in(&shot, bar, look.colors.power_fill, 6), 0);
}
