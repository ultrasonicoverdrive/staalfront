//! How repair and sale are shown on the map (WP6.6), in `assets/config/repair_look.ron`: the sale
//! progress bar, the repair sign and the words of the mode line. Presentation only; the numbers of
//! repair and sale are `repair.ron` (`docs/rules/repair_sell.md`).
//!
//! ```
//! use sf_data::repair_look::RepairLook;
//!
//! let look = RepairLook::embedded();
//! assert!(look.sign.size_px > 0 && look.sale_bar.height_px > 0);
//! ```

use serde::Deserialize;

use crate::build_stages::BarLook;

/// The shipped file, compiled in, like the other tables.
pub const EMBEDDED_REPAIR_LOOK: &str = include_str!("../../../assets/config/repair_look.ron");

/// An sRGB colour with alpha last.
pub type Rgba = (u8, u8, u8, u8);

/// The repair sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignLook {
    /// Side of the square badge.
    pub size_px: u16,
    /// How far above the middle of the structure the badge floats.
    pub lift_px: u16,
    /// No sign when the front edge of the structure is narrower than this on the screen.
    pub min_width_px: u16,
    /// Length of the plus, in percent of the badge.
    pub plus_pct: u8,
    /// Thickness of the plus.
    pub plus_px: u16,
    /// The back of the badge.
    pub back: Rgba,
    /// The plus.
    pub plus: Rgba,
    /// The back while the owner cannot pay.
    pub waiting_back: Rgba,
    /// The plus while the owner cannot pay.
    pub waiting_plus: Rgba,
}

/// The look of repair and sale on the map.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairLook {
    /// The sale progress bar, with the same fields as the construction bar.
    pub sale_bar: BarLook,
    /// The repair sign.
    pub sign: SignLook,
    /// The line under the top bar in repair mode.
    pub hint_repair: String,
    /// The line under the top bar in sell mode.
    pub hint_sell: String,
}

impl RepairLook {
    /// Reads and checks the look. The error says what is wrong.
    pub fn parse(text: &str) -> Result<RepairLook, String> {
        let look: RepairLook = ron::from_str(text).map_err(|e| e.to_string())?;
        let bar = &look.sale_bar;
        if bar.height_px == 0 {
            return Err("sale_bar.height_px must be above 0".into());
        }
        if bar.min_width_px == 0 || bar.max_width_px < bar.min_width_px {
            return Err(format!(
                "sale_bar.min_width_px {} must be above 0 and at most max_width_px {}",
                bar.min_width_px, bar.max_width_px
            ));
        }
        let sign = &look.sign;
        if sign.size_px == 0 || sign.plus_px == 0 || sign.plus_px > sign.size_px {
            return Err(
                "sign.size_px and sign.plus_px must be above 0, plus_px at most size_px".into(),
            );
        }
        if sign.plus_pct == 0 || sign.plus_pct > 100 {
            return Err("sign.plus_pct must be 1 to 100".into());
        }
        if look.hint_repair.is_empty() || look.hint_sell.is_empty() {
            return Err("hint_repair and hint_sell must not be empty".into());
        }
        Ok(look)
    }

    /// The shipped look. Panics if the shipped file is wrong (a test reads it).
    pub fn embedded() -> RepairLook {
        RepairLook::parse(EMBEDDED_REPAIR_LOOK)
            .unwrap_or_else(|e| panic!("assets/config/repair_look.ron: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_look_is_pinned() {
        let look = RepairLook::embedded();
        assert_eq!(look.sale_bar.fill, (255, 200, 60, 240));
        assert_eq!(look.sale_bar.back, (10, 14, 20, 190));
        assert_eq!(
            (
                look.sale_bar.height_px,
                look.sale_bar.min_width_px,
                look.sale_bar.max_width_px,
                look.sale_bar.gap_px
            ),
            (6, 18, 220, 12)
        );
        let s = look.sign;
        assert_eq!(
            (s.size_px, s.lift_px, s.min_width_px, s.plus_pct, s.plus_px),
            (20, 30, 18, 64, 4)
        );
        assert_eq!(s.back, (24, 84, 36, 235));
        assert_eq!(s.plus, (150, 255, 160, 255));
        assert_eq!(s.waiting_back, (120, 44, 24, 235));
        assert_eq!(s.waiting_plus, (255, 176, 126, 255));
        assert!(look.hint_repair.starts_with("Repair:"));
        assert!(look.hint_sell.starts_with("Sell:"));
    }

    #[test]
    fn a_broken_look_is_refused() {
        let good = EMBEDDED_REPAIR_LOOK;
        let zero_h = good.replacen("height_px: 6", "height_px: 0", 1);
        assert!(
            RepairLook::parse(&zero_h)
                .unwrap_err()
                .contains("height_px")
        );
        let narrow = good.replacen("max_width_px: 220", "max_width_px: 10", 1);
        assert!(
            RepairLook::parse(&narrow)
                .unwrap_err()
                .contains("min_width_px")
        );
        let plus = good.replacen("plus_pct: 64", "plus_pct: 101", 1);
        assert!(RepairLook::parse(&plus).unwrap_err().contains("plus_pct"));
        let empty = good.replacen(
            "hint_sell: \"Sell: click one of your structures (right click or Esc to leave)\"",
            "hint_sell: \"\"",
            1,
        );
        assert!(RepairLook::parse(&empty).unwrap_err().contains("hint_sell"));
        let unknown = good.replacen("hint_repair:", "hint_repairr:", 1);
        assert!(RepairLook::parse(&unknown).is_err());
    }
}
