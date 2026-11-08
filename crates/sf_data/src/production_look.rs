//! How production is shown (WP6.5), in `assets/config/production_look.ron`: the queue readout of
//! the selected factory, the progress bar under a factory and the rally flag. Presentation only;
//! what a unit costs and which factory builds it is `unit_defs.ron` and the numbers of the queue
//! are `production.ron` (`docs/rules/production.md`).
//!
//! ```
//! use sf_data::production_look::ProductionLook;
//!
//! let look = ProductionLook::embedded();
//! assert!(look.bar.height_px > 0);
//! ```

use serde::Deserialize;

use crate::build_stages::BarLook;

/// The shipped file, compiled in, like the other tables.
pub const EMBEDDED_PRODUCTION_LOOK: &str =
    include_str!("../../../assets/config/production_look.ron");

/// The look of production.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionLook {
    /// The progress bar under a factory that builds.
    pub bar: BarLook,
    /// The fill of that bar while the factory waits.
    pub waiting: (u8, u8, u8, u8),
    /// The readout panel.
    pub panel: PanelLook,
    /// The colour of the text.
    pub text: (u8, u8, u8, u8),
    /// The colour of the item being built.
    pub accent: (u8, u8, u8, u8),
    /// The colour of a factory that waits.
    pub warn: (u8, u8, u8, u8),
    /// The rally flag.
    pub rally: RallyLook,
}

/// The readout panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelLook {
    /// Width of a line.
    pub width_px: u16,
    /// Height of a line.
    pub line_px: u16,
    /// Distance from the left edge of the window.
    pub margin_px: u16,
    /// Distance of the first line from the top.
    pub top_px: u16,
    /// Left padding of the text.
    pub pad_px: u16,
    /// Size of the text.
    pub text_px: u16,
}

/// The rally flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RallyLook {
    /// The pole.
    pub pole: (u8, u8, u8, u8),
    /// The flag.
    pub flag: (u8, u8, u8, u8),
    /// The flag of the selected factory.
    pub flag_selected: (u8, u8, u8, u8),
    /// Height of the pole.
    pub pole_height_px: u16,
    /// Width of the pole.
    pub pole_width_px: u16,
    /// Width of the flag.
    pub flag_width_px: u16,
    /// Height of the flag.
    pub flag_height_px: u16,
}

impl ProductionLook {
    /// Reads and checks the look. The error says what is wrong.
    pub fn parse(text: &str) -> Result<ProductionLook, String> {
        let look: ProductionLook = ron::from_str(text).map_err(|e| e.to_string())?;
        if look.bar.height_px == 0 {
            return Err("bar.height_px must be above 0".into());
        }
        if look.bar.min_width_px == 0 || look.bar.max_width_px < look.bar.min_width_px {
            return Err(format!(
                "bar.min_width_px {} must be above 0 and at most bar.max_width_px {}",
                look.bar.min_width_px, look.bar.max_width_px
            ));
        }
        let p = &look.panel;
        if p.width_px == 0 || p.line_px == 0 || p.text_px == 0 {
            return Err("panel: width_px, line_px and text_px must be above 0".into());
        }
        let r = &look.rally;
        if r.pole_height_px == 0
            || r.pole_width_px == 0
            || r.flag_width_px == 0
            || r.flag_height_px == 0
            || r.flag_height_px > r.pole_height_px
        {
            return Err(
                "rally: every size must be above 0 and the flag no higher than the pole".into(),
            );
        }
        Ok(look)
    }

    /// The shipped look. Panics if the shipped file is wrong (a test reads it).
    pub fn embedded() -> ProductionLook {
        ProductionLook::parse(EMBEDDED_PRODUCTION_LOOK)
            .unwrap_or_else(|e| panic!("assets/config/production_look.ron: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_look_is_pinned() {
        let look = ProductionLook::embedded();
        assert_eq!(look.bar.fill, (90, 200, 255, 235));
        assert_eq!(look.waiting, (255, 110, 80, 245));
        assert_eq!(
            (
                look.panel.width_px,
                look.panel.line_px,
                look.panel.margin_px,
                look.panel.top_px,
                look.panel.pad_px,
                look.panel.text_px
            ),
            (360, 22, 6, 100, 10, 15)
        );
        assert_eq!(look.text, (225, 230, 240, 255));
        assert_eq!(look.accent, (120, 220, 255, 255));
        assert_eq!(look.warn, (255, 110, 80, 255));
        assert_eq!(look.rally.pole, (235, 235, 240, 255));
        assert_eq!(look.rally.flag, (90, 200, 255, 235));
        assert_eq!(look.rally.flag_selected, (255, 220, 90, 245));
        assert_eq!(
            (
                look.rally.pole_height_px,
                look.rally.pole_width_px,
                look.rally.flag_width_px,
                look.rally.flag_height_px
            ),
            (30, 2, 18, 11)
        );
    }

    #[test]
    fn a_broken_look_is_refused() {
        let good = EMBEDDED_PRODUCTION_LOOK;
        let zero_bar = good.replacen("height_px: 6", "height_px: 0", 1);
        assert!(
            ProductionLook::parse(&zero_bar)
                .unwrap_err()
                .contains("height_px")
        );
        let tall_flag = good.replacen("flag_height_px: 11", "flag_height_px: 31", 1);
        assert!(
            ProductionLook::parse(&tall_flag)
                .unwrap_err()
                .contains("pole")
        );
        let no_text = good.replacen("text_px: 15", "text_px: 0", 1);
        assert!(
            ProductionLook::parse(&no_text)
                .unwrap_err()
                .contains("panel")
        );
        assert!(ProductionLook::parse("(bar: 3)").is_err());
        let extra = good.replacen("waiting:", "extra: 1,\n    waiting:", 1);
        assert!(ProductionLook::parse(&extra).is_err());
    }
}
