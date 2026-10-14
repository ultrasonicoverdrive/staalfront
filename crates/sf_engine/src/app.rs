//! The window and main loop on top of winit.
//!
//! The application implements [`App`]; [`run`] owns the event loop and drives
//! `fixed_update(tick)` at exactly 20 Hz and `render(alpha)` once per frame.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{MouseScrollDelta, WindowEvent};
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
    pub fullscreen: bool,
    /// Upper bound on the frame rate while nothing else paces the loop.
    /// `None` = uncapped. Once wgpu presents with vsync (WP1.4) this can be
    /// `None`; until then it keeps the empty loop from spinning a core.
    pub frame_cap_hz: Option<u32>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            title: "Staalfront".to_owned(),
            size: (1280, 720),
            fullscreen: false,
            frame_cap_hz: Some(60),
        }
    }
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

#[cfg(debug_assertions)]
fn canonical(p: &Path) -> PathBuf {
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
        if self.config.fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
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
        match event {
            WindowEvent::CloseRequested => self.exit = true,
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => self.resized(),
            WindowEvent::Focused(f) => self.input.set_focused(f),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.input.key_event(code, event.state, event.repeat);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => self.input.mouse_button(button, state),
            WindowEvent::CursorMoved { position, .. } => {
                self.input.cursor_moved(position.x, position.y)
            }
            WindowEvent::CursorLeft { .. } => self.input.cursor_left(),
            WindowEvent::MouseWheel { delta, .. } => match delta {
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
