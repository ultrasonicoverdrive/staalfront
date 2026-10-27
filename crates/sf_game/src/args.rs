//! Command line of `sf_game` and `sf_demo`.
//!
//! One parser for both binaries; they differ only in the [`Preset`] whose
//! defaults apply. Unknown flags are ignored (the old behaviour), a flag whose
//! value does not parse falls back to the default with a note on stderr.

use std::path::PathBuf;
use std::time::Duration;

use sf_engine::ScreenMode;
use sf_render::Present;

use crate::debug_ui::UnitDisplay;

/// What the world starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    /// A crowd of wandering placeholder entities (the engine demo and its
    /// benchmark, and `sf_game --crowd`): kinds are opaque numbers that
    /// [`crate::population`] maps to shapes.
    Crowd,
    /// The sandbox (WP3.2): a few units and structures of each side from the
    /// unit definitions, spawned on the playfield ([`crate::sandbox`]).
    Sandbox,
}

/// Which binary is running: sets the defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// `sf_game`: the development viewer, the sandbox.
    Game,
    /// `sf_demo`: the engine demo of milestone M0, 5,000 units, the test
    /// pack's models in the mix, benchmark mode.
    Demo,
}

/// Placeholder units of the dev viewer's `--crowd` scene.
pub const GAME_UNITS: u32 = 2000;
/// Units of the engine demo (PLAN.md WP1.14).
pub const DEMO_UNITS: u32 = 5000;
/// The demo's map side in gameplay cells (1,024 m at 4 m per cell, B12).
pub const DEFAULT_MAP_CELLS: u32 = 256;
/// The model pack of the demo (`assets/config/models.ron`).
pub const DEMO_PACK: &str = "testpack";
/// Length of the benchmark flight in seconds.
pub const DEFAULT_BENCH_SECONDS: f64 = 75.0;
/// The reference resolution of the benchmark (PLAN.md B10).
pub const BENCH_RESOLUTION: (u32, u32) = (1920, 1080);

/// `--bench`: the scripted camera flight with frame-time measurement.
#[derive(Clone, Debug, PartialEq)]
pub struct BenchOptions {
    /// Length of the flight in seconds (time based unless `frames` is set).
    pub seconds: f64,
    /// Fly the whole path in exactly this many frames instead of this many
    /// seconds (CI smoke test, `--frames 300`).
    pub frames: Option<u32>,
    /// Where the JSON goes.
    pub out: PathBuf,
}

/// Everything the command line decides.
#[derive(Clone, Debug)]
pub struct Options {
    pub preset: Preset,
    /// What the world starts with.
    pub scene: Scene,
    /// Placeholder wanderers: the crowd of the crowd scene, extra ones on top
    /// of the sandbox (none by default).
    pub units: u32,
    /// Model pack name for the crowd scene, `None` = placeholder shapes only.
    pub models: Option<String>,
    /// Draw the roles of the unit definitions with their glTF models
    /// (sandbox scene); false (`--models none`) = placeholder shapes only.
    pub role_models: bool,
    /// Map directory; `None` = generate a `map_cells` square map.
    pub map_dir: Option<String>,
    pub map_cells: u32,
    pub exit_after: Option<Duration>,
    pub auto_zoom: bool,
    pub panels: bool,
    pub grid: bool,
    pub lod_tint: bool,
    pub hover: Option<(f32, f32)>,
    /// `--build <role>`: start with this structure chosen to build, as if B had been pressed
    /// until it came up (WP6.2). Sandbox only; a role the local faction does not build is reported.
    pub build: Option<String>,
    /// `--build-at x,y`: the ghost sits on this cell whatever the cursor does (screenshots).
    pub build_at: Option<(u32, u32)>,
    pub unit_display: UnitDisplay,
    pub zoom: Option<f64>,
    pub focus: Option<(f64, f64)>,
    pub frame_cap: Option<u32>,
    pub lod_px: Option<f32>,
    pub lod_level: Option<u8>,
    pub ra_audio: Option<String>,
    pub bench: Option<BenchOptions>,
    /// `--shots <dir>`: write three screenshots (close-up, middle, whole map)
    /// into `dir` and exit (CI smoke test).
    pub shots: Option<PathBuf>,
    pub screen: ScreenMode,
    pub present: Present,
    /// Allocate the unit instance buffers at their largest size at load
    /// (the fix of follow-up 14); `--no-reserve` turns it off to reproduce
    /// the hitches.
    pub reserve: bool,
    /// `--sim-delay-ms N`: every sim tick takes N ms longer, as a heavy one
    /// would (WP1.15): to see the sim fall behind (the F1 panel says so) while
    /// the frames stay smooth. Zero = off.
    pub sim_delay: Duration,
    /// Flags that were given but could not be used.
    pub warnings: Vec<String>,
}

impl Options {
    /// The options for `preset` and the command line `args` (without the
    /// program name).
    pub fn parse(preset: Preset, args: impl IntoIterator<Item = String>) -> Self {
        let demo = preset == Preset::Demo;
        let mut o = Options {
            preset,
            scene: if demo { Scene::Crowd } else { Scene::Sandbox },
            units: if demo { DEMO_UNITS } else { 0 },
            models: demo.then(|| DEMO_PACK.to_owned()),
            role_models: true,
            map_dir: None,
            map_cells: DEFAULT_MAP_CELLS,
            exit_after: None,
            auto_zoom: false,
            panels: false,
            grid: false,
            lod_tint: false,
            hover: None,
            build: None,
            build_at: None,
            unit_display: UnitDisplay::Auto,
            zoom: None,
            focus: None,
            frame_cap: None,
            lod_px: None,
            lod_level: None,
            ra_audio: None,
            bench: None,
            shots: None,
            screen: ScreenMode::Windowed,
            present: Present::Vsync,
            reserve: true,
            sim_delay: Duration::ZERO,
            warnings: Vec::new(),
        };
        let mut units_given = false;
        let mut bench = false;
        let mut bench_seconds = DEFAULT_BENCH_SECONDS;
        let mut bench_frames = None;
        let mut bench_out = PathBuf::from("bench.json");
        let mut screen_given = false;
        let mut present_given = false;

        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            // The value of a flag; a missing one is reported once.
            let mut value = |warnings: &mut Vec<String>| {
                let v = args.next();
                if v.is_none() {
                    warnings.push(format!("{arg} needs a value"));
                }
                v
            };
            match arg.as_str() {
                "--units" => {
                    if let Some(v) = value(&mut o.warnings) {
                        match v.parse() {
                            Ok(n) => {
                                o.units = n;
                                units_given = true;
                            }
                            Err(_) => o.warnings.push(format!("--units {v}: not a number")),
                        }
                    }
                }
                "--models" => {
                    if let Some(v) = value(&mut o.warnings) {
                        o.role_models = v != "none";
                        o.models = (v != "none").then_some(v);
                    }
                }
                "--crowd" => o.scene = Scene::Crowd,
                "--sandbox" => o.scene = Scene::Sandbox,
                "--map" => o.map_dir = value(&mut o.warnings),
                "--map-cells" => {
                    if let Some(v) = value(&mut o.warnings) {
                        match v.parse::<u32>() {
                            Ok(n) if (1..=256).contains(&n) => o.map_cells = n,
                            _ => o
                                .warnings
                                .push(format!("--map-cells {v}: expected 1 to 256")),
                        }
                    }
                }
                "--exit-after" => {
                    if let Some(v) = value(&mut o.warnings) {
                        match v.parse::<f64>() {
                            Ok(s) if s >= 0.0 && s.is_finite() => {
                                o.exit_after = Some(Duration::from_secs_f64(s));
                            }
                            _ => o.warnings.push(format!("--exit-after {v}: not seconds")),
                        }
                    }
                }
                "--auto-zoom" => o.auto_zoom = true,
                "--debug-ui" => o.panels = true,
                "--debug-grid" => o.grid = true,
                "--debug-lod" => o.lod_tint = true,
                "--hover" => {
                    o.hover = value(&mut o.warnings).and_then(|v| {
                        let (x, y) = v.split_once(',')?;
                        Some((x.parse().ok()?, y.parse().ok()?))
                    });
                }
                "--build" => o.build = value(&mut o.warnings),
                "--build-at" => {
                    o.build_at = value(&mut o.warnings).and_then(|v| {
                        let (x, y) = v.split_once(',')?;
                        Some((x.parse().ok()?, y.parse().ok()?))
                    });
                    if o.build_at.is_none() {
                        o.warnings.push("--build-at: expected x,y in cells".into());
                    }
                }
                "--unit-display" => {
                    o.unit_display = match value(&mut o.warnings).as_deref() {
                        Some("models") => UnitDisplay::Models,
                        Some("icons") => UnitDisplay::Icons,
                        _ => UnitDisplay::Auto,
                    };
                }
                "--zoom" => o.zoom = value(&mut o.warnings).and_then(|v| v.parse().ok()),
                "--focus" => {
                    o.focus = value(&mut o.warnings).and_then(|v| {
                        let (x, z) = v.split_once(',')?;
                        Some((x.parse().ok()?, z.parse().ok()?))
                    });
                }
                "--frame-cap" => o.frame_cap = value(&mut o.warnings).and_then(|v| v.parse().ok()),
                "--lod-px" => o.lod_px = value(&mut o.warnings).and_then(|v| v.parse().ok()),
                "--lod-level" => o.lod_level = value(&mut o.warnings).and_then(|v| v.parse().ok()),
                "--ra-audio" => o.ra_audio = value(&mut o.warnings),
                "--bench" => bench = true,
                "--seconds" => {
                    if let Some(v) = value(&mut o.warnings) {
                        match v.parse::<f64>() {
                            Ok(s) if s > 0.0 && s.is_finite() => bench_seconds = s,
                            _ => o
                                .warnings
                                .push(format!("--seconds {v}: not a positive number")),
                        }
                    }
                }
                "--frames" => {
                    if let Some(v) = value(&mut o.warnings) {
                        match v.parse::<u32>() {
                            Ok(n) if n > 0 => bench_frames = Some(n),
                            _ => o
                                .warnings
                                .push(format!("--frames {v}: not a positive number")),
                        }
                    }
                }
                "--bench-out" | "--out" => {
                    if let Some(v) = value(&mut o.warnings) {
                        bench_out = PathBuf::from(v);
                    }
                }
                "--shots" => o.shots = value(&mut o.warnings).map(PathBuf::from),
                "--windowed" => {
                    o.screen = ScreenMode::Windowed;
                    screen_given = true;
                }
                "--fullscreen" => {
                    o.screen = ScreenMode::Borderless;
                    screen_given = true;
                }
                "--resolution" => {
                    if let Some(v) = value(&mut o.warnings) {
                        match v.split_once('x').and_then(|(w, h)| {
                            Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))
                        }) {
                            Some((width, height)) if width > 0 && height > 0 => {
                                o.screen = ScreenMode::Fixed { width, height };
                                screen_given = true;
                            }
                            _ => o
                                .warnings
                                .push(format!("--resolution {v}: expected WIDTHxHEIGHT")),
                        }
                    }
                }
                "--present" => {
                    if let Some(v) = value(&mut o.warnings) {
                        match v.as_str() {
                            "vsync" => o.present = Present::Vsync,
                            "uncapped" | "immediate" | "mailbox" => o.present = Present::Uncapped,
                            _ => o
                                .warnings
                                .push(format!("--present {v}: expected vsync or uncapped")),
                        }
                        present_given = true;
                    }
                }
                "--no-reserve" => o.reserve = false,
                "--sim-delay-ms" => {
                    if let Some(v) = value(&mut o.warnings) {
                        match v.parse::<u32>() {
                            Ok(ms) if ms <= 10_000 => {
                                o.sim_delay = Duration::from_millis(u64::from(ms));
                            }
                            _ => o
                                .warnings
                                .push(format!("--sim-delay-ms {v}: expected 0 to 10000")),
                        }
                    }
                }
                _ => {}
            }
        }

        // The viewer's crowd is the old 2,000 placeholder units.
        if !demo && o.scene == Scene::Crowd && !units_given {
            o.units = GAME_UNITS;
        }
        if bench {
            o.bench = Some(BenchOptions {
                seconds: bench_seconds,
                frames: bench_frames,
                out: bench_out,
            });
            // The benchmark runs in real fullscreen at the reference
            // resolution and without vsync unless told otherwise.
            if !screen_given {
                o.screen = ScreenMode::Fixed {
                    width: BENCH_RESOLUTION.0,
                    height: BENCH_RESOLUTION.1,
                };
            }
            if !present_given {
                o.present = Present::Uncapped;
            }
        }
        o
    }

    /// The options of this process.
    pub fn from_env(preset: Preset) -> Self {
        Self::parse(preset, std::env::args().skip(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(preset: Preset, line: &str) -> Options {
        Options::parse(preset, line.split_whitespace().map(str::to_owned))
    }

    #[test]
    fn the_presets_differ_in_their_defaults_only() {
        let game = parse(Preset::Game, "");
        assert_eq!(
            (game.scene, game.units, game.models.as_deref()),
            (Scene::Sandbox, 0, None)
        );
        assert!(game.role_models);
        let demo = parse(Preset::Demo, "");
        assert_eq!(
            (demo.scene, demo.units, demo.models.as_deref()),
            (Scene::Crowd, DEMO_UNITS, Some(DEMO_PACK))
        );
        assert_eq!(demo.map_cells, 256);
        assert!(demo.bench.is_none() && demo.shots.is_none());
        assert_eq!(demo.present, Present::Vsync);
        assert_eq!(demo.screen, ScreenMode::Windowed);
        assert!(demo.reserve);
    }

    #[test]
    fn the_viewer_can_still_show_the_old_crowd() {
        let o = parse(Preset::Game, "--crowd");
        assert_eq!((o.scene, o.units), (Scene::Crowd, GAME_UNITS));
        let o = parse(Preset::Game, "--crowd --units 300 --models testpack");
        assert_eq!((o.scene, o.units), (Scene::Crowd, 300));
        assert_eq!(o.models.as_deref(), Some("testpack"));
        // Extra wanderers on top of the sandbox.
        let o = parse(Preset::Game, "--units 500");
        assert_eq!((o.scene, o.units), (Scene::Sandbox, 500));
        // The demo is the crowd whatever else is said, unless asked for the sandbox.
        assert_eq!(parse(Preset::Demo, "--bench").scene, Scene::Crowd);
        assert_eq!(parse(Preset::Demo, "--sandbox").scene, Scene::Sandbox);
        // `--models none` turns the glTF models of the roles off too.
        let o = parse(Preset::Game, "--models none");
        assert!(!o.role_models && o.models.is_none());
    }

    #[test]
    fn flags_override_the_preset() {
        let o = parse(
            Preset::Demo,
            "--units 123 --models none --map-cells 64 --no-reserve",
        );
        assert_eq!(o.units, 123);
        assert_eq!(o.models, None);
        assert_eq!(o.map_cells, 64);
        assert!(!o.reserve);
        assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    }

    #[test]
    fn bench_means_fullscreen_1080p_without_vsync() {
        let o = parse(Preset::Demo, "--bench");
        let b = o.bench.expect("bench");
        assert_eq!(b.seconds, DEFAULT_BENCH_SECONDS);
        assert_eq!(b.out, PathBuf::from("bench.json"));
        assert_eq!(
            o.screen,
            ScreenMode::Fixed {
                width: 1920,
                height: 1080
            }
        );
        assert_eq!(o.present, Present::Uncapped);
    }

    #[test]
    fn bench_defaults_can_be_overridden() {
        let o = parse(
            Preset::Demo,
            "--bench --frames 300 --windowed --present vsync --out /tmp/x.json",
        );
        let b = o.bench.expect("bench");
        assert_eq!(b.frames, Some(300));
        assert_eq!(b.out, PathBuf::from("/tmp/x.json"));
        assert_eq!(o.screen, ScreenMode::Windowed);
        assert_eq!(o.present, Present::Vsync);
        let o = parse(Preset::Demo, "--bench --resolution 1280x720");
        assert_eq!(
            o.screen,
            ScreenMode::Fixed {
                width: 1280,
                height: 720
            }
        );
    }

    #[test]
    fn bad_values_are_reported_and_ignored() {
        let o = parse(
            Preset::Game,
            "--units many --map-cells 999 --resolution big --exit-after",
        );
        assert_eq!(o.units, 0);
        assert_eq!(o.map_cells, DEFAULT_MAP_CELLS);
        assert_eq!(o.screen, ScreenMode::Windowed);
        assert_eq!(o.warnings.len(), 4, "{:?}", o.warnings);
    }

    #[test]
    fn the_old_viewer_flags_still_parse() {
        let o = parse(
            Preset::Game,
            "--hover 10,20 --zoom 0.5 --focus 100,200 --unit-display icons --debug-ui \
             --lod-px 3 --lod-level 2 --exit-after 1.5 --auto-zoom --frame-cap 30",
        );
        assert_eq!(o.hover, Some((10.0, 20.0)));
        assert_eq!(o.zoom, Some(0.5));
        assert_eq!(o.focus, Some((100.0, 200.0)));
        assert_eq!(o.unit_display, UnitDisplay::Icons);
        assert!(o.panels && o.auto_zoom);
        assert_eq!((o.lod_px, o.lod_level), (Some(3.0), Some(2)));
        assert_eq!(o.exit_after, Some(Duration::from_millis(1500)));
        assert_eq!(o.frame_cap, Some(30));
        assert!(o.warnings.is_empty());
    }

    #[test]
    fn sim_delay_is_off_by_default_and_takes_milliseconds() {
        assert_eq!(parse(Preset::Game, "").sim_delay, Duration::ZERO);
        let o = parse(Preset::Game, "--sim-delay-ms 70");
        assert_eq!(o.sim_delay, Duration::from_millis(70));
        assert!(o.warnings.is_empty());
        let bad = parse(Preset::Game, "--sim-delay-ms slow");
        assert_eq!(bad.sim_delay, Duration::ZERO);
        assert_eq!(bad.warnings.len(), 1);
    }
}
