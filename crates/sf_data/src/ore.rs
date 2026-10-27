//! The ore rules (WP7.1): how much a field cell holds and how fast it grows back,
//! in `assets/config/ore.ron`. `sf_bridge::ore_rules` hands the numbers to the
//! world (`World::with_ore`). The rules are in `docs/rules/economy.md`.
//!
//! ```
//! use sf_data::ore::OreSettings;
//!
//! let ore = OreSettings::embedded();
//! assert!(ore.start <= ore.max);
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};

/// The shipped file, compiled in like the other tables.
pub const EMBEDDED_ORE: &str = include_str!("../../../assets/config/ore.ron");

/// The most ticks between two turns of a cell (an hour of game time); equal to `sf_sim`'s.
pub const MAX_REGROW_PERIOD: u32 = 72_000;

/// The ore numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OreSettings {
    /// What every ore cell holds when the world is made.
    pub start: u16,
    /// The most a cell can hold.
    pub max: u16,
    /// What a cell below the maximum gains in its turn; 0 turns regrowth off.
    pub regrow_amount: u16,
    /// Ticks between two turns of the same cell, 1 to [`MAX_REGROW_PERIOD`].
    pub regrow_period: u32,
}

/// Why an ore file was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OreError {
    /// The text is not valid RON for the settings.
    Ron(String),
    /// The numbers do not fit together.
    Invalid(String),
}

impl fmt::Display for OreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OreError::Ron(e) | OreError::Invalid(e) => write!(f, "ore.ron: {e}"),
        }
    }
}

impl std::error::Error for OreError {}

impl OreSettings {
    /// Reads and checks the text of an ore file: `max` at least 1, `start` not above it,
    /// `regrow_period` between 1 and [`MAX_REGROW_PERIOD`].
    pub fn from_ron(text: &str) -> Result<OreSettings, OreError> {
        let s: OreSettings = ron::from_str(text).map_err(|e| OreError::Ron(e.to_string()))?;
        if s.max == 0 {
            return Err(OreError::Invalid("max must be at least 1".into()));
        }
        if s.start > s.max {
            return Err(OreError::Invalid(format!(
                "start {} is above max {}",
                s.start, s.max
            )));
        }
        if !(1..=MAX_REGROW_PERIOD).contains(&s.regrow_period) {
            return Err(OreError::Invalid(format!(
                "regrow_period {} is not between 1 and {MAX_REGROW_PERIOD}",
                s.regrow_period
            )));
        }
        Ok(s)
    }

    /// The shipped settings.
    ///
    /// # Panics
    ///
    /// Never for the file in the repository: a test reads it.
    pub fn embedded() -> OreSettings {
        OreSettings::from_ron(EMBEDDED_ORE).expect("the embedded ore.ron is valid")
    }
}
