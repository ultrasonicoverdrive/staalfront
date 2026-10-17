//! Keys and clicks as the player understands them.
//!
//! The player does not know about a window system. A host (the viewer, the
//! game) translates its own key events into [`Key`] and asks [`Bindings`] which
//! [`Action`] that is; [`crate::Player::apply`] carries the action out.
//!
//! Two presets: [`Bindings::game`] is what a player of the game gets in a
//! briefing (Esc, Space or a click skips to the end; one configurable key
//! jumps to the next shot), [`Bindings::viewer`] is for a writer previewing
//! a scene (Space pauses, the arrow keys seek). Both are plain data and can
//! be written as RON (`Bindings` derives `Serialize` and `Deserialize`), so
//! the key for "next shot" can be set in a settings file.

use serde::{Deserialize, Serialize};

use crate::math::Ms;

/// A key or button, reduced to what the player cares about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Key {
    Escape,
    Space,
    Enter,
    Tab,
    Left,
    Right,
    Home,
    End,
    /// The primary mouse button, or a tap.
    Click,
    /// A letter or digit key, as a lower-case character.
    Char(char),
}

/// What the player can be told to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    TogglePause,
    SkipToEnd,
    NextShot,
    Restart,
    SeekBack,
    SeekForward,
}

/// Which keys do what.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bindings {
    pub toggle_pause: Vec<Key>,
    pub skip_to_end: Vec<Key>,
    /// The configurable key: jump to the start of the next shot.
    pub next_shot: Vec<Key>,
    pub restart: Vec<Key>,
    pub seek_back: Vec<Key>,
    pub seek_forward: Vec<Key>,
    /// How far the seek keys jump.
    pub seek_step_ms: Ms,
}

impl Default for Bindings {
    fn default() -> Bindings {
        Bindings::game()
    }
}

impl Bindings {
    /// Briefings and interludes in the game: Esc, Space or a click skips the
    /// scene; Enter jumps to the next shot (change it with `next_shot`).
    pub fn game() -> Bindings {
        Bindings {
            toggle_pause: vec![],
            skip_to_end: vec![Key::Escape, Key::Space, Key::Click],
            next_shot: vec![Key::Enter],
            restart: vec![],
            seek_back: vec![],
            seek_forward: vec![],
            seek_step_ms: 2000,
        }
    }

    /// The viewer for writers: Space or a click pauses, `N` or Tab jumps to
    /// the next shot, the arrows seek 2 s, Home restarts, End skips to the
    /// end. (The viewer itself quits on Esc before asking.)
    pub fn viewer() -> Bindings {
        Bindings {
            toggle_pause: vec![Key::Space, Key::Click],
            skip_to_end: vec![Key::End],
            next_shot: vec![Key::Char('n'), Key::Tab],
            restart: vec![Key::Home, Key::Char('r')],
            seek_back: vec![Key::Left],
            seek_forward: vec![Key::Right],
            seek_step_ms: 2000,
        }
    }

    /// The action for `key`, if any. When a key is listed twice the first of
    /// pause, skip, next shot, restart, seek back, seek forward wins.
    pub fn action(&self, key: Key) -> Option<Action> {
        let table = [
            (&self.toggle_pause, Action::TogglePause),
            (&self.skip_to_end, Action::SkipToEnd),
            (&self.next_shot, Action::NextShot),
            (&self.restart, Action::Restart),
            (&self.seek_back, Action::SeekBack),
            (&self.seek_forward, Action::SeekForward),
        ];
        table
            .into_iter()
            .find(|(keys, _)| keys.contains(&key))
            .map(|(_, a)| a)
    }

    /// Read bindings from RON text; missing fields keep the game preset.
    pub fn from_ron_str(text: &str) -> Result<Bindings, ron::error::SpannedError> {
        ron::from_str(text)
    }
}

impl crate::player::Player {
    /// A key press: look it up and carry the action out. Returns the action
    /// that was taken (`None` for a key that does nothing).
    pub fn press(&mut self, key: Key, bindings: &Bindings) -> Option<Action> {
        let action = bindings.action(key)?;
        self.apply(action, bindings.seek_step_ms);
        Some(action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::load_str;
    use crate::player::Player;

    #[test]
    fn the_game_preset_skips_on_escape_space_and_click() {
        let b = Bindings::game();
        for k in [Key::Escape, Key::Space, Key::Click] {
            assert_eq!(b.action(k), Some(Action::SkipToEnd), "{k:?}");
        }
        assert_eq!(b.action(Key::Enter), Some(Action::NextShot));
        assert_eq!(b.action(Key::Char('x')), None);
    }

    #[test]
    fn the_viewer_preset_pauses_on_space() {
        let b = Bindings::viewer();
        assert_eq!(b.action(Key::Space), Some(Action::TogglePause));
        assert_eq!(b.action(Key::Left), Some(Action::SeekBack));
        assert_eq!(b.action(Key::Home), Some(Action::Restart));
        assert_eq!(b.action(Key::Escape), None, "the viewer handles Esc itself");
    }

    #[test]
    fn the_next_shot_key_is_configurable_from_ron() {
        let b = Bindings::from_ron_str("(next_shot: [Char('k')])").expect("ron");
        assert_eq!(b.action(Key::Char('k')), Some(Action::NextShot));
        assert_eq!(b.action(Key::Enter), None, "replaced, not added to");
        // The rest keeps the game preset.
        assert_eq!(b.action(Key::Escape), Some(Action::SkipToEnd));
        assert!(
            Bindings::from_ron_str("(nxt_shot: [])").is_err(),
            "typos are errors"
        );
    }

    #[test]
    fn pressing_keys_drives_the_player() {
        let scene = load_str(
            r##"#![enable(implicit_some)]
            Scene(name: "k", timeline: (tracks: [(name: "c", events: [
                (at: 0, kind: CameraCut(shot: "a")),
                (at: 1000, kind: CameraCut(shot: "b")),
                (at: 2000, kind: Wait(duration_ms: 1000)),
            ])]))"##,
            None,
        )
        .unwrap()
        .scene;
        let mut p = Player::new(scene);
        let b = Bindings::viewer();
        assert_eq!(p.press(Key::Space, &b), Some(Action::TogglePause));
        assert!(p.is_paused());
        p.press(Key::Right, &b);
        assert_eq!(p.time_ms(), 2000);
        p.press(Key::Left, &b);
        assert_eq!(p.time_ms(), 0);
        p.press(Key::Char('n'), &b);
        assert_eq!(p.time_ms(), 1000);
        assert_eq!(p.press(Key::Char('q'), &b), None);
        p.press(Key::End, &b);
        assert!(p.is_finished());
        // The game preset: a click ends the scene.
        let mut p = Player::new(p.scene().clone());
        p.press(Key::Click, &Bindings::game());
        assert!(p.is_finished());
    }
}
