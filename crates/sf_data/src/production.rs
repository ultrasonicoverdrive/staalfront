//! The production numbers (WP6.5), in `assets/config/production.ron`: how long a factory queue
//! may be and how far a finished unit looks for a free spot. `sf_bridge::production_rules` hands
//! them to the world (`World::with_production`). The rules are in `docs/rules/production.md`.
//!
//! ```
//! use sf_data::production::ProductionSettings;
//!
//! let p = ProductionSettings::embedded();
//! assert!(p.queue_limit >= 1);
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};

/// The shipped file, compiled in like the other tables.
pub const EMBEDDED_PRODUCTION: &str = include_str!("../../../assets/config/production.ron");

/// The longest queue a file may ask for (the simulation's limit, `sf_sim::MAX_QUEUE_LIMIT`; a
/// test in `sf_bridge` holds the two equal).
pub const MAX_QUEUE_LIMIT: u8 = 20;
/// The widest search for a free spot (`sf_sim::MAX_EXIT_RADIUS`).
pub const MAX_EXIT_RADIUS: u8 = 8;

/// The production numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionSettings {
    /// Most items in one queue, the one being built included.
    pub queue_limit: u8,
    /// Cells round the exit cell in which a finished unit looks for a free spot.
    pub exit_radius: u8,
}

/// Why a production file was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProductionError {
    /// The text is not valid RON for the settings.
    Ron(String),
    /// The numbers are out of bounds.
    Invalid(String),
}

impl fmt::Display for ProductionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProductionError::Ron(e) | ProductionError::Invalid(e) => {
                write!(f, "production.ron: {e}")
            }
        }
    }
}

impl std::error::Error for ProductionError {}

impl ProductionSettings {
    /// Reads and checks the text of a production file.
    pub fn from_ron(text: &str) -> Result<ProductionSettings, ProductionError> {
        let s: ProductionSettings =
            ron::from_str(text).map_err(|e| ProductionError::Ron(e.to_string()))?;
        if s.queue_limit == 0 || s.queue_limit > MAX_QUEUE_LIMIT {
            return Err(ProductionError::Invalid(format!(
                "queue_limit must be 1 to {MAX_QUEUE_LIMIT}, not {}",
                s.queue_limit
            )));
        }
        if s.exit_radius == 0 || s.exit_radius > MAX_EXIT_RADIUS {
            return Err(ProductionError::Invalid(format!(
                "exit_radius must be 1 to {MAX_EXIT_RADIUS}, not {}",
                s.exit_radius
            )));
        }
        Ok(s)
    }

    /// The shipped settings.
    ///
    /// # Panics
    ///
    /// Never for the file in the repository: a test reads it.
    pub fn embedded() -> ProductionSettings {
        ProductionSettings::from_ron(EMBEDDED_PRODUCTION)
            .expect("the embedded production.ron is valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_numbers_are_pinned() {
        let s = ProductionSettings::embedded();
        assert_eq!(s.queue_limit, 5);
        assert_eq!(s.exit_radius, 4);
    }

    #[test]
    fn numbers_out_of_bounds_are_refused() {
        let ok = |q: u8, r: u8| {
            ProductionSettings::from_ron(&format!("(queue_limit: {q}, exit_radius: {r})"))
        };
        assert!(ok(1, 1).is_ok());
        assert!(ok(MAX_QUEUE_LIMIT, MAX_EXIT_RADIUS).is_ok());
        assert!(matches!(ok(0, 1), Err(ProductionError::Invalid(_))));
        assert!(matches!(
            ok(MAX_QUEUE_LIMIT + 1, 1),
            Err(ProductionError::Invalid(_))
        ));
        assert!(matches!(ok(1, 0), Err(ProductionError::Invalid(_))));
        assert!(matches!(
            ok(1, MAX_EXIT_RADIUS + 1),
            Err(ProductionError::Invalid(_))
        ));
        assert!(matches!(
            ProductionSettings::from_ron("(queue_limit: 1)"),
            Err(ProductionError::Ron(_))
        ));
        assert!(matches!(
            ProductionSettings::from_ron("(queue_limit: 1, exit_radius: 1, extra: 1)"),
            Err(ProductionError::Ron(_))
        ));
    }
}
