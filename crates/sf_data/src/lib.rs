//! Unit, building and weapon definitions (RON): validation and loading.
//!
//! WP1.7 adds the engine map format ([`map`]): a RON header, a 16-bit
//! heightmap and a per-cell material index map, with a validating loader and
//! a deterministic procedural generator. WP2.1 adds the terrain types
//! ([`terrain`]): a RON table of what a gameplay cell can be (grass, road,
//! water, rock, ore, bridge) and an optional per-cell layer of them in a map.
//! WP3.1 adds the unit, structure and weapon definitions ([`units`]): RON
//! files with validation that names the file and line, and their conversion
//! to fixed-point numbers for the simulation. WP4.1 adds the terrain cost of
//! each movement class ([`movement`]): the classes of `unit_classes.ron` put
//! together with the terrain types and checked against each other. WP4.2 adds
//! the settings of the simulation itself ([`sim_settings`], `sim.ron`): today the
//! node budget of the path service.

pub mod build_stages;
pub mod effects;
pub mod map;
pub mod movement;
pub mod ore;
pub mod sim_settings;
pub mod skirmish;
pub mod terrain;
pub mod units;

/// Name of this crate, a placeholder until the real API lands.
pub const CRATE: &str = "sf_data";

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "sf_data");
    }
}
