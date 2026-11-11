//! How the menus and the settings screen look and what they say (WP11.5a): every size, colour and word
//! in `assets/config/menu_look.ron`, compiled in like the other looks and checked when it is parsed.
//! Presentation only: nothing here reaches the simulation. The rules are in `docs/rules/menus.md`.

use std::sync::OnceLock;

use serde::Deserialize;
use sf_engine::Action;

use crate::settings::{PaletteChoice, WindowMode};

/// The text of `assets/config/menu_look.ron`, compiled in.
pub const EMBEDDED_MENU_LOOK: &str = include_str!("../../../assets/config/menu_look.ron");

/// An sRGB colour, alpha last.
pub type Rgba = (u8, u8, u8, u8);

/// The panel in the middle of the window.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PanelLook {
    pub width_px: u16,
    /// The narrower panel of the start and pause menus.
    pub compact_width_px: u16,
    pub margin_px: u16,
    pub pad_px: u16,
    pub gap_px: u16,
    pub border_px: u16,
    pub border: Rgba,
    pub title_px: u16,
    pub title: Rgba,
    pub notice_height_px: u16,
    pub notice: Rgba,
    pub hint: Rgba,
}

/// The buttons of the start and pause menus.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ButtonLook {
    pub width_px: u16,
    pub height_px: u16,
}

/// The tabs of the settings.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TabLook {
    pub height_px: u16,
}

/// One line of the settings.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RowLook {
    pub height_px: u16,
    pub label_px: u16,
    pub gap_px: u16,
    pub heading: Rgba,
}

/// The line of buttons under the settings.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FooterLook {
    pub height_px: u16,
    pub button_width_px: u16,
}

/// The key list.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct KeysLook {
    pub columns: u8,
    pub column_min_px: u16,
    pub key_weight: u8,
    pub reset_weight: u8,
    pub fixed_key: String,
}

/// The ring round the current choice.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SelectedLook {
    pub ring_px: u16,
    pub ring: Rgba,
}

/// The swatch of a player colour.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SwatchLook {
    pub size_px: u16,
    pub outline: Rgba,
}

/// The buttons under the banner of a decided match.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResultButtonsLook {
    pub gap_px: u16,
    pub below_banner_px: u16,
    pub width_px: u16,
    pub height_px: u16,
}

/// What the settings are before the player has chosen anything.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    pub window: WindowMode,
    pub frame_cap: Option<u32>,
    pub vsync: bool,
    pub ui_scale: f32,
    pub palette: PaletteChoice,
}

/// One row of the key list.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct KeyRow {
    pub action: Action,
    pub label: String,
}

/// A group of rows under a heading.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct KeyGroup {
    pub title: String,
    pub rows: Vec<KeyRow>,
}

/// Every word the menus say. `{name}` marks a place that is filled in.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Words {
    pub game_title: String,
    pub paused_title: String,
    pub settings_title: String,
    pub start_match: String,
    pub start_sandbox: String,
    pub settings: String,
    pub quit: String,
    pub resume: String,
    pub restart: String,
    pub main_menu: String,
    pub back: String,
    pub tab_keys: String,
    pub tab_graphics: String,
    pub tab_accessibility: String,
    pub tab_sound: String,
    pub keys_hint: String,
    pub keys_capture: String,
    pub keys_capture_notice: String,
    pub keys_conflict: String,
    pub keys_menu_row: String,
    pub keys_reset: String,
    pub keys_reset_all: String,
    pub keys_page: String,
    pub keys_prev: String,
    pub keys_next: String,
    pub window: String,
    pub windowed: String,
    pub borderless: String,
    pub frame_cap: String,
    pub frame_cap_off: String,
    pub frame_cap_hz: String,
    pub vsync: String,
    pub on: String,
    pub off: String,
    pub ui_scale: String,
    pub ui_scale_percent: String,
    pub player_colours: String,
    pub palette_standard: String,
    pub palette_colour_blind: String,
    pub sound_volumes: String,
    pub sound_master: String,
    pub sound_music: String,
    pub sound_effects: String,
    pub sound_voices: String,
    pub sound_less: String,
    pub sound_more: String,
    pub sound_percent: String,
    pub sound_test: String,
    pub sound_ra_heading: String,
    pub sound_ra_switch: String,
    pub sound_ra_folder: String,
    pub sound_ra_folder_none: String,
    pub sound_ra_hint: String,
    pub sound_ra_no_folder: String,
    pub sound_ra_not_found: String,
    pub sound_ra_loading: String,
    pub sound_ra_found: String,
}

/// The whole of `menu_look.ron`, checked.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MenuLook {
    pub dim: Rgba,
    pub panel: PanelLook,
    pub button: ButtonLook,
    pub tab: TabLook,
    pub row: RowLook,
    pub footer: FooterLook,
    pub keys: KeysLook,
    pub selected: SelectedLook,
    pub swatch: SwatchLook,
    pub result_buttons: ResultButtonsLook,
    pub ui_scales: Vec<f32>,
    pub frame_caps: Vec<Option<u32>>,
    pub defaults: Defaults,
    pub key_groups: Vec<KeyGroup>,
    pub words: Words,
}

impl MenuLook {
    /// Parses and checks `text`: sizes above zero, a panel that holds the title, UI scales that rise
    /// inside the range the UI allows, frame caps that rise, defaults that are among the choices, and
    /// key rows that name an action once and are not the menu key.
    pub fn from_ron(text: &str) -> Result<MenuLook, String> {
        let look: MenuLook = ron::from_str(text).map_err(|e| e.to_string())?;
        look.check()?;
        Ok(look)
    }

    fn check(&self) -> Result<(), String> {
        let p = &self.panel;
        if p.width_px == 0 || p.compact_width_px == 0 || p.title_px == 0 || p.notice_height_px == 0
        {
            return Err(
                "panel.width_px, compact_width_px, title_px and notice_height_px must be above 0"
                    .into(),
            );
        }
        if self.button.width_px == 0 || self.button.height_px == 0 {
            return Err("button.width_px and height_px must be above 0".into());
        }
        if self.tab.height_px == 0 || self.row.height_px == 0 || self.footer.height_px == 0 {
            return Err("tab, row and footer heights must be above 0".into());
        }
        if self.row.label_px == 0 || self.footer.button_width_px == 0 {
            return Err("row.label_px and footer.button_width_px must be above 0".into());
        }
        if self.keys.columns == 0
            || self.keys.columns > 4
            || self.keys.key_weight == 0
            || self.keys.reset_weight == 0
        {
            return Err("keys.columns must be 1 to 4 and the weights above 0".into());
        }
        if self.result_buttons.width_px == 0 || self.result_buttons.height_px == 0 {
            return Err("result_buttons.width_px and height_px must be above 0".into());
        }
        if self.swatch.size_px == 0 {
            return Err("swatch.size_px must be above 0".into());
        }
        let scales = &self.ui_scales;
        if scales.is_empty() || scales.len() > 8 {
            return Err("ui_scales needs 1 to 8 choices".into());
        }
        let range = sf_ui::color::MIN_SCALE..=sf_ui::color::MAX_SCALE;
        if scales.iter().any(|s| !s.is_finite() || !range.contains(s))
            || scales.windows(2).any(|w| w[0] >= w[1])
        {
            return Err(format!(
                "ui_scales must rise and lie between {} and {}",
                sf_ui::color::MIN_SCALE,
                sf_ui::color::MAX_SCALE
            ));
        }
        if !scales.contains(&self.defaults.ui_scale) {
            return Err("defaults.ui_scale must be one of ui_scales".into());
        }
        let caps = &self.frame_caps;
        if caps.is_empty() || caps.len() > 8 {
            return Err("frame_caps needs 1 to 8 choices".into());
        }
        let hz: Vec<u32> = caps.iter().flatten().copied().collect();
        if hz.contains(&0) || hz.windows(2).any(|w| w[0] >= w[1]) {
            return Err("frame_caps must be above 0 and rise".into());
        }
        if !caps.contains(&self.defaults.frame_cap) {
            return Err("defaults.frame_cap must be one of frame_caps".into());
        }
        let mut seen: Vec<Action> = Vec::new();
        for row in self.key_groups.iter().flat_map(|g| &g.rows) {
            if row.action == Action::Quit {
                return Err("Quit is the menu key and cannot be a row of the key list".into());
            }
            if seen.contains(&row.action) {
                return Err(format!("{:?} is in the key list twice", row.action));
            }
            if row.label.is_empty() {
                return Err(format!("{:?} has no label", row.action));
            }
            seen.push(row.action);
        }
        if seen.is_empty() {
            return Err("the key list is empty".into());
        }
        Ok(())
    }

    /// The look compiled into the binary.
    pub fn embedded() -> &'static MenuLook {
        static LOOK: OnceLock<MenuLook> = OnceLock::new();
        LOOK.get_or_init(|| {
            MenuLook::from_ron(EMBEDDED_MENU_LOOK).expect("the shipped menu_look.ron is valid")
        })
    }
}

/// `text` with each `{name}` replaced by its value (`pairs` are `(name, value)`).
pub fn fill(text: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = text.to_owned();
    for (name, value) in pairs {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_look_is_pinned() {
        let l = MenuLook::embedded();
        assert_eq!(l.dim, (0, 0, 0, 170));
        let p = &l.panel;
        assert_eq!(
            (p.width_px, p.margin_px, p.pad_px, p.gap_px, p.border_px),
            (760, 12, 18, 8, 2)
        );
        assert_eq!(p.compact_width_px, 360);
        assert_eq!((p.title_px, p.notice_height_px), (28, 22));
        assert_eq!(
            (p.title, p.notice, p.hint),
            (
                (236, 240, 246, 255),
                (255, 200, 110, 255),
                (170, 178, 190, 255)
            )
        );
        assert_eq!((l.button.width_px, l.button.height_px), (300, 38));
        assert_eq!(l.tab.height_px, 30);
        assert_eq!(
            (l.row.height_px, l.row.label_px, l.row.gap_px),
            (28, 170, 6)
        );
        assert_eq!((l.footer.height_px, l.footer.button_width_px), (32, 120));
        let k = &l.keys;
        assert_eq!(
            (k.columns, k.column_min_px, k.key_weight, k.reset_weight),
            (2, 300, 3, 2)
        );
        assert_eq!(k.fixed_key, "Esc");
        assert_eq!(
            (l.selected.ring_px, l.selected.ring),
            (2, (240, 200, 90, 255))
        );
        assert_eq!(l.swatch.size_px, 14);
        let r = &l.result_buttons;
        assert_eq!(
            (r.gap_px, r.below_banner_px, r.width_px, r.height_px),
            (8, 8, 150, 32)
        );
        assert_eq!(l.ui_scales, [0.75, 1.0, 1.25, 1.5, 2.0]);
        assert_eq!(
            l.frame_caps,
            [None, Some(30), Some(60), Some(120), Some(144)]
        );
        let d = &l.defaults;
        assert_eq!(
            (d.window, d.frame_cap, d.vsync, d.ui_scale, d.palette),
            (
                WindowMode::Windowed,
                None,
                true,
                1.0,
                PaletteChoice::Standard
            )
        );
        let titles: Vec<_> = l.key_groups.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Camera",
                "Orders",
                "Building and production",
                "Selection",
                "Window and tools"
            ]
        );
        let rows: usize = l.key_groups.iter().map(|g| g.rows.len()).sum();
        assert_eq!(rows, 31);
        let w = &l.words;
        assert_eq!(
            (&*w.game_title, &*w.paused_title, &*w.settings_title),
            ("Staalfront", "Paused", "Settings")
        );
        assert_eq!(
            [&*w.start_match, &*w.start_sandbox, &*w.settings, &*w.quit],
            ["Match", "Sandbox", "Settings", "Quit"]
        );
        assert_eq!(
            [&*w.resume, &*w.restart, &*w.main_menu, &*w.back],
            ["Resume", "Restart", "Main menu", "Back"]
        );
        assert_eq!(
            [
                &*w.tab_keys,
                &*w.tab_graphics,
                &*w.tab_accessibility,
                &*w.tab_sound
            ],
            ["Keys", "Graphics", "Accessibility", "Sound"]
        );
    }

    #[test]
    fn text_is_filled_in_by_name() {
        assert_eq!(
            fill(
                "{key} is used for {action}.",
                &[("key", "W"), ("action", "Pan up")]
            ),
            "W is used for Pan up."
        );
        assert_eq!(fill("no places", &[("key", "W")]), "no places");
        assert_eq!(fill("{a}{a}", &[("a", "x")]), "xx");
    }

    /// The shipped file with one value changed.
    fn broken(from: &str, to: &str) -> Result<MenuLook, String> {
        assert!(
            EMBEDDED_MENU_LOOK.contains(from),
            "{from:?} is not in the file"
        );
        MenuLook::from_ron(&EMBEDDED_MENU_LOOK.replacen(from, to, 1))
    }

    #[test]
    fn a_look_that_cannot_work_is_refused() {
        const SCALES: &str = "ui_scales: [0.75, 1.0, 1.25, 1.5, 2.0]";
        const CAPS: &str = "[None, Some(30), Some(60), Some(120), Some(144)]";
        assert!(broken("width_px: 760", "width_px: 0").is_err());
        assert!(broken("height_px: 38", "height_px: 0").is_err());
        assert!(broken("columns: 2", "columns: 0").is_err());
        assert!(broken("columns: 2", "columns: 9").is_err());
        assert!(broken(SCALES, "ui_scales: [1.0, 0.75]").is_err());
        assert!(broken(SCALES, "ui_scales: [1.0, 9.0]").is_err());
        // The default (1.0) must be a choice.
        assert!(broken(SCALES, "ui_scales: [0.75, 1.25]").is_err());
        assert!(broken(CAPS, "[None, Some(60), Some(30)]").is_err());
        assert!(broken(CAPS, "[Some(30)]").is_err());
        // The menu key cannot be a row, and a row cannot be there twice.
        assert!(broken("action: CameraPanUp,", "action: Quit,").is_err());
        assert!(broken("action: CameraPanDown,", "action: CameraPanUp,").is_err());
        assert!(broken("label: \"Pan up\"", "label: \"\"").is_err());
        // A word that is missing, a field that is unknown, text that is not RON.
        assert!(broken("quit: \"Quit\",", "").is_err());
        assert!(broken("dim: (0, 0, 0, 170),", "dim: (0, 0, 0, 170), colour: 1,").is_err());
        assert!(MenuLook::from_ron("not ron").is_err());
        // The unchanged file is fine.
        assert!(broken("width_px: 760", "width_px: 760").is_ok());
    }
}
