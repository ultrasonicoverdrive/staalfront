//! The game shell: glues all crates together. Library of the two binaries
//! `sf_game` (the development viewer) and `sf_demo` (the engine demo and
//! benchmark of milestone M0, `tools/sf_demo`).
//!
//! # Why one library and two binaries (WP1.14)
//!
//! `sf_demo` is a separate binary because PLAN.md and the README name it
//! (`cargo run --release -p sf_demo -- --bench`) and because it is a *fixed*
//! thing: the thing M0 was measured with. `sf_game` will become the real game
//! (phase 2 on: data-driven units, orders, the actual UI) and will stop being
//! a demo. Both need the same window loop, camera, HUD, F1 panels and
//! renderer glue, so that code lives here once and a binary only picks the
//! defaults ([`Preset`]): `sf_game` starts in the sandbox (below), `sf_demo` shows
//! 5,000 wandering units with the test pack's glTF models mixed in and has
//! `--bench`. Nothing is duplicated, and the day `sf_game` grows beyond the demo the shared part
//! can be split off without touching `sf_demo`.
//!
//! # What the window shows
//!
//! Opens the window (sf_engine), draws the sf_render world scene through the
//! game camera every frame and prints the sim tick and frame rates once per
//! second. Camera controls come from `assets/config/input.ron`: wheel = zoom
//! toward the cursor, WASD/arrows/screen edge = pan, middle mouse drag = grab
//! the ground.
//!
//! **Scale (B12).** The sim counts 4 m cells; the map handed to the sim is
//! `Map::size_in_cells()` of a terrain with four 1 m samples per cell side.
//! The camera, the instancer, the minimap and the sound listener work in
//! metres, and cells become metres only through the helpers of
//! `sf_math::render_bridge` (`sf_math::CELL_METRES`): the units in
//! `Motion::from_fixed` and the minimap dots in `hud.rs` both use
//! `vec2_cells_to_metres`. The default map is 256 x 256 cells = 1,024 m
//! (`--map-cells n` for a smaller one, `--map <dir>` for a map directory).
//!
//! **Sandbox (WP3.2).** `sf_game` starts with a few units and structures of each
//! side, spawned from the unit definitions (`assets/config/unit_defs.ron`)
//! through the command queue ([`sandbox`]); each role is drawn with the model
//! or placeholder shape and the strategic icon its definition names, structures
//! at their footprint of 4 m cells ([`roster`], [`looks`]); the F1 panel names the
//! hovered unit and shows its hit points and its order. Units are selected (WP3.3,
//! [`selection`]): click, drag a rectangle, shift to add or toggle, double-click for the
//! same kind in view, control + 1-9 to assign a group, 1-9 to recall it (twice to centre
//! the camera), a click on an enemy to inspect it; the feedback is in [`selection_view`].
//! Selected units take orders (WP3.4, [`commands`]): right-click moves, shift-right-click
//! queues, C stops, X scatters, H holds position (all rebindable actions of
//! `assets/config/input.ron`). The cursor shows what a click will do (WP3.5,
//! [`cursor`]): right-click gives exactly the order of the cursor, so it moves on the
//! ground, attacks an enemy, guards one of your own, attack-moves with F held, and does
//! nothing where the units cannot go.
//! `--crowd` brings back the old scene of 2,000 placeholder wanderers, `--units
//! N` adds N of them to the sandbox, `--models none` draws the roles as shapes.
//! `sf_demo` is always the crowd (it is what the M0 benchmark was measured on).
//!
//! **Units (WP1.6).** An `sf_sim` world with `--units N` wanderers is stepped
//! at 20 Hz and drawn instanced on top of the terrain, positions interpolated
//! between the last two ticks, tile-culled against the camera frustum.
//!
//! **The simulation has a thread of its own (WP1.15).** The world is stepped by
//! [`sim_thread`] with its own 20 Hz clock; the main thread sends it commands
//! and draws from the [`sim_snapshot::SimSnapshot`] it publishes after each
//! tick, so a slow tick (13 to 22 ms for 2,000 colliding units) never stalls a
//! frame. If the sim cannot keep its clock the F1 panel says "sim behind" and
//! the frames carry on (`--sim-delay-ms N` shows it). `docs/architecture.md`
//! section 18 has the thread model. The sim is 2D; the
//! height a unit stands on is looked up in the terrain for display only. Three
//! placeholder shapes (cube, cylinder, cone) in four player colors; with
//! `--models testpack` the glTF vehicles of the test pack join them and its
//! buildings stand still in a base at the centre of the map
//! ([`population`]). `--exit-after <s>` prints a frame-time and instancing
//! summary on exit.
//!
//! **F1** toggles the egui debug panels (WP1.11, [`debug_ui`]): frame time
//! graph, draw calls and instances, live camera tunables, sim tick and
//! checksum, LOD colouring and the 4 m cell grid. `--debug-ui` starts with
//! them open. While hidden they cost nothing. In debug builds
//! `assets/config/units.ron` is hot-reloaded like the input and audio RON.
//!
//! **HUD (WP1.12).** `sf_ui` draws a top bar (fps, units, tick, a button that
//! toggles the unit dots) and the minimap bottom-right ([`hud`]). The UI sees
//! the mouse first: a press or wheel over it never reaches the camera. A
//! click on the terrain plays a sound where it lands.
//!
//! **Model LOD, strategic icons and hover (WP1.9).** Each model has a chain of
//! meshes chosen per unit by projected size (`assets/config/units.ron`); past
//! a zoom threshold the units become screen-fixed icons in player colour,
//! with a dithered cross-fade between the two. The unit under the cursor is
//! picked, highlighted and named in the F1 panel. `--hover x,y` fixes the
//! cursor for screenshots, `--unit-display auto|models|icons` forces the
//! display.
//!
//! **Terrain (WP1.7, WP1.8).** The ground is the engine terrain with levels
//! of detail and culling (`assets/config/terrain.ron`; `--lod-px`,
//! `--lod-level`, `--debug-lod`), and the camera works against the real
//! terrain.
//!
//! # Benchmark and smoke test
//!
//! `--bench` flies a fixed camera path ([`bench`]) in real fullscreen at
//! 1920 x 1080 without vsync and writes `bench.json`; `--shots <dir>` writes
//! three screenshots (close-up, middle, whole map) and exits. Both are what
//! CI runs on the software adapters. See the README, "Benchmark (M0)".

pub mod args;
pub mod bench;
pub mod build_mode;
pub mod cargo_view;
pub mod cell_overlay;
pub mod commands;
pub mod construction_view;
pub mod cursor;
pub mod debug_ui;
pub mod fx;
pub mod fx_events;
pub mod hud;
pub mod looks;
pub mod ore_view;
pub mod placement_view;
pub mod playfield;
pub mod population;
pub mod roster;
pub mod runner;
pub mod sandbox;
pub mod selection;
pub mod selection_view;
pub mod sim_snapshot;
pub mod sim_thread;
mod sound_browser;
pub mod wreck_looks;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sf_engine::EngineConfig;
use sf_render::TerrainConfig;

pub use args::{Options, Preset};

/// Run the game window with the options of this process's command line.
/// Returns the process exit code: 0 after a normal run, 1 when the options,
/// the map or the model pack are unusable, the renderer could not start, a
/// frame failed or a requested file could not be written.
pub fn run(preset: Preset) -> i32 {
    let name = match preset {
        Preset::Game => "sf_game",
        Preset::Demo => "sf_demo",
    };
    let options = Options::from_env(preset);
    for warning in &options.warnings {
        eprintln!("{name}: {warning}");
    }
    let pack = match &options.models {
        Some(pack) => match runner::load_pack(pack) {
            Ok(pack) => Some(pack),
            Err(e) => {
                eprintln!("{name}: --models {pack}: {e}");
                return 1;
            }
        },
        None => None,
    };
    let map = match runner::make_map(&options) {
        Ok(map) => map,
        Err(e) => {
            eprintln!("{name}: {e}");
            return 1;
        }
    };
    let mut lod = TerrainConfig::embedded().lod;
    if let Some(px) = options.lod_px {
        lod.error_px = px;
    }
    lod.force_level = options.lod_level;

    let config = EngineConfig {
        title: format!("Staalfront ({name})"),
        frame_cap_hz: options.frame_cap,
        screen_mode: options.screen,
        ..EngineConfig::default()
    };
    let failed = Arc::new(AtomicBool::new(false));
    let app = match runner::Runner::new(options, map, lod, pack, failed.clone()) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("{name}: {e}");
            return 1;
        }
    };
    if let Err(e) = sf_engine::run(config, app) {
        eprintln!("{name}: {e}");
        return 1;
    }
    i32::from(failed.load(Ordering::Relaxed))
}
