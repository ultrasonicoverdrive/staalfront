//! The colour-blind safe set of player colours (WP11.5a): `assets/config/palette_colour_blind.ron`,
//! compiled in. The standard colours stay where they were, in `units.ron` (the instancer's style) and,
//! for the dots of the minimap, in `hud.rs`; this is the second set that takes their place when the
//! player chooses it. Presentation only. The rule and the check are in `docs/rules/menus.md` and
//! `tests/palette_cvd.rs`.

use std::sync::OnceLock;

use serde::Deserialize;
use sf_ui::Color;

use crate::settings::PaletteChoice;

/// The text of `assets/config/palette_colour_blind.ron`, compiled in.
pub const EMBEDDED_PALETTE: &str = include_str!("../../../assets/config/palette_colour_blind.ron");

/// One player colour.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlayerColour {
    /// What the F1 panel calls it.
    pub name: String,
    pub srgb: (u8, u8, u8),
}

/// A second set of player colours.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    /// The colours of players `0..`, in the order of `units.ron`.
    pub players: Vec<PlayerColour>,
    /// The dots of the minimap, one per player.
    pub minimap: Vec<(u8, u8, u8)>,
}

impl Palette {
    /// Parses and checks `text`: one to eight players with a name each, and a minimap dot for each.
    pub fn from_ron(text: &str) -> Result<Palette, String> {
        let p: Palette = ron::from_str(text).map_err(|e| e.to_string())?;
        if p.players.is_empty() || p.players.len() > 8 {
            return Err("a palette needs 1 to 8 players".into());
        }
        if p.players.iter().any(|c| c.name.is_empty()) {
            return Err("every player colour needs a name".into());
        }
        if p.minimap.len() != p.players.len() {
            return Err("the minimap needs one dot for each player".into());
        }
        Ok(p)
    }

    /// The set compiled into the binary.
    pub fn embedded() -> &'static Palette {
        static PALETTE: OnceLock<Palette> = OnceLock::new();
        PALETTE.get_or_init(|| {
            Palette::from_ron(EMBEDDED_PALETTE).expect("the shipped palette is valid")
        })
    }

    /// The colours as `(name, sRGB)` for `Style::with_palette` and `Instancer::set_palette`.
    pub fn style_colours(&self) -> Vec<(String, [u8; 3])> {
        self.players
            .iter()
            .map(|c| (c.name.clone(), [c.srgb.0, c.srgb.1, c.srgb.2]))
            .collect()
    }

    /// The dots of the minimap.
    pub fn minimap_dots(&self) -> Vec<Color> {
        self.minimap
            .iter()
            .map(|&(r, g, b)| Color::rgb(r, g, b))
            .collect()
    }
}

/// The player colours for the player's choice; `None` for the standard set, which the style has.
pub fn player_colours(choice: PaletteChoice) -> Option<Vec<(String, [u8; 3])>> {
    match choice {
        PaletteChoice::Standard => None,
        PaletteChoice::ColourBlind => Some(Palette::embedded().style_colours()),
    }
}

/// The standard player colours, the first ones of `units.ron` as many as the second set has: what a
/// style gets back when the player returns from the colour-blind safe set.
pub fn standard_colours() -> Vec<(String, [u8; 3])> {
    let style = sf_render::Style::embedded();
    (0..Palette::embedded().players.len() as u8)
        .map(|p| (style.color_name(p).to_owned(), style.srgb(p)))
        .collect()
}

/// The minimap dots for the choice; `None` for the standard set, which the HUD has.
pub fn minimap_dots(choice: PaletteChoice) -> Option<Vec<Color>> {
    match choice {
        PaletteChoice::Standard => None,
        PaletteChoice::ColourBlind => Some(Palette::embedded().minimap_dots()),
    }
}
