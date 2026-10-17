//! The struct-of-arrays entity store: one `Vec` per column, one slot per
//! index.
//!
//! All columns always have the same length (the slot count). A slot is either
//! live or free; free slots keep their place so that indices never move. The
//! store does not decide which slot a new entity gets or when a generation
//! changes; [`World`](crate::World) owns that policy (free list, generation
//! bump) and uses [`Entities::fill`] and [`Entities::clear`] to write rows.

use sf_math::{Facing, Rng, Vec2};

use crate::behavior::{Area, Behavior};
use crate::ids::{Kind, PlayerId};
use crate::terrain::NO_CELL;

/// The `def` column value of an entity that has no entry in the unit table
/// (every legacy `Spawn` wanderer and `Place` structure).
pub(crate) const NO_DEF: u32 = u32::MAX;

/// One row of values for a freshly spawned entity.
#[derive(Clone, Debug)]
pub(crate) struct NewEntity {
    pub kind: Kind,
    pub owner: PlayerId,
    pub pos: Vec2,
    pub facing: Facing,
    pub behavior: Behavior,
    pub area: Area,
    pub rng: Rng,
    /// Position in the unit table, or [`NO_DEF`].
    pub def: u32,
    /// Hit points at birth (zero without a table entry).
    pub hp: u32,
}

/// The columns. Index `i` of every vector describes slot `i`.
///
/// Fields of a free slot hold neutral values and are ignored by everything
/// (the checksum, the snapshot, the behaviours) except `generation`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct Entities {
    /// How often the slot has been freed. Part of every [`EntityId`](crate::EntityId).
    pub generation: Vec<u32>,
    /// Whether the slot holds an entity.
    pub alive: Vec<bool>,
    pub kind: Vec<Kind>,
    pub owner: Vec<PlayerId>,
    /// Position in cells at the end of the last executed tick.
    pub pos: Vec<Vec2>,
    /// Position one tick earlier, for render interpolation. Derived data; not
    /// part of the checksum.
    pub prev_pos: Vec<Vec2>,
    pub facing: Vec<Facing>,
    /// Facing one tick earlier, for render interpolation. Derived data; not
    /// part of the checksum.
    pub prev_facing: Vec<Facing>,
    pub behavior: Vec<Behavior>,
    /// Where the entity is walking to, if anywhere.
    pub target: Vec<Option<Vec2>>,
    /// The area wander draws its targets from.
    pub area: Vec<Area>,
    /// The entity's own random stream.
    pub rng: Vec<Rng>,
    /// The row-major index of the cell the entity occupies, or [`NO_CELL`] if
    /// it occupies none (wanderers). Only `Place` sets it; the world's
    /// occupancy grid is the same information indexed by cell, and the
    /// checksum covers it from there.
    pub cell: Vec<u32>,
    /// The position of the entity's kind in the world's unit table, or
    /// [`NO_DEF`] for an entity that was not made from a table entry. Derived
    /// from the kind and the table, so it is not hashed on its own.
    pub def: Vec<u32>,
    /// Current hit points; zero for an entity without a table entry and for
    /// one that cannot be damaged. Hashed (it will change in phase 5).
    pub hp: Vec<u32>,
}

impl Entities {
    /// The number of slots (live and free).
    #[inline]
    pub fn len(&self) -> usize {
        self.generation.len()
    }

    /// Appends one free slot with generation zero and returns its index.
    pub fn push_free_slot(&mut self) -> usize {
        let index = self.generation.len();
        self.generation.push(0);
        self.alive.push(false);
        self.kind.push(Kind::default());
        self.owner.push(PlayerId::default());
        self.pos.push(Vec2::ZERO);
        self.prev_pos.push(Vec2::ZERO);
        self.facing.push(Facing::default());
        self.prev_facing.push(Facing::default());
        self.behavior.push(Behavior::Idle);
        self.target.push(None);
        self.area.push(Area::new(Vec2::ZERO, Vec2::ZERO));
        self.rng.push(Rng::new(0));
        self.cell.push(NO_CELL);
        self.def.push(NO_DEF);
        self.hp.push(0);
        index
    }

    /// Writes a new entity into free slot `i` and marks it live. The previous
    /// position and facing start equal to the current ones, so a new entity
    /// does not streak across the screen on its first frame.
    pub fn fill(&mut self, i: usize, row: NewEntity) {
        self.alive[i] = true;
        self.kind[i] = row.kind;
        self.owner[i] = row.owner;
        self.pos[i] = row.pos;
        self.prev_pos[i] = row.pos;
        self.facing[i] = row.facing;
        self.prev_facing[i] = row.facing;
        self.behavior[i] = row.behavior;
        self.target[i] = None;
        self.area[i] = row.area;
        self.rng[i] = row.rng;
        self.cell[i] = NO_CELL;
        self.def[i] = row.def;
        self.hp[i] = row.hp;
    }

    /// Frees slot `i`: marks it dead and resets every column to its neutral
    /// value, so nothing of the old entity survives in the slot. The
    /// generation is left to the caller.
    pub fn clear(&mut self, i: usize) {
        self.alive[i] = false;
        self.kind[i] = Kind::default();
        self.owner[i] = PlayerId::default();
        self.pos[i] = Vec2::ZERO;
        self.prev_pos[i] = Vec2::ZERO;
        self.facing[i] = Facing::default();
        self.prev_facing[i] = Facing::default();
        self.behavior[i] = Behavior::Idle;
        self.target[i] = None;
        self.area[i] = Area::new(Vec2::ZERO, Vec2::ZERO);
        self.rng[i] = Rng::new(0);
        self.cell[i] = NO_CELL;
        self.def[i] = NO_DEF;
        self.hp[i] = 0;
    }
}
