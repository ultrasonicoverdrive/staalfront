//! Screenshot test for the per-faction infantry models of art set v3
//! (`models:` in `assets/config/unit_defs.ron`), through the real renderer on a
//! software adapter (Mesa lavapipe / DX12 WARP): Meridian and Veyran
//! infantry side by side at close zoom, drawn with the glTF files the
//! definitions name and the roster picks by owner.
//!
//! No golden image (the two software drivers differ a little). The checks say
//! what has to be true: every unit is drawn (the picture changes where it
//! stands), the two factions' units of one role look different, each carries
//! its own player colour through the `team` material, and a unit that walks
//! east is facing east. Set `SF_SAVE_SHOTS=<dir>` to write the images for a
//! look.
//!
//! Adapter rules as in `sf_render`'s screenshot tests: a software adapter is
//! preferred; on CI (`CI` set) a missing one is a failure, locally any
//! adapter is accepted. No adapter at all fails the test.

use std::path::PathBuf;
use std::sync::Arc;

use sf_data::map::{Map, generate::generate_cells};
use sf_data::units::UnitDefs;
use sf_game::roster::{MODELS_FILE, Roster};
use sf_game::sandbox::{MERIDIAN, NEUTRAL, VEYRAN};
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
use sf_sim::{Command, EntityId, Kind, Order, PlayerId, World, WorldConfig};

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
        image::write_png(&PathBuf::from(dir).join(name), img).expect("save shot");
    }
}

/// The sandbox's world without its sandbox: an empty map the test fills with
/// the units it wants to look at, drawn with the roles' real models.
struct Scene {
    map: Map,
    ground: Arc<TerrainGround>,
    world: World,
    defs: UnitDefs,
    roster: Roster,
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
        let models_ron = std::fs::read_to_string(assets().join(MODELS_FILE)).unwrap();
        let base = Style::embedded().scaled(3.0);
        let roster = Roster::build(&defs, &base, &icon_set(), &assets(), &models_ron, true)
            .expect("the roster builds");
        assert!(roster.warnings.is_empty(), "{:?}", roster.warnings);
        // The roles' models after the placeholders, as the game puts them on
        // the instancer.
        let placeholders = roster.style(&base);
        let pack = roster.pack().expect("the roles use glTF models");
        let style = pack.style_after(&placeholders).unwrap();
        let set = ModelSet::from_style(&placeholders)
            .concat(&pack.set)
            .unwrap();
        let mut instancer = Instancer::new(placeholders, (map.width() as f32, map.height() as f32));
        instancer.set_style_and_models(style.clone(), set);
        instancer.set_height_range(ground.min_height() as f32, ground.max_height() as f32);
        Scene {
            map,
            ground,
            world,
            defs,
            roster,
            style,
            instancer,
        }
    }

    fn kind(&self, role: &str) -> Kind {
        Kind(self.defs.kind_of(role).unwrap())
    }

    /// A unit of `role` for `owner` at a place given in half cells (2 m), made
    /// the way the game does.
    fn spawn(&mut self, role: &str, owner: u8, half_cells: (i32, i32)) -> EntityId {
        let before: Vec<EntityId> = self.world.snapshot().iter().map(|v| v.id).collect();
        let command = Command::SpawnUnit {
            kind: self.kind(role),
            pos: Vec2::from_ints(half_cells.0, half_cells.1).div_int(2),
            owner: PlayerId(owner),
        };
        let tick = self.world.tick();
        self.world.schedule(tick, PlayerId(owner), command).unwrap();
        self.world.step();
        self.world
            .snapshot()
            .iter()
            .map(|v| v.id)
            .find(|id| !before.contains(id))
            .expect("the unit was made")
    }

    /// Everyone walks `cells` to the east, long enough to turn and be on the way.
    fn walk_east(&mut self, ids: &[EntityId], cells: i32) {
        let tick = self.world.tick();
        for &id in ids {
            let at = self.world.get(id).unwrap().pos;
            let target = at + Vec2::from_ints(cells, 0);
            let owner = self.world.get(id).unwrap().owner;
            self.world
                .schedule(
                    tick,
                    owner,
                    Command::Order {
                        ids: vec![id],
                        order: Order::Move(target),
                        queued: false,
                    },
                )
                .unwrap();
        }
        for _ in 0..40 {
            self.world.step();
        }
    }

    /// The instancer takes the world as it is, as the game does each tick;
    /// `with` false draws nothing (the empty ground to compare against).
    fn sync(&mut self, with: bool) {
        let views: Vec<_> = self.world.snapshot().iter().collect();
        let roster = &self.roster;
        let motions = views
            .iter()
            .filter(|_| with)
            .map(|v| {
                let shape = roster
                    .shape_for(v.kind, sf_game::roster::faction_of_owner(v.owner.0))
                    .unwrap_or(0);
                Motion::from_fixed(shape, v.owner.0, v.prev_pos, v.pos, v.prev_facing, v.facing)
                    .with_id(UnitId::from_parts(v.id.index(), v.id.generation()))
            })
            .collect::<Vec<_>>();
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

    /// One frame of `cam`, close enough for the finest models.
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

    /// Where a unit stands, in metres.
    fn metres(&self, id: EntityId) -> [f32; 2] {
        vec2_cells_to_metres(self.world.get(id).unwrap().pos)
    }

    /// The screen position of a unit's ground point.
    fn screen(&self, cam: &Camera, id: EntityId) -> [f32; 2] {
        let [x, z] = self.metres(id);
        let y = self.ground.height(f64::from(x), f64::from(z)) as f32;
        project_px(&cam.view_proj(), [x, y, z].into(), [W as f32, H as f32])
            .expect("in front of the camera")
            .0
    }
}

/// A box of `half` pixels each way (and `above` more up) around a point.
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

/// Pixels of `rect` whose hue is the palette colour of `player`.
fn player_pixels(style: &Style, img: &Rgba8, rect: [u32; 4], player: u8) -> usize {
    let q = style.srgb(player).map(f32::from);
    let qmax = q[0].max(q[1]).max(q[2]);
    let mut n = 0;
    for y in rect[1]..=rect[3] {
        for x in rect[0]..=rect[2] {
            let p = img.pixel(x, y);
            let c = [p[0], p[1], p[2]].map(f32::from);
            let max = c[0].max(c[1]).max(c[2]);
            if max < 30.0 {
                continue;
            }
            let d: f32 = (0..3)
                .map(|k| (c[k] / max - q[k] / qmax).powi(2))
                .sum::<f32>()
                .sqrt();
            n += usize::from(d < 0.12);
        }
    }
    n
}

/// Pixels in the two crops (of equal size) that differ visibly.
fn differ(img: &Rgba8, a: [u32; 4], b: [u32; 4]) -> usize {
    let (w, h) = (
        (a[2] - a[0]).min(b[2] - b[0]),
        (a[3] - a[1]).min(b[3] - b[1]),
    );
    let mut n = 0;
    for y in 0..=h {
        for x in 0..=w {
            let (p, q) = (img.pixel(a[0] + x, a[1] + y), img.pixel(b[0] + x, b[1] + y));
            let d: i32 = (0..3)
                .map(|i| (i32::from(p[i]) - i32::from(q[i])).abs())
                .sum();
            n += usize::from(d > 24);
        }
    }
    n
}

/// A Meridian and a Veyran rifleman side by side, walking east.
#[test]
fn meridian_and_veyran_riflemen_side_by_side_at_close_zoom() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let m = scene.spawn("rifleman", MERIDIAN, (60, 64));
    let v = scene.spawn("rifleman", VEYRAN, (62, 64));
    scene.walk_east(&[m, v], 4);

    // Both turned to the east (+x, facing 0) on the way.
    for id in [m, v] {
        let facing = scene.world.get(id).unwrap().facing.raw();
        assert!(
            facing <= 3 || facing >= 253,
            "{id}: facing {facing}, east is 0"
        );
    }

    let focus = [
        (scene.metres(m)[0] + scene.metres(v)[0]) / 2.0,
        (scene.metres(m)[1] + scene.metres(v)[1]) / 2.0,
    ];
    let cam = scene.camera(0.02, focus);
    scene.sync(false);
    let empty = scene.shoot(&mut r, &cam);
    scene.sync(true);
    let img = scene.shoot(&mut r, &cam);
    save("riflemen_close.png", &img);

    let (pm, pv) = (scene.screen(&cam, m), scene.screen(&cam, v));
    assert!(
        pm[0] > 60.0 && pm[0] < W as f32 - 60.0 && pm[1] > 100.0 && pm[1] < H as f32 - 20.0,
        "the camera sees the Meridian rifleman at {pm:?}"
    );
    assert!(
        pv[0] > 60.0 && pv[0] < W as f32 - 60.0 && pv[1] > 100.0 && pv[1] < H as f32 - 20.0,
        "the camera sees the Veyran rifleman at {pv:?}"
    );
    let (bm, bv) = (crop(pm, 40.0, 90.0), crop(pv, 40.0, 90.0));
    // Both are drawn as figures: the picture changes where they stand.
    let (nm, nv) = (changed(&empty, &img, bm), changed(&empty, &img, bv));
    println!("pixels: meridian {nm}, veyran {nv}");
    assert!(nm > 600 && nv > 600, "meridian {nm}, veyran {nv}");
    // Two models, not one model twice.
    let apart = differ(&img, bm, bv);
    println!("pixels that differ between the two crops: {apart}");
    assert!(apart > 600, "the two riflemen look alike: {apart}");
    // The team material carries each owner's colour, and not the other's.
    let own = |rect, player| player_pixels(&scene.style, &img, rect, player);
    let (mm, mv, vm, vv) = (own(bm, 0), own(bm, 1), own(bv, 0), own(bv, 1));
    println!(
        "team colour pixels: meridian crop red {mm} blue {mv}, veyran crop red {vm} blue {vv}"
    );
    assert!(mm > 40, "Meridian's own colour on its figure: {mm}");
    assert!(vv > 40, "Veyran's own colour on its figure: {vv}");
    assert!(mm > 3 * mv && vv > 3 * vm, "the colours do not swap");
}

/// All the infantry of set v3 at once, a row per role: Meridian left, Veyran
/// right, the neutral civilians and technicians apart. A look at the whole
/// set, with a check that every figure is drawn and upright.
#[test]
fn the_infantry_of_set_v3_stands_upright_in_a_row_per_role() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let mut figures: Vec<(&str, u8, EntityId)> = Vec::new();
    for (row, role) in [
        "rifleman",
        "rocket_trooper",
        "engineer",
        "scout",
        "sniper",
        "medic",
        "combat_engineer",
        "demolition_team",
    ]
    .iter()
    .enumerate()
    {
        let y = 56 + row as i32;
        for (owner, x) in [(MERIDIAN, 58), (VEYRAN, 61)] {
            let def = scene.defs.unit(role).unwrap();
            let faction = sf_game::roster::faction_of_owner(owner).unwrap();
            if def.factions.contains(&faction) {
                figures.push((role, owner, scene.spawn(role, owner, (x, y))));
            }
        }
    }
    for (i, role) in ["civilian", "technician"].iter().enumerate() {
        figures.push((
            role,
            NEUTRAL,
            scene.spawn(role, NEUTRAL, (65, 56 + i as i32)),
        ));
    }
    // Upright, on the ground, a person's height: every infantry model as the
    // roster loaded it (front +X and up +Y were converted once, in the loader).
    let people: Vec<_> = scene
        .roster
        .pack()
        .unwrap()
        .models
        .iter()
        .filter(|m| {
            ["meridian_", "veyran_", "neutral_"]
                .iter()
                .any(|p| m.name.starts_with(p))
        })
        .collect();
    assert_eq!(
        people.len(),
        14,
        "all the infantry of the set is in the roster"
    );
    for model in people {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for part in &model.levels[0].parts {
            for v in &part.mesh.vertices {
                for k in 0..3 {
                    lo[k] = lo[k].min(v.position[k]);
                    hi[k] = hi[k].max(v.position[k]);
                }
            }
        }
        let (width, height, depth) = (hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
        assert!(lo[1].abs() < 0.05, "{}: feet at {}", model.name, lo[1]);
        assert!(
            (1.6..2.0).contains(&height),
            "{}: {height} m tall",
            model.name
        );
        assert!(
            height > width && height > depth,
            "{}: {width} x {height} x {depth} m, lying down",
            model.name
        );
    }
    let ids: Vec<EntityId> = figures.iter().map(|f| f.2).collect();
    scene.walk_east(&ids, 3);
    let n = ids.len() as f32;
    let focus = ids.iter().fold([0.0, 0.0], |sum, &id| {
        let at = scene.metres(id);
        [sum[0] + at[0] / n, sum[1] + at[1] / n]
    });
    let cam = scene.camera(0.03, focus);
    scene.sync(false);
    let empty = scene.shoot(&mut r, &cam);
    scene.sync(true);
    let img = scene.shoot(&mut r, &cam);
    save("infantry_set_v3.png", &img);
    let mut seen = 0;
    for (role, owner, id) in &figures {
        let p = scene.screen(&cam, *id);
        if !(30.0..(W as f32 - 30.0)).contains(&p[0]) || !(60.0..(H as f32 - 5.0)).contains(&p[1]) {
            continue;
        }
        let n = changed(&empty, &img, crop(p, 25.0, 50.0));
        assert!(n > 150, "{role} of {owner}: only {n} pixels drawn at {p:?}");
        seen += 1;
    }
    println!("figures in view: {seen} of {}", figures.len());
    assert!(seen >= 8, "the camera sees most of the set: {seen}");
}
