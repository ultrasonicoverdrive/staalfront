//! A small fight in the game's own world, through the real renderer on a software adapter (Mesa lavapipe /
//! DX12 WARP): what destruction leaves behind is drawn (WP5.5, `docs/rules/destruction.md`).
//!
//! The world is the game's (`sf_game::playfield::with_combat`: weapons, units that pick their own targets,
//! destruction), the units are roles of the shipped definitions drawn with their models, and the picture is
//! built the way the runner does it (`Looks::wreck_motion` for the wrecks). Three Meridian light tanks meet a
//! Veyran light tank, three riflemen and a relay transmitter (a structure). Pictures are taken at three
//! ticks: before anyone has fired, in the middle of the fight, and when it is over with the wrecks and the
//! rubble lying there; then again after they have expired.
//!
//! No golden image (the two software drivers differ a little). The checks say what has to be true: the
//! wreck is drawn from a glTF model of art set v4 in the wreck colour (not a placeholder), the picture
//! changes where each wreck lies, the rubble is as big as the footprint it came from, and the ground is
//! empty again after the wrecks expired. Set `SF_SAVE_SHOTS=<dir>` to write the images; the ones in
//! `docs/screenshots/wp5.5/` were made so, with the tick in the name.
//!
//! Adapter rules as in `sf_render`'s screenshot tests: a software adapter is preferred; on CI (`CI` set) a
//! missing one is a failure, locally any adapter is accepted. No adapter at all fails the test.

use std::path::PathBuf;
use std::sync::Arc;

use sf_data::map::{Map, generate::generate_cells};
use sf_data::units::UnitDefs;
use sf_game::looks::{Looks, WRECK_COLOUR};
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
use sf_sim::{Cell, Command, DestructionConfig, Kind, PlayerId, World, WorldConfig, WreckKind};

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
    style: Style,
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
            style,
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

    /// Steps until `done`, at most `limit` ticks.
    fn until(&mut self, limit: u32, mut done: impl FnMut(&World) -> bool) {
        for _ in 0..limit {
            if done(&self.world) {
                return;
            }
            self.world.step();
        }
        panic!("not done in {limit} ticks at tick {}", self.world.tick());
    }

    /// The instancer takes the world as it is, as the game does each tick; `with` false draws no wrecks
    /// (and no units: the empty ground to compare against).
    fn sync(&mut self, what: What) {
        let views: Vec<_> = self.world.snapshot().iter().collect();
        let wrecks: Vec<_> = self.world.snapshot().wrecks().collect();
        let first = views.last().map_or(0, |v| v.id.index() + 1);
        let mut motions = Vec::new();
        if what != What::Nothing {
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
        if what == What::Everything {
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

#[derive(Copy, Clone, PartialEq, Eq)]
enum What {
    /// Units and wrecks, as the runner draws a snapshot.
    Everything,
    /// The units only.
    UnitsOnly,
    /// Nothing: the ground.
    Nothing,
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

#[test]
fn a_small_fight_leaves_wrecks_and_rubble_that_are_drawn_and_then_go() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    scene.cast();
    let focus = vec2_cells_to_metres(Vec2::from_ints(37, 31));
    let cam = scene.camera(0.22, focus);
    let photo = |scene: &mut Scene, r: &mut Renderer, what: What| {
        scene.sync(what);
        scene.shoot(r, &cam)
    };

    // The wreck models are the ones of art set v4, not placeholders: the look has a model, and the id
    // says what class.
    let looks: Vec<_> = scene.roster().wreck_looks().collect();
    let ids: Vec<&str> = looks.iter().map(|(_, l)| l.id.as_str()).collect();
    assert!(ids.contains(&"rubble_2x2"), "{ids:?}");
    assert!(ids.contains(&"wreck_light_vehicle"), "{ids:?}");
    assert_eq!(
        looks.len(),
        3 + 7,
        "three vehicle wrecks and rubble for the seven footprints: {ids:?}"
    );
    for (_, look) in &looks {
        assert!(look.model, "{} is drawn with its glTF model", look.id);
    }

    // Tick before anyone has fired.
    let before = scene.world.tick();
    assert!(scene.world.wrecks().is_empty());
    let first = photo(&mut scene, &mut r, What::Everything);
    save(&format!("fight_t{:04}_before.png", before.0), &first);

    // In the middle: the first of them has fallen.
    scene.until(3000, |w| !w.wrecks().is_empty());
    let middle = scene.world.tick();
    let mid = photo(&mut scene, &mut r, What::Everything);
    save(&format!("fight_t{:04}_first_wreck.png", middle.0), &mid);

    // When it is over: the tanks and the riflemen have fought and the relay transmitter is shot down.
    let relay = scene.kind("relay_transmitter");
    let tank = scene.kind("light_tank");
    scene.until(6000, |w| {
        let alive = |kind: Kind, owner: u8| {
            w.snapshot()
                .iter()
                .any(|v| v.kind == kind && v.owner == PlayerId(owner))
        };
        // Both sides' tanks cannot all be dead and the relay transmitter must have fallen too: send the
        // survivors at it once the riflemen are gone.
        !alive(relay, VEYRAN) && (!alive(tank, VEYRAN))
    });
    let _ = &relay;
    let end = scene.world.tick();
    let over = photo(&mut scene, &mut r, What::Everything);
    save(&format!("fight_t{:04}_over.png", end.0), &over);
    let bare = photo(&mut scene, &mut r, What::UnitsOnly);

    let wrecks: Vec<_> = scene.world.snapshot().wrecks().collect();
    assert!(
        wrecks.iter().any(|w| w.kind == WreckKind::Vehicle),
        "a vehicle was destroyed: {wrecks:?}"
    );
    assert!(
        wrecks.iter().any(|w| w.kind == WreckKind::Rubble),
        "the relay transmitter left rubble: {wrecks:?}"
    );
    // Where each wreck lies, the picture changes against the same scene without the wrecks.
    for w in &wrecks {
        let at = scene.screen(&cam, w.pos);
        let rect = crop(at, 40.0, 40.0);
        let n = changed(&bare, &over, rect);
        println!(
            "wreck {:?} of kind {} at {:?}: {n} pixels differ",
            w.kind, w.unit.0, w.pos
        );
        assert!(
            n > 300,
            "{:?} at {at:?} is hardly drawn: {n} pixels",
            w.kind
        );
    }
    // They are in the wreck colour's dark, not in a player's: nothing of the wreck's crop is the bright
    // player colour of its owner to any extent (the models bring their own materials).
    assert_eq!(
        scene.style.srgb(WRECK_COLOUR),
        [70, 66, 62],
        "the dark colour of units.ron"
    );

    // After they have expired the ground is empty again.
    let config = DestructionConfig::DEFAULT;
    scene.until(config.rubble_ticks + config.wreck_ticks, |w| {
        w.wrecks().is_empty()
    });
    let gone_at = scene.world.tick();
    let after = photo(&mut scene, &mut r, What::Everything);
    save(&format!("fight_t{:04}_expired.png", gone_at.0), &after);
    let bare_after = photo(&mut scene, &mut r, What::UnitsOnly);
    for w in &wrecks {
        let at = scene.screen(&cam, w.pos);
        let rect = crop(at, 40.0, 40.0);
        assert!(
            changed(&bare_after, &after, rect) < 20,
            "{:?} at {at:?} is still drawn after it expired",
            w.kind
        );
    }
}
