//! How the terrain types look (WP2.2): the data file
//! `assets/config/terrain_look.ron`, the procedural texture of every look and
//! the per-cell index the terrain shader reads.
//!
//! The gameplay terrain type of a 4 m cell (`sf_data::terrain`, WP2.1) decides
//! what the ground looks like, so what you see is what units can do. A map
//! with a terrain layer is drawn from it: one texture-array layer per entry of
//! the terrain types table (in id order) and one index texel per gameplay
//! cell. The material map of the map file (one material per 1 m sample) is not
//! used then; a map without a terrain layer is drawn from it as before
//! (WP1.7), with one texel per sample.
//!
//! The mapping type -> look lives in data, by type *name* (the ids are what map
//! files store, the names are what the tools use). A type without an entry
//! gets a colour derived from its name, so a new type renders before somebody
//! has chosen its colours; the test `every_shipped_type_has_a_look` makes sure
//! the shipped ones are all chosen.
//!
//! The index texel is two bytes: the texture-array layer and the style bits
//! ([`STYLE_FLAT`], [`STYLE_LIQUID`]). The shader blends the four nearest cell
//! centres with a narrow ramp around the cell border (see `terrain.wgsl`).

use serde::Deserialize;
use sf_data::map::Map;
use sf_data::terrain::TerrainTypes;

use crate::terrain::{TEXTURE_SIZE, material_texture, name_hash, tile_noise};

/// The shipped looks, compiled in like the terrain types table.
pub const EMBEDDED_TERRAIN_LOOK: &str = include_str!("../../../assets/config/terrain_look.ron");

/// Style bit: drawn flat, without hill shading (water, a bridge deck).
pub const STYLE_FLAT: u8 = 1;
/// Style bit: a liquid; the shader draws a light line along its shore.
pub const STYLE_LIQUID: u8 = 2;

/// The procedural pattern of a look.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Pattern {
    /// Soft patches of three sizes (ground, road).
    Noise,
    /// Patches with dark cracks (rock).
    Cracks,
    /// Thin light wave crests (water).
    Ripples,
    /// `count` planks across the 4 m tile with dark gaps (a deck).
    Planks { count: u32 },
    /// Bright specks on a darker ground (ore).
    Flecks { color: [u8; 3] },
}

/// One look: what a terrain type is drawn like.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainLook {
    /// Name of the terrain type (`terrain_types.ron`).
    pub terrain: String,
    /// Base colour, sRGB.
    pub color: [u8; 3],
    /// Strength of the variation, `0..=1`.
    pub contrast: f32,
    pub pattern: Pattern,
    /// No hill shading: the ground is drawn level.
    #[serde(default)]
    pub flat: bool,
    /// A liquid: gets the shore line.
    #[serde(default)]
    pub liquid: bool,
}

/// How two types meet (see the file comment of `terrain_look.ron`).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub ramp_m: f32,
    pub wobble_m: f32,
    pub shore: [f32; 3],
    pub shore_amount: f32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LookFile {
    edge: Edge,
    looks: Vec<TerrainLook>,
}

/// Why a looks file was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LookError(pub String);

impl std::fmt::Display for LookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "terrain looks: {}", self.0)
    }
}

impl std::error::Error for LookError {}

/// A validated looks file.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainLooks {
    pub edge: Edge,
    looks: Vec<TerrainLook>,
}

impl TerrainLooks {
    /// Parse and validate: unique names, `contrast` in `0..=1`, a ramp of at
    /// most one cell, a wobble of at most one metre and a half, plank counts
    /// 1 to 64, shore strength `0..=1`.
    pub fn from_ron(text: &str) -> Result<Self, LookError> {
        let file: LookFile = ron::from_str(text).map_err(|e| LookError(e.to_string()))?;
        let cell = sf_math::render_bridge::METRES_PER_CELL;
        let e = file.edge;
        if !(0.0..=cell).contains(&e.ramp_m) {
            return Err(LookError(format!("ramp_m must be 0..={cell}")));
        }
        if !(0.0..=1.5).contains(&e.wobble_m) {
            return Err(LookError("wobble_m must be 0..=1.5".into()));
        }
        if !(0.0..=1.0).contains(&e.shore_amount) {
            return Err(LookError("shore_amount must be 0..=1".into()));
        }
        for (i, l) in file.looks.iter().enumerate() {
            if !(0.0..=1.0).contains(&l.contrast) {
                return Err(LookError(format!("{}: contrast must be 0..=1", l.terrain)));
            }
            if let Pattern::Planks { count } = l.pattern
                && !(1..=64).contains(&count)
            {
                return Err(LookError(format!("{}: planks count 1..=64", l.terrain)));
            }
            if file.looks[..i].iter().any(|o| o.terrain == l.terrain) {
                return Err(LookError(format!("{} appears twice", l.terrain)));
            }
        }
        Ok(Self {
            edge: e,
            looks: file.looks,
        })
    }

    /// The shipped file.
    ///
    /// # Panics
    ///
    /// If the shipped file is invalid, which a test rules out.
    pub fn embedded() -> Self {
        Self::from_ron(EMBEDDED_TERRAIN_LOOK).expect("the embedded terrain_look.ron is valid")
    }

    /// The entry for a terrain type name, if the file has one.
    pub fn get(&self, name: &str) -> Option<&TerrainLook> {
        self.looks.iter().find(|l| l.terrain == name)
    }

    /// The look of a terrain type: its entry, or a plain one in a colour
    /// derived from the name.
    pub fn look_for(&self, name: &str) -> TerrainLook {
        self.get(name).cloned().unwrap_or_else(|| {
            let h = name_hash(name);
            TerrainLook {
                terrain: name.to_owned(),
                color: [
                    64 + (h & 0x7f) as u8,
                    64 + ((h >> 8) & 0x7f) as u8,
                    64 + ((h >> 16) & 0x7f) as u8,
                ],
                contrast: 0.25,
                pattern: Pattern::Noise,
                flat: false,
                liquid: false,
            }
        })
    }
}

impl TerrainLook {
    /// The style bits of the cell index.
    pub fn style_bits(&self) -> u8 {
        (if self.flat { STYLE_FLAT } else { 0 }) | (if self.liquid { STYLE_LIQUID } else { 0 })
    }

    /// RGBA8 sRGB pixels of the tileable texture, [`TEXTURE_SIZE`] squared,
    /// mip level 0. It repeats every `TILE_METERS` (4 m: one gameplay cell,
    /// aligned to the cells).
    pub fn texture(&self) -> Vec<u8> {
        let seed = name_hash(&self.terrain);
        let n = TEXTURE_SIZE;
        let mut out = Vec::with_capacity((n * n * 4) as usize);
        for y in 0..n {
            for x in 0..n {
                let (u, v) = (x as f32 / n as f32, y as f32 / n as f32);
                let rgb = self.texel(seed, u, v);
                out.extend_from_slice(&rgb);
                out.push(255);
            }
        }
        out
    }

    fn texel(&self, seed: u32, u: f32, v: f32) -> [u8; 3] {
        let coarse = tile_noise(seed, u, v, 4);
        let medium = tile_noise(seed ^ 1, u, v, 16);
        let fine = tile_noise(seed ^ 2, u, v, 64);
        let base = 0.45 * coarse + 0.35 * medium + 0.20 * fine;
        let mut color = self.color.map(f32::from);
        let t = match &self.pattern {
            Pattern::Noise => base,
            Pattern::Cracks => {
                let r = 1.0 - (2.0 * tile_noise(seed ^ 3, u, v, 8) - 1.0).abs();
                0.6 * base + 0.4 * r * r
            }
            Pattern::Ripples => {
                // Waves run along u; the whole number of periods keeps the tile
                // seamless. Thin bright crests on a calm base.
                let phase = v * 6.0 + 0.8 * (coarse - 0.5);
                let wave = 0.5 + 0.5 * (std::f32::consts::TAU * phase).sin();
                0.5 * (0.6 * coarse + 0.4 * medium) + 0.5 * wave * wave * wave
            }
            Pattern::Planks { count } => {
                let c = *count as f32;
                let (index, along) = ((v * c).floor(), (v * c).fract());
                // A dark gap between planks, a shade of its own per plank and
                // grain along it.
                let gap = if !(0.07..=0.93).contains(&along) {
                    0.62
                } else {
                    1.0
                };
                let shade = 0.80
                    + 0.40
                        * ((crate::terrain::hash(seed, index as u32, 7) >> 8) as f32
                            / 16_777_216.0);
                for ch in &mut color {
                    *ch *= gap * shade;
                }
                0.5 + 0.5 * (fine - 0.5)
            }
            Pattern::Flecks { color: fleck } => {
                let speck = tile_noise(seed ^ 5, u, v, 24);
                let k = ((speck - 0.64) / 0.06).clamp(0.0, 1.0);
                for (ch, f) in color.iter_mut().zip(fleck) {
                    *ch += (f32::from(*f) - *ch) * k;
                }
                base
            }
        };
        let k = 1.0 + (t - 0.5) * 2.0 * self.contrast;
        color.map(|c| (c * k).clamp(0.0, 255.0).round() as u8)
    }
}

/// Everything the terrain shader needs from a map, on the CPU: built by
/// [`terrain_surface`], uploaded by `terrain_gpu`.
#[derive(Clone, Debug)]
pub struct TerrainSurface {
    /// RGBA8 sRGB level-0 texture of every texture-array layer.
    pub layers: Vec<Vec<u8>>,
    /// Two bytes per texel (layer, style bits), row-major.
    pub index: Vec<u8>,
    /// Size of the index in texels.
    pub size: (u32, u32),
    /// Metres per index texel: one gameplay cell, or 1 m without a layer.
    pub index_meters: f32,
    /// Half the width of the blend ramp, in texels (`0.5` = the whole way from
    /// one texel centre to the next).
    pub ramp: f32,
    /// Wobble of the borders, in texels.
    pub wobble: f32,
    /// Shore line colour (linear) and strength.
    pub shore: [f32; 3],
    pub shore_amount: f32,
}

/// The surface of a map: from its terrain layer if it has one (the table of
/// terrain types and the looks decide), otherwise from its material map.
pub fn terrain_surface(map: &Map, types: &TerrainTypes, looks: &TerrainLooks) -> TerrainSurface {
    let edge = looks.edge;
    let Some(terrain) = map.terrain() else {
        // Without a terrain layer: the material of every 1 m sample, blended
        // over the full distance between sample centres, as WP1.7 did.
        return TerrainSurface {
            layers: map
                .materials()
                .iter()
                .map(|n| material_texture(n))
                .collect(),
            index: map.cells().iter().flat_map(|&m| [m, 0]).collect(),
            size: (map.width(), map.height()),
            index_meters: 1.0,
            ramp: 0.5,
            wobble: 0.0,
            shore: edge.shore,
            shore_amount: 0.0,
        };
    };
    let mut layer_of = [0_u8; 256];
    let mut style_of = [0_u8; 256];
    let mut layers = Vec::with_capacity(types.len());
    for (i, t) in types.iter().enumerate() {
        let look = looks.look_for(&t.name);
        layer_of[usize::from(t.id)] = i as u8;
        style_of[usize::from(t.id)] = look.style_bits();
        layers.push(look.texture());
    }
    let cell = sf_math::render_bridge::METRES_PER_CELL;
    TerrainSurface {
        layers,
        index: terrain
            .ids()
            .iter()
            .flat_map(|&id| [layer_of[usize::from(id)], style_of[usize::from(id)]])
            .collect(),
        size: (terrain.width(), terrain.height()),
        index_meters: cell,
        ramp: edge.ramp_m / cell * 0.5,
        wobble: edge.wobble_m / cell,
        shore: edge.shore,
        shore_amount: edge.shore_amount,
    }
}

#[cfg(test)]
mod tests;
