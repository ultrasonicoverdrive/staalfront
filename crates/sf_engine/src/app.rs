//! The window and main loop on top of winit.
//!
//! The application implements [`App`]; [`run`] owns the event loop and drives
//! `fixed_update(tick)` at exactly 20 Hz and `render(alpha)` once per frame.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Fullscreen, Window, WindowId};

use crate::assets::AssetRoot;
use crate::input::{ActionState, Bindings, InputState};
use crate::time::{FixedStep, FrameStats};

/// Startup options for the window and loop.
#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub title: String,
    /// Initial inner size in logical (DPI-independent) pixels.
    pub size: (u32, u32),
    /// Windowed, borderless fullscreen or exclusive fullscreen at a fixed
    /// resolution; see [`ScreenMode`].
    pub screen_mode: ScreenMode,
    /// Optional upper bound on the frame rate. `None` (default) = render at
    /// display rate: the loop polls and the present mode (vsync, WP1.4) paces
    /// it. Until WP1.4 a blank window therefore spins a core; use a cap for
    /// smoke tests.
    pub frame_cap_hz: Option<u32>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            title: "Staalfront".to_owned(),
            size: (1280, 720),
            screen_mode: ScreenMode::Windowed,
            frame_cap_hz: None,
        }
    }
}

/// How the window covers the screen at start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenMode {
    /// A normal window of [`EngineConfig::size`] logical pixels.
    Windowed,
    /// Borderless fullscreen on the primary monitor, at its own resolution.
    Borderless,
    /// Fullscreen on the primary monitor at exactly `width` x `height`
    /// pixels. When the monitor already runs at that size this is borderless
    /// fullscreen (no mode switch). Otherwise the best video mode of that
    /// size is requested (exclusive fullscreen), and when there is none the
    /// result is borderless at the native size; [`ScreenReport`] says which.
    Fixed { width: u32, height: u32 },
}

/// What [`ScreenMode`] came out as, for logs and the benchmark file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenReport {
    /// `windowed`, `borderless` or `exclusive`.
    pub mode: &'static str,
    /// The monitor's name, if the platform gives one.
    pub monitor: Option<String>,
    /// The monitor's size in pixels.
    pub monitor_size: Option<(u32, u32)>,
    /// Refresh rate of the mode in use, millihertz (0 = unknown).
    pub refresh_mhz: u32,
    /// Why the request was not met exactly (no such video mode, no monitor).
    pub note: Option<String>,
}

/// The fullscreen state to ask winit for, and what to report about it.
fn choose_fullscreen(
    event_loop: &ActiveEventLoop,
    mode: ScreenMode,
) -> (Option<Fullscreen>, ScreenReport) {
    let monitor = event_loop
        .primary_monitor()
        .or_else(|| event_loop.available_monitors().next());
    let mut report = ScreenReport {
        mode: "windowed",
        monitor: monitor.as_ref().and_then(|m| m.name()),
        monitor_size: monitor.as_ref().map(|m| {
            let s = m.size();
            (s.width, s.height)
        }),
        refresh_mhz: monitor
            .as_ref()
            .and_then(|m| m.refresh_rate_millihertz())
            .unwrap_or(0),
        note: None,
    };
    let borderless = |report: &mut ScreenReport| {
        report.mode = "borderless";
        Some(Fullscreen::Borderless(monitor.clone()))
    };
    let fullscreen = match mode {
        ScreenMode::Windowed => None,
        ScreenMode::Borderless => borderless(&mut report),
        ScreenMode::Fixed { width, height } => match &monitor {
            None => {
                report.note = Some("no monitor found; borderless on the default one".into());
                borderless(&mut report)
            }
            Some(_) if report.monitor_size == Some((width, height)) => borderless(&mut report),
            Some(m) => {
                // The mode with the most pixel depth, then the highest refresh.
                let best = m
                    .video_modes()
                    .filter(|v| v.size().width == width && v.size().height == height)
                    .max_by_key(|v| (v.refresh_rate_millihertz(), v.bit_depth()));
                match best {
                    Some(v) => {
                        report.mode = "exclusive";
                        report.refresh_mhz = v.refresh_rate_millihertz();
                        Some(Fullscreen::Exclusive(v))
                    }
                    None => {
                        report.note = Some(format!(
                            "the monitor has no {width}x{height} video mode; borderless at {}x{}",
                            report.monitor_size.map_or(0, |s| s.0),
                            report.monitor_size.map_or(0, |s| s.1)
                        ));
                        borderless(&mut report)
                    }
                }
            }
        },
    };
    (fullscreen, report)
}

/// Window size and DPI scale, delivered by [`App::on_resize`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resized {
    /// Physical pixels.
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
}

/// What the application can see and do during a callback.
pub struct Context<'a> {
    window: &'a Arc<Window>,
    input: &'a InputState,
    actions: &'a ActionState,
    stats: &'a FrameStats,
    assets: &'a AssetRoot,
    screen: &'a ScreenReport,
    exit: &'a mut bool,
}

impl Context<'_> {
    pub fn window(&self) -> &Arc<Window> {
        self.window
    }
    pub fn input(&self) -> &InputState {
        self.input
    }
    pub fn actions(&self) -> &ActionState {
        self.actions
    }
    pub fn frame_stats(&self) -> &FrameStats {
        self.stats
    }
    pub fn assets(&self) -> &AssetRoot {
        self.assets
    }
    /// DPI scale factor of the window's current monitor.
    pub fn scale_factor(&self) -> f64 {
        self.window.scale_factor()
    }
    /// How the window covers the screen, as it was set up at start.
    pub fn screen_report(&self) -> &ScreenReport {
        self.screen
    }
    /// Inner size in physical pixels.
    pub fn size(&self) -> PhysicalSize<u32> {
        self.window.inner_size()
    }
    pub fn is_fullscreen(&self) -> bool {
        self.window.fullscreen().is_some()
    }
    /// Switch between windowed and borderless fullscreen.
    pub fn toggle_fullscreen(&self) {
        let next = if self.is_fullscreen() {
            None
        } else {
            Some(Fullscreen::Borderless(None))
        };
        self.window.set_fullscreen(next);
    }
    /// Leave the loop after the current frame.
    pub fn request_exit(&mut self) {
        *self.exit = true;
    }
}

/// The application driven by the engine loop. All methods but the two
/// required ones have empty defaults.
pub trait App {
    /// Once, after the window exists.
    fn on_start(&mut self, _ctx: &mut Context) {}

    /// Fixed simulation step; `tick` counts from 0. Exactly 20 per second of
    /// run time (modulo the catch-up cap). Input seen here is the current
    /// frame's snapshot: turn it into commands in `render`, not here.
    fn fixed_update(&mut self, ctx: &mut Context, tick: u64);

    /// Once per frame. `alpha` in `[0,1)` is how far we are between the last
    /// and the next tick; use it to interpolate. Read input/actions here.
    fn render(&mut self, ctx: &mut Context, alpha: f32);

    /// A raw window event, offered before the engine turns it into input.
    /// Return `true` when an overlay UI (e.g. the egui debug panels) consumed
    /// it: the engine then does not feed presses and wheel notches to the
    /// input state, so a click on a slider does not also pan the camera.
    /// Releases and cursor moves always reach the input state, so nothing
    /// stays "held" and the cursor position stays current. Lifecycle events
    /// (close, resize, focus) are never withheld.
    fn on_window_event(&mut self, _ctx: &mut Context, _event: &WindowEvent) -> bool {
        false
    }

    /// Window resized or DPI scale changed (also once at start).
    fn on_resize(&mut self, _ctx: &mut Context, _size: Resized) {}

    /// A watched RON asset changed (debug builds only). Input bindings are
    /// already reloaded by the engine before this is called.
    fn on_asset_changed(&mut self, _ctx: &mut Context, _path: &Path) {}

    /// Once, when the loop ends.
    fn on_exit(&mut self, _ctx: &mut Context) {}
}

/// Open the window and run until quit. Blocks.
pub fn run(config: EngineConfig, app: impl App) -> Result<(), winit::error::EventLoopError> {
    let event_loop = EventLoop::new()?;
    let assets = AssetRoot::resolve();
    let mut handler = Handler::new(config, assets, app);
    event_loop.run_app(&mut handler)?;
    Ok(())
}

struct Handler<A: App> {
    config: EngineConfig,
    assets: AssetRoot,
    bindings: Bindings,
    app: A,
    window: Option<Arc<Window>>,
    input: InputState,
    actions: ActionState,
    stats: FrameStats,
    fixed: FixedStep,
    screen: ScreenReport,
    exit: bool,
    started: bool,
    last_frame: Option<Instant>,
    #[cfg(debug_assertions)]
    hot: Option<crate::assets::HotReload>,
}

impl<A: App> Handler<A> {
    fn new(config: EngineConfig, assets: AssetRoot, app: A) -> Self {
        let bindings = match Bindings::load(&assets.input_bindings()) {
            Ok(b) => b,
            Err(e) => {
                eprintln!(
                    "sf_engine: {} ({e}); using built-in bindings",
                    assets.input_bindings().display()
                );
                Bindings::builtin()
            }
        };
        Self {
            config,
            assets,
            bindings,
            app,
            window: None,
            input: InputState::new(),
            actions: ActionState::default(),
            stats: FrameStats::default(),
            fixed: FixedStep::sim_default(),
            screen: ScreenReport {
                mode: "windowed",
                monitor: None,
                monitor_size: None,
                refresh_mhz: 0,
                note: None,
            },
            exit: false,
            started: false,
            last_frame: None,
            #[cfg(debug_assertions)]
            hot: None,
        }
    }

    /// Run `f` with a [`Context`]. Returns `None` before the window exists.
    fn with_ctx<R>(&mut self, f: impl FnOnce(&mut A, &mut Context) -> R) -> Option<R> {
        let window = self.window.as_ref()?;
        let mut ctx = Context {
            window,
            input: &self.input,
            actions: &self.actions,
            stats: &self.stats,
            assets: &self.assets,
            screen: &self.screen,
            exit: &mut self.exit,
        };
        Some(f(&mut self.app, &mut ctx))
    }

    fn resized(&mut self) {
        let Some(window) = &self.window else { return };
        let size = window.inner_size();
        self.input.set_window_size(size.width, size.height);
        let event = Resized {
            width: size.width,
            height: size.height,
            scale_factor: window.scale_factor(),
        };
        self.with_ctx(|app, ctx| app.on_resize(ctx, event));
    }

    #[cfg(debug_assertions)]
    fn poll_hot_reload(&mut self) {
        let Some(hot) = &self.hot else { return };
        let changed = hot.poll();
        if changed.is_empty() {
            return;
        }
        let bindings_path = canonical(&self.assets.input_bindings());
        for path in changed {
            if canonical(&path) == bindings_path {
                match Bindings::load(&path) {
                    Ok(b) => {
                        self.bindings = b;
                        eprintln!("sf_engine: reloaded {}", path.display());
                    }
                    Err(e) => eprintln!("sf_engine: keeping old bindings, {e}"),
                }
            }
            self.with_ctx(|app, ctx| app.on_asset_changed(ctx, &path));
        }
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = self
            .last_frame
            .map_or(Duration::ZERO, |t| now.duration_since(t));
        self.last_frame = Some(now);
        if self.started && dt > Duration::ZERO {
            self.stats.record(dt);
        }

        #[cfg(debug_assertions)]
        self.poll_hot_reload();

        self.actions.update(&self.input, &self.bindings);

        let advance = self.fixed.advance(dt);
        for tick in advance.tick_range() {
            self.with_ctx(|app, ctx| app.fixed_update(ctx, tick));
        }
        let alpha = self.fixed.alpha();
        self.with_ctx(|app, ctx| app.render(ctx, alpha));

        self.input.end_frame();
        if self.exit {
            event_loop.exit();
        }
    }
}

/// Whether a key or button event the app consumed is kept from the input
/// state: presses are, releases are not (a release of something that was
/// never pressed is ignored by the input state, and one that was pressed
/// before the overlay took over must still end the hold).
fn withhold(consumed: bool, state: ElementState) -> bool {
    consumed && state.is_pressed()
}

#[cfg(debug_assertions)]
fn canonical(p: &Path) -> std::path::PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

impl<A: App> ApplicationHandler for Handler<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let (w, h) = self.config.size;
        let mut attrs = Window::default_attributes()
            .with_title(self.config.title.clone())
            .with_inner_size(LogicalSize::new(w, h));
        let (fullscreen, report) = choose_fullscreen(event_loop, self.config.screen_mode);
        self.screen = report;
        if fullscreen.is_some() {
            attrs = attrs.with_fullscreen(fullscreen);
        }
        match event_loop.create_window(attrs) {
            Ok(window) => self.window = Some(Arc::new(window)),
            Err(e) => {
                eprintln!("sf_engine: cannot create window: {e}");
                event_loop.exit();
                return;
            }
        }

        #[cfg(debug_assertions)]
        {
            match crate::assets::HotReload::new(self.assets.root()) {
                Ok(h) => self.hot = Some(h),
                Err(e) => eprintln!(
                    "sf_engine: hot-reload off, cannot watch {}: {e}",
                    self.assets.root().display()
                ),
            }
        }

        self.resized();
        self.with_ctx(|app, ctx| app.on_start(ctx));
        self.started = true;
        self.last_frame = Some(Instant::now());
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let consumed = self
            .with_ctx(|app, ctx| app.on_window_event(ctx, &event))
            .unwrap_or(false);
        match event {
            WindowEvent::CloseRequested => self.exit = true,
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => self.resized(),
            WindowEvent::Focused(f) => self.input.set_focused(f),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key
                    && !withhold(consumed, event.state)
                {
                    self.input.key_event(code, event.state, event.repeat);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if !withhold(consumed, state) {
                    self.input.mouse_button(button, state);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.input.cursor_moved(position.x, position.y)
            }
            WindowEvent::CursorLeft { .. } => self.input.cursor_left(),
            WindowEvent::MouseWheel { delta, .. } if !consumed => match delta {
                MouseScrollDelta::LineDelta(_, y) => self.input.wheel(y),
                MouseScrollDelta::PixelDelta(p) => self.input.wheel_pixels(p.y as f32),
            },
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
        if self.exit {
            self.with_ctx(|app, ctx| app.on_exit(ctx));
            self.exit = false;
            self.window = None; // no further callbacks
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(window) = &self.window else { return };
        let Some(hz) = self.config.frame_cap_hz.filter(|&h| h > 0) else {
            event_loop.set_control_flow(ControlFlow::Poll);
            window.request_redraw();
            return;
        };
        let period = Duration::from_secs(1) / hz;
        let next = self.last_frame.map_or_else(Instant::now, |t| t + period);
        if Instant::now() >= next {
            event_loop.set_control_flow(ControlFlow::Poll);
            window.request_redraw();
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(next));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumed_presses_are_withheld_but_releases_pass() {
        assert!(withhold(true, ElementState::Pressed));
        assert!(!withhold(true, ElementState::Released));
        assert!(!withhold(false, ElementState::Pressed));
        assert!(!withhold(false, ElementState::Released));
    }

    /// The handler's path for a key event, as in `window_event`.
    fn feed_key(input: &mut InputState, consumed: bool, state: ElementState) {
        if !withhold(consumed, state) {
            input.key_event(winit::keyboard::KeyCode::KeyW, state, false);
        }
    }

    #[test]
    fn a_consumed_press_then_a_passed_release_leaves_the_key_up() {
        let mut input = InputState::new();
        feed_key(&mut input, true, ElementState::Pressed);
        assert!(!input.key_down(winit::keyboard::KeyCode::KeyW));
        feed_key(&mut input, true, ElementState::Released);
        assert!(!input.key_down(winit::keyboard::KeyCode::KeyW));
        input.end_frame();
        assert!(!input.key_down(winit::keyboard::KeyCode::KeyW));
    }

    #[test]
    fn a_press_before_the_panel_took_over_still_ends_on_release() {
        let mut input = InputState::new();
        feed_key(&mut input, false, ElementState::Pressed);
        assert!(input.key_down(winit::keyboard::KeyCode::KeyW));
        // The panel opens and consumes the release: it must still pass.
        feed_key(&mut input, true, ElementState::Released);
        assert!(!input.key_down(winit::keyboard::KeyCode::KeyW));
    }
}
