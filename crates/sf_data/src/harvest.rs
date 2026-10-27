//! The harvest rules (WP7.2): what a harvester carries, how fast it mines and is unloaded, how far
//! it looks for ore, in `assets/config/harvest.ron`. `sf_bridge::harvest_rules` hands the numbers
//! to the world (`World::with_harvest`). The rules are in `docs/rules/economy.md`.
//!
//! ```
//! use sf_data::harvest::HarvestSettings;
//!
//! let h = HarvestSettings::embedded();
//! assert!(h.mine_amount <= h.capacity);
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};

/// The shipped file, compiled in like the other tables.
pub const EMBEDDED_HARVEST: &str = include_str!("../../../assets/config/harvest.ron");

/// The harvest numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarvestSettings {
    /// Units of ore a harvester carries.
    pub capacity: u16,
    /// Ore taken in one bite.
    pub mine_amount: u16,
    /// Ticks between two bites.
    pub mine_period: u16,
    /// Ore the refinery takes off a harvester in one bite.
    pub unload_amount: u16,
    /// Ticks between two unloading bites.
    pub unload_period: u16,
    /// How far, in cells, a harvester looks for ore.
    pub search_radius: u16,
    /// Ticks between two looks of a harvester that found nothing.
    pub retry_period: u16,
    /// Cells from the refinery at which a harvester asks for a place.
    pub call_range: u16,
    /// Ticks a granted harvester has to arrive.
    pub grant_timeout: u16,
}

/// Why a harvest file was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HarvestError {
    /// The text is not valid RON for the settings.
    Ron(String),
    /// The numbers do not fit together.
    Invalid(String),
}

impl fmt::Display for HarvestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HarvestError::Ron(e) | HarvestError::Invalid(e) => write!(f, "harvest.ron: {e}"),
        }
    }
}

impl std::error::Error for HarvestError {}

impl HarvestSettings {
    /// Reads and checks the text of a harvest file: every number at least 1 and a bite no bigger
    /// than the load.
    pub fn from_ron(text: &str) -> Result<HarvestSettings, HarvestError> {
        let s: HarvestSettings =
            ron::from_str(text).map_err(|e| HarvestError::Ron(e.to_string()))?;
        let at_least_one = [
            ("capacity", s.capacity),
            ("mine_amount", s.mine_amount),
            ("mine_period", s.mine_period),
            ("unload_amount", s.unload_amount),
            ("unload_period", s.unload_period),
            ("search_radius", s.search_radius),
            ("retry_period", s.retry_period),
            ("call_range", s.call_range),
            ("grant_timeout", s.grant_timeout),
        ];
        for (name, value) in at_least_one {
            if value == 0 {
                return Err(HarvestError::Invalid(format!("{name} must be at least 1")));
            }
        }
        if s.mine_amount > s.capacity {
            return Err(HarvestError::Invalid(format!(
                "mine_amount {} is above capacity {}",
                s.mine_amount, s.capacity
            )));
        }
        Ok(s)
    }

    /// The shipped settings.
    ///
    /// # Panics
    ///
    /// Never for the file in the repository: a test reads it.
    pub fn embedded() -> HarvestSettings {
        HarvestSettings::from_ron(EMBEDDED_HARVEST).expect("the embedded harvest.ron is valid")
    }
}
