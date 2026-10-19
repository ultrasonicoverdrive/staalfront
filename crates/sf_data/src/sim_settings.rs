//! Settings of the simulation (WP4.2): the numbers a designer turns that are
//! not a property of one unit, in `assets/config/sim.ron`.
//!
//! Today that is the node budget of the path service per tick (WP4.2), the
//! flowfield threshold (WP4.3): how many units of one class a single order must
//! send to one place to get a flowfield, and the collision settings (WP4.4):
//! whether bodies collide, how long a blocked unit waits before it detours, how
//! long without progress makes it step to a free neighbouring spot, and how
//! many such steps before it asks for a new route, and the replan settings (WP4.7): how
//! many ticks between two replans of a stuck unit, how many it may ask for in one order
//! before the order is given up, and how many ticks between two new searches for a
//! target that keeps moving, and the formation settings (WP4.6): whether a group move order
//! forms a formation, how many units it needs and may have, how wide the lattice is, how
//! fast the formation goes and how far from its slot a member may be. Game data is in RON and not in code
//! (PLAN.md section 5, rule 6), so the numbers live in the file; the simulation
//! keeps its own defaults (the same numbers) for worlds that are given none,
//! which is what the tests and the frozen headless scenarios use, so that
//! turning a knob in the file never moves their checksums.
//! `sf_bridge::path_budget`, `sf_bridge::flowfield_threshold`,
//! `sf_bridge::collision_config`, `sf_bridge::replan_config` and
//! `sf_bridge::formation_config` hand the values to the
//! world, which checks them against its minimums.
//!
//! ```
//! use sf_data::sim_settings::SimSettings;
//!
//! assert_eq!(SimSettings::embedded().path_node_budget, 4096);
//! assert_eq!(SimSettings::embedded().flowfield_threshold, 8);
//! assert!(SimSettings::embedded().collision);
//! assert_eq!(SimSettings::embedded().stuck_ticks, 60);
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

/// The most ticks a unit may be given before it is called stuck: a bound against
/// a typing mistake (fifty seconds).
pub const MAX_STUCK_TICKS: u16 = 1000;

/// The most ticks a replan cooldown may be (fifty seconds).
pub const MAX_REPLAN_COOLDOWN: u16 = 1000;

/// The most replans one order may allow a stuck unit.
pub const MAX_REPLAN_MAX: u8 = 100;

/// The most units a formation may hold (the arrival slots of a field are as many).
pub const MAX_FORMATION_UNITS: u16 = 256;

fn default_collision() -> bool {
    true
}

fn default_wait_ticks() -> u16 {
    6
}

fn default_stuck_ticks() -> u16 {
    60
}

fn default_stuck_attempts() -> u8 {
    3
}

fn default_replan_cooldown() -> u16 {
    100
}

fn default_replan_max() -> u8 {
    3
}

fn default_retarget_cooldown() -> u16 {
    20
}

fn default_formation() -> bool {
    true
}

fn default_formation_min_units() -> u16 {
    3
}

fn default_formation_max_units() -> u16 {
    24
}

fn default_formation_spacing_permille() -> u16 {
    1250
}

fn default_formation_spread_permille() -> u16 {
    1500
}

fn default_formation_pace_permille() -> u16 {
    900
}

fn default_formation_catchup_permille() -> u16 {
    1000
}

fn default_formation_reform_tenths() -> u16 {
    30
}

fn default_formation_patience_ticks() -> u16 {
    300
}

/// The settings of the simulation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimSettings {
    /// Nodes of path search work per tick, for all units together.
    pub path_node_budget: u32,
    /// Units of one movement class that one order must send to one place for a
    /// flowfield; zero turns flowfields off.
    pub flowfield_threshold: u32,
    /// Whether bodies collide (WP4.4); false lets units pass through each other.
    #[serde(default = "default_collision")]
    pub collision: bool,
    /// Ticks a unit that cannot get on waits before it detours.
    #[serde(default = "default_wait_ticks")]
    pub wait_ticks: u16,
    /// Ticks without progress after which a unit steps to a free neighbouring
    /// spot.
    #[serde(default = "default_stuck_ticks")]
    pub stuck_ticks: u16,
    /// Such steps in a row after which it asks for a new route.
    #[serde(default = "default_stuck_attempts")]
    pub stuck_attempts: u8,
    /// Ticks between two replans of a stuck unit (WP4.7).
    #[serde(default = "default_replan_cooldown")]
    pub replan_cooldown: u16,
    /// Replans a stuck unit may ask for in one order; one more and the order is given
    /// up (WP4.7).
    #[serde(default = "default_replan_max")]
    pub replan_max: u8,
    /// Ticks between two new searches for a target that keeps moving (WP4.7).
    #[serde(default = "default_retarget_cooldown")]
    pub retarget_cooldown: u16,
    /// Whether a group move order forms a formation (WP4.6); false moves the group as a
    /// group order did before.
    #[serde(default = "default_formation")]
    pub formation: bool,
    /// Units that cross the same ground that one move order must send to one place to form
    /// a formation. 2 to `formation_max_units`.
    #[serde(default = "default_formation_min_units")]
    pub formation_min_units: u16,
    /// The most units one move order may send and still make formations; a bigger selection is
    /// a crowd and moves as a group order. Up to [`MAX_FORMATION_UNITS`].
    #[serde(default = "default_formation_max_units")]
    pub formation_max_units: u16,
    /// The spacing of the lattice, as thousandths of what the widest body needs. 1000 to 3000.
    #[serde(default = "default_formation_spacing_permille")]
    pub formation_spacing_permille: u16,
    /// The most a formation may spread, as thousandths of the radius of a disc that holds
    /// the bodies. 1000 to 4000.
    #[serde(default = "default_formation_spread_permille")]
    pub formation_spread_permille: u16,
    /// The pace of the formation, as thousandths of the speed of its slowest member.
    /// 500 to 1000.
    #[serde(default = "default_formation_pace_permille")]
    pub formation_pace_permille: u16,
    /// The most speed a member uses to catch up with its slot, as thousandths of its own;
    /// not less than the pace. At most 1000.
    #[serde(default = "default_formation_catchup_permille")]
    pub formation_catchup_permille: u16,
    /// How far from its slot a member may be, in tenths of a cell, before the leader waits
    /// for it. 5 to 100.
    #[serde(default = "default_formation_reform_tenths")]
    pub formation_reform_tenths: u16,
    /// For how many ticks the leader waits for a straggler that does not come closer. 20 to
    /// 2000.
    #[serde(default = "default_formation_patience_ticks")]
    pub formation_patience_ticks: u16,
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
    /// The collision numbers are not a valid combination: at least one tick to
    /// wait, a stuck limit above it and at most [`MAX_STUCK_TICKS`], and at
    /// least one attempt.
    Collision {
        /// `wait_ticks`.
        wait: u16,
        /// `stuck_ticks`.
        stuck: u16,
        /// `stuck_attempts`.
        attempts: u8,
    },
    /// The replan numbers are not valid: both cooldowns between 1 and
    /// [`MAX_REPLAN_COOLDOWN`] ticks, and `replan_max` between 1 and [`MAX_REPLAN_MAX`].
    Replan {
        /// `replan_cooldown`.
        cooldown: u16,
        /// `replan_max`.
        max: u8,
        /// `retarget_cooldown`.
        retarget: u16,
    },
    /// The formation numbers are out of their ranges (see the fields of
    /// [`SimSettings`]).
    Formation,
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
            Self::Collision {
                wait,
                stuck,
                attempts,
            } => write!(
                f,
                "sim.ron: wait_ticks {wait}, stuck_ticks {stuck} and stuck_attempts {attempts} do not fit: \
                 wait_ticks is at least 1, stuck_ticks is above it and at most {MAX_STUCK_TICKS}, \
                 stuck_attempts is at least 1"
            ),
            Self::Replan {
                cooldown,
                max,
                retarget,
            } => write!(
                f,
                "sim.ron: replan_cooldown {cooldown}, replan_max {max} and retarget_cooldown \
                 {retarget} do not fit: both cooldowns are between 1 and {MAX_REPLAN_COOLDOWN}, \
                 replan_max is between 1 and {MAX_REPLAN_MAX}"
            ),
            Self::Formation => write!(
                f,
                "sim.ron: the formation numbers do not fit: formation_min_units is 2 or more and \
                 at most formation_max_units, which is at most {MAX_FORMATION_UNITS}; \
                 formation_spacing_permille is 1000 to 3000, formation_spread_permille 1000 to \
                 4000, formation_pace_permille 500 to 1000, formation_catchup_permille from the \
                 pace up to 1000, formation_reform_tenths 5 to 100 and formation_patience_ticks \
                 20 to 2000"
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
        if settings.wait_ticks < 1
            || settings.stuck_ticks <= settings.wait_ticks
            || settings.stuck_ticks > MAX_STUCK_TICKS
            || settings.stuck_attempts < 1
        {
            return Err(SimSettingsError::Collision {
                wait: settings.wait_ticks,
                stuck: settings.stuck_ticks,
                attempts: settings.stuck_attempts,
            });
        }
        if settings.replan_cooldown < 1
            || settings.replan_cooldown > MAX_REPLAN_COOLDOWN
            || settings.retarget_cooldown < 1
            || settings.retarget_cooldown > MAX_REPLAN_COOLDOWN
            || settings.replan_max < 1
            || settings.replan_max > MAX_REPLAN_MAX
        {
            return Err(SimSettingsError::Replan {
                cooldown: settings.replan_cooldown,
                max: settings.replan_max,
                retarget: settings.retarget_cooldown,
            });
        }
        if settings.formation_min_units < 2
            || settings.formation_max_units < settings.formation_min_units
            || settings.formation_max_units > MAX_FORMATION_UNITS
            || !(1000..=3000).contains(&settings.formation_spacing_permille)
            || !(1000..=4000).contains(&settings.formation_spread_permille)
            || !(500..=1000).contains(&settings.formation_pace_permille)
            || settings.formation_catchup_permille < settings.formation_pace_permille
            || settings.formation_catchup_permille > 1000
            || !(5..=100).contains(&settings.formation_reform_tenths)
            || !(20..=2000).contains(&settings.formation_patience_ticks)
        {
            return Err(SimSettingsError::Formation);
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
    fn the_collision_settings_have_defaults_and_are_checked() {
        let s = SimSettings::from_ron("(path_node_budget: 4096, flowfield_threshold: 8)").unwrap();
        assert!(s.collision);
        assert_eq!((s.wait_ticks, s.stuck_ticks, s.stuck_attempts), (6, 60, 3));
        let with = |wait: u16, stuck: u16, attempts: u8| {
            SimSettings::from_ron(&format!(
                "(path_node_budget: 4096, flowfield_threshold: 8, collision: false, \
                 wait_ticks: {wait}, stuck_ticks: {stuck}, stuck_attempts: {attempts})"
            ))
        };
        assert!(!with(1, 2, 1).unwrap().collision, "can be turned off");
        assert!(with(0, 40, 3).is_err(), "at least a tick to wait");
        assert!(with(40, 40, 3).is_err(), "stuck comes after waiting");
        assert!(with(6, 1001, 3).is_err(), "bounded");
        assert!(with(6, 1000, 3).is_ok());
        assert!(with(6, 40, 0).is_err(), "at least one attempt");
    }

    #[test]
    fn the_replan_settings_have_defaults_and_are_checked() {
        let s = SimSettings::from_ron("(path_node_budget: 4096, flowfield_threshold: 8)").unwrap();
        assert_eq!(
            (s.replan_cooldown, s.replan_max, s.retarget_cooldown),
            (100, 3, 20)
        );
        let with = |cooldown: u16, max: u8, retarget: u16| {
            SimSettings::from_ron(&format!(
                "(path_node_budget: 4096, flowfield_threshold: 8, replan_cooldown: {cooldown}, \
                 replan_max: {max}, retarget_cooldown: {retarget})"
            ))
        };
        assert!(with(1, 1, 1).is_ok());
        assert!(with(1000, 100, 1000).is_ok());
        assert!(with(0, 3, 20).is_err(), "a cooldown is at least a tick");
        assert!(with(100, 3, 0).is_err());
        assert!(with(1001, 3, 20).is_err(), "bounded");
        assert!(with(100, 3, 1001).is_err(), "bounded");
        assert!(with(100, 0, 20).is_err(), "at least one replan");
        assert!(with(100, 101, 20).is_err(), "bounded");
    }

    #[test]
    fn the_formation_settings_have_defaults_and_are_checked() {
        let s = SimSettings::from_ron("(path_node_budget: 4096, flowfield_threshold: 8)").unwrap();
        assert!(s.formation);
        assert_eq!(
            (
                s.formation_min_units,
                s.formation_max_units,
                s.formation_spacing_permille,
                s.formation_spread_permille,
                s.formation_pace_permille,
                s.formation_catchup_permille,
                s.formation_reform_tenths,
                s.formation_patience_ticks
            ),
            (3, 24, 1250, 1500, 900, 1000, 30, 300)
        );
        let with = |field: &str, value: &str| {
            SimSettings::from_ron(&format!(
                "(path_node_budget: 4096, flowfield_threshold: 8, {field}: {value})"
            ))
        };
        assert!(with("formation", "false").is_ok_and(|s| !s.formation));
        assert!(with("formation_min_units", "2").is_ok());
        assert!(with("formation_min_units", "1").is_err(), "a pair at least");
        assert!(
            with("formation_min_units", "25").is_err(),
            "above the maximum"
        );
        assert!(with("formation_max_units", "256").is_ok());
        assert!(with("formation_max_units", "257").is_err(), "bounded");
        assert!(with("formation_spacing_permille", "999").is_err());
        assert!(with("formation_spacing_permille", "3000").is_ok());
        assert!(with("formation_spread_permille", "4001").is_err());
        assert!(with("formation_pace_permille", "499").is_err());
        assert!(
            with("formation_pace_permille", "950")
                .is_ok_and(|s| s.formation_catchup_permille >= 950)
        );
        assert!(
            with("formation_catchup_permille", "899").is_err(),
            "not below the pace"
        );
        assert!(with("formation_catchup_permille", "1001").is_err());
        assert!(with("formation_reform_tenths", "4").is_err());
        assert!(with("formation_reform_tenths", "100").is_ok());
        assert!(with("formation_patience_ticks", "19").is_err());
        assert!(with("formation_patience_ticks", "2000").is_ok());
        assert_eq!(
            with("formation_patience_ticks", "2001").err(),
            Some(SimSettingsError::Formation)
        );
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
