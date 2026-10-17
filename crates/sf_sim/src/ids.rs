//! Small identifier types and the tick constants.
//!
//! None of these carries game rules: a [`Kind`] is an opaque number the data
//! layer will give meaning to later, a [`PlayerId`] is an opaque seat number,
//! and a [`Tick`] is a count of simulation steps.

use core::fmt;

use sf_math::Fx;

// Simulation steps per second (PLAN.md decision B3) and the milliseconds of
// game time per tick (`1000 / TICKS_PER_SECOND`). Defined in `sf_math` so that
// `sf_data` can use them without depending on this crate; re-exported here.
// `sf_engine` runs its fixed-step loop at the same rate; it does not depend on
// `sf_math` for it, so the number is repeated there on purpose.
pub use sf_math::{TICK_MILLIS, TICKS_PER_SECOND};

/// The duration of one tick in seconds as a fixed-point number: one twentieth,
/// rounded to the nearest raw unit (3277 of 65536, which is a hair more
/// than a twentieth). One twentieth is not representable in binary, so prefer
/// [`per_tick`] over multiplying by this constant when converting a rate.
pub const TICK_DURATION: Fx = Fx::from_ratio(1, TICKS_PER_SECOND as i64);

/// Converts a rate per second (cells per second, say) to a rate per tick,
/// with a single rounding step (`rate / 20`, to nearest).
#[inline]
pub const fn per_tick(per_second: Fx) -> Fx {
    per_second.div_int(TICKS_PER_SECOND as i64)
}

/// A count of simulation ticks.
///
/// [`World::tick`](crate::World::tick) is the number of ticks that have been
/// executed, which is also the number of the tick the next
/// [`World::step`](crate::World::step) will run. Commands are scheduled for a
/// tick number and run at the start of that tick.
#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Tick(pub u64);

impl Tick {
    /// The first tick.
    pub const ZERO: Tick = Tick(0);

    /// The tick after this one (saturating; 2^64 ticks last far longer than
    /// the universe at 20 per second).
    #[inline]
    pub const fn next(self) -> Tick {
        Tick(self.0.saturating_add(1))
    }

    /// `self` plus `n` ticks, saturating.
    #[inline]
    pub const fn saturating_add(self, n: u64) -> Tick {
        Tick(self.0.saturating_add(n))
    }
}

impl fmt::Display for Tick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A player seat. Commands are issued by a player; entities belong to one.
/// The engine attaches no meaning to the number beyond ordering (commands of
/// one tick run in ascending player order).
#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PlayerId(pub u8);

/// What an entity is, as an opaque number. The simulation core stores it and
/// hands it to the presentation (which picks a mesh or an icon); the data
/// layer (`sf_data`, phase 3) will map it to a unit definition.
#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Kind(pub u16);

/// A generation-checked handle to an entity.
///
/// `index` is the slot in the columns, `generation` counts how often that slot
/// has been freed. An id whose generation no longer matches its slot is stale
/// (the entity is gone, perhaps replaced by a different one in the same slot)
/// and every lookup treats it as not found. The pair is public data on
/// purpose: replays and scenario files carry ids as two plain numbers, and
/// [`EntityId::new`] rebuilds one. Ids are assigned deterministically (see the
/// crate documentation), so a script can name an entity before it exists.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct EntityId {
    index: u32,
    generation: u32,
}

impl EntityId {
    /// An id from its two parts.
    #[inline]
    pub const fn new(index: u32, generation: u32) -> EntityId {
        EntityId { index, generation }
    }

    /// The slot number.
    #[inline]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// The slot's generation when this id was issued.
    #[inline]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}v{}", self.index, self.generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_constants_agree() {
        assert_eq!(TICKS_PER_SECOND, 20);
        assert_eq!(TICK_MILLIS, 50);
        assert_eq!(TICK_MILLIS * TICKS_PER_SECOND, 1000);
        // One twentieth to the nearest raw unit.
        assert_eq!(TICK_DURATION.raw(), 3277);
    }

    #[test]
    fn per_tick_divides_by_the_tick_rate() {
        assert_eq!(per_tick(Fx::from_int(20)), Fx::ONE);
        assert_eq!(per_tick(Fx::from_int(4)).raw(), 13_107); // a fifth of a cell
        assert_eq!(per_tick(Fx::ZERO), Fx::ZERO);
        assert_eq!(per_tick(Fx::from_int(-20)), Fx::NEG_ONE);
    }

    #[test]
    fn tick_arithmetic_saturates() {
        assert_eq!(Tick::ZERO.next(), Tick(1));
        assert_eq!(Tick(u64::MAX).next(), Tick(u64::MAX));
        assert_eq!(Tick(5).saturating_add(7), Tick(12));
        assert_eq!(Tick(u64::MAX).saturating_add(7), Tick(u64::MAX));
        assert_eq!(Tick(12).to_string(), "12");
    }

    #[test]
    fn entity_id_round_trips() {
        let id = EntityId::new(7, 3);
        assert_eq!((id.index(), id.generation()), (7, 3));
        assert_eq!(id.to_string(), "7v3");
        assert_ne!(id, EntityId::new(7, 4));
        assert_ne!(id, EntityId::new(8, 3));
    }
}
