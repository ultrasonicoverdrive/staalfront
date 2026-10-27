//! The effects in a small fight, through the real renderer on a software adapter (Mesa lavapipe / DX12 WARP)
//! (WP5.6a, `docs/rules/effects.md`).
//!
//! The scene is the one of `destruction_screenshot.rs` (the game's world: weapons, units that pick their own
//! targets, destruction; three Meridian light tanks against a Veyran light tank, three riflemen and a relay
//! transmitter, drawn from their models). The world is stepped one tick at a time; after each step the
//! events are recorded as the sim thread does (`fx_events::record`), handed to an `FxPlayer` at each of three
//! zoom levels, and a frame of effects is made. At the step where the first shot, the first hit and the first
//! death happen the picture is taken with the effects and without them, at the same instant:
//!
//! * close zoom: the picture changes, with bright pixels, at the muzzle within two ticks of the shot, at the
//!   impact point and at the death point; the frame is one effect draw call, counted in `DrawStats`, and
//!   no GPU buffer is created for it;
//! * strategic zoom (only icons): the effects of the shot and the hit are gone from the picture, and a death
//!   leaves one flash no bigger than an icon.
//!
//! No golden image (the two software drivers differ a little). Set `SF_SAVE_SHOTS=<dir>` to write the
//! pictures; the ones in `docs/screenshots/wp5.6a/` were made with `sf_game --fight`.
//!
//! Adapter rules as in `sf_render`'s screenshot tests: a software adapter is preferred; on CI (`CI` set) a
//! missing one is a failure, locally any adapter is accepted. No adapter at all fails the test.

use std::path::PathBuf;
use std::sync::Arc;

use sf_data::map::{Map, generate::generate_cells};
use sf_data::units::UnitDefs;
use sf_game::fx::{FxFrame, FxPlayer};
use sf_game::fx_events::{FxEvent, FxKind, record};
use sf_game::looks::Looks;
use sf_game::population::Population;
use sf_game::roster::{MODELS_FILE, Roster};
use sf_game::sim_snapshot::SimSnapshot;
use sf_math::Vec2;
use sf_math::render_bridge::vec2_cells_to_metres;
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8};
use sf_render::model::EyeView;
use sf_render::pick::project_px;
use sf_render::{
    Blend, Camera, CameraConfig, EffectInstance, FrameView, Frustum, GpuOptions, Ground, Instancer,
    ModelSet, Motion, Renderer, Style, TerrainGround, UnitId,
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
        let world = sf_game::playfield::with_combat(world, &defs);
        assert!(
            world.destruction_config().is_some(),
            "the game's world destroys"
        );
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

    /// The scene: three Meridian tanks west, a Veyran tank, three riflemen and a relay transmitter east. They
    /// stand 9 cells apart (the game's units see 4 to 8 cells), so nothing fires on the first tick; the
    /// tanks are sent east on attack-move at tick 2 and the fight takes its course.
    fn cast(&mut self) {
        let tick = self.world.tick();
        let send = |w: &mut World, c: Command| w.schedule(tick, PlayerId(0), c).unwrap();
        let tank = self.kind("light_tank");
        let rifle = self.kind("rifleman");
        let relay = self.kind("relay_transmitter");
        for (i, y) in [28, 31, 34].into_iter().enumerate() {
            let _ = i;
            send(
                &mut self.world,
                Command::SpawnUnit {
                    kind: tank,
                    pos: Vec2::from_ints(24, y).add_scalar_half(),
                    owner: PlayerId(MERIDIAN),
                },
            );
        }
        send(
            &mut self.world,
            Command::SpawnUnit {
                kind: tank,
                pos: Vec2::from_ints(35, 31).add_scalar_half(),
                owner: PlayerId(VEYRAN),
            },
        );
        for y in [29, 31, 33] {
            send(
                &mut self.world,
                Command::SpawnUnit {
                    kind: rifle,
                    pos: Vec2::from_ints(37, y).add_scalar_half(),
                    owner: PlayerId(VEYRAN),
                },
            );
        }
        send(
            &mut self.world,
            Command::PlaceStructure {
                kind: relay,
                cell: Cell::new(39, 31),
                owner: PlayerId(VEYRAN),
            },
        );
        self.world.step();
        let tanks: Vec<_> = self
            .world
            .snapshot()
            .iter()
            .filter(|v| v.kind == tank && v.owner == PlayerId(MERIDIAN))
            .map(|v| v.id)
            .collect();
        assert_eq!(tanks.len(), 3);
        let tick = self.world.tick();
        // Attack-move onto the east group, and one tank straight at the relay transmitter.
        self.world
            .schedule(
                tick,
                PlayerId(MERIDIAN),
                Command::Order {
                    ids: tanks,
                    order: sf_sim::Order::AttackMove(Vec2::from_ints(37, 31)),
                    queued: false,
                },
            )
            .unwrap();
        self.world.step();
    }

    /// The instancer takes the world as it is, as the game does each tick.
    fn sync(&mut self) {
        let views: Vec<_> = self.world.snapshot().iter().collect();
        let wrecks: Vec<_> = self.world.snapshot().wrecks().collect();
        let first = views.last().map_or(0, |v| v.id.index() + 1);
        let mut motions = Vec::new();
        for v in &views {
            let shape = self
                .roster()
                .shape_for(v.kind, sf_game::roster::faction_of_owner(v.owner.0))
                .unwrap_or(0);
            let motion =
                Motion::from_fixed(shape, v.owner.0, v.prev_pos, v.pos, v.prev_facing, v.facing)
                    .with_id(UnitId::from_parts(v.id.index(), v.id.generation()));
            motions.push(match v.turret {
                Some(t) => motion.with_turret(t.prev_facing, t.facing),
                None => motion,
            });
        }

        for (n, w) in wrecks.iter().enumerate() {
            motions.extend(self.looks.wreck_motion(w, first + n as u32));
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

    fn shoot(&mut self, r: &mut Renderer, cam: &Camera, blend: Blend) -> Rgba8 {
        self.shoot_at(r, cam, blend, 0.0)
    }

    /// [`Scene::shoot`] with the units interpolated to `alpha` (1 = the state of the last tick).
    fn shoot_at(&mut self, r: &mut Renderer, cam: &Camera, blend: Blend, alpha: f32) -> Rgba8 {
        let vp = cam.view_proj();
        r.set_view_proj(vp);
        let frustum = Frustum::from_view_proj(vp);
        let eye = EyeView::from_view_proj(vp, H as f32);
        let ground = &self.ground;
        self.instancer
            .build_frame(&FrameView::new(&frustum, alpha, eye, blend, 1.0), |x, z| {
                ground.height(f64::from(x), f64::from(z)) as f32
            });
        r.upload_units(&self.instancer);
        r.render_to_image().expect("render")
    }

    /// What a frame of the game knows of this camera: the cross-fade of the instancer's icon settings, as
    /// `Runner::build_units` takes it.
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

/// Pixels of `with` inside `rect` that are clearly brighter than in `base` (an additive flash, not a shadow).
fn brighter(base: &Rgba8, with: &Rgba8, rect: [u32; 4]) -> usize {
    let mut n = 0;
    for y in rect[1]..=rect[3] {
        for x in rect[0]..=rect[2] {
            let lum = |p: [u8; 4]| i32::from(p[0]) + i32::from(p[1]) + i32::from(p[2]);
            n += usize::from(lum(with.pixel(x, y)) - lum(base.pixel(x, y)) > 90);
        }
    }
    n
}

/// A camera at three zooms on the fight, with the effects of its own.
struct Eye {
    name: &'static str,
    cam: Camera,
    fx: FxPlayer,
}

/// The ground point (metres) where the first flash of a `Fire` event starts: out along the mount's facing
/// from the middle of the shooter by the muzzle rule of `effects.ron`.
fn muzzle_of(ev: &FxEvent) -> [f32; 3] {
    let m = sf_data::effects::Effects::embedded().muzzle;
    let reach = (m.reach_fraction * ev.size).max(m.min_reach_m);
    let (s, c) = (f32::from(ev.facing) * (std::f32::consts::TAU / 256.0)).sin_cos();
    [
        ev.pos[0] + c * reach,
        m.height_fraction * ev.size,
        ev.pos[1] + s * reach,
    ]
}

#[test]
fn shots_hits_and_deaths_are_drawn_close_up_and_fade_to_a_flash_from_afar() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    scene.cast();
    let focus = vec2_cells_to_metres(Vec2::from_ints(33, 31));
    let mut eyes = [
        Eye {
            name: "close",
            cam: scene.camera(0.10, focus),
            fx: FxPlayer::embedded(),
        },
        Eye {
            name: "middle",
            cam: scene.camera(0.45, focus),
            fx: FxPlayer::embedded(),
        },
        Eye {
            name: "strategic",
            cam: scene.camera(1.0, focus),
            fx: FxPlayer::embedded(),
        },
    ];
    // The three views are what they are named: models up close, the cross-fade or small models in the
    // middle, only icons from afar.
    let weights: Vec<f32> = eyes
        .iter()
        .map(|e| scene.zoom_of(&e.cam).1.model_weight)
        .collect();
    for (eye, w) in eyes.iter().zip(&weights) {
        println!("{}: model weight {w}", eye.name);
    }
    assert_eq!(weights[0], 1.0, "close: models only");
    assert!(weights[1] > 0.0, "middle: models are still drawn");
    assert_eq!(weights[2], 0.0, "strategic: icons only");
    let middle_ref = scene.zoom_of(&eyes[1].cam).1.reference_px;
    let close_ref = scene.zoom_of(&eyes[0].cam).1.reference_px;
    assert!(
        middle_ref < sf_data::effects::Effects::embedded().limits.middle_below_px
            || weights[1] < 1.0,
        "middle band"
    );
    assert!(
        close_ref >= sf_data::effects::Effects::embedded().limits.middle_below_px,
        "close band: {close_ref}"
    );

    let mut checked_fire = false;
    let mut checked_hit = false;
    let mut checked_death = false;
    let mut new_events = Vec::new();
    for _ in 0..3000 {
        scene.world.step();
        new_events.clear();
        record(&scene.world, &mut new_events);
        let snap = SimSnapshot::capture(&scene.world);
        let mut frames: Vec<Vec<EffectInstance>> = Vec::new();
        for eye in &mut eyes {
            eye.fx.push_events(&new_events);
            let (_, f) = scene.zoom_of(&eye.cam);
            frames.push(eye.fx.frame(&snap, &*scene.ground, &f).to_vec());
        }
        scene.sync();
        let tick = scene.world.tick().0;

        // --- the first shot, at the muzzle, within two ticks
        let fire = new_events
            .iter()
            .find(|e| matches!(e.kind, FxKind::Fire { .. }));
        if let (Some(ev), false) = (fire, checked_fire) {
            checked_fire = true;
            let m = muzzle_of(ev);
            let (_, f) = scene.zoom_of(&eyes[0].cam);
            let at = project_px(&f.view_proj, m.into(), [W as f32, H as f32])
                .expect("in view")
                .0;
            let rect = crop(at, 36.0, 36.0);
            // The same instant, without and with the effects.
            r.set_effects(&[]);
            let (blend, _) = scene.zoom_of(&eyes[0].cam);
            let base = scene.shoot(&mut r, &eyes[0].cam, blend);
            assert_eq!(r.draw_stats().effect_draw_calls, 0);
            let allocs = r.alloc_stats();
            assert_eq!(r.set_effects(&frames[0]), 0, "the frame fits the buffer");
            let with = scene.shoot(&mut r, &eyes[0].cam, blend);
            save(&format!("fx_t{tick:04}_close_fire.png"), &with);
            let stats = r.draw_stats();
            assert_eq!(stats.effect_draw_calls, 1, "one draw call for every effect");
            assert_eq!(stats.effect_instances as usize, frames[0].len());
            assert!(
                frames[0].len() >= 2,
                "a flash is several particles close up"
            );
            assert_eq!(
                r.alloc_stats(),
                allocs,
                "drawing the effects created no buffer"
            );
            let n = changed(&base, &with, rect);
            let b = brighter(&base, &with, rect);
            println!(
                "{}: tick {tick}: shot of weapon {} at {at:?}: {n} pixels changed, {b} brighter",
                eyes[0].name, ev.weapon
            );
            assert!(n > 40, "the muzzle flash changes the picture: {n}");
            assert!(b > 15, "and it is bright: {b}");
            // From afar the same instant shows nothing of it: only icons, so no effect instance at all.
            assert!(
                frames[2].is_empty(),
                "strategic: a shot draws nothing, got {}",
                frames[2].len()
            );
            let (blend_far, f_far) = scene.zoom_of(&eyes[2].cam);
            assert_eq!(f_far.model_weight, 0.0);
            r.set_effects(&[]);
            let far_base = scene.shoot(&mut r, &eyes[2].cam, blend_far);
            r.set_effects(&frames[2]);
            let far = scene.shoot(&mut r, &eyes[2].cam, blend_far);
            assert_eq!(
                changed(&far_base, &far, [0, 0, W - 1, H - 1]),
                0,
                "the picture from afar is the same with and without the effects of a shot"
            );
            // Two ticks later the flash is still there (it lives longer than that).
            save(&format!("fx_t{tick:04}_strategic_fire.png"), &far);
        }

        // --- the first hit, where it lands
        let impact = new_events
            .iter()
            .find(|e| matches!(e.kind, FxKind::Impact { .. }));
        if let (Some(ev), false) = (impact, checked_hit) {
            checked_hit = true;
            let m = [ev.pos[0], 0.6, ev.pos[1]];
            let (blend, f) = scene.zoom_of(&eyes[0].cam);
            let at = project_px(&f.view_proj, m.into(), [W as f32, H as f32])
                .expect("in view")
                .0;
            let rect = crop(at, 36.0, 36.0);
            r.set_effects(&[]);
            let base = scene.shoot(&mut r, &eyes[0].cam, blend);
            r.set_effects(&frames[0]);
            let with = scene.shoot(&mut r, &eyes[0].cam, blend);
            save(&format!("fx_t{tick:04}_close_hit.png"), &with);
            let n = changed(&base, &with, rect);
            println!("tick {tick}: hit at {at:?}: {n} pixels changed");
            assert!(n > 30, "the hit changes the picture where it lands: {n}");
        }

        // --- the first death: a blast close up, one small flash from afar
        let died = new_events
            .iter()
            .find(|e| matches!(e.kind, FxKind::Death { .. }));
        if let (Some(ev), false) = (died, checked_death) {
            checked_death = true;
            let m = [ev.pos[0], 0.6, ev.pos[1]];
            let (blend, f) = scene.zoom_of(&eyes[0].cam);
            let at = project_px(&f.view_proj, m.into(), [W as f32, H as f32])
                .expect("in view")
                .0;
            let rect = crop(at, 60.0, 60.0);
            r.set_effects(&[]);
            let base = scene.shoot(&mut r, &eyes[0].cam, blend);
            r.set_effects(&frames[0]);
            let with = scene.shoot(&mut r, &eyes[0].cam, blend);
            save(&format!("fx_t{tick:04}_close_death.png"), &with);
            let n = changed(&base, &with, rect);
            let b = brighter(&base, &with, rect);
            println!(
                "tick {tick}: death at {at:?}: {n} pixels changed, {b} brighter, {} instances",
                frames[0].len()
            );
            assert!(n > 200, "an explosion is a big change: {n}");
            assert!(b > 60, "and it is bright: {b}");

            // The middle zoom has fewer particles than the close one for the same event.
            assert!(
                frames[1].len() < frames[0].len(),
                "fewer particles at middle zoom: {} against {}",
                frames[1].len(),
                frames[0].len()
            );
            // Strategic: one flash, no bigger than an icon, visible against the icons-only picture.
            assert_eq!(frames[2].len(), 1, "one flash from afar");
            let (blend_far, f_far) = scene.zoom_of(&eyes[2].cam);
            assert!(frames[2][0].size_px <= f_far.icon_px);
            let far_at = project_px(&f_far.view_proj, m.into(), [W as f32, H as f32])
                .expect("in view")
                .0;
            r.set_effects(&[]);
            let far_base = scene.shoot(&mut r, &eyes[2].cam, blend_far);
            r.set_effects(&frames[2]);
            let far = scene.shoot(&mut r, &eyes[2].cam, blend_far);
            save(&format!("fx_t{tick:04}_strategic_death.png"), &far);
            let rect = crop(far_at, 14.0, 14.0);
            let n = changed(&far_base, &far, rect);
            let all = changed(&far_base, &far, [0, 0, W - 1, H - 1]);
            println!(
                "tick {tick}: death flash from afar at {far_at:?}: {n} pixels near it, {all} in all"
            );
            assert!(n > 4, "the flash is visible from afar: {n}");
            let side = (f_far.icon_px as usize) + 3;
            assert!(
                all <= side * side,
                "and no bigger than an icon: {all} pixels in all"
            );
            // Middle zoom picture for the record.
            let (blend_mid, _) = scene.zoom_of(&eyes[1].cam);
            r.set_effects(&frames[1]);
            let mid = scene.shoot(&mut r, &eyes[1].cam, blend_mid);
            save(&format!("fx_t{tick:04}_middle_death.png"), &mid);
        }
        if checked_fire && checked_hit && checked_death {
            break;
        }
    }
    assert!(
        checked_fire && checked_hit && checked_death,
        "the fight had a shot, a hit and a death"
    );
    // The pool saw the whole fight and was never pushed out.
    let stats = eyes[0].fx.pool_stats();
    assert!(stats.spawned > 10 && stats.dropped == 0, "{stats:?}");
}

/// WP5.6b: the flash starts at the barrel tip of the drawn model. A Veyran heavy tank (set v4, a twin cannon with
/// two muzzles) stands with its hull facing east and a Meridian light tank four cells north of it, so its turret
/// must turn about 90 degrees off the hull to fire. Every shot of the heavy tank is followed through the
/// real path (`record`, `FxPlayer::frame_with`, `DrawnAnchors` on the instancer) and checked against the
/// instancer's own answer for the muzzle it should come from; the picture (lavapipe) shows a bright change
/// there. The first two shots of a burst come from different muzzles.
#[test]
fn a_flash_starts_at_the_barrel_tip_and_a_twin_cannon_alternates_its_muzzles() {
    use sf_game::fx::DrawnAnchors;
    use sf_game::fx_events::NO_SHOOTER;
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let (heavy, light) = (scene.kind("heavy_tank"), scene.kind("light_tank"));
    let tick = scene.world.tick();
    let spawn = |w: &mut World, kind, at: (i32, i32), owner| {
        w.schedule(
            tick,
            PlayerId(owner),
            Command::SpawnUnit {
                kind,
                pos: Vec2::from_ints(at.0, at.1).add_scalar_half(),
                owner: PlayerId(owner),
            },
        )
        .unwrap()
    };
    spawn(&mut scene.world, heavy, (30, 30), VEYRAN);
    spawn(&mut scene.world, light, (30, 26), MERIDIAN);
    scene.world.step();
    let tank = scene
        .world
        .snapshot()
        .iter()
        .find(|v| v.kind == heavy)
        .expect("the heavy tank")
        .id;
    let focus = vec2_cells_to_metres(Vec2::from_ints(30, 29));
    let cam = scene.camera(0.06, focus);
    let (blend, f) = scene.zoom_of(&cam);
    assert_eq!(f.model_weight, 1.0, "models only");
    let mut fx = FxPlayer::embedded();
    let ground = scene.ground.clone();
    // (tick, the instances of the frame, the pool instances this shot made, the shot's muzzle number)
    let mut shots: Vec<(u64, Vec<EffectInstance>, usize, usize)> = Vec::new();
    let mut events = Vec::new();
    for _ in 0..400 {
        scene.world.step();
        events.clear();
        record(&scene.world, &mut events);
        let snap = SimSnapshot::capture(&scene.world);
        scene.sync();
        let mine: Vec<FxEvent> = events
            .iter()
            .filter(|e| matches!(e.kind, FxKind::Fire { .. }) && e.shooter == tank.index())
            .copied()
            .collect();
        assert!(
            events
                .iter()
                .all(|e| e.shooter == NO_SHOOTER || matches!(e.kind, FxKind::Fire { .. }))
        );
        let view = scene.world.get(tank).expect("alive");
        let off = view
            .turret
            .map(|t| t.facing.signed_diff(view.facing))
            .unwrap_or(0);
        // Only the heavy tank's own shots, so the particles counted below are all its flash and smoke (the
        // light tank shoots back from four cells away).
        fx.push_events(&mine);
        let before = fx.pool_stats().spawned;
        let anchors = DrawnAnchors {
            instancer: &scene.instancer,
            ground: &*ground,
            first_wreck_slot: sf_game::looks::first_wreck_slot(&snap),
        };
        let inst = fx.frame_with(&snap, &*ground, &anchors, &f).to_vec();
        if mine.is_empty() {
            continue;
        }
        let made = (fx.pool_stats().spawned - before) as usize;
        let dots = fx.counters().tracer_dots as usize;
        println!(
            "tick {}: heavy tank fires, turret {off} steps off the hull, {made} particles",
            snap.tick().0
        );
        assert!(
            off.unsigned_abs() >= 40,
            "the turret is turned off the hull: {off}"
        );
        shots.push((
            snap.tick().0,
            inst[..inst.len() - dots].to_vec(),
            made,
            shots.len(),
        ));
        if shots.len() == 2 {
            break;
        }
    }
    assert_eq!(shots.len(), 2, "the twin cannon fired a burst of two");
    // One tank, one weapon slot: the n-th shot comes from muzzle n.
    let mut centres = Vec::new();
    for (tick, inst, made, n) in &shots {
        let want = scene
            .instancer
            .anchor_world(tank.index(), "muzzle", *n, 1.0, |x, z| {
                ground.height(f64::from(x), f64::from(z)) as f32
            })
            .expect("the heavy tank has its muzzle anchors");
        let fresh = &inst[inst.len() - made..];
        let near =
            |p: &[f32; 3], q: [f32; 3]| ((p[0] - q[0]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
        let worst = fresh
            .iter()
            .map(|i| near(&i.position, want))
            .fold(0.0, f32::max);
        println!(
            "tick {tick}: shot {n}: {made} new particles within {worst:.3} m of the anchor {want:?}"
        );
        assert!(
            worst < 0.3,
            "shot {n}: a particle {worst} m from its muzzle"
        );
        centres.push((want, fresh.to_vec()));
    }
    // The two muzzles are the two ends of the 0.912 m wide barrel mouth, one turret turn apart from rest.
    let apart = ((centres[0].0[0] - centres[1].0[0]).powi(2)
        + (centres[0].0[2] - centres[1].0[2]).powi(2))
    .sqrt();
    assert!(
        (apart - 0.912).abs() < 0.05,
        "the two muzzles are {apart} m apart"
    );
    // The pictures: the same instant with and without the effects, and the change at the muzzle.
    for (tick, inst, _, n) in &shots {
        let want = centres[*n].0;
        let at = project_px(&f.view_proj, want.into(), [W as f32, H as f32])
            .expect("in view")
            .0;
        let rect = crop(at, 30.0, 30.0);
        r.set_effects(&[]);
        let base = scene.shoot_at(&mut r, &cam, blend, 1.0);
        assert_eq!(r.set_effects(inst), 0);
        let with = scene.shoot_at(&mut r, &cam, blend, 1.0);
        save(&format!("anchor_t{tick:04}_heavy_muzzle{n}.png"), &with);
        let (c, b) = (changed(&base, &with, rect), brighter(&base, &with, rect));
        println!("tick {tick}: muzzle {n} at {at:?}: {c} pixels changed, {b} brighter");
        assert!(c > 40 && b > 15, "{c} changed, {b} brighter");
    }
}

/// A light tank (one `muzzle` node) with its turret turned: the flash is at the barrel, ahead of the turret and
/// not at the hull's rule point.
#[test]
fn a_single_muzzle_tank_with_its_turret_turned_flashes_at_the_barrel() {
    use sf_game::fx::DrawnAnchors;
    let mut scene = Scene::new();
    let light = scene.kind("light_tank");
    let tick = scene.world.tick();
    for (at, owner) in [((30, 30), VEYRAN), ((26, 30), MERIDIAN)] {
        scene
            .world
            .schedule(
                tick,
                PlayerId(owner),
                Command::SpawnUnit {
                    kind: light,
                    pos: Vec2::from_ints(at.0, at.1).add_scalar_half(),
                    owner: PlayerId(owner),
                },
            )
            .unwrap();
    }
    scene.world.step();
    let tank = scene
        .world
        .snapshot()
        .iter()
        .find(|v| v.owner == PlayerId(VEYRAN))
        .unwrap()
        .id;
    let ground = scene.ground.clone();
    let mut r = renderer(&scene.map);
    let cam = scene.camera(0.06, vec2_cells_to_metres(Vec2::from_ints(29, 30)));
    let (blend, f) = scene.zoom_of(&cam);
    let mut fx = FxPlayer::embedded();
    let mut events = Vec::new();
    for _ in 0..400 {
        scene.world.step();
        events.clear();
        record(&scene.world, &mut events);
        let snap = SimSnapshot::capture(&scene.world);
        scene.sync();
        let Some(ev) = events
            .iter()
            .find(|e| matches!(e.kind, FxKind::Fire { .. }) && e.shooter == tank.index())
            .copied()
        else {
            continue;
        };
        fx.push_events(&[ev]);
        let anchors = DrawnAnchors {
            instancer: &scene.instancer,
            ground: &*ground,
            first_wreck_slot: sf_game::looks::first_wreck_slot(&snap),
        };
        let inst = fx.frame_with(&snap, &*ground, &anchors, &f).to_vec();
        let want = scene
            .instancer
            .anchor_world(tank.index(), "muzzle", 0, 1.0, |x, z| {
                ground.height(f64::from(x), f64::from(z)) as f32
            })
            .expect("the light tank has a muzzle");
        let rule = muzzle_of(&ev);
        let flat =
            |p: [f32; 3], q: [f32; 3]| ((p[0] - q[0]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
        let nearest = inst
            .iter()
            .map(|i| flat(i.position, want))
            .fold(f32::MAX, f32::min);
        println!(
            "tick {}: anchor {want:?}, the rule's point {rule:?}, nearest particle {nearest:.3} m from the anchor",
            snap.tick().0
        );
        assert!(nearest < 0.3, "{nearest}");
        assert_eq!(fx.counters().flashes_at_anchor, 1);
        // The barrel tip is well off the rule's point (it is 2.5 m higher): the rule would start the flash in
        // the hull.
        let apart = ((want[0] - rule[0]).powi(2)
            + (want[1] - rule[1]).powi(2)
            + (want[2] - rule[2]).powi(2))
        .sqrt();
        assert!(apart > 1.0, "{want:?} vs {rule:?}: {apart} m");
        // The picture: a bright change at the barrel tip.
        let at = project_px(&f.view_proj, want.into(), [W as f32, H as f32])
            .expect("in view")
            .0;
        r.set_effects(&[]);
        let base = scene.shoot_at(&mut r, &cam, blend, 1.0);
        assert_eq!(r.set_effects(&inst), 0);
        let with = scene.shoot_at(&mut r, &cam, blend, 1.0);
        save(
            &format!("anchor_t{:04}_light_muzzle.png", snap.tick().0),
            &with,
        );
        let rect = crop(at, 30.0, 30.0);
        let (c, b) = (changed(&base, &with, rect), brighter(&base, &with, rect));
        println!("light tank: {c} pixels changed, {b} brighter at the barrel tip {at:?}");
        assert!(c > 30 && b > 10, "{c} changed, {b} brighter");
        return;
    }
    panic!("the light tank never fired");
}

/// Relative cost of the effects: not a test that can fail, a measurement (`--ignored --nocapture`, release
/// build). The CPU part is the pool's update and emit for a full pool and 300 projectiles. The GPU part is the
/// software rasterizer's time for a whole frame (`render_to_image`, readback included) with no effects, with a
/// typical load and with the largest frame the buffer takes at the largest size on the screen, alternating
/// runs. A software rasterizer is not the reference GPU: only the ratios mean anything, and the real number is
/// the laptop's benchmark M0c (after WP5.6b).
#[test]
#[ignore = "a measurement, run by hand: cargo test --release -p sf_game --test effects_screenshot -- --ignored --nocapture"]
fn what_the_effects_cost() {
    use sf_render::effects::{Burst, BurstDef, EmitView, ParticlePool, tracer_dots};
    use std::time::Instant;
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    scene.cast();
    for _ in 0..60 {
        scene.world.step();
    }
    scene.sync();
    let focus = vec2_cells_to_metres(Vec2::from_ints(33, 31));
    let cam = scene.camera(0.10, focus);
    let (blend, f) = scene.zoom_of(&cam);
    let effects = sf_data::effects::Effects::embedded();
    let limits = &effects.limits;

    // --- CPU: a full pool in view, and the tracers of 300 projectiles
    let mut pool = ParticlePool::new(limits);
    let def = BurstDef {
        life_s: 1000.0,
        size_m: 3.0,
        growth_mps: 0.0,
        colour: (255, 180, 80),
        alpha: 0.5,
        glow: true,
        rise_mps: 0.0,
        spread_mps: 0.0,
    };
    for i in 0..limits.max_particles {
        let (a, b) = ((i % 64) as f32, (i / 64) as f32);
        pool.spawn(&Burst {
            position: [focus[0] - 30.0 + a, 1.0, focus[1] - 30.0 + b],
            radius_m: 0.0,
            count: 1,
            def,
            flash: false,
        });
    }
    let view = EmitView {
        view_proj: f.view_proj,
        focal_px: f.focal_px,
        viewport: f.viewport,
        model_weight: 1.0,
        icon_px: f.icon_px,
        limits,
    };
    let mut out = Vec::with_capacity((limits.max_particles + limits.max_tracer_dots) as usize);
    let runs = 200;
    let began = Instant::now();
    let mut emitted = 0;
    for _ in 0..runs {
        pool.update(0.016);
        emitted = pool.emit(&view, &mut out);
        for k in 0..300 {
            let at = [
                focus[0] - 20.0 + (k % 30) as f32,
                1.0,
                focus[1] - 20.0 + (k / 30) as f32,
            ];
            tracer_dots(
                &effects.tracers.straight,
                at,
                [1.0, 0.0, 0.0],
                &view,
                2048,
                &mut out,
            );
        }
    }
    let per_frame = began.elapsed() / runs;
    println!(
        "CPU: update + emit of {} particles in view + 300 tracers: {:?} per frame ({} instances)",
        emitted,
        per_frame,
        out.len()
    );

    // --- CPU: the whole of `FxPlayer::frame` with an empty pool and no events (what a quiet frame costs; the
    // snapshot has the shells that are in flight at that tick, 8 tracer dots)
    let mut idle = FxPlayer::embedded();
    let snap = SimSnapshot::capture(&scene.world);
    let ground = &*scene.ground;
    let began = Instant::now();
    let mut n = 0;
    for _ in 0..100_000 {
        n += idle.frame(&snap, ground, &f).len();
    }
    println!(
        "CPU: FxPlayer::frame, empty pool, no events: {:?} per frame ({} tracer dots each)",
        began.elapsed() / 100_000,
        n / 100_000
    );

    // --- software GPU: whole frames, alternating
    let mut typical = Vec::new();
    let mut worst = Vec::new();
    for i in 0..500_u32 {
        let (a, b) = ((i % 25) as f32, (i / 25) as f32);
        typical.push(EffectInstance {
            position: [focus[0] - 12.0 + a, 1.5, focus[1] - 8.0 + b],
            size_px: 12.0 + (i % 20) as f32,
            color: [0.4, 0.25, 0.1, 0.0],
        });
    }
    for i in 0..(limits.max_particles + limits.max_tracer_dots) {
        let (a, b) = ((i % 80) as f32, (i / 80) as f32);
        worst.push(EffectInstance {
            position: [focus[0] - 20.0 + a * 0.5, 1.5, focus[1] - 20.0 + b * 0.5],
            size_px: limits.max_px,
            color: [0.1, 0.06, 0.03, 0.0],
        });
    }
    let mut times = [Vec::new(), Vec::new(), Vec::new()];
    let sets: [&[EffectInstance]; 3] = [&[], &typical, &worst];
    // One warm-up frame each, then alternating.
    for set in sets {
        r.set_effects(set);
        scene.shoot(&mut r, &cam, blend);
    }
    for _ in 0..12 {
        for (i, set) in sets.iter().enumerate() {
            r.set_effects(set);
            let began = Instant::now();
            scene.shoot(&mut r, &cam, blend);
            times[i].push(began.elapsed().as_secs_f64() * 1e3);
            if i > 0 {
                assert_eq!(r.draw_stats().effect_draw_calls, 1);
            }
        }
    }
    let median = |v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    let [mut none, mut typ, mut wor] = times;
    let (n, t, w) = (median(&mut none), median(&mut typ), median(&mut wor));
    println!(
        "software rasterizer, whole frame at {W}x{H}, median of 12 alternating runs: no effects {n:.1} ms, \
         500 instances of 12-31 px {t:.1} ms (+{:.1} ms), {} instances of {} px {w:.1} ms (+{:.1} ms)",
        t - n,
        worst.len(),
        limits.max_px,
        w - n
    );
}
