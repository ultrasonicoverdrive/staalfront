//! Input layer: raw events -> [`InputState`] -> named [`Action`]s.
//!
//! `InputState` is fed by plain method calls (the window loop translates
//! winit events), so everything here is testable without a window.
//! Actions are engine-generic names; the game maps them to commands.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use winit::event::{ElementState, MouseButton};
use winit::keyboard::KeyCode;

/// Default width of the screen-edge band, in pixels.
pub const DEFAULT_EDGE_MARGIN: u32 = 4;

/// Engine-generic action names (camera, window, debug). Not game commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Action {
    CameraZoomIn,
    CameraZoomOut,
    CameraPanLeft,
    CameraPanRight,
    CameraPanUp,
    CameraPanDown,
    CameraRotateLeft,
    CameraRotateRight,
    CameraDrag,
    ToggleFullscreen,
    ToggleDebug,
    /// Plays the audio test sound (engine smoke test, not a game command).
    PlayTestSound,
    Quit,
    /// Order the selected units to the cursor (right-click). A game action
    /// named here so that it is rebindable like the rest (WP3.4).
    OrderMove,
    /// Held: orders are added to the queue instead of replacing the orders
    /// (shift).
    OrderQueue,
    /// Stop the selected units.
    OrderStop,
    /// Scatter the selected units.
    OrderScatter,
    /// The selected units hold position.
    OrderHold,
    /// Temporary debug selection until WP3.3: select all own units in view.
    DebugSelectAll,
}

impl Action {
    pub const ALL: [Action; 19] = [
        Action::CameraZoomIn,
        Action::CameraZoomOut,
        Action::CameraPanLeft,
        Action::CameraPanRight,
        Action::CameraPanUp,
        Action::CameraPanDown,
        Action::CameraRotateLeft,
        Action::CameraRotateRight,
        Action::CameraDrag,
        Action::ToggleFullscreen,
        Action::ToggleDebug,
        Action::PlayTestSound,
        Action::Quit,
        Action::OrderMove,
        Action::OrderQueue,
        Action::OrderStop,
        Action::OrderScatter,
        Action::OrderHold,
        Action::DebugSelectAll,
    ];
}

/// Modifier keys (either side counts).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Modifier {
    Alt,
    Ctrl,
    Shift,
}

/// What physically triggers a binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Trigger {
    Key(KeyCode),
    Mouse(MouseButton),
    WheelUp,
    WheelDown,
    EdgeLeft,
    EdgeRight,
    EdgeTop,
    EdgeBottom,
}

/// A trigger plus modifiers that must be held. Extra modifiers are ignored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub trigger: Trigger,
    #[serde(default)]
    pub mods: Vec<Modifier>,
}

impl Binding {
    pub fn new(trigger: Trigger) -> Self {
        Self {
            trigger,
            mods: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub enum BindingsError {
    Io(std::io::Error),
    Parse(ron::error::SpannedError),
}

impl std::fmt::Display for BindingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindingsError::Io(e) => write!(f, "cannot read bindings: {e}"),
            BindingsError::Parse(e) => write!(f, "cannot parse bindings: {e}"),
        }
    }
}

impl std::error::Error for BindingsError {}

/// Action -> bindings map, loaded from RON.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bindings {
    #[serde(default = "default_edge_margin")]
    pub edge_margin: u32,
    pub bindings: BTreeMap<Action, Vec<Binding>>,
}

fn default_edge_margin() -> u32 {
    DEFAULT_EDGE_MARGIN
}

/// Built-in copy of `assets/config/input.ron`, used when the file is missing.
const BUILTIN_RON: &str = include_str!("../../../assets/config/input.ron");

impl Bindings {
    pub fn from_ron_str(text: &str) -> Result<Self, BindingsError> {
        ron::from_str(text).map_err(BindingsError::Parse)
    }

    pub fn load(path: &Path) -> Result<Self, BindingsError> {
        let text = std::fs::read_to_string(path).map_err(BindingsError::Io)?;
        Self::from_ron_str(&text)
    }

    /// The bindings shipped in the repository, compiled in.
    pub fn builtin() -> Self {
        Self::from_ron_str(BUILTIN_RON).expect("built-in input.ron must parse")
    }

    pub fn for_action(&self, action: Action) -> &[Binding] {
        self.bindings.get(&action).map_or(&[], Vec::as_slice)
    }
}

/// Raw input state for the current frame.
#[derive(Debug, Default)]
pub struct InputState {
    keys_down: HashSet<KeyCode>,
    keys_pressed: HashSet<KeyCode>,
    keys_released: HashSet<KeyCode>,
    buttons_down: HashSet<MouseButton>,
    buttons_pressed: HashSet<MouseButton>,
    buttons_released: HashSet<MouseButton>,
    cursor: Option<(f64, f64)>,
    cursor_delta: (f64, f64),
    wheel: f32,
    window_size: (u32, u32),
    focused: bool,
}

/// Pixels of a high-resolution scroll that count as one wheel notch.
pub const PIXELS_PER_NOTCH: f32 = 120.0;

impl InputState {
    pub fn new() -> Self {
        Self {
            focused: true,
            ..Self::default()
        }
    }

    // ---- feeding (called by the window loop) ----

    pub fn key_event(&mut self, key: KeyCode, state: ElementState, repeat: bool) {
        match state {
            ElementState::Pressed => {
                if !repeat && self.keys_down.insert(key) {
                    self.keys_pressed.insert(key);
                }
            }
            ElementState::Released => {
                if self.keys_down.remove(&key) {
                    self.keys_released.insert(key);
                }
            }
        }
    }

    pub fn mouse_button(&mut self, button: MouseButton, state: ElementState) {
        match state {
            ElementState::Pressed => {
                if self.buttons_down.insert(button) {
                    self.buttons_pressed.insert(button);
                }
            }
            ElementState::Released => {
                if self.buttons_down.remove(&button) {
                    self.buttons_released.insert(button);
                }
            }
        }
    }

    pub fn cursor_moved(&mut self, x: f64, y: f64) {
        if let Some((px, py)) = self.cursor {
            self.cursor_delta.0 += x - px;
            self.cursor_delta.1 += y - py;
        }
        self.cursor = Some((x, y));
    }

    pub fn cursor_left(&mut self) {
        self.cursor = None;
    }

    /// Wheel movement in notches; positive = away from the user (zoom in).
    pub fn wheel(&mut self, notches: f32) {
        self.wheel += notches;
    }

    /// Wheel movement from a pixel-based (touchpad) scroll.
    pub fn wheel_pixels(&mut self, pixels: f32) {
        self.wheel(pixels / PIXELS_PER_NOTCH);
    }

    pub fn set_window_size(&mut self, width: u32, height: u32) {
        self.window_size = (width, height);
    }

    /// On focus loss everything is released so nothing stays stuck.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        if !focused {
            self.keys_released.extend(self.keys_down.drain());
            self.buttons_released.extend(self.buttons_down.drain());
            self.cursor = None;
        }
    }

    /// Clear per-frame data (pressed/released/wheel/delta). Call at frame end.
    pub fn end_frame(&mut self) {
        self.keys_pressed.clear();
        self.keys_released.clear();
        self.buttons_pressed.clear();
        self.buttons_released.clear();
        self.wheel = 0.0;
        self.cursor_delta = (0.0, 0.0);
    }

    // ---- queries ----

    pub fn key_down(&self, key: KeyCode) -> bool {
        self.keys_down.contains(&key)
    }
    pub fn key_pressed(&self, key: KeyCode) -> bool {
        self.keys_pressed.contains(&key)
    }
    pub fn key_released(&self, key: KeyCode) -> bool {
        self.keys_released.contains(&key)
    }
    pub fn button_down(&self, b: MouseButton) -> bool {
        self.buttons_down.contains(&b)
    }
    pub fn button_pressed(&self, b: MouseButton) -> bool {
        self.buttons_pressed.contains(&b)
    }
    pub fn button_released(&self, b: MouseButton) -> bool {
        self.buttons_released.contains(&b)
    }
    /// Cursor position in physical pixels; `None` if outside the window.
    pub fn cursor(&self) -> Option<(f64, f64)> {
        self.cursor
    }
    /// Cursor movement this frame in physical pixels.
    pub fn cursor_delta(&self) -> (f64, f64) {
        self.cursor_delta
    }
    /// Wheel notches this frame (positive = zoom in direction).
    pub fn wheel_delta(&self) -> f32 {
        self.wheel
    }
    pub fn window_size(&self) -> (u32, u32) {
        self.window_size
    }
    pub fn focused(&self) -> bool {
        self.focused
    }

    pub fn modifier_down(&self, m: Modifier) -> bool {
        let (a, b) = match m {
            Modifier::Alt => (KeyCode::AltLeft, KeyCode::AltRight),
            Modifier::Ctrl => (KeyCode::ControlLeft, KeyCode::ControlRight),
            Modifier::Shift => (KeyCode::ShiftLeft, KeyCode::ShiftRight),
        };
        self.keys_down.contains(&a) || self.keys_down.contains(&b)
    }

    /// Is the cursor within `margin` pixels of the given window edge?
    /// Only while focused and inside the window.
    pub fn at_edge(&self, trigger: Trigger, margin: u32) -> bool {
        let (Some((x, y)), true) = (self.cursor, self.focused) else {
            return false;
        };
        let (w, h) = self.window_size;
        if w == 0 || h == 0 {
            return false;
        }
        let m = f64::from(margin);
        match trigger {
            Trigger::EdgeLeft => x < m,
            Trigger::EdgeRight => x >= f64::from(w) - m,
            Trigger::EdgeTop => y < m,
            Trigger::EdgeBottom => y >= f64::from(h) - m,
            _ => false,
        }
    }

    /// `(held, pulse)` for a binding this frame. `pulse` = it went active
    /// this frame (also true for a tap shorter than a frame and for wheel).
    fn binding_state(&self, b: &Binding, edge_margin: u32) -> (bool, bool) {
        if !b.mods.iter().all(|m| self.modifier_down(*m)) {
            return (false, false);
        }
        match b.trigger {
            Trigger::Key(k) => (self.key_down(k), self.key_pressed(k)),
            Trigger::Mouse(m) => (self.button_down(m), self.button_pressed(m)),
            Trigger::WheelUp => {
                let on = self.wheel > 0.0;
                (on, on)
            }
            Trigger::WheelDown => {
                let on = self.wheel < 0.0;
                (on, on)
            }
            edge => (self.at_edge(edge, edge_margin), false),
        }
    }
}

/// Which actions are held / pressed / released this frame.
#[derive(Clone, Debug, Default)]
pub struct ActionState {
    held: BTreeSet<Action>,
    pressed: BTreeSet<Action>,
    released: BTreeSet<Action>,
}

impl ActionState {
    /// Recompute from the input; call once per frame before querying.
    pub fn update(&mut self, input: &InputState, bindings: &Bindings) {
        let previous = std::mem::take(&mut self.held);
        self.pressed.clear();
        self.released.clear();
        for (&action, list) in &bindings.bindings {
            let mut held = false;
            let mut pulse = false;
            for b in list {
                let (h, p) = input.binding_state(b, bindings.edge_margin);
                held |= h;
                pulse |= p;
            }
            if held {
                self.held.insert(action);
            }
            let was = previous.contains(&action);
            if pulse || (held && !was) {
                self.pressed.insert(action);
            }
            if !held && was {
                self.released.insert(action);
            }
        }
    }

    pub fn held(&self, a: Action) -> bool {
        self.held.contains(&a)
    }
    pub fn pressed(&self, a: Action) -> bool {
        self.pressed.contains(&a)
    }
    pub fn released(&self, a: Action) -> bool {
        self.released.contains(&a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(input: &InputState, b: &Bindings) -> ActionState {
        let mut s = ActionState::default();
        s.update(input, b);
        s
    }

    fn sized() -> InputState {
        let mut i = InputState::new();
        i.set_window_size(800, 600);
        i
    }

    #[test]
    fn builtin_parses_and_binds_every_action() {
        let b = Bindings::builtin();
        for a in Action::ALL {
            assert!(!b.for_action(a).is_empty(), "{a:?} has no binding");
        }
        assert_eq!(b.edge_margin, 4);
    }

    #[test]
    fn builtin_binds_no_key_or_button_to_two_actions() {
        let b = Bindings::builtin();
        let mut seen: Vec<(Binding, Action)> = Vec::new();
        for a in Action::ALL {
            for binding in b.for_action(a) {
                if let Some((_, other)) = seen.iter().find(|(x, _)| x == binding) {
                    panic!("{binding:?} is bound to {other:?} and {a:?}");
                }
                seen.push((binding.clone(), a));
            }
        }
    }

    #[test]
    fn the_order_keys_are_not_camera_keys() {
        let b = Bindings::builtin();
        for (action, key) in [
            (Action::OrderStop, KeyCode::KeyC),
            (Action::OrderScatter, KeyCode::KeyX),
            (Action::OrderHold, KeyCode::KeyH),
            (Action::DebugSelectAll, KeyCode::F2),
        ] {
            assert_eq!(b.for_action(action), [Binding::new(Trigger::Key(key))]);
        }
        let right = [Binding::new(Trigger::Mouse(MouseButton::Right))];
        assert_eq!(b.for_action(Action::OrderMove), right);
        // The camera still has WASD.
        for key in [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD] {
            assert!(
                b.bindings
                    .values()
                    .flatten()
                    .any(|x| x.trigger == Trigger::Key(key)),
                "{key:?}"
            );
        }
        assert_eq!(b.for_action(Action::OrderQueue).len(), 2);
    }

    #[test]
    fn ron_parsing_basics() {
        let b = Bindings::from_ron_str(
            "(bindings: { Quit: [(trigger: Key(KeyQ), mods: [Ctrl, Shift]), (trigger: Mouse(Right))] })",
        )
        .unwrap();
        assert_eq!(b.edge_margin, DEFAULT_EDGE_MARGIN); // default applies
        let q = b.for_action(Action::Quit);
        assert_eq!(q.len(), 2);
        assert_eq!(q[0].trigger, Trigger::Key(KeyCode::KeyQ));
        assert_eq!(q[0].mods, [Modifier::Ctrl, Modifier::Shift]);
        assert_eq!(q[1].trigger, Trigger::Mouse(MouseButton::Right));
        assert!(q[1].mods.is_empty());
        assert!(b.for_action(Action::ToggleDebug).is_empty());
    }

    #[test]
    fn ron_rejects_unknown_names() {
        assert!(Bindings::from_ron_str("(bindings: { Fire: [] })").is_err());
        assert!(
            Bindings::from_ron_str("(bindings: { Quit: [(trigger: Key(NoSuchKey))] })").is_err()
        );
        assert!(Bindings::from_ron_str("(bindings: {").is_err());
    }

    #[test]
    fn ron_round_trip() {
        let b = Bindings::builtin();
        let text = ron::to_string(&b).unwrap();
        assert_eq!(Bindings::from_ron_str(&text).unwrap(), b);
    }

    #[test]
    fn key_press_hold_release_lifecycle() {
        let b = Bindings::builtin();
        let mut i = sized();
        let mut s = ActionState::default();

        i.key_event(KeyCode::KeyW, ElementState::Pressed, false);
        s.update(&i, &b);
        assert!(s.pressed(Action::CameraPanUp) && s.held(Action::CameraPanUp));
        i.end_frame();

        i.key_event(KeyCode::KeyW, ElementState::Pressed, true); // OS repeat
        s.update(&i, &b);
        assert!(s.held(Action::CameraPanUp) && !s.pressed(Action::CameraPanUp));
        i.end_frame();

        i.key_event(KeyCode::KeyW, ElementState::Released, false);
        s.update(&i, &b);
        assert!(s.released(Action::CameraPanUp) && !s.held(Action::CameraPanUp));
        i.end_frame();

        s.update(&i, &b);
        assert!(!s.released(Action::CameraPanUp));
    }

    #[test]
    fn two_keys_one_action_stays_held() {
        let b = Bindings::builtin();
        let mut i = sized();
        i.key_event(KeyCode::KeyA, ElementState::Pressed, false);
        i.key_event(KeyCode::ArrowLeft, ElementState::Pressed, false);
        i.key_event(KeyCode::KeyA, ElementState::Released, false);
        let s = eval(&i, &b);
        assert!(s.held(Action::CameraPanLeft));
    }

    #[test]
    fn tap_within_one_frame_still_counts_as_pressed() {
        let b = Bindings::builtin();
        let mut i = sized();
        i.key_event(KeyCode::F1, ElementState::Pressed, false);
        i.key_event(KeyCode::F1, ElementState::Released, false);
        let s = eval(&i, &b);
        assert!(s.pressed(Action::ToggleDebug));
        assert!(!s.held(Action::ToggleDebug));
    }

    #[test]
    fn alt_enter_needs_alt_but_f11_does_not() {
        let b = Bindings::builtin();

        let mut i = sized();
        i.key_event(KeyCode::Enter, ElementState::Pressed, false);
        assert!(!eval(&i, &b).pressed(Action::ToggleFullscreen));

        let mut i = sized();
        i.key_event(KeyCode::AltRight, ElementState::Pressed, false);
        i.key_event(KeyCode::Enter, ElementState::Pressed, false);
        assert!(eval(&i, &b).pressed(Action::ToggleFullscreen));

        let mut i = sized();
        i.key_event(KeyCode::F11, ElementState::Pressed, false);
        assert!(eval(&i, &b).pressed(Action::ToggleFullscreen));
    }

    #[test]
    fn wheel_maps_to_zoom_each_frame() {
        let b = Bindings::builtin();
        let mut i = sized();
        let mut s = ActionState::default();
        i.wheel(2.0);
        s.update(&i, &b);
        assert!(s.pressed(Action::CameraZoomIn) && !s.held(Action::CameraZoomOut));
        assert_eq!(i.wheel_delta(), 2.0);
        i.end_frame();
        i.wheel(1.0); // consecutive frames both register
        s.update(&i, &b);
        assert!(s.pressed(Action::CameraZoomIn));
        i.end_frame();
        i.wheel_pixels(-60.0);
        s.update(&i, &b);
        assert!(s.pressed(Action::CameraZoomOut));
        assert_eq!(i.wheel_delta(), -0.5);
        i.end_frame();
        s.update(&i, &b);
        assert!(!s.held(Action::CameraZoomOut) && !s.pressed(Action::CameraZoomOut));
    }

    #[test]
    fn middle_mouse_drag_with_delta() {
        let b = Bindings::builtin();
        let mut i = sized();
        i.cursor_moved(100.0, 100.0);
        i.end_frame();
        i.mouse_button(MouseButton::Middle, ElementState::Pressed);
        i.cursor_moved(130.0, 90.0);
        let s = eval(&i, &b);
        assert!(s.held(Action::CameraDrag));
        assert_eq!(i.cursor_delta(), (30.0, -10.0));
        i.end_frame();
        assert_eq!(i.cursor_delta(), (0.0, 0.0));
    }

    #[test]
    fn edge_detection() {
        let b = Bindings::builtin();
        let cases = [
            ((0.0, 300.0), Action::CameraPanLeft),
            ((799.0, 300.0), Action::CameraPanRight),
            ((400.0, 0.0), Action::CameraPanUp),
            ((400.0, 599.0), Action::CameraPanDown),
        ];
        for ((x, y), expected) in cases {
            let mut i = sized();
            i.cursor_moved(x, y);
            let s = eval(&i, &b);
            for a in [
                Action::CameraPanLeft,
                Action::CameraPanRight,
                Action::CameraPanUp,
                Action::CameraPanDown,
            ] {
                assert_eq!(s.held(a), a == expected, "cursor ({x},{y}) action {a:?}");
            }
        }
        // Middle of the screen: nothing.
        let mut i = sized();
        i.cursor_moved(400.0, 300.0);
        assert!(!eval(&i, &b).held(Action::CameraPanLeft));
        // Band is `margin` wide: 3 px in is edge, 4 px in is not.
        let mut i = sized();
        i.cursor_moved(3.0, 300.0);
        assert!(eval(&i, &b).held(Action::CameraPanLeft));
        i.cursor_moved(4.0, 300.0);
        assert!(!eval(&i, &b).held(Action::CameraPanLeft));
    }

    #[test]
    fn no_edge_scroll_when_cursor_outside_or_unfocused() {
        let b = Bindings::builtin();
        let mut i = sized();
        i.cursor_moved(0.0, 300.0);
        i.cursor_left();
        assert!(!eval(&i, &b).held(Action::CameraPanLeft));

        let mut i = sized();
        i.cursor_moved(0.0, 300.0);
        i.set_focused(false);
        i.cursor_moved(0.0, 300.0);
        assert!(!eval(&i, &b).held(Action::CameraPanLeft));
    }

    #[test]
    fn focus_loss_releases_everything() {
        let b = Bindings::builtin();
        let mut i = sized();
        let mut s = ActionState::default();
        i.key_event(KeyCode::KeyD, ElementState::Pressed, false);
        s.update(&i, &b);
        i.end_frame();
        i.set_focused(false);
        s.update(&i, &b);
        assert!(!s.held(Action::CameraPanRight));
        assert!(s.released(Action::CameraPanRight));
        assert!(!i.key_down(KeyCode::KeyD));
    }

    #[test]
    fn unbound_action_never_fires() {
        let b = Bindings::from_ron_str("(bindings: {})").unwrap();
        let mut i = sized();
        i.key_event(KeyCode::Escape, ElementState::Pressed, false);
        assert!(!eval(&i, &b).pressed(Action::Quit));
    }
}
