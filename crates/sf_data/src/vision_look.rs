//! How the debug view of shroud and fog is drawn (WP8.1), in `assets/config/vision_look.ron`.
//! Presentation only; no checksum depends on it. The real look is WP8.3 and WP8.4.
//!
//! ```
//! use sf_data::vision_look::VisionLook;
//!
//! let look = VisionLook::embedded();
//! assert!(look.shroud.3 > look.fog.3);
//! ```

use serde::Deserialize;

pub use crate::repair_look::Rgba;

/// The shipped file, compiled in like the other tables.
pub const EMBEDDED_VISION_LOOK: &str = include_str!("../../../assets/config/vision_look.ron");

/// The look of the debug view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisionLook {
    /// Cells never seen.
    pub shroud: Rgba,
    /// Cells seen before but not now.
    pub fog: Rgba,
    /// A placement preview cell that holds something the player cannot see.
    pub placement_unknown: Rgba,
    /// The most cells of one row drawn as one quad: 1 to 64.
    pub run_cells: u32,
}

impl VisionLook {
    /// Reads and checks the look. The error says what is wrong.
    pub fn parse(text: &str) -> Result<VisionLook, String> {
        let look: VisionLook = ron::from_str(text).map_err(|e| e.to_string())?;
        if look.run_cells == 0 || look.run_cells > 64 {
            return Err(format!("run_cells must be 1 to 64, not {}", look.run_cells));
        }
        Ok(look)
    }

    /// The shipped look. Panics if the shipped file is wrong (a test reads it).
    pub fn embedded() -> VisionLook {
        VisionLook::parse(EMBEDDED_VISION_LOOK)
            .unwrap_or_else(|e| panic!("assets/config/vision_look.ron: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_look_is_pinned() {
        let look = VisionLook::embedded();
        assert_eq!(look.shroud, (0, 0, 0, 215));
        assert_eq!(look.fog, (10, 14, 24, 120));
        assert_eq!(look.placement_unknown, (150, 150, 160, 110));
        assert_eq!(look.run_cells, 8);
    }

    #[test]
    fn a_broken_look_is_refused() {
        let good = EMBEDDED_VISION_LOOK;
        assert!(
            VisionLook::parse(&good.replacen("run_cells: 8", "run_cells: 0", 1))
                .unwrap_err()
                .contains("run_cells")
        );
        assert!(VisionLook::parse(&good.replacen("run_cells: 8", "run_cells: 65", 1)).is_err());
        assert!(VisionLook::parse(&good.replacen("shroud:", "shrouds:", 1)).is_err());
    }
}
