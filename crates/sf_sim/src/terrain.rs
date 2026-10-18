//! Terrain type and occupancy per cell (WP2.1).
//!
//! The playfield is a grid of gameplay cells (PLAN.md decision B12: one cell
//! is 4 m, `Fx::ONE` of position is one cell). Every cell has a **terrain
//! type**, fixed when the world is created, and an **occupant**, the entity
//! that stands on it, if any.
//!
//! # Terrain
//!
//! [`Terrain`] is one byte per cell, row-major (`y * width + x`), plus a table
//! of up to 256 type definitions ([`TerrainDef`]): the id of a type and its
//! flags ([`TerrainFlags`]: whether a structure may stand on it, whether it can
//! be harvested). Which movement classes cross a type, and how fast, is the
//! movement table's ([`crate::movement`], WP4.1), not the type's. The
//! simulation knows ids and flags only, never names; the names, and the RON
//! file the table comes from, belong to `sf_data`. `sf_game` and `sf_headless` read the
//! map and hand the numbers to [`World::with_terrain`](crate::World::with_terrain),
//! so the simulation does no I/O.
//!
//! A world made with [`World::new`](crate::World::new) has no terrain layer:
//! every cell is type 0 with open flags ([`Terrain::open`]). That is what the
//! wanderer scenarios run on, and it is the reason the checksum of such a world
//! has not changed (see [`crate::checksum`]).
//!
//! The terrain never changes after creation, so the world computes a digest of
//! it once ([`Terrain::digest`]) and the per-tick checksum folds that one
//! number in. (Terrain that changes during a game, a destroyed bridge say,
//! would be a command that rewrites a cell and the digest.)
//!
//! # Occupancy
//!
//! [`Occupancy`] answers "who is on this cell". One `u32` per cell holds the
//! slot index of the occupant, or [`NO_OCCUPANT`]; the generation is not stored
//! because an occupant is always alive (despawning frees the cell in the same
//! command), so it is read from the entity columns when asked. The layout is
//! fixed by the map size (row-major, allocated once), never a map keyed by
//! hash, and the checksum visits the cells in ascending order.
//!
//! Only the tick changes it (quality rule 1): the cell is claimed when a
//! [`Command::Place`](crate::Command::Place) spawns a structure and released
//! when that entity is despawned. Everything public here is a read-only query.
//! **Wanderers do not occupy cells**: thousands of them share a cell, and
//! collision comes in phase four. Only entities created by `Place` do.

use core::fmt;

use sf_math::hash::Fnv1a64;

/// A gameplay cell, `x` east and `y` south, both counted from zero.
#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Cell {
    /// Column, `0..width`.
    pub x: u32,
    /// Row, `0..height`.
    pub y: u32,
}

impl Cell {
    /// The cell in column `x`, row `y`.
    pub const fn new(x: u32, y: u32) -> Cell {
        Cell { x, y }
    }
}

impl fmt::Display for Cell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

/// The id of a terrain type, as a map file stores it. The simulation attaches
/// no meaning to the number beyond its flags; `sf_data` names them.
#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct TerrainId(pub u8);

/// What a terrain type allows. `buildable` is read by
/// [`Command::Place`](crate::Command::Place) and
/// [`Command::PlaceStructure`](crate::Command::PlaceStructure), `harvestable`
/// by nothing yet (phase 7). Movement is not a flag: see [`crate::movement`].
#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
pub struct TerrainFlags {
    /// A structure may stand on it.
    pub buildable: bool,
    /// Harvesters can take resources from it.
    pub harvestable: bool,
    /// Open ground: all three ground movement classes (foot, wheeled, tracked)
    /// can enter it. A summary **derived from the movement costs** by whoever
    /// builds the definitions (`sf_bridge::terrain_defs`), never written by
    /// hand, and not read by the movement rules, which use the movement table.
    /// It is what a placement helper wants ("a cell any ground unit may be put
    /// on") and it is part of the terrain digest, in the three bits that the
    /// three passable flags of WP2.1 used, so the checksums pinned before
    /// WP4.1 stay valid.
    pub ground: bool,
}

impl TerrainFlags {
    /// Open ground: ground units can enter it, buildable, not harvestable.
    pub const OPEN: TerrainFlags = TerrainFlags {
        buildable: true,
        harvestable: false,
        ground: true,
    };

    /// The flags as one byte for the digest. The layout is the one WP2.1
    /// pinned: bits 0 to 2 were foot, wheeled and tracked (all three are now
    /// `ground`), bit 3 buildable, bit 4 harvestable.
    const fn bits(self) -> u8 {
        ((self.ground as u8) * 0b111)
            | ((self.buildable as u8) << 3)
            | ((self.harvestable as u8) << 4)
    }
}

/// One entry of the table of terrain types.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct TerrainDef {
    /// The id the layer uses for this type.
    pub id: TerrainId,
    /// What the type allows.
    pub flags: TerrainFlags,
}

/// Why a terrain layer was refused.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TerrainError {
    /// The number of ids is not `width * height`.
    Size,
    /// The definitions list the same id twice.
    DuplicateDef(TerrainId),
    /// A cell has an id that has no definition.
    UnknownId {
        /// The first such cell, row-major.
        cell: Cell,
        /// Its id.
        id: TerrainId,
    },
}

impl fmt::Display for TerrainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TerrainError::Size => f.write_str("the number of terrain ids is not width * height"),
            TerrainError::DuplicateDef(id) => {
                write!(f, "terrain type {} is defined twice", id.0)
            }
            TerrainError::UnknownId { cell, id } => {
                write!(
                    f,
                    "cell {cell} has terrain type {}, which is not defined",
                    id.0
                )
            }
        }
    }
}

impl std::error::Error for TerrainError {}

/// Written first into the digest; bump when its layout changes.
const DIGEST_TAG: &[u8] = b"sf_sim/terrain/v1";

/// The terrain type of every cell, with the flags of each type. Immutable once
/// built. See the module documentation.
#[derive(Clone, PartialEq, Eq)]
pub struct Terrain {
    width: u32,
    height: u32,
    /// `width * height` type ids, row-major.
    ids: Box<[u8]>,
    /// The definition of each id (`None` if undefined).
    flags: [Option<TerrainFlags>; 256],
    /// False for [`Terrain::open`]: no layer was given.
    layered: bool,
    /// Hash of everything above; zero without a layer.
    digest: u64,
}

impl fmt::Debug for Terrain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Terrain")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("layered", &self.layered)
            .field("digest", &format_args!("{:#018x}", self.digest))
            .finish()
    }
}

impl Terrain {
    /// A map with no terrain layer: every cell is type 0, open ground
    /// ([`TerrainFlags::OPEN`]). What [`World::new`](crate::World::new) uses.
    pub fn open(width: u32, height: u32) -> Terrain {
        let mut flags = [None; 256];
        flags[0] = Some(TerrainFlags::OPEN);
        Terrain {
            width,
            height,
            ids: vec![0; width as usize * height as usize].into_boxed_slice(),
            flags,
            layered: false,
            digest: 0,
        }
    }

    /// A terrain layer: `ids` is `width * height` type ids, row-major, and
    /// `defs` defines every id that occurs in it.
    pub fn from_layer(
        width: u32,
        height: u32,
        ids: &[u8],
        defs: &[TerrainDef],
    ) -> Result<Terrain, TerrainError> {
        if ids.len() as u64 != u64::from(width) * u64::from(height) {
            return Err(TerrainError::Size);
        }
        let mut flags = [None; 256];
        for d in defs {
            let slot = &mut flags[usize::from(d.id.0)];
            if slot.is_some() {
                return Err(TerrainError::DuplicateDef(d.id));
            }
            *slot = Some(d.flags);
        }
        if let Some(i) = ids.iter().position(|&id| flags[usize::from(id)].is_none()) {
            return Err(TerrainError::UnknownId {
                cell: Cell::new(i as u32 % width, i as u32 / width),
                id: TerrainId(ids[i]),
            });
        }
        let mut h = Fnv1a64::new();
        h.write_bytes(DIGEST_TAG);
        h.write_u32(width);
        h.write_u32(height);
        // The definitions in ascending id order, then every cell.
        for (id, f) in flags.iter().enumerate() {
            if let Some(f) = f {
                h.write_u8(id as u8);
                h.write_u8(f.bits());
            }
        }
        h.write_u64(ids.len() as u64);
        h.write_bytes(ids);
        Ok(Terrain {
            width,
            height,
            ids: ids.into(),
            flags,
            layered: true,
            digest: h.finish(),
        })
    }

    /// Width in cells.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in cells.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// True if the terrain came from a layer, false for [`Terrain::open`].
    pub fn has_layer(&self) -> bool {
        self.layered
    }

    /// A hash of the size, the definitions and every cell; zero without a
    /// layer. Part of the world checksum.
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// The type ids, `width * height`, row-major.
    pub fn ids(&self) -> &[u8] {
        &self.ids
    }

    fn index(&self, cell: Cell) -> Option<usize> {
        (cell.x < self.width && cell.y < self.height)
            .then(|| cell.y as usize * self.width as usize + cell.x as usize)
    }

    /// The type of `cell`; `None` outside the map.
    pub fn id_at(&self, cell: Cell) -> Option<TerrainId> {
        self.index(cell).map(|i| TerrainId(self.ids[i]))
    }

    /// The ids of the terrain types the map defines, ascending.
    pub fn defined_ids(&self) -> impl Iterator<Item = TerrainId> + Clone + '_ {
        self.flags
            .iter()
            .enumerate()
            .filter(|(_, f)| f.is_some())
            .map(|(id, _)| TerrainId(id as u8))
    }

    /// What `id` allows; `None` for an id the map does not define.
    pub fn flags_of(&self, id: TerrainId) -> Option<TerrainFlags> {
        self.flags[usize::from(id.0)]
    }

    /// What the terrain of `cell` allows; `None` outside the map.
    pub fn flags_at(&self, cell: Cell) -> Option<TerrainFlags> {
        self.id_at(cell).and_then(|id| self.flags_of(id))
    }
}

/// The slot value of a cell nobody occupies.
pub const NO_OCCUPANT: u32 = u32::MAX;

/// The entity column value of an entity that occupies no cell.
pub(crate) const NO_CELL: u32 = u32::MAX;

/// Who stands on each cell. Allocated once for the map size; changed only by
/// the tick (through the world's commands). See the module documentation.
#[derive(Clone, PartialEq, Eq)]
pub struct Occupancy {
    width: u32,
    height: u32,
    /// Slot index of the occupant of each cell, or [`NO_OCCUPANT`].
    slots: Box<[u32]>,
    /// How many cells are occupied.
    count: u32,
}

impl fmt::Debug for Occupancy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Occupancy")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("occupied", &self.count)
            .finish()
    }
}

impl Occupancy {
    pub(crate) fn new(width: u32, height: u32) -> Occupancy {
        Occupancy {
            width,
            height,
            slots: vec![NO_OCCUPANT; width as usize * height as usize].into_boxed_slice(),
            count: 0,
        }
    }

    /// The row-major index of `cell`, if it is inside the map.
    pub(crate) fn index(&self, cell: Cell) -> Option<u32> {
        (cell.x < self.width && cell.y < self.height).then(|| cell.y * self.width + cell.x)
    }

    /// The cell at a row-major index.
    pub(crate) fn cell(&self, index: u32) -> Cell {
        Cell::new(index % self.width, index / self.width)
    }

    /// The slot index of the occupant of the cell at `index`.
    pub(crate) fn slot_at(&self, index: u32) -> Option<u32> {
        match self.slots.get(index as usize) {
            Some(&s) if s != NO_OCCUPANT => Some(s),
            _ => None,
        }
    }

    /// Claims the cell at `index` for `slot`. False (and no change) if it is
    /// occupied or outside the map.
    pub(crate) fn claim(&mut self, index: u32, slot: u32) -> bool {
        match self.slots.get_mut(index as usize) {
            Some(s) if *s == NO_OCCUPANT => {
                *s = slot;
                self.count += 1;
                true
            }
            _ => false,
        }
    }

    /// Frees the cell at `index` (a no-op if it is free).
    pub(crate) fn release(&mut self, index: u32) {
        if let Some(s) = self.slots.get_mut(index as usize)
            && *s != NO_OCCUPANT
        {
            *s = NO_OCCUPANT;
            self.count -= 1;
        }
    }

    /// How many cells are occupied.
    pub fn count(&self) -> usize {
        self.count as usize
    }

    /// The occupied cells in ascending (row-major) order, with the slot index
    /// of each occupant.
    pub(crate) fn occupied(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter(|&(_, &s)| s != NO_OCCUPANT)
            .map(|(i, &s)| (i as u32, s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRASS: TerrainDef = TerrainDef {
        id: TerrainId(0),
        flags: TerrainFlags::OPEN,
    };
    const WATER: TerrainDef = TerrainDef {
        id: TerrainId(2),
        flags: TerrainFlags {
            buildable: false,
            harvestable: false,
            ground: false,
        },
    };

    #[test]
    fn open_terrain_is_type_zero_everywhere() {
        let t = Terrain::open(3, 2);
        assert!(!t.has_layer());
        assert_eq!(t.digest(), 0);
        assert_eq!(t.ids(), [0; 6]);
        assert_eq!(t.id_at(Cell::new(2, 1)), Some(TerrainId(0)));
        assert_eq!(t.flags_at(Cell::new(0, 0)), Some(TerrainFlags::OPEN));
        assert_eq!(t.id_at(Cell::new(3, 0)), None);
        assert_eq!(t.flags_at(Cell::new(0, 2)), None);
    }

    #[test]
    fn a_layer_answers_per_cell_and_checks_its_input() {
        let t = Terrain::from_layer(3, 2, &[0, 2, 0, 2, 2, 0], &[GRASS, WATER]).unwrap();
        assert!(t.has_layer());
        assert_ne!(t.digest(), 0);
        assert_eq!(t.id_at(Cell::new(1, 0)), Some(TerrainId(2)));
        assert_eq!(t.id_at(Cell::new(0, 1)), Some(TerrainId(2)));
        assert_eq!(t.id_at(Cell::new(2, 1)), Some(TerrainId(0)));
        assert!(!t.flags_at(Cell::new(1, 0)).unwrap().buildable);
        assert!(t.flags_at(Cell::new(0, 0)).unwrap().buildable);
        assert_eq!(t.flags_of(TerrainId(7)), None);

        assert_eq!(
            Terrain::from_layer(3, 2, &[0; 5], &[GRASS]),
            Err(TerrainError::Size)
        );
        assert_eq!(
            Terrain::from_layer(3, 2, &[0; 6], &[GRASS, GRASS]),
            Err(TerrainError::DuplicateDef(TerrainId(0)))
        );
        assert_eq!(
            Terrain::from_layer(3, 2, &[0, 0, 0, 0, 9, 0], &[GRASS]),
            Err(TerrainError::UnknownId {
                cell: Cell::new(1, 1),
                id: TerrainId(9)
            })
        );
    }

    #[test]
    fn the_digest_covers_the_size_the_cells_and_the_flags() {
        let base = Terrain::from_layer(2, 2, &[0, 0, 2, 0], &[GRASS, WATER]).unwrap();
        let same = Terrain::from_layer(2, 2, &[0, 0, 2, 0], &[WATER, GRASS]).unwrap();
        // The order of the definitions does not matter.
        assert_eq!(base.digest(), same.digest());
        let moved = Terrain::from_layer(2, 2, &[0, 2, 0, 0], &[GRASS, WATER]).unwrap();
        assert_ne!(base.digest(), moved.digest());
        let wide = Terrain::from_layer(4, 1, &[0, 0, 2, 0], &[GRASS, WATER]).unwrap();
        assert_ne!(base.digest(), wide.digest());
        let mut other_flags = WATER;
        other_flags.flags.ground = true;
        let flagged = Terrain::from_layer(2, 2, &[0, 0, 2, 0], &[GRASS, other_flags]).unwrap();
        assert_ne!(base.digest(), flagged.digest());
    }

    #[test]
    fn occupancy_claims_and_releases_cells() {
        let mut o = Occupancy::new(4, 3);
        assert_eq!(o.count(), 0);
        let i = o.index(Cell::new(2, 1)).unwrap();
        assert_eq!(i, 6);
        assert_eq!(o.cell(i), Cell::new(2, 1));
        assert_eq!(o.index(Cell::new(4, 0)), None);
        assert_eq!(o.index(Cell::new(0, 3)), None);
        assert!(o.claim(i, 5));
        assert!(!o.claim(i, 6), "an occupied cell cannot be claimed");
        assert_eq!(o.slot_at(i), Some(5));
        assert_eq!(o.count(), 1);
        assert!(o.claim(0, 1));
        let listed: Vec<(u32, u32)> = o.occupied().collect();
        assert_eq!(listed, [(0, 1), (6, 5)], "ascending cell order");
        o.release(i);
        o.release(i);
        assert_eq!(o.slot_at(i), None);
        assert_eq!(o.count(), 1);
        assert!(!o.claim(99, 1), "outside the map");
    }
}
