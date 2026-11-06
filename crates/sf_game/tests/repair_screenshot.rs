//! Screenshot test for repair and sale (WP6.6, `docs/rules/repair_sell.md`), through the real renderer on a
//! software adapter (Mesa lavapipe / DX12 WARP): the playfield with a construction centre, a power plant, a
//! barracks and a refinery, as `sf_game` draws it, with the sidebar (its Repair and Sell buttons), the repair
//! cursor, the repair sign and the sale bar made by the functions the game calls
//! (`repair_mode::click`/`cursor`, `repair_view::draw`, `sidebar_view::frame`, `SelectionView::mark`).
//!
//! The states are *found* in the simulation: four Veyran light tanks shoot the power plant down to about a
//! third; with the Repair mode on the cursor is the spanner over the plant and the click sends `Repair`; the
//! plant repairs under its sign (hit points rising, money falling by the exact cost) up to full; a sale
//! shows its bar growing and the structure is gone at the end. The checks look at the pixels and at the
//! numbers. No golden image. Set `SF_SAVE_SHOTS=<dir>` to write the images with their tick in the name
//! (`docs/screenshots/wp6.6/`).

use std::path::PathBuf;
use std::sync::Arc;

use sf_data::map::Map;
use sf_data::money_look::MoneyLook;
use sf_data::sidebar_look::SidebarLook;
use sf_data::units::{Faction, UnitDefs};
use sf_game::cursor::{CursorArt, CursorKind};
use sf_game::placement_view::GhostFrame;
use sf_game::playfield::{
    make_world, with_buildings, with_combat, with_harvest, with_money, with_movement, with_power,
    with_production, with_repair, with_storage,
};
use sf_game::repair_mode::{self, ToolMode};
use sf_game::repair_view;
use sf_game::roster::Roster;
use sf_game::selection::Selection;
use sf_game::selection_view::{FrameGeometry, SelectionView};
use sf_game::sidebar::{self, Catalog, Situation, Tab};
use sf_game::sidebar_layout;
use sf_game::sidebar_view::{self, SidebarState};
use sf_game::sim_snapshot::SimSnapshot;
use sf_math::Vec2;
use sf_math::render_bridge::vec2_cells_to_metres;
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8};
use sf_render::model::EyeView;
use sf_render::pick::project_px;
use sf_render::{
    Camera, CameraConfig, FrameView, Frustum, GpuOptions, Ground, Instancer, Motion, Renderer,
    Style, TerrainGround, UnitId,
};
use sf_sim::{Cell, Command, EntityId, Kind, Order, PlayerId, Tick, World, WorldConfig};
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
        let world = with_repair(with_power(with_production(with_money(world), &defs)));
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

    /// The screen position of the middle of the structure `id`.
    fn screen_of(&self, cam: &Camera, id: EntityId) -> (f32, f32) {
        let v = self.world.get(id).expect("it is there");
        let [x, z] = vec2_cells_to_metres(v.pos);
        let y = self.ground.height(f64::from(x), f64::from(z)) as f32;
        let p = project_px(&cam.view_proj(), [x, y, z].into(), [W as f32, H as f32])
            .expect("in front of the camera")
            .0;
        (p[0], p[1])
    }

    /// One frame as `sf_game` draws it: the world, the health bar of `selected`, the repair signs and
    /// the sale bars, the sidebar with `mode` on and the cursor.
    fn shoot(
        &mut self,
        r: &mut Renderer,
        cam: &Camera,
        mode: Option<ToolMode>,
        selected: Option<EntityId>,
        cursor: Option<(CursorKind, (f32, f32))>,
    ) -> Rgba8 {
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
        let ground = self.ground.clone();
        let height = |x: f32, z: f32| ground.height(f64::from(x), f64::from(z)) as f32;
        self.instancer
            .build_frame(&FrameView::new(&frustum, 0.0, eye, blend, 1.0), height);
        let mut ui = Ui::new(UiConfig::default());
        let set = sf_ui::CursorSet::load(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/ui/cursors"),
            ui.atlas_mut(),
        )
        .expect("the cursor set loads");
        let art = CursorArt::new(set).expect("every kind has a picture");
        let input = UiInput::default();
        ui.begin_frame(input, (W, H), 1.0);
        let sit = Situation::read(&snap, P0, &self.catalog, &[]);
        let power = sidebar::power_reading(&snap, P0);
        let model = sidebar::model(
            &self.catalog,
            &sit,
            Tab::Structures,
            None,
            power,
            &self.look,
        )
        .with_tools(mode);
        let money = MoneyLook::embedded();
        let setup = sidebar_view::Setup {
            look: &self.look,
            money: &money,
            edge_band_px: 4.0,
            minimap: true,
        };
        let mut state = SidebarState {
            tab: Tab::Structures,
            scroll: 0,
        };
        sidebar_view::frame(&mut ui, &input, &mut state, &model, &setup);
        ui.end_frame();
        // The map's marks on the UI's world layer, as the runner draws them.
        let ghost_frame = GhostFrame {
            view_proj: vp,
            viewport: [W as f32, H as f32],
            ui_scale: 1.0,
        };
        {
            let mut selection = Selection::new(P0);
            if let Some(id) = selected {
                selection.click(&snap, Some(id), false);
            }
            let frame = FrameGeometry {
                view_proj: vp,
                viewport: [W as f32, H as f32],
                ui_scale: 1.0,
                blend,
                icon_px: self.instancer.icon_params().size_px,
            };
            let mut layer = ui.world_layer();
            SelectionView::default().mark(
                &mut self.instancer,
                &selection,
                &snap,
                &frame,
                &mut layer,
            );
            repair_view::draw(
                &mut layer,
                &ghost_frame,
                &height,
                &snap,
                repair_view::shipped_look(),
            );
        }
        if let Some((kind, at)) = cursor {
            ui.draw_cursor(art.set(), art.sprite_of(kind), at);
        }
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

/// The part of the picture that is the map: everything left of the sidebar.
fn map_area() -> Rect {
    Rect::new(0.0, 0.0, column().x, H as f32)
}

fn tools() -> [Rect; 2] {
    sidebar_layout::layout(
        Rect::new(0.0, 0.0, W as f32, H as f32),
        1.0,
        &SidebarLook::embedded(),
        4.0,
        true,
    )
    .tools
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

impl Scene {
    /// Sends `command` as `player` for the tick about to run, and runs it.
    fn command_as(&mut self, player: PlayerId, command: Command) {
        let tick = self.world.tick();
        self.world.schedule(tick, player, command).unwrap();
        self.world.step();
    }

    fn run(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.world.step();
        }
        self.sync();
    }

    /// Four Veyran light tanks shoot `target` down to at most `hp` hit points and are then removed.
    /// Returns the hit points left.
    fn hurt(&mut self, target: EntityId, hp: u32) -> u32 {
        let tank = self.kind("light_tank");
        let tanks_of = |w: &World| -> Vec<EntityId> {
            w.snapshot()
                .iter()
                .filter(|v| v.kind == tank && v.owner == P1)
                .map(|v| v.id)
                .collect()
        };
        for (x, y) in (60..76)
            .step_by(2)
            .flat_map(|x| (60..72).step_by(2).map(move |y| (x, y)))
        {
            if tanks_of(&self.world).len() == 4 {
                break;
            }
            self.command_as(
                P1,
                Command::SpawnUnit {
                    kind: tank,
                    pos: Vec2::from_ints(x, y) + Vec2::new(sf_math::Fx::HALF, sf_math::Fx::HALF),
                    owner: P1,
                },
            );
        }
        let tanks = tanks_of(&self.world);
        assert_eq!(tanks.len(), 4);
        self.command_as(
            P1,
            Command::Order {
                ids: tanks.clone(),
                order: Order::Attack(target),
                queued: false,
            },
        );
        self.run_until(2000, |w| w.hp_of(target).is_some_and(|h| h <= hp));
        for t in tanks {
            self.command_as(P1, Command::Despawn { id: t });
        }
        // Whatever shots were in the air land; then it is quiet.
        self.run(60);
        self.world.hp_of(target).expect("it still stands")
    }
}

#[test]
fn a_hurt_plant_is_repaired_under_its_sign_with_exact_money_up_to_full_hit_points() {
    let look = SidebarLook::embedded();
    let sign = sf_data::repair_look::RepairLook::embedded().sign;
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let cam = scene.camera();
    let plant = scene.id_of("power_plant");
    let hurt = scene.hurt(plant, 300);
    assert!(hurt > 0 && hurt <= 300, "{hurt}");
    let credits = scene.world.credits(P0);
    let at = scene.screen_of(&cam, plant);

    // The Repair mode is on and the cursor is over the plant: the spanner, and the button is lit.
    let snap = SimSnapshot::capture(&scene.world);
    assert_eq!(
        repair_mode::cursor(&snap, ToolMode::Repair, Some(plant), P0),
        CursorKind::Repair
    );
    let barracks = scene.id_of("barracks");
    assert_eq!(
        repair_mode::cursor(&snap, ToolMode::Repair, Some(barracks), P0),
        CursorKind::NoGo,
        "a whole barracks has nothing to repair"
    );
    let plain = scene.shoot(&mut r, &cam, None, Some(plant), None);
    let with_cursor = scene.shoot(
        &mut r,
        &cam,
        Some(ToolMode::Repair),
        Some(plant),
        Some((CursorKind::Repair, at)),
    );
    let t0 = scene.world.tick().0;
    save(
        &format!("repair_mode_cursor_t{t0:05}_hp{hurt:03}.png"),
        &with_cursor,
    );
    let [repair_button, sell_button] = tools();
    assert!(count_in(&with_cursor, repair_button, look.colors.tool_active, 6) > 300);
    assert_eq!(
        count_in(&with_cursor, sell_button, look.colors.tool_active, 6),
        0
    );
    assert_eq!(
        count_in(&plain, repair_button, look.colors.tool_active, 6),
        0
    );
    // The cursor is drawn at the plant: the picture around it differs from the one without.
    let differs = (at.1 as u32 - 16..at.1 as u32 + 32)
        .flat_map(|y| (at.0 as u32 - 16..at.0 as u32 + 32).map(move |x| (x, y)))
        .filter(|&(x, y)| plain.pixel(x, y) != with_cursor.pixel(x, y))
        .count();
    assert!(differs > 20, "{differs}");
    // No sign yet: nothing repairs.
    assert_eq!(count_in(&with_cursor, map_area(), sign.plus, 6), 0);
    save(&format!("hurt_t{t0:05}_hp{hurt:03}.png"), &plain);

    // The click sends the command the sim takes; the plant gains 3 hit points a tick under its sign.
    let command = repair_mode::click(&snap, ToolMode::Repair, Some(plant), P0).unwrap();
    assert_eq!(
        command,
        Command::Repair {
            structure: plant,
            on: true
        }
    );
    scene.command_as(P0, command);
    scene.run(40);
    assert!(scene.world.is_repairing(plant));
    let mid = scene.world.hp_of(plant).unwrap();
    assert_eq!(
        mid,
        hurt + 3 * 40,
        "3 hit points a tick from the tick after the command"
    );
    // The price by hand: 300 credits * 250 / 1000 for a whole repair = h / 8 credits at h hit points.
    assert_eq!(
        credits - scene.world.credits(P0),
        u64::from(mid / 8 - hurt / 8)
    );
    let shot = scene.shoot(&mut r, &cam, Some(ToolMode::Repair), Some(plant), None);
    let t1 = scene.world.tick().0;
    save(&format!("repairing_t{t1:05}_hp{mid:03}.png"), &shot);
    let signs = count_in(&shot, map_area(), sign.plus, 6);
    assert!(signs > 20, "the plus of the sign is on the map: {signs}");

    // To the end: full hit points, the flag gone, the sign gone, the whole cost paid and no more.
    while scene.world.is_repairing(plant) {
        scene.world.step();
    }
    scene.sync();
    assert_eq!(scene.world.hp_of(plant), Some(600));
    assert_eq!(
        credits - scene.world.credits(P0),
        u64::from(600 / 8 - hurt / 8)
    );
    let done = scene.shoot(&mut r, &cam, None, Some(plant), None);
    let t2 = scene.world.tick().0;
    save(&format!("repaired_t{t2:05}_hp600.png"), &done);
    assert_eq!(
        count_in(&done, map_area(), sign.plus, 6),
        0,
        "no sign at full hit points"
    );
    // The plant makes all of its power again from the next count.
    scene.world.step();
    assert_eq!(scene.world.power_of(P0).unwrap().made, 100);
}

#[test]
fn a_sale_shows_its_bar_growing_and_the_structure_is_gone_with_the_refund_at_the_end() {
    let bar = sf_data::repair_look::RepairLook::embedded().sale_bar;
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let cam = scene.camera();
    let barracks = scene.id_of("barracks");
    let credits = scene.world.credits(P0);
    let at = scene.screen_of(&cam, barracks);

    let snap = SimSnapshot::capture(&scene.world);
    assert_eq!(
        repair_mode::cursor(&snap, ToolMode::Sell, Some(barracks), P0),
        CursorKind::Sell
    );
    let cursor = scene.shoot(
        &mut r,
        &cam,
        Some(ToolMode::Sell),
        None,
        Some((CursorKind::Sell, at)),
    );
    let t0 = scene.world.tick().0;
    save(&format!("sell_mode_cursor_t{t0:05}.png"), &cursor);
    assert!(
        count_in(
            &cursor,
            tools()[1],
            SidebarLook::embedded().colors.tool_active,
            6
        ) > 300
    );
    assert_eq!(
        count_in(&cursor, map_area(), bar.fill, 24),
        0,
        "no sale yet"
    );

    let command = repair_mode::click(&snap, ToolMode::Sell, Some(barracks), P0).unwrap();
    assert_eq!(
        command,
        Command::Sell {
            structure: barracks
        }
    );
    scene.command_as(P0, command);
    // 100 ticks in all: 40 ticks in, the bar is two fifths full, 90 ticks in nine tenths.
    scene.run(40);
    assert_eq!(scene.world.sale_of(barracks).map(|s| s.left), Some(60));
    let early = scene.shoot(&mut r, &cam, None, None, None);
    save(
        &format!("selling_t{:05}_40_percent.png", scene.world.tick().0),
        &early,
    );
    scene.run(50);
    assert_eq!(scene.world.sale_of(barracks).map(|s| s.left), Some(10));
    let late = scene.shoot(&mut r, &cam, None, None, None);
    save(
        &format!("selling_t{:05}_90_percent.png", scene.world.tick().0),
        &late,
    );
    let (a, b) = (
        count_in(&early, map_area(), bar.fill, 24),
        count_in(&late, map_area(), bar.fill, 24),
    );
    assert!(a > 40, "the fill of the bar is on the map: {a}");
    // 90 / 40 = 2.25 times as much fill (the bar is as wide in both pictures).
    assert!(b * 10 > a * 20 && b * 10 < a * 26, "{a} then {b}");

    // At the end: gone, the refund paid (400 credits * 500 / 1000 at full hit points) and no bar.
    scene.run(11);
    assert!(scene.world.hp_of(barracks).is_none());
    assert_eq!(scene.world.credits(P0), credits + 200);
    let gone = scene.shoot(&mut r, &cam, None, None, None);
    save(&format!("sold_t{:05}.png", scene.world.tick().0), &gone);
    assert_eq!(count_in(&gone, map_area(), bar.fill, 24), 0);
}
