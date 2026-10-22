//! Terrain types of the gameplay grid (WP2.1).
//!
//! The visual terrain of a map ([`crate::map`]) is per 1 m sample; the game
//! rules need to know what every 4 m gameplay cell *is* (PLAN.md decision
//! B12): grass, road, water, rock, ore or bridge. This module has the two
//! halves of that:
//!
//! * [`TerrainTypes`]: the table of terrain types, game data in RON
//!   (`assets/config/terrain_types.ron`). Each type has a numeric `id` (what
//!   map files store), a `name` (what tools and the generator use) and flags:
//!   whether a building may stand on it and whether it can be harvested.
//!   Adding a type is a data change. Which movement classes can cross a type,
//!   and how fast, is not a property of the type: it is the cost table of the
//!   movement classes (`unit_classes.ron`, [`crate::movement`]).
//! * [`TerrainLayer`]: one type id per gameplay cell of one map, row-major.
//!
//! `docs/rules/terrain.md` says what the flags mean for the game. Nothing here
//! is simulation code: the simulation gets plain numbers handed over by
//! `sf_game` or `sf_headless` (through `sf_bridge`).
//!
//! ```
//! use sf_data::terrain::TerrainTypes;
//!
//! let types = TerrainTypes::embedded();
//! let water = types.id_of("water").unwrap();
//! assert!(!types.get(water).unwrap().buildable);
//! assert!(types.get(types.id_of("grass").unwrap()).unwrap().buildable);
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};

/// The shipped table, compiled in so a map can be loaded without knowing
/// where the assets are (the same way the terrain LOD settings are embedded).
/// The file is `assets/config/terrain_types.ron`.
pub const EMBEDDED_TERRAIN_TYPES: &str = include_str!("../../../assets/config/terrain_types.ron");

/// One terrain type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainType {
    /// The number a map file stores for a cell of this type. Unique.
    pub id: u8,
    /// A short unique name (`"grass"`), for tools and the generator.
    pub name: String,
    /// Whether a building (or any structure the sim `Place`s) may stand on it.
    pub buildable: bool,
    /// Whether harvesters can take resources from it.
    pub harvestable: bool,
    /// The flat colour of this type on the minimap (WP2.3), sRGB. Chosen so
    /// that every pair of types can be told apart at a few pixels per cell;
    /// a test on the shipped table holds that. Pure presentation: the sim never
    /// reads it, and it is not what the 3D terrain looks like (WP2.2 maps the
    /// types to materials on its own).
    pub minimap_colour: [u8; 3],
}

/// The RON file: a list of types.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TerrainTypesFile {
    types: Vec<TerrainType>,
}

/// Why a terrain type table was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerrainTypesError {
    /// The text is not valid RON for the table.
    Ron(String),
    /// The table lists no type.
    Empty,
    /// Two types have the same id.
    DuplicateId(u8),
    /// A name is empty or only white space.
    EmptyName(u8),
    /// Two types have the same name.
    DuplicateName(String),
}

impl fmt::Display for TerrainTypesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ron(e) => write!(f, "terrain types: {e}"),
            Self::Empty => write!(f, "terrain types: the table lists no type"),
            Self::DuplicateId(id) => write!(f, "terrain types: id {id} appears twice"),
            Self::EmptyName(id) => write!(f, "terrain types: type {id} has an empty name"),
            Self::DuplicateName(name) => {
                write!(f, "terrain types: the name \"{name}\" appears twice")
            }
        }
    }
}

impl std::error::Error for TerrainTypesError {}

/// A validated table of terrain types, sorted by id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainTypes {
    types: Vec<TerrainType>,
}

impl TerrainTypes {
    /// Parses and validates a table: at least one type, unique ids, unique
    /// non-empty names.
    pub fn from_ron(text: &str) -> Result<Self, TerrainTypesError> {
        let file: TerrainTypesFile =
            ron::from_str(text).map_err(|e| TerrainTypesError::Ron(e.to_string()))?;
        Self::new(file.types)
    }

    /// Validates a list of types (see [`TerrainTypes::from_ron`]) and sorts it
    /// by id.
    pub fn new(mut types: Vec<TerrainType>) -> Result<Self, TerrainTypesError> {
        if types.is_empty() {
            return Err(TerrainTypesError::Empty);
        }
        types.sort_by_key(|t| t.id);
        for (i, t) in types.iter().enumerate() {
            if t.name.trim().is_empty() {
                return Err(TerrainTypesError::EmptyName(t.id));
            }
            if i > 0 && types[i - 1].id == t.id {
                return Err(TerrainTypesError::DuplicateId(t.id));
            }
            if types[..i].iter().any(|other| other.name == t.name) {
                return Err(TerrainTypesError::DuplicateName(t.name.clone()));
            }
        }
        Ok(Self { types })
    }

    /// The shipped table ([`EMBEDDED_TERRAIN_TYPES`]).
    ///
    /// # Panics
    ///
    /// If the shipped file is invalid, which a test rules out.
    pub fn embedded() -> Self {
        Self::from_ron(EMBEDDED_TERRAIN_TYPES).expect("the embedded terrain_types.ron is valid")
    }

    /// The type with this id, if the table has it.
    pub fn get(&self, id: u8) -> Option<&TerrainType> {
        self.types
            .binary_search_by_key(&id, |t| t.id)
            .ok()
            .map(|i| &self.types[i])
    }

    /// The id of the type with this name.
    pub fn id_of(&self, name: &str) -> Option<u8> {
        self.types.iter().find(|t| t.name == name).map(|t| t.id)
    }

    /// Every type, ascending by id.
    pub fn iter(&self) -> impl Iterator<Item = &TerrainType> {
        self.types.iter()
    }

    /// The number of types.
    pub fn len(&self) -> usize {
        self.types.len()
    }

    /// Always false: a table has at least one type.
    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }
}

/// Why a terrain layer could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerrainLayerError {
    /// The id list does not have `width * height` entries.
    Size {
        /// Width in cells.
        width: u32,
        /// Height in cells.
        height: u32,
        /// How many ids were given.
        got: usize,
    },
}

impl fmt::Display for TerrainLayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Size { width, height, got } => write!(
                f,
                "terrain layer: expected {width}x{height} = {} cells, got {got}",
                u64::from(*width) * u64::from(*height)
            ),
        }
    }
}

impl std::error::Error for TerrainLayerError {}

/// The terrain type id of every gameplay cell of a map, row-major (`y` rows,
/// like the material map). Built by the generator or read from a map's
/// `terrain.png`; [`crate::map::Map::with_terrain`] checks it against the
/// map's size and the table of types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainLayer {
    width: u32,
    height: u32,
    ids: Vec<u8>,
}

impl TerrainLayer {
    /// A layer of `width` x `height` cells.
    pub fn new(width: u32, height: u32, ids: Vec<u8>) -> Result<Self, TerrainLayerError> {
        if ids.len() as u64 != u64::from(width) * u64::from(height) {
            return Err(TerrainLayerError::Size {
                width,
                height,
                got: ids.len(),
            });
        }
        Ok(Self { width, height, ids })
    }

    /// Width in gameplay cells.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in gameplay cells.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The type ids, `width * height`, row-major.
    pub fn ids(&self) -> &[u8] {
        &self.ids
    }

    /// The type id of cell `(x, y)`; `None` outside the layer.
    pub fn get(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.ids
            .get(y as usize * self.width as usize + x as usize)
            .copied()
    }

    /// How many cells have each id (index = id).
    pub fn histogram(&self) -> [u32; 256] {
        let mut counts = [0_u32; 256];
        for &id in &self.ids {
            counts[usize::from(id)] += 1;
        }
        counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ty(id: u8, name: &str) -> TerrainType {
        TerrainType {
            id,
            name: name.into(),
            buildable: true,
            harvestable: false,
            minimap_colour: [id.wrapping_mul(40), 90, 90],
        }
    }

    #[test]
    fn the_shipped_table_has_the_six_types_of_the_plan() {
        let types = TerrainTypes::embedded();
        let names: Vec<&str> = types.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["grass", "road", "water", "rock", "ore", "bridge"]);
        // Ids are what map files store: pinned.
        for (id, name) in [
            (0, "grass"),
            (1, "road"),
            (2, "water"),
            (3, "rock"),
            (4, "ore"),
            (5, "bridge"),
        ] {
            assert_eq!(types.id_of(name), Some(id), "{name}");
            assert_eq!(types.get(id).unwrap().name, name);
        }
        assert_eq!(types.get(6), None);
        assert_eq!(types.id_of("lava"), None);
        assert!(!types.is_empty());
        assert_eq!(types.len(), 6);
    }

    /// The minimap shows each type as a flat colour (WP2.3): two types that
    /// look alike would be unreadable at a few pixels per cell, with the light
    /// shading of the relief on top.
    #[test]
    fn the_shipped_minimap_colours_are_clearly_different() {
        let types = TerrainTypes::embedded();
        let all: Vec<&TerrainType> = types.iter().collect();
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                let d2: i32 = a
                    .minimap_colour
                    .iter()
                    .zip(&b.minimap_colour)
                    .map(|(x, y)| (i32::from(*x) - i32::from(*y)).pow(2))
                    .sum();
                assert!(
                    d2 >= 50 * 50,
                    "{} and {} are too close on the minimap (distance {:.0})",
                    a.name,
                    b.name,
                    f64::from(d2).sqrt()
                );
            }
        }
    }

    #[test]
    fn the_shipped_flags_make_sense() {
        let types = TerrainTypes::embedded();
        let t = |name: &str| types.get(types.id_of(name).unwrap()).unwrap().clone();
        // Only grass is buildable; only ore is harvestable.
        for (name, buildable) in [
            ("grass", true),
            ("road", false),
            ("water", false),
            ("rock", false),
            ("ore", false),
            ("bridge", false),
        ] {
            assert_eq!(t(name).buildable, buildable, "{name}");
        }
        assert!(t("ore").harvestable);
        assert_eq!(types.iter().filter(|t| t.harvestable).count(), 1);
    }

    #[test]
    fn a_table_with_a_problem_is_refused_with_a_clear_message() {
        let err = |types: Vec<TerrainType>| TerrainTypes::new(types).unwrap_err();
        assert_eq!(err(vec![]), TerrainTypesError::Empty);
        assert_eq!(
            err(vec![ty(1, "a"), ty(1, "b")]),
            TerrainTypesError::DuplicateId(1)
        );
        assert_eq!(
            err(vec![ty(1, "a"), ty(2, "a")]),
            TerrainTypesError::DuplicateName("a".into())
        );
        assert_eq!(err(vec![ty(7, "  ")]), TerrainTypesError::EmptyName(7));
        assert!(
            err(vec![ty(1, "a"), ty(1, "b")])
                .to_string()
                .contains("id 1")
        );
        // Not RON, or an unknown field.
        assert!(matches!(
            TerrainTypes::from_ron("(types: [)"),
            Err(TerrainTypesError::Ron(_))
        ));
        let extra = "(types: [(id: 0, name: \"a\", buildable: true, harvestable: false, \
                     speed: 3)])";
        // The placeholder passable flags of WP2.1 are gone: the movement
        // classes carry the costs.
        let old = "(types: [(id: 0, name: \"a\", passable: (foot: true, wheeled: true, \
                   tracked: true), buildable: true, harvestable: false)])";
        assert!(matches!(
            TerrainTypes::from_ron(old),
            Err(TerrainTypesError::Ron(_))
        ));
        assert!(matches!(
            TerrainTypes::from_ron(extra),
            Err(TerrainTypesError::Ron(_))
        ));
        // A flag that is left out is an error too: no silent default.
        let missing = "(types: [(id: 0, name: \"a\", buildable: true)])";
        assert!(matches!(
            TerrainTypes::from_ron(missing),
            Err(TerrainTypesError::Ron(_))
        ));
    }

    #[test]
    fn types_are_sorted_by_id_whatever_the_file_order() {
        let types = TerrainTypes::new(vec![ty(9, "z"), ty(2, "b"), ty(5, "m")]).unwrap();
        let ids: Vec<u8> = types.iter().map(|t| t.id).collect();
        assert_eq!(ids, [2, 5, 9]);
        assert_eq!(types.get(5).unwrap().name, "m");
        assert_eq!(types.id_of("z"), Some(9));
    }

    #[test]
    fn a_layer_checks_its_size_and_answers_per_cell() {
        let layer = TerrainLayer::new(3, 2, vec![0, 1, 2, 3, 4, 5]).unwrap();
        assert_eq!((layer.width(), layer.height()), (3, 2));
        assert_eq!(layer.get(0, 0), Some(0));
        assert_eq!(layer.get(2, 0), Some(2));
        assert_eq!(layer.get(0, 1), Some(3));
        assert_eq!(layer.get(2, 1), Some(5));
        assert_eq!(layer.get(3, 0), None);
        assert_eq!(layer.get(0, 2), None);
        let h = layer.histogram();
        assert_eq!(&h[..7], [1, 1, 1, 1, 1, 1, 0]);
        let err = TerrainLayer::new(3, 2, vec![0; 5]).unwrap_err();
        assert!(err.to_string().contains("3x2 = 6"), "{err}");
    }
}
