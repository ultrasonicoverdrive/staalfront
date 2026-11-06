//! The vision numbers (WP8.1), in `assets/config/vision.ron`: the sight of an entity that has no unit
//! table entry. How far each real kind sees is `sight` in `unit_defs.ron`. `sf_bridge::vision_rules`
//! hands both to the world (`World::with_vision`). The rules are in `docs/rules/vision.md`.
//!
//! ```
//! use sf_data::vision::VisionSettings;
//!
//! let v = VisionSettings::embedded();
//! assert!(v.legacy_sight_cells <= 64);
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};

/// The shipped file, compiled in like the other tables.
pub const EMBEDDED_VISION: &str = include_str!("../../../assets/config/vision.ron");

/// The farthest an entity may see, in cells (the bound `unit_defs.ron` is checked to).
pub const MAX_SIGHT_CELLS: u32 = 64;

/// The vision numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisionSettings {
    /// The sight, in whole cells (0 to [`MAX_SIGHT_CELLS`]), of an entity made without a unit table
    /// entry.
    pub legacy_sight_cells: u32,
}

/// Why a vision file was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VisionError {
    /// The text is not valid RON for the settings.
    Ron(String),
    /// The numbers are out of bounds.
    Invalid(String),
}

impl fmt::Display for VisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VisionError::Ron(e) | VisionError::Invalid(e) => write!(f, "vision.ron: {e}"),
        }
    }
}

impl std::error::Error for VisionError {}

impl VisionSettings {
    /// Reads and checks the text of a vision file.
    pub fn from_ron(text: &str) -> Result<VisionSettings, VisionError> {
        let s: VisionSettings = ron::from_str(text).map_err(|e| VisionError::Ron(e.to_string()))?;
        if s.legacy_sight_cells > MAX_SIGHT_CELLS {
            return Err(VisionError::Invalid(format!(
                "legacy_sight_cells must be 0 to {MAX_SIGHT_CELLS}, not {}",
                s.legacy_sight_cells
            )));
        }
        Ok(s)
    }

    /// The shipped settings.
    ///
    /// # Panics
    ///
    /// Never for the file in the repository: a test reads it.
    pub fn embedded() -> VisionSettings {
        VisionSettings::from_ron(EMBEDDED_VISION).expect("the embedded vision.ron is valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_number_is_pinned() {
        assert_eq!(VisionSettings::embedded().legacy_sight_cells, 5);
    }

    #[test]
    fn numbers_out_of_bounds_are_refused() {
        assert!(VisionSettings::from_ron("(legacy_sight_cells: 0)").is_ok());
        assert!(VisionSettings::from_ron("(legacy_sight_cells: 64)").is_ok());
        assert!(matches!(
            VisionSettings::from_ron("(legacy_sight_cells: 65)"),
            Err(VisionError::Invalid(_))
        ));
        assert!(matches!(
            VisionSettings::from_ron("(legacy_sight_cells: 1, extra: 1)"),
            Err(VisionError::Ron(_))
        ));
        assert!(matches!(
            VisionSettings::from_ron("()"),
            Err(VisionError::Ron(_))
        ));
    }
}
