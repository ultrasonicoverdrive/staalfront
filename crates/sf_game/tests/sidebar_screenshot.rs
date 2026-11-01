//! Screenshot test for the sidebar (WP11.1), through the real renderer on a software adapter (Mesa
//! lavapipe / DX12 WARP): the playfield with a construction centre, a power plant, a barracks, a
//! vehicle factory and a refinery, as `sf_game` draws it, with the sidebar on the right edge made by
//! the functions the HUD calls (`sidebar::model`, `sidebar_view::frame`).
//!
//! The states are *found* in the simulation: a power plant going up (the structures tab, an item in
//! progress), two riflemen in the barracks queue (the infantry tab), a light tank that waits for
//! credits (the vehicles tab, an item waiting) and a tab with nothing to build (the vessels tab).
//! The checks look at the pixels: the colours of `sidebar.ron` appear exactly where the sidebar says
//! they are, the picture of every item is in its own tile and the rest of the window is the world's.
//! No golden image. Set `SF_SAVE_SHOTS=<dir>` to write the images with their tick in the name
//! (`docs/screenshots/wp11.1/`).

use std::path::PathBuf;
use std::sync::Arc;

use sf_data::map::Map;
use sf_data::money_look::MoneyLook;
use sf_data::sidebar_look::SidebarLook;
use sf_data::units::{Faction, UnitDefs};
use sf_game::playfield::{
    make_world, with_buildings, with_harvest, with_money, with_movement, with_production,
    with_storage,
};
use sf_game::roster::Roster;
use sf_game::sidebar::{self, Catalog, Situation, Tab};
use sf_game::sidebar_layout;
use sf_game::sidebar_view::{self, SidebarState};
use sf_game::sim_snapshot::SimSnapshot;
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8};
use sf_render::model::EyeView;
use sf_render::{
    Camera, CameraConfig, FrameView, Frustum, GpuOptions, Ground, Instancer, Motion, Renderer,
    Style, TerrainGround, UnitId,
};
use sf_sim::{Cell, Command, EntityId, FactoryStatus, Kind, PlayerId, Tick, World, WorldConfig};
use sf_ui::{Rect, Ui, UiConfig, UiInput, UiRenderer};

const W: u32 = 1280;
const H: u32 = 720;
const P0: PlayerId = PlayerId(0);

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
        let mut world = with_production(with_money(world), &defs);
        let kind = |name: &str| Kind(defs.kind_of(name).unwrap());
        for (name, cell) in [
            ("construction_center", (84, 74)),
            ("power_plant", (78, 74)),
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
        let input = UiInput::default();
        ui.begin_frame(input, (W, H), 1.0);
        let sit = Situation::read(&snap, P0, &self.catalog, &[]);
        let model = sidebar::model(&self.catalog, &sit, tab, None, None, &self.look);
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

#[test]
fn the_sidebar_shows_the_state_of_the_game_on_every_tab() {
    let look = SidebarLook::embedded();
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let cam = scene.camera();
    let col = column();
    let (barracks, workshop) = (scene.id_of("barracks"), scene.id_of("vehicle_factory"));
    let (rifle, tank) = (scene.kind("rifleman"), scene.kind("light_tank"));
    let credits = scene.world.credits(P0);
    assert_eq!(credits, 1000, "the shipped starting credits");

    // A power plant goes up (300), two riflemen (200) and a tank (700) are queued: 1,200 credits
    // asked of 1,000.
    let cell = scene.free_cell_for("power_plant");
    scene.command(Command::BuildStructure {
        kind: scene.kind("power_plant"),
        cell,
    });
    for kind in [rifle, rifle] {
        scene.command(Command::QueueUnit {
            factory: barracks,
            kind,
        });
    }
    scene.command(Command::QueueUnit {
        factory: workshop,
        kind: tank,
    });

    // The structures tab with the plant going up: its progress bar, its count, its own picture.

    let t0 = scene.run_until(400, |w| {
        w.constructions().any(|(_, c)| c.permille() >= 300)
            && w.production_of(barracks)
                .is_some_and(|v| v.status == FactoryStatus::Building && v.front_permille >= 150)
    });
    let structures = scene.shoot(&mut r, &cam, Tab::Structures);
    save(&format!("sidebar_t{t0:05}_structures.png"), &structures);
    let plant = scene
        .catalog
        .entry(scene.kind("power_plant"))
        .unwrap()
        .clone();
    assert!(
        count_in(&structures, col, opaque(plant.picture.color), 6) > 400,
        "the picture of the power plant"
    );
    assert!(
        count_in(&structures, bar_strip(0), look.colors.bar_fill, 60) > 10,
        "the progress bar of the plant"
    );
    assert_eq!(
        count_in(&structures, bar_strip(0), look.colors.bar_waiting, 30),
        0
    );
    assert!(
        count_in(&structures, col, look.colors.tab_active, 6) > 500,
        "the active tab"
    );
    // The world is what it was outside the column: the buildings are drawn left of it.
    let world_part = Rect::new(0.0, 0.0, col.x, H as f32);
    assert!(count_in(&structures, world_part, look.colors.tile_back, 6) == 0);

    // The infantry tab: both riflemen in the queue, the front one with its progress.
    let infantry = scene.shoot(&mut r, &cam, Tab::Infantry);
    save(&format!("sidebar_t{t0:05}_infantry.png"), &infantry);
    let rifle_entry = scene.catalog.entry(rifle).unwrap().clone();
    assert!(count_in(&infantry, col, opaque(rifle_entry.picture.color), 6) > 400);
    assert!(count_in(&infantry, bar_strip(0), look.colors.bar_fill, 60) > 10);
    // The queue count: the badge's dark back in the corner of a picture (it shows through the
    // picture, so the match is loose); the rifleman is queued twice, the scout not at all.
    let layout = sidebar_layout::layout(
        Rect::new(0.0, 0.0, W as f32, H as f32),
        1.0,
        &look,
        4.0,
        true,
    );
    let corner = |n: usize| {
        let slot = layout.slot(n);
        Rect::new(slot.right() - 14.0, slot.y + 1.0, 12.0, 12.0)
    };
    let dark = (
        look.colors.badge_back.0,
        look.colors.badge_back.1,
        look.colors.badge_back.2,
        255,
    );
    let badge = count_in(&infantry, corner(0), dark, 130);
    assert!(
        badge > 30,
        "the queue count of the rifleman: {badge} dark pixels"
    );
    assert_eq!(
        count_in(&infantry, corner(3), dark, 130),
        0,
        "none for the scout"
    );
    // The structures' pictures are not on this tab.
    assert_eq!(
        count_in(&infantry, col, opaque(plant.picture.color), 0),
        0,
        "no power plant on the infantry tab"
    );

    // 500 credits are left for a tank that costs 700: the workshop waits.
    let t1 = scene.run_until(2000, |w| {
        w.production_of(workshop)
            .is_some_and(|v| v.status == FactoryStatus::WaitingForMoney && v.front_permille > 0)
    });
    let vehicles = scene.shoot(&mut r, &cam, Tab::Vehicles);
    save(&format!("sidebar_t{t1:05}_vehicles_waiting.png"), &vehicles);
    assert!(
        count_in(&vehicles, bar_strip(1), look.colors.bar_waiting, 40) > 10,
        "the bar of the tank turns to the waiting colour"
    );
    assert_eq!(
        count_in(&vehicles, bar_strip(1), look.colors.bar_fill, 20),
        0,
        "nothing advances"
    );
    let tank_entry = scene.catalog.entry(tank).unwrap().clone();
    assert!(count_in(&vehicles, col, opaque(tank_entry.picture.color), 6) > 400);

    // The vessels tab: no shipyard stands, so the tab says what it needs and shows no tile.
    let vessels = scene.shoot(&mut r, &cam, Tab::Vessels);
    save(&format!("sidebar_t{t1:05}_vessels.png"), &vessels);
    let layout = sidebar_layout::layout(
        Rect::new(0.0, 0.0, W as f32, H as f32),
        1.0,
        &look,
        4.0,
        true,
    );
    assert_eq!(count_in(&vessels, layout.area, look.colors.tile_back, 4), 0);
    assert!(
        count_in(&vessels, layout.area, look.colors.text_dim, 40) > 30,
        "the hint"
    );
    // And the structures tab again at this tick, the plant done: its bar is gone.
    let later = scene.shoot(&mut r, &cam, Tab::Structures);
    save(&format!("sidebar_t{t1:05}_structures.png"), &later);
}
