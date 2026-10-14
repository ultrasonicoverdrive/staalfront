//! Unit, building and weapon definitions (RON): validation and loading.

/// Name of this crate, a placeholder until the real API lands.
pub const CRATE: &str = "sf_data";

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "sf_data");
    }
}
