//! The game binary: glues all crates together.
//!
//! Opens the window (sf_engine), draws the sf_render world scene through the
//! game camera every frame (vsync) and prints the sim tick and frame rates
//! once per second. Camera controls come from `assets/config/input.ron`:
//! wheel = zoom toward the cursor, WASD/arrows/screen edge = pan, middle
//! mouse drag = grab the ground.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sf_engine::{Action, App, Context, EngineConfig, Resized};
use sf_render::camera::DVec2;
use sf_render::scene::world_scene;
use sf_render::{Camera, CameraConfig, CameraInput, GpuOptions, Renderer};

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

struct Runner {
    started: Option<Instant>,
    report_at: Instant,
    ticks: u32,
    frames: u32,
    total_ticks: u64,
    exit_after: Option<Duration>,
    renderer: Option<Renderer>,
    camera: Camera,
    last_frame: Option<Instant>,
    auto_zoom: bool,
    /// Set when the renderer could not start or a frame failed; `main` then
    /// exits with a non-zero code.
    failed: Arc<AtomicBool>,
}

impl Runner {
    fn new(exit_after: Option<Duration>, auto_zoom: bool, failed: Arc<AtomicBool>) -> Self {
        let now = Instant::now();
        Self {
            started: None,
            report_at: now + Duration::from_secs(1),
            ticks: 0,
            frames: 0,
            total_ticks: 0,
            exit_after,
            renderer: None,
            camera: Camera::new(CameraConfig::default(), 1280, 720),
            last_frame: None,
            auto_zoom,
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
        CameraInput {
            pan: DVec2::new(
                axis(Action::CameraPanRight, Action::CameraPanLeft),
                axis(Action::CameraPanUp, Action::CameraPanDown),
            ),
            zoom_notches,
            cursor,
            drag: actions.held(Action::CameraDrag),
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
                renderer.set_scene(&world_scene(256));
                renderer.watch_shader_file(
                    ctx.assets().root().join("shaders/scene.wgsl"),
                    Duration::from_millis(250),
                );
                self.camera.set_viewport(s.width, s.height);
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
    }

    fn render(&mut self, ctx: &mut Context, _alpha: f32) {
        self.frames += 1;
        let now = Instant::now();
        let dt = self
            .last_frame
            .replace(now)
            .map_or(0.0, |t| now.duration_since(t).as_secs_f64())
            .min(MAX_CAMERA_DT);
        let input = self.camera_input(ctx, dt);
        self.camera.update(dt, &input);
        if let Some(renderer) = &mut self.renderer {
            renderer.set_view_proj(self.camera.view_proj());
        }
        if let Some(renderer) = &mut self.renderer
            && let Err(e) = renderer.render()
        {
            eprintln!("sf_game: render failed: {e}");
            self.failed.store(true, Ordering::Relaxed);
            ctx.request_exit();
        }
        let actions = ctx.actions();
        if actions.pressed(Action::ToggleFullscreen) {
            ctx.toggle_fullscreen();
            println!("fullscreen: {}", ctx.is_fullscreen());
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
                    "{} fps, {} ticks/s (total {}), frame ms avg {:.2} p99 {:.2} max {:.2}",
                    self.frames,
                    self.ticks,
                    self.total_ticks,
                    f.avg.as_secs_f64() * 1e3,
                    f.p99.as_secs_f64() * 1e3,
                    f.max.as_secs_f64() * 1e3,
                ),
                None => println!("{} fps, {} ticks/s", self.frames, self.ticks),
            }
            self.frames = 0;
            self.ticks = 0;
            self.report_at += Duration::from_secs(1);
        }
    }

    fn on_resize(&mut self, _ctx: &mut Context, size: Resized) {
        self.camera.set_viewport(size.width, size.height);
        if let Some(renderer) = &mut self.renderer {
            renderer.resize(size.width, size.height);
        }
        println!(
            "resized {}x{} @ {}",
            size.width, size.height, size.scale_factor
        );
    }

    fn on_asset_changed(&mut self, _ctx: &mut Context, path: &std::path::Path) {
        println!("asset changed: {}", path.display());
    }

    fn on_exit(&mut self, _ctx: &mut Context) {
        self.renderer = None; // release the surface before the window goes
        println!("exit after {} ticks", self.total_ticks);
    }
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

fn main() {
    let config = EngineConfig {
        frame_cap_hz: frame_cap_arg(),
        ..EngineConfig::default()
    };
    let failed = Arc::new(AtomicBool::new(false));
    if let Err(e) = sf_engine::run(
        config,
        Runner::new(exit_after_arg(), auto_zoom_arg(), failed.clone()),
    ) {
        eprintln!("sf_game: {e}");
        std::process::exit(1);
    }
    if failed.load(Ordering::Relaxed) {
        std::process::exit(1);
    }
}
