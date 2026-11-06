//! How shroud and fog of war look in the game (WP8.3, WP8.4), in `assets/config/fog_look.ron`.
//! Presentation only; no checksum depends on it. The debug view of WP8.1 has its own file
//! ([`crate::vision_look`]).
//!
//! ```
//! use sf_data::fog_look::FogLook;
//!
//! let look = FogLook::embedded();
//! assert!(look.fog_dim > 0.0 && look.fog_dim < 1.0);
//! ```

use serde::Deserialize;

/// The shipped file, compiled in like the other tables.
pub const EMBEDDED_FOG_LOOK: &str = include_str!("../../../assets/config/fog_look.ron");

/// The most passes of the softening blur.
pub const MAX_SOFTNESS: u8 = 4;

/// An sRGB colour, bytes.
pub type Rgb = (u8, u8, u8);

/// The look of shroud, fog and ghosts.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FogLook {
    /// The terrain's colour where a cell was never seen.
    pub shroud: Rgb,
    /// The tint fog pulls the terrain towards.
    pub fog_tint: Rgb,
    /// How far, `0.0..=1.0`.
    pub fog_dim: f32,
    /// Passes of the 3 by 3 blur that softens the edges: `0..=4`.
    pub softness: u8,
    /// The team colour a ghost's model gets.
    pub ghost_colour: Rgb,
    /// A ghost's dot on the minimap.
    pub ghost_dot: Rgb,
    /// The minimap's colour for shroud.
    pub minimap_shroud: Rgb,
    /// How far fog pulls a minimap cell towards `minimap_shroud`, `0.0..=1.0`.
    pub minimap_fog_dim: f32,
}

impl FogLook {
    /// Reads and checks the look. The error says what is wrong.
    pub fn parse(text: &str) -> Result<FogLook, String> {
        let look: FogLook = ron::from_str(text).map_err(|e| e.to_string())?;
        let unit = |name: &str, v: f32| {
            if (0.0..=1.0).contains(&v) {
                Ok(())
            } else {
                Err(format!("{name} must be 0 to 1, not {v}"))
            }
        };
        unit("fog_dim", look.fog_dim)?;
        unit("minimap_fog_dim", look.minimap_fog_dim)?;
        if look.softness > MAX_SOFTNESS {
            return Err(format!(
                "softness must be 0 to {MAX_SOFTNESS}, not {}",
                look.softness
            ));
        }
        Ok(look)
    }

    /// The shipped look. Panics if the shipped file is wrong (a test reads it).
    pub fn embedded() -> FogLook {
        FogLook::parse(EMBEDDED_FOG_LOOK)
            .unwrap_or_else(|e| panic!("assets/config/fog_look.ron: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_look_is_pinned() {
        let look = FogLook::embedded();
        assert_eq!(look.shroud, (3, 4, 8));
        assert_eq!(look.fog_tint, (16, 22, 38));
        assert_eq!(look.fog_dim, 0.55);
        assert_eq!(look.softness, 2);
        assert_eq!(look.ghost_colour, (88, 96, 112));
        assert_eq!(look.ghost_dot, (150, 156, 170));
        assert_eq!(look.minimap_shroud, (0, 0, 0));
        assert_eq!(look.minimap_fog_dim, 0.5);
    }

    #[test]
    fn a_broken_look_is_refused() {
        let good = EMBEDDED_FOG_LOOK;
        assert!(
            FogLook::parse(&good.replacen("fog_dim: 0.55", "fog_dim: 1.5", 1))
                .unwrap_err()
                .contains("fog_dim")
        );
        assert!(
            FogLook::parse(&good.replacen("softness: 2", "softness: 5", 1))
                .unwrap_err()
                .contains("softness")
        );
        assert!(
            FogLook::parse(&good.replacen("minimap_fog_dim: 0.5", "minimap_fog_dim: -0.1", 1))
                .is_err()
        );
        assert!(FogLook::parse(&good.replacen("shroud:", "shrouds:", 1)).is_err());
    }
}
