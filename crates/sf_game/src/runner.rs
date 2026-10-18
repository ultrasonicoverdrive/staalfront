//! The application: [`Runner`] is the `sf_engine` app of both binaries.
//!
//! It owns the sim world, the camera, the instancer, the HUD and the F1
//! panels, steps the sim in the engine's fixed tick, draws every frame and,
//! depending on the options, flies the benchmark path ([`crate::bench`]) or
//! takes the CI screenshots. See the crate docs for what each part does.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sf_audio::ra::{AudioConfig, RaAudioConfig};
use sf_audio::{AudioSystem, Listener};
use sf_data::map::{Map, generate::generate_cells};
use sf_data::units::UnitDefs;
use sf_engine::winit::event::MouseButton;
use sf_engine::{Action, App, Context, Resized, winit};
use sf_math::Fx;
use sf_math::render_bridge::{METRES_PER_CELL, vec2_metres_to_cells};
use sf_render::camera::DVec2;
use sf_render::glb::{self, LoadedPack};
use sf_render::scene::world_markers;
use sf_render::{
    Blend, Camera, CameraConfig, CameraInput, DebugView, EyeView, FrameView, Frustum, GpuOptions,
    Ground, IconAtlas, InstanceStats, Instancer, LodConfig, ModelSet, Motion, Pick, PickQuery,
    PickVia, RectQuery, Renderer, ScreenRect, Style, TerrainGround, TerrainStats, UnitId,
};
use sf_sim::{EntityView, World, WorldConfig};

use crate::args::{BenchOptions, Options, Scene};
use crate::bench::{self, FrameSample, Recorder, Report};
use crate::commands;
use crate::cursor::{self, CursorArt, CursorKind, Hover, Modifiers};
use crate::debug_ui::{DebugUi, HoveredUnit, PanelData, UnitDisplay, UnitView};
use crate::hud::{Hud, HudStats};
use crate::looks::Looks;
use crate::population::Population;
use crate::roster::{self, Roster};
use crate::selection::{Gesture, Pointer, Recall, Selection};
use crate::selection_view::{self, FrameGeometry, SelectionView};

/// Walking speed of the demo's units: 1.5 cells a second, 6 m/s (22 km/h),
/// a believable tank. The sim's default of 4 cells a second was a walking
/// speed when a cell was a metre; at 4 m per cell it would be 58 km/h.
const DEMO_SPEED: Fx = Fx::from_ratio(3, 2);

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

/// How much bigger than `units.ron` the placeholder shapes are drawn: that
/// file is about a metre (the days of a 1 m cell), a vehicle on a 4 m cell is
/// three to four.
const PLACEHOLDER_SCALE: f32 = 3.0;

/// The placeholder look: `units.ron` at vehicle size.
fn placeholder_style(style: Style) -> Style {
    style.scaled(PLACEHOLDER_SCALE)
}

/// Frames to let the camera settle before a screenshot is taken.
const SHOT_SETTLE_FRAMES: u32 = 20;

/// The screenshots of `--shots`: name, zoom (0 = closest, 1 = whole map), and
/// whether the F1 panels are open in it.
const SHOTS: [(&str, f64, bool); 3] = [
    ("close_up", 0.10, false),
    ("middle", 0.45, true),
    ("whole_map", 1.0, false),
];

/// State of the benchmark flight.
struct BenchRun {
    opts: BenchOptions,
    rec: Recorder,
    /// Frames rendered so far, warm-up included.
    frame: u32,
    /// When the measured flight began.
    flight_start: Option<Instant>,
    /// The report has been written; the runner is leaving.
    done: bool,
    /// Why there are no GPU times, if there are none.
    gpu_note: Option<String>,
    gpu_enabled: bool,
    allocs_seen: u64,
}

/// State of the `--shots` run.
struct ShotRun {
    dir: PathBuf,
    /// Index into [`SHOTS`].
    step: usize,
    settled: u32,
    requested: bool,
}

pub struct Runner {
    started: Option<Instant>,
    report_at: Instant,
    ticks: u32,
    frames: u32,
    total_ticks: u64,
    exit_after: Option<Duration>,
    renderer: Option<Renderer>,
    /// The strategic icons (WP3.6), read from `assets/icons` at start and
    /// uploaded once when the renderer exists.
    icons: IconAtlas,
    audio: Option<AudioSystem>,
    /// Why the last sound-browser play failed (shown in the browser).
    ra_play_error: Option<String>,
    camera: Camera,
    hud: Hud,
    /// Frames counted in the last full second, for the top bar.
    fps_shown: u32,
    map: Map,
    /// The terrain as a ground: camera picking, units and sounds stand on it.
    ground: Arc<TerrainGround>,
    lod: LodConfig,
    world: World,
    instancer: Instancer,
    /// The glTF models the units are drawn with, after the placeholders.
    pack: Option<LoadedPack>,
    /// How an entity is drawn (and named in the F1 panel).
    looks: Looks,
    buildings: u32,
    /// Map size in sim cells (B12): what the sim was created with.
    cells: (u32, u32),
    views: Vec<EntityView>,
    units: u32,
    /// Latest instancing and draw counters, for the once-a-second line.
    stats: InstanceStats,
    terrain: TerrainStats,
    draw_calls: u32,
    frame_ms: Samples,
    build_ms: Samples,
    pick_ms: Samples,
    upload_ms: Samples,
    /// Recording, submitting and presenting the frame (waiting for the
    /// swap chain included).
    render_ms: Samples,
    tick_ms: Samples,
    last_frame: Option<Instant>,
    auto_zoom: bool,
    /// The unit under the cursor in the last frame, highlighted in the
    /// picture and named in the F1 panel; a click selects it.
    hovered: Option<Pick>,
    /// Model/icon state of the last frame, for the F1 panel.
    unit_view: UnitView,
    /// `--hover x,y`: pretend the cursor is here (pixels), for screenshots
    /// taken without a mouse.
    hover_at: Option<(f32, f32)>,
    /// F1 panels (hidden by default).
    debug: DebugUi,
    /// Window size in pixels, to rebuild the camera when its tunables change.
    viewport: (u32, u32),
    /// What the command line asked for.
    options: Options,
    bench: Option<BenchRun>,
    shots: Option<ShotRun>,
    /// Time spent in `fixed_update` since the last frame (the bench's CPU
    /// time counts the sim too).
    sim_since_frame: Duration,
    /// The selected units, the inspected one and the control groups (WP3.3):
    /// presentation state that only ever produces orders.
    selection: Selection,
    /// The select button's click and drag recogniser.
    pointer: Pointer,
    /// Rings, health bars and the drag rectangle.
    selection_view: SelectionView,
    /// Scratch for what a drag rectangle or a double-click finds.
    hits: Vec<UnitId>,
    /// What is under the mouse this frame: the entity, the ground point and
    /// the screen edges (WP3.5). One value feeds both the cursor and the
    /// right-click, so they cannot disagree.
    hover: Hover,
    /// The cursor the mouse showed in the last frame.
    cursor_kind: CursorKind,
    /// The software cursors; `None` in the crowd scene (the engine demo and
    /// its benchmark show no game cursor) and when the pictures could not be
    /// read (the operating system's cursor stays).
    cursor_art: Option<CursorArt>,
    /// The operating system's cursor is hidden right now (only ever while
    /// the software cursor is drawn).
    os_cursor_hidden: bool,
    /// Set when the renderer could not start or a frame failed; `main` then
    /// exits with a non-zero code.
    failed: Arc<AtomicBool>,
}

/// The map of the options: a directory, or a generated square of cells.
pub fn make_map(options: &Options) -> Result<Map, String> {
    match &options.map_dir {
        Some(dir) => {
            Map::load(std::path::Path::new(dir)).map_err(|e| format!("cannot load map {dir}: {e}"))
        }
        None => {
            let started = Instant::now();
            let map = generate_cells(options.map_cells, options.map_cells, 1);
            println!(
                "map: generated {0}x{0} cells = {1}x{1} m in {2:.0} ms",
                options.map_cells,
                map.width(),
                started.elapsed().as_secs_f64() * 1e3
            );
            Ok(map)
        }
    }
}

impl Runner {
    pub fn new(
        options: Options,
        map: Map,
        lod: LodConfig,
        pack: Option<LoadedPack>,
        failed: Arc<AtomicBool>,
    ) -> Self {
        let now = Instant::now();
        // B12: the sim counts cells, the map and everything drawn metres.
        let cells = map.size_in_cells();
        let config = WorldConfig {
            speed: DEMO_SPEED,
            ..WorldConfig::new(1).with_map_size(cells.0, cells.1)
        };
        // WP2.1: the map's terrain types go to the sim (open ground if it has
        // no terrain layer); nothing is drawn from them yet (WP2.2 to WP2.4).
        let world = crate::playfield::make_world(config, &map);
        let style = placeholder_style(Style::embedded());
        let icons = load_icons();
        let start = match options.scene {
            Scene::Crowd => crowd_scene(world, &options, pack.as_ref(), cells),
            Scene::Sandbox => sandbox_scene(world, &options, &style, &icons, cells),
        };
        let SceneStart {
            world,
            looks,
            views,
            units,
            buildings,
        } = start;

        let map_metres = (map.width() as f32, map.height() as f32);
        let mut instancer = Instancer::new(style.clone(), map_metres);
        apply_looks(&mut instancer, &looks, pack.as_ref(), style);
        if pack.is_some() || looks.roster().is_some_and(|r| r.pack().is_some()) {
            instancer.set_aim(Some([0.5 * map_metres.0, 0.5 * map_metres.1]));
        }
        instancer.update_tick(views.iter().map(|v| motion_of(&looks, v)));
        let ground = Arc::new(TerrainGround::new(&map));
        instancer.set_height_range(ground.min_height() as f32, ground.max_height() as f32);
        let camera_config = CameraConfig {
            map_size: DVec2::new(f64::from(map.width()), f64::from(map.height())),
            ..CameraConfig::default()
        };
        let mut camera = Camera::new(camera_config, 1280, 720);
        camera.set_ground(ground.clone());
        let bench = options.bench.clone().map(|opts| BenchRun {
            opts,
            rec: Recorder::default(),
            frame: 0,
            flight_start: None,
            done: false,
            gpu_note: None,
            gpu_enabled: false,
            allocs_seen: 0,
        });
        let shots = options.shots.clone().map(|dir| ShotRun {
            dir,
            step: 0,
            settled: 0,
            requested: false,
        });
        let mut debug = DebugUi::new(camera_config);
        debug.set_visible(options.panels);
        debug.state.grid = options.grid;
        debug.state.lod_tint = options.lod_tint;
        debug.state.unit_display = options.unit_display;
        if let Some(zoom) = options.zoom {
            camera.set_zoom(zoom);
        }
        if let Some((x, z)) = options.focus {
            camera.set_focus(DVec2::new(x, z));
        }
        let assets_root = sf_engine::AssetRoot::resolve().root().to_path_buf();
        let mut hud = Hud::new(&assets_root);
        // The game cursor belongs to the sandbox, the scene with selection and
        // orders; the benchmark and the engine demo's crowd show none.
        let cursor_art = (options.scene == Scene::Sandbox && options.bench.is_none())
            .then(|| CursorArt::load(&mut hud, &assets_root.join("ui/cursors")))
            .and_then(|loaded| {
                loaded
                    .map_err(|e| eprintln!("sf_game: cursors: {e}; keeping the system cursor"))
                    .ok()
            });
        Self {
            started: None,
            report_at: now + Duration::from_secs(1),
            ticks: 0,
            frames: 0,
            total_ticks: 0,
            exit_after: options.exit_after,
            renderer: None,
            icons,
            audio: None,
            ra_play_error: None,
            camera,
            hud,
            fps_shown: 0,
            map,
            ground,
            lod,
            world,
            instancer,
            pack,
            looks,
            buildings,
            cells,
            views,
            units,
            stats: InstanceStats::default(),
            terrain: TerrainStats::default(),
            draw_calls: 0,
            frame_ms: Samples::default(),
            build_ms: Samples::default(),
            pick_ms: Samples::default(),
            upload_ms: Samples::default(),
            render_ms: Samples::default(),
            tick_ms: Samples::default(),
            last_frame: None,
            auto_zoom: options.auto_zoom,
            hovered: None,
            unit_view: UnitView::default(),
            hover_at: options.hover,
            debug,
            viewport: (1280, 720),
            options,
            bench,
            shots,
            sim_since_frame: Duration::ZERO,
            selection: Selection::new(commands::LOCAL_PLAYER),
            pointer: Pointer::default(),
            selection_view: SelectionView::default(),
            hits: Vec::new(),
            hover: Hover::default(),
            cursor_kind: CursorKind::Select,
            cursor_art,
            os_cursor_hidden: false,
            failed,
        }
    }
}

/// An order in words, for the F1 panel: `idle`, `move (x, y)`, `attack 12v0`.
fn order_text(order: sf_sim::Order, world: &World) -> String {
    use sf_sim::{GuardTarget, Order};
    let point = |p: sf_math::Vec2| {
        let [x, y] = sf_math::render_bridge::vec2_to_f32(p);
        format!("({x:.1}, {y:.1})")
    };
    let id_text = |id: sf_sim::EntityId| {
        if world.is_alive(id) {
            id.to_string()
        } else {
            format!("{id} (gone)")
        }
    };
    match order {
        Order::Idle => "idle".to_owned(),
        Order::Move(p) => format!("move {}", point(p)),
        Order::Attack(id) => format!("attack {}", id_text(id)),
        Order::AttackMove(p) => format!("attack-move {}", point(p)),
        Order::Guard(GuardTarget::Point(p)) => format!("guard {}", point(p)),
        Order::Guard(GuardTarget::Entity(id)) => format!("guard {}", id_text(id)),
        Order::Scatter => "scatter".to_owned(),
        Order::HoldPosition => "hold position".to_owned(),
    }
}

/// A sim entity as the instancer wants it. The positions are in cells;
/// `Motion::from_fixed` is where they become metres (B12).
fn motion_of(looks: &Looks, v: &EntityView) -> Motion {
    Motion::from_fixed(
        looks.shape_of(v),
        v.owner.0,
        v.prev_pos,
        v.pos,
        v.prev_facing,
        v.facing,
    )
    .with_id(UnitId::from_parts(v.id.index(), v.id.generation()))
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
        // The UI saw the mouse first: a wheel over it is not a zoom (the
        // `CameraZoom*` actions fire on the raw wheel, so skip them too).
        let wheel_blocked = self.hud.wheel_consumed() && wheel != 0.0;
        let (zoom_notches, cursor) = if wheel_blocked {
            (0.0, cursor)
        } else if self.auto_zoom {
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
            drag: actions.held(Action::CameraDrag) && !ui_busy && !self.hud.middle_captured(),
        }
    }

    /// Milliseconds since the app started: the clock of double-clicks and
    /// double taps.
    fn now_ms(&self) -> u64 {
        self.started.map_or(0, |s| {
            s.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
        })
    }

    /// The entities whose picture touches `rect` in the frame just drawn.
    fn entities_in(&mut self, rect: ScreenRect) -> Vec<sf_sim::EntityId> {
        let (width, height) = self.viewport;
        let query = RectQuery {
            rect,
            view_proj: self.camera.view_proj(),
            viewport: [width as f32, height as f32],
        };
        self.instancer.pick_rect(&query, &mut self.hits);
        self.hits
            .iter()
            .map(|&id| selection_view::entity_of(id))
            .collect()
    }

    /// The select button and the group keys (WP3.3, [`crate::selection`]).
    /// A click picks the unit under the cursor (`hovered`, the picture's own
    /// answer for models and icons), a drag takes what the rectangle touches,
    /// a double-click what is of the same kind on the screen; shift adds or
    /// toggles; the number keys recall control groups and control + number
    /// assigns them. Which key or button each is lives in
    /// `assets/config/input.ron` (`SelectClick`, `SelectAdd`, `GroupAssign`,
    /// `Group1` to `Group9`). Selecting changes nothing in the world.
    fn handle_selection(&mut self, ctx: &Context) {
        let actions = ctx.actions();
        let ui_busy = self.hud.pointer_over_ui() || self.debug.pointer_over_panel();
        let cursor = ctx.input().cursor().map(|(x, y)| [x as f32, y as f32]);
        let now = self.now_ms();
        let scale = self.hud.scale();
        if actions.pressed(Action::SelectClick)
            && !ui_busy
            && let Some(at) = cursor
        {
            self.pointer.press(at);
        }
        if let Some(at) = cursor {
            self.pointer.moved(at, scale);
        }
        // A press and release inside one frame (a quick click on a slow
        // machine) shows as a pulse and never as held, so a button that is
        // not held while a press is being tracked counts as released.
        if actions.released(Action::SelectClick)
            || (self.pointer.is_down() && !actions.held(Action::SelectClick))
        {
            let additive = actions.held(Action::SelectAdd);
            let gesture = match cursor {
                Some(at) => self.pointer.release(at, now, scale),
                None => {
                    self.pointer.cancel();
                    None
                }
            };
            let hit = self
                .hovered
                .as_ref()
                .map(|p| selection_view::entity_of(p.id));
            match gesture {
                Some(Gesture::Click { double: false }) => {
                    self.selection.click(&self.world, hit, additive);
                }
                Some(Gesture::Click { double: true }) => {
                    let (width, height) = self.viewport;
                    let screen = ScreenRect::viewport([width as f32, height as f32]);
                    let in_view = self.entities_in(screen);
                    self.selection
                        .select_same_kind(&self.world, hit, &in_view, additive);
                }
                Some(Gesture::Drag(rect)) => {
                    let found = self.entities_in(rect);
                    self.selection.drag(&self.world, &found, additive);
                }
                None => {}
            }
        }
        let assign = actions.held(Action::GroupAssign);
        for (n, action) in Action::GROUPS.into_iter().enumerate() {
            if !actions.pressed(action) {
                continue;
            }
            if assign {
                self.selection.assign_group(n);
                println!("group {}: {} units", n + 1, self.selection.group(n).len());
            } else if let Recall::Centre([x, y]) = self.selection.recall_group(&self.world, n, now)
            {
                self.camera.set_focus(DVec2::new(
                    f64::from(x * METRES_PER_CELL),
                    f64::from(y * METRES_PER_CELL),
                ));
            }
        }
    }

    /// What the mouse is over this frame, the cursor that goes with it and
    /// its drawing (WP3.5, [`crate::cursor`]). `at` is the mouse in pixels
    /// (`None` over the UI, an F1 panel or outside the window): the operating
    /// system's cursor stays visible there and the software cursor is not
    /// drawn. The hover is also what a right-click orders from
    /// ([`Runner::handle_orders`]), so the picture and the click share one
    /// decision.
    fn update_cursor(&mut self, ctx: &Context, at: Option<(f32, f32)>) {
        if self.bench.is_some() {
            return;
        }
        self.hover = Hover {
            entity: self
                .hovered
                .as_ref()
                .map(|p| selection_view::entity_of(p.id)),
            point: at
                .and_then(|(x, y)| {
                    self.camera
                        .ground_point(DVec2::new(f64::from(x), f64::from(y)))
                })
                .map(|g| vec2_metres_to_cells([g.x as f32, g.y as f32])),
            edges: ctx.actions().edges(),
        };
        self.cursor_kind = cursor::cursor_context(
            &self.world,
            &self.selection,
            &self.hover,
            Modifiers::from_actions(ctx.actions()),
        );
        let drawn = match (&self.cursor_art, at) {
            (Some(art), Some(pos)) => {
                self.hud
                    .draw_cursor(art.set(), art.sprite_of(self.cursor_kind), pos);
                true
            }
            _ => false,
        };
        // The system cursor is hidden only while ours is drawn over the game
        // view; over a panel, the minimap or outside the window it is back.
        if drawn != self.os_cursor_hidden {
            ctx.window().set_cursor_visible(!drawn);
            self.os_cursor_hidden = drawn;
        }
    }

    /// The keys and clicks that give orders (WP3.4, [`crate::commands`]):
    /// move, queue, stop, scatter and hold position, to the selected units
    /// that can take them. Which key or button each is lives in
    /// `assets/config/input.ron` (`OrderMove`, `OrderQueue`, `OrderStop`,
    /// `OrderScatter`, `OrderHold`). They only schedule commands for the next
    /// tick; the simulation does the rest.
    fn handle_orders(&mut self, ctx: &Context) {
        let actions = ctx.actions();
        let queued = actions.held(Action::OrderQueue);
        let ui_busy = self.hud.pointer_over_ui() || self.debug.pointer_over_panel();
        // A right-click gives the order the cursor shows (WP3.5): the same
        // `cursor_context` decides both. Over the UI there is no hover, so
        // nothing is ordered.
        if actions.pressed(Action::OrderMove)
            && !ui_busy
            && let Some(command) = commands::right_click(
                &self.world,
                &self.selection,
                &self.hover,
                Modifiers::from_actions(actions),
            )
        {
            commands::issue(&mut self.world, command);
        }
        if actions.pressed(Action::OrderStop) {
            let ids = self.selection.orderable(&self.world);
            if let Some(command) = commands::stop_command(&ids) {
                commands::issue(&mut self.world, command);
            }
        }
        if actions.pressed(Action::OrderScatter) {
            let ids = self.selection.orderable(&self.world);
            if let Some(command) = commands::scatter_command(&ids, queued) {
                commands::issue(&mut self.world, command);
            }
        }
        if actions.pressed(Action::OrderHold) {
            let ids = self.selection.orderable(&self.world);
            if let Some(command) = commands::hold_command(&ids, queued) {
                commands::issue(&mut self.world, command);
            }
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
            units: self.unit_view.clone(),
            sim_tick: self.world.tick().0,
            checksum: self.world.checksum_history().last().copied(),
            entities: self.world.len(),
            ra_status: self.audio.as_ref().map(AudioSystem::original_audio_status),
            ra_catalog: self
                .audio
                .as_ref()
                .and_then(AudioSystem::original_audio_catalog),
            ra_play_error: self.ra_play_error.clone(),
        }
    }

    /// Sound browser: play the entry whose button was clicked (one short
    /// AUD decode, on click only).
    fn play_browser_sound(&mut self, catalog: Option<&sf_audio::ra::Catalog>) {
        let Some((a, e)) = self.debug.state.ra_play.take() else {
            return;
        };
        let entry = catalog.and_then(|c| c.archives.get(a)?.entries.get(e));
        if let (Some(entry), Some(audio)) = (entry, &mut self.audio) {
            self.ra_play_error = audio.play_original_entry(entry).err();
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
            self.play_browser_sound(data.ra_catalog.as_deref());
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
    /// Build this frame's models and icons (level of detail per unit, the
    /// cross-fade by zoom), pick the unit under the cursor in what was built
    /// and highlight it. A click on it selects it ([`Runner::handle_selection`]).
    fn build_units(&mut self, alpha: f32, cursor: Option<(f32, f32)>) {
        let view_proj = self.camera.view_proj();
        let (width, height) = self.viewport;
        let frustum = Frustum::from_view_proj(view_proj);
        let eye = EyeView::from_view_proj(view_proj, height as f32);
        let scale = self.hud.scale();
        let icons = *self.instancer.icon_config();
        let distance = self.camera.distance() as f32;
        let blend = match self.debug.state.unit_display {
            UnitDisplay::Auto => icons.blend_for_view(
                eye.focal_px,
                distance,
                self.camera.max_distance() as f32,
                scale,
            ),
            UnitDisplay::Models => Blend::MODELS,
            UnitDisplay::Icons => Blend::ICONS,
        };

        // Cull by tile, interpolate, put the units on the ground, choose the
        // level of detail and the model or icon.
        let started = Instant::now();
        let ground = &self.ground;
        self.stats = self.instancer.build_frame(
            &FrameView::new(&frustum, alpha, eye, blend, scale),
            |x, z| ground.height(f64::from(x), f64::from(z)) as f32,
        );
        self.build_ms.push(started.elapsed());

        // Hover: the model's box up close, the icon's quad far away. A model
        // behind terrain that the cursor ray meets first is hidden by it.
        let started = Instant::now();
        let size = [width as f32, height as f32];
        self.hovered = cursor.and_then(|(x, y)| {
            let query = PickQuery::new([x, y], view_proj, size).with_ground(&**ground);
            self.instancer.pick(&query)
        });
        if let Some(pick) = &self.hovered {
            self.instancer.highlight(pick);
        }
        self.pick_ms.push(started.elapsed());

        let hovered = self.hovered.as_ref().map(|p| {
            let view = self
                .views
                .iter()
                .find(|v| v.id.index() == p.id.index() && v.id.generation() == p.id.generation());
            let owner = view.map(|v| v.owner.0);
            // The role's name and its hit points (current and full) for an
            // entity the simulation made from the unit definitions.
            let kind = view.map_or_else(String::new, |v| self.looks.name_of(v));
            let hp = view.filter(|v| v.has_stats).map(|v| {
                let full = self.world.stats_of(v.id).map_or(v.hp, |s| s.hp);
                (v.hp, full)
            });
            HoveredUnit {
                id: p.id.to_string(),
                kind,
                hp,
                via: match p.via {
                    PickVia::Model => "model",
                    PickVia::Icon => "icon",
                },
                player: owner.map_or_else(String::new, |o| {
                    self.instancer.style().color_name(o).to_owned()
                }),
                distance: p.distance,
                order: view
                    .and_then(|v| self.world.order_of(v.id))
                    .map_or_else(String::new, |o| order_text(o, &self.world)),
            }
        });
        let (above, below) = (icons.model_above_px * scale, icons.icon_below_px * scale);
        // The band as `blend_for_view` applies it (with its shrink on a map
        // whose whole-map view would still show models).
        let band = icons.view_band(
            eye.focal_px,
            distance,
            self.camera.max_distance() as f32,
            scale,
        );
        self.unit_view = UnitView {
            model_weight: blend.model(),
            zoom: self.camera.zoom() as f32,
            distance,
            reference_px: band.reference_px,
            model_above_px: above,
            icon_below_px: below,
            models_until: band.models_until,
            icons_from: band.icons_from,
            ui_scale: scale,
            hovered,
        };
    }

    /// The selection feedback of this frame (WP3.3): the selected icons get
    /// their outline (before the icons are uploaded), and the rings, health
    /// bars and drag rectangle go on the UI's world layer. With nothing
    /// selected and no drag it does nothing.
    fn mark_selection(&mut self) {
        let dragging = self.pointer.drag_rect();
        if self.selection.is_empty() && self.selection.inspected().is_none() && dragging.is_none() {
            return;
        }
        let (width, height) = self.viewport;
        let scale = self.hud.scale();
        let frame = FrameGeometry {
            view_proj: self.camera.view_proj(),
            viewport: [width as f32, height as f32],
            ui_scale: scale,
            blend: self.instancer.blend(),
            icon_px: self.instancer.icon_params().size_px,
        };
        let mut layer = self.hud.world_layer();
        self.selection_view.mark(
            &mut self.instancer,
            &self.selection,
            &self.world,
            &frame,
            &mut layer,
        );
        if let Some(rect) = dragging {
            selection_view::draw_drag_rect(&mut layer, rect, scale);
        }
    }

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
            .and_then(|text| Style::from_ron(&text).map_err(|e| e.to_string()))
            .map(placeholder_style);
        match result {
            Ok(style) => {
                // Keep the models; take the new palette and settings.
                apply_looks(&mut self.instancer, &self.looks, self.pack.as_ref(), style);
                println!("reloaded {}", units.display());
            }
            Err(e) => eprintln!(
                "sf_game: keeping the old unit style, {}: {e}",
                units.display()
            ),
        }
    }
}

impl Runner {
    /// The camera pose the benchmark or the screenshot run dictates for this
    /// frame, if one of them is running.
    fn scripted_pose(&mut self, now: Instant) -> Option<(f64, (f64, f64), usize)> {
        let map = (f64::from(self.map.width()), f64::from(self.map.height()));
        if let Some(run) = &mut self.bench {
            let warmup = run.frame < bench::WARMUP_FRAMES;
            let progress = if warmup {
                0.0
            } else {
                let start = *run.flight_start.get_or_insert(now);
                match run.opts.frames {
                    Some(n) => run.rec.frames.len() as f64 / f64::from(n),
                    None => now.duration_since(start).as_secs_f64() / run.opts.seconds,
                }
            };
            let pose = bench::pose_at(progress, map);
            return Some((pose.zoom, pose.focus, pose.segment));
        }
        if let Some(shot) = &self.shots {
            let zoom = SHOTS[shot.step.min(SHOTS.len() - 1)].1;
            return Some((zoom, (map.0 * 0.5, map.1 * 0.5), 0));
        }
        None
    }

    /// One frame of the `--shots` run: let the camera settle, capture the
    /// frame, write it, go to the next zoom level. Returns `false` when the
    /// run is over.
    fn shots_after_render(&mut self) -> bool {
        let Some(shot) = &mut self.shots else {
            return true;
        };
        let Some(renderer) = &mut self.renderer else {
            return true;
        };
        if !shot.requested {
            shot.settled += 1;
            return true;
        }
        let name = SHOTS[shot.step].0;
        let Some(image) = renderer.take_capture() else {
            eprintln!("sf_game: --shots: the window could not be captured ({name})");
            self.failed.store(true, Ordering::Relaxed);
            return false;
        };
        let path = shot.dir.join(format!("{name}.png"));
        if let Err(e) = std::fs::create_dir_all(&shot.dir)
            .map_err(|e| e.to_string())
            .and_then(|()| sf_render::image::write_png(&path, &image).map_err(|e| e.to_string()))
        {
            eprintln!("sf_game: --shots: cannot write {}: {e}", path.display());
            self.failed.store(true, Ordering::Relaxed);
            return false;
        }
        println!(
            "shot: {} ({}x{}, {} units in view, {} draw calls)",
            path.display(),
            image.width,
            image.height,
            self.stats.visible,
            self.draw_calls
        );
        shot.step += 1;
        shot.settled = 0;
        shot.requested = false;
        shot.step < SHOTS.len()
    }

    /// Ask for the capture once the camera has settled on the step's pose.
    fn shots_before_render(&mut self) {
        let Some(shot) = &mut self.shots else { return };
        let panels = SHOTS[shot.step.min(SHOTS.len() - 1)].2;
        if self.debug.visible() != panels {
            self.debug.set_visible(panels);
            if !panels && let Some(renderer) = &mut self.renderer {
                renderer.clear_overlay();
            }
        }
        if shot.settled >= SHOT_SETTLE_FRAMES
            && !shot.requested
            && let Some(renderer) = &mut self.renderer
        {
            renderer.capture_next_frame();
            shot.requested = true;
        }
    }

    /// Record the finished frame of the benchmark; write the report after
    /// the last one.
    fn bench_after_render(&mut self, ctx: &mut Context, started: Instant, segment: usize) {
        let Some(run) = &mut self.bench else { return };
        let Some(renderer) = &mut self.renderer else {
            return;
        };
        run.frame += 1;
        let gpu = renderer.take_gpu_frame_times();
        let sim_ms = std::mem::take(&mut self.sim_since_frame).as_secs_f64() * 1e3;
        let wait = renderer.last_acquire_wait().as_secs_f64() * 1e3;
        let callback_ms = started.elapsed().as_secs_f64() * 1e3;
        let allocs = renderer.alloc_stats().instance_buffers;
        let stats = renderer.draw_stats();
        if run.frame <= bench::WARMUP_FRAMES {
            run.allocs_seen = allocs;
            return;
        }
        // The interval to the previous frame's start (the first measured
        // frame follows the last warm-up frame).
        let frame_ms = self.frame_ms.0.last().copied().unwrap_or(callback_ms);
        let last = |s: &Samples| s.0.last().copied().unwrap_or(0.0);
        run.rec.push(FrameSample {
            frame_ms,
            cpu_ms: (sim_ms + callback_ms - wait).max(0.0),
            build_ms: last(&self.build_ms),
            upload_ms: last(&self.upload_ms),
            render_ms: (last(&self.render_ms) - wait).max(0.0),
            sim_ms,
            units_in_view: self.stats.visible,
            draw_calls: stats.total_draw_calls(),
            triangles: stats.terrain_triangles + stats.dynamic_triangles,
            buffer_allocs: (allocs - run.allocs_seen) as u32,
            segment: segment as u8,
        });
        run.allocs_seen = allocs;
        run.rec.push_gpu(gpu, bench::WARMUP_FRAMES);
        let finished = match run.opts.frames {
            Some(n) => run.rec.frames.len() >= n as usize,
            None => run
                .flight_start
                .is_some_and(|s| s.elapsed().as_secs_f64() >= run.opts.seconds),
        };
        if finished && !run.done {
            run.done = true;
            self.finish_bench(ctx);
        }
    }

    /// Write `bench.json` and leave.
    fn finish_bench(&mut self, ctx: &mut Context) {
        // The checksum after every 100th tick: `history[k]` is the world after
        // the (k + 1)-th tick, so the list reads "after N ticks".
        if let Some(run) = &mut self.bench {
            let history = self.world.checksum_history();
            run.rec.checksums = (100..=history.len())
                .step_by(100)
                .map(|n| (n as u64, history[n - 1]))
                .collect();
        }
        let Some(run) = &self.bench else { return };
        let Some(renderer) = &self.renderer else {
            return;
        };
        let screen = ctx.screen_report();
        let size = ctx.size();
        let requested = match self.options.screen {
            sf_engine::ScreenMode::Windowed => "windowed".to_owned(),
            sf_engine::ScreenMode::Borderless => "borderless fullscreen".to_owned(),
            sf_engine::ScreenMode::Fixed { width, height } => {
                format!("{width}x{height} fullscreen")
            }
        };
        let adapter = renderer.adapter_summary();
        let software = adapter.contains("Cpu") || adapter.to_lowercase().contains("warp");
        let present = renderer.present_info();
        let facts = bench::RunFacts {
            host: bench::Host {
                os: std::env::consts::OS,
                arch: std::env::consts::ARCH,
                adapter,
                backend_env: std::env::var("WGPU_BACKEND").ok(),
            },
            display: bench::Display {
                requested,
                actual: renderer.size(),
                screen_mode: screen.mode.to_owned(),
                monitor: screen.monitor.clone(),
                monitor_size: screen.monitor_size,
                refresh_hz: (screen.refresh_mhz > 0)
                    .then(|| f64::from(screen.refresh_mhz) / 1000.0),
                note: screen.note.clone().or_else(|| {
                    let fullscreen = screen.mode != "windowed";
                    let wanted = match self.options.screen {
                        sf_engine::ScreenMode::Fixed { width, height } => Some((width, height)),
                        _ => screen.monitor_size,
                    };
                    (fullscreen && Some(renderer.size()) != wanted).then(|| {
                        format!(
                            "fullscreen was requested but the frames are {}x{} (the window \
                             system did not grant it)",
                            size.width, size.height
                        )
                    })
                }),
            },
            present: bench::Present {
                mode: present.mode.clone(),
                vsync: present.mode == "Fifo",
                requested: match self.options.present {
                    sf_render::Present::Vsync => "vsync",
                    sf_render::Present::Uncapped => "uncapped",
                },
                note: present.note.clone(),
            },
            scene: bench::Scene {
                units: self.units,
                buildings: self.buildings,
                models: self.options.models.clone(),
                map_cells: self.cells,
                map_metres: (self.map.width(), self.map.height()),
                metres_per_cell: sf_math::CELL_METRES,
                instance_buffers_reserved: self.options.reserve,
            },
            flight: bench::FlightInfo {
                progress_by: if run.opts.frames.is_some() {
                    "frames"
                } else {
                    "time"
                },
                seconds: run.opts.seconds,
                frames_limit: run.opts.frames,
                warmup_frames: bench::WARMUP_FRAMES,
                segments: bench::SEGMENTS.iter().map(|s| s.name.to_owned()).collect(),
            },
            gpu_note: run.gpu_note.clone(),
            gpu_enabled: run.gpu_enabled,
            software_adapter: software,
            ticks: self.world.tick().0,
        };
        let report = Report::build(&run.rec, facts);
        match std::fs::write(&run.opts.out, report.to_json()) {
            Ok(()) => println!("bench: wrote {}", run.opts.out.display()),
            Err(e) => {
                eprintln!("sf_game: cannot write {}: {e}", run.opts.out.display());
                self.failed.store(true, Ordering::Relaxed);
            }
        }
        if let Some(s) = &report.frame_ms {
            println!(
                "bench: {} frames, frame ms p50 {:.2} p95 {:.2} p99 {:.2} max {:.2}; \
                 cpu p99 {}; gpu {}; hitches > {} ms: {}",
                s.samples,
                s.p50,
                s.p95,
                s.p99,
                s.max,
                report
                    .cpu_ms
                    .as_ref()
                    .map_or("n/a".to_owned(), |c| format!("{:.2}", c.p99)),
                report
                    .gpu_ms
                    .stats
                    .as_ref()
                    .map_or("n/a".to_owned(), |g| format!("p99 {:.2}", g.p99)),
                bench::HITCH_MS,
                report.hitches.count,
            );
        }
        for caveat in &report.criterion.caveats {
            println!("bench: note: {caveat}");
        }
        ctx.request_exit();
    }
}

impl App for Runner {
    fn on_start(&mut self, ctx: &mut Context) {
        let now = Instant::now();
        self.started = Some(now);
        self.report_at = now + Duration::from_secs(1);
        let s = ctx.size();
        let screen = ctx.screen_report();
        println!(
            "window {}x{} px ({}{}), scale factor {}, assets {}",
            s.width,
            s.height,
            screen.mode,
            screen
                .note
                .as_ref()
                .map_or(String::new(), |n| format!(", {n}")),
            ctx.scale_factor(),
            ctx.assets().root().display()
        );
        let mut audio = AudioSystem::new(ctx.assets().root());
        // Original Red Alert audio (WP11.7): config file, `--ra-audio` wins.
        let (mut audio_config, warning) = AudioConfig::load(ctx.assets().root());
        if let Some(w) = warning {
            eprintln!("sf_audio: {w}");
        }
        if let Some(path) = self.options.ra_audio.clone() {
            audio_config.ra_audio = RaAudioConfig {
                enabled: true,
                path,
            };
        }
        audio.enable_original_audio(&audio_config.ra_audio);
        println!(
            "audio: {}",
            if audio.is_silent() {
                "silent (no device)"
            } else {
                "device open"
            }
        );
        self.audio = Some(audio);
        let gpu_options = GpuOptions {
            present: self.options.present,
            // Screenshots copy the window frame out; nothing else needs it.
            capture: self.options.shots.is_some(),
            ..GpuOptions::window()
        };
        println!(
            "renderer: starting ({:?}, {:?})",
            gpu_options.backends, gpu_options.power_preference
        );
        match Renderer::for_window(ctx.window().clone(), s.width, s.height, &gpu_options) {
            Ok(mut renderer) => {
                let present = renderer.present_info();
                println!(
                    "renderer: {} ({:?}, present {}{})",
                    renderer.adapter_summary(),
                    renderer.color_format(),
                    present.mode,
                    present
                        .note
                        .as_ref()
                        .map_or(String::new(), |n| format!(", NOTE: {n}"))
                );
                // The icon pictures, once.
                renderer.set_icon_atlas(&self.icons);
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
                if self.options.reserve {
                    let started = Instant::now();
                    renderer.reserve_units(&self.instancer);
                    let a = renderer.alloc_stats();
                    println!(
                        "renderer: reserved {} instance buffers, {:.1} MB, in {:.1} ms",
                        a.instance_buffers,
                        a.instance_bytes as f64 / 1e6,
                        started.elapsed().as_secs_f64() * 1e3
                    );
                }
                if let Some(run) = &mut self.bench {
                    run.gpu_enabled = renderer.enable_gpu_timing();
                    if !run.gpu_enabled {
                        run.gpu_note = Some(format!(
                            "{} has no timestamp queries",
                            renderer.adapter_summary()
                        ));
                    }
                    println!(
                        "bench: flight of {}, warm-up {} frames, GPU timing {}",
                        match run.opts.frames {
                            Some(n) => format!("{n} frames"),
                            None => format!("{:.0} s", run.opts.seconds),
                        },
                        bench::WARMUP_FRAMES,
                        if run.gpu_enabled { "on" } else { "n/a" }
                    );
                }
                self.camera.set_viewport(s.width, s.height);
                self.hud.attach(&renderer, &self.map);
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
        let started = Instant::now();
        self.ticks += 1;
        self.total_ticks += 1;
        self.world.step();
        // Hand the new state to the presentation: once per tick the fixed-point
        // positions (cells) are converted to f32 metres and bucketed by map tile.
        self.world.snapshot().collect_into(&mut self.views);
        let looks = &self.looks;
        self.instancer
            .update_tick(self.views.iter().map(|v| motion_of(looks, v)));
        let spent = started.elapsed();
        self.tick_ms.push(spent);
        self.sim_since_frame += spent;
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
        self.shots_before_render();
        // Build the UI first (it may move the camera on a minimap click), then
        // let the camera read the input the UI left over.
        self.selection.forget_the_dead(&self.world);
        let stats = HudStats {
            fps: self.fps_shown,
            units: self.world.len() as u32,
            selected: self.selection.len() as u32,
            inspecting: self.selection.inspected().is_some(),
            tick: self.total_ticks,
        };
        self.hud.frame(ctx, &mut self.camera, &self.views, stats);
        let scripted = self.scripted_pose(now);
        if let Some((zoom, focus, _)) = scripted {
            // The benchmark and the screenshots fly their own path; input is
            // ignored. Zoom first: setting the focus puts the pivot on the
            // ground at the zoom's clearance.
            self.camera.set_zoom(zoom);
            self.camera.set_focus(DVec2::new(focus.0, focus.1));
        } else {
            let input = self.camera_input(ctx, dt);
            self.camera.update(dt, &input);
        }
        if self.renderer.is_some() {
            let view_proj = self.camera.view_proj();
            if let Some(renderer) = &mut self.renderer {
                renderer.set_view_proj(view_proj);
            }
            // The cursor, unless the UI or an F1 panel has it.
            let blocked = self.hud.pointer_over_ui() || self.debug.pointer_over_panel();
            let cursor = self.hover_at.or_else(|| {
                ctx.input()
                    .cursor()
                    .filter(|_| !blocked)
                    .map(|(x, y)| (x as f32, y as f32))
            });
            self.build_units(alpha, cursor);
            self.mark_selection();
            self.update_cursor(ctx, cursor);
            let started = Instant::now();
            if let Some(renderer) = &mut self.renderer {
                renderer.upload_units(&self.instancer);
            }
            self.upload_ms.push(started.elapsed());
        }
        // The listener follows the camera (it does not rotate, so right = +X).
        // The camera works in metres, so does the audio.
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
            // The GPU time of this frame arrives frames later; the number
            // lets the benchmark credit it to this frame (`bench_after_render`
            // counts `run.frame` up after the draw).
            renderer.set_gpu_frame_tag(self.bench.as_ref().map_or(0, |b| u64::from(b.frame)));
            let drawing = Instant::now();
            let drawn = self.hud.draw(renderer);
            self.render_ms.push(drawing.elapsed());
            if let Err(e) = drawn {
                eprintln!("sf_game: render failed: {e}");
                self.failed.store(true, Ordering::Relaxed);
                ctx.request_exit();
            }
            // Level selection and culling happen inside render().
            self.terrain = renderer.terrain_stats();
            self.draw_calls = renderer.draw_stats().total_draw_calls();
        }
        self.handle_selection(ctx);
        self.handle_orders(ctx);
        let actions = ctx.actions();
        if actions.pressed(Action::ToggleFullscreen) {
            ctx.toggle_fullscreen();
            println!("fullscreen: {}", ctx.is_fullscreen());
        }
        // A sound where the cursor points on the terrain (the focus if it is
        // off the window or the sky), so pan and zoom can be heard: on the
        // test-sound key and on a click that the UI did not take.
        let clicked = ctx.input().button_pressed(MouseButton::Left)
            && !self.hud.pointer_over_ui()
            && !self.debug.pointer_over_panel();
        if (actions.pressed(Action::PlayTestSound) || clicked)
            && let Some(audio) = &mut self.audio
        {
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

        if let Some((_, _, segment)) = scripted
            && self.bench.is_some()
        {
            self.bench_after_render(ctx, now, segment);
        }
        if !self.shots_after_render() {
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
                     units {}/{} visible ({} models, levels {:?}, {} icons), \
                     terrain {}/{} chunks {} tris, {} draw calls",
                    self.frames,
                    self.ticks,
                    self.total_ticks,
                    f.avg.as_secs_f64() * 1e3,
                    f.p99.as_secs_f64() * 1e3,
                    f.max.as_secs_f64() * 1e3,
                    self.stats.visible,
                    self.stats.total,
                    self.stats.models,
                    self.stats.per_lod,
                    self.stats.icons,
                    self.terrain.chunks_visible,
                    self.terrain.chunks_total,
                    self.terrain.triangles,
                    self.draw_calls,
                ),
                None => println!("{} fps, {} ticks/s", self.frames, self.ticks),
            }
            self.fps_shown = self.frames;
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
        if path.file_name().is_some_and(|n| n == "ui.ron") {
            self.hud.reload_config(ctx.assets().root());
        }
        println!("asset changed: {}", path.display());
    }

    fn on_exit(&mut self, ctx: &mut Context) {
        if self.os_cursor_hidden {
            ctx.window().set_cursor_visible(true);
            self.os_cursor_hidden = false;
        }
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
            "summary: build+cull+LOD+icons ms {}",
            self.build_ms.describe(WARMUP_FRAMES)
        );
        println!(
            "summary: pick+terrain ray ms {}",
            self.pick_ms.describe(WARMUP_FRAMES)
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
        println!(
            "summary: last frame {} models (levels {:?}, {} triangles, {} draw calls), \
             {} icons ({} draw call), model weight {:.2}, hovered {}",
            self.stats.models,
            self.stats.per_lod,
            self.stats.model_triangles,
            self.stats.model_draw_calls,
            self.stats.icons,
            self.stats.icon_draw_calls,
            self.unit_view.model_weight,
            self.unit_view
                .hovered
                .as_ref()
                .map_or_else(|| "none".to_string(), |h| format!("{} via {}", h.id, h.via)),
        );
    }
}

/// Draw the units with the placeholder shapes of `base` and, with a pack,
/// the pack's models after them (shape `PLACEHOLDER_SHAPES + i` is model
/// `i`), with the palette and settings of `base`.
/// Put the look of the scene on the instancer: the crowd's placeholder shapes
/// and model pack, or the sandbox's size variants and role models.
fn apply_looks(
    instancer: &mut Instancer,
    looks: &Looks,
    crowd_pack: Option<&LoadedPack>,
    base: Style,
) {
    match looks.roster() {
        Some(roster) => {
            let style = roster.style(&base);
            apply_pack(instancer, roster.pack(), style);
        }
        None => apply_pack(instancer, crowd_pack, base),
    }
}

/// What a scene starts with.
struct SceneStart {
    world: World,
    looks: Looks,
    /// The entities after tick 0.
    views: Vec<EntityView>,
    /// Entities, as the scene counts them.
    units: u32,
    /// Buildings of the crowd's base.
    buildings: u32,
}

/// The engine demo's scene: a crowd of placeholder wanderers and, with a model
/// pack, a base of its buildings. Unchanged since WP1.14 (the M0 benchmark is
/// measured on it).
fn crowd_scene(
    mut world: World,
    options: &Options,
    pack: Option<&LoadedPack>,
    cells: (u32, u32),
) -> SceneStart {
    let population = Population::new(pack);
    let units = options.units;
    let buildings = population.spawn(&mut world, units, cells);
    // Tick 0 creates the entities; the buildings are then told to stay.
    let mut views = Vec::new();
    world.step();
    world.snapshot().collect_into(&mut views);
    population.plant_buildings(&mut world, &views);
    SceneStart {
        world,
        looks: Looks::Crowd(population),
        views,
        units,
        buildings,
    }
}

/// The sandbox (WP3.2): units and structures of each side, spawned from the
/// unit definitions, and the looks the definitions give them. Anything that
/// goes wrong (a definition file that does not load, a role without a model)
/// is reported and the game carries on with what it has.
fn sandbox_scene(
    mut world: World,
    options: &Options,
    style: &Style,
    icons: &IconAtlas,
    cells: (u32, u32),
) -> SceneStart {
    let assets = sf_engine::AssetRoot::resolve().root().to_path_buf();
    let defs = UnitDefs::load(&assets.join("config"), &assets).unwrap_or_else(|e| {
        eprintln!("sf_game: {e}; using the unit definitions compiled in");
        UnitDefs::embedded()
    });
    match sf_bridge::unit_table(&defs) {
        Ok(table) => {
            world = world
                .with_units(table)
                .expect("a world that has not started takes its unit table");
        }
        Err(e) => eprintln!("sf_game: unit table: {e}; the sandbox stays empty"),
    }
    world = crate::playfield::with_movement(world, &defs);
    let models_ron =
        std::fs::read_to_string(assets.join(roster::MODELS_FILE)).unwrap_or_else(|e| {
            eprintln!(
                "sf_game: {}: {e}; roles are drawn as shapes",
                roster::MODELS_FILE
            );
            String::new()
        });
    let use_models = options.role_models && !models_ron.is_empty();
    let roster = Roster::build(&defs, style, icons, &assets, &models_ron, use_models)
        .unwrap_or_else(|e| {
            eprintln!("sf_game: {e}; roles are drawn as shapes");
            Roster::build(&defs, style, icons, &assets, "", false).expect("shapes alone fit")
        });
    for warning in &roster.warnings {
        eprintln!("sf_game: {warning}");
    }
    let plan = crate::sandbox::schedule(&mut world, &defs);
    // Extra placeholder wanderers (`--units`), the crowd's way.
    let crowd = Population::new(None);
    crowd.spawn(&mut world, options.units, cells);
    let mut views = Vec::new();
    let report = world.step();
    world.snapshot().collect_into(&mut views);
    println!(
        "sandbox: {} structures and {} units placed ({} refused, {} left out), {} placeholder wanderers",
        plan.structures, plan.units, report.commands_ignored, plan.skipped, options.units
    );
    let units = views.len() as u32;
    SceneStart {
        world,
        looks: Looks::Roster {
            roster: Box::new(roster),
            crowd,
        },
        views,
        units,
        buildings: plan.structures,
    }
}

/// Read the icon set from `assets/icons`. What cannot be read is reported and
/// the roles that wanted it are drawn as their base shapes.
fn load_icons() -> IconAtlas {
    let dir = sf_engine::AssetRoot::resolve().root().join("icons");
    let icons = IconAtlas::load(&dir);
    for warning in icons.warnings() {
        eprintln!("sf_game: icons: {warning}");
    }
    icons
}

fn apply_pack(instancer: &mut Instancer, pack: Option<&LoadedPack>, base: Style) {
    let Some(pack) = pack else {
        instancer.set_style(base);
        return;
    };
    let combined = pack.style_after(&base).ok().and_then(|style| {
        let set = ModelSet::from_style(&base).concat(&pack.set)?;
        Some((style, set))
    });
    match combined {
        Some((style, set)) => instancer.set_style_and_models(style, set),
        None => eprintln!("sf_game: cannot use the model pack"),
    }
}

/// Load the model pack `name` from `assets/config/models.ron`. A pack that
/// does not load is an error with the loader's message.
pub fn load_pack(name: &str) -> Result<LoadedPack, String> {
    let assets = sf_engine::AssetRoot::resolve().root().to_path_buf();
    let config = assets.join(glb::MODELS_CONFIG);
    let text =
        std::fs::read_to_string(&config).map_err(|e| format!("{}: {e}", config.display()))?;
    let started = Instant::now();
    let pack = glb::load_pack(&text, name, &assets).map_err(|e| e.to_string())?;
    println!(
        "models: pack {name}, {} models, {} part meshes, loaded in {:.1} ms",
        pack.models.len(),
        pack.set.extra_meshes().len(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    Ok(pack)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::Preset;
    use sf_data::map::generate::generate_cells;
    use sf_sim::{Command, Kind, PlayerId, Tick};

    fn options(preset: Preset, line: &str) -> Options {
        Options::parse(preset, line.split_whitespace().map(str::to_owned))
    }

    fn world(cells: u32) -> World {
        let map = generate_cells(cells, cells, 1);
        let config = WorldConfig {
            speed: DEMO_SPEED,
            ..WorldConfig::new(1).with_map_size(cells, cells)
        };
        crate::playfield::make_world(config, &map)
    }

    /// The engine demo's scene is what it was before WP3.2: the crowd, made
    /// by `Population` with the test pack, no unit table, the buildings told
    /// to stay. The benchmark (milestone M0) is measured on it.
    #[test]
    fn the_demo_scene_is_the_crowd_it_always_was() {
        let o = options(Preset::Demo, "");
        assert_eq!((o.scene, o.units), (Scene::Crowd, 5000));
        let pack = load_pack("testpack").unwrap();
        let cells = (64, 64);
        let start = crowd_scene(world(64), &o, Some(&pack), cells);
        assert!(start.world.unit_table().is_empty(), "no unit table");
        assert_eq!(start.units, 5000);
        assert!(start.buildings > 0);
        assert!(start.views.iter().all(|v| !v.has_stats));

        // The same by hand, the way Runner::new did it before the scenes.
        let mut by_hand = world(64);
        let population = Population::new(Some(&pack));
        population.spawn(&mut by_hand, 5000, cells);
        let mut views = Vec::new();
        by_hand.step();
        by_hand.snapshot().collect_into(&mut views);
        population.plant_buildings(&mut by_hand, &views);
        let mut mine = start.world;
        for _ in 0..150 {
            mine.step();
            by_hand.step();
        }
        assert_eq!(mine.checksum_history(), by_hand.checksum_history());
        // Entities keep the shapes the population gave them.
        for v in &start.views {
            assert_eq!(start.looks.shape_of(v), population.shape_of(v.kind));
        }
    }

    #[test]
    fn the_sandbox_is_made_from_the_definitions_and_every_unit_has_a_shape() {
        let o = options(Preset::Game, "");
        assert_eq!(o.scene, Scene::Sandbox);
        let style = placeholder_style(Style::embedded());
        let start = sandbox_scene(world(128), &o, &style, &load_icons(), (128, 128));
        assert_eq!(start.world.unit_table().len(), 43);
        let (structures, units) = crate::sandbox::wanted(&UnitDefs::embedded());
        let all = (structures + units) as usize;
        assert_eq!(start.views.len(), all);
        assert_eq!(start.units as usize, all);
        assert_eq!(start.buildings, structures);
        let roster = start.looks.roster().expect("the sandbox has a roster");
        let shapes =
            roster.style(&style).shape_count() + roster.pack().map_or(0, |p| p.set.models().len());
        for v in &start.views {
            assert!(v.has_stats);
            assert!(usize::from(start.looks.shape_of(v)) < shapes);
            let name = start.looks.name_of(v);
            assert_eq!(Some(name.as_str()), roster.name_of(v.kind));
            // The hit points the panel shows are the definition's.
            assert_eq!(Some(v.hp), start.world.stats_of(v.id).map(|s| s.hp));
        }
        // The same start every time.
        let again = sandbox_scene(world(128), &o, &style, &load_icons(), (128, 128));
        let (mut a, mut b) = (start.world, again.world);
        for _ in 0..30 {
            a.step();
            b.step();
        }
        assert_eq!(a.checksum_history(), b.checksum_history());
    }

    #[test]
    fn extra_placeholder_wanderers_join_the_sandbox_and_are_not_roles() {
        let o = options(Preset::Game, "--units 200");
        let style = placeholder_style(Style::embedded());
        let mut start = sandbox_scene(world(128), &o, &style, &load_icons(), (128, 128));
        let all = {
            let (a, b) = crate::sandbox::wanted(&UnitDefs::embedded());
            (a + b) as usize
        };
        assert_eq!(start.views.len(), all + 200);
        let (roles, crowd): (Vec<&EntityView>, Vec<&EntityView>) =
            start.views.iter().partition(|v| v.has_stats);
        assert_eq!((roles.len(), crowd.len()), (all, 200));
        // The wanderers keep the demo's shapes, whatever role shares their kind.
        for v in &crowd {
            assert!(v.kind.0 < 3);
            assert_eq!(usize::from(start.looks.shape_of(v)), usize::from(v.kind.0));
            assert_eq!(start.looks.name_of(v), format!("placeholder {}", v.kind.0));
        }
        // And they wander; the roles do not.
        let before = start.views.clone();
        for _ in 0..40 {
            start.world.step();
        }
        start.world.snapshot().collect_into(&mut start.views);
        for (a, b) in before.iter().zip(&start.views) {
            assert_eq!(a.pos != b.pos, !a.has_stats, "{:?}", a.kind);
        }
    }

    #[test]
    fn models_none_draws_the_roles_as_shapes() {
        let o = options(Preset::Game, "--models none");
        let style = placeholder_style(Style::embedded());
        let start = sandbox_scene(world(128), &o, &style, &load_icons(), (128, 128));
        let roster = start.looks.roster().unwrap();
        assert!(roster.pack().is_none());
        assert!(!start.views.is_empty());
    }

    #[test]
    fn a_unit_can_still_be_spawned_into_the_sandbox_through_the_queue() {
        let o = options(Preset::Game, "");
        let style = placeholder_style(Style::embedded());
        let mut start = sandbox_scene(world(128), &o, &style, &load_icons(), (128, 128));
        let tick = start.world.tick();
        let before = start.world.len();
        start
            .world
            .schedule(
                tick,
                PlayerId(0),
                Command::SpawnUnit {
                    kind: Kind(20),
                    pos: sf_math::Vec2::from_ints(3, 3),
                    owner: PlayerId(0),
                },
            )
            .unwrap();
        assert_eq!(tick, Tick(1));
        let report = start.world.step();
        assert_eq!(report.commands_executed, 1);
        assert_eq!(start.world.len(), before + 1);
    }
}
