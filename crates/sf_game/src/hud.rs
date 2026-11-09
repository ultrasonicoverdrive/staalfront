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

use crate::sim_snapshot::SimSnapshot;
use sf_data::map::Map;
use sf_engine::Context;
use sf_engine::winit::event::MouseButton as WinitButton;
use sf_math::render_bridge::vec2_cells_to_metres;
use sf_render::camera::DVec2;
use sf_render::{Camera, RenderError, Renderer};
use sf_ui::{Anchor, Color, Minimap, MouseButton, Ui, UiConfig, UiInput, UiRenderer, fov_polygon};

use crate::menu::{Item, Menu};

/// Longest side of the baked minimap image in pixels.
const MINIMAP_BAKE_SIDE: u32 = 256;
/// Margin to the window edges and the minimap size, logical pixels.
const EDGE: f32 = 6.0;
const MINIMAP_SIZE: f32 = 200.0;
/// The width of the top bar and of the line under it, logical pixels, and the least it is narrowed to.
const TOP_BAR_WIDTH: f32 = 520.0;
const TOP_BAR_LEAST: f32 = 300.0;

/// The width of the top bar in logical pixels for a window `screen_px` wide at UI `scale`. The bar is
/// centred; with the sidebar on the right edge (WP11.1) it is narrowed so that it does not reach over
/// the column at a larger UI scale (WP11.5a: at 150 % in a 1280-pixel window the "Dots" button lay on
/// the credits). Unchanged where it fitted before.
fn top_bar_width(screen_px: f32, scale: f32, sidebar: bool) -> f32 {
    if !sidebar {
        return TOP_BAR_WIDTH;
    }
    let look = crate::sidebar_view::shipped_look();
    let reserved = f32::from(look.width_px) + 2.0 * f32::from(look.margin_px);
    let room = screen_px / scale - 2.0 * reserved;
    TOP_BAR_WIDTH.min(room.max(TOP_BAR_LEAST))
}

/// Colors of the four players' unit dots (same order as the unit styles).
const OWNER_DOTS: [Color; 4] = [
    Color(230, 70, 60, 255),
    Color(70, 130, 240, 255),
    Color(240, 210, 70, 255),
    Color(80, 210, 110, 255),
];

/// Numbers shown in the top bar.
#[derive(Clone, Debug, Default)]
pub struct HudStats {
    pub fps: u32,
    pub units: u32,
    /// Own units selected (WP3.3).
    pub selected: u32,
    /// A unit of someone else is being inspected (and nothing of ours is
    /// selected).
    pub inspecting: bool,
    pub tick: u64,
    /// A second line under the top bar: what is being built and whether the ghost's place is
    /// allowed (WP6.2). Nothing while no structure is chosen.
    pub build_line: Option<String>,
    /// The credits of the local player and whether a build waits for them (WP7.4); `None` in a
    /// world without money rules.
    pub money: Option<crate::money_view::MoneyHud>,
    /// The queue readout of the selected factory (WP6.5); empty when none is selected.
    pub production: Vec<(String, crate::production_view::Tone)>,
    /// The result of the match once it is decided (WP7.6); `None` before, and in a world without a
    /// match. The banner is drawn from it ([`crate::match_view::draw_result`]).
    pub result: Option<sf_sim::MatchResult>,
    /// The sidebar of this frame (WP11.1); `None` in a scene without building and production (the
    /// crowd). It then replaces the credits readout in the corner.
    pub sidebar: Option<crate::sidebar::SidebarModel>,
}

pub struct Hud {
    ui: Ui,
    gpu: Option<UiRenderer>,
    minimap: Option<Minimap>,
    show_dots: bool,
    /// The tab and scroll of the sidebar (WP11.1).
    sidebar: crate::sidebar_view::SidebarState,
    /// What the sidebar's clicks asked for in the last frame; the runner takes them.
    sidebar_actions: Vec<crate::sidebar::Action>,
    /// The edge-scroll band of `input.ron`, in physical pixels: the sidebar keeps out of it.
    edge_band: f32,
    /// `ui.ron` as it is on disk; the UI runs with its scale times [`Hud::set_user_scale`] (WP11.5a).
    base_config: UiConfig,
    user_scale: f32,
    /// The dots of the minimap in the player's own set of colours (WP11.5a); `None` = [`OWNER_DOTS`].
    dots: Option<Vec<Color>>,
    /// What the menu's clicks asked for in the last frame; the runner takes them.
    menu_actions: Vec<Item>,
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

/// The width of the edge-scroll band in `input.ron` (`edge_margin`), or the engine's default.
fn load_edge_band(assets: &Path) -> f32 {
    sf_engine::Bindings::load(&assets.join("config/input.ron"))
        .unwrap_or_else(|_| sf_engine::Bindings::builtin())
        .edge_margin as f32
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
        let base_config = load_config(assets);
        Self {
            ui: Ui::new(base_config.clone()),
            gpu: None,
            minimap: None,
            show_dots: true,
            sidebar: crate::sidebar_view::SidebarState::default(),
            sidebar_actions: Vec::new(),
            edge_band: load_edge_band(assets),
            base_config,
            user_scale: 1.0,
            dots: None,
            menu_actions: Vec::new(),
        }
    }

    /// Re-read `ui.ron` (hot reload).
    pub fn reload_config(&mut self, assets: &Path) {
        self.base_config = load_config(assets);
        self.apply_config();
    }

    /// The UI scale of the player's settings (WP11.5a): everything the HUD draws, the sidebar and the
    /// menus included, is scaled by `ui.ron`'s scale times this (and the window's DPI factor). The
    /// sidebar keeps out of the edge-scroll band at any scale ([`crate::sidebar_layout::layout`]).
    pub fn set_user_scale(&mut self, scale: f32) {
        self.user_scale = scale;
        self.apply_config();
    }

    fn apply_config(&mut self) {
        let mut config = self.base_config.clone();
        config.scale *= self.user_scale;
        self.ui.set_config(config);
    }

    /// The dots of the minimap in these colours (one per player), or `None` for the standard ones.
    pub fn set_owner_dots(&mut self, dots: Option<Vec<Color>>) {
        self.dots = dots.filter(|d| !d.is_empty());
    }

    /// The colour of a player's dot on the minimap: the player's own set, else the standard one.
    fn dot_color(dots: &Option<Vec<Color>>, owner: u8) -> Color {
        match dots {
            Some(dots) => dots[usize::from(owner) % dots.len()],
            None => OWNER_DOTS[usize::from(owner) % OWNER_DOTS.len()],
        }
    }

    /// The colour of a player's dot now (tests read it).
    pub fn owner_dot(&self, owner: u8) -> Color {
        Self::dot_color(&self.dots, owner)
    }

    /// Bake the minimap again, for another map (a new scene, WP11.5a).
    pub fn rebake_minimap(&mut self, renderer: &Renderer, map: &Map) {
        if self.gpu.is_some() {
            let gpu = renderer.gpu();
            let minimap = Minimap::bake(map, MINIMAP_BAKE_SIDE);
            if let Some(ui_gpu) = &mut self.gpu {
                ui_gpu.set_minimap(&gpu.device, &gpu.queue, minimap.image());
            }
            self.minimap = Some(minimap);
        } else {
            self.attach(renderer, map);
        }
    }

    /// What the menu's clicks of the last frame asked for; taken once.
    pub fn take_menu_actions(&mut self) -> Vec<Item> {
        std::mem::take(&mut self.menu_actions)
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

    /// Repaint single cells of the minimap (the ore a cell still holds, WP7.1) and upload the
    /// image again if any pixel changed.
    pub fn paint_minimap(&mut self, renderer: &Renderer, map: &Map, cells: &[(u32, u32, [u8; 3])]) {
        if let (Some(minimap), Some(ui_gpu)) = (&mut self.minimap, &mut self.gpu)
            && minimap.paint_cells(map, cells) > 0
        {
            let gpu = renderer.gpu();
            ui_gpu.set_minimap(&gpu.device, &gpu.queue, minimap.image());
        }
    }

    /// Lays the fog of war over the minimap, or takes it off (`None`), and uploads the image when it
    /// changed (WP8.4). Call when the snapshot's fog version changed, not every frame.
    pub fn set_minimap_fog(
        &mut self,
        renderer: &Renderer,
        map: &Map,
        fog: Option<(&[u8], sf_ui::MinimapFog)>,
    ) {
        if let (Some(minimap), Some(ui_gpu)) = (&mut self.minimap, &mut self.gpu)
            && minimap.set_fog(map, fog)
        {
            let gpu = renderer.gpu();
            ui_gpu.set_minimap(&gpu.device, &gpu.queue, minimap.image());
        }
    }

    /// Build this frame's UI. A click or drag on the minimap moves `camera`. With a `menu` open (WP11.5a)
    /// the game's panels are built with an input that sees nothing, so nothing under the menu can be
    /// hovered or clicked, and the menu is drawn over them with the real input; what it was clicked
    /// for is [`Hud::take_menu_actions`].
    pub fn frame(
        &mut self,
        ctx: &Context,
        camera: &mut Camera,
        snap: &SimSnapshot,
        ghosts: &crate::fog_view::GhostBook,
        stats: HudStats,
        menu: Option<&Menu>,
    ) {
        let size = ctx.size();
        let real = ui_input(ctx);
        let ui = &mut self.ui;
        // With a menu open the game's panels are built blind (the input they get sees nothing).
        let input = crate::menu_view::begin_frame(
            ui,
            real,
            (size.width, size.height),
            ctx.scale_factor(),
            menu.is_some(),
        );

        // Top bar: a label and a button.
        let bar_width = top_bar_width(size.width as f32, ui.scale(), stats.sidebar.is_some());
        let bar = ui.panel(Anchor::Top, (0.0, EDGE), (bar_width, 32.0));
        let text = ui.config().text;
        ui.text_in(
            bar,
            Anchor::Left,
            (12.0, 0.0),
            &format!(
                "{} fps   {} units   {}   tick {}",
                stats.fps,
                stats.units,
                if stats.inspecting {
                    "inspecting".to_owned()
                } else {
                    format!("{} selected", stats.selected)
                },
                stats.tick
            ),
            text,
        );
        if let Some(line) = &stats.build_line {
            let strip = ui.panel(Anchor::Top, (0.0, EDGE + 32.0 + 4.0), (bar_width, 26.0));
            ui.text_in(strip, Anchor::Left, (12.0, 0.0), line, text);
        }
        self.sidebar_actions = draw_panels(
            ui,
            &input,
            &stats,
            &mut self.sidebar,
            self.edge_band,
            self.minimap.is_some(),
        );
        let mut menu_actions = Vec::new();
        if crate::match_view::draw_result(ui, stats.result, crate::commands::LOCAL_PLAYER)
            && menu.is_none()
        {
            // The match is decided: Restart and Main menu stand under the banner (WP11.5a).
            let bottom = crate::match_view::shipped_look().banner;
            let bottom = bottom.top_px.saturating_add(bottom.height_px);
            menu_actions.extend(crate::menu_view::result_buttons(
                ui,
                crate::menu_look::MenuLook::embedded(),
                bottom,
            ));
        }
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
            let fov = fov_polygon(camera, (size.width as f32, size.height as f32), map);
            let palette = &self.dots;
            let ghost_dot = crate::fog_view::shipped_look().ghost_dot;
            let ghost_dot = Color(ghost_dot.0, ghost_dot.1, ghost_dot.2, 255);
            // Only what the local player sees, and the ghosts of structures it saw (WP8.4).
            let dots = self.show_dots.then(|| {
                crate::fog_view::minimap_dots(snap, ghosts).map(move |(pos, dot)| {
                    let [x, z] = vec2_cells_to_metres(pos);
                    let color = match dot {
                        crate::fog_view::Dot::Owner(o) => Self::dot_color(palette, o),
                        crate::fog_view::Dot::Ghost => ghost_dot,
                    };
                    ((x, z), color)
                })
            });
            if let Some((x, z)) = ui.minimap(
                minimap,
                "minimap",
                frame,
                Some(&fov),
                dots.into_iter().flatten(),
            ) {
                camera.set_focus(DVec2::new(f64::from(x), f64::from(z)));
            }
        }
        // The menu over everything (WP11.5a), with the real pointer.
        menu_actions.extend(crate::menu_view::finish_frame(ui, real, menu));
        self.menu_actions = menu_actions;
    }

    /// The tab the sidebar shows (WP11.1): the model of the next frame is built for it.
    pub fn sidebar_tab(&self) -> crate::sidebar::Tab {
        self.sidebar.tab
    }

    /// The sidebar's tab and scroll. A new scene starts from the default ([`Hud::reset_sidebar`]).
    pub fn sidebar_state(&self) -> crate::sidebar_view::SidebarState {
        self.sidebar
    }

    /// Puts the sidebar back on its default tab at scroll 0 (a Restart or a new scene).
    pub fn reset_sidebar(&mut self) {
        self.sidebar = crate::sidebar_view::SidebarState::default();
    }

    /// Sets the sidebar's tab and scroll (the tests put it somewhere before a restart).
    #[cfg(test)]
    pub(crate) fn set_sidebar_state(&mut self, state: crate::sidebar_view::SidebarState) {
        self.sidebar = state;
    }

    /// What the sidebar's clicks of the last frame asked for; taken once.
    pub fn take_sidebar_actions(&mut self) -> Vec<crate::sidebar::Action> {
        std::mem::take(&mut self.sidebar_actions)
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

    /// The layer for what the game draws over the scene but under the panels
    /// (selection rings, health bars, the drag rectangle). Valid after
    /// [`Hud::frame`], which clears it.
    pub fn world_layer(&mut self) -> sf_ui::Layer<'_> {
        self.ui.world_layer()
    }

    /// Packs the cursor pictures of `dir` into the UI's atlas (WP3.5).
    pub fn load_cursors(&mut self, dir: &Path) -> Result<sf_ui::CursorSet, sf_ui::CursorError> {
        sf_ui::CursorSet::load(dir, self.ui.atlas_mut())
    }

    /// Draws cursor `index` of `set` with its hotspot at `at` (physical
    /// pixels) over everything, panels included. Valid after [`Hud::frame`],
    /// which clears the list; call it last, before [`Hud::draw`].
    pub fn draw_cursor(&mut self, set: &sf_ui::CursorSet, index: usize, at: (f32, f32)) {
        self.ui.draw_cursor(set, index, at);
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

/// The panels of a frame below the top bar: the sidebar (which shows the credits itself) or, without
/// one, the credits readout in the corner, then the production readout. Each once; returns what the
/// sidebar's clicks asked for.
fn draw_panels(
    ui: &mut Ui,
    input: &UiInput,
    stats: &HudStats,
    sidebar: &mut crate::sidebar_view::SidebarState,
    edge_band_px: f32,
    minimap: bool,
) -> Vec<crate::sidebar::Action> {
    let mut actions = Vec::new();
    match &stats.sidebar {
        Some(model) => {
            let setup = crate::sidebar_view::Setup {
                look: crate::sidebar_view::shipped_look(),
                money: crate::money_view::shipped_look(),
                edge_band_px,
                minimap,
            };
            actions = crate::sidebar_view::frame(ui, input, sidebar, model, &setup);
        }
        None => {
            if let Some(money) = &stats.money {
                crate::money_view::draw_panel(ui, money, crate::money_view::shipped_look());
            }
        }
    }
    if !stats.production.is_empty() {
        crate::production_view::draw_panel(
            ui,
            &stats.production,
            crate::production_view::shipped_look(),
        );
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::production_view::Tone;

    fn quads(stats: &HudStats, only_production: bool) -> usize {
        let mut ui = Ui::new(UiConfig::default());
        let input = UiInput::default();
        ui.begin_frame(input, (1280, 720), 1.0);
        if only_production {
            crate::production_view::draw_panel(
                &mut ui,
                &stats.production,
                crate::production_view::shipped_look(),
            );
        } else {
            let mut state = crate::sidebar_view::SidebarState::default();
            draw_panels(&mut ui, &input, stats, &mut state, 4.0, false);
        }
        ui.draw_list().quad_count()
    }

    /// At every UI scale of the settings the centred top bar stays left of the sidebar in a 1280-wide
    /// window (up to 150 %), is what it always was at 100 %, and without a sidebar is unchanged.
    #[test]
    fn the_top_bar_does_not_reach_over_the_sidebar() {
        let side = crate::sidebar_view::shipped_look();
        let (w, band) = (1280.0_f32, 4.0_f32);
        for scale in [0.75_f32, 1.0, 1.25, 1.5] {
            let bar = top_bar_width(w, scale, true) * scale;
            let column = crate::sidebar_layout::layout(
                sf_ui::Rect::new(0.0, 0.0, w, 720.0),
                scale,
                side,
                band,
                true,
            )
            .column;
            assert!(
                w * 0.5 + bar * 0.5 <= column.x,
                "scale {scale}: the bar reaches {} and the column starts at {}",
                w * 0.5 + bar * 0.5,
                column.x
            );
        }
        assert_eq!(top_bar_width(1280.0, 1.0, true), 520.0);
        assert_eq!(top_bar_width(1920.0, 1.5, true), 520.0);
        assert_eq!(top_bar_width(1280.0, 1.5, false), 520.0);
        // A window too narrow for it does not make it vanish.
        assert_eq!(top_bar_width(1280.0, 2.0, true), 300.0);
    }

    /// The merge of 7460c3b drew the production readout twice (a darker panel, text drawn over
    /// itself): the panels of a frame lay it out exactly once.
    #[test]
    fn the_production_readout_is_drawn_once_per_frame() {
        let stats = HudStats {
            production: vec![
                ("barracks: rifleman 40%".to_owned(), Tone::Plain),
                ("vehicle factory: idle".to_owned(), Tone::Accent),
            ],
            ..HudStats::default()
        };
        let once = quads(&stats, true);
        assert!(once > 0);
        assert_eq!(quads(&stats, false), once);
        // Nothing to show: nothing drawn.
        assert_eq!(quads(&HudStats::default(), false), 0);
    }
}
