//! The first match through the real renderer on a software adapter (Mesa lavapipe / DX12 WARP), WP7.6:
//! the start, the fight at the opponent's base and the banner when the match is decided.
//!
//! The world is the headless scenario `match` (`assets/scenarios/match.ron`: the human builds, harvests,
//! makes units and attacks; its checksum is pinned in `tools/sf_headless/tests/match_m1.rs`), drawn the way
//! the runner draws a snapshot: the models of the roster, the credits readout (`money_view::draw_panel`) and,
//! once the match is decided, the banner (`match_view::banner_of` and `draw_banner`, the functions the HUD
//! calls). The states are *found* in the simulation, not assumed: the start is tick 2, the fight the tick
//! the first unit dies, the victory the tick the result is decided.
//!
//! No golden image. The checks look at the pixels: the banner's frame and text colours (from
//! `match_look.ron`) appear when the result is decided and not before, in the colours of the standing of the
//! local player. Set `SF_SAVE_SHOTS=<dir>` to write the images with their tick in the name
//! (`docs/screenshots/wp7.6/`).

use std::path::PathBuf;
use std::sync::Arc;

use sf_data::map::Map;
use sf_data::units::UnitDefs;
use sf_game::looks::Looks;
use sf_game::match_view;
use sf_game::money_view::{self, MoneyHud};
use sf_game::population::Population;
use sf_game::roster::{MODELS_FILE, Roster};
use sf_headless::scenario::Scenario;
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8};
use sf_render::model::EyeView;
use sf_render::{
    Blend, Camera, CameraConfig, FrameView, Frustum, GpuOptions, Ground, Instancer, ModelSet,
    Motion, Renderer, Style, TerrainGround, UnitId,
};
use sf_sim::{PlayerId, Verdict, World};
use sf_ui::{Ui, UiConfig, UiInput, UiRenderer};

const W: u32 = 960;
const H: u32 = 540;
const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

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

/// The scripted match with everything to draw it.
struct Scene {
    map: Map,
    ground: Arc<TerrainGround>,
    world: World,
    looks: Looks,
    instancer: Instancer,
}

impl Scene {
    fn new() -> Scene {
        let scenario = Scenario::load(&assets().join("scenarios/match.ron")).unwrap();
        let world = scenario.build_world().unwrap();
        let map = Map::load(&assets().join("maps/m1")).unwrap();
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
        let looks = Looks::Roster {
            roster: Box::new(roster),
            crowd: Population::new(None),
        };
        Scene {
            map,
            ground,
            world,
            looks,
            instancer,
        }
    }

    /// Steps until the world is at tick `tick`.
    fn run_to(&mut self, tick: u64) {
        while self.world.tick().0 < tick {
            self.world.step();
        }
    }

    /// Steps until `stop` holds after a step; the tick the world is at.
    fn run_until(&mut self, limit: u64, mut stop: impl FnMut(&World) -> bool) -> u64 {
        for _ in 0..limit {
            self.world.step();
            if stop(&self.world) {
                return self.world.tick().0;
            }
        }
        panic!("not reached within {limit} ticks");
    }

    /// The instancer takes the world as it is, as the game does each tick.
    fn sync(&mut self) {
        let roster = self.looks.roster().expect("a roster");
        let views: Vec<_> = self.world.snapshot().iter().collect();
        let motions = views.iter().map(|v| {
            let shape = roster
                .shape_for(v.kind, sf_game::roster::faction_of_owner(v.owner.0))
                .unwrap_or(0);
            let motion =
                Motion::from_fixed(shape, v.owner.0, v.prev_pos, v.pos, v.prev_facing, v.facing)
                    .with_id(UnitId::from_parts(v.id.index(), v.id.generation()));
            match v.turret {
                Some(t) => motion.with_turret(t.prev_facing, t.facing),
                None => motion,
            }
        });
        self.instancer.update_tick(motions.collect::<Vec<_>>());
    }

    /// A camera over the map looking at the cell `focus`; `zoom` as the game's camera has it.
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

    /// One frame as `sf_game` draws it: the models, the credits readout of the local player and, once
    /// the match is decided, the banner for `local`.
    fn shoot(&mut self, r: &mut Renderer, cam: &Camera, local: PlayerId) -> Rgba8 {
        self.sync();
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
        let mut ui = Ui::new(UiConfig::default());
        ui.begin_frame(UiInput::default(), (W, H), 1.0);
        money_view::draw_panel(
            &mut ui,
            &MoneyHud {
                credits: self.world.credits(P0),
                waiting: false,
            },
            money_view::shipped_look(),
        );
        let drawn = match_view::draw_result(&mut ui, self.world.match_result(), local);
        assert_eq!(drawn, self.world.match_result().is_some());
        ui.end_frame();
        let mut ur = UiRenderer::new(&r.gpu().device, r.color_format());
        r.render_to_image_with(|t| ur.render(t, &mut ui))
            .expect("render")
    }
}

/// How many pixels of `img` inside `area` (x0, y0, x1, y1) are within `tol` steps (summed over the
/// channels) of `c`.
fn count_in(img: &Rgba8, area: (u32, u32, u32, u32), c: (u8, u8, u8, u8), tol: i32) -> usize {
    let (r, g, b, _) = c;
    (area.1..area.3.min(img.height))
        .flat_map(|y| (area.0..area.2.min(img.width)).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = img.pixel(x, y);
            (i32::from(p[0]) - i32::from(r)).abs()
                + (i32::from(p[1]) - i32::from(g)).abs()
                + (i32::from(p[2]) - i32::from(b)).abs()
                <= tol
        })
        .count()
}

/// The area of the banner in the window at the UI scale 1: centred, `top_px` from the top.
fn banner_area() -> (u32, u32, u32, u32) {
    let b = match_view::shipped_look().banner;
    let x0 = (W - u32::from(b.width_px)) / 2;
    let y0 = u32::from(b.top_px);
    (
        x0,
        y0,
        x0 + u32::from(b.width_px),
        y0 + u32::from(b.height_px),
    )
}

/// How many pixels differ visibly between the two pictures.
fn changed(a: &Rgba8, b: &Rgba8) -> usize {
    (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let (p, q) = (a.pixel(x, y), b.pixel(x, y));
            (0..3)
                .map(|i| (i32::from(p[i]) - i32::from(q[i])).abs())
                .sum::<i32>()
                > 40
        })
        .count()
}

#[test]
fn the_start_the_fight_and_the_banner() {
    let look = match_view::shipped_look();
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let whole = scene.camera(0.9, (48.0, 48.0));
    let at_base = scene.camera(0.45, (24.0, 66.0));
    let at_rally = scene.camera(0.2, (30.0, 63.0));
    let at_enemy = scene.camera(0.3, (70.0, 24.0));
    let at_fight = scene.camera(0.2, (66.0, 26.0));
    let at_victory = scene.camera(0.22, (76.0, 19.0));

    // The start: the construction centre, nothing built yet, the opponent's base far away.
    scene.run_to(2);
    let start_whole = scene.shoot(&mut r, &whole, P0);
    save("match_t00002_start_whole_map.png", &start_whole);
    let start = scene.shoot(&mut r, &at_base, P0);
    save("match_t00002_start_base.png", &start);
    let enemy_start = scene.shoot(&mut r, &at_enemy, P0);
    save("match_t00002_start_enemy_base.png", &enemy_start);
    assert_eq!(scene.world.match_result(), None);

    // The base built, the army made: the human's units stand at the rally point.
    scene.run_to(2300);
    let army = scene.shoot(&mut r, &at_base, P0);
    save("match_t02300_base_and_harvester.png", &army);
    assert!(
        changed(&start, &army) > 3000,
        "the base and the harvester are there"
    );
    let rally = scene.shoot(&mut r, &at_rally, P0);
    save("match_t02300_army_at_the_rally_point.png", &rally);

    // The fight: the tick the first unit dies, at the opponent's base.
    let fight_tick = scene.run_until(2000, |w| !w.deaths().is_empty());
    scene.run_to(fight_tick + 250);
    let fight = scene.shoot(&mut r, &at_fight, P0);
    save(
        &format!("match_t{:05}_fight_at_the_enemy_base.png", fight_tick + 250),
        &fight,
    );
    // The human's units are among the opponent's: one of them within 8 cells of something of the
    // opponent's.
    let near = {
        let views: Vec<_> = scene.world.snapshot().iter().collect();
        views.iter().filter(|a| a.owner == P0).any(|a| {
            views.iter().filter(|b| b.owner == P1).any(|b| {
                let (dx, dy) = ((a.pos.x - b.pos.x).abs(), (a.pos.y - b.pos.y).abs());
                dx.max(dy) <= sf_math::Fx::from_int(8)
            })
        })
    };
    assert!(near, "the army is at the opponent's base");
    assert!(changed(&enemy_start, &fight) > 3000, "the picture shows it");
    assert_eq!(scene.world.match_result(), None, "nobody has lost yet");
    assert_eq!(
        count_in(&fight, banner_area(), look.victory.border, 30),
        0,
        "no banner before the result"
    );

    // The result: the tick the match is decided; the banner stands in the frame.
    let decided = scene.run_until(4000, |w| w.match_result().is_some());
    let result = scene.world.match_result().unwrap();
    assert_eq!(
        result.tick.0 + 1,
        decided,
        "the step that ran the deciding tick"
    );
    assert_eq!(result.verdict, Verdict::Won(P0));
    let victory = scene.shoot(&mut r, &at_victory, P0);
    save(&format!("match_t{decided:05}_victory_banner.png"), &victory);
    let frame = count_in(&victory, banner_area(), look.victory.border, 30);
    let text = count_in(&victory, banner_area(), look.victory.text, 30);
    assert!(frame > 400, "the banner's frame is drawn: {frame} pixels");
    assert!(text > 150, "the word and the line are drawn: {text} pixels");
    // The banner is in the colours of a victory, not of a defeat or a draw.
    assert_eq!(count_in(&victory, banner_area(), look.defeat.border, 30), 0);
    assert_eq!(count_in(&victory, banner_area(), look.draw.border, 30), 0);
    // And nowhere else in the window.
    let outside = count_in(&victory, (0, 0, W, 90), look.victory.border, 30)
        + count_in(&victory, (0, 220, W, H), look.victory.border, 30);
    assert_eq!(outside, 0, "the banner stands only where the look puts it");

    // The same result for the other player is a defeat, in the colours of one.
    let defeat = scene.shoot(&mut r, &at_victory, P1);
    save(
        &format!("match_t{decided:05}_defeat_banner_for_the_opponent.png"),
        &defeat,
    );
    assert!(count_in(&defeat, banner_area(), look.defeat.border, 30) > 400);
    assert!(count_in(&defeat, banner_area(), look.defeat.text, 30) > 150);
    assert_eq!(count_in(&defeat, banner_area(), look.victory.border, 30), 0);

    // The game keeps running behind the banner and the result stays.
    scene.run_to(decided + 200);
    assert_eq!(scene.world.match_result(), Some(result));
    let later = scene.shoot(&mut r, &at_victory, P0);
    assert!(count_in(&later, banner_area(), look.victory.border, 30) > 400);
}
