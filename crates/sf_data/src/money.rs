//! The money numbers (WP7.4), in `assets/config/money.ron`: what a unit of ore is worth in credits
//! and what each player starts with. `sf_bridge::money_rules` hands them to the world
//! (`World::with_money`). The rules are in `docs/rules/economy.md`, "Money".
//!
//! ```
//! use sf_data::money::MoneySettings;
//!
//! let m = MoneySettings::embedded();
//! assert!(m.credits_per_ore >= 1);
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};

/// The shipped file, compiled in like the other tables.
pub const EMBEDDED_MONEY: &str = include_str!("../../../assets/config/money.ron");

/// The most credits one unit of ore can be worth (the simulation's limit,
/// `sf_sim::MAX_CREDITS_PER_ORE`; a test in `sf_bridge` holds the two equal).
pub const MAX_CREDITS_PER_ORE: u32 = 1000;
/// The most credits a player can start with (`sf_sim::MAX_STARTING_CREDITS`).
pub const MAX_STARTING_CREDITS: u64 = 1_000_000_000;

/// The money numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoneySettings {
    /// Credits one unit of ore is worth.
    pub credits_per_ore: u32,
    /// Credits every player starts with.
    pub starting_credits: u64,
}

/// Why a money file was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MoneyError {
    /// The text is not valid RON for the settings.
    Ron(String),
    /// The numbers are out of bounds.
    Invalid(String),
}

impl fmt::Display for MoneyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MoneyError::Ron(e) | MoneyError::Invalid(e) => write!(f, "money.ron: {e}"),
        }
    }
}

impl std::error::Error for MoneyError {}

impl MoneySettings {
    /// Reads and checks the text of a money file.
    pub fn from_ron(text: &str) -> Result<MoneySettings, MoneyError> {
        let s: MoneySettings = ron::from_str(text).map_err(|e| MoneyError::Ron(e.to_string()))?;
        if s.credits_per_ore == 0 || s.credits_per_ore > MAX_CREDITS_PER_ORE {
            return Err(MoneyError::Invalid(format!(
                "credits_per_ore must be 1 to {MAX_CREDITS_PER_ORE}, not {}",
                s.credits_per_ore
            )));
        }
        if s.starting_credits > MAX_STARTING_CREDITS {
            return Err(MoneyError::Invalid(format!(
                "starting_credits must be at most {MAX_STARTING_CREDITS}, not {}",
                s.starting_credits
            )));
        }
        Ok(s)
    }

    /// The shipped settings.
    ///
    /// # Panics
    ///
    /// Never for the file in the repository: a test reads it.
    pub fn embedded() -> MoneySettings {
        MoneySettings::from_ron(EMBEDDED_MONEY).expect("the embedded money.ron is valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_numbers_are_pinned() {
        let s = MoneySettings::embedded();
        assert_eq!(s.credits_per_ore, 1);
        assert_eq!(s.starting_credits, 1000);
    }

    #[test]
    fn numbers_out_of_bounds_are_refused() {
        let ok = |p: u32, s: u64| {
            MoneySettings::from_ron(&format!("(credits_per_ore: {p}, starting_credits: {s})"))
        };
        assert!(ok(1, 0).is_ok());
        assert!(ok(MAX_CREDITS_PER_ORE, MAX_STARTING_CREDITS).is_ok());
        assert!(matches!(ok(0, 5), Err(MoneyError::Invalid(_))));
        assert!(matches!(
            ok(MAX_CREDITS_PER_ORE + 1, 5),
            Err(MoneyError::Invalid(_))
        ));
        assert!(matches!(
            ok(1, MAX_STARTING_CREDITS + 1),
            Err(MoneyError::Invalid(_))
        ));
        assert!(matches!(
            MoneySettings::from_ron("(credits_per_ore: 1)"),
            Err(MoneyError::Ron(_))
        ));
        assert!(matches!(
            MoneySettings::from_ron("(credits_per_ore: 1, starting_credits: 5, extra: 1)"),
            Err(MoneyError::Ron(_))
        ));
    }
}
