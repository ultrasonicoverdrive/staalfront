//! The palette of set v5, read from `assets/config/model_palette.ron` (compiled in, so a run
//! does not depend on the working directory) and checked when it loads.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::kit::{Material, TEAM};

/// The shipped palette file.
pub const PALETTE_RON: &str = include_str!("../../../assets/config/model_palette.ron");

/// The colours of one faction, one per surface type (docs/art-style.md).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FactionColours {
    pub paint: [u8; 3],
    pub paint_light: [u8; 3],
    pub paint2: [u8; 3],
    pub paint2_shade: [u8; 3],
    pub metal: [u8; 3],
    pub rubber: [u8; 3],
    pub accent: [u8; 3],
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    pub factions: BTreeMap<String, FactionColours>,
    pub team_base: [u8; 3],
}

impl Palette {
    /// Parse and check a palette.
    pub fn parse(text: &str) -> Result<Self, String> {
        let p: Palette = ron::from_str(text).map_err(|e| e.to_string())?;
        for name in ["meridian", "veyran"] {
            if !p.factions.contains_key(name) {
                return Err(format!("faction {name} is missing"));
            }
        }
        Ok(p)
    }

    /// The shipped palette.
    ///
    /// # Panics
    /// When the shipped file does not parse (a test pins it).
    pub fn shipped() -> Self {
        Self::parse(PALETTE_RON).expect("assets/config/model_palette.ron")
    }

    /// The material slots of a faction's model, in this fixed order (see [`slot`]):
    /// paint, paint2, team, metal, rubber, accent.
    pub fn materials(&self, faction: &str) -> Vec<Material> {
        let f = &self.factions[faction];
        let m = |name: &str, srgb: [u8; 3]| Material {
            name: name.to_owned(),
            srgb,
        };
        vec![
            m("paint", f.paint),
            m("paint2", f.paint2),
            m(TEAM, self.team_base),
            m("metal", f.metal),
            m("rubber", f.rubber),
            m("accent", f.accent),
        ]
    }
}

/// The material slots of [`Palette::materials`].
pub mod slot {
    use crate::kit::Mat;
    pub const PAINT: Mat = Mat(0);
    pub const PAINT2: Mat = Mat(1);
    pub const TEAM: Mat = Mat(2);
    pub const METAL: Mat = Mat(3);
    pub const RUBBER: Mat = Mat(4);
    pub const ACCENT: Mat = Mat(5);
}
