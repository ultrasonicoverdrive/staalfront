//! The computer opponent's base in the game's own picture (WP9.1d), on a software adapter (Mesa lavapipe).
//!
//! The match is `assets/matches/m1_ai.ron` on the game's sim thread (`SimHandle::spawn_with_ais`, stepped
//! by hand, the human idle), so every picture is a snapshot the sim thread published with the AI thinking
//! in it, drawn the way the runner draws one (the models of the roster, `runner::view_motion` per view).
//! The pictures show every view, seen or not (as `match_screenshot` does): in the game window the human
//! sees the AI's base only when its units or structures have seen it (the fog, `vision.md`), which the last
//! check shows with the game's own `SimSnapshot::seen`.
//!
//! No golden image. The checks are on the simulation (the number of structures the AI owns grows as its
//! goal list is walked) and on the pixels (the picture of the base at tick 4000 differs from the one at
//! tick 2). Set `SF_SAVE_SHOTS=<dir>` to write the images with their tick in the name
//! (`docs/screenshots/wp9.1d/`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use sf_data::map::Map;
use sf_data::units::UnitDefs;
use sf_game::looks::Looks;
use sf_game::population::Population;
use sf_game::roster::{MODELS_FILE, Roster};
use sf_game::sim_snapshot::SimSnapshot;
use sf_game::sim_thread::{SimConfig, SimHandle};
use sf_game::{bench_scene, matchplay, playfield};
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8};
use sf_render::model::EyeView;
use sf_render::{
    Blend, Camera, CameraConfig, FrameView, Frustum, GpuOptions, Ground, Instancer, ModelSet,
    Renderer, Style, TerrainGround,
};
use sf_sim::{PlayerId, Tick};

const W: u32 = 960;
const H: u32 = 540;
const AI: PlayerId = PlayerId(1);

fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
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
    let atlas = sf_render::IconAtlas::load(&assets().join("icons"));
    r.set_icon_atlas(&atlas);
    r
}

fn save(name: &str, img: &Rgba8) {
    if let Some(dir) = std::env::var_os("SF_SAVE_SHOTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("shot directory");
        image::write_png(&dir.join(name), img).expect("save shot");
    }
}

struct Pictures {
    map: Map,
    ground: Arc<TerrainGround>,
    looks: Looks,
    instancer: Instancer,
}

impl Pictures {
    fn new() -> Pictures {
        let map = Map::load(&assets().join("maps/m1")).unwrap();
        let ground = Arc::new(TerrainGround::new(&map));
        let defs = UnitDefs::embedded();
        let models_ron = std::fs::read_to_string(assets().join(MODELS_FILE)).unwrap();
        let base = sf_game::looks::with_wreck_colour(Style::embedded().scaled(3.0));
        let atlas = sf_render::IconAtlas::load(&assets().join("icons"));
        let roster = Roster::build(&defs, &base, &atlas, &assets(), &models_ron, true)
            .expect("the roster builds");
        let placeholders = roster.style(&base);
        let pack = roster.pack().expect("the roles use glTF models");
        let style = pack.style_after(&placeholders).unwrap();
        let set = ModelSet::from_style(&placeholders)
            .concat(&pack.set)
            .unwrap();
        let mut instancer = Instancer::new(placeholders, (map.width() as f32, map.height() as f32));
        instancer.set_style_and_models(style, set);
        instancer.set_height_range(ground.min_height() as f32, ground.max_height() as f32);
        Pictures {
            map,
            ground,
            looks: Looks::Roster {
                roster: Box::new(roster),
                crowd: Population::new(None),
            },
            instancer,
        }
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

    /// `snap` drawn as the runner draws a snapshot, every view whether seen or not.
    fn shoot(&mut self, r: &mut Renderer, cam: &Camera, snap: &SimSnapshot) -> Rgba8 {
        let motions: Vec<_> = (0..snap.views().len())
            .map(|i| sf_game::runner::view_motion(&self.looks, snap, i))
            .collect();
        self.instancer.update_tick(motions);
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
}

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

fn structures_of_ai(snap: &SimSnapshot) -> usize {
    (0..snap.views().len())
        .filter(|&i| snap.views()[i].owner == AI && snap.is_structure_at(i))
        .count()
}

#[test]
fn the_ai_builds_its_base_in_the_games_picture() {
    let assets = assets();
    let file = matchplay::file_path(Some(std::path::Path::new("m1_ai")), &assets);
    let def = matchplay::load(&file).unwrap();
    let map = Map::load(&matchplay::map_dir(&def, &assets)).unwrap();
    let cells = map.size_in_cells();
    let defs = UnitDefs::load(&assets.join("config"), &assets).unwrap();
    let world = playfield::make_world(bench_scene::world_config(cells), &map).unwrap();
    let mut world = matchplay::configure(world, &def, &defs).unwrap();
    let ais = matchplay::ais(&def, &defs, &assets).unwrap();
    matchplay::schedule(&mut world, &def, &defs).unwrap();
    world.step();
    let mut sim = SimHandle::spawn_with_ais(world, SimConfig::manual(), ais).unwrap();

    let mut pics = Pictures::new();
    let mut r = renderer(&pics.map);
    let at_ai = pics.camera(0.4, (72.0, 17.0));
    let whole = pics.camera(0.9, (48.0, 48.0));
    let mut counts = Vec::new();
    let mut ai_close = Vec::new();
    for tick in [2u64, 1_000, 2_500, 4_000] {
        let have = sim.latest().tick().0;
        for t in have..tick {
            sim.advance(1);
            sim.wait_for_tick(Tick(t + 1), Duration::from_secs(120))
                .unwrap();
        }
        let snap = sim.latest();
        assert_eq!(snap.tick().0, tick);
        counts.push((tick, structures_of_ai(&snap)));
        let close = pics.shoot(&mut r, &at_ai, &snap);
        save(&format!("m1_ai_t{tick:05}_ai_base.png"), &close);
        if tick == 2 || tick == 4_000 {
            let all = pics.shoot(&mut r, &whole, &snap);
            save(&format!("m1_ai_t{tick:05}_whole_map.png"), &all);
        }
        ai_close.push(close);
        if tick == 4_000 {
            // What the human sees of it: nothing of the AI's base is in the human's sight yet.
            let seen = (0..snap.views().len())
                .filter(|&i| snap.views()[i].owner == AI && snap.seen(snap.views()[i].id))
                .count();
            println!("tick {tick}: the human sees {seen} of the AI's things");
            assert_eq!(seen, 0, "the AI's base is behind the fog for the human");
        }
    }
    println!("structures of the AI by tick: {counts:?}");
    // The construction centre alone at the start, and the goal list walked by tick 4000.
    assert_eq!(counts[0], (2, 1));
    assert!(counts.windows(2).all(|w| w[0].1 <= w[1].1), "{counts:?}");
    assert!(counts[3].1 >= 6, "{counts:?}");
    let diff = changed(&ai_close[0], &ai_close[3]);
    assert!(
        diff > 3_000,
        "the base is in the picture: {diff} pixels differ"
    );
    assert!(!sim.stop().panicked);
}
