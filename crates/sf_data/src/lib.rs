//! Unit, building and weapon definitions (RON): validation and loading.
//!
//! WP1.7 adds the engine map format ([`map`]): a RON header, a 16-bit
//! heightmap and a per-cell material index map, with a validating loader and
//! a deterministic procedural generator. Game rules (terrain types) are not
//! part of it; those come in phase 2.

pub mod map;

/// Name of this crate, a placeholder until the real API lands.
pub const CRATE: &str = "sf_data";

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "sf_data");
    }
}
