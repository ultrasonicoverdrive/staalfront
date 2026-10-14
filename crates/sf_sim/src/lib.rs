#![deny(clippy::float_arithmetic)]
//! The complete game simulation: no GPU, no I/O, no clock, no floats.
//!
//! Determinism rules (PLAN.md section 5): no floating point, no std hash
//! collections, no clock, no I/O. Enforced by `scripts/check-sim-determinism.sh`, by
//! `clippy::float_arithmetic` and by `clippy.toml`.

/// Name of this crate, a placeholder until the real API lands.
pub const CRATE: &str = "sf_sim";

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "sf_sim");
    }
}
