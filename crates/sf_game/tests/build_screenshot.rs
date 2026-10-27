//! Screenshot test for the build animation (WP6.3), through the real renderer on a software adapter
//! (Mesa lavapipe / DX12 WARP): a structure of each faction built with `BuildStructure` is drawn as
//! its foundation, its scaffolding and its finished model, picked by the progress the simulation
//! reports in the snapshot, with the progress bar over it while it goes up.
//!
//! The picture follows the state and never leads it: every frame is taken from a `SimSnapshot` of
//! the world at that tick, the stage comes from `Looks::shape_at` with the snapshot's construction,
//! and the checks say the structure's pixels differ from stage to stage, that the bar is there
//! while it goes up and gone the tick it is complete. No golden image (the two software drivers
//! differ a little). Set `SF_SAVE_SHOTS=<dir>` to write the images for a look
//! (`docs/screenshots/wp6.3/`, named with the tick).
//!
//! Adapter rules as in `selection_screenshot.rs`: a software adapter is preferred; on CI (`CI` set)
//! a missing one is a failure, locally any adapter is accepted.

use std::path::PathBuf;
use std::sync::Arc;

use sf_data::map::{Map, generate::generate_cells};
use sf_data::units::UnitDefs;
use sf_game::construction_view;
use sf_game::looks::Looks;
use sf_game::placement_view::GhostFrame;
use sf_game::playfield::{make_world, with_buildings, with_movement};
use sf_game::population::Population;
use sf_game::roster::{MODELS_FILE, Roster};
use sf_game::sandbox::{self, MERIDIAN, VEYRAN};
use sf_game::sim_snapshot::SimSnapshot;
use sf_math::render_bridge::vec2_cells_to_metres;
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8};
use sf_render::model::EyeView;
use sf_render::pick::project_px;
use sf_render::{
    Blend, Camera, CameraConfig, FrameView, Frustum, GpuOptions, Ground, Instancer, ModelSet,
    Motion, Renderer, Style, TerrainGround, UnitId,
};
use sf_sim::{Cell, Command, EntityId, Kind, PlayerId, World, WorldConfig};
use sf_ui::{Ui, UiConfig, UiInput, UiRenderer};

const W: u32 = 960;
const H: u32 = 540;
const CELLS: u32 = 64;

fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn icon_set() -> sf_render::IconAtlas {
    let atlas = sf_render::IconAtlas::load(&assets().join("icons"));
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

/// The sandbox as the game starts it, with the building rules and the real models.
struct Scene {
    map: Map,
    ground: Arc<TerrainGround>,
    world: World,
    defs: UnitDefs,
    looks: Looks,
    instancer: Instancer,
}

impl Scene {
    fn new() -> Self {
        let map = generate_cells(CELLS, CELLS, 1);
        let ground = Arc::new(TerrainGround::new(&map));
        let defs = UnitDefs::embedded();
        let config = WorldConfig::new(1).with_map_size(CELLS, CELLS);
        let world = make_world(config, &map)
            .unwrap()
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap();
        let mut world = with_buildings(with_movement(world, &defs), &defs);
        sandbox::schedule(&mut world, &defs);
        world.step();
        let models_ron = std::fs::read_to_string(assets().join(MODELS_FILE)).unwrap();
        let base = Style::embedded().scaled(3.0);
        let roster = Roster::build(&defs, &base, &icon_set(), &assets(), &models_ron, true)
            .expect("the roster builds");
        assert!(roster.warnings.is_empty(), "{:?}", roster.warnings);
        let placeholders = roster.style(&base);
        let pack = roster.pack().expect("the roles use glTF models");
        let style = pack.style_after(&placeholders).unwrap();
        let set = ModelSet::from_style(&placeholders)
            .concat(&pack.set)
            .unwrap();
        let mut instancer = Instancer::new(placeholders, (map.width() as f32, map.height() as f32));
        instancer.set_style_and_models(style, set);
        instancer.set_height_range(ground.min_height() as f32, ground.max_height() as f32);
        Scene {
            map,
            ground,
            world,
            defs,
            looks: Looks::Roster {
                roster: Box::new(roster),
                crowd: Population::new(None),
            },
            instancer,
        }
    }

    fn kind(&self, role: &str) -> Kind {
        Kind(self.defs.kind_of(role).unwrap())
    }

    fn base_of(&self, owner: u8) -> Cell {
        let anchor = self.kind("construction_center");
        self.world
            .snapshot()
            .iter()
            .find(|v| v.kind == anchor && v.owner == PlayerId(owner))
            .and_then(|v| self.world.cell_of(v.id))
            .expect("the sandbox gives every side an anchor")
    }

    /// Builds `role` for `owner` on the cell nearest to its base that the simulation allows.
    fn build(&mut self, role: &str, owner: u8) -> EntityId {
        let kind = self.kind(role);
        let near = self.base_of(owner);
        let mut cells: Vec<Cell> = (near.y.saturating_sub(14)..=(near.y + 14).min(CELLS - 1))
            .flat_map(|y| {
                (near.x.saturating_sub(14)..=(near.x + 14).min(CELLS - 1))
                    .map(move |x| Cell::new(x, y))
            })
            .collect();
        cells.sort_by_key(|c| (c.x.abs_diff(near.x).max(c.y.abs_diff(near.y)), c.y, c.x));
        let cell = cells
            .into_iter()
            .find(|&c| self.world.check_placement(kind, c, PlayerId(owner)).is_ok())
            .expect("the simulation allows a place near the base");
        let tick = self.world.tick();
        self.world
            .schedule(
                tick,
                PlayerId(owner),
                Command::BuildStructure { kind, cell },
            )
            .unwrap();
        let report = self.world.step();
        assert_eq!(report.commands_executed, 1);
        self.world.occupant_at(cell).expect("it was placed")
    }

    /// Steps until the structure is `permille` thousandths built (or complete when 1000).
    fn run_to(&mut self, id: EntityId, permille: u16) {
        loop {
            match self.world.construction_of(id) {
                Some(c) if c.permille() < permille => {}
                _ => return,
            }
            self.world.step();
        }
    }

    /// The instancer takes the world as the game does: through a snapshot, the shape picked by the
    /// construction in it.
    fn sync(&mut self) -> SimSnapshot {
        let snap = SimSnapshot::capture(&self.world);
        let looks = &self.looks;
        self.instancer.update_tick(snap.views().iter().map(|v| {
            let building = snap.construction_of(v.id).map(|c| c.permille());
            Motion::from_fixed(
                looks.shape_at(v, building),
                v.owner.0,
                v.prev_pos,
                v.pos,
                v.prev_facing,
                v.facing,
            )
            .with_id(UnitId::from_parts(v.id.index(), v.id.generation()))
        }));
        snap
    }

    fn camera(&self, zoom: f64, focus: [f32; 2]) -> Camera {
        let cfg = CameraConfig {
            map_size: DVec2::new(f64::from(self.map.width()), f64::from(self.map.height())),
            ..CameraConfig::default()
        };
        let mut c = Camera::new(cfg, W, H);
        c.set_ground(self.ground.clone());
        c.set_zoom(zoom);
        c.set_focus(DVec2::new(f64::from(focus[0]), f64::from(focus[1])));
        c
    }

    /// One frame as `sf_game` draws it: the units, then the progress bars on the UI's world layer.
    /// Returns the picture and the quads the bars put on the layer.
    fn shoot(&mut self, r: &mut Renderer, cam: &Camera, snap: &SimSnapshot) -> (Rgba8, usize) {
        let vp = cam.view_proj();
        r.set_view_proj(vp);
        let frustum = Frustum::from_view_proj(vp);
        let eye = EyeView::from_view_proj(vp, H as f32);
        let ground = &self.ground;
        self.instancer.build_frame(
            &FrameView::new(&frustum, 0.0, eye, Blend::MODELS, 1.0),
            |x, z| ground.height(f64::from(x), f64::from(z)) as f32,
        );
        let mut ui = Ui::new(UiConfig::default());
        ui.begin_frame(UiInput::default(), (W, H), 1.0);
        ui.end_frame();
        let frame = GhostFrame {
            view_proj: vp,
            viewport: [W as f32, H as f32],
            ui_scale: 1.0,
        };
        let height = |x: f32, z: f32| ground.height(f64::from(x), f64::from(z)) as f32;
        let bar = self.looks.roster().unwrap().stages().bar;
        let mut layer = ui.world_layer();
        construction_view::draw_bars(&mut layer, &frame, &height, snap, &bar);
        let quads = layer.quad_count();
        r.upload_units(&self.instancer);
        let mut ur = UiRenderer::new(&r.gpu().device, r.color_format());
        let img = r
            .render_to_image_with(|t| ur.render(t, &mut ui))
            .expect("render");
        (img, quads)
    }

    /// The screen box of the structure, from its footprint corners on the ground and its height.
    fn crop(&self, cam: &Camera, id: EntityId) -> [u32; 4] {
        let v = self.world.get(id).unwrap();
        let fp = self.world.footprint_of(id).unwrap();
        let [cx, cz] = vec2_cells_to_metres(v.pos);
        let (hw, hh) = (f32::from(fp.width) * 2.0, f32::from(fp.height) * 2.0);
        let mut min = [f32::MAX; 2];
        let mut max = [f32::MIN; 2];
        for (x, z, up) in [
            (cx - hw, cz - hh, 0.0),
            (cx + hw, cz - hh, 0.0),
            (cx - hw, cz + hh, 0.0),
            (cx + hw, cz + hh, 0.0),
            (cx, cz, 8.0),
        ] {
            let y = self.ground.height(f64::from(x), f64::from(z)) as f32 + up;
            let p = project_px(&cam.view_proj(), [x, y, z].into(), [W as f32, H as f32])
                .expect("in front of the camera")
                .0;
            for k in 0..2 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        [
            min[0].max(0.0) as u32,
            min[1].max(0.0) as u32,
            (max[0] as u32).min(W - 1),
            (max[1] as u32).min(H - 1),
        ]
    }
}

/// Pixels that differ visibly between the two pictures inside `rect`.
fn changed(a: &Rgba8, b: &Rgba8, rect: [u32; 4]) -> usize {
    let mut n = 0;
    for y in rect[1]..=rect[3] {
        for x in rect[0]..=rect[2] {
            let (p, q) = (a.pixel(x, y), b.pixel(x, y));
            let d: i32 = (0..3)
                .map(|i| (i32::from(p[i]) - i32::from(q[i])).abs())
                .sum();
            n += usize::from(d > 24);
        }
    }
    n
}

#[test]
fn a_structure_is_drawn_as_foundation_scaffolding_and_finished_for_both_factions() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    for (owner, faction) in [(MERIDIAN, "meridian"), (VEYRAN, "veyran")] {
        let id = scene.build("power_plant", owner);
        let total = scene.world.construction_of(id).unwrap().total;
        let v = scene.world.get(id).unwrap();
        let [fx, fz] = vec2_cells_to_metres(v.pos);
        let cam = scene.camera(0.1, [fx, fz]);

        // The ground without the structure, to see what it covers.
        let mut shots: Vec<(&str, Rgba8, usize, u64)> = Vec::new();
        for (stage, permille) in [
            ("foundation", 100),
            ("scaffolding", 700),
            ("finished", 1000),
        ] {
            scene.run_to(id, permille);
            let tick = scene.world.tick().0;
            let snap = scene.sync();
            if permille == 1000 {
                assert!(scene.world.stands(id), "{faction}: complete at {tick}");
                assert!(snap.construction_of(id).is_none());
            } else {
                let c = snap.construction_of(id).expect("it is going up");
                assert_eq!(c.total, total);
                assert!(c.permille() >= permille);
            }
            let (img, quads) = scene.shoot(&mut r, &cam, &snap);
            save(
                &format!("{faction}_power_plant_{stage}_tick{tick}.png"),
                &img,
            );
            shots.push((stage, img, quads, tick));
        }
        // The bar (a back and a fill quad) while it goes up, none when it is complete.
        assert_eq!(shots[0].2, 2, "{faction}: bar at the foundation");
        assert_eq!(shots[1].2, 2, "{faction}: bar at the scaffolding");
        assert_eq!(shots[2].2, 0, "{faction}: no bar when complete");
        // The picture of each stage differs from the next one over the structure.
        let rect = scene.crop(&cam, id);
        for pair in shots.windows(2) {
            let n = changed(&pair[0].1, &pair[1].1, rect);
            assert!(
                n > 200,
                "{faction}: {} -> {} changed only {n} pixels of the structure",
                pair[0].0,
                pair[1].0
            );
        }
        // Stages follow the ticks: strictly later.
        assert!(shots[0].3 < shots[1].3 && shots[1].3 < shots[2].3);
    }
}

#[test]
fn the_decoy_and_the_bunker_have_their_stages_too() {
    // The two faction structures: each side builds its own, and both have foundation models.
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    // Their prerequisite is the barracks, which the sandbox places complete.
    for (owner, faction, role) in [
        (MERIDIAN, "meridian", "decoy_emplacement"),
        (VEYRAN, "veyran", "fortified_bunker"),
    ] {
        let id = scene.build(role, owner);
        let v = scene.world.get(id).unwrap();
        let [fx, fz] = vec2_cells_to_metres(v.pos);
        let cam = scene.camera(0.1, [fx, fz]);
        scene.run_to(id, 100);
        let snap = scene.sync();
        let (img, quads) = scene.shoot(&mut r, &cam, &snap);
        save(
            &format!(
                "{faction}_{role}_foundation_tick{}.png",
                scene.world.tick().0
            ),
            &img,
        );
        assert_eq!(quads, 2, "{role}: the bar");
        scene.run_to(id, 1000);
        let snap = scene.sync();
        let (done, quads) = scene.shoot(&mut r, &cam, &snap);
        save(
            &format!("{faction}_{role}_finished_tick{}.png", scene.world.tick().0),
            &done,
        );
        assert_eq!(quads, 0);
        assert!(changed(&img, &done, scene.crop(&cam, id)) > 50, "{role}");
    }
}
