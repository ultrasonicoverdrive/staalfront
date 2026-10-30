//! How money is shown (WP7.4), in `assets/config/money_look.ron`: the readout panel of the HUD and
//! the colours of the progress bar of a structure that waits for money. Presentation only; the
//! money itself is the simulation's (`docs/rules/economy.md`, "Money").
//!
//! ```
//! use sf_data::money_look::MoneyLook;
//!
//! let look = MoneyLook::embedded();
//! assert!(look.panel.height_px > 0);
//! ```

use serde::Deserialize;

/// The shipped file, compiled in, like the other tables.
pub const EMBEDDED_MONEY_LOOK: &str = include_str!("../../../assets/config/money_look.ron");

/// The readout panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelLook {
    /// Width in logical pixels.
    pub width_px: u16,
    /// Height in logical pixels.
    pub height_px: u16,
    /// Distance from the top and the right edge of the window.
    pub margin_px: u16,
    /// Padding left of the text.
    pub pad_px: u16,
    /// Size of the text.
    pub text_px: u16,
    /// Gap between the panel and the strip under it that says a build is paused.
    pub gap_px: u16,
}

/// The progress bar of a structure that waits for money.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PausedLook {
    /// The part that is done.
    pub fill: (u8, u8, u8, u8),
    /// The rest of the bar.
    pub back: (u8, u8, u8, u8),
    /// Width of one of the two pause strokes at the right end of the bar, logical pixels.
    pub stroke_width_px: u16,
    /// Gap between the two strokes.
    pub stroke_gap_px: u16,
    /// How far a stroke stands out above and below the bar.
    pub stroke_extra_px: u16,
}

/// The look of money.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoneyLook {
    /// The readout panel.
    pub panel: PanelLook,
    /// The word in front of the number.
    pub label: String,
    /// The colour of the readout.
    pub text: (u8, u8, u8, u8),
    /// The colour when the credits are few or a structure waits.
    pub low: (u8, u8, u8, u8),
    /// Fewer credits than this count as few.
    pub low_below: u64,
    /// What the panel says while a structure waits for money.
    pub waiting: String,
    /// The progress bar of a waiting structure.
    pub paused: PausedLook,
}

impl MoneyLook {
    /// Reads and checks the look. The error says what is wrong.
    pub fn parse(text: &str) -> Result<MoneyLook, String> {
        let look: MoneyLook = ron::from_str(text).map_err(|e| e.to_string())?;
        let p = &look.panel;
        if p.width_px == 0 || p.height_px == 0 || p.text_px == 0 {
            return Err("panel.width_px, height_px and text_px must be above 0".into());
        }
        if look.paused.stroke_width_px == 0 {
            return Err("paused.stroke_width_px must be above 0".into());
        }
        if look.label.is_empty() || look.waiting.is_empty() {
            return Err("label and waiting must not be empty".into());
        }
        Ok(look)
    }

    /// The shipped look. Panics if the shipped file is wrong (a test reads it).
    pub fn embedded() -> MoneyLook {
        MoneyLook::parse(EMBEDDED_MONEY_LOOK)
            .unwrap_or_else(|e| panic!("assets/config/money_look.ron: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_look_is_pinned() {
        let look = MoneyLook::embedded();
        assert_eq!(
            look.panel,
            PanelLook {
                width_px: 250,
                height_px: 32,
                margin_px: 6,
                pad_px: 12,
                text_px: 18,
                gap_px: 4,
            }
        );
        assert_eq!(look.label, "Credits");
        assert_eq!(look.text, (240, 200, 70, 255));
        assert_eq!(look.low, (255, 110, 80, 255));
        assert_eq!(look.low_below, 100);
        assert_eq!(look.waiting, "build paused: no credits");
        assert_eq!(
            look.paused,
            PausedLook {
                fill: (255, 110, 80, 245),
                back: (60, 20, 16, 210),
                stroke_width_px: 3,
                stroke_gap_px: 3,
                stroke_extra_px: 3,
            }
        );
    }

    #[test]
    fn a_look_with_a_zero_size_or_an_empty_word_is_refused() {
        let text = EMBEDDED_MONEY_LOOK;
        assert!(MoneyLook::parse(&text.replace("width_px: 250", "width_px: 0")).is_err());
        assert!(
            MoneyLook::parse(&text.replace("stroke_width_px: 3", "stroke_width_px: 0")).is_err()
        );
        assert!(MoneyLook::parse(&text.replace("label: \"Credits\"", "label: \"\"")).is_err());
        assert!(MoneyLook::parse(&text.replace("gap_px: 4", "gap_px: -4")).is_err());
    }
}
