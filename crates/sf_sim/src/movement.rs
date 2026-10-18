//! Terrain costs per movement class (WP4.1).
//!
//! A [`MovementTable`] says, for every movement class and terrain type, whether
//! a unit of that class can enter a cell of that type and how fast it moves
//! there: a [`Cost`], either [`Cost::Impassable`] or a speed multiplier in
//! thousandths of the unit's own speed ([`Cost::Speed`]; `1000` is normal).
//! `sf_data` reads and checks the table (`unit_classes.ron`, every class names
//! every terrain type) and `sf_bridge` copies it in here, the way the unit
//! table arrives ([`World::with_movement`](crate::World::with_movement)). The
//! simulation knows classes by their permanent `kind` number and terrain types
//! by their id, never by name.
//!
//! # The queries
//!
//! [`World::cost`](crate::World::cost) and [`World::passable`](crate::World::passable)
//! answer for one class and one cell. They are read-only and the same function
//! the movement itself uses ([`Ground`]), so what the cursor shows and what the
//! unit does cannot disagree. The answer, in this order:
//!
//! * (1) a cell outside the map: impassable;
//! * (2) a world without a terrain layer: open ground, [`Cost::OPEN`], for every
//!   class (what the wanderer scenarios and the early unit scenarios run on);
//! * (3) a world with a layer but no movement table: the same, open ground;
//! * (4) a class the table does not have: open ground (a class that is not
//!   understood is never blocked; the bridge checks that every unit's class
//!   is in the table);
//! * (5) otherwise the table's entry for the class and the cell's terrain type.
//!
//! # Moving by it
//!
//! A unit made from the unit table and having a movement class moves at its own
//! speed per tick times the multiplier of the cell **it stands in**
//! ([`scale_step`]), and a step into another cell happens only if that cell is
//! passable for the class (and, for a diagonal step, so are the two cells it
//! cuts across, so a unit cannot slip through the corner between two blocked
//! cells). A refused step leaves the unit where it is, still facing its
//! waypoint, with its order kept: it stands at the edge. A unit that walks an
//! order is routed round such cells (WP4.2, [`crate::path`]); the legacy `MoveTo`
//! stands at the edge. Wanderers and anything else without a movement class
//! ignore the terrain, as before.

use core::fmt;

use sf_math::Fx;
use sf_math::hash::Fnv1a64;

use crate::terrain::{Cell, Occupancy, Terrain, TerrainId};
use crate::turning::{Turning, TurningError};

/// Written first into the digest; bump when its layout changes.
const DIGEST_TAG: &[u8] = b"sf_sim/movement/v1";

/// Written before the turning profiles in the digest.
const TURNING_TAG: &[u8] = b"sf_sim/movement/turning/v1";

/// The multiplier of normal speed: `1000` thousandths.
pub const FULL_PERMILLE: u16 = 1000;

/// A unit never moves more than this many cells in one tick on terrain, so a
/// step reaches at most the next cell and no cell is ever jumped over (an
/// eighth under one cell: seventeen and a half cells per second).
pub const MAX_STEP: Fx = Fx::from_ratio(7, 8);

/// What a movement class does on one terrain type.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Cost {
    /// The class cannot enter a cell of this type.
    Impassable,
    /// The class moves at this many thousandths of its own speed (at least 1).
    Speed(u16),
}

impl Cost {
    /// Normal speed: what open ground costs every class.
    pub const OPEN: Cost = Cost::Speed(FULL_PERMILLE);

    /// Whether a unit may enter.
    pub const fn is_passable(self) -> bool {
        matches!(self, Cost::Speed(_))
    }

    /// The multiplier in thousandths; zero for [`Cost::Impassable`].
    pub const fn permille(self) -> u16 {
        match self {
            Cost::Impassable => 0,
            Cost::Speed(p) => p,
        }
    }
}

/// The costs of one movement class, as handed to [`MovementTable::new`]: one
/// entry per terrain type.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ClassCosts {
    /// The permanent kind number of the movement class.
    pub class: u16,
    /// The cost on each terrain type; every type of the map has to be listed.
    pub costs: Vec<(TerrainId, Cost)>,
}

/// Why a movement table was refused.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum MovementTableError {
    /// Two entries have the same class.
    DuplicateClass(u16),
    /// A class lists the same terrain type twice.
    DuplicateTerrain {
        /// The class.
        class: u16,
        /// The terrain type.
        terrain: TerrainId,
    },
    /// A class has `Speed(0)`; a cell that cannot be entered is `Impassable`.
    ZeroSpeed {
        /// The class.
        class: u16,
        /// The terrain type.
        terrain: TerrainId,
    },
    /// A turning profile for a class the table does not have (WP4.5).
    UnknownTurningClass(u16),
    /// Two turning profiles for the same class.
    DuplicateTurning(u16),
    /// A turning profile whose numbers do not hang together.
    BadTurning {
        /// The class.
        class: u16,
        /// What is wrong.
        problem: TurningError,
    },
}

impl fmt::Display for MovementTableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateClass(class) => write!(f, "movement class {class} is listed twice"),
            Self::DuplicateTerrain { class, terrain } => write!(
                f,
                "movement class {class} lists terrain type {} twice",
                terrain.0
            ),
            Self::ZeroSpeed { class, terrain } => write!(
                f,
                "movement class {class} has speed 0 on terrain type {} (use Impassable)",
                terrain.0
            ),
            Self::UnknownTurningClass(class) => {
                write!(
                    f,
                    "a turning profile for movement class {class}, which the table does not have"
                )
            }
            Self::DuplicateTurning(class) => {
                write!(f, "movement class {class} has two turning profiles")
            }
            Self::BadTurning { class, problem } => {
                write!(f, "movement class {class}: turning: {problem}")
            }
        }
    }
}

impl std::error::Error for MovementTableError {}

/// One class: its costs indexed by terrain id; `None` where it lists nothing.
#[derive(Clone, PartialEq, Eq)]
struct Entry {
    class: u16,
    costs: Box<[Option<Cost>; 256]>,
    /// How the class turns its hull (WP4.5); [`Turning::LEGACY`] unless given.
    turning: Turning,
}

/// The cost of every movement class on every terrain type. Immutable once
/// built; an empty table is a world without movement classes (open ground for
/// everyone).
#[derive(Clone, PartialEq, Eq, Default)]
pub struct MovementTable {
    /// Sorted by class, so a lookup is a binary search and the order the caller
    /// listed them in never matters.
    entries: Vec<Entry>,
    /// Hash of everything above; zero when empty.
    digest: u64,
}

impl fmt::Debug for MovementTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MovementTable")
            .field("classes", &self.entries.len())
            .field("digest", &format_args!("{:#018x}", self.digest))
            .finish()
    }
}

impl MovementTable {
    /// The table of a world without movement classes.
    pub fn empty() -> MovementTable {
        MovementTable::default()
    }

    /// Checks `classes` and sorts them by class. The order of the input, and of
    /// the terrain types inside each class, does not matter: any order gives an
    /// equal table with an equal digest.
    pub fn new(mut classes: Vec<ClassCosts>) -> Result<MovementTable, MovementTableError> {
        classes.sort_by_key(|c| c.class);
        let mut entries: Vec<Entry> = Vec::with_capacity(classes.len());
        for c in classes {
            if entries.last().is_some_and(|e| e.class == c.class) {
                return Err(MovementTableError::DuplicateClass(c.class));
            }
            let mut costs = Box::new([None; 256]);
            for (terrain, cost) in c.costs {
                if cost == Cost::Speed(0) {
                    return Err(MovementTableError::ZeroSpeed {
                        class: c.class,
                        terrain,
                    });
                }
                let slot = &mut costs[usize::from(terrain.0)];
                if slot.is_some() {
                    return Err(MovementTableError::DuplicateTerrain {
                        class: c.class,
                        terrain,
                    });
                }
                *slot = Some(cost);
            }
            entries.push(Entry {
                class: c.class,
                costs,
                turning: Turning::LEGACY,
            });
        }
        let digest = if entries.is_empty() {
            0
        } else {
            digest_of(&entries)
        };
        Ok(MovementTable { entries, digest })
    }

    /// The same table with turning profiles for some of its classes (WP4.5,
    /// `docs/rules/turning.md`). A class that is not listed keeps
    /// [`Turning::LEGACY`], the walk as it was before; the digest changes only
    /// for a class whose profile is not that one.
    pub fn with_turning(
        mut self,
        profiles: Vec<(u16, Turning)>,
    ) -> Result<MovementTable, MovementTableError> {
        let mut seen: Vec<u16> = Vec::with_capacity(profiles.len());
        for (class, turning) in profiles {
            let Ok(at) = self.entries.binary_search_by_key(&class, |e| e.class) else {
                return Err(MovementTableError::UnknownTurningClass(class));
            };
            if seen.contains(&class) {
                return Err(MovementTableError::DuplicateTurning(class));
            }
            turning
                .check()
                .map_err(|problem| MovementTableError::BadTurning { class, problem })?;
            seen.push(class);
            self.entries[at].turning = turning;
        }
        self.digest = if self.entries.is_empty() {
            0
        } else {
            digest_of(&self.entries)
        };
        Ok(self)
    }

    /// How the class turns its hull. A class the table does not have turns as
    /// [`Turning::LEGACY`].
    pub fn turning(&self, class: u16) -> Turning {
        self.entry(class).map_or(Turning::LEGACY, |e| e.turning)
    }

    /// True if there are no classes.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The number of classes.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the table has this class.
    pub fn has_class(&self, class: u16) -> bool {
        self.entry(class).is_some()
    }

    /// The cost of `class` on terrain type `terrain`: `None` for a class the
    /// table does not have, `Some(None)` for a type the class does not list.
    pub fn get(&self, class: u16, terrain: TerrainId) -> Option<Option<Cost>> {
        self.entry(class).map(|e| e.costs[usize::from(terrain.0)])
    }

    /// A hash of every class and its costs; zero for an empty table. Part of
    /// the world checksum for a world with a terrain layer.
    pub fn digest(&self) -> u64 {
        self.digest
    }

    fn entry(&self, class: u16) -> Option<&Entry> {
        self.entries
            .binary_search_by_key(&class, |e| e.class)
            .ok()
            .map(|i| &self.entries[i])
    }

    /// The classes of the table, ascending.
    pub(crate) fn class_ids(&self) -> impl Iterator<Item = u16> + '_ {
        self.entries.iter().map(|e| e.class)
    }

    /// Whether some terrain type stops the class: the class is bound by the
    /// terrain, so it is a ground class that routes are found for. A class
    /// that can enter every terrain type it lists (aircraft) is not.
    pub(crate) fn restricts(&self, class: u16) -> bool {
        self.entry(class)
            .is_some_and(|e| e.costs.iter().any(|c| matches!(c, Some(Cost::Impassable))))
    }

    /// The first terrain type of `defined` (ascending ids) that some class
    /// does not list: `(class, terrain)`.
    pub(crate) fn first_gap(
        &self,
        defined: impl Iterator<Item = TerrainId> + Clone,
    ) -> Option<(u16, TerrainId)> {
        self.entries.iter().find_map(|e| {
            defined
                .clone()
                .find(|t| e.costs[usize::from(t.0)].is_none())
                .map(|t| (e.class, t))
        })
    }
}

fn digest_of(entries: &[Entry]) -> u64 {
    let mut h = Fnv1a64::new();
    h.write_bytes(DIGEST_TAG);
    h.write_u64(u64::try_from(entries.len()).unwrap_or(u64::MAX));
    for e in entries {
        h.write_bytes(&e.class.to_le_bytes());
        let listed = e.costs.iter().filter(|c| c.is_some()).count();
        h.write_u64(u64::try_from(listed).unwrap_or(u64::MAX));
        for (id, cost) in e.costs.iter().enumerate() {
            if let Some(cost) = cost {
                h.write_u8(id as u8);
                h.write_bytes(&cost.permille().to_le_bytes());
            }
        }
    }
    // Turning (WP4.5): only the classes that turn otherwise than the walk always
    // did, and after everything above, so a table without them hashes as before.
    let custom = entries.iter().filter(|e| e.turning != Turning::LEGACY);
    if custom.clone().next().is_some() {
        h.write_bytes(TURNING_TAG);
        h.write_u64(u64::try_from(custom.clone().count()).unwrap_or(u64::MAX));
        for e in custom {
            h.write_bytes(&e.class.to_le_bytes());
            e.turning.write_hash(&mut h);
        }
    }
    h.finish()
}

/// The terrain and the movement table together: what answers "may this class
/// enter this cell, and at what speed". Borrowed from the world for one tick.
#[derive(Copy, Clone)]
pub(crate) struct Ground<'a> {
    pub terrain: &'a Terrain,
    pub movement: &'a MovementTable,
    /// The structures. With it, a cell a structure covers is impassable for
    /// every class that terrain restricts (the classes that are routed);
    /// aircraft fly over. `None` where the caller asks about the terrain alone.
    pub occupancy: Option<&'a Occupancy>,
}

impl Ground<'_> {
    /// The cost of `class` on `cell`; see the module documentation for the
    /// order of the rules.
    pub fn cost(&self, class: u16, cell: Cell) -> Cost {
        let Some(id) = self.terrain.id_at(cell) else {
            return Cost::Impassable;
        };
        if !self.terrain.has_layer() {
            return Cost::OPEN;
        }
        if let Some(occupancy) = self.occupancy
            && occupancy
                .index(cell)
                .is_some_and(|i| occupancy.slot_at(i).is_some())
            && self.movement.restricts(class)
        {
            return Cost::Impassable;
        }
        match self.movement.get(class, id) {
            None => Cost::OPEN,
            Some(Some(cost)) => cost,
            // A class that lists no cost for a type of the map: the world
            // refuses such a table when it takes it, so this cannot be reached
            // through a world; blocked is the safe answer.
            Some(None) => Cost::Impassable,
        }
    }

    /// The cell of a position, if it is on the map.
    pub fn cell_at(&self, pos: sf_math::Vec2) -> Option<Cell> {
        let (x, y) = pos.cell();
        let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
        (x < self.terrain.width() && y < self.terrain.height()).then_some(Cell::new(x, y))
    }

    /// The speed multiplier of the cell a unit stands in. A unit that stands
    /// in a cell it could not enter (nothing puts it there today) moves at
    /// normal speed, so it can walk out.
    pub fn permille_at(&self, class: u16, pos: sf_math::Vec2) -> u16 {
        match self.cell_at(pos).map(|c| self.cost(class, c)) {
            Some(Cost::Speed(p)) => p,
            _ => FULL_PERMILLE,
        }
    }

    /// Whether a unit of `class` may step from `from` to `to`: the cell it
    /// ends in is passable, and for a step that changes both column and row so
    /// are the two cells it cuts across. A step that stays in the same cell is
    /// always allowed.
    pub fn may_step(&self, class: u16, from: sf_math::Vec2, to: sf_math::Vec2) -> bool {
        let (Some(a), Some(b)) = (self.cell_at(from), self.cell_at(to)) else {
            return false;
        };
        if a == b {
            return true;
        }
        if !self.cost(class, b).is_passable() {
            return false;
        }
        if a.x != b.x && a.y != b.y {
            return self.cost(class, Cell::new(b.x, a.y)).is_passable()
                && self.cost(class, Cell::new(a.x, b.y)).is_passable();
        }
        true
    }
}

/// A step of `step` cells per tick at `permille` thousandths of normal speed:
/// `step * permille / 1000`, rounded once to the nearest raw unit, never zero
/// for a positive step and never more than [`MAX_STEP`]. At `1000` the result
/// is `step` itself, bit for bit (unless it is above [`MAX_STEP`]).
pub fn scale_step(step: Fx, permille: u16) -> Fx {
    let scaled = if permille == FULL_PERMILLE {
        step
    } else {
        step.mul_int(i64::from(permille)).div_int(1000)
    };
    let floor = if step > Fx::ZERO && permille > 0 {
        Fx::from_raw(1)
    } else {
        Fx::ZERO
    };
    scaled.max(floor).min(MAX_STEP)
}

#[cfg(test)]
mod tests;
