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
    /// The flat colour of the type on the minimap (WP2.3), sRGB, when it
    /// should differ from `color`: the minimap shows one colour per cell, so a
    /// look whose texture is far from its base colour (ore: dark ground with
    /// bright flecks) names its average here. Without it the minimap uses
    /// `color`. A test keeps it near the mean of the texture.
    #[serde(default)]
    pub minimap: Option<[u8; 3]>,
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

/// How a resource cell looks at one fill level (WP7.1): the ore cell is drawn from the first
/// level whose `up_to_percent` is at least the cell's fill, so a field shows what is left in it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceLevel {
    /// The highest fill, in percent of the cell maximum, this level shows. 0 is an empty cell;
    /// the last level is 100.
    pub up_to_percent: u8,
    /// Base colour of the ground at this level, sRGB.
    pub color: [u8; 3],
    /// How many of the bright flecks of the look's `Flecks` pattern are drawn, in percent
    /// (0: none, 100: as the look itself).
    pub flecks_percent: u8,
    /// The flat colour of a cell of this level on the minimap, sRGB.
    pub minimap: [u8; 3],
}

/// The looks of a resource terrain type by fill level (WP7.1, `docs/rules/economy.md`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceLooks {
    /// The terrain type (by name) whose cells show their fill. Its own look has the
    /// `Flecks` pattern.
    pub terrain: String,
    /// From empty to full.
    pub levels: Vec<ResourceLevel>,
}

impl ResourceLooks {
    /// The level a cell shows when it holds `amount` of at most `max`: the first level whose
    /// `up_to_percent` is at least the fill (a non-empty cell shows at least 1 percent).
    pub fn level_of(&self, amount: u16, max: u16) -> usize {
        let max = u32::from(max.max(1));
        let percent = if amount == 0 {
            0
        } else {
            (u32::from(amount) * 100 / max).max(1)
        };
        self.levels
            .iter()
            .position(|l| u32::from(l.up_to_percent) >= percent)
            .unwrap_or(self.levels.len() - 1)
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LookFile {
    edge: Edge,
    looks: Vec<TerrainLook>,
    #[serde(default)]
    resource: Option<ResourceLooks>,
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
    resource: Option<ResourceLooks>,
}

impl TerrainLooks {
    /// Parse and validate: unique names, `contrast` in `0..=1`, a ramp of at
    /// most one cell, a wobble of at most one metre and a half, plank counts
    /// 1 to 64, shore colour and strength `0..=1`. Whether the names are real
    /// terrain types is [`TerrainLooks::check_names`], against the table.
    pub fn from_ron(text: &str) -> Result<Self, LookError> {
        // `minimap: (r, g, b)` without `Some(...)`: implicit options.
        let file: LookFile = ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .from_str(text)
            .map_err(|e| LookError(e.to_string()))?;
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
        if !e.shore.iter().all(|c| (0.0..=1.0).contains(c)) {
            return Err(LookError("shore must be a linear colour, 0..=1".into()));
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
        if let Some(r) = &file.resource {
            let ls = &r.levels;
            let ok = (1..=8).contains(&ls.len())
                && ls[0].up_to_percent == 0
                && ls.last().is_some_and(|l| l.up_to_percent == 100)
                && ls
                    .windows(2)
                    .all(|w| w[0].up_to_percent < w[1].up_to_percent)
                && ls.iter().all(|l| l.flecks_percent <= 100);
            if !ok {
                return Err(LookError(format!(
                    "{}: resource levels must be 1 to 8, start at up_to_percent 0, end at 100, \
                     rise strictly, with flecks_percent at most 100",
                    r.terrain
                )));
            }
            let flecks = file
                .looks
                .iter()
                .any(|l| l.terrain == r.terrain && matches!(l.pattern, Pattern::Flecks { .. }));
            if !flecks {
                return Err(LookError(format!(
                    "{}: a resource needs a look with the Flecks pattern",
                    r.terrain
                )));
            }
        }
        Ok(Self {
            edge: e,
            looks: file.looks,
            resource: file.resource,
        })
    }

    /// The fill levels of the resource terrain type, if the file has them.
    pub fn resource(&self) -> Option<&ResourceLooks> {
        self.resource.as_ref()
    }

    /// Every entry names a type of `types`: a misspelt name would silently
    /// leave its type in the derived colour.
    pub fn check_names(&self, types: &TerrainTypes) -> Result<(), LookError> {
        if let Some(r) = &self.resource
            && types.id_of(&r.terrain).is_none()
        {
            return Err(LookError(format!("{}: no such terrain type", r.terrain)));
        }
        match self
            .looks
            .iter()
            .find(|l| types.id_of(&l.terrain).is_none())
        {
            Some(l) => Err(LookError(format!("{}: no such terrain type", l.terrain))),
            None => Ok(()),
        }
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
                minimap: None,
            }
        })
    }
}

impl TerrainLook {
    /// The flat colour of this type on the minimap: `minimap` if the look
    /// names one, otherwise its base `color`. The 3D look is the one source
    /// of a type's colour (WP2.3); the gameplay table has none.
    pub fn minimap_color(&self) -> [u8; 3] {
        self.minimap.unwrap_or(self.color)
    }

    /// The style bits of the cell index.
    pub fn style_bits(&self) -> u8 {
        (if self.flat { STYLE_FLAT } else { 0 }) | (if self.liquid { STYLE_LIQUID } else { 0 })
    }

    /// RGBA8 sRGB pixels of the tileable texture, [`TEXTURE_SIZE`] squared,
    /// mip level 0. It repeats every `TILE_METERS` (4 m: one gameplay cell,
    /// aligned to the cells).
    pub fn texture(&self) -> Vec<u8> {
        self.texture_with_flecks(100)
    }

    /// The texture of a fill level of a resource look (WP7.1): the look with `level`'s ground
    /// colour and its share of flecks. The fleck pattern is the same, so the flecks that remain
    /// stay where they were.
    pub fn level_texture(&self, level: &ResourceLevel) -> Vec<u8> {
        let mut look = self.clone();
        look.color = level.color;
        look.texture_with_flecks(u32::from(level.flecks_percent))
    }

    fn texture_with_flecks(&self, fleck_percent: u32) -> Vec<u8> {
        let seed = name_hash(&self.terrain);
        let n = TEXTURE_SIZE;
        let mut out = Vec::with_capacity((n * n * 4) as usize);
        for y in 0..n {
            for x in 0..n {
                let (u, v) = (x as f32 / n as f32, y as f32 / n as f32);
                let rgb = self.texel(seed, u, v, fleck_percent);
                out.extend_from_slice(&rgb);
                out.push(255);
            }
        }
        out
    }

    fn texel(&self, seed: u32, u: f32, v: f32, fleck_percent: u32) -> [u8; 3] {
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
                // 100 percent is the threshold of the look itself (0.64); fewer flecks raise it.
                let threshold = 1.0 - 0.36 * fleck_percent as f32 / 100.0;
                let k = ((speck - threshold) / 0.06).clamp(0.0, 1.0);
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
    /// The resource type's fill levels (WP7.1): the texture-array layer of the first level (the
    /// others follow it), the terrain type id whose cells they replace and the style bits of
    /// that type. `None` without a terrain layer or without `resource` in the looks.
    pub resource: Option<ResourceLayers>,
}

/// Where the fill levels of the resource type are in the texture array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceLayers {
    /// The terrain type id of the resource (`ore`).
    pub type_id: u8,
    /// The layer of level 0; level `n` is `first_layer + n`.
    pub first_layer: u8,
    /// The style bits of the type, kept in the index texel.
    pub style: u8,
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
            resource: None,
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
    // The fill levels of the resource type: extra layers after the types'.
    let resource = looks.resource().and_then(|r| {
        let type_id = types.id_of(&r.terrain)?;
        let look = looks.look_for(&r.terrain);
        let first_layer = layers.len() as u8;
        for level in &r.levels {
            layers.push(look.level_texture(level));
        }
        Some(ResourceLayers {
            type_id,
            first_layer,
            style: look.style_bits(),
        })
    });
    debug_assert!(
        terrain.ids().iter().all(|&id| types.get(id).is_some()),
        "the terrain layer holds an id that is not in the table of terrain types"
    );
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
        resource,
    }
}

#[cfg(test)]
mod tests;
