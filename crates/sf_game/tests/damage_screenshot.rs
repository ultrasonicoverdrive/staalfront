//! The damage display (WP6.8) in a real fight, through the real renderer on a software adapter (Mesa
//! lavapipe / DX12 WARP): a power plant of each faction is shot by three enemy tanks until it is damaged,
//! then heavily damaged, then destroyed; the picture of each stage is the stage model the art set has,
//! with smoke at its `fx_smoke` anchor and (heavily damaged) fire at its `fx_fire` anchor, and when the
//! plant is destroyed the damage effect stops and the wreck's smoke takes over.
//!
//! Everything goes through the production functions: the sandbox world with combat, `runner::motions_of`
//! for the shapes (`Looks::shape_in_state` and `SimSnapshot::damage_stage` inside), `fx_events::record` and
//! `FxPlayer::frame_with` with the instancer's anchors for the effects. No golden image (the two software
//! drivers differ a little). Set `SF_SAVE_SHOTS=<dir>` to write the images
//! (`docs/screenshots/wp6.8/`, named with the stage and the tick).
//!
//! Adapter rules as in `build_screenshot.rs`: a software adapter is preferred; on CI (`CI` set) a missing
//! one is a failure, locally any adapter is accepted.

use std::path::PathBuf;
use std::sync::Arc;

use sf_data::damage::Stage;
use sf_data::map::{Map, generate::generate_cells};
use sf_data::units::UnitDefs;
use sf_game::fx::{DrawnAnchors, FxFrame, FxPlayer};
use sf_game::fx_events::{FxEvent, record};
use sf_game::looks::Looks;
use sf_game::playfield::{make_world, with_buildings, with_combat, with_movement};
use sf_game::population::Population;
use sf_game::roster::{MODELS_FILE, Roster};
use sf_game::runner;
use sf_game::sandbox::{self, MERIDIAN, VEYRAN};
use sf_game::sim_snapshot::SimSnapshot;
use sf_math::Vec2;
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8};
use sf_render::model::EyeView;
use sf_render::pick::project_px;
use sf_render::{
    Blend, Camera, CameraConfig, EffectInstance, FrameView, Frustum, GpuOptions, Ground, Instancer,
    ModelSet, Renderer, Style, TerrainGround,
};
use sf_sim::{Cell, Command, EntityId, Kind, Order, PlayerId, World, WorldConfig};

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

/// The sandbox with combat and the real models, as the game draws it.
struct Scene {
    map: Map,
    ground: Arc<TerrainGround>,
    world: World,
    defs: UnitDefs,
    looks: Looks,
    instancer: Instancer,
    fx: FxPlayer,
    events: Vec<FxEvent>,
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
        let world = with_buildings(world, &defs);
        let world = with_combat(with_movement(world, &defs), &defs);
        assert!(world.destruction_config().is_some(), "the world destroys");
        let mut world = world;
        sandbox::schedule(&mut world, &defs);
        world.step();
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
            fx: FxPlayer::embedded(),
            events: Vec::new(),
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

    fn send(&mut self, owner: u8, command: Command) {
        let tick = self.world.tick();
        self.world.schedule(tick, PlayerId(owner), command).unwrap();
    }

    /// A complete power plant of `owner` on the cell nearest to its base that the building rules allow.
    fn place_plant(&mut self, owner: u8) -> EntityId {
        let kind = self.kind("power_plant");
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
            .expect("a place near the base");
        self.send(
            owner,
            Command::PlaceStructure {
                kind,
                cell,
                owner: PlayerId(owner),
            },
        );
        self.step();
        self.world.occupant_at(cell).expect("it was placed")
    }

    /// Three tanks of `owner` three cells east of `target`, ordered to attack it.
    fn attackers(&mut self, owner: u8, target: EntityId) -> Vec<EntityId> {
        let tank = self.kind("light_tank");
        let class = self
            .world
            .unit_table()
            .get(tank)
            .and_then(|s| s.movement_class)
            .expect("a tank has a movement class");
        let at = self.world.cell_of(target).expect("the target stands");
        // Three free, passable cells five to eight cells from the plant, two apart.
        let mut cells: Vec<Cell> = Vec::new();
        'search: for d in 5..=8u32 {
            for dy in -3..=3i32 {
                for sx in [1i32, -1] {
                    let (x, y) = (at.x as i32 + sx * d as i32, at.y as i32 + dy);
                    if x < 0 || y < 0 || x >= CELLS as i32 || y >= CELLS as i32 {
                        continue;
                    }
                    let cell = Cell::new(x as u32, y as u32);
                    if self.world.passable(class, cell)
                        && !self.world.is_occupied(cell)
                        && cells
                            .iter()
                            .all(|c| c.x.abs_diff(cell.x).max(c.y.abs_diff(cell.y)) >= 2)
                    {
                        cells.push(cell);
                        if cells.len() == 3 {
                            break 'search;
                        }
                    }
                }
            }
        }
        assert_eq!(cells.len(), 3, "room for three tanks near the plant");
        for cell in cells {
            let pos = Vec2::from_ints(cell.x as i32, cell.y as i32)
                + Vec2::new(sf_math::Fx::HALF, sf_math::Fx::HALF);
            self.send(
                owner,
                Command::SpawnUnit {
                    kind: tank,
                    pos,
                    owner: PlayerId(owner),
                },
            );
        }
        self.step();
        let ids: Vec<EntityId> = self
            .world
            .snapshot()
            .iter()
            .filter(|v| v.kind == tank && v.owner == PlayerId(owner))
            .map(|v| v.id)
            .collect();
        assert!(ids.len() >= 3, "the tanks were spawned");
        self.order(owner, &ids, Order::Attack(target));
        ids
    }

    fn order(&mut self, owner: u8, ids: &[EntityId], order: Order) {
        self.send(
            owner,
            Command::Order {
                ids: ids.to_vec(),
                order,
                queued: false,
            },
        );
    }

    /// One tick as the game lives it: the world steps, its events are recorded for the effects, the
    /// instancer takes the snapshot through the game's own function.
    fn step(&mut self) -> SimSnapshot {
        self.world.step();
        record(&self.world, &mut self.events);
        let snap = SimSnapshot::capture(&self.world);
        self.instancer
            .update_tick(runner::motions_of(&self.looks, &snap));
        snap
    }

    fn stage_of(&self, snap: &SimSnapshot, id: EntityId) -> Stage {
        snap.get(id)
            .map_or(Stage::Intact, |v| self.looks.damage_stage(snap, &v))
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

    /// What a frame of the game knows of this camera (as `Runner::build_units` takes it).
    fn zoom_of(&self, cam: &Camera) -> (Blend, FxFrame) {
        let vp = cam.view_proj();
        let eye = EyeView::from_view_proj(vp, H as f32);
        let icons = *self.instancer.icon_config();
        let (distance, max) = (cam.distance() as f32, cam.max_distance() as f32);
        let blend = icons.blend_for_view(eye.focal_px, distance, max, 1.0);
        let band = icons.view_band(eye.focal_px, distance, max, 1.0);
        let frame = FxFrame {
            view_proj: vp,
            focal_px: eye.focal_px,
            viewport: [W as f32, H as f32],
            model_weight: blend.model(),
            reference_px: band.reference_px,
            icon_px: self.instancer.icon_params().size_px,
            dt: 0.05,
            alpha: 1.0,
        };
        (blend, frame)
    }

    /// The effects of the tick just stepped: the recorded events go in, the frame is made with the
    /// instancer's anchors, as `Runner::build_effects` does. Returns the frame's instances.
    fn effects(&mut self, snap: &SimSnapshot, frame: &FxFrame) -> Vec<EffectInstance> {
        self.fx.push_events(&self.events);
        self.events.clear();
        let anchors = DrawnAnchors {
            instancer: &self.instancer,
            ground: &*self.ground,
            first_wreck_slot: sf_game::looks::first_wreck_slot(snap),
        };
        self.fx
            .frame_with(snap, &*self.ground, &anchors, frame)
            .to_vec()
    }

    /// One tick: step, then the effects of that tick at the camera.
    fn tick(&mut self, cam: &Camera) -> (SimSnapshot, Vec<EffectInstance>) {
        let snap = self.step();
        let (_, frame) = self.zoom_of(cam);
        let fx = self.effects(&snap, &frame);
        (snap, fx)
    }

    fn shoot(&mut self, r: &mut Renderer, cam: &Camera, fx: &[EffectInstance]) -> Rgba8 {
        let vp = cam.view_proj();
        r.set_view_proj(vp);
        let (blend, _) = self.zoom_of(cam);
        let frustum = Frustum::from_view_proj(vp);
        let eye = EyeView::from_view_proj(vp, H as f32);
        let ground = &self.ground;
        self.instancer
            .build_frame(&FrameView::new(&frustum, 1.0, eye, blend, 1.0), |x, z| {
                ground.height(f64::from(x), f64::from(z)) as f32
            });
        r.upload_units(&self.instancer);
        r.set_effects(fx);
        r.render_to_image().expect("render")
    }

    /// The screen box of the structure and the air above it, from its footprint corners on the ground
    /// and a point `up` metres above its middle.
    fn crop(&self, cam: &Camera, id: EntityId, up: f32) -> [u32; 4] {
        let v = self.world.get(id).unwrap();
        let fp = self.world.footprint_of(id).unwrap();
        let [cx, cz] = sf_math::render_bridge::vec2_cells_to_metres(v.pos);
        let (hw, hh) = (f32::from(fp.width) * 2.0, f32::from(fp.height) * 2.0);
        let mut min = [f32::MAX; 2];
        let mut max = [f32::MIN; 2];
        for (x, z, up) in [
            (cx - hw, cz - hh, 0.0),
            (cx + hw, cz - hh, 0.0),
            (cx - hw, cz + hh, 0.0),
            (cx + hw, cz + hh, 0.0),
            (cx, cz, up),
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

    /// Where the drawn model's `prefix` anchor is on the screen.
    fn anchor_px(&self, cam: &Camera, id: EntityId, prefix: &str) -> Option<[f32; 2]> {
        let ground = &self.ground;
        let p = self
            .instancer
            .anchor_world(id.index(), prefix, 0, 1.0, |x, z| {
                ground.height(f64::from(x), f64::from(z)) as f32
            })?;
        project_px(&cam.view_proj(), p.into(), [W as f32, H as f32]).map(|p| p.0)
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

/// Pixels inside `rect` that `with` makes clearly brighter than `without`: light from fire (additive glow;
/// over a blue roof it is pink, over a red one orange, so the hue is not tested).
fn glow_pixels(with: &Rgba8, without: &Rgba8, rect: [u32; 4]) -> usize {
    let mut n = 0;
    for y in rect[1]..=rect[3] {
        for x in rect[0]..=rect[2] {
            let (p, q) = (with.pixel(x, y), without.pixel(x, y));
            let gain: i32 = (0..3).map(|i| i32::from(p[i]) - i32::from(q[i])).sum();
            n += usize::from(gain > 120);
        }
    }
    n
}

fn around(p: [f32; 2], half: f32) -> [u32; 4] {
    [
        (p[0] - half).max(0.0) as u32,
        (p[1] - half).max(0.0) as u32,
        ((p[0] + half) as u32).min(W - 1),
        ((p[1] + half) as u32).min(H - 1),
    ]
}

/// Steps until `stage` is reached by `id` (or it is gone). Returns the snapshot.
fn run_to_stage(scene: &mut Scene, cam: &Camera, id: EntityId, stage: Stage) -> SimSnapshot {
    for _ in 0..4000 {
        let (snap, _) = scene.tick(cam);
        if scene.stage_of(&snap, id) >= stage || !snap.is_alive(id) {
            return snap;
        }
    }
    panic!("the plant never reached {stage:?}");
}

#[test]
fn a_structure_shot_up_shows_its_damaged_models_smoke_and_fire() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    for (owner, enemy, faction) in [(MERIDIAN, VEYRAN, "meridian"), (VEYRAN, MERIDIAN, "veyran")] {
        let plant = scene.place_plant(owner);
        let max = scene.world.hp_of(plant).unwrap();
        assert_eq!(max, 600, "the power plant has 600 hit points");
        let v = scene.world.get(plant).unwrap();
        let [fx_m, fz_m] = sf_math::render_bridge::vec2_cells_to_metres(v.pos);
        let cam = scene.camera(0.16, [fx_m, fz_m]);
        let rect = scene.crop(&cam, plant, 20.0);
        let body = scene.crop(&cam, plant, 6.0);
        let (blend, frame) = scene.zoom_of(&cam);
        assert!(
            blend.model() >= 1.0 && frame.reference_px >= 60.0,
            "close zoom"
        );

        // Intact: no smoke, no fire, the finished model.
        let (snap, fx) = scene.tick(&cam);
        assert_eq!(scene.stage_of(&snap, plant), Stage::Intact);
        assert!(fx.is_empty(), "an intact structure gives off nothing");
        let tick = scene.world.tick().0;
        let intact = scene.shoot(&mut r, &cam, &fx);
        save(
            &format!("{faction}_power_plant_intact_tick{tick}.png"),
            &intact,
        );

        // Shot to damaged, then the tanks pulled back so that it stays there while the smoke builds.
        let tanks = scene.attackers(enemy, plant);
        let snap = run_to_stage(&mut scene, &cam, plant, Stage::Damaged);
        let hp = snap.get(plant).unwrap().hp;
        assert_eq!(
            scene.stage_of(&snap, plant),
            Stage::Damaged,
            "hp {hp} of 600"
        );
        // The thresholds of damage.ron as literals: 660 thousandths of 600 is 396, 330 thousandths 198.
        assert!(
            (199..=396).contains(&hp),
            "damaged means 199..=396 of 600, hp {hp}"
        );
        let away = Order::Move(Vec2::new(
            sf_math::Fx::from_int(CELLS as i32 - 4),
            sf_math::Fx::from_int(CELLS as i32 - 4),
        ));
        scene.order(enemy, &tanks, away);
        let mut last = Vec::new();
        for _ in 0..50 {
            let (snap, fx) = scene.tick(&cam);
            assert_eq!(
                scene.stage_of(&snap, plant),
                Stage::Damaged,
                "it holds at damaged"
            );
            last = fx;
        }
        let tick = scene.world.tick().0;
        let damaged_plain = scene.shoot(&mut r, &cam, &[]);
        let damaged = scene.shoot(&mut r, &cam, &last);
        save(
            &format!("{faction}_power_plant_damaged_tick{tick}.png"),
            &damaged,
        );
        // The damaged model is a different picture of the structure, and smoke is on top of it.
        let model = changed(&intact, &damaged_plain, body);
        let smoke = changed(&damaged_plain, &damaged, rect);
        println!("{faction} damaged: model changed {model} px, smoke {smoke} px");
        assert!(
            model > 200,
            "{faction}: the damaged model differs ({model})"
        );
        assert!(smoke > 100, "{faction}: smoke is drawn ({smoke})");
        let c = scene.fx.counters();
        assert!(c.damage_puffs > 0 && c.damage_sources >= 1);
        // Set v4 structures have the anchors (a hurt tank of the sandbox smokes by the rule, so the two
        // kinds of puff are not told apart here; the pictures below are).
        assert!(
            c.damage_puffs_at_anchor > 0,
            "{faction}: puffs at the anchor"
        );
        // Smoke only: no light added to the picture.
        let glow_damaged = glow_pixels(&damaged, &damaged_plain, rect);
        // The smoke is near the anchor of the drawn damaged model.
        let at = scene
            .anchor_px(&cam, plant, "fx_smoke")
            .expect("the model has fx_smoke_0");
        assert!(changed(&damaged_plain, &damaged, around(at, 120.0)) > 50);

        // Shot on to heavily damaged.
        let tanks = scene.attackers(enemy, plant);
        let snap = run_to_stage(&mut scene, &cam, plant, Stage::Heavy);
        let hp = snap.get(plant).unwrap().hp;
        assert_eq!(scene.stage_of(&snap, plant), Stage::Heavy, "hp {hp} of 600");
        assert!(
            (1..=198).contains(&hp),
            "heavily damaged means 1..=198 of 600, hp {hp}"
        );
        scene.order(enemy, &tanks, away);
        let before = scene.fx.counters().damage_puffs;
        for _ in 0..50 {
            let (snap, fx) = scene.tick(&cam);
            assert_eq!(
                scene.stage_of(&snap, plant),
                Stage::Heavy,
                "it holds at heavy"
            );
            last = fx;
        }
        assert!(scene.fx.counters().damage_puffs > before);
        let tick = scene.world.tick().0;
        let heavy_plain = scene.shoot(&mut r, &cam, &[]);
        let heavy = scene.shoot(&mut r, &cam, &last);
        save(
            &format!("{faction}_power_plant_heavy_tick{tick}.png"),
            &heavy,
        );
        let model = changed(&damaged_plain, &heavy_plain, body);
        let smoke = changed(&heavy_plain, &heavy, rect);
        let fire_at = scene
            .anchor_px(&cam, plant, "fx_fire")
            .expect("the model has fx_fire_0");
        let glow = glow_pixels(&heavy, &heavy_plain, around(fire_at, 90.0));
        println!(
            "{faction} heavy: model changed {model} px, smoke and fire {smoke} px, flame px near the fire anchor {glow} (damaged: {glow_damaged})"
        );
        assert!(model > 200, "{faction}: the heavy model differs ({model})");
        assert!(smoke > 150, "{faction}: smoke and fire are drawn ({smoke})");
        assert!(glow > 100, "{faction}: fire at the fire anchor ({glow})");
        assert!(
            glow_damaged < 50,
            "{faction}: damaged is smoke, no light ({glow_damaged})"
        );

        // Destroyed: the damage effect stops (the entity is gone from the snapshot) and the wreck's
        // smoke takes over.
        let tanks = scene.attackers(enemy, plant);
        let mut gone = false;
        for _ in 0..4000 {
            let (snap, _) = scene.tick(&cam);
            if !snap.is_alive(plant) {
                gone = true;
                break;
            }
        }
        assert!(gone, "it was destroyed");
        let made = scene.fx.counters().damage_puffs;
        let spawned = scene.fx.pool_stats().spawned;
        scene.order(enemy, &tanks, away);
        for _ in 0..40 {
            scene.tick(&cam);
        }
        assert!(
            scene.fx.counters().damage_puffs - made <= 60,
            "no new damage puffs once the plant is gone (other damaged objects aside)"
        );
        assert!(
            !scene.world.snapshot().wrecks().next().is_none(),
            "it left rubble"
        );
        assert!(scene.fx.pool_stats().spawned > spawned, "the rubble smokes");
        let tick = scene.world.tick().0;
        let (_, frame) = scene.zoom_of(&cam);
        let snap = SimSnapshot::capture(&scene.world);
        let fx = scene.effects(&snap, &frame);
        let rubble = scene.shoot(&mut r, &cam, &fx);
        save(
            &format!("{faction}_power_plant_destroyed_tick{tick}.png"),
            &rubble,
        );
    }
}
