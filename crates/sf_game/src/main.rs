//! The game binary: glues all crates together.
//!
//! Opens the window (sf_engine), draws the sf_render world scene through the
//! game camera every frame (vsync) and prints the sim tick and frame rates
//! once per second. Camera controls come from `assets/config/input.ron`:
//! wheel = zoom toward the cursor, WASD/arrows/screen edge = pan, middle
//! mouse drag = grab the ground.
//!
//! Units (WP1.6): an `sf_sim` world with `--units N` wanderers (default 2000)
//! is stepped by the engine's fixed 20 Hz tick and drawn instanced on top of
//! the terrain: three placeholder shapes (cube, cylinder, cone) in four
//! player colors, positions interpolated between the last two ticks with the
//! engine's alpha, tile-culled against the camera frustum. The sim is 2D; the
//! height a unit stands on is looked up in the terrain for display only.
//! `--exit-after <s>` prints a frame-time and instancing summary on exit.
//!
//! F1 toggles the egui debug panels (WP1.11, [`debug_ui`]): frame time graph,
//! draw calls and instances, live camera tunables, sim tick and checksum, LOD
//! colouring and a 1 m cell grid. `--debug-ui` starts with them open. While
//! hidden they cost nothing: no egui pass, no events forwarded. In debug builds
//! `assets/config/units.ron` is hot-reloaded like the input and audio RON.
//!
//! The ground is the engine terrain (WP1.7): a procedural 256x256 m map
//! (seed 1) by default, or a map directory given with `--map <dir>`. Since
//! WP1.8 it has levels of detail and culling (`assets/config/terrain.ron`; the
//! flags `--lod-px <pixels>` and `--lod-level <n>` override the error budget
//! and force a level; the F1 panel's LOD switch, `--debug-lod`, tints chunks
//! by level), and the camera
//! works against the real terrain: the point under the cursor, the zoom
//! toward it and the middle-mouse grab are ray-terrain picks, and units and
//! the click sound sit on the terrain surface.

mod debug_ui;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use debug_ui::{DebugUi, PanelData};
use sf_audio::{AudioSystem, Listener};
use sf_data::map::{Map, generate::generate};
use sf_engine::{Action, App, Context, EngineConfig, Resized, winit};
use sf_math::{Fx, Rng, Vec2};
use sf_render::camera::DVec2;
use sf_render::scene::{MeshKind, world_markers};
use sf_render::{
    Camera, CameraConfig, CameraInput, DebugView, Frustum, GpuOptions, Ground, InstanceStats,
    Instancer, LodConfig, Motion, Renderer, Style, TerrainConfig, TerrainGround, TerrainStats,
};
use sf_sim::{Command, EntityView, Kind, PlayerId, Tick, World, WorldConfig};

/// Longest frame time fed to the camera; a stall (window drag, breakpoint)
/// must not make it jump.
const MAX_CAMERA_DT: f64 = 0.25;

/// Scripted wheel input for `--auto-zoom`: sweeps from the middle out to the
/// whole map and back in toward a fixed cursor position, repeating every 12 s.
fn auto_zoom_notches(elapsed: f64, prev: f64) -> f64 {
    const STEP: f64 = 0.2; // one notch per 0.2 s
    let phase = |t: f64| t % 12.0;
    let fired = |t: f64| (t / STEP).floor();
    // Notches whose time fell into (prev, elapsed].
    let count = fired(elapsed) - fired(prev);
    if count <= 0.0 {
        return 0.0;
    }
    match phase(elapsed) {
        p if p < 4.0 => -count,
        p if (6.0..11.0).contains(&p) => count,
        _ => 0.0,
    }
}

/// Default number of wandering units.
const DEFAULT_UNITS: u32 = 2000;

/// Frames ignored in the exit summary (shader and buffer warm-up).
const WARMUP_FRAMES: usize = 30;

/// Fewest samples for which the exit summary prints a p99.
const MIN_P99_SAMPLES: usize = 30;

/// Millisecond samples with average / p99 / max (nearest rank).
#[derive(Default)]
struct Samples(Vec<f64>);

impl Samples {
    fn push(&mut self, d: Duration) {
        self.0.push(d.as_secs_f64() * 1e3);
    }

    fn summary(&self, skip: usize) -> Option<(f64, f64, f64)> {
        let mut v: Vec<f64> = self.0.iter().skip(skip).copied().collect();
        if v.is_empty() {
            return None;
        }
        v.sort_by(f64::total_cmp);
        let avg = v.iter().sum::<f64>() / v.len() as f64;
        let p99 = v[(v.len() * 99).div_ceil(100).max(1) - 1];
        Some((avg, p99, v[v.len() - 1]))
    }

    /// "avg .. p99 .. max .. (n samples)". A p99 over fewer than
    /// [`MIN_P99_SAMPLES`] samples is just the maximum, so it is not printed.
    fn describe(&self, skip: usize) -> String {
        let n = self.0.len().saturating_sub(skip);
        self.summary(skip).map_or_else(
            || "n/a (0 samples)".to_string(),
            |(avg, p99, max)| {
                if n >= MIN_P99_SAMPLES {
                    format!("avg {avg:.3} p99 {p99:.3} max {max:.3} ({n} samples)")
                } else {
                    format!(
                        "avg {avg:.3} max {max:.3} ({n} samples, p99 n/a below {MIN_P99_SAMPLES})"
                    )
                }
            },
        )
    }
}

/// The placeholder shape of a sim entity kind (data-driven units come later).
fn shape_of(kind: Kind) -> u8 {
    (kind.0 % 256) as u8
}

/// Fill a world with `units` wanderers: kinds and owners are spread
/// deterministically (kind = i mod 3, owner = (i / 3) mod 4, so every
/// shape/color pair occurs), positions come from a seeded `sf_math::Rng`.
fn spawn_units(world: &mut World, units: u32, map: (u32, u32)) {
    let mut rng = Rng::new(0x5742_0106);
    let (w, h) = (Fx::from_int(map.0 as i32), Fx::from_int(map.1 as i32));
    for i in 0..units {
        let pos = Vec2::new(rng.fx_range(Fx::ZERO, w), rng.fx_range(Fx::ZERO, h));
        let command = Command::Spawn {
            kind: Kind((i % 3) as u16),
            pos,
            owner: PlayerId(((i / 3) % 4) as u8),
        };
        world
            .schedule(Tick(0), PlayerId(0), command)
            .expect("tick 0 is not in the past");
    }
}

struct Runner {
    started: Option<Instant>,
    report_at: Instant,
    ticks: u32,
    frames: u32,
    total_ticks: u64,
    exit_after: Option<Duration>,
    renderer: Option<Renderer>,
    audio: Option<AudioSystem>,
    camera: Camera,
    map: Map,
    /// The terrain as a ground: camera picking, units and sounds stand on it.
    ground: Arc<TerrainGround>,
    lod: LodConfig,
    world: World,
    instancer: Instancer,
    views: Vec<EntityView>,
    units: u32,
    /// Latest instancing and draw counters, for the once-a-second line.
    stats: InstanceStats,
    terrain: TerrainStats,
    draw_calls: u32,
    frame_ms: Samples,
    build_ms: Samples,
    upload_ms: Samples,
    tick_ms: Samples,
    last_frame: Option<Instant>,
    auto_zoom: bool,
    /// F1 panels (hidden by default).
    debug: DebugUi,
    /// Window size in pixels, to rebuild the camera when its tunables change.
    viewport: (u32, u32),
    /// Set when the renderer could not start or a frame failed; `main` then
    /// exits with a non-zero code.
    failed: Arc<AtomicBool>,
}

impl Runner {
    fn new(
        exit_after: Option<Duration>,
        auto_zoom: bool,
        map: Map,
        units: u32,
        lod: LodConfig,
        failed: Arc<AtomicBool>,
    ) -> Self {
        let now = Instant::now();
        let map_size = (map.width(), map.height());
        let mut world = World::new(WorldConfig::new(1).with_map_size(map_size.0, map_size.1))
            .expect("the map size is valid for a world");
        spawn_units(&mut world, units, map_size);
        let mut instancer =
            Instancer::new(Style::embedded(), (map_size.0 as f32, map_size.1 as f32));
        let ground = Arc::new(TerrainGround::new(&map));
        instancer.set_height_range(ground.min_height() as f32, ground.max_height() as f32);
        let camera_config = CameraConfig {
            map_size: DVec2::new(f64::from(map.width()), f64::from(map.height())),
            ..CameraConfig::default()
        };
        let mut camera = Camera::new(camera_config, 1280, 720);
        camera.set_ground(ground.clone());
        Self {
            started: None,
            report_at: now + Duration::from_secs(1),
            ticks: 0,
            frames: 0,
            total_ticks: 0,
            exit_after,
            renderer: None,
            audio: None,
            camera,
            map,
            ground,
            lod,
            world,
            instancer,
            views: Vec::new(),
            units,
            stats: InstanceStats::default(),
            terrain: TerrainStats::default(),
            draw_calls: 0,
            frame_ms: Samples::default(),
            build_ms: Samples::default(),
            upload_ms: Samples::default(),
            tick_ms: Samples::default(),
            last_frame: None,
            auto_zoom,
            debug: DebugUi::new(camera_config),
            viewport: (1280, 720),
            failed,
        }
    }
}

impl Runner {
    /// Translate this frame's actions into device-independent camera input.
    fn camera_input(&mut self, ctx: &Context, dt: f64) -> CameraInput {
        let actions = ctx.actions();
        let axis = |plus: Action, minus: Action| {
            f64::from(i8::from(actions.held(plus)) - i8::from(actions.held(minus)))
        };
        let cursor = ctx.input().cursor().map(|(x, y)| DVec2::new(x, y));
        let wheel = f64::from(ctx.input().wheel_delta());
        let (zoom_notches, cursor) = if self.auto_zoom {
            let size = ctx.size();
            let t = self.started.map_or(0.0, |s| s.elapsed().as_secs_f64());
            let notches = auto_zoom_notches(t, (t - dt).max(0.0));
            let at = DVec2::new(f64::from(size.width) * 0.7, f64::from(size.height) * 0.35);
            (notches, Some(at))
        } else if actions.pressed(Action::CameraZoomIn) || actions.pressed(Action::CameraZoomOut) {
            // A bound wheel gives fractional notches, a bound key one notch.
            let keys = f64::from(i8::from(actions.pressed(Action::CameraZoomIn)))
                - f64::from(i8::from(actions.pressed(Action::CameraZoomOut)));
            (if wheel != 0.0 { wheel } else { keys }, cursor)
        } else {
            (0.0, cursor)
        };
        // A slider being dragged owns the pointer: its position (even at the
        // screen edge) must not pan the camera.
        let ui_busy = self.debug.using_pointer();
        CameraInput {
            pan: if ui_busy {
                DVec2::ZERO
            } else {
                DVec2::new(
                    axis(Action::CameraPanRight, Action::CameraPanLeft),
                    axis(Action::CameraPanUp, Action::CameraPanDown),
                )
            },
            zoom_notches,
            cursor,
            drag: actions.held(Action::CameraDrag) && !ui_busy,
        }
    }

    /// Replace the camera with one built from `config`, keeping where it
    /// looks, how far it is zoomed and the terrain it picks against (the
    /// camera has no setter for its tunables; they only change here, from the
    /// debug panel).
    fn apply_camera_config(&mut self, config: CameraConfig) {
        self.camera = self.camera.with_config(config);
        self.camera.set_viewport(self.viewport.0, self.viewport.1);
    }

    /// What the F1 panels show this frame.
    fn panel_data(&self, ctx: &Context) -> PanelData {
        let stats = ctx.frame_stats();
        PanelData {
            frame_ms: stats
                .frames()
                .map(|d| (d.as_secs_f64() * 1e3) as f32)
                .collect(),
            summary: stats.summary(),
            draw: self
                .renderer
                .as_ref()
                .map(Renderer::draw_stats)
                .unwrap_or_default(),
            instances: self.stats,
            sim_tick: self.world.tick().0,
            checksum: self.world.checksum_history().last().copied(),
            entities: self.world.len(),
        }
    }

    /// F1 handling, the panel pass and the terrain debug view. While the
    /// panels are hidden the egui pass does not run at all.
    fn update_debug_ui(&mut self, ctx: &Context) {
        if ctx.actions().pressed(Action::ToggleDebug) {
            let visible = self.debug.toggle();
            println!("debug panels: {}", if visible { "on" } else { "off" });
            if !visible && let Some(renderer) = &mut self.renderer {
                renderer.clear_overlay();
            }
        }
        let mut camera_changed = None;
        if self.debug.visible() {
            // Show what the camera really has (it may have been reset).
            self.debug.state.camera = *self.camera.config();
            let data = self.panel_data(ctx);
            if let Some((frame, changed)) = self.debug.frame(ctx.window(), &data) {
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_overlay(frame);
                } else {
                    // Nothing to paint on; the deltas must still be consumed.
                    let mut textures = frame.textures_delta;
                    textures.clear();
                }
                if changed {
                    camera_changed = Some(self.debug.state.camera);
                }
            }
        }
        if let Some(config) = camera_changed {
            self.apply_camera_config(config);
        }
        if let Some(renderer) = &mut self.renderer {
            let state = &self.debug.state;
            renderer.set_debug_view(DebugView {
                lod_tint: state.lod_tint,
                grid_alpha: if state.grid {
                    DebugView::grid_alpha_for_distance(self.camera.distance())
                } else {
                    0.0
                },
            });
        }
    }
}

impl Runner {
    /// Debug builds: `assets/config/units.ron` changed on disk. A broken
    /// file is reported and the old look stays.
    fn reload_style(&mut self, ctx: &Context, path: &std::path::Path) {
        let units = ctx.assets().path("config/units.ron");
        let same = |a: &std::path::Path, b: &std::path::Path| {
            a.canonicalize()
                .ok()
                .zip(b.canonicalize().ok())
                .is_some_and(|(a, b)| a == b)
        };
        if !same(path, &units) {
            return;
        }
        let result = std::fs::read_to_string(&units)
            .map_err(|e| e.to_string())
            .and_then(|text| Style::from_ron(&text).map_err(|e| e.to_string()));
        match result {
            Ok(style) => {
                self.instancer.set_style(style);
                println!("reloaded {}", units.display());
            }
            Err(e) => eprintln!(
                "sf_game: keeping the old unit style, {}: {e}",
                units.display()
            ),
        }
    }
}

impl App for Runner {
    fn on_start(&mut self, ctx: &mut Context) {
        let now = Instant::now();
        self.started = Some(now);
        self.report_at = now + Duration::from_secs(1);
        let s = ctx.size();
        println!(
            "window {}x{} px, scale factor {}, assets {}",
            s.width,
            s.height,
            ctx.scale_factor(),
            ctx.assets().root().display()
        );
        let audio = AudioSystem::new(ctx.assets().root());
        println!(
            "audio: {}",
            if audio.is_silent() {
                "silent (no device)"
            } else {
                "device open"
            }
        );
        self.audio = Some(audio);
        match Renderer::for_window(
            ctx.window().clone(),
            s.width,
            s.height,
            &GpuOptions::default(),
        ) {
            Ok(mut renderer) => {
                println!(
                    "renderer: {} ({:?}, vsync)",
                    renderer.adapter_summary(),
                    renderer.color_format()
                );
                renderer.set_terrain(&self.map);
                renderer.set_terrain_lod(self.lod);
                let ground = &self.ground;
                let map = &self.map;
                renderer.set_scene(&world_markers(map.width().min(map.height()), |x, z| {
                    ground.height(f64::from(x), f64::from(z)) as f32
                }));
                renderer.watch_shader_file(
                    ctx.assets().root().join("shaders/scene.wgsl"),
                    Duration::from_millis(250),
                );
                self.camera.set_viewport(s.width, s.height);
                self.viewport = (s.width, s.height);
                self.renderer = Some(renderer);
            }
            Err(e) => {
                eprintln!("sf_game: cannot start the renderer: {e}");
                self.failed.store(true, Ordering::Relaxed);
                ctx.request_exit();
            }
        }
    }

    fn fixed_update(&mut self, _ctx: &mut Context, _tick: u64) {
        self.ticks += 1;
        self.total_ticks += 1;
        self.world.step();
        // Hand the new state to the presentation: once per tick the fixed-point
        // positions are converted to f32 and bucketed by map tile.
        let started = Instant::now();
        self.world.snapshot().collect_into(&mut self.views);
        self.instancer.update_tick(self.views.iter().map(|v| {
            Motion::from_fixed(
                shape_of(v.kind),
                v.owner.0,
                v.prev_pos,
                v.pos,
                v.prev_facing,
                v.facing,
            )
        }));
        self.tick_ms.push(started.elapsed());
    }

    fn render(&mut self, ctx: &mut Context, alpha: f32) {
        self.frames += 1;
        let now = Instant::now();
        let frame_dt = self.last_frame.replace(now).map(|t| now.duration_since(t));
        if let Some(d) = frame_dt {
            self.frame_ms.push(d);
        }
        let dt = frame_dt.map_or(0.0, |d| d.as_secs_f64()).min(MAX_CAMERA_DT);
        self.update_debug_ui(ctx);
        let input = self.camera_input(ctx, dt);
        self.camera.update(dt, &input);
        if let Some(renderer) = &mut self.renderer {
            let view_proj = self.camera.view_proj();
            renderer.set_view_proj(view_proj);
            // Cull by tile, interpolate, put the units on the ground.
            let started = Instant::now();
            let ground = &self.ground;
            self.stats =
                self.instancer
                    .build(&Frustum::from_view_proj(view_proj), alpha, |x, z| {
                        ground.height(f64::from(x), f64::from(z)) as f32
                    });
            self.build_ms.push(started.elapsed());
            let started = Instant::now();
            for kind in MeshKind::ALL {
                renderer.set_instances(kind, self.instancer.instances(kind));
            }
            self.upload_ms.push(started.elapsed());
            // Of the previous frame: counted where the draws are recorded.
            self.draw_calls = renderer.draw_stats().total_draw_calls();
        }
        // The listener follows the camera (it does not rotate, so right = +X).
        let focus = self.camera.focus();
        let focus_y = self.camera.focus_height() as f32;
        if let Some(audio) = &mut self.audio {
            audio.set_listener(Listener::new(
                [focus.x as f32, focus_y, focus.y as f32],
                [1.0, 0.0, 0.0],
                self.camera.distance() as f32,
            ));
            audio.update();
        }
        if let Some(renderer) = &mut self.renderer {
            if let Err(e) = renderer.render() {
                eprintln!("sf_game: render failed: {e}");
                self.failed.store(true, Ordering::Relaxed);
                ctx.request_exit();
            }
            // Level selection and culling happen inside render().
            self.terrain = renderer.terrain_stats();
        }
        let actions = ctx.actions();
        if actions.pressed(Action::ToggleFullscreen) {
            ctx.toggle_fullscreen();
            println!("fullscreen: {}", ctx.is_fullscreen());
        }
        if actions.pressed(Action::PlayTestSound)
            && let Some(audio) = &mut self.audio
        {
            // Played where the cursor points on the terrain (the focus if it
            // is off the window or the sky), so pan and zoom can be heard.
            let at = ctx
                .input()
                .cursor()
                .and_then(|(x, y)| self.camera.ground_point_3d(DVec2::new(x, y)))
                .unwrap_or(sf_render::camera::DVec3::new(
                    focus.x,
                    focus_y.into(),
                    focus.y,
                ));
            audio.play_at("test_click", [at.x as f32, at.y as f32, at.z as f32]);
        }
        if actions.pressed(Action::Quit) {
            println!("quit requested");
            ctx.request_exit();
        }

        if let Some(limit) = self.exit_after
            && self.started.is_some_and(|s| now.duration_since(s) >= limit)
        {
            println!("--exit-after reached");
            ctx.request_exit();
        }
        if now >= self.report_at {
            let ft = ctx.frame_stats().summary();
            match ft {
                Some(f) => println!(
                    "{} fps, {} ticks/s (total {}), frame ms avg {:.2} p99 {:.2} max {:.2}, \
                     units {}/{} visible, terrain {}/{} chunks {} tris, {} draw calls",
                    self.frames,
                    self.ticks,
                    self.total_ticks,
                    f.avg.as_secs_f64() * 1e3,
                    f.p99.as_secs_f64() * 1e3,
                    f.max.as_secs_f64() * 1e3,
                    self.stats.visible,
                    self.stats.total,
                    self.terrain.chunks_visible,
                    self.terrain.chunks_total,
                    self.terrain.triangles,
                    self.draw_calls,
                ),
                None => println!("{} fps, {} ticks/s", self.frames, self.ticks),
            }
            self.frames = 0;
            self.ticks = 0;
            self.report_at += Duration::from_secs(1);
        }
    }

    fn on_window_event(&mut self, ctx: &mut Context, event: &winit::event::WindowEvent) -> bool {
        // egui sees events first while the panels are open; hidden = free.
        self.debug.on_event(ctx.window(), event)
    }

    fn on_resize(&mut self, _ctx: &mut Context, size: Resized) {
        self.viewport = (size.width, size.height);
        self.camera.set_viewport(size.width, size.height);
        if let Some(renderer) = &mut self.renderer {
            renderer.resize(size.width, size.height);
        }
        println!(
            "resized {}x{} @ {}",
            size.width, size.height, size.scale_factor
        );
    }

    fn on_asset_changed(&mut self, ctx: &mut Context, path: &std::path::Path) {
        if let Some(audio) = &mut self.audio {
            audio.on_asset_changed(path);
        }
        self.reload_style(ctx, path);
        println!("asset changed: {}", path.display());
    }

    fn on_exit(&mut self, _ctx: &mut Context) {
        self.renderer = None; // release the surface before the window goes
        println!("exit after {} ticks", self.total_ticks);
        // Summary for the stress test (software rendering: no fps claim).
        println!(
            "summary: {} units requested, {} in the world, {} frames",
            self.units,
            self.world.len(),
            self.frame_ms.0.len() + 1,
        );
        println!(
            "summary: frame ms {} (first {WARMUP_FRAMES} skipped)",
            self.frame_ms.describe(WARMUP_FRAMES)
        );
        println!(
            "summary: build+cull ms {}",
            self.build_ms.describe(WARMUP_FRAMES)
        );
        println!(
            "summary: instance upload ms {}",
            self.upload_ms.describe(WARMUP_FRAMES)
        );
        println!(
            "summary: per-tick convert+bucket ms {}",
            self.tick_ms.describe(WARMUP_FRAMES / 3)
        );
        println!(
            "summary: last frame {} of {} instances visible ({} of {} tiles), \
             per mesh {:?}, {} draw calls",
            self.stats.visible,
            self.stats.total,
            self.stats.tiles_visible,
            self.stats.tiles_total,
            self.stats.per_mesh,
            self.draw_calls,
        );
        println!(
            "summary: last frame terrain {}/{} chunks, {} triangles, {} terrain draw calls, \
             chunks per level {:?}",
            self.terrain.chunks_visible,
            self.terrain.chunks_total,
            self.terrain.triangles,
            self.terrain.draw_calls,
            self.terrain.per_level,
        );
    }
}

/// `--units <n>`: number of wandering units (default [`DEFAULT_UNITS`]).
fn units_arg() -> u32 {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--units" {
            return args
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_UNITS);
        }
    }
    DEFAULT_UNITS
}

/// `--exit-after <seconds>`: quit by itself (smoke tests).
fn exit_after_arg() -> Option<Duration> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--exit-after" {
            return args
                .next()?
                .parse::<f64>()
                .ok()
                .map(Duration::from_secs_f64);
        }
    }
    None
}

/// `--auto-zoom`: scripted zoom sweep (demo, screenshots).
fn auto_zoom_arg() -> bool {
    std::env::args().skip(1).any(|a| a == "--auto-zoom")
}

/// `--debug-ui`: start with the F1 panels open (screenshots, smoke tests);
/// `--debug-grid` / `--debug-lod` also switch the cell grid / LOD tint on.
fn debug_ui_args() -> (bool, bool, bool) {
    let has = |flag: &str| std::env::args().skip(1).any(|a| a == flag);
    (has("--debug-ui"), has("--debug-grid"), has("--debug-lod"))
}

/// `--zoom <0..1>` and `--focus <x,z>`: start the camera there (screenshots).
fn camera_args() -> (Option<f64>, Option<DVec2>) {
    let mut args = std::env::args().skip(1);
    let (mut zoom, mut focus) = (None, None);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--zoom" => zoom = args.next().and_then(|v| v.parse().ok()),
            "--focus" => {
                focus = args.next().and_then(|v| {
                    let (x, z) = v.split_once(',')?;
                    Some(DVec2::new(x.parse().ok()?, z.parse().ok()?))
                });
            }
            _ => {}
        }
    }
    (zoom, focus)
}

/// `--frame-cap <hz>`: limit the frame rate (smoke tests; default uncapped).
fn frame_cap_arg() -> Option<u32> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--frame-cap" {
            return args.next()?.parse().ok();
        }
    }
    None
}

/// The terrain settings: `assets/config/terrain.ron` as embedded, with the
/// flags `--lod-px <pixels>` (error budget) and `--lod-level <n>` (every
/// chunk at that level) on top. The tint by level is the debug panel's LOD
/// switch (`--debug-lod`).
fn lod_config_args() -> LodConfig {
    let mut lod = TerrainConfig::embedded().lod;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--lod-px" => {
                if let Some(px) = args.next().and_then(|v| v.parse::<f32>().ok()) {
                    lod.error_px = px;
                }
            }
            "--lod-level" => lod.force_level = args.next().and_then(|v| v.parse().ok()),
            _ => {}
        }
    }
    lod
}

/// `--map <dir>`: load a map directory instead of generating the default.
fn map_arg() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--map" {
            return args.next();
        }
    }
    None
}

fn main() {
    let map = match map_arg() {
        Some(dir) => Map::load(std::path::Path::new(&dir)).unwrap_or_else(|e| {
            eprintln!("sf_game: cannot load map {dir}: {e}");
            std::process::exit(1);
        }),
        None => generate(256, 256, 1),
    };
    let config = EngineConfig {
        frame_cap_hz: frame_cap_arg(),
        ..EngineConfig::default()
    };
    let failed = Arc::new(AtomicBool::new(false));
    let mut runner = Runner::new(
        exit_after_arg(),
        auto_zoom_arg(),
        map,
        units_arg(),
        lod_config_args(),
        failed.clone(),
    );
    let (panels, grid, lod) = debug_ui_args();
    runner.debug.set_visible(panels);
    runner.debug.state.grid = grid;
    runner.debug.state.lod_tint = lod;
    let (zoom, focus) = camera_args();
    if let Some(zoom) = zoom {
        runner.camera.set_zoom(zoom);
    }
    if let Some(focus) = focus {
        runner.camera.set_focus(focus);
    }
    if let Err(e) = sf_engine::run(config, runner) {
        eprintln!("sf_game: {e}");
        std::process::exit(1);
    }
    if failed.load(Ordering::Relaxed) {
        std::process::exit(1);
    }
}
