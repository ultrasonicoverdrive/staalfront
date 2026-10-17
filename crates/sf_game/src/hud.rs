//! The demo HUD (WP1.12): a top bar with a label and a button, and the
//! minimap bottom-right. No gameplay; it only exercises `sf_ui` (anchors,
//! text, buttons, input consumption, minimap click and field-of-view frame).
//!
//! Frame order in `main.rs`: [`Hud::frame`] builds the UI from this frame's
//! input and may move the camera (minimap click); the camera then asks
//! [`Hud::wheel_consumed`] and [`Hud::middle_captured`] so a press, drag or
//! wheel that belongs to the UI never reaches it; [`Hud::draw`] renders the
//! scene with the UI on top.
//!
//! The HUD keeps 6 logical pixels from the window edges: the engine's
//! screen-edge scrolling (4 px band, `input.ron`) would otherwise pan the
//! camera while the cursor rests on the minimap's outer pixels.

use std::path::Path;

use sf_data::map::Map;
use sf_engine::Context;
use sf_engine::winit::event::MouseButton as WinitButton;
use sf_math::render_bridge::vec2_cells_to_metres;
use sf_render::camera::DVec2;
use sf_render::{Camera, RenderError, Renderer};
use sf_sim::EntityView;
use sf_ui::{Anchor, Color, Minimap, MouseButton, Ui, UiConfig, UiInput, UiRenderer, fov_quad};

/// Longest side of the baked minimap image in pixels.
const MINIMAP_BAKE_SIDE: u32 = 256;
/// Margin to the window edges and the minimap size, logical pixels.
const EDGE: f32 = 6.0;
const MINIMAP_SIZE: f32 = 200.0;

/// Colors of the four players' unit dots (same order as the unit styles).
const OWNER_DOTS: [Color; 4] = [
    Color(230, 70, 60, 255),
    Color(70, 130, 240, 255),
    Color(240, 210, 70, 255),
    Color(80, 210, 110, 255),
];

/// Numbers shown in the top bar.
#[derive(Clone, Copy, Debug, Default)]
pub struct HudStats {
    pub fps: u32,
    pub units: u32,
    pub tick: u64,
}

pub struct Hud {
    ui: Ui,
    gpu: Option<UiRenderer>,
    minimap: Option<Minimap>,
    show_dots: bool,
}

/// `assets/config/ui.ron`, or the defaults when it is missing or broken.
fn load_config(assets: &Path) -> UiConfig {
    let path = assets.join("config/ui.ron");
    match std::fs::read_to_string(&path) {
        Ok(text) => UiConfig::from_ron_str(&text).unwrap_or_else(|e| {
            eprintln!(
                "sf_game: {} ({e}); using default UI settings",
                path.display()
            );
            UiConfig::default()
        }),
        Err(e) => {
            eprintln!(
                "sf_game: {} ({e}); using default UI settings",
                path.display()
            );
            UiConfig::default()
        }
    }
}

/// This frame's pointer input in the UI's terms.
fn ui_input(ctx: &Context) -> UiInput {
    let input = ctx.input();
    let buttons = [WinitButton::Left, WinitButton::Middle, WinitButton::Right];
    let mut out = UiInput {
        cursor: input.cursor().map(|(x, y)| (x as f32, y as f32)),
        wheel: input.wheel_delta(),
        ..UiInput::default()
    };
    for (i, b) in buttons.into_iter().enumerate() {
        out.down[i] = input.button_down(b);
        out.pressed[i] = input.button_pressed(b);
        out.released[i] = input.button_released(b);
    }
    out
}

impl Hud {
    pub fn new(assets: &Path) -> Self {
        Self {
            ui: Ui::new(load_config(assets)),
            gpu: None,
            minimap: None,
            show_dots: true,
        }
    }

    /// Re-read `ui.ron` (hot reload).
    pub fn reload_config(&mut self, assets: &Path) {
        self.ui.set_config(load_config(assets));
    }

    /// Create the GPU side and bake the minimap of `map`.
    pub fn attach(&mut self, renderer: &Renderer, map: &Map) {
        let gpu = &renderer.gpu();
        let mut ui_gpu = UiRenderer::new(&gpu.device, renderer.color_format());
        let minimap = Minimap::bake(map, MINIMAP_BAKE_SIDE);
        ui_gpu.set_minimap(&gpu.device, &gpu.queue, minimap.image());
        self.gpu = Some(ui_gpu);
        self.minimap = Some(minimap);
    }

    /// Build this frame's UI. A click or drag on the minimap moves `camera`.
    pub fn frame(
        &mut self,
        ctx: &Context,
        camera: &mut Camera,
        views: &[EntityView],
        stats: HudStats,
    ) {
        let size = ctx.size();
        let ui = &mut self.ui;
        ui.begin_frame(ui_input(ctx), (size.width, size.height), ctx.scale_factor());

        // Top bar: a label and a button.
        let bar = ui.panel(Anchor::Top, (0.0, EDGE), (420.0, 32.0));
        let text = ui.config().text;
        ui.text_in(
            bar,
            Anchor::Left,
            (12.0, 0.0),
            &format!(
                "{} fps   {} units   tick {}",
                stats.fps, stats.units, stats.tick
            ),
            text,
        );
        let button = ui.place(bar, Anchor::Right, (6.0, 0.0), (110.0, 22.0));
        let label = if self.show_dots {
            "Dots: on"
        } else {
            "Dots: off"
        };
        if ui.button("toggle-dots", button, label, true).clicked {
            self.show_dots = !self.show_dots;
        }

        // Minimap bottom-right.
        if let Some(minimap) = &self.minimap {
            let frame = ui.place(
                ui.screen(),
                Anchor::BottomRight,
                (EDGE, EDGE),
                (MINIMAP_SIZE, MINIMAP_SIZE),
            );
            let map = minimap.map_size();
            let fov = fov_quad(camera, (size.width as f32, size.height as f32), map);
            let dots = self.show_dots.then(|| {
                views.iter().map(|v| {
                    let [x, z] = vec2_cells_to_metres(v.pos);
                    (
                        (x, z),
                        OWNER_DOTS[usize::from(v.owner.0) % OWNER_DOTS.len()],
                    )
                })
            });
            if let Some((x, z)) = ui.minimap(
                minimap,
                "minimap",
                frame,
                Some(fov),
                dots.into_iter().flatten(),
            ) {
                camera.set_focus(DVec2::new(f64::from(x), f64::from(z)));
            }
        }
        ui.end_frame();
    }

    /// Whether the UI took this frame's wheel movement.
    pub fn wheel_consumed(&self) -> bool {
        self.ui.wheel_consumed()
    }

    /// The UI scale (`ui.ron` scale times the window's DPI factor) of the
    /// last frame: strategic icons and their click size follow it.
    pub fn scale(&self) -> f32 {
        self.ui.scale()
    }

    /// The cursor is over a UI panel, button or the minimap: the units
    /// behind it are not hovered.
    pub fn pointer_over_ui(&self) -> bool {
        self.ui.pointer_over_ui()
    }

    /// The middle button went down on the UI and is still held.
    pub fn middle_captured(&self) -> bool {
        self.ui.button_captured(MouseButton::Middle)
    }

    /// Render the scene and the UI on top.
    pub fn draw(&mut self, renderer: &mut Renderer) -> Result<(), RenderError> {
        match &mut self.gpu {
            Some(gpu) => {
                let ui = &mut self.ui;
                renderer.render_with(|target| gpu.render(target, ui))
            }
            None => renderer.render(),
        }
    }
}
