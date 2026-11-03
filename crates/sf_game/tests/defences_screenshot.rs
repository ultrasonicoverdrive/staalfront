//! Defensive buildings (WP6.7, `docs/rules/defences.md`) through the real renderer on a software adapter
//! (Mesa lavapipe / DX12 WARP): the shipped tower and emplacement drawn with their glTF models, the turret
//! part turning with the facing the simulation gives it.
//!
//! The world is the game's (`sf_game::playfield`: units, buildings, power, combat). Three scenes are *found*
//! in the simulation, not drawn by hand: a tower shoots a tank with its turret turned to it (the picture at
//! the tick of the first shot differs from the tower's before it aimed, in the turret's place); a tower
//! without power stays silent with its turret at rest while the tank stands in its reach; an emplacement
//! turns its slow turret through three ticks (three pictures, three different turret facings). No golden
//! image. Set `SF_SAVE_SHOTS=<dir>` to write the images with their tick in the name
//! (`docs/screenshots/wp6.7/`).
//!
//! Adapter rules as in `sf_render`'s screenshot tests: a software adapter is preferred; on CI (`CI` set) a
//! missing one is a failure, locally any adapter is accepted. No adapter at all fails the test.

use std::path::PathBuf;
use std::sync::Arc;

use sf_data::map::{Map, generate::generate_cells};
use sf_data::units::UnitDefs;
use sf_game::looks::Looks;
use sf_game::population::Population;
use sf_game::roster::{MODELS_FILE, Roster};
use sf_math::Vec2;
use sf_math::render_bridge::vec2_cells_to_metres;
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8};
use sf_render::model::EyeView;
use sf_render::pick::project_px;
use sf_render::{
    Blend, Camera, CameraConfig, FrameView, Frustum, GpuOptions, Ground, Instancer, ModelSet,
    Motion, Renderer, Style, TerrainGround, UnitId,
};
use sf_sim::{Cell, Command, Kind, PlayerId, World, WorldConfig};

const W: u32 = 960;
const H: u32 = 540;
const CELLS: u32 = 64;
const MERIDIAN: u8 = 0;
const VEYRAN: u8 = 1;

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
        image::write_png(&PathBuf::from(dir).join(name), img).expect("save shot");
    }
}

/// The game's world with a small fight in it, and everything to draw it.
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
        let world = World::new(WorldConfig::new(1).with_map_size(CELLS, CELLS))
            .unwrap()
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap();
        let world = sf_game::playfield::with_buildings(world, &defs);
        let world = sf_game::playfield::with_power(world);
        let world = sf_game::playfield::with_combat(world, &defs);
        assert!(world.power_rules().is_some(), "the game's world has power");
        assert!(world.targeting_config().is_some());
        let models_ron = std::fs::read_to_string(assets().join(MODELS_FILE)).unwrap();
        let base = sf_game::looks::with_wreck_colour(Style::embedded().scaled(3.0));
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
        instancer.set_style_and_models(style.clone(), set);
        instancer.set_height_range(ground.min_height() as f32, ground.max_height() as f32);
        let looks = Looks::Roster {
            roster: Box::new(roster),
            crowd: Population::new(None),
        };
        Scene {
            map,
            ground,
            world,
            defs,
            looks,
            instancer,
        }
    }

    fn roster(&self) -> &Roster {
        self.looks.roster().expect("a roster")
    }

    fn kind(&self, role: &str) -> Kind {
        Kind(self.defs.kind_of(role).unwrap())
    }

    /// Puts `role` for `owner` on `cell` (a structure by its corner, a unit in the middle of the cell).
    fn put(&mut self, role: &str, owner: u8, cell: (u32, u32)) {
        let kind = self.kind(role);
        let tick = self.world.tick();
        let command = if self.defs.unit(role).is_some_and(|u| u.footprint.is_some()) {
            Command::PlaceStructure {
                kind,
                cell: Cell::new(cell.0, cell.1),
                owner: PlayerId(owner),
            }
        } else {
            Command::SpawnUnit {
                kind,
                pos: Vec2::from_ints(cell.0 as i32, cell.1 as i32).add_scalar_half(),
                owner: PlayerId(owner),
            }
        };
        self.world.schedule(tick, PlayerId(owner), command).unwrap();
    }

    /// The id of the first entity of `role`.
    fn find(&self, role: &str) -> sf_sim::EntityId {
        let kind = self.kind(role);
        self.world
            .snapshot()
            .iter()
            .find(|v| v.kind == kind)
            .unwrap_or_else(|| panic!("no {role}"))
            .id
    }

    /// The instancer takes the world as it is, as the game does each tick.
    fn sync(&mut self) {
        let views: Vec<_> = self.world.snapshot().iter().collect();
        let wrecks: Vec<_> = self.world.snapshot().wrecks().collect();
        let first = views.last().map_or(0, |v| v.id.index() + 1);
        let mut motions = Vec::new();
        {
            for v in &views {
                let shape = self
                    .roster()
                    .shape_for(v.kind, sf_game::roster::faction_of_owner(v.owner.0))
                    .unwrap_or(0);
                let motion = Motion::from_fixed(
                    shape,
                    v.owner.0,
                    v.prev_pos,
                    v.pos,
                    v.prev_facing,
                    v.facing,
                )
                .with_id(UnitId::from_parts(v.id.index(), v.id.generation()));
                motions.push(match v.turret {
                    Some(t) => motion.with_turret(t.prev_facing, t.facing),
                    None => motion,
                });
            }
        }
        {
            for (n, w) in wrecks.iter().enumerate() {
                motions.extend(self.looks.wreck_motion(w, first + n as u32));
            }
        }
        self.instancer.update_tick(motions);
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

    fn shoot(&mut self, r: &mut Renderer, cam: &Camera) -> Rgba8 {
        let vp = cam.view_proj();
        r.set_view_proj(vp);
        let frustum = Frustum::from_view_proj(vp);
        let eye = EyeView::from_view_proj(vp, H as f32);
        let ground = &self.ground;
        self.instancer.build_frame(
            &FrameView::new(&frustum, 0.0, eye, Blend::MODELS, 1.0),
            |x, z| ground.height(f64::from(x), f64::from(z)) as f32,
        );
        r.upload_units(&self.instancer);
        r.render_to_image().expect("render")
    }

    /// The screen position of a ground point given in cells.
    fn screen(&self, cam: &Camera, at: Vec2) -> [f32; 2] {
        let [x, z] = vec2_cells_to_metres(at);
        let y = self.ground.height(f64::from(x), f64::from(z)) as f32;
        project_px(&cam.view_proj(), [x, y, z].into(), [W as f32, H as f32])
            .expect("in front of the camera")
            .0
    }
}

trait HalfCell {
    fn add_scalar_half(self) -> Vec2;
}

impl HalfCell for Vec2 {
    /// The middle of the cell whose corner this is.
    fn add_scalar_half(self) -> Vec2 {
        self + Vec2::new(sf_math::Fx::HALF, sf_math::Fx::HALF)
    }
}

fn crop(p: [f32; 2], half: f32, above: f32) -> [u32; 4] {
    let x0 = (p[0] - half).max(0.0) as u32;
    let y0 = (p[1] - half - above).max(0.0) as u32;
    let x1 = ((p[0] + half) as u32).min(W - 1);
    let y1 = ((p[1] + half) as u32).min(H - 1);
    [x0, y0, x1, y1]
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

/// Steps to the first tick on which `shooter` fires; returns the tick that ran.
fn until_fired(scene: &mut Scene, shooter: sf_sim::EntityId, limit: u32) -> u64 {
    for _ in 0..limit {
        let report = scene.world.step();
        if scene.world.shots().iter().any(|s| s.attacker == shooter) {
            return report.tick.0;
        }
    }
    panic!("{shooter:?} never fired in {limit} ticks");
}

fn bearing(world: &World, from: sf_sim::EntityId, to: sf_sim::EntityId) -> sf_math::Facing {
    let a = world.get(from).unwrap().pos;
    let b = world.get(to).unwrap().pos;
    (b - a).facing().unwrap()
}

#[test]
fn a_tower_turns_its_turret_to_a_tank_and_shoots() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    scene.put("power_plant", MERIDIAN, (20, 40));
    scene.put("defence_tower", MERIDIAN, (30, 30));
    scene.put("light_tank", VEYRAN, (34, 33));
    scene.world.step();
    let tower = scene.find("defence_tower");
    let tank = scene.find("light_tank");
    let focus = vec2_cells_to_metres(Vec2::from_ints(32, 32));
    let cam = scene.camera(0.22, focus);
    let at = scene.screen(&cam, Vec2::from_ints(30, 30).add_scalar_half());
    let rect = crop(at, 45.0, 45.0);

    // The tick it stands: the turret has hardly come round yet.
    let rest = scene.world.turret_of(tower).unwrap();
    scene.sync();
    let first = scene.shoot(&mut r, &cam);
    save(
        &format!("tower_t{:04}_standing.png", scene.world.tick().0),
        &first,
    );

    until_fired(&mut scene, tower, 100);
    let turret = scene.world.turret_of(tower).unwrap();
    let want = bearing(&scene.world, tower, tank);
    assert!(turret.abs_diff(want) <= 8, "{turret:?} {want:?}");
    assert!(
        turret.abs_diff(rest) > 8,
        "the turret came round: {rest:?} to {turret:?}"
    );
    assert_eq!(scene.world.turret_target_of(tower), Some(tank));
    scene.sync();
    let firing = scene.shoot(&mut r, &cam);
    save(
        &format!("tower_t{:04}_firing.png", scene.world.tick().0),
        &firing,
    );
    let n = changed(&first, &firing, rect);
    println!("tower crop: {n} pixels differ between the tick it stood and the first shot");
    assert!(n > 60, "the turret part turned in the picture: {n}");
}

#[test]
fn a_tower_without_power_is_silent_with_its_turret_at_rest_until_a_plant_stands() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    scene.put("defence_tower", MERIDIAN, (30, 30));
    scene.put("light_tank", VEYRAN, (34, 33));
    scene.world.step();
    scene.world.step();
    let tower = scene.find("defence_tower");
    let tank = scene.find("light_tank");
    let focus = vec2_cells_to_metres(Vec2::from_ints(32, 32));
    let cam = scene.camera(0.22, focus);
    let rest = scene.world.turret_of(tower).unwrap();
    // 120 ticks with the tank in its reach (5 cells) and no plant: 0 power made, 20 used.
    for _ in 0..120 {
        scene.world.step();
        assert!(
            scene.world.shots().iter().all(|s| s.attacker != tower),
            "the tower fired without power at tick {}",
            scene.world.tick().0
        );
    }
    assert!(!scene.world.defences_powered(PlayerId(MERIDIAN)));
    assert_eq!(scene.world.turret_of(tower), Some(rest), "turret at rest");
    assert_eq!(scene.world.turret_target_of(tower), None);
    scene.sync();
    let dark = scene.shoot(&mut r, &cam);
    save(
        &format!("tower_t{:04}_no_power_silent.png", scene.world.tick().0),
        &dark,
    );

    // A plant stands: the speed is full from the next tick and the tower comes round and shoots.
    scene.put("power_plant", MERIDIAN, (20, 40));
    until_fired(&mut scene, tower, 100);
    assert_eq!(scene.world.turret_target_of(tower), Some(tank));
    scene.sync();
    let lit = scene.shoot(&mut r, &cam);
    save(
        &format!("tower_t{:04}_power_back_firing.png", scene.world.tick().0),
        &lit,
    );
    let at = scene.screen(&cam, Vec2::from_ints(30, 30).add_scalar_half());
    let n = changed(&dark, &lit, crop(at, 45.0, 45.0));
    println!("tower crop: {n} pixels differ between silent and firing");
    assert!(n > 60, "{n}");
}

#[test]
fn an_emplacements_slow_turret_turns_through_the_picture() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    scene.put("power_plant", MERIDIAN, (20, 40));
    scene.put("advanced_defences", MERIDIAN, (30, 30));
    // West of the emplacement: the other way round from its rest facing.
    scene.put("light_tank", VEYRAN, (24, 31));
    scene.world.step();
    let empl = scene.find("advanced_defences");
    let focus = vec2_cells_to_metres(Vec2::from_ints(30, 31));
    let cam = scene.camera(0.22, focus);
    let at = scene.screen(&cam, Vec2::from_ints(31, 31));
    let rect = crop(at, 55.0, 55.0);
    let mut shots = Vec::new();
    let mut facings = Vec::new();
    for (k, wait) in [0u32, 4, 5].into_iter().enumerate() {
        for _ in 0..wait {
            scene.world.step();
        }
        let turret = scene.world.turret_of(empl).unwrap();
        facings.push(turret.raw());
        scene.sync();
        let img = scene.shoot(&mut r, &cam);
        save(
            &format!("emplacement_t{:04}_turning_{k}.png", scene.world.tick().0),
            &img,
        );
        shots.push(img);
    }
    println!("emplacement turret facings: {facings:?}");
    // 8 steps a tick, the same way round each time; the three facings differ and so do the pictures.
    assert!(
        facings[0] != facings[1] && facings[1] != facings[2],
        "{facings:?}"
    );
    for pair in [(0, 1), (1, 2)] {
        let n = changed(&shots[pair.0], &shots[pair.1], rect);
        println!("emplacement crop {pair:?}: {n} pixels differ");
        assert!(n > 60, "{pair:?}: {n}");
    }
}
