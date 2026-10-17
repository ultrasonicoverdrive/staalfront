//! The unit table: what the simulation knows about a unit kind (WP3.2).
//!
//! `sf_data` reads and validates `assets/config/unit_defs.ron` and converts it
//! to plain integers (`SimUnitDef`); this crate may not depend on it, so the
//! caller (`sf_game`, `sf_headless`) copies the few fields the rules read into
//! a [`UnitStats`] and hands the list over once, when the world is made
//! ([`World::with_units`](crate::World::with_units)), exactly as it does the
//! terrain. The simulation never reads a file and never sees a name: a role is
//! its permanent [`Kind`] number.
//!
//! What a [`UnitStats`] holds is what the rules of this work package use:
//! hit points (copied into the entity when it is spawned, because they will
//! change), the movement class, speed per tick and turn rate (a spawned unit
//! moves by its own, not by the world's placeholder speed), the size, and
//! the footprint of a structure. Weapons, armor, cost and the abilities join
//! when the work packages that read them need them.
//!
//! A kind is one of three things, told by its fields:
//!
//! | Class | Has | Spawned by | Occupies |
//! |---|---|---|---|
//! | mobile | a movement class | [`Command::SpawnUnit`](crate::Command::SpawnUnit) | no cell |
//! | structure | a footprint | [`Command::PlaceStructure`](crate::Command::PlaceStructure) | every cell of the footprint |
//! | object | neither | [`Command::SpawnUnit`](crate::Command::SpawnUnit) | no cell, and never moves |
//!
//! The table is fixed for the whole game. A world with a non-empty table
//! hashes a digest of it into the checksum (see [`crate::checksum`]).

use core::fmt;

use sf_math::Fx;
use sf_math::hash::Fnv1a64;

use crate::ids::Kind;

/// Written first into the digest; bump when its layout changes.
const DIGEST_TAG: &[u8] = b"sf_sim/units/v1";

/// The cells a structure covers: `width` along x, `height` along y, both at
/// least one.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Footprint {
    /// Cells along x.
    pub width: u8,
    /// Cells along y.
    pub height: u8,
}

impl Footprint {
    /// One cell, what a legacy [`Command::Place`](crate::Command::Place)
    /// structure covers.
    pub const ONE: Footprint = Footprint {
        width: 1,
        height: 1,
    };
}

/// What the rules read about one unit kind.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct UnitStats {
    /// The permanent kind number of the role.
    pub kind: Kind,
    /// Hit points a new entity starts with; zero means it cannot be damaged.
    pub hp: u32,
    /// Kind number of the movement class; `None` for a structure or an
    /// object. Terrain costs per class are WP4.1.
    pub movement_class: Option<u16>,
    /// Distance per tick, rounded once from the exact decimal. Nonzero for a
    /// mobile kind.
    pub speed_per_tick: Fx,
    /// Facing steps (of 256) per tick. Nonzero for a mobile kind.
    pub turn_per_tick: u8,
    /// Diameter in cells (zero for a structure); collision comes in phase four.
    pub size: Fx,
    /// The cells a structure covers; `None` for everything that is not a
    /// structure.
    pub footprint: Option<Footprint>,
}

/// What sort of thing a kind is, derived from its fields.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum UnitClass {
    /// Has a movement class: infantry, a vehicle or a vessel.
    Mobile,
    /// Has a footprint: a building.
    Structure,
    /// Neither: a crate, an ore field. Stands where it is spawned.
    Object,
}

impl UnitStats {
    /// Whether this kind moves, stands on a footprint or is an object.
    pub const fn class(&self) -> UnitClass {
        if self.movement_class.is_some() {
            UnitClass::Mobile
        } else if self.footprint.is_some() {
            UnitClass::Structure
        } else {
            UnitClass::Object
        }
    }
}

/// Why a list of unit stats was refused.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum UnitTableError {
    /// Two entries have the same kind.
    DuplicateKind(Kind),
    /// An entry has both a movement class and a footprint.
    MovingStructure(Kind),
    /// A mobile entry has no speed (it could never arrive anywhere).
    NoSpeed(Kind),
    /// A mobile entry has a turn rate of zero (it could never face its way).
    NoTurning(Kind),
    /// A footprint has a side of zero cells.
    EmptyFootprint(Kind),
}

impl fmt::Display for UnitTableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnitTableError::DuplicateKind(k) => write!(f, "kind {} is listed twice", k.0),
            UnitTableError::MovingStructure(k) => {
                write!(f, "kind {} has both a movement class and a footprint", k.0)
            }
            UnitTableError::NoSpeed(k) => write!(f, "mobile kind {} has no speed", k.0),
            UnitTableError::NoTurning(k) => {
                write!(f, "mobile kind {} has a turn rate of zero", k.0)
            }
            UnitTableError::EmptyFootprint(k) => {
                write!(f, "kind {} has a footprint with a side of zero cells", k.0)
            }
        }
    }
}

impl std::error::Error for UnitTableError {}

/// The stats of every kind a world can spawn, sorted by kind. Immutable once
/// built. An empty table is a world without data-driven units: the legacy
/// `Spawn` and `Place` commands still work and hash exactly as before.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct UnitTable {
    /// Sorted by kind, so a lookup is a binary search and the order never
    /// depends on how the caller listed them.
    stats: Vec<UnitStats>,
    /// Hash of everything above; zero when empty.
    digest: u64,
}

impl fmt::Debug for UnitTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UnitTable")
            .field("kinds", &self.stats.len())
            .field("digest", &format_args!("{:#018x}", self.digest))
            .finish()
    }
}

impl UnitTable {
    /// The table of a world without data-driven units.
    pub fn empty() -> UnitTable {
        UnitTable::default()
    }

    /// Checks `stats` and sorts them by kind. The order of the input does not
    /// matter: any order gives an equal table with an equal digest.
    pub fn new(mut stats: Vec<UnitStats>) -> Result<UnitTable, UnitTableError> {
        stats.sort_by_key(|s| s.kind);
        for pair in stats.windows(2) {
            if pair[0].kind == pair[1].kind {
                return Err(UnitTableError::DuplicateKind(pair[0].kind));
            }
        }
        for s in &stats {
            if s.movement_class.is_some() && s.footprint.is_some() {
                return Err(UnitTableError::MovingStructure(s.kind));
            }
            if s.movement_class.is_some() {
                if s.speed_per_tick <= Fx::ZERO {
                    return Err(UnitTableError::NoSpeed(s.kind));
                }
                if s.turn_per_tick == 0 {
                    return Err(UnitTableError::NoTurning(s.kind));
                }
            }
            if let Some(fp) = s.footprint
                && (fp.width == 0 || fp.height == 0)
            {
                return Err(UnitTableError::EmptyFootprint(s.kind));
            }
        }
        let digest = if stats.is_empty() {
            0
        } else {
            digest_of(&stats)
        };
        Ok(UnitTable { stats, digest })
    }

    /// True if there are no kinds.
    pub fn is_empty(&self) -> bool {
        self.stats.is_empty()
    }

    /// The number of kinds.
    pub fn len(&self) -> usize {
        self.stats.len()
    }

    /// Every kind, ascending.
    pub fn iter(&self) -> impl Iterator<Item = &UnitStats> {
        self.stats.iter()
    }

    /// The stats of `kind`; `None` for a kind the table does not define.
    pub fn get(&self, kind: Kind) -> Option<&UnitStats> {
        self.index_of(kind).map(|i| &self.stats[i])
    }

    /// A hash of every entry; zero for an empty table. Part of the world
    /// checksum.
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// The position of `kind` in the sorted list: what an entity stores.
    pub(crate) fn index_of(&self, kind: Kind) -> Option<usize> {
        self.stats.binary_search_by_key(&kind, |s| s.kind).ok()
    }

    /// The entry at a position from [`UnitTable::index_of`].
    pub(crate) fn at(&self, index: usize) -> &UnitStats {
        &self.stats[index]
    }
}

fn digest_of(stats: &[UnitStats]) -> u64 {
    let mut h = Fnv1a64::new();
    h.write_bytes(DIGEST_TAG);
    h.write_u64(u64::try_from(stats.len()).unwrap_or(u64::MAX));
    for s in stats {
        h.write_bytes(&s.kind.0.to_le_bytes());
        h.write_u32(s.hp);
        match s.movement_class {
            Some(class) => {
                h.write_u8(1);
                h.write_bytes(&class.to_le_bytes());
            }
            None => h.write_u8(0),
        }
        h.write_fx(s.speed_per_tick);
        h.write_u8(s.turn_per_tick);
        h.write_fx(s.size);
        match s.footprint {
            Some(fp) => {
                h.write_u8(1);
                h.write_u8(fp.width);
                h.write_u8(fp.height);
            }
            None => h.write_u8(0),
        }
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mobile(kind: u16) -> UnitStats {
        UnitStats {
            kind: Kind(kind),
            hp: 50,
            movement_class: Some(0),
            speed_per_tick: Fx::from_ratio(1, 20),
            turn_per_tick: 8,
            size: Fx::from_ratio(2, 5),
            footprint: None,
        }
    }

    fn structure(kind: u16, w: u8, h: u8) -> UnitStats {
        UnitStats {
            kind: Kind(kind),
            hp: 600,
            movement_class: None,
            speed_per_tick: Fx::ZERO,
            turn_per_tick: 0,
            size: Fx::ZERO,
            footprint: Some(Footprint {
                width: w,
                height: h,
            }),
        }
    }

    fn object(kind: u16) -> UnitStats {
        UnitStats {
            footprint: None,
            ..structure(kind, 1, 1)
        }
    }

    #[test]
    fn the_class_follows_the_fields() {
        assert_eq!(mobile(1).class(), UnitClass::Mobile);
        assert_eq!(structure(2, 2, 3).class(), UnitClass::Structure);
        assert_eq!(object(3).class(), UnitClass::Object);
    }

    #[test]
    fn the_input_order_does_not_matter() {
        let a = UnitTable::new(vec![mobile(7), structure(2, 2, 2), object(40)]).unwrap();
        let b = UnitTable::new(vec![object(40), mobile(7), structure(2, 2, 2)]).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.digest(), b.digest());
        assert_ne!(a.digest(), 0);
        let kinds: Vec<u16> = a.iter().map(|s| s.kind.0).collect();
        assert_eq!(kinds, vec![2, 7, 40]);
        assert_eq!(a.get(Kind(7)), Some(&mobile(7)));
        assert_eq!(a.get(Kind(8)), None);
    }

    #[test]
    fn an_empty_table_has_no_digest() {
        let t = UnitTable::new(Vec::new()).unwrap();
        assert!(t.is_empty());
        assert_eq!(t.digest(), 0);
        assert_eq!(t, UnitTable::empty());
    }

    #[test]
    fn every_stat_changes_the_digest() {
        let base = UnitTable::new(vec![mobile(1)]).unwrap().digest();
        let changed = |f: fn(&mut UnitStats)| {
            let mut s = mobile(1);
            f(&mut s);
            UnitTable::new(vec![s]).unwrap().digest()
        };
        assert_ne!(base, changed(|s| s.hp += 1));
        assert_ne!(base, changed(|s| s.movement_class = Some(1)));
        assert_ne!(
            base,
            changed(|s| s.speed_per_tick = Fx::from_raw(s.speed_per_tick.raw() + 1))
        );
        assert_ne!(base, changed(|s| s.turn_per_tick += 1));
        assert_ne!(base, changed(|s| s.size = Fx::ONE));
        assert_ne!(base, changed(|s| s.kind = Kind(2)));
    }

    #[test]
    fn bad_entries_are_refused() {
        let err = |v| UnitTable::new(v).unwrap_err();
        assert_eq!(
            err(vec![mobile(1), mobile(1)]),
            UnitTableError::DuplicateKind(Kind(1))
        );
        let moving = UnitStats {
            footprint: Some(Footprint::ONE),
            ..mobile(2)
        };
        assert_eq!(err(vec![moving]), UnitTableError::MovingStructure(Kind(2)));
        let slow = UnitStats {
            speed_per_tick: Fx::ZERO,
            ..mobile(3)
        };
        assert_eq!(err(vec![slow]), UnitTableError::NoSpeed(Kind(3)));
        let stuck = UnitStats {
            turn_per_tick: 0,
            ..mobile(4)
        };
        assert_eq!(err(vec![stuck]), UnitTableError::NoTurning(Kind(4)));
        assert_eq!(
            err(vec![structure(5, 0, 2)]),
            UnitTableError::EmptyFootprint(Kind(5))
        );
        assert!(!UnitTableError::NoSpeed(Kind(1)).to_string().is_empty());
    }
}
