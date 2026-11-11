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

/// The largest numerator or denominator of the build speed (`sf_sim::MAX_BUILD_SPEED_TERM`; a
/// test in `sf_bridge` holds the two equal).
pub const MAX_BUILD_SPEED_TERM: u32 = 100;
/// The most the build speed may change the build times: this many times faster or slower
/// (`sf_sim::MAX_BUILD_SPEED_RATIO`).
pub const MAX_BUILD_SPEED_RATIO: u32 = 4;

/// The build speed (WP6.11): everything, units and structures, is built `num / den` times as fast
/// as the build times of the unit definitions say. An exact fraction, integers only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildSpeedSetting {
    /// Numerator (1 to 100).
    pub num: u32,
    /// Denominator (1 to 100).
    pub den: u32,
}

impl Default for BuildSpeedSetting {
    /// 1 / 1: the build times as the definitions give them (a file without the setting).
    fn default() -> BuildSpeedSetting {
        BuildSpeedSetting { num: 1, den: 1 }
    }
}

/// The production numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionSettings {
    /// Most items in one queue, the one being built included.
    pub queue_limit: u8,
    /// Cells round the exit cell in which a finished unit looks for a free spot.
    pub exit_radius: u8,
    /// How much faster than the definitions' build times everything is built (WP6.11); left out
    /// in a file: 1 / 1.
    #[serde(default)]
    pub build_speed: BuildSpeedSetting,
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
        let b = s.build_speed;
        let in_terms = |n: u32| (1..=MAX_BUILD_SPEED_TERM).contains(&n);
        if !in_terms(b.num)
            || !in_terms(b.den)
            || b.num > MAX_BUILD_SPEED_RATIO * b.den
            || b.den > MAX_BUILD_SPEED_RATIO * b.num
        {
            return Err(ProductionError::Invalid(format!(
                "build_speed (num: {}, den: {}) must have both terms 1 to {MAX_BUILD_SPEED_TERM} \
                 and be between 1/{MAX_BUILD_SPEED_RATIO} and {MAX_BUILD_SPEED_RATIO} times",
                b.num, b.den
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
        // WP6.11: one and a half times as fast.
        assert_eq!(s.build_speed, BuildSpeedSetting { num: 3, den: 2 });
    }

    #[test]
    fn a_file_without_the_build_speed_means_one_over_one() {
        let s = ProductionSettings::from_ron("(queue_limit: 5, exit_radius: 4)").unwrap();
        assert_eq!(s.build_speed, BuildSpeedSetting { num: 1, den: 1 });
    }

    #[test]
    fn a_build_speed_out_of_bounds_is_refused() {
        let ok = |n: u32, d: u32| {
            ProductionSettings::from_ron(&format!(
                "(queue_limit: 5, exit_radius: 4, build_speed: (num: {n}, den: {d}))"
            ))
        };
        assert!(ok(3, 2).is_ok() && ok(1, 1).is_ok() && ok(2, 3).is_ok());
        assert!(ok(MAX_BUILD_SPEED_RATIO, 1).is_ok() && ok(1, MAX_BUILD_SPEED_RATIO).is_ok());
        for (n, d) in [(0, 1), (1, 0), (101, 100), (1, 101), (9, 2), (2, 9)] {
            assert!(
                matches!(ok(n, d), Err(ProductionError::Invalid(_))),
                "{n}/{d}"
            );
        }
        // A negative or fractional term is not RON for a whole number.
        assert!(matches!(
            ProductionSettings::from_ron(
                "(queue_limit: 5, exit_radius: 4, build_speed: (num: -3, den: 2))"
            ),
            Err(ProductionError::Ron(_))
        ));
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
