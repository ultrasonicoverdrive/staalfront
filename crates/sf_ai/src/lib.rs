//! Computer opponent; talks to sf_sim only through commands.

/// Name of this crate, a placeholder until the real API lands.
pub const CRATE: &str = "sf_ai";

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "sf_ai");
    }
}
