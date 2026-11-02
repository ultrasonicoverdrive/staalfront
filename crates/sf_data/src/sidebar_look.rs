//! How the sidebar looks (WP11.1), in `assets/config/sidebar.ron`: the size and the parts of the
//! column, the colours, the words and the default picture of a role that has none. Presentation
//! only; what an item costs and how long it takes is `unit_defs.ron`, and what the sidebar does is
//! `docs/rules/sidebar.md`.
//!
//! ```
//! use sf_data::sidebar_look::SidebarLook;
//!
//! let look = SidebarLook::embedded();
//! assert!(look.width_px > 0 && look.tile_columns > 0);
//! ```

use serde::Deserialize;

/// The shipped file, compiled in, like the other tables.
pub const EMBEDDED_SIDEBAR_LOOK: &str = include_str!("../../../assets/config/sidebar.ron");

/// An sRGB colour with alpha last.
pub type Rgba = (u8, u8, u8, u8);

/// The four build tabs' values, in the order structures, infantry, vehicles, vessels.
pub type PerTab<T> = (T, T, T, T);

/// The words on the sidebar.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Words {
    /// The label of the power bar.
    pub power: String,
    /// What the power bar says while the world has no power rules.
    pub power_none: String,
    /// On an item whose factory waits for credits.
    pub waiting_credits: String,
    /// On an item that is finished but cannot leave the factory.
    pub exit_blocked: String,
    /// On an item whose factory is stopped.
    pub stopped: String,
    /// Before the name of the structure an item waits for.
    pub needs: String,
    /// What an empty tab says when nothing is missing, there is just nothing to build.
    pub nothing: String,
}

/// The colours of the sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Colors {
    /// A tab.
    pub tab: Rgba,
    /// A tab under the cursor.
    pub tab_hover: Rgba,
    /// The tab that is shown.
    pub tab_active: Rgba,
    /// The text of a tab with nothing to build.
    pub tab_empty_text: Rgba,
    /// The back of an item.
    pub tile_back: Rgba,
    /// Laid over the item under the cursor.
    pub tile_hover: Rgba,
    /// The ring round the structure chosen to be placed.
    pub tile_chosen: Rgba,
    /// Text.
    pub text: Rgba,
    /// Less important text (the price).
    pub text_dim: Rgba,
    /// A price the player cannot pay now, and the words of a waiting item.
    pub warn: Rgba,
    /// The part of the progress bar that is done.
    pub bar_fill: Rgba,
    /// The back of the progress bar.
    pub bar_back: Rgba,
    /// The fill while the item waits.
    pub bar_waiting: Rgba,
    /// The back of the queue count.
    pub badge_back: Rgba,
    /// The back of the power bar.
    pub power_back: Rgba,
    /// The power bar while the world has no power rules.
    pub power_neutral: Rgba,
    /// The power bar's fill.
    pub power_fill: Rgba,
    /// The fill when more power is used than made.
    pub power_low: Rgba,
}

/// The look of the sidebar.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidebarLook {
    /// Width of the column.
    pub width_px: u16,
    /// Distance from the right and the top edge of the window; at least the edge-scroll band.
    pub margin_px: u16,
    /// Room left under the column for the minimap.
    pub bottom_reserve_px: u16,
    /// Space between the parts.
    pub gap_px: u16,
    /// Space inside the column.
    pub pad_px: u16,
    /// Height of the credits block.
    pub money_height_px: u16,
    /// Height of the power bar.
    pub power_height_px: u16,
    /// Height of a tab.
    pub tab_height_px: u16,
    /// Tabs per row.
    pub tab_columns: u8,
    /// The names of the tabs.
    pub tab_labels: PerTab<String>,
    /// What an empty unit tab says while the factory for it does not stand.
    pub empty_hints: PerTab<String>,
    /// Items per row.
    pub tile_columns: u8,
    /// Height of an item.
    pub tile_height_px: u16,
    /// Height of the progress bar of an item.
    pub bar_height_px: u16,
    /// Size of the text.
    pub text_px: u16,
    /// Size of the small text (price, state).
    pub small_px: u16,
    /// Size of the label on a picture.
    pub label_px: u16,
    /// Size of the credits figure, in percent of `text_px`.
    pub credits_text_pct: u16,
    /// Distance of the credits figure from the left edge of its block.
    pub credits_inset_px: u16,
    /// Height of the strip under a picture, in percent of `small_px`.
    pub strip_pct: u16,
    /// The tile back left round a picture.
    pub picture_inset_px: u16,
    /// Padding of the queue count and of the strip text.
    pub text_pad_px: u16,
    /// Width of the ring round the chosen structure.
    pub chosen_ring_px: u16,
    /// The colour of the default picture, per tab.
    pub default_colors: PerTab<(u8, u8, u8)>,
    /// How many letters of the id the default picture shows.
    pub default_label_chars: u8,
    /// The words.
    pub words: Words,
    /// The colours.
    pub colors: Colors,
}

impl SidebarLook {
    /// Reads and checks the look. The error says what is wrong.
    pub fn parse(text: &str) -> Result<SidebarLook, String> {
        let look: SidebarLook = ron::from_str(text).map_err(|e| e.to_string())?;
        for (name, v) in [
            ("width_px", look.width_px),
            ("money_height_px", look.money_height_px),
            ("power_height_px", look.power_height_px),
            ("tab_height_px", look.tab_height_px),
            ("tile_height_px", look.tile_height_px),
            ("bar_height_px", look.bar_height_px),
            ("text_px", look.text_px),
            ("small_px", look.small_px),
            ("label_px", look.label_px),
            ("credits_text_pct", look.credits_text_pct),
            ("strip_pct", look.strip_pct),
            ("chosen_ring_px", look.chosen_ring_px),
        ] {
            if v == 0 {
                return Err(format!("{name} must be above 0"));
            }
        }
        if look.tab_columns == 0 || look.tab_columns > 4 {
            return Err("tab_columns must be 1 to 4".into());
        }
        if look.tile_columns == 0 || look.tile_columns > 6 {
            return Err("tile_columns must be 1 to 6".into());
        }
        if look.default_label_chars == 0 || look.default_label_chars > 6 {
            return Err("default_label_chars must be 1 to 6".into());
        }
        // The tile has to be wide enough for its parts: with the gaps and the padding the columns
        // must leave at least 24 px each.
        let inner = i32::from(look.width_px) - 2 * i32::from(look.pad_px);
        let gaps = i32::from(look.gap_px) * (i32::from(look.tile_columns) - 1);
        if (inner - gaps) / i32::from(look.tile_columns) < 24 {
            return Err("width_px is too small for tile_columns".into());
        }
        if look.credits_text_pct > 300 || look.strip_pct > 300 {
            return Err("credits_text_pct and strip_pct must be 1 to 300".into());
        }
        if look.picture_inset_px > 8 || look.text_pad_px > 16 || look.chosen_ring_px > 8 {
            return Err("picture_inset_px, text_pad_px or chosen_ring_px is too large".into());
        }
        if look.bar_height_px >= look.tile_height_px {
            return Err("bar_height_px must be less than tile_height_px".into());
        }
        Ok(look)
    }

    /// The shipped look. Panics if the shipped file is wrong (a test reads it).
    pub fn embedded() -> SidebarLook {
        SidebarLook::parse(EMBEDDED_SIDEBAR_LOOK)
            .unwrap_or_else(|e| panic!("assets/config/sidebar.ron: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_look_is_pinned() {
        let look = SidebarLook::embedded();
        assert_eq!(
            (
                look.width_px,
                look.margin_px,
                look.bottom_reserve_px,
                look.gap_px,
                look.pad_px
            ),
            (200, 6, 210, 4, 4)
        );
        assert_eq!(
            (
                look.money_height_px,
                look.power_height_px,
                look.tab_height_px,
                look.tile_height_px,
                look.bar_height_px
            ),
            (26, 16, 24, 62, 5)
        );
        assert_eq!((look.tab_columns, look.tile_columns), (2, 2));
        assert_eq!(
            look.tab_labels,
            (
                "Structures".to_owned(),
                "Infantry".to_owned(),
                "Vehicles".to_owned(),
                "Vessels".to_owned()
            )
        );
        assert_eq!((look.text_px, look.small_px, look.label_px), (14, 12, 17));
        assert_eq!(
            (
                look.credits_text_pct,
                look.credits_inset_px,
                look.strip_pct,
                look.picture_inset_px,
                look.text_pad_px,
                look.chosen_ring_px
            ),
            (115, 6, 150, 1, 3, 2)
        );
        assert_eq!(
            look.default_colors,
            (
                (80, 100, 140),
                (80, 130, 90),
                (140, 110, 70),
                (70, 120, 140)
            )
        );
        assert_eq!(look.default_label_chars, 4);
        assert_eq!(
            look.words,
            Words {
                power: "Power".into(),
                power_none: "no power rules".into(),
                waiting_credits: "no credits".into(),
                exit_blocked: "exit blocked".into(),
                stopped: "stopped".into(),
                needs: "needs".into(),
                nothing: "Nothing to build".into(),
            }
        );
        assert_eq!(
            look.empty_hints,
            (
                "Needs a construction centre".to_owned(),
                "Needs a barracks".to_owned(),
                "Needs a vehicle factory".to_owned(),
                "Needs a shipyard".to_owned()
            )
        );
        assert_eq!(
            look.colors,
            Colors {
                tab: (46, 58, 80, 255),
                tab_hover: (64, 82, 116, 255),
                tab_active: (92, 126, 184, 255),
                tab_empty_text: (130, 136, 146, 255),
                tile_back: (20, 24, 32, 255),
                tile_hover: (255, 255, 255, 40),
                tile_chosen: (255, 220, 90, 255),
                text: (236, 240, 246, 255),
                text_dim: (170, 176, 188, 255),
                warn: (255, 110, 80, 255),
                bar_fill: (90, 200, 255, 235),
                bar_back: (10, 14, 20, 190),
                bar_waiting: (255, 110, 80, 245),
                badge_back: (10, 14, 20, 220),
                power_back: (16, 20, 28, 255),
                power_neutral: (70, 76, 88, 255),
                power_fill: (120, 220, 120, 255),
                power_low: (255, 110, 80, 255),
            }
        );
    }

    #[test]
    fn a_broken_look_is_refused() {
        let good = EMBEDDED_SIDEBAR_LOOK;
        let zero = good.replacen("tile_height_px: 62", "tile_height_px: 0", 1);
        assert!(
            SidebarLook::parse(&zero)
                .unwrap_err()
                .contains("tile_height_px")
        );
        let thin = good.replacen("width_px: 200", "width_px: 40", 1);
        assert!(SidebarLook::parse(&thin).unwrap_err().contains("too small"));
        let columns = good.replacen("tab_columns: 2", "tab_columns: 0", 1);
        assert!(
            SidebarLook::parse(&columns)
                .unwrap_err()
                .contains("tab_columns")
        );
        let bar = good.replacen("bar_height_px: 5", "bar_height_px: 62", 1);
        assert!(
            SidebarLook::parse(&bar)
                .unwrap_err()
                .contains("bar_height_px")
        );
        let unknown = good.replacen("gap_px: 4", "gap_px: 4, unknown: 1", 1);
        assert!(SidebarLook::parse(&unknown).is_err());
    }
}
