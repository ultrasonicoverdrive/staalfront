//! The immediate-mode UI: widgets declared every frame, pointer input that
//! the UI sees first.
//!
//! # Immediate mode
//! The game calls [`Ui::begin_frame`], declares panels, labels and buttons in
//! painter's order, and calls [`Ui::end_frame`]. A button's result (`clicked`)
//! comes back from the call that declares it, so there are no callbacks and no
//! widget tree to keep in sync with game state; the only state kept between
//! frames is which widget the left button went down on (`active`) and which
//! mouse buttons the UI has captured. Widgets are identified by a string id.
//!
//! # Input: the UI first
//! Every panel, button and area registers its rectangle as a *blocker*. At
//! [`Ui::end_frame`] the UI decides what it consumed this frame:
//!
//! * a **press** of any mouse button while the cursor is over a blocker is
//!   consumed, and that button stays *captured* until it is released, so a
//!   drag that started on the UI never leaks into the game when the cursor
//!   leaves the panel;
//! * a **wheel** notch while the cursor is over a blocker is consumed;
//! * a **release** is never consumed: the game always sees it, so nothing it
//!   considers held can get stuck.
//!
//! [`Ui::filter_for_game`] applies exactly that to a raw [`UiInput`]. A press
//! that started in the game and then moves over the UI (a camera drag) is not
//! captured and continues.

use crate::atlas::Atlas;
use crate::color::{Color, UiConfig};
use crate::draw::{DrawList, Layer, Texture, Uv};
use crate::layout::{Anchor, Rect, place, place_physical};
use crate::skin::{BORDER, Skin};
use crate::text::{Font, GlyphCache};

/// The mouse buttons the UI tracks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left = 0,
    Middle = 1,
    Right = 2,
}

/// One frame of pointer input, in physical window pixels. Independent of
/// winit; the game fills it from `sf_engine`'s `InputState`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UiInput {
    /// Cursor position; `None` when it is outside the window.
    pub cursor: Option<(f32, f32)>,
    /// Button is down now, went down this frame, went up this frame
    /// (indexed by [`MouseButton`]).
    pub down: [bool; 3],
    pub pressed: [bool; 3],
    pub released: [bool; 3],
    /// Wheel notches this frame (positive = away from the user).
    pub wheel: f32,
}

impl UiInput {
    pub fn down(&self, b: MouseButton) -> bool {
        self.down[b as usize]
    }
    pub fn pressed(&self, b: MouseButton) -> bool {
        self.pressed[b as usize]
    }
    pub fn released(&self, b: MouseButton) -> bool {
        self.released[b as usize]
    }
}

/// What a button reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Response {
    /// The cursor is over it (and it is enabled).
    pub hovered: bool,
    /// The left button went down on it and is still held, cursor still over it.
    pub pressed: bool,
    /// The left button was released over it after going down on it.
    pub clicked: bool,
}

/// What an interactive area (the minimap) reports.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AreaResponse {
    pub hovered: bool,
    /// The left button went down on the area this frame.
    pub pressed: bool,
    /// The left button went down on the area earlier and is still held (the
    /// cursor may have left the area).
    pub dragging: bool,
    /// The left button was released this frame after going down on the area.
    pub released: bool,
    /// Cursor position while `pressed` or `dragging`.
    pub cursor: Option<(f32, f32)>,
}

/// FNV-1a: a stable, dependency-free id for a widget name.
fn widget_id(name: &str) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in name.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

pub struct Ui {
    pub(crate) cfg: UiConfig,
    pub(crate) font: Font,
    pub(crate) atlas: Atlas,
    glyphs: GlyphCache,
    pub(crate) skin: Skin,
    pub(crate) list: DrawList,
    /// What the game draws under the panels ([`Ui::world_layer`]).
    under: DrawList,
    scale: f32,
    screen: Rect,
    pub(crate) input: UiInput,
    /// Rectangles of everything declared this frame (and, between frames, of
    /// the last one).
    blockers: Vec<Rect>,
    /// The widget the left button went down on.
    active: Option<u64>,
    captured: [bool; 3],
    over_ui: bool,
    press_consumed: [bool; 3],
    wheel_consumed: bool,
}

impl Ui {
    pub fn new(cfg: UiConfig) -> Self {
        let mut atlas = Atlas::default();
        let skin = Skin::build(&mut atlas);
        Self {
            cfg,
            font: Font::embedded(),
            atlas,
            glyphs: GlyphCache::default(),
            skin,
            list: DrawList::default(),
            under: DrawList::default(),
            scale: 1.0,
            screen: Rect::default(),
            input: UiInput::default(),
            blockers: Vec::new(),
            active: None,
            captured: [false; 3],
            over_ui: false,
            press_consumed: [false; 3],
            wheel_consumed: false,
        }
    }

    pub fn config(&self) -> &UiConfig {
        &self.cfg
    }

    /// Replace the settings (hot reload); takes effect next frame.
    pub fn set_config(&mut self, cfg: UiConfig) {
        self.cfg = cfg;
    }

    /// Effective scale: config scale times the window's DPI factor.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// The window as a rectangle in physical pixels.
    pub fn screen(&self) -> Rect {
        self.screen
    }

    pub fn draw_list(&self) -> &DrawList {
        &self.list
    }

    /// The draw list of the world layer: drawn before (under) the panels of
    /// [`Ui::draw_list`].
    pub fn underlay(&self) -> &DrawList {
        &self.under
    }

    /// Draw on the world layer: shapes the game places over the scene but
    /// under the panels (WP3.3: selection rings, health bars, the drag
    /// rectangle). Cleared by [`Ui::begin_frame`]; may be used after
    /// [`Ui::end_frame`], so the game can draw what depends on this frame's
    /// camera after the UI was built. Does not block input.
    pub fn world_layer(&mut self) -> Layer<'_> {
        Layer::new(&mut self.under, self.skin.white)
    }

    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    pub fn atlas_mut(&mut self) -> &mut Atlas {
        &mut self.atlas
    }

    /// Glyphs the atlas had no room for (should stay 0).
    pub fn dropped_glyphs(&self) -> u32 {
        self.glyphs.dropped
    }

    // ---- frame ----

    /// Start a frame. `screen` is the window size in physical pixels, `dpi`
    /// the window's scale factor.
    pub fn begin_frame(&mut self, input: UiInput, screen: (u32, u32), dpi: f64) {
        self.list.clear();
        self.under.clear();
        self.blockers.clear();
        self.input = input;
        self.scale = self.cfg.effective_scale(dpi);
        self.screen = Rect::new(0.0, 0.0, screen.0 as f32, screen.1 as f32);
    }

    /// Finish the frame: decide what the UI consumed (see the module docs).
    pub fn end_frame(&mut self) {
        let over = self
            .input
            .cursor
            .is_some_and(|(x, y)| self.blockers.iter().any(|r| r.contains(x, y)));
        self.over_ui = over;
        for b in 0..3 {
            self.press_consumed[b] = self.input.pressed[b] && over;
            if self.press_consumed[b] {
                self.captured[b] = true;
            }
            if !self.input.down[b] {
                self.captured[b] = false;
            }
        }
        self.wheel_consumed = over && self.input.wheel != 0.0;
        if !self.input.down[0] {
            self.active = None;
        }
    }

    // ---- what the UI consumed ----

    /// The cursor is over a panel, button or area (as of the last
    /// [`Ui::end_frame`]).
    pub fn pointer_over_ui(&self) -> bool {
        self.over_ui
    }

    /// Would a click at `(x, y)` hit the UI? Uses the rectangles of the last
    /// frame, so it can answer in an event hook before this frame is built.
    pub fn blocks_pointer_at(&self, x: f32, y: f32) -> bool {
        self.blockers.iter().any(|r| r.contains(x, y))
    }

    /// The press of `b` this frame went to the UI.
    pub fn press_consumed(&self, b: MouseButton) -> bool {
        self.press_consumed[b as usize]
    }

    /// `b` went down on the UI and has not been released yet.
    pub fn button_captured(&self, b: MouseButton) -> bool {
        self.captured[b as usize]
    }

    /// The wheel movement of this frame went to the UI.
    pub fn wheel_consumed(&self) -> bool {
        self.wheel_consumed
    }

    /// The input the game should act on: presses over the UI are removed,
    /// buttons captured by the UI do not count as held, the wheel is zeroed
    /// when the UI took it. Releases pass untouched. Call after
    /// [`Ui::end_frame`].
    pub fn filter_for_game(&self, raw: &UiInput) -> UiInput {
        let mut g = *raw;
        for b in 0..3 {
            g.pressed[b] &= !self.press_consumed[b];
            g.down[b] &= !self.captured[b];
        }
        if self.wheel_consumed {
            g.wheel = 0.0;
        }
        g
    }

    // ---- layout ----

    /// Place a box of `size` (logical px) in `parent` (physical px); see
    /// [`crate::layout::place`] for the offset convention.
    pub fn place(
        &self,
        parent: Rect,
        anchor: Anchor,
        offset: (f32, f32),
        size: (f32, f32),
    ) -> Rect {
        place(parent, anchor, offset, size, self.scale)
    }

    /// The default text size in physical pixels.
    pub fn text_px(&self) -> f32 {
        (self.cfg.font_size * self.scale).round()
    }

    /// Size of `text` in physical pixels at `px` physical pixels (em size).
    pub fn measure_px(&self, text: &str, px: f32) -> (f32, f32) {
        self.font.measure(text, px.round())
    }

    /// Size of `text` in physical pixels at the default text size.
    pub fn measure(&self, text: &str) -> (f32, f32) {
        self.font.measure(text, self.text_px())
    }

    // ---- drawing ----

    fn block(&mut self, rect: Rect) {
        self.blockers.push(rect);
    }

    /// A panel (nine-slice, tinted with the config color) that blocks input.
    pub fn panel_rect(&mut self, rect: Rect) {
        self.block(rect);
        let border = (BORDER * self.scale).round();
        let skin = self.skin.panel;
        self.list.nine_slice(&skin, rect, border, self.cfg.panel);
    }

    /// A panel of `size` (logical px) anchored in the window.
    pub fn panel(&mut self, anchor: Anchor, offset: (f32, f32), size: (f32, f32)) -> Rect {
        let rect = self.place(self.screen, anchor, offset, size);
        self.panel_rect(rect);
        rect
    }

    /// A solid, non-blocking rectangle.
    pub fn fill(&mut self, rect: Rect, color: Color) {
        self.list.rect(self.skin.white, rect, color);
    }

    /// Text with its top-left at `(x, y)` physical pixels, `px` physical
    /// pixels tall (em size).
    pub fn text_px_at(&mut self, x: f32, y: f32, text: &str, px: f32, color: Color) {
        let px = px.round();
        let (placed, _) = self.font.layout(text, px);
        for p in placed {
            let Some(slot) = self.glyphs.get(&self.font, &mut self.atlas, p.ch, px) else {
                continue;
            };
            let r = slot.rect;
            let rect = Rect::new(
                (x + p.x).round() + slot.left,
                (y + p.baseline).round() + slot.top,
                r.w as f32,
                r.h as f32,
            );
            let uv = [
                r.x as f32,
                r.y as f32,
                (r.x + r.w) as f32,
                (r.y + r.h) as f32,
            ];
            self.list.quad(Texture::Atlas, rect, uv, color);
        }
    }

    /// Text at the default size, its box anchored in `parent`. Returns the
    /// box. Text does not block input.
    pub fn text_in(
        &mut self,
        parent: Rect,
        anchor: Anchor,
        offset: (f32, f32),
        text: &str,
        color: Color,
    ) -> Rect {
        let px = self.text_px();
        let size = self.font.measure(text, px);
        let size = (size.0.ceil(), size.1.ceil());
        let rect = place_physical(parent, anchor, offset, size, self.scale);
        self.text_px_at(rect.x, rect.y, text, px, color);
        rect
    }

    /// Any textured quad (the minimap image).
    pub(crate) fn image(&mut self, texture: Texture, rect: Rect, uv: Uv, color: Color) {
        self.list.quad(texture, rect, uv, color);
    }

    // ---- widgets ----

    /// A button. `id` must be unique among this frame's widgets.
    pub fn button(&mut self, id: &str, rect: Rect, label: &str, enabled: bool) -> Response {
        let key = widget_id(id);
        self.block(rect);
        let over = self.input.cursor.is_some_and(|(x, y)| rect.contains(x, y));
        let mut resp = Response::default();
        if enabled {
            if over && self.input.pressed(MouseButton::Left) {
                self.active = Some(key);
            }
            let mine = self.active == Some(key);
            resp.hovered = over && (mine || !self.input.down(MouseButton::Left));
            resp.pressed = mine && over && self.input.down(MouseButton::Left);
            if mine && self.input.released(MouseButton::Left) {
                resp.clicked = over;
                self.active = None;
            }
            // A press and a release inside one frame still counts as a click
            // and must not leave the button pressed.
            if resp.clicked {
                resp.pressed = false;
            }
        }
        let tint = if !enabled {
            self.cfg.button_disabled
        } else if resp.pressed {
            self.cfg.button_pressed
        } else if resp.hovered {
            self.cfg.button_hover
        } else {
            self.cfg.button
        };
        let border = (BORDER * self.scale).round();
        let skin = self.skin.button;
        self.list.nine_slice(&skin, rect, border, tint);
        let text_color = if enabled {
            self.cfg.text
        } else {
            self.cfg.text_disabled
        };
        let (tw, th) = self.measure(label);
        let nudge = if resp.pressed {
            self.scale.round().max(1.0)
        } else {
            0.0
        };
        let px = self.text_px();
        self.text_px_at(
            (rect.x + (rect.w - tw) * 0.5).round(),
            (rect.y + (rect.h - th) * 0.5).round() + nudge,
            label,
            px,
            text_color,
        );
        resp
    }

    /// An invisible interactive area that blocks input and tracks a left
    /// button drag that started on it (the minimap uses it).
    pub fn area(&mut self, id: &str, rect: Rect) -> AreaResponse {
        let key = widget_id(id);
        self.block(rect);
        let over = self.input.cursor.is_some_and(|(x, y)| rect.contains(x, y));
        let mut resp = AreaResponse::default();
        if over && self.input.pressed(MouseButton::Left) && self.active.is_none() {
            self.active = Some(key);
            resp.pressed = true;
        }
        let mine = self.active == Some(key);
        resp.hovered = over && (mine || !self.input.down(MouseButton::Left));
        resp.dragging = mine && !resp.pressed && self.input.down(MouseButton::Left);
        if mine && self.input.released(MouseButton::Left) {
            resp.released = true;
            self.active = None;
        }
        if resp.pressed || resp.dragging {
            resp.cursor = self.input.cursor;
        }
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (u32, u32) = (800, 600);
    const BTN: Rect = Rect::new(100.0, 100.0, 120.0, 30.0);

    fn at(x: f32, y: f32) -> UiInput {
        UiInput {
            cursor: Some((x, y)),
            ..UiInput::default()
        }
    }

    fn press(mut i: UiInput, b: MouseButton) -> UiInput {
        i.pressed[b as usize] = true;
        i.down[b as usize] = true;
        i
    }

    fn hold(mut i: UiInput, b: MouseButton) -> UiInput {
        i.down[b as usize] = true;
        i
    }

    fn release(mut i: UiInput, b: MouseButton) -> UiInput {
        i.released[b as usize] = true;
        i
    }

    /// One frame with a single button; returns its response.
    fn frame(ui: &mut Ui, input: UiInput) -> Response {
        ui.begin_frame(input, SCREEN, 1.0);
        let r = ui.button("go", BTN, "Go", true);
        ui.end_frame();
        r
    }

    #[test]
    fn hover_press_click_cycle() {
        let mut ui = Ui::new(UiConfig::default());
        assert_eq!(frame(&mut ui, at(10.0, 10.0)), Response::default());
        let r = frame(&mut ui, at(150.0, 110.0));
        assert!(r.hovered && !r.pressed && !r.clicked);
        let r = frame(&mut ui, press(at(150.0, 110.0), MouseButton::Left));
        assert!(r.hovered && r.pressed && !r.clicked);
        let r = frame(&mut ui, hold(at(150.0, 110.0), MouseButton::Left));
        assert!(r.pressed && !r.clicked);
        let r = frame(&mut ui, release(at(150.0, 110.0), MouseButton::Left));
        assert!(r.clicked && !r.pressed);
        let r = frame(&mut ui, at(150.0, 110.0));
        assert!(r.hovered && !r.clicked, "one click only");
    }

    #[test]
    fn releasing_outside_cancels_the_click() {
        let mut ui = Ui::new(UiConfig::default());
        frame(&mut ui, press(at(150.0, 110.0), MouseButton::Left));
        let away = hold(at(400.0, 400.0), MouseButton::Left);
        let r = frame(&mut ui, away);
        assert!(
            !r.pressed && !r.hovered,
            "pressed look only while the cursor is over it"
        );
        let r = frame(&mut ui, release(at(400.0, 400.0), MouseButton::Left));
        assert!(!r.clicked);
        // And the next frame nothing is stuck.
        let r = frame(&mut ui, at(150.0, 110.0));
        assert!(r.hovered && !r.pressed);
    }

    #[test]
    fn pressing_elsewhere_and_releasing_over_the_button_is_not_a_click() {
        let mut ui = Ui::new(UiConfig::default());
        frame(&mut ui, press(at(400.0, 400.0), MouseButton::Left));
        let r = frame(&mut ui, hold(at(150.0, 110.0), MouseButton::Left));
        assert!(
            !r.pressed && !r.hovered,
            "not hot while another press is active"
        );
        let r = frame(&mut ui, release(at(150.0, 110.0), MouseButton::Left));
        assert!(!r.clicked);
    }

    #[test]
    fn a_press_and_release_in_one_frame_is_a_click() {
        let mut ui = Ui::new(UiConfig::default());
        let mut input = press(at(150.0, 110.0), MouseButton::Left);
        input.down[0] = false;
        input.released[0] = true;
        let r = frame(&mut ui, input);
        assert!(r.clicked && !r.pressed);
    }

    #[test]
    fn a_disabled_button_does_nothing_but_still_blocks() {
        let mut ui = Ui::new(UiConfig::default());
        let input = press(at(150.0, 110.0), MouseButton::Left);
        ui.begin_frame(input, SCREEN, 1.0);
        let r = ui.button("go", BTN, "Go", false);
        ui.end_frame();
        assert_eq!(r, Response::default());
        assert!(ui.press_consumed(MouseButton::Left));
    }

    #[test]
    fn a_press_over_a_button_is_consumed_and_the_release_passes() {
        let mut ui = Ui::new(UiConfig::default());
        let raw = press(at(150.0, 110.0), MouseButton::Left);
        frame(&mut ui, raw);
        assert!(ui.pointer_over_ui() && ui.press_consumed(MouseButton::Left));
        assert!(ui.button_captured(MouseButton::Left));
        let game = ui.filter_for_game(&raw);
        assert!(
            !game.pressed(MouseButton::Left),
            "the game does not see the press"
        );
        assert!(!game.down(MouseButton::Left), "nor the hold");

        let raw = release(at(150.0, 110.0), MouseButton::Left);
        frame(&mut ui, raw);
        let game = ui.filter_for_game(&raw);
        assert!(
            game.released(MouseButton::Left),
            "the release always reaches the game"
        );
        assert!(
            !ui.button_captured(MouseButton::Left),
            "capture ends with the release"
        );
    }

    #[test]
    fn a_press_outside_the_ui_reaches_the_game_and_keeps_going_over_it() {
        let mut ui = Ui::new(UiConfig::default());
        let raw = press(at(400.0, 400.0), MouseButton::Middle);
        frame(&mut ui, raw);
        let game = ui.filter_for_game(&raw);
        assert!(game.pressed(MouseButton::Middle) && game.down(MouseButton::Middle));
        // The drag moves over the button: still the game's.
        let raw = hold(at(150.0, 110.0), MouseButton::Middle);
        frame(&mut ui, raw);
        assert!(!ui.press_consumed(MouseButton::Middle));
        assert!(ui.filter_for_game(&raw).down(MouseButton::Middle));
    }

    #[test]
    fn a_drag_that_started_on_the_ui_stays_captured_off_it() {
        let mut ui = Ui::new(UiConfig::default());
        frame(&mut ui, press(at(150.0, 110.0), MouseButton::Middle));
        let raw = hold(at(500.0, 500.0), MouseButton::Middle);
        frame(&mut ui, raw);
        assert!(ui.button_captured(MouseButton::Middle));
        assert!(!ui.filter_for_game(&raw).down(MouseButton::Middle));
    }

    #[test]
    fn a_focus_loss_release_ends_the_capture() {
        // The engine releases everything on focus loss: down turns false
        // without the cursor being over anything.
        let mut ui = Ui::new(UiConfig::default());
        frame(&mut ui, press(at(150.0, 110.0), MouseButton::Left));
        let mut lost = UiInput::default();
        lost.released[0] = true;
        frame(&mut ui, lost);
        assert!(!ui.button_captured(MouseButton::Left));
        assert!(ui.filter_for_game(&lost).released(MouseButton::Left));
    }

    #[test]
    fn the_wheel_over_the_ui_is_consumed_elsewhere_it_passes() {
        let mut ui = Ui::new(UiConfig::default());
        let mut raw = at(150.0, 110.0);
        raw.wheel = 2.0;
        frame(&mut ui, raw);
        assert!(ui.wheel_consumed());
        assert_eq!(ui.filter_for_game(&raw).wheel, 0.0);
        let mut raw = at(500.0, 500.0);
        raw.wheel = 2.0;
        frame(&mut ui, raw);
        assert!(!ui.wheel_consumed());
        assert_eq!(ui.filter_for_game(&raw).wheel, 2.0);
        // A frame without wheel movement over the UI consumes nothing.
        frame(&mut ui, at(150.0, 110.0));
        assert!(!ui.wheel_consumed());
    }

    #[test]
    fn panels_block_like_buttons_and_text_does_not() {
        let mut ui = Ui::new(UiConfig::default());
        ui.begin_frame(at(30.0, 12.0), SCREEN, 1.0);
        let bar = ui.panel(Anchor::TopLeft, (0.0, 0.0), (200.0, 40.0));
        ui.text_in(
            Rect::new(300.0, 300.0, 100.0, 20.0),
            Anchor::TopLeft,
            (0.0, 0.0),
            "free",
            Color::WHITE,
        );
        ui.end_frame();
        assert!(bar.contains(30.0, 12.0));
        assert!(ui.pointer_over_ui());
        assert!(ui.blocks_pointer_at(10.0, 10.0));
        assert!(!ui.blocks_pointer_at(310.0, 305.0), "text is not a blocker");
    }

    #[test]
    fn area_tracks_a_drag_that_leaves_it() {
        let mut ui = Ui::new(UiConfig::default());
        let area = Rect::new(600.0, 400.0, 150.0, 150.0);
        let run = |ui: &mut Ui, input: UiInput| {
            ui.begin_frame(input, SCREEN, 1.0);
            let r = ui.area("mini", area);
            ui.end_frame();
            r
        };
        let r = run(&mut ui, press(at(650.0, 450.0), MouseButton::Left));
        assert!(r.pressed && !r.dragging && r.cursor == Some((650.0, 450.0)));
        let r = run(&mut ui, hold(at(900.0, 700.0), MouseButton::Left));
        assert!(
            !r.pressed && r.dragging && !r.hovered,
            "outside the area, still dragging"
        );
        assert_eq!(r.cursor, Some((900.0, 700.0)));
        let r = run(&mut ui, release(at(900.0, 700.0), MouseButton::Left));
        assert!(r.released && !r.dragging);
        let r = run(&mut ui, at(900.0, 700.0));
        assert!(!r.dragging && r.cursor.is_none());
    }

    #[test]
    fn dpi_scale_changes_layout_and_text_size() {
        let mut ui = Ui::new(UiConfig::default());
        ui.begin_frame(UiInput::default(), SCREEN, 2.0);
        assert_eq!(ui.scale(), 2.0);
        let r = ui.place(ui.screen(), Anchor::BottomRight, (8.0, 8.0), (100.0, 50.0));
        assert_eq!(
            r,
            Rect::new(800.0 - 200.0 - 16.0, 600.0 - 100.0 - 16.0, 200.0, 100.0)
        );
        assert_eq!(ui.text_px(), 28.0);
        let wide = ui.measure("Hello").0;
        ui.begin_frame(UiInput::default(), SCREEN, 1.0);
        assert!((wide / ui.measure("Hello").0 - 2.0).abs() < 0.1);
    }

    #[test]
    fn widget_ids_are_stable_and_distinct() {
        assert_eq!(widget_id("a"), widget_id("a"));
        assert_ne!(widget_id("a"), widget_id("b"));
        assert_ne!(widget_id(""), widget_id("a"));
    }
}
