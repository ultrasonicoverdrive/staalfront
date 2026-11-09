//! The power numbers (WP6.4), in `assets/config/power.ron`: how far a shortage can slow things down
//! and when defences go dark. `sf_bridge::power_rules` hands them to the world
//! (`World::with_power`). The rules are in `docs/rules/power.md`; the power each structure makes or
//! uses is `power` in `unit_defs.ron`.
//!
//! ```
//! use sf_data::power::PowerSettings;
//!
//! let p = PowerSettings::embedded();
//! assert!(p.shortage_floor_permille >= 1);
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};

/// The shipped file, compiled in like the other tables.
pub const EMBEDDED_POWER: &str = include_str!("../../../assets/config/power.ron");

/// The power numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerSettings {
    /// The lowest speed a shortage brings things down to, in thousandths: 1 to 1000. (The
    /// simulation takes 0 too; a file may not ask for it, since a base without a plant could then
    /// never build one.)
    pub shortage_floor_permille: u32,
    /// Defences work while the speed is at least this, in thousandths: 0 to 1000.
    pub defence_cutoff_permille: u32,
}

/// Why a power file was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PowerError {
    /// The text is not valid RON for the settings.
    Ron(String),
    /// The numbers are out of bounds.
    Invalid(String),
}

impl fmt::Display for PowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PowerError::Ron(e) | PowerError::Invalid(e) => write!(f, "power.ron: {e}"),
        }
    }
}

impl std::error::Error for PowerError {}

impl PowerSettings {
    /// Reads and checks the text of a power file.
    pub fn from_ron(text: &str) -> Result<PowerSettings, PowerError> {
        let s: PowerSettings = ron::from_str(text).map_err(|e| PowerError::Ron(e.to_string()))?;
        if s.shortage_floor_permille == 0 || s.shortage_floor_permille > 1000 {
            return Err(PowerError::Invalid(format!(
                "shortage_floor_permille must be 1 to 1000, not {}",
                s.shortage_floor_permille
            )));
        }
        if s.defence_cutoff_permille > 1000 {
            return Err(PowerError::Invalid(format!(
                "defence_cutoff_permille must be 0 to 1000, not {}",
                s.defence_cutoff_permille
            )));
        }
        Ok(s)
    }

    /// The shipped settings.
    ///
    /// # Panics
    ///
    /// Never for the file in the repository: a test reads it.
    pub fn embedded() -> PowerSettings {
        PowerSettings::from_ron(EMBEDDED_POWER).expect("the embedded power.ron is valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_numbers_are_pinned() {
        let s = PowerSettings::embedded();
        assert_eq!(s.shortage_floor_permille, 250);
        assert_eq!(s.defence_cutoff_permille, 1000);
    }

    #[test]
    fn numbers_out_of_bounds_are_refused() {
        let ok = |f: u32, c: u32| {
            PowerSettings::from_ron(&format!(
                "(shortage_floor_permille: {f}, defence_cutoff_permille: {c})"
            ))
        };
        assert!(ok(1, 0).is_ok());
        assert!(ok(1000, 1000).is_ok());
        assert!(matches!(ok(0, 0), Err(PowerError::Invalid(_))));
        assert!(matches!(ok(1001, 0), Err(PowerError::Invalid(_))));
        assert!(matches!(ok(1, 1001), Err(PowerError::Invalid(_))));
        assert!(matches!(
            PowerSettings::from_ron("(shortage_floor_permille: 1)"),
            Err(PowerError::Ron(_))
        ));
        assert!(matches!(
            PowerSettings::from_ron(
                "(shortage_floor_permille: 1, defence_cutoff_permille: 1, extra: 1)"
            ),
            Err(PowerError::Ron(_))
        ));
    }
}
