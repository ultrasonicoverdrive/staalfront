//! How the ore storage is shown (WP7.3), in `assets/config/storage_look.ron`: the fill bar under a
//! structure that stores ore. Presentation only; what a structure stores is the `storage` of its
//! building block in `unit_defs.ron` (`docs/rules/economy.md`, "Storage").
//!
//! ```
//! use sf_data::storage_look::StorageLook;
//!
//! let look = StorageLook::embedded();
//! assert!(look.bar.height_px > 0);
//! ```

use serde::Deserialize;

use crate::build_stages::BarLook;

/// The shipped file, compiled in, like the other tables.
pub const EMBEDDED_STORAGE_LOOK: &str = include_str!("../../../assets/config/storage_look.ron");

/// The look of the storage bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageLook {
    /// The bar: colours, height, widths and gap, as the construction bar has them.
    pub bar: BarLook,
    /// The fill colour of a full store (sRGB, alpha last).
    pub full: (u8, u8, u8, u8),
}

impl StorageLook {
    /// Reads and checks the look. The error says what is wrong.
    pub fn parse(text: &str) -> Result<StorageLook, String> {
        let look: StorageLook = ron::from_str(text).map_err(|e| e.to_string())?;
        if look.bar.height_px == 0 {
            return Err("bar.height_px must be above 0".into());
        }
        if look.bar.min_width_px == 0 || look.bar.max_width_px < look.bar.min_width_px {
            return Err(format!(
                "bar.min_width_px {} must be above 0 and at most bar.max_width_px {}",
                look.bar.min_width_px, look.bar.max_width_px
            ));
        }
        Ok(look)
    }

    /// The shipped look. Panics if the shipped file is wrong (a test reads it).
    pub fn embedded() -> StorageLook {
        StorageLook::parse(EMBEDDED_STORAGE_LOOK)
            .unwrap_or_else(|e| panic!("assets/config/storage_look.ron: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_look_is_pinned() {
        let look = StorageLook::embedded();
        assert_eq!(look.bar.fill, (240, 170, 40, 235));
        assert_eq!(look.bar.back, (10, 14, 20, 190));
        assert_eq!(
            (
                look.bar.height_px,
                look.bar.min_width_px,
                look.bar.max_width_px,
                look.bar.gap_px
            ),
            (6, 18, 220, 4)
        );
        assert_eq!(look.full, (255, 90, 60, 245));
    }

    #[test]
    fn a_broken_look_is_refused() {
        let good = EMBEDDED_STORAGE_LOOK;
        let zero_h = good.replacen("height_px: 6", "height_px: 0", 1);
        assert!(
            StorageLook::parse(&zero_h)
                .unwrap_err()
                .contains("height_px")
        );
        let narrow = good.replacen("max_width_px: 220", "max_width_px: 10", 1);
        assert!(
            StorageLook::parse(&narrow)
                .unwrap_err()
                .contains("min_width_px")
        );
        let unknown = good.replacen("full:", "fulll:", 1);
        assert!(StorageLook::parse(&unknown).is_err());
    }
}
