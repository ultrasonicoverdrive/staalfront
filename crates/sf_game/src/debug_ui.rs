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

/// Everything the panels show, gathered by the game once per frame.
#[derive(Default)]
pub struct PanelData {
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
    /// Original Red Alert audio (WP11.7): `None` = no audio system.
    pub ra_status: Option<sf_audio::RaStatus>,
    pub ra_catalog: Option<std::sync::Arc<sf_audio::ra::Catalog>>,
    /// Why the last sound-browser play failed.
    pub ra_play_error: Option<String>,
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
    /// 1 m cell grid on the terrain, fading with the zoom.
    pub grid: bool,
    /// Sound browser: (archive, entry) whose play button was clicked.
    pub ra_play: Option<(usize, usize)>,
}

impl PanelState {
    pub fn new(camera: CameraConfig) -> Self {
        Self {
            camera,
            camera_default: camera,
            lod_tint: false,
            grid: false,
            ra_play: None,
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
        "{} draw calls: units {}, scene {}, terrain {}",
        d.dynamic_draw_calls + d.scene_draw_calls + d.terrain_draw_calls,
        d.dynamic_draw_calls,
        d.scene_draw_calls,
        d.terrain_draw_calls,
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

fn simulation(ui: &mut egui::Ui, data: &PanelData) {
    ui.label(format!("ticks run {}", data.sim_tick));
    match data.checksum {
        Some(c) => ui.label(format!("checksum {c:016x}")),
        None => ui.label("checksum -"),
    };
    ui.label(format!("{} entities", data.entities));
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
    ui.checkbox(&mut state.grid, "Cell grid (1 m)")
        .on_hover_text("Fades out when zoomed far out; chunk borders (32 m) are yellow.");
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
                terrain_chunks_total: 64,
                terrain_chunks_visible: 12,
                terrain_triangles: 9_216,
                terrain_chunks_per_level: [4, 5, 3, 0, 0, 0],
            },
            instances: InstanceStats {
                total: 1350,
                visible: 40,
                tiles_total: 60,
                tiles_visible: 9,
                per_mesh,
                total_per_mesh,
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
            "Cell grid (1 m)",
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
    fn graph_handles_no_frames() {
        let ctx = egui::Context::default();
        let mut state = PanelState::new(CameraConfig::default());
        let (texts, _) = pass(&ctx, &PanelData::default(), &mut state);
        assert!(texts.iter().any(|t| t.contains("no frames yet")));
        assert!(texts.iter().any(|t| t.contains("checksum -")));
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
