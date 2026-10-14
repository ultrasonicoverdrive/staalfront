//! Main loop, window, input, time and assets; no game rules.

/// Name of this crate, a placeholder until the real API lands.
pub const CRATE: &str = "sf_engine";

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "sf_engine");
    }
}
