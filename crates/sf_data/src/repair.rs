//! The repair and sale numbers (WP6.6), in `assets/config/repair.ron`: how fast a structure is
//! repaired and what that costs, how long a sale takes and what it pays back.
//! `sf_bridge::repair_rules` hands them to the world (`World::with_repair`). The rules are in
//! `docs/rules/repair_sell.md`.
//!
//! ```
//! use sf_data::repair::RepairSettings;
//!
//! let r = RepairSettings::embedded();
//! assert!(r.hp_per_tick >= 1 && r.sell_ticks >= 1);
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};

/// The shipped file, compiled in like the other tables.
pub const EMBEDDED_REPAIR: &str = include_str!("../../../assets/config/repair.ron");

/// Most hit points per tick a file may ask for (`sf_sim::MAX_REPAIR_HP_PER_TICK`; a test in
/// `sf_bridge` holds the two equal).
pub const MAX_HP_PER_TICK: u16 = 1000;
/// The longest sale a file may ask for (`sf_sim::MAX_SELL_TICKS`).
pub const MAX_SELL_TICKS: u32 = 6000;

/// The repair and sale numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairSettings {
    /// Hit points gained per tick while repairing: 1 to [`MAX_HP_PER_TICK`].
    pub hp_per_tick: u16,
    /// Cost of a repair from 0 to full hit points, in thousandths of the price: 0 to 1000.
    pub full_cost_permille: u32,
    /// Ticks a sale takes: 1 to [`MAX_SELL_TICKS`].
    pub sell_ticks: u32,
    /// The part of the price paid back at full hit points, in thousandths: 0 to 1000.
    pub refund_permille: u32,
}

/// Why a repair file was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepairError {
    /// The text is not valid RON for the settings.
    Ron(String),
    /// The numbers are out of bounds.
    Invalid(String),
}

impl fmt::Display for RepairError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RepairError::Ron(e) | RepairError::Invalid(e) => write!(f, "repair.ron: {e}"),
        }
    }
}

impl std::error::Error for RepairError {}

fn out_of_bounds(what: &str, n: u64, lo: u64, hi: u64) -> RepairError {
    RepairError::Invalid(format!("{what} must be {lo} to {hi}, not {n}"))
}

impl RepairSettings {
    /// Reads and checks the text of a repair file.
    pub fn from_ron(text: &str) -> Result<RepairSettings, RepairError> {
        let s: RepairSettings = ron::from_str(text).map_err(|e| RepairError::Ron(e.to_string()))?;
        if s.hp_per_tick == 0 || s.hp_per_tick > MAX_HP_PER_TICK {
            return Err(out_of_bounds(
                "hp_per_tick",
                s.hp_per_tick.into(),
                1,
                MAX_HP_PER_TICK.into(),
            ));
        }
        if s.full_cost_permille > 1000 {
            return Err(out_of_bounds(
                "full_cost_permille",
                s.full_cost_permille.into(),
                0,
                1000,
            ));
        }
        if s.sell_ticks == 0 || s.sell_ticks > MAX_SELL_TICKS {
            return Err(out_of_bounds(
                "sell_ticks",
                s.sell_ticks.into(),
                1,
                MAX_SELL_TICKS.into(),
            ));
        }
        if s.refund_permille > 1000 {
            return Err(out_of_bounds(
                "refund_permille",
                s.refund_permille.into(),
                0,
                1000,
            ));
        }
        Ok(s)
    }

    /// The shipped settings.
    ///
    /// # Panics
    ///
    /// Never for the file in the repository: a test reads it.
    pub fn embedded() -> RepairSettings {
        RepairSettings::from_ron(EMBEDDED_REPAIR).expect("the embedded repair.ron is valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_numbers_are_pinned() {
        let s = RepairSettings::embedded();
        assert_eq!(s.hp_per_tick, 3);
        assert_eq!(s.full_cost_permille, 250);
        assert_eq!(s.sell_ticks, 100);
        assert_eq!(s.refund_permille, 500);
    }

    #[test]
    fn numbers_out_of_bounds_are_refused() {
        let ok = |h: u32, c: u32, t: u32, r: u32| {
            RepairSettings::from_ron(&format!(
                "(hp_per_tick: {h}, full_cost_permille: {c}, sell_ticks: {t}, refund_permille: {r})"
            ))
        };
        assert!(ok(1, 0, 1, 0).is_ok());
        assert!(ok(1000, 1000, 6000, 1000).is_ok());
        for bad in [
            ok(0, 0, 1, 0),
            ok(1001, 0, 1, 0),
            ok(1, 1001, 1, 0),
            ok(1, 0, 0, 0),
            ok(1, 0, 6001, 0),
            ok(1, 0, 1, 1001),
        ] {
            assert!(matches!(bad, Err(RepairError::Invalid(_))), "{bad:?}");
        }
        assert!(matches!(
            RepairSettings::from_ron("(hp_per_tick: 1)"),
            Err(RepairError::Ron(_))
        ));
        assert!(matches!(
            RepairSettings::from_ron(
                "(hp_per_tick: 1, full_cost_permille: 0, sell_ticks: 1, refund_permille: 0, extra: 1)"
            ),
            Err(RepairError::Ron(_))
        ));
    }
}
