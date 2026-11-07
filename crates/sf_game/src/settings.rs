//! The player's settings (WP11.5a): keys, graphics and accessibility, kept in a RON file in the user's
//! configuration directory, read at the start, written on every change, and replaced by the defaults
//! when the file is missing or cannot be used. Nothing here touches the shipped `input.ron`: the keys
//! the player changed are kept as overrides over it ([`Settings::effective_bindings`]).
//!
//! Where the file lives, per operating system ([`settings_path`]): Linux `$XDG_CONFIG_HOME/staalfront/settings.ron`,
//! else `$HOME/.config/staalfront/settings.ron`; Windows `%APPDATA%\Staalfront\settings.ron`; macOS
//! `$HOME/Library/Application Support/Staalfront/settings.ron`. The environment variable
//! `STAALFRONT_SETTINGS` names another file (the tests and the screenshots use it).
//!
//! Presentation only: the simulation never hears of any of this. The rules are in
//! `docs/rules/menus.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sf_engine::input::{Binding, Trigger};
use sf_engine::winit::keyboard::KeyCode;
use sf_engine::{Action, Bindings};

use crate::menu_look::MenuLook;

/// The environment variable that names the settings file.
pub const ENV_PATH: &str = "STAALFRONT_SETTINGS";

/// The file name inside the configuration directory.
pub const FILE_NAME: &str = "settings.ron";

/// Windowed or borderless fullscreen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowMode {
    #[default]
    Windowed,
    Borderless,
}

/// Which set of player colours is used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaletteChoice {
    #[default]
    Standard,
    ColourBlind,
}

/// Everything the player can set. A field that is missing from the file keeps its default; a field
/// the file has that this version does not know is ignored (a later version may add the sound
/// settings), and is dropped the next time the file is written.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub window: WindowMode,
    /// The most frames a second; `None` = no cap.
    pub frame_cap: Option<u32>,
    pub vsync: bool,
    /// The factor the UI is drawn at (the window's DPI factor is applied on top).
    pub ui_scale: f32,
    pub palette: PaletteChoice,
    /// The keys the player changed: the whole binding list of each such action, over `input.ron`.
    pub keys: BTreeMap<Action, Vec<Binding>>,
}

impl Default for Settings {
    fn default() -> Self {
        let d = &MenuLook::embedded().defaults;
        Settings {
            window: d.window,
            frame_cap: d.frame_cap,
            vsync: d.vsync,
            ui_scale: d.ui_scale,
            palette: d.palette,
            keys: BTreeMap::new(),
        }
    }
}

/// The least and the most a hand-edited frame cap may be.
pub const FRAME_CAP_RANGE: std::ops::RangeInclusive<u32> = 10..=1000;

/// Why a key could not be given to an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebindError {
    /// The key opens the menu (Escape) and nothing else can have it.
    Reserved,
    /// The action is the menu key itself.
    NotRebindable,
    /// Another action already has this key.
    Conflict(Action),
}

/// Whether the player can give `action` another key: all but the menu key.
pub fn is_rebindable(action: Action) -> bool {
    action != Action::Quit
}

/// The first binding of `list` that is a key.
fn first_key(list: &[Binding]) -> Option<usize> {
    list.iter()
        .position(|b| matches!(b.trigger, Trigger::Key(_)))
}

impl Settings {
    /// Brings every value into what the game can use, and says what it changed: the UI scale to the
    /// nearest choice of the menu, a frame cap into [`FRAME_CAP_RANGE`] (0 is no cap), and no override
    /// for the menu key or with an empty list. The key overrides are checked against `shipped`, the
    /// bindings the game runs with (the `input.ron` read from disk), see [`Settings::sanitize_keys`].
    pub fn sanitize(&mut self, shipped: &Bindings) -> Vec<String> {
        let mut said = Vec::new();
        let look = MenuLook::embedded();
        let wanted = self.ui_scale;
        let nearest = if wanted.is_finite() {
            look.ui_scales
                .iter()
                .copied()
                .min_by(|a, b| (a - wanted).abs().total_cmp(&(b - wanted).abs()))
        } else {
            None
        };
        let nearest = nearest.unwrap_or(look.defaults.ui_scale);
        if nearest != wanted {
            said.push(format!(
                "ui_scale {wanted} is not a choice; using {nearest}"
            ));
            self.ui_scale = nearest;
        }
        match self.frame_cap {
            Some(0) => self.frame_cap = None,
            Some(hz) if !FRAME_CAP_RANGE.contains(&hz) => {
                let clamped = hz.clamp(*FRAME_CAP_RANGE.start(), *FRAME_CAP_RANGE.end());
                said.push(format!("frame_cap {hz} is out of range; using {clamped}"));
                self.frame_cap = Some(clamped);
            }
            _ => {}
        }
        said.extend(self.sanitize_keys(shipped));
        said
    }

    /// The key part of [`Settings::sanitize`], also run again when `input.ron` is hot-reloaded (the
    /// overrides are checked against the new file): no override for the menu key or with an empty
    /// list, none that uses Esc or a key another action has in `shipped`. Says what it dropped.
    pub fn sanitize_keys(&mut self, shipped: &Bindings) -> Vec<String> {
        let mut said = Vec::new();
        let before = self.keys.len();
        self.keys
            .retain(|action, list| is_rebindable(*action) && !list.is_empty());
        if self.keys.len() != before {
            said.push("ignored key overrides for the menu key or with no key".to_owned());
        }
        let refused = self.drop_refused_keys(shipped);
        if !refused.is_empty() {
            said.push(format!(
                "ignored key overrides that use Esc or a key another action has (the shipped keys stay): {}",
                refused
                    .iter()
                    .map(|a| format!("{a:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        said
    }

    /// The rule of [`Settings::rebind`] for a file edited by hand: an override that gives an action
    /// Escape, or a key that another action has (whatever its modifiers), is dropped, so the action
    /// keeps the shipped keys. Only keys the shipped list of the action does not have are checked (a
    /// key the shipped file lets two actions share stays with both). The actions are taken in order;
    /// an override kept earlier counts as the other action's key for a later one. Returns the
    /// actions dropped.
    fn drop_refused_keys(&mut self, shipped: &Bindings) -> Vec<Action> {
        let mut refused = Vec::new();
        // The overrides are put back one by one: `self.keys` holds the ones kept so far.
        for (action, list) in std::mem::take(&mut self.keys) {
            let own = shipped.for_action(action);
            let now = self.effective_bindings(shipped);
            let bad = list.iter().any(|b| {
                let Trigger::Key(key) = b.trigger else {
                    return false;
                };
                if own.iter().any(|o| o.trigger == Trigger::Key(key)) {
                    return false;
                }
                key == KeyCode::Escape
                    || now.bindings.iter().any(|(other, others)| {
                        *other != action && others.iter().any(|o| o.trigger == Trigger::Key(key))
                    })
            });
            if bad {
                refused.push(action);
            } else {
                self.keys.insert(action, list);
            }
        }
        refused
    }

    /// The bindings the game runs with: `shipped` with the player's lists in place of the shipped
    /// ones, for the actions the player changed.
    pub fn effective_bindings(&self, shipped: &Bindings) -> Bindings {
        let mut bindings = shipped.clone();
        for (action, list) in &self.keys {
            if is_rebindable(*action) {
                bindings.bindings.insert(*action, list.clone());
            }
        }
        bindings
    }

    /// Gives `action` the key `key`: its first key binding is replaced by the plain key (the other
    /// keys, the mouse, wheel and edge bindings of the action stay), or the key is added when the
    /// action had none. Refused, and nothing changes, when the key is Escape, or when another action
    /// has a binding on the same key whatever its modifiers (no exceptions: the pair that shares
    /// Shift in `input.ron` is restored with a reset, not by hand).
    pub fn rebind(
        &mut self,
        shipped: &Bindings,
        action: Action,
        key: KeyCode,
    ) -> Result<(), RebindError> {
        if !is_rebindable(action) {
            return Err(RebindError::NotRebindable);
        }
        if key == KeyCode::Escape {
            return Err(RebindError::Reserved);
        }
        let now = self.effective_bindings(shipped);
        for (other, list) in &now.bindings {
            if *other != action && list.iter().any(|b| b.trigger == Trigger::Key(key)) {
                return Err(RebindError::Conflict(*other));
            }
        }
        let mut list = now.for_action(action).to_vec();
        let new = Binding::new(Trigger::Key(key));
        match first_key(&list) {
            Some(i) => list[i] = new,
            None => list.push(new),
        }
        self.set_keys(shipped, action, list);
        Ok(())
    }

    /// Back to the shipped keys of `action`.
    pub fn reset_key(&mut self, action: Action) {
        self.keys.remove(&action);
    }

    /// Back to the shipped keys of every action.
    pub fn reset_all_keys(&mut self) {
        self.keys.clear();
    }

    /// Keeps `list` for `action` unless it is what the shipped file says (then there is nothing to keep).
    fn set_keys(&mut self, shipped: &Bindings, action: Action, list: Vec<Binding>) {
        if shipped.for_action(action) == list.as_slice() {
            self.keys.remove(&action);
        } else {
            self.keys.insert(action, list);
        }
    }
}

/// The operating systems that keep their configuration in different places.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Linux,
    Windows,
    MacOs,
}

impl Os {
    /// The system this was built for.
    pub fn current() -> Os {
        if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

/// Where the settings file is for `os`, with `env` answering environment variables: the file named by
/// [`ENV_PATH`] if set (and not empty), else the user's configuration directory (see the module
/// documentation). `None` when no such directory is known (then nothing is read or written).
pub fn settings_path(os: Os, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let get = |name: &str| env(name).filter(|v| !v.is_empty());
    if let Some(file) = get(ENV_PATH) {
        return Some(PathBuf::from(file));
    }
    match os {
        Os::Linux => get("XDG_CONFIG_HOME")
            .map(|d| PathBuf::from(d).join("staalfront"))
            .or_else(|| get("HOME").map(|h| PathBuf::from(h).join(".config/staalfront"))),
        Os::Windows => get("APPDATA").map(|d| PathBuf::from(d).join("Staalfront")),
        Os::MacOs => {
            get("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/Staalfront"))
        }
    }
    .map(|dir| dir.join(FILE_NAME))
}

/// [`settings_path`] for this process.
pub fn current_settings_path() -> Option<PathBuf> {
    settings_path(Os::current(), &|name| std::env::var(name).ok())
}

/// The settings file this run reads and writes. The engine demo and the benchmark never touch it (they
/// are measurements); a `--shots` run only when [`ENV_PATH`] names a file (so that a screenshot does not
/// depend on whoever ran it); every other run uses the user's configuration directory.
pub fn path_for(
    options: &crate::args::Options,
    os: Os,
    env: &dyn Fn(&str) -> Option<String>,
) -> Option<PathBuf> {
    if options.preset == crate::args::Preset::Demo || options.bench.is_some() {
        return None;
    }
    if options.shots.is_some() && env(ENV_PATH).is_none_or(|v| v.is_empty()) {
        return None;
    }
    settings_path(os, env)
}

/// How the window starts, from the command line and the settings: a flag of the command line wins
/// over the setting, and the setting over the default. A benchmark or the demo ignores the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Startup {
    pub screen: sf_engine::ScreenMode,
    pub present: sf_render::Present,
    pub frame_cap: Option<u32>,
}

/// [`Startup`] for `options` and `settings`.
pub fn startup(options: &crate::args::Options, settings: &Settings) -> Startup {
    use sf_engine::ScreenMode;
    use sf_render::Present;
    let own = Startup {
        screen: options.screen,
        present: options.present,
        frame_cap: options.frame_cap,
    };
    if options.preset == crate::args::Preset::Demo || options.bench.is_some() {
        return own;
    }
    Startup {
        screen: if !options.screen_given && settings.window == WindowMode::Borderless {
            ScreenMode::Borderless
        } else {
            own.screen
        },
        present: if !options.present_given && !settings.vsync {
            Present::Uncapped
        } else {
            own.present
        },
        frame_cap: own.frame_cap.or(settings.frame_cap),
    }
}

/// What reading the file gave.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub settings: Settings,
    /// What was wrong, to say once at the start: an unreadable or invalid file, or values that were
    /// changed into ones the game can use.
    pub warnings: Vec<String>,
}

/// Reads `path`. A file that is not there gives the defaults and no warning (it is the first run);
/// one that cannot be read or parsed gives the defaults and one warning that says it will be replaced
/// on the next change; one with unusable values keeps the rest and warns about those. `shipped` is
/// the `input.ron` the game runs with; the key overrides are checked against it.
pub fn load_from(path: &Path, shipped: &Bindings) -> Loaded {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Loaded {
                settings: Settings::default(),
                warnings: Vec::new(),
            };
        }
        Err(e) => return unusable(path, &e.to_string()),
    };
    match ron::from_str::<Settings>(&text) {
        Ok(mut settings) => {
            let warnings = settings
                .sanitize(shipped)
                .into_iter()
                .map(|w| format!("{}: {w}", path.display()))
                .collect();
            Loaded { settings, warnings }
        }
        Err(e) => unusable(path, &e.to_string()),
    }
}

fn unusable(path: &Path, why: &str) -> Loaded {
    Loaded {
        settings: Settings::default(),
        warnings: vec![format!(
            "{}: {why}; using the default settings (the file is replaced on the next change)",
            path.display()
        )],
    }
}

/// Writes `settings` to `path`, making the directory if needed. The file is written beside the
/// target and renamed over it, so a crash cannot leave half a file.
pub fn save_to(path: &Path, settings: &Settings) -> Result<(), String> {
    let text = ron::ser::to_string_pretty(settings, ron::ser::PrettyConfig::default())
        .map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("ron.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The settings of a run together with where they are kept, and the one-time warning of a failed
/// write.
#[derive(Debug)]
pub struct Store {
    path: Option<PathBuf>,
    warned_write: bool,
}

impl Store {
    pub fn new(path: Option<PathBuf>) -> Store {
        Store {
            path,
            warned_write: false,
        }
    }

    /// Where the file is, if anywhere.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// A write has failed (and was said).
    pub fn warned(&self) -> bool {
        self.warned_write
    }

    /// Writes the settings. A failure is said once on stderr and never stops the game.
    pub fn save(&mut self, settings: &Settings) {
        let Some(path) = &self.path else { return };
        if let Err(e) = save_to(path, settings)
            && !self.warned_write
        {
            self.warned_write = true;
            eprintln!("sf_game: cannot save the settings: {e}");
        }
    }
}

/// A key as the player reads it: `KeyW` is `W`, `ArrowLeft` is `Left`, `ShiftLeft` is `Left Shift`,
/// `Numpad1` is `Num 1`, `Escape` is `Esc`; any other name is split into words (`PageUp` is `Page Up`).
pub fn key_name(key: KeyCode) -> String {
    let debug = format!("{key:?}");
    let name = debug.as_str();
    if name == "Escape" {
        return "Esc".to_owned();
    }
    if let Some(letter) = name.strip_prefix("Key").filter(|l| l.len() == 1) {
        return letter.to_owned();
    }
    if let Some(digit) = name.strip_prefix("Digit") {
        return digit.to_owned();
    }
    if let Some(rest) = name.strip_prefix("Numpad") {
        return format!("Num {}", words(rest));
    }
    if let Some(rest) = name.strip_prefix("Arrow") {
        return words(rest);
    }
    for (suffix, side) in [("Left", "Left"), ("Right", "Right")] {
        if let Some(base) = name.strip_suffix(suffix)
            && matches!(base, "Shift" | "Control" | "Alt" | "Super")
        {
            let base = if base == "Control" { "Ctrl" } else { base };
            return format!("{side} {base}");
        }
    }
    words(name)
}

/// `PageUp` as `Page Up`: a space before each capital letter after the first.
fn words(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// A binding as the player reads it: modifiers first (`Alt+Enter`), a mouse button as `Left mouse`,
/// the wheel and the screen edges by name.
pub fn binding_name(binding: &Binding) -> String {
    use sf_engine::input::Modifier;
    use sf_engine::winit::event::MouseButton;
    let mut out = String::new();
    for m in &binding.mods {
        out.push_str(match m {
            Modifier::Alt => "Alt+",
            Modifier::Ctrl => "Ctrl+",
            Modifier::Shift => "Shift+",
        });
    }
    out.push_str(&match binding.trigger {
        Trigger::Key(k) => key_name(k),
        Trigger::Mouse(MouseButton::Left) => "Left mouse".to_owned(),
        Trigger::Mouse(MouseButton::Right) => "Right mouse".to_owned(),
        Trigger::Mouse(MouseButton::Middle) => "Middle mouse".to_owned(),
        Trigger::Mouse(MouseButton::Back) => "Back mouse".to_owned(),
        Trigger::Mouse(MouseButton::Forward) => "Forward mouse".to_owned(),
        Trigger::Mouse(MouseButton::Other(n)) => format!("Mouse {n}"),
        Trigger::WheelUp => "Wheel up".to_owned(),
        Trigger::WheelDown => "Wheel down".to_owned(),
        Trigger::EdgeLeft => "Edge left".to_owned(),
        Trigger::EdgeRight => "Edge right".to_owned(),
        Trigger::EdgeTop => "Edge top".to_owned(),
        Trigger::EdgeBottom => "Edge bottom".to_owned(),
    });
    out
}

/// The key a row of the key list shows for `action`: the first key binding, as the player reads it.
/// `None` when the action has no key.
pub fn primary_key_name(bindings: &Bindings, action: Action) -> Option<String> {
    bindings
        .for_action(action)
        .iter()
        .find(|b| matches!(b.trigger, Trigger::Key(_)))
        .map(binding_name)
}

#[cfg(test)]
mod tests;
