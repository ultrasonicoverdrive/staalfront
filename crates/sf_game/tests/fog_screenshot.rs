//! Shroud, fog and ghosts (WP8.3, WP8.4) through the real renderer on a software adapter (Mesa
//! lavapipe / DX12 WARP): the start of the first match with the shroud round the base, a fog edge close
//! up, the ghost of an enemy structure the scout has left behind, the minimap with fog, and a world
//! without vision rules that is drawn as it always was.
//!
//! Everything goes through the production functions the runner calls: `SimSnapshot::capture`,
//! `GhostBook::update`, `runner::scene_motions`, `fog_view::sync_terrain` (the fog texture of the
//! terrain), `Minimap::set_fog` with `fog_states` and `minimap_dots`. The checks look at pixels: the
//! shroud is the colour of `fog_look.ron`, a fog cell is the terrain pulled towards the tint by `fog_dim`
//! (worked out here from the picture without fog), the edge between visible and fog goes through
//! in-between values (a soft edge, not a staircase), and a world without vision rules draws the same
//! pixels as a renderer that never heard of fog.
//!
//! Adapter rules as in `build_screenshot.rs`: a software adapter is preferred; on CI (`CI` set) a missing
//! one is a failure, locally any adapter is accepted. Set `SF_SAVE_SHOTS=<dir>` to write the images
//! (`docs/screenshots/wp8.3/`, named with the tick).

use std::path::PathBuf;
use std::sync::Arc;

use sf_data::map::Map;
use sf_data::units::UnitDefs;
use sf_game::fog_view::{self, Dot, GhostBook, minimap_dots, shipped_look};
use sf_game::looks::{GHOST_COLOUR, Looks};
use sf_game::matchplay;
use sf_game::population::Population;
use sf_game::roster::{MODELS_FILE, Roster};
use sf_game::runner::scene_motions;
use sf_game::sim_snapshot::SimSnapshot;
use sf_math::{Fx, Vec2};
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8};
use sf_render::model::EyeView;
use sf_render::pick::project_px;
use sf_render::{
    Blend, Camera, CameraConfig, FrameView, Frustum, GpuOptions, Ground, Instancer, ModelSet,
    Renderer, Style, TerrainGround,
};
use sf_sim::{Cell, Command, PlayerId, Visibility, World, WorldConfig};
use sf_ui::{Minimap, Rect, Ui, UiConfig, UiInput, UiRenderer};

const W: u32 = 960;
const H: u32 = 540;
const ME: PlayerId = PlayerId(0);
const THEM: PlayerId = PlayerId(1);

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

fn rgb(img: &Rgba8, x: u32, y: u32) -> [u8; 3] {
    let p = img.pixel(x, y);
    [p[0], p[1], p[2]]
}

fn lin(c: u8) -> f32 {
    sf_render::fog::srgb_to_linear(c)
}

fn srgb(l: f32) -> u8 {
    let l = l.clamp(0.0, 1.0);
    let c = if l <= 0.003_130_8 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round() as u8
}

/// `a` pulled towards `b` by `t`, in linear light, as sRGB bytes.
fn mix_srgb(a: [u8; 3], b: (u8, u8, u8), t: f32) -> [u8; 3] {
    let m = |a: u8, b: u8| srgb(lin(a) + (lin(b) - lin(a)) * t);
    [m(a[0], b.0), m(a[1], b.1), m(a[2], b.2)]
}

fn close(a: [u8; 3], b: [u8; 3], tol: i32) -> bool {
    (0..3).all(|i| (i32::from(a[i]) - i32::from(b[i])).abs() <= tol)
}

fn brightness(p: [u8; 3]) -> i32 {
    i32::from(p[0]) + i32::from(p[1]) + i32::from(p[2])
}

/// A world on the first map with its models, a renderer, and the presentation state of the runner.
struct Scene {
    map: Map,
    ground: Arc<TerrainGround>,
    world: World,
    looks: Looks,
    instancer: Instancer,
    ghosts: GhostBook,
    applied: Option<u64>,
}

impl Scene {
    fn new(world: World, map: Map) -> Scene {
        let ground = Arc::new(TerrainGround::new(&map));
        let defs = UnitDefs::embedded();
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
            looks: Looks::Roster {
                roster: Box::new(roster),
                crowd: Population::new(None),
            },
            instancer,
            ghosts: GhostBook::new(),
            applied: None,
        }
    }

    /// The first match as the game makes it (`--match`): every rule including vision, the starting
    /// objects of both sides.
    fn first_match() -> Scene {
        let defs = UnitDefs::embedded();
        let def = matchplay::load(&matchplay::file_path(None, &assets())).unwrap();
        let map = Map::load(&matchplay::map_dir(&def, &assets())).unwrap();
        let cells = map.size_in_cells();
        let world = sf_game::playfield::make_world(
            WorldConfig::new(def.seed).with_map_size(cells.0, cells.1),
            &map,
        )
        .unwrap();
        let mut world = matchplay::configure(world, &def, &defs).unwrap();
        matchplay::schedule(&mut world, &def, &defs).unwrap();
        world.step();
        Scene::new(world, map)
    }

    /// A small world on the same map: the local player's scout beside the other side's refinery and
    /// defence tower, no weapons (so the scout lives), with or without vision rules.
    fn scouting(vision: bool) -> Scene {
        let defs = UnitDefs::embedded();
        let map = Map::load(&assets().join("maps/m1")).unwrap();
        let cells = map.size_in_cells();
        let world = sf_game::playfield::make_world(WorldConfig::new(7).with_map_size(cells.0, cells.1), &map)
            .unwrap()
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap();
        let world = sf_game::playfield::with_buildings(world, &defs);
        let world = if vision {
            sf_game::playfield::with_vision(world, &defs)
        } else {
            world
        };
        let mut world = sf_game::playfield::with_movement(world, &defs);
        let kind = |role: &str| sf_sim::Kind(defs.kind_of(role).unwrap());
        let t = world.tick();
        for (owner, role, x, y) in [
            (ME, "construction_center", 12, 68),
            (THEM, "power_plant", 76, 20),
            (THEM, "refinery", 70, 26),
            (THEM, "defence_tower", 66, 22),
        ] {
            world
                .schedule(
                    t,
                    owner,
                    Command::PlaceStructure {
                        kind: kind(role),
                        cell: Cell::new(x, y),
                        owner,
                    },
                )
                .unwrap();
        }
        world
            .schedule(
                t,
                ME,
                Command::SpawnUnit {
                    kind: kind("scout"),
                    pos: Vec2::new(Fx::from_int(64) + Fx::HALF, Fx::from_int(28) + Fx::HALF),
                    owner: ME,
                },
            )
            .unwrap();
        let report = world.step();
        assert_eq!(report.commands_ignored, 0);
        Scene::new(world, map)
    }

    fn run_to(&mut self, tick: u64) {
        while self.world.tick().0 < tick {
            self.world.step();
        }
    }

    fn send_scout(&mut self, x: i32, y: i32) {
        let id = self
            .world
            .snapshot()
            .iter()
            .find(|v| v.owner == ME && v.kind.0 == 15)
            .expect("the scout")
            .id;
        let t = self.world.tick();
        self.world
            .schedule(
                t,
                ME,
                Command::MoveTo {
                    ids: vec![id],
                    target: Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF),
                },
            )
            .unwrap();
    }

    fn camera(&self, zoom: f64, focus: (f64, f64)) -> Camera {
        let cfg = CameraConfig {
            map_size: DVec2::new(f64::from(self.map.width()), f64::from(self.map.height())),
            ..CameraConfig::default()
        };
        let mut c = Camera::new(cfg, W, H);
        c.set_ground(self.ground.clone());
        c.set_zoom(zoom);
        c.set_focus(DVec2::new(focus.0 * 4.0, focus.1 * 4.0));
        c
    }

    /// One frame the way the runner makes it: a snapshot, the ghosts, the motions, the fog of the
    /// terrain (when the version changed), then the draw. `fog` false leaves the terrain's fog alone
    /// (so a test can take the picture without it after the one with it).
    fn shoot_with(&mut self, r: &mut Renderer, cam: &Camera, fog: bool) -> (Rgba8, SimSnapshot) {
        let snap = SimSnapshot::capture(&self.world);
        self.ghosts.update(&snap, &self.looks);
        self.instancer
            .update_tick(scene_motions(&self.looks, &snap, &self.ghosts).collect::<Vec<_>>());
        if fog {
            fog_view::sync_terrain(r, &snap, &mut self.applied);
        }
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
        (r.render_to_image().expect("render"), snap)
    }

    fn shoot(&mut self, r: &mut Renderer, cam: &Camera) -> Rgba8 {
        self.shoot_with(r, cam, true).0
    }

    /// Where the middle of cell `(x, y)` is on the screen.
    fn screen_of(&self, cam: &Camera, x: f32, y: f32) -> Option<(u32, u32)> {
        let (mx, mz) = ((x + 0.5) * 4.0, (y + 0.5) * 4.0);
        let h = self.ground.height(f64::from(mx), f64::from(mz)) as f32;
        let (p, _) = project_px(
            &cam.view_proj(),
            sf_render::glam::Vec3::new(mx, h, mz),
            [W as f32, H as f32],
        )?;
        (p[0] >= 0.0 && p[1] >= 0.0 && p[0] < W as f32 && p[1] < H as f32)
            .then(|| (p[0] as u32, p[1] as u32))
    }
}

fn changed(a: &Rgba8, b: &Rgba8) -> usize {
    (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .filter(|&(x, y)| a.pixel(x, y) != b.pixel(x, y))
        .count()
}

// ---- the start of the first match --------------------------------------------------------------------

#[test]
fn the_start_of_the_first_match_has_shroud_round_the_base_and_a_dimmed_explored_ring() {
    let look = shipped_look();
    let mut scene = Scene::first_match();
    let mut r = renderer(&scene.map);
    // The base at (12, 68) sees ten cells beyond its 4 by 4 footprint; the camera looks at it.
    let cam = scene.camera(0.45, (16.0, 66.0));
    let (with_fog, snap) = scene.shoot_with(&mut r, &cam, true);
    assert!(r.fog_on());
    let tick = snap.tick().0;
    save(&format!("m1_t{tick:05}_start_base.png"), &with_fog);
    // The whole map: the pixel checks below are made on it.
    let cam = scene.camera(0.9, (48.0, 48.0));
    let (with_fog, snap) = scene.shoot_with(&mut r, &cam, true);
    save(&format!("m1_t{tick:05}_start_whole_map.png"), &with_fog);

    // The same view without the fog: the terrain as it is, for the expected colours.
    r.clear_fog();
    let (clean, _) = scene.shoot_with(&mut r, &cam, false);
    assert!(!r.fog_on());
    scene.applied = None;

    // A cell deep in the shroud (far from anything of the local player) is the shroud colour.
    let mut shroud_hits = 0;
    let mut visible_hits = 0;
    for y in (0..96).step_by(2) {
        for x in (0..96).step_by(2) {
            let Some((px, py)) = scene.screen_of(&cam, x as f32, y as f32) else {
                continue;
            };
            let state = snap.visibility(Cell::new(x, y));
            // Only cells whose whole 5 by 5 neighbourhood has the same state: away from every edge.
            let same = (-3..=3).all(|dy: i32| {
                (-3..=3).all(|dx: i32| {
                    let c = Cell::new((x as i32 + dx).max(0) as u32, (y as i32 + dy).max(0) as u32);
                    snap.visibility(c) == state
                })
            });
            if !same {
                continue;
            }
            // A model standing there changes the pixel: compare only where the clean picture and the
            // fogged one are on bare terrain (no unit of the local player within 3 cells).
            let near_object = snap.views().iter().enumerate().any(|(i, v)| {
                let (cx, cy) = v.pos.cell();
                snap.view_shown(i) && (cx - x as i32).abs() <= 6 && (cy - y as i32).abs() <= 6
            });
            if near_object {
                continue;
            }
            let got = rgb(&with_fog, px, py);
            match state {
                Visibility::Shroud => {
                    let want = [look.shroud.0, look.shroud.1, look.shroud.2];
                    assert!(
                        close(got, want, 3),
                        "shroud at {x},{y}: {got:?} wanted about {want:?}"
                    );
                    shroud_hits += 1;
                }
                Visibility::Visible => {
                    assert_eq!(got, rgb(&clean, px, py), "visible at {x},{y} is untouched");
                    visible_hits += 1;
                }
                Visibility::Fog => {
                    let want = mix_srgb(rgb(&clean, px, py), look.fog_tint, look.fog_dim);
                    assert!(
                        close(got, want, 4),
                        "fog at {x},{y}: {got:?} wanted about {want:?} from {:?}",
                        rgb(&clean, px, py)
                    );
                }
            }
        }
    }
    assert!(shroud_hits > 15, "shroud cells checked: {shroud_hits}");
    assert!(visible_hits > 3, "visible cells checked: {visible_hits}");
    // The picture changed a lot: most of the screen is shroud.
    assert!(changed(&with_fog, &clean) > 100_000);
}

#[test]
fn the_edge_between_seen_and_unseen_is_a_ramp_and_not_a_step() {
    let mut scene = Scene::first_match();
    let mut r = renderer(&scene.map);
    // East of the base along row 70: visible, then shroud (nothing was explored beyond the sight).
    let cam = scene.camera(0.2, (26.0, 70.0));
    let (img, snap) = scene.shoot_with(&mut r, &cam, true);
    save(
        &format!("m1_t{:05}_fog_edge_close_up.png", snap.tick().0),
        &img,
    );
    let row = 70;
    let mut last_visible = 0;
    for x in 0..96 {
        if snap.visibility(Cell::new(x, row)) == Visibility::Visible {
            last_visible = x;
        }
    }
    assert!(last_visible > 15, "{last_visible}");
    assert_eq!(
        snap.visibility(Cell::new(last_visible + 1, row)),
        Visibility::Shroud
    );
    // Brightness along the row from five cells inside to five cells outside, one sample per half cell.
    let mut samples: Vec<i32> = Vec::new();
    let mut f = last_visible as f32 - 5.0;
    while f < last_visible as f32 + 6.0 {
        if let Some((px, py)) = scene.screen_of(&cam, f, row as f32) {
            samples.push(brightness(rgb(&img, px, py)));
        }
        f += 0.5;
    }
    assert!(samples.len() > 12, "{} samples", samples.len());
    let (hi, lo) = (
        *samples.iter().max().unwrap(),
        *samples.iter().min().unwrap(),
    );
    assert!(hi - lo > 100, "the edge is there: {hi} down to {lo}");
    // A ramp: no single step between neighbours takes more than a third of the whole drop, and at
    // least five samples sit strictly between the two ends.
    let biggest = samples.windows(2).map(|p| (p[0] - p[1]).abs()).max().unwrap();
    assert!(biggest * 3 < hi - lo, "a step of {biggest} in a drop of {}", hi - lo);
    let between = samples
        .iter()
        .filter(|&&s| s > lo + (hi - lo) / 10 && s < hi - (hi - lo) / 10)
        .count();
    assert!(between >= 5, "{between} samples between: {samples:?}");
}

// ---- ghosts and the minimap ---------------------------------------------------------------------------

#[test]
fn the_ghost_of_the_refinery_the_scout_left_behind_is_drawn_and_goes_when_the_place_is_seen_empty() {
    let mut scene = Scene::scouting(true);
    let mut r = renderer(&scene.map);
    let cam = scene.camera(0.22, (68.0, 25.0));
    let refinery = scene.world.occupant_at(Cell::new(70, 26)).unwrap();

    // The scout is beside it: the refinery is seen and drawn as itself, no ghost.
    scene.run_to(3);
    let (seen, snap) = scene.shoot_with(&mut r, &cam, true);
    assert!(snap.seen(refinery));
    assert_eq!(scene.ghosts.drawn(&snap).count(), 0);
    save(&format!("m1_t{:05}_scout_sees_the_refinery.png", snap.tick().0), &seen);

    // The scout walks home: the cells go to fog and the refinery is hidden; its ghost is drawn.
    scene.send_scout(30, 60);
    scene.run_to(900);
    let (left, snap) = scene.shoot_with(&mut r, &cam, true);
    assert!(!snap.seen(refinery), "the real refinery is hidden");
    assert_eq!(snap.visibility(Cell::new(70, 26)), Visibility::Fog);
    let ghosts: Vec<_> = scene.ghosts.drawn(&snap).map(|g| g.id).collect();
    assert!(ghosts.contains(&refinery), "{ghosts:?}");
    save(&format!("m1_t{:05}_ghost_of_the_refinery.png", snap.tick().0), &left);
    // The ghost is a model in the ghost colour at the structure's place; the picture has changed
    // from "seen" by the fog and the ghost colour (not an empty field).
    let motions: Vec<_> = scene_motions(&scene.looks, &snap, &scene.ghosts).collect();
    assert!(motions.iter().any(|m| m.player == GHOST_COLOUR));
    let ghost_px = scene
        .screen_of(&cam, 71.0, 27.0)
        .expect("the refinery is on the screen");
    let bare = {
        let mut empty = Scene::scouting(true);
        empty.run_to(3);
        let id = empty.world.occupant_at(Cell::new(70, 26)).unwrap();
        let t = empty.world.tick();
        empty.world.schedule(t, THEM, Command::Despawn { id }).unwrap();
        empty.run_to(10);
        empty.send_scout(30, 60);
        empty.run_to(900);
        empty.shoot(&mut r, &cam)
    };
    // Without the ghost the fogged field is plain terrain; with it a model stands there.
    assert!(
        brightness(rgb(&left, ghost_px.0, ghost_px.1)) != brightness(rgb(&bare, ghost_px.0, ghost_px.1)),
        "the ghost model changes the pixels over the refinery"
    );
    r.set_view_proj(cam.view_proj());

    // The minimap: shroud, fog, the ghost dot, no dot for anything hidden.
    let (visible, explored) = snap.fog_words();
    let (w, h) = snap.map_size();
    let states = sf_ui::fog_states(w, h, visible, explored);
    let mut minimap = Minimap::bake(&scene.map, 256);
    let clean = minimap.clean_rgba().to_vec();
    assert!(minimap.set_fog(
        &scene.map,
        Some((&states, fog_view::minimap_fog_of(shipped_look())))
    ));
    assert_ne!(minimap.image().rgba, clean, "the fog changed the image");
    let dots: Vec<_> = minimap_dots(&snap, &scene.ghosts).collect();
    assert_eq!(dots.iter().filter(|d| d.1 == Dot::Ghost).count(), ghosts.len());
    assert!(
        dots.iter().all(|d| matches!(d.1, Dot::Ghost | Dot::Owner(0))),
        "no dot of the other side except ghosts: {dots:?}"
    );
    let mut ui = Ui::new(UiConfig::default());
    ui.begin_frame(UiInput::default(), (W, H), 1.0);
    let frame = Rect::new(W as f32 - 296.0, H as f32 - 296.0, 280.0, 280.0);
    let geom_map = minimap.map_size();
    let dot_color = |d: Dot| match d {
        Dot::Owner(_) => sf_ui::Color(60, 200, 90, 255),
        Dot::Ghost => {
            let g = shipped_look().ghost_dot;
            sf_ui::Color(g.0, g.1, g.2, 255)
        }
    };
    let units = dots.iter().map(|&(p, d)| {
        let [x, z] = sf_math::render_bridge::vec2_cells_to_metres(p);
        ((x, z), dot_color(d))
    });
    ui.minimap(&minimap, "minimap", frame, None, units);
    ui.end_frame();
    let mut ur = UiRenderer::new(&r.gpu().device, r.color_format());
    ur.set_minimap(&r.gpu().device, &r.gpu().queue, minimap.image());
    let with_map = r
        .render_to_image_with(|t| ur.render(t, &mut ui))
        .expect("render");
    assert!(geom_map.0 > 0.0);
    save(&format!("m1_t{:05}_minimap_with_fog.png", snap.tick().0), &with_map);

    // The scout comes back and sees the empty place after the refinery is gone: the ghost is removed.
    let t = scene.world.tick();
    scene
        .world
        .schedule(t, THEM, Command::Despawn { id: refinery })
        .unwrap();
    scene.run_to(scene.world.tick().0 + 3);
    let (still, snap) = scene.shoot_with(&mut r, &cam, true);
    assert_eq!(scene.ghosts.drawn(&snap).count(), ghosts.len(), "nobody looked");
    let _ = still;
    scene.send_scout(66, 27);
    scene.run_to(scene.world.tick().0 + 700);
    let (after, snap) = scene.shoot_with(&mut r, &cam, true);
    assert_eq!(snap.visibility(Cell::new(70, 26)), Visibility::Visible);
    assert!(
        scene.ghosts.get(refinery).is_none(),
        "seen empty: the ghost is gone"
    );
    save(&format!("m1_t{:05}_ghost_gone.png", snap.tick().0), &after);
}

// ---- no vision rules ----------------------------------------------------------------------------------

#[test]
fn a_world_without_vision_rules_draws_the_same_pixels_as_a_renderer_that_never_heard_of_fog() {
    let mut scene = Scene::scouting(false);
    let mut r = renderer(&scene.map);
    let cam = scene.camera(0.3, (66.0, 24.0));
    scene.run_to(3);
    // The path the runner takes: the snapshot has no fog version, so nothing is uploaded or cleared.
    let (plain, snap) = scene.shoot_with(&mut r, &cam, true);
    assert_eq!(snap.fog_version(), None);
    assert!(!r.fog_on(), "no fog without vision rules");
    save(&format!("sandbox_t{:05}_no_fog.png", snap.tick().0), &plain);
    // The same world and view with the fog path never touched.
    let (untouched, _) = scene.shoot_with(&mut r, &cam, false);
    assert_eq!(changed(&plain, &untouched), 0);
    // And a world with rules does change the same view (the comparison can fail).
    let mut with_rules = Scene::scouting(true);
    with_rules.run_to(3);
    let (fogged, _) = with_rules.shoot_with(&mut r, &cam, true);
    assert!(r.fog_on());
    assert!(changed(&plain, &fogged) > 100_000, "fog changes the view");
    // A new scene (a restart on a world without rules) gets a new terrain: the fog is gone with the
    // old one and the picture is the old picture again.
    r.set_terrain(&scene.map);
    scene.applied = None;
    let (back, _) = scene.shoot_with(&mut r, &cam, true);
    assert!(!r.fog_on());
    assert_eq!(changed(&plain, &back), 0, "back to the old picture");
}
