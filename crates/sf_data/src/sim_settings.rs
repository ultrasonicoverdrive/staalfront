//! Settings of the simulation (WP4.2): the numbers a designer turns that are
//! not a property of one unit, in `assets/config/sim.ron`.
//!
//! Today that is the node budget of the path service per tick (WP4.2) and the
//! flowfield threshold (WP4.3): how many units of one class a single order must
//! send to one place to get a flowfield. Game data is in RON and not in code
//! (PLAN.md section 5, rule 6), so the numbers live in the file; the simulation
//! keeps its own defaults (the same numbers) for worlds that are given none,
//! which is what the tests and the frozen headless scenarios use, so that
//! turning a knob in the file never moves their checksums.
//! `sf_bridge::path_budget` and `sf_bridge::flowfield_threshold` hand the values
//! to the world, which checks them against its minimums.
//!
//! ```
//! use sf_data::sim_settings::SimSettings;
//!
//! assert_eq!(SimSettings::embedded().path_node_budget, 4096);
//! assert_eq!(SimSettings::embedded().flowfield_threshold, 8);
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};

/// The shipped file, compiled in, like the other tables.
pub const EMBEDDED_SIM_SETTINGS: &str = include_str!("../../../assets/config/sim.ron");

/// The most nodes a tick may be given: a bound against a typing mistake (a
/// budget of a few million would make one tick as long as a whole second).
pub const MAX_PATH_NODE_BUDGET: u32 = 1_000_000;

/// The most units a group order may need to get a flowfield: a bound against a
/// typing mistake.
pub const MAX_FLOWFIELD_THRESHOLD: u32 = 1000;

/// The settings of the simulation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimSettings {
    /// Nodes of path search work per tick, for all units together.
    pub path_node_budget: u32,
    /// Units of one movement class that one order must send to one place for a
    /// flowfield; zero turns flowfields off.
    pub flowfield_threshold: u32,
}

/// Why a settings file was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimSettingsError {
    /// The text is not valid RON for the settings.
    Ron(String),
    /// `path_node_budget` is zero or above [`MAX_PATH_NODE_BUDGET`].
    PathNodeBudget(u32),
    /// `flowfield_threshold` is one (a lone unit is no group) or above
    /// [`MAX_FLOWFIELD_THRESHOLD`].
    FlowfieldThreshold(u32),
}

impl fmt::Display for SimSettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ron(e) => write!(f, "sim.ron: {e}"),
            Self::PathNodeBudget(n) => write!(
                f,
                "sim.ron: path_node_budget {n} is not between 1 and {MAX_PATH_NODE_BUDGET}"
            ),
            Self::FlowfieldThreshold(n) => write!(
                f,
                "sim.ron: flowfield_threshold {n} is not 0 (off) or between 2 and {MAX_FLOWFIELD_THRESHOLD}"
            ),
        }
    }
}

impl std::error::Error for SimSettingsError {}

impl SimSettings {
    /// Reads and checks the text of a settings file.
    pub fn from_ron(text: &str) -> Result<SimSettings, SimSettingsError> {
        let settings: SimSettings =
            ron::from_str(text).map_err(|e| SimSettingsError::Ron(e.to_string()))?;
        if settings.path_node_budget == 0 || settings.path_node_budget > MAX_PATH_NODE_BUDGET {
            return Err(SimSettingsError::PathNodeBudget(settings.path_node_budget));
        }
        if settings.flowfield_threshold == 1
            || settings.flowfield_threshold > MAX_FLOWFIELD_THRESHOLD
        {
            return Err(SimSettingsError::FlowfieldThreshold(
                settings.flowfield_threshold,
            ));
        }
        Ok(settings)
    }

    /// The shipped settings ([`EMBEDDED_SIM_SETTINGS`]).
    ///
    /// # Panics
    ///
    /// Never for the file in the repository: a test reads it.
    pub fn embedded() -> SimSettings {
        SimSettings::from_ron(EMBEDDED_SIM_SETTINGS).expect("the embedded sim.ron is valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads() {
        let s = SimSettings::from_ron(EMBEDDED_SIM_SETTINGS).unwrap();
        assert_eq!(s, SimSettings::embedded());
        assert_eq!(s.path_node_budget, 4096);
    }

    #[test]
    fn a_bad_file_is_refused_with_its_reason() {
        assert!(matches!(
            SimSettings::from_ron("(path_node_budget: 0, flowfield_threshold: 8)"),
            Err(SimSettingsError::PathNodeBudget(0))
        ));
        assert!(matches!(
            SimSettings::from_ron("(path_node_budget: 2000000, flowfield_threshold: 8)"),
            Err(SimSettingsError::PathNodeBudget(2_000_000))
        ));
        assert!(matches!(
            SimSettings::from_ron("(path_node_budget: -4, flowfield_threshold: 8)"),
            Err(SimSettingsError::Ron(_))
        ));
        assert!(matches!(
            SimSettings::from_ron("(path_node_budget: 4096, flowfield_threshold: 8, typo: 1)"),
            Err(SimSettingsError::Ron(_))
        ));
        assert!(matches!(
            SimSettings::from_ron("()"),
            Err(SimSettingsError::Ron(_))
        ));
        // The threshold is a field of its own, not optional.
        assert!(matches!(
            SimSettings::from_ron("(path_node_budget: 4096)"),
            Err(SimSettingsError::Ron(_))
        ));
    }

    #[test]
    fn the_flowfield_threshold_is_off_or_a_group() {
        let with = |n: u32| {
            SimSettings::from_ron(&format!(
                "(path_node_budget: 4096, flowfield_threshold: {n})"
            ))
        };
        assert_eq!(with(0).unwrap().flowfield_threshold, 0, "off");
        assert_eq!(with(2).unwrap().flowfield_threshold, 2);
        assert_eq!(with(1000).unwrap().flowfield_threshold, 1000);
        assert_eq!(
            with(1).err(),
            Some(SimSettingsError::FlowfieldThreshold(1)),
            "a lone unit is no group"
        );
        assert_eq!(
            with(1001).err(),
            Some(SimSettingsError::FlowfieldThreshold(1001))
        );
    }
}
