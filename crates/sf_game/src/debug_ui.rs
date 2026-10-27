//! The F1 debug panels (WP1.11), built with egui.
//!
//! Layering: `sf_render` paints ([`sf_render::OverlayFrame`], `egui-wgpu`),
//! this module builds the panels and feeds window events (`egui`,
//! `egui-winit`), `sf_game` glues both to the game state. The panels read plain
//! data ([`PanelData`]) and edit plain data ([`PanelState`]); only
//! [`DebugUi`] touches the window, so [`run_frame`] is testable without one.
//!
//! Off by default and free while off: no egui pass runs, no window event
//! reaches egui, the renderer records no overlay pass, and the egui/winit
//! state is created on the first show.

use egui::{Align2, CollapsingHeader, Color32, RichText, Stroke, Vec2};
use sf_engine::{FrameSummary, winit};
use sf_render::scene::MeshKind;
use sf_render::{CameraConfig, DrawStats, InstanceStats, OverlayFrame};

/// What the units are drawn as (WP1.9): by the zoom, or forced for a look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnitDisplay {
    /// Models up close, icons far away, a dithered cross-fade between.
    #[default]
    Auto,
    /// Always models (the level of detail still applies).
    Models,
    /// Always icons.
    Icons,
}

/// The unit under the cursor, for the readout.
#[derive(Clone, Debug, PartialEq)]
pub struct HoveredUnit {
    /// The entity id (`index`v`generation`).
    pub id: String,
    /// What it is: the role id of the unit definitions (`light_tank`), or
    /// `placeholder <n>` for an entity the simulation did not make from them.
    pub kind: String,
    /// Current and full hit points; `None` for an entity without any (the
    /// placeholder crowd).
    pub hp: Option<(u32, u32)>,
    /// `"model"` or `"icon"`: which volume the cursor is in.
    pub via: &'static str,
    /// Name of the owner's colour.
    pub player: String,
    /// Distance in metres along the cursor ray (model) or from the eye (icon).
    pub distance: f32,
    /// Its current order in words (`idle`, `move (12.5, 3.5)`, `attack 12v0`);
    /// empty for an entity that is not a unit of the definitions.
    pub order: String,
}

/// The model/icon state of the frame, for the panel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UnitView {
    /// Share of the picture that is models; the icons have `1 - model_weight`.
    pub model_weight: f32,
    /// Camera zoom `0..=1` and the eye-to-focus distance in metres.
    pub zoom: f32,
    pub distance: f32,
    /// Apparent size of the reference unit at the focus, in pixels, and the
    /// thresholds of the cross-fade (already times the UI scale). The size is
    /// the one the blend compares, after `IconConfig::view_shrink`.
    pub reference_px: f32,
    pub model_above_px: f32,
    pub icon_below_px: f32,
    /// The camera distances (metres) where the models end and the icons
    /// begin, as `blend_for_view` switches (shrink included).
    pub models_until: f32,
    pub icons_from: f32,
    pub ui_scale: f32,
    pub hovered: Option<HoveredUnit>,
}

/// Everything the panels show, gathered by the game once per frame.
#[derive(Default)]
pub struct PanelData {
    /// Models, icons and the unit under the cursor.
    pub units: UnitView,
    /// The last frames, oldest first, in milliseconds (the graph).
    pub frame_ms: Vec<f32>,
    /// Average / p99 / max over those frames (the engine's nearest-rank
    /// percentiles), `None` before the first frame.
    pub summary: Option<FrameSummary>,
    /// Draw calls of the last rendered frame, counted at the draw calls.
    pub draw: DrawStats,
    /// Instancing counters of the last build.
    pub instances: InstanceStats,
    /// Ticks the sim has run (= the number of the next tick) and the checksum
    /// of the world right after the last one.
    pub sim_tick: u64,
    pub checksum: Option<u64>,
    pub entities: usize,
    /// The simulation thread (WP1.15): tick time and whether it keeps up.
    pub sim: SimPanel,
    /// Original Red Alert audio (WP11.7): `None` = no audio system.
    pub ra_status: Option<sf_audio::RaStatus>,
    pub ra_catalog: Option<std::sync::Arc<sf_audio::ra::Catalog>>,
    /// Why the last sound-browser play failed.
    pub ra_play_error: Option<String>,
}

/// The simulation thread as the F1 panel shows it (WP1.15).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SimPanel {
    /// How long the newest tick took, milliseconds.
    pub tick_ms: f32,
    /// The slowest of the last 20 ticks (one second), milliseconds.
    pub slowest_ms: f32,
    /// The sim is behind its 20 Hz clock now or was within the last second:
    /// the picture keeps going on the newest snapshot.
    pub behind: bool,
    /// How far behind, milliseconds (of the newest snapshot).
    pub lag_ms: f32,
    /// Times the sim gave up catching up and restarted its clock.
    pub clock_resets: u64,
    /// Commands that came for a tick that had run already.
    pub late_commands: u64,
    /// Snapshots that were published after the picture needed them.
    pub late_publishes: u64,
    /// The thread has ended (it panicked, or was stopped).
    pub stopped: bool,
    /// The effects (WP5.6a): events recorded by the sim thread, dropped by its full queue, without a
    /// position, expired when their turn came, and waiting for the picture to reach their tick.
    pub fx_recorded: u64,
    pub fx_dropped: u64,
    pub fx_unresolved: u64,
    pub fx_expired: u64,
    pub fx_pending: u32,
    /// The particle pool: alive, its size, the most alive at once, pushed out of a full pool, smoke refused.
    pub particles: u32,
    pub particle_cap: u32,
    pub particles_peak: u32,
    pub particles_dropped: u64,
    pub smoke_refused: u64,
    /// Instances drawn in the last frame, and the tracer dots among them.
    pub effect_instances: u32,
    pub tracer_dots: u32,
}

/// What the panels edit.
#[derive(Clone, Copy, Debug)]
pub struct PanelState {
    /// The live camera tunables; the game applies a change at once.
    pub camera: CameraConfig,
    /// Reset target of the camera section.
    pub camera_default: CameraConfig,
    /// LOD colouring: tint terrain chunks by their level of detail.
    pub lod_tint: bool,
    /// Cell grid (one line per 4 m cell) on the terrain, fading with the zoom.
    pub grid: bool,
    /// Sound browser: (archive, entry) whose play button was clicked.
    pub ra_play: Option<(usize, usize)>,
    /// Models, icons or by the zoom.
    pub unit_display: UnitDisplay,
}

impl PanelState {
    pub fn new(camera: CameraConfig) -> Self {
        Self {
            camera,
            camera_default: camera,
            lod_tint: false,
            grid: false,
            ra_play: None,
            unit_display: UnitDisplay::Auto,
        }
    }
}

/// Size of the frame-time graph in egui points.
const GRAPH_SIZE: Vec2 = Vec2::new(300.0, 70.0);
/// Frame times above this are drawn red (30 fps), above [`GOOD_MS`] amber.
const BAD_MS: f32 = 33.4;
const GOOD_MS: f32 = 16.8;

/// The window title of the performance panel (tests look for it).
pub const PERFORMANCE_TITLE: &str = "Performance (F1)";
/// The window title of the camera and view panel.
pub const CAMERA_TITLE: &str = "Camera & view";

/// Build both panels. Returns `true` when a camera tunable was edited.
pub fn show_panels(ctx: &egui::Context, data: &PanelData, state: &mut PanelState) -> bool {
    egui::Window::new(PERFORMANCE_TITLE)
        .anchor(Align2::LEFT_TOP, [8.0, 8.0])
        .resizable(false)
        .show(ctx, |ui| {
            CollapsingHeader::new("Frame time")
                .default_open(true)
                .show(ui, |ui| frame_time(ui, data));
            CollapsingHeader::new("Draw calls and instances")
                .default_open(true)
                .show(ui, |ui| draw_calls(ui, data));
            CollapsingHeader::new("Units: LOD, icons, hover")
                .default_open(true)
                .show(ui, |ui| unit_levels(ui, data));
            CollapsingHeader::new("Simulation")
                .default_open(true)
                .show(ui, |ui| simulation(ui, data));
        });
    let mut camera_changed = false;
    egui::Window::new(CAMERA_TITLE)
        .anchor(Align2::RIGHT_TOP, [-8.0, 8.0])
        .resizable(false)
        .show(ctx, |ui| {
            CollapsingHeader::new("Camera")
                .default_open(true)
                .show(ui, |ui| camera_changed = camera_controls(ui, state));
            CollapsingHeader::new("View")
                .default_open(true)
                .show(ui, |ui| view_toggles(ui, state));
        });
    crate::sound_browser::show(ctx, data, state);
    camera_changed
}

fn frame_time(ui: &mut egui::Ui, data: &PanelData) {
    match &data.summary {
        Some(s) => {
            ui.label(format!(
                "{:.0} fps  avg {:.2} ms  p99 {:.2} ms  max {:.2} ms",
                s.fps(),
                s.avg.as_secs_f64() * 1e3,
                s.p99.as_secs_f64() * 1e3,
                s.max.as_secs_f64() * 1e3,
            ));
            ui.label(format!("last {} frames", s.count));
        }
        None => {
            ui.label("no frames yet");
        }
    }
    frame_graph(ui, &data.frame_ms);
}

/// One bar per frame, green / amber / red, with 60 and 30 fps guide lines.
fn frame_graph(ui: &mut egui::Ui, frames_ms: &[f32]) {
    let (rect, _) = ui.allocate_exact_size(GRAPH_SIZE, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, Color32::from_black_alpha(140));
    // The scale is fixed to at least 40 ms so a calm graph does not look noisy.
    let top = frames_ms.iter().copied().fold(40.0_f32, f32::max);
    let y_of = |ms: f32| rect.bottom() - (ms / top).clamp(0.0, 1.0) * rect.height();
    for guide in [GOOD_MS, BAD_MS] {
        let y = y_of(guide);
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            Stroke::new(1.0, Color32::from_white_alpha(60)),
        );
    }
    painter.text(
        rect.left_top() + Vec2::new(3.0, 1.0),
        Align2::LEFT_TOP,
        format!("{top:.0} ms"),
        egui::FontId::monospace(10.0),
        Color32::from_white_alpha(140),
    );
    let step = rect.width() / frames_ms.len().max(1) as f32;
    for (i, &ms) in frames_ms.iter().enumerate() {
        let colour = if ms > BAD_MS {
            Color32::from_rgb(230, 70, 60)
        } else if ms > GOOD_MS {
            Color32::from_rgb(235, 190, 60)
        } else {
            Color32::from_rgb(90, 200, 110)
        };
        let x = rect.left() + (i as f32 + 0.5) * step;
        painter.line_segment(
            [egui::pos2(x, rect.bottom()), egui::pos2(x, y_of(ms))],
            Stroke::new(step.clamp(1.0, 4.0), colour),
        );
    }
}

fn draw_calls(ui: &mut egui::Ui, data: &PanelData) {
    let d = &data.draw;
    ui.label(format!(
        "{} draw calls: units {}, scene {}, terrain {}, cell overlay {}",
        d.dynamic_draw_calls
            + d.scene_draw_calls
            + d.terrain_draw_calls
            + d.cell_overlay_draw_calls,
        d.dynamic_draw_calls,
        d.scene_draw_calls,
        d.terrain_draw_calls,
        d.cell_overlay_draw_calls,
    ));
    ui.label(format!(
        "effects: {} instances in {} draw call",
        d.effect_instances, d.effect_draw_calls
    ));
    ui.label(format!(
        "terrain chunks drawn {} / {}, {} triangles",
        d.terrain_chunks_visible, d.terrain_chunks_total, d.terrain_triangles
    ));
    let levels: Vec<String> = d
        .terrain_chunks_per_level
        .iter()
        .map(u32::to_string)
        .collect();
    ui.label(format!("chunks per level 0..5: {}", levels.join(" / ")));
    let i = &data.instances;
    ui.label(format!(
        "unit instances visible {} / {} ({} / {} tiles)",
        i.visible, i.total, i.tiles_visible, i.tiles_total
    ));
    egui::Grid::new("instances_per_mesh")
        .num_columns(3)
        .show(ui, |ui| {
            ui.label(RichText::new("mesh").weak());
            ui.label(RichText::new("visible").weak());
            ui.label(RichText::new("total").weak());
            ui.end_row();
            for kind in MeshKind::ALL {
                let (visible, total) = (i.per_mesh[kind.index()], i.total_per_mesh[kind.index()]);
                if total == 0 {
                    continue;
                }
                ui.label(format!("{kind:?}"));
                ui.label(visible.to_string());
                ui.label(total.to_string());
                ui.end_row();
            }
        });
}

/// Levels of detail, the cross-fade to icons and the hovered unit (WP1.9).
fn unit_levels(ui: &mut egui::Ui, data: &PanelData) {
    let (i, u, d) = (&data.instances, &data.units, &data.draw);
    ui.label(format!(
        "models {} (level 0..3: {} / {} / {} / {}), icons {}",
        i.models, i.per_lod[0], i.per_lod[1], i.per_lod[2], i.per_lod[3], i.icons
    ));
    ui.label(format!(
        "unit draw calls {} models + {} icons, {} triangles",
        d.dynamic_draw_calls, d.icon_draw_calls, d.dynamic_triangles
    ));
    ui.label(format!(
        "cross-fade: models {:.0} %, icons {:.0} %",
        u.model_weight * 100.0,
        (1.0 - u.model_weight) * 100.0
    ));
    ui.add(egui::ProgressBar::new(1.0 - u.model_weight).desired_width(220.0));
    ui.label(format!(
        "zoom {:.2}, camera {:.0} m, reference unit {:.1} px",
        u.zoom, u.distance, u.reference_px
    ));
    ui.label(format!(
        "models above {:.0} px (to {:.0} m), icons below {:.0} px (from {:.0} m), UI scale {:.2}",
        u.model_above_px, u.models_until, u.icon_below_px, u.icons_from, u.ui_scale
    ));
    match &u.hovered {
        Some(h) => {
            ui.label(format!(
                "hovering: {} {} ({}) by {}, {:.1} m, {}",
                h.kind,
                h.id,
                h.player,
                h.via,
                h.distance,
                hp_text(h.hp)
            ));
            if !h.order.is_empty() {
                ui.label(format!("order: {}", h.order));
            }
        }
        None => {
            ui.label("hovering: none");
        }
    };
}

/// "HP 40/50", or "no hit points" for an entity that has none.
fn hp_text(hp: Option<(u32, u32)>) -> String {
    match hp {
        Some((now, full)) => format!("HP {now}/{full}"),
        None => "no hit points".to_owned(),
    }
}

fn simulation(ui: &mut egui::Ui, data: &PanelData) {
    ui.label(format!("ticks run {}", data.sim_tick));
    match data.checksum {
        Some(c) => ui.label(format!("checksum {c:016x}")),
        None => ui.label("checksum -"),
    };
    ui.label(format!("{} entities", data.entities));
    let sim = &data.sim;
    ui.label(format!(
        "sim thread: tick {:.1} ms, slowest of the last second {:.1} ms (budget 50)",
        sim.tick_ms, sim.slowest_ms
    ));
    if sim.behind {
        ui.colored_label(
            egui::Color32::from_rgb(255, 170, 60),
            format!("sim behind by {:.0} ms (frames carry on)", sim.lag_ms),
        );
    }
    if sim.stopped {
        ui.colored_label(egui::Color32::from_rgb(255, 90, 90), "sim thread stopped");
    }
    if sim.clock_resets + sim.late_commands + sim.late_publishes > 0 {
        ui.label(format!(
            "clock restarts {}, late commands {}, late snapshots {}",
            sim.clock_resets, sim.late_commands, sim.late_publishes
        ));
    }
    if sim.fx_recorded > 0 || sim.particles_peak > 0 {
        ui.label(format!(
            "effects: {} drawn ({} tracer dots), pool {}/{} (peak {}, pushed out {}, smoke refused {})",
            sim.effect_instances,
            sim.tracer_dots,
            sim.particles,
            sim.particle_cap,
            sim.particles_peak,
            sim.particles_dropped,
            sim.smoke_refused
        ));
        ui.label(format!(
            "effect events: {} recorded, queue dropped {}, no position {}, expired {}, waiting {}",
            sim.fx_recorded, sim.fx_dropped, sim.fx_unresolved, sim.fx_expired, sim.fx_pending
        ));
    }
}

/// The camera tunables as sliders; `true` when one moved.
fn camera_controls(ui: &mut egui::Ui, state: &mut PanelState) -> bool {
    let mut changed = false;
    let c = &mut state.camera;
    changed |= ui
        .add(egui::Slider::new(&mut c.distance_per_notch, 0.5..=0.97).text("zoom step per notch"))
        .on_hover_text("One wheel notch multiplies the distance by this: lower zooms faster.")
        .changed();
    changed |= ui
        .add(egui::Slider::new(&mut c.zoom_smooth_secs, 0.005..=0.6).text("zoom damping (s)"))
        .changed();
    changed |= ui
        .add(egui::Slider::new(&mut c.pan_speed, 0.1..=2.5).text("pan speed"))
        .on_hover_text("Distances per second: scales with the zoom.")
        .changed();
    changed |= ui
        .add(egui::Slider::new(&mut c.pan_smooth_secs, 0.005..=0.6).text("pan damping (s)"))
        .changed();
    // Pitch in degrees; the camera stores radians.
    for (label, value) in [
        ("pitch zoomed in (deg)", &mut c.pitch_near),
        ("pitch zoomed out (deg)", &mut c.pitch_far),
    ] {
        let mut degrees = value.to_degrees();
        if ui
            .add(egui::Slider::new(&mut degrees, 15.0..=89.0).text(label))
            .changed()
        {
            *value = degrees.to_radians();
            changed = true;
        }
    }
    if ui.button("Reset camera").clicked() {
        state.camera = state.camera_default;
        changed = true;
    }
    changed
}

fn view_toggles(ui: &mut egui::Ui, state: &mut PanelState) {
    ui.checkbox(&mut state.lod_tint, "LOD colouring")
        .on_hover_text("Tints terrain chunks by level of detail: red = full detail, then orange, yellow, green, blue, purple (coarsest).");
    ui.checkbox(&mut state.grid, "Cell grid (4 m)")
        .on_hover_text("Fades out when zoomed far out; chunk borders (32 m) are yellow.");
    ui.horizontal(|ui| {
        ui.label("Units:");
        ui.radio_value(&mut state.unit_display, UnitDisplay::Auto, "by zoom")
            .on_hover_text("Models up close, icons far away, a dithered cross-fade between.");
        ui.radio_value(&mut state.unit_display, UnitDisplay::Models, "models");
        ui.radio_value(&mut state.unit_display, UnitDisplay::Icons, "icons");
    });
}

/// Run one egui pass for `input` and tessellate it. Returns the frame to
/// paint, what egui wants from the platform (cursor, text), and whether a
/// camera tunable was edited. No window involved.
pub fn run_frame(
    ctx: &egui::Context,
    input: egui::RawInput,
    data: &PanelData,
    state: &mut PanelState,
) -> (OverlayFrame, egui::PlatformOutput, bool) {
    let mut camera_changed = false;
    let output = ctx.run_ui(input, |ui| {
        camera_changed |= show_panels(ui.ctx(), data, state);
    });
    let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
    let frame = OverlayFrame {
        jobs,
        textures_delta: output.textures_delta,
        pixels_per_point: output.pixels_per_point,
    };
    (frame, output.platform_output, camera_changed)
}

/// The panels plus their window integration.
pub struct DebugUi {
    visible: bool,
    ctx: egui::Context,
    /// Created on the first show (needs the window).
    winit: Option<egui_winit::State>,
    pub state: PanelState,
}

impl DebugUi {
    pub fn new(camera: CameraConfig) -> Self {
        Self {
            visible: false,
            ctx: egui::Context::default(),
            winit: None,
            state: PanelState::new(camera),
        }
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    /// F1: flip the panels. Returns the new state.
    pub fn toggle(&mut self) -> bool {
        self.set_visible(!self.visible);
        self.visible
    }

    /// Show or hide. Hiding drops all egui state (pointer interaction
    /// included): egui gets no events while hidden, so a slider held when F1
    /// was pressed would otherwise never see its release and keep owning
    /// the pointer.
    pub fn set_visible(&mut self, visible: bool) {
        if self.visible && !visible {
            self.ctx = egui::Context::default();
            self.winit = None;
        }
        self.visible = visible;
    }

    /// A dragged slider owns the pointer even outside the panel.
    pub fn using_pointer(&self) -> bool {
        self.visible && self.ctx.egui_is_using_pointer()
    }

    /// The pointer is over a panel (or a widget is held): hovering the
    /// units behind it would be wrong.
    pub fn pointer_over_panel(&self) -> bool {
        self.visible && self.ctx.egui_wants_pointer_input()
    }

    fn winit_state(&mut self, window: &winit::window::Window) -> &mut egui_winit::State {
        self.winit.get_or_insert_with(|| {
            let mut state = egui_winit::State::new(
                self.ctx.clone(),
                egui::ViewportId::ROOT,
                window,
                Some(window.scale_factor() as f32),
                window.theme(),
                None,
            );
            // The window is focused already; winit only reports changes.
            state.egui_input_mut().focused = true;
            state
        })
    }

    /// Offer a window event to egui first. `true` = egui used it (the pointer
    /// is over a panel or a slider is held, or a text field has the keyboard);
    /// the engine then keeps it from the camera. Nothing happens while the
    /// panels are hidden.
    pub fn on_event(
        &mut self,
        window: &winit::window::Window,
        event: &winit::event::WindowEvent,
    ) -> bool {
        if !self.visible {
            return false;
        }
        self.winit_state(window)
            .on_window_event(window, event)
            .consumed
    }

    /// Run the panels for this frame and return what to paint, or `None`
    /// while hidden (then egui does not run at all). The second value is
    /// `true` when a camera tunable changed.
    pub fn frame(
        &mut self,
        window: &winit::window::Window,
        data: &PanelData,
    ) -> Option<(OverlayFrame, bool)> {
        if !self.visible {
            return None;
        }
        let ctx = self.ctx.clone();
        let mut state = self.state;
        let winit = self.winit_state(window);
        let input = winit.take_egui_input(window);
        let (frame, platform, camera_changed) = run_frame(&ctx, input, data, &mut state);
        winit.handle_platform_output(window, platform);
        self.state = state;
        Some((frame, camera_changed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn screen() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 720.0),
            )),
            ..Default::default()
        }
    }

    fn sample_data() -> PanelData {
        let frames: Vec<f32> = (0..300).map(|i| 10.0 + (i % 7) as f32 * 5.0).collect();
        let mut per_mesh = [0; MeshKind::ALL.len()];
        let mut total_per_mesh = [0; MeshKind::ALL.len()];
        per_mesh[MeshKind::Cube.index()] = 40;
        total_per_mesh[MeshKind::Cube.index()] = 700;
        total_per_mesh[MeshKind::Cone.index()] = 650;
        PanelData {
            summary: Some(FrameSummary {
                count: frames.len(),
                avg: Duration::from_micros(16_700),
                p50: Duration::from_micros(16_000),
                p95: Duration::from_micros(30_000),
                p99: Duration::from_micros(41_000),
                max: Duration::from_micros(55_000),
            }),
            frame_ms: frames,
            draw: DrawStats {
                dynamic_instances: 40,
                dynamic_draw_calls: 2,
                scene_instances: 5,
                scene_draw_calls: 3,
                terrain_draw_calls: 4,
                dynamic_triangles: 1280,
                icon_instances: 30,
                icon_draw_calls: 1,
                terrain_chunks_total: 64,
                terrain_chunks_visible: 12,
                terrain_triangles: 9_216,
                terrain_chunks_per_level: [4, 5, 3, 0, 0, 0],
                cell_overlay_draw_calls: 0,
                ..DrawStats::default()
            },
            instances: InstanceStats {
                total: 1350,
                visible: 40,
                tiles_total: 60,
                tiles_visible: 9,
                per_mesh,
                total_per_mesh,
                per_lod: [20, 12, 8, 0],
                models: 40,
                icons: 30,
                ..InstanceStats::default()
            },
            units: UnitView {
                model_weight: 0.75,
                zoom: 0.62,
                distance: 96.0,
                reference_px: 21.5,
                model_above_px: 28.0,
                icon_below_px: 14.0,
                models_until: 80.0,
                icons_from: 160.0,
                ui_scale: 1.0,
                hovered: Some(HoveredUnit {
                    id: "12v3".into(),
                    kind: "light_tank".into(),
                    hp: Some((320, 400)),
                    via: "model",
                    player: "blue".into(),
                    distance: 41.25,
                    order: "move (12.5, 3.5)".into(),
                }),
            },
            sim_tick: 1234,
            checksum: Some(0x00de_adbe_ef12_3456),
            entities: 1350,
            ..PanelData::default()
        }
    }

    /// All text drawn by a pass, in paint order.
    fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        fn walk(shape: &egui::epaint::Shape, out: &mut Vec<String>) {
            match shape {
                egui::epaint::Shape::Text(t) => out.push(t.galley.text().to_owned()),
                egui::epaint::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        shapes.iter().for_each(|c| walk(&c.shape, &mut out));
        out
    }

    /// One settled pass: egui lays a window out invisibly the first time it
    /// sees it, so a warm-up pass runs first.
    fn pass(ctx: &egui::Context, data: &PanelData, state: &mut PanelState) -> (Vec<String>, bool) {
        let mut warm_up = ctx.run_ui(screen(), |ui| {
            show_panels(ui.ctx(), data, state);
        });
        warm_up.textures_delta.clear();
        let mut changed = false;
        let out = ctx.run_ui(screen(), |ui| {
            changed = show_panels(ui.ctx(), data, state);
        });
        let t = texts(&out.shapes);
        // `TexturesDelta` asserts (debug) that it was handled before dropping.
        let mut deltas = out.textures_delta;
        deltas.clear();
        (t, changed)
    }

    #[test]
    fn builds_every_panel_with_the_expected_widgets() {
        let ctx = egui::Context::default();
        let mut state = PanelState::new(CameraConfig::default());
        let (texts, changed) = pass(&ctx, &sample_data(), &mut state);
        // A second pass lets the windows lay out (the first only measures).
        let (texts2, _) = pass(&ctx, &sample_data(), &mut state);
        assert!(!changed);
        let all = texts.iter().chain(&texts2).cloned().collect::<Vec<_>>();
        let has = |needle: &str| all.iter().any(|t| t.contains(needle));
        for needle in [
            PERFORMANCE_TITLE,
            CAMERA_TITLE,
            "Frame time",
            "Draw calls and instances",
            "Simulation",
            "Camera",
            "View",
            "avg 16.70 ms",
            "p99 41.00 ms",
            "last 300 frames",
            "terrain chunks drawn 12 / 64, 9216 triangles",
            "chunks per level 0..5: 4 / 5 / 3 / 0 / 0 / 0",
            "unit instances visible 40 / 1350",
            "9 / 60 tiles",
            "ticks run 1234",
            "checksum 00deadbeef123456",
            "LOD colouring",
            "Cell grid (4 m)",
            "Units: LOD, icons, hover",
            "models 40 (level 0..3: 20 / 12 / 8 / 0), icons 30",
            "unit draw calls 2 models + 1 icons, 1280 triangles",
            "cross-fade: models 75 %, icons 25 %",
            "zoom 0.62, camera 96 m, reference unit 21.5 px",
            "models above 28 px (to 80 m), icons below 14 px (from 160 m)",
            "hovering: light_tank 12v3 (blue) by model, 41.2 m, HP 320/400",
            "by zoom",
            "zoom step per notch",
            "pan speed",
            "pitch zoomed in (deg)",
            "Reset camera",
        ] {
            assert!(has(needle), "missing {needle:?} in {all:?}");
        }
        // Only mesh kinds that exist are listed.
        assert!(has("Cube") && has("Cone") && !has("Pyramid"));
    }

    #[test]
    fn the_hovered_unit_is_named_with_its_hit_points() {
        let ctx = egui::Context::default();
        let mut state = PanelState::new(CameraConfig::default());
        let mut data = sample_data();
        // An entity without hit points (the placeholder crowd).
        if let Some(h) = data.units.hovered.as_mut() {
            h.kind = "placeholder 2".into();
            h.hp = None;
        }
        let (texts, _) = pass(&ctx, &data, &mut state);
        assert!(
            texts
                .iter()
                .any(|t| t.contains("hovering: placeholder 2 12v3 (blue)")
                    && t.contains("no hit points")),
            "{texts:?}"
        );
        assert_eq!(hp_text(Some((0, 50))), "HP 0/50");
    }

    #[test]
    fn the_panel_says_when_the_sim_is_behind() {
        let ctx = egui::Context::default();
        let mut state = PanelState::new(CameraConfig::default());
        let mut data = sample_data();
        // Keeping up: the tick time is there, no warning.
        data.sim = SimPanel {
            tick_ms: 13.2,
            slowest_ms: 22.0,
            ..SimPanel::default()
        };
        let (texts, _) = pass(&ctx, &data, &mut state);
        assert!(
            texts
                .iter()
                .any(|t| t.contains("sim thread: tick 13.2 ms") && t.contains("22.0 ms")),
            "{texts:?}"
        );
        assert!(!texts.iter().any(|t| t.contains("sim behind")), "{texts:?}");

        // Behind: it says so, and that the frames carry on.
        data.sim.behind = true;
        data.sim.lag_ms = 120.0;
        data.sim.clock_resets = 2;
        data.sim.late_commands = 1;
        let (texts, _) = pass(&ctx, &data, &mut state);
        assert!(
            texts
                .iter()
                .any(|t| t.contains("sim behind by 120 ms") && t.contains("frames carry on")),
            "{texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t.contains("clock restarts 2, late commands 1")),
            "{texts:?}"
        );

        data.sim.stopped = true;
        let (texts, _) = pass(&ctx, &data, &mut state);
        assert!(texts.iter().any(|t| t.contains("sim thread stopped")));
    }

    /// The effects lines show the event queue's drop counter and the pool's counters (WP5.6a).
    #[test]
    fn the_panel_shows_the_effects_counters() {
        let ctx = egui::Context::default();
        let mut state = PanelState::new(CameraConfig::default());
        let mut data = sample_data();
        let (texts, _) = pass(&ctx, &data, &mut state);
        assert!(
            !texts.iter().any(|t| t.contains("effect events")),
            "nothing to say yet: {texts:?}"
        );
        data.sim.fx_recorded = 120;
        data.sim.fx_dropped = 7;
        data.sim.particles = 300;
        data.sim.particle_cap = 4096;
        data.sim.particles_peak = 900;
        data.sim.effect_instances = 310;
        data.sim.tracer_dots = 10;
        let (texts, _) = pass(&ctx, &data, &mut state);
        assert!(
            texts
                .iter()
                .any(|t| t.contains("120 recorded, queue dropped 7")),
            "{texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t.contains("310 drawn (10 tracer dots), pool 300/4096 (peak 900")),
            "{texts:?}"
        );
    }

    #[test]
    fn graph_handles_no_frames() {
        let ctx = egui::Context::default();
        let mut state = PanelState::new(CameraConfig::default());
        let (texts, _) = pass(&ctx, &PanelData::default(), &mut state);
        assert!(texts.iter().any(|t| t.contains("no frames yet")));
        assert!(texts.iter().any(|t| t.contains("checksum -")));
        assert!(texts.iter().any(|t| t.contains("hovering: none")));
    }

    #[test]
    fn the_unit_display_radio_changes_the_state() {
        // Click the "icons" radio button: the state follows.
        let ctx = egui::Context::default();
        let mut state = PanelState::new(CameraConfig::default());
        assert_eq!(state.unit_display, UnitDisplay::Auto);
        let data = sample_data();
        for _ in 0..2 {
            let out = ctx.run_ui(screen(), |ui| {
                show_panels(ui.ctx(), &data, &mut state);
            });
            let mut td = out.textures_delta;
            td.clear();
        }
        // Find the radio's label and click on it (the whole widget responds).
        fn find(shape: &egui::epaint::Shape, at: &mut Option<egui::Pos2>) {
            match shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == "icons" => {
                    *at = Some(t.pos + t.galley.size() * 0.5);
                }
                egui::epaint::Shape::Vec(v) => v.iter().for_each(|s| find(s, at)),
                _ => {}
            }
        }
        let out = ctx.run_ui(screen(), |ui| {
            show_panels(ui.ctx(), &data, &mut state);
        });
        let mut at = None;
        out.shapes.iter().for_each(|c| find(&c.shape, &mut at));
        let mut td = out.textures_delta;
        td.clear();
        let at = at.expect("the radio is drawn");
        for pressed in [None, Some(true), Some(false)] {
            let mut input = screen();
            input.events = match pressed {
                None => vec![egui::Event::PointerMoved(at)],
                Some(p) => vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: p,
                    modifiers: egui::Modifiers::NONE,
                }],
            };
            let out = ctx.run_ui(input, |ui| {
                show_panels(ui.ctx(), &data, &mut state);
            });
            let mut td = out.textures_delta;
            td.clear();
        }
        assert_eq!(state.unit_display, UnitDisplay::Icons);
    }

    #[test]
    fn a_hidden_panel_runs_no_egui_pass() {
        // `DebugUi::frame` needs a window, so check the gate it is built on:
        // the pass counter only moves when `run_frame` is called.
        let mut ui = DebugUi::new(CameraConfig::default());
        assert!(!ui.visible());
        assert!(!ui.using_pointer());
        assert_eq!(ui.ctx.cumulative_pass_nr(), 0);
        assert!(ui.toggle());
        assert!(ui.visible());
        assert!(!ui.toggle());
        assert!(!ui.visible());
        assert_eq!(
            ui.ctx.cumulative_pass_nr(),
            0,
            "toggling alone runs no pass"
        );

        let mut state = ui.state;
        let mut last = None;
        // egui lays windows out invisibly on their first pass: run two.
        for _ in 0..2 {
            let (mut frame, _, changed) = run_frame(&ui.ctx, screen(), &sample_data(), &mut state);
            assert!(!changed);
            frame.textures_delta.clear();
            last = Some(frame.jobs.len());
        }
        assert!(last.unwrap() > 0, "visible panels paint something");
        assert_eq!(ui.ctx.cumulative_pass_nr(), 2);
    }

    /// Centre of the "Reset camera" button after the windows settled.
    fn settle_and_find_reset(ctx: &egui::Context, state: &mut PanelState) -> egui::Pos2 {
        fn find(shape: &egui::epaint::Shape, at: &mut Option<egui::Pos2>) {
            match shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == "Reset camera" => {
                    *at = Some(t.pos + t.galley.size() * 0.5);
                }
                egui::epaint::Shape::Vec(v) => v.iter().for_each(|s| find(s, at)),
                _ => {}
            }
        }
        let data = sample_data();
        let mut at = None;
        for _ in 0..2 {
            let out = ctx.run_ui(screen(), |ui| {
                show_panels(ui.ctx(), &data, state);
            });
            at = None;
            out.shapes.iter().for_each(|c| find(&c.shape, &mut at));
            let mut td = out.textures_delta;
            td.clear();
        }
        at.expect("the reset button is drawn")
    }

    #[test]
    fn hiding_mid_press_releases_the_pointer() {
        let mut ui = DebugUi::new(CameraConfig::default());
        ui.set_visible(true);
        let mut state = ui.state;
        let at = settle_and_find_reset(&ui.ctx, &mut state);
        // Press and hold on a widget: egui now owns the pointer.
        let data = sample_data();
        for events in [
            vec![egui::Event::PointerMoved(at)],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        ] {
            let mut input = screen();
            input.events = events;
            let out = ui.ctx.run_ui(input, |u| {
                show_panels(u.ctx(), &data, &mut state);
            });
            let mut td = out.textures_delta;
            td.clear();
        }
        assert!(ui.using_pointer(), "a held widget owns the pointer");
        // F1 mid-press: hide, then show again. The release never arrived.
        assert!(!ui.toggle());
        assert!(!ui.using_pointer());
        assert!(ui.toggle());
        assert!(
            !ui.using_pointer(),
            "a stale press must not block the camera after reopening"
        );
    }

    #[test]
    fn clicking_reset_restores_the_default_camera() {
        // Drive the real widget: press the button in the second pass.
        let ctx = egui::Context::default();
        let mut state = PanelState::new(CameraConfig::default());
        state.camera.pan_speed = 2.0;
        let data = sample_data();
        // Find the button by painting and locating its text galley.
        let mut warm_up = ctx.run_ui(screen(), |ui| {
            show_panels(ui.ctx(), &data, &mut state);
        });
        warm_up.textures_delta.clear();
        let out = ctx.run_ui(screen(), |ui| {
            show_panels(ui.ctx(), &data, &mut state);
        });
        let mut button = None;
        fn find(shape: &egui::epaint::Shape, at: &mut Option<egui::Pos2>) {
            match shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == "Reset camera" => {
                    *at = Some(t.pos + t.galley.size() * 0.5);
                }
                egui::epaint::Shape::Vec(v) => v.iter().for_each(|s| find(s, at)),
                _ => {}
            }
        }
        out.shapes.iter().for_each(|c| find(&c.shape, &mut button));
        let mut td = out.textures_delta;
        td.clear();
        let at = button.expect("the reset button is drawn");

        let mut changed = false;
        for events in [
            vec![egui::Event::PointerMoved(at)],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ] {
            let mut input = screen();
            input.events = events;
            let out = ctx.run_ui(input, |ui| {
                changed |= show_panels(ui.ctx(), &data, &mut state);
            });
            let mut td = out.textures_delta;
            td.clear();
        }
        assert!(changed, "the click was reported as a camera change");
        assert_eq!(state.camera.pan_speed, CameraConfig::default().pan_speed);
    }
}
