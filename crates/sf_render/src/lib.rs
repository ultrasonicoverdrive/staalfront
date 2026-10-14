//! wgpu rendering: terrain, models, camera, zoom, effects and LOD.

/// Name of this crate, a placeholder until the real API lands.
pub const CRATE: &str = "sf_render";

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "sf_render");
    }
}
