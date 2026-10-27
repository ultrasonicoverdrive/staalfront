//! The building table: what the simulation knows about a structure kind
//! beyond its footprint (WP6.1, `docs/rules/buildings.md`).
//!
//! `sf_data` reads the `building` block of `unit_defs.ron`, checks it and
//! converts it; this crate may not depend on it, so the caller
//! (`sf_bridge::building_table`) copies the fields into a [`BuildingStats`] per
//! structure and hands the list over once, right after the unit table
//! ([`World::with_buildings`](crate::World::with_buildings)).
//!
//! **No behaviour lives here yet.** The table only answers questions:
//! what a structure is for ([`BuildingFunction`]), on which side units use it
//! ([`Side`]), how long it takes to build, what it costs, what power it makes
//! or uses, who may build it and what must stand first. Placement is WP6.2,
//! power WP6.4, production WP6.5; each reads this table.
//!
//! The table is fixed for the whole game. A world with a non-empty table hashes
//! its digest into the checksum (section `B`, see [`crate::checksum`]); a world
//! given none hashes as before.

use core::fmt;

use sf_math::hash::Fnv1a64;

use crate::ids::Kind;
use crate::units::Footprint;

/// Written first into the digest; bump when its layout changes.
const DIGEST_TAG: &[u8] = b"sf_sim/buildings/v1";

/// What a structure does for its owner. The numbers are permanent (they are in
/// the digest) and match `sf_data::units::BuildingFunction::number`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BuildingFunction {
    /// Nothing the rules act on.
    None,
    /// The base anchor: the owner builds around it.
    Anchor,
    /// Produces power.
    PowerPlant,
    /// Harvesters unload here.
    HarvesterDock,
    /// Produces infantry.
    InfantryFactory,
    /// Produces vehicles.
    VehicleFactory,
    /// Produces vessels.
    VesselFactory,
    /// Enables the minimap.
    Radar,
    /// Fights or shelters.
    Defence,
    /// Looks like another building to the enemy AI.
    Decoy,
}

impl BuildingFunction {
    /// The permanent number of the function.
    pub const fn number(self) -> u8 {
        match self {
            BuildingFunction::None => 0,
            BuildingFunction::Anchor => 1,
            BuildingFunction::PowerPlant => 2,
            BuildingFunction::HarvesterDock => 3,
            BuildingFunction::InfantryFactory => 4,
            BuildingFunction::VehicleFactory => 5,
            BuildingFunction::VesselFactory => 6,
            BuildingFunction::Radar => 7,
            BuildingFunction::Defence => 8,
            BuildingFunction::Decoy => 9,
        }
    }

    /// Whether units come out of it.
    pub const fn is_factory(self) -> bool {
        matches!(
            self,
            BuildingFunction::InfantryFactory
                | BuildingFunction::VehicleFactory
                | BuildingFunction::VesselFactory
        )
    }

    /// Whether units go in or out of it, so it has an exit side.
    pub const fn needs_exit(self) -> bool {
        self.is_factory() || matches!(self, BuildingFunction::HarvesterDock)
    }
}

/// A side of a footprint: +x is east and +y is south.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    /// East, +x.
    PosX,
    /// West, -x.
    NegX,
    /// South, +y.
    PosY,
    /// North, -y.
    NegY,
}

impl Side {
    /// The permanent number of the side.
    pub const fn number(self) -> u8 {
        match self {
            Side::PosX => 0,
            Side::NegX => 1,
            Side::PosY => 2,
            Side::NegY => 3,
        }
    }
}

/// One structure kind.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BuildingStats {
    /// The kind; must be a structure of the unit table with the same footprint.
    pub kind: Kind,
    /// Cells covered, both sides at least one.
    pub footprint: Footprint,
    /// What it does.
    pub function: BuildingFunction,
    /// The side units come out of or go in by; `Some` exactly for a function
    /// that [`needs_exit`](BuildingFunction::needs_exit).
    pub exit_side: Option<Side>,
    /// Ticks to build; zero for a structure that cannot be built.
    pub build_ticks: u32,
    /// Price in credits; `None` if it cannot be bought (read from phase 7).
    pub cost: Option<u32>,
    /// Power produced (positive) or used (negative).
    pub power: i32,
    /// Bit mask of the factions that may build it.
    pub factions: u8,
    /// The structures its owner must have standing, in any order.
    pub requires: Vec<Kind>,
}

/// Why a list of building stats was refused.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum BuildingsError {
    /// Two entries have the same kind.
    DuplicateKind(Kind),
    /// A footprint has a side of zero cells.
    EmptyFootprint(Kind),
    /// A factory or the harvester dock has no exit side.
    NoExitSide(Kind),
    /// A building has an exit side without a function that uses one.
    UselessExitSide(Kind),
    /// A prerequisite is not an entry of the table.
    UnknownPrerequisite(Kind, Kind),
    /// A building needs itself, directly or through others, or is behind a
    /// cycle (the lowest kind that cannot be built in any order).
    Cycle(Kind),
    /// Something with a build time has no cost, or the other way round.
    CostAndTime(Kind),
    /// A structure with prerequisites cannot be built.
    RequiresWithoutCost(Kind),
    /// A kind that is not in the unit table (also every kind when the unit
    /// table is not set yet) or is not a structure there (`World::with_buildings`).
    NotAStructure(Kind),
    /// A footprint that is not the unit table's (`World::with_buildings`).
    FootprintMismatch(Kind),
}

impl fmt::Display for BuildingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildingsError::DuplicateKind(k) => write!(f, "kind {} is listed twice", k.0),
            BuildingsError::EmptyFootprint(k) => {
                write!(f, "kind {} has a footprint with a side of zero cells", k.0)
            }
            BuildingsError::NoExitSide(k) => {
                write!(f, "kind {} is a factory or dock without an exit side", k.0)
            }
            BuildingsError::UselessExitSide(k) => {
                write!(
                    f,
                    "kind {} has an exit side but no function that uses it",
                    k.0
                )
            }
            BuildingsError::UnknownPrerequisite(k, p) => {
                write!(
                    f,
                    "kind {} needs kind {}, which is not a building",
                    k.0, p.0
                )
            }
            BuildingsError::Cycle(k) => {
                write!(f, "kind {} needs itself through its prerequisites", k.0)
            }
            BuildingsError::CostAndTime(k) => {
                write!(
                    f,
                    "kind {} has a cost without a build time or the reverse",
                    k.0
                )
            }
            BuildingsError::RequiresWithoutCost(k) => {
                write!(f, "kind {} has prerequisites but cannot be built", k.0)
            }
            BuildingsError::NotAStructure(k) => {
                write!(f, "kind {} is not a structure of the unit table", k.0)
            }
            BuildingsError::FootprintMismatch(k) => {
                write!(
                    f,
                    "kind {} has another footprint than in the unit table",
                    k.0
                )
            }
        }
    }
}

impl std::error::Error for BuildingsError {}

/// The building stats of every structure kind, sorted by kind. Immutable once
/// built. An empty table is a world without building rules.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct BuildingTable {
    stats: Vec<BuildingStats>,
    /// Hash of everything above; zero when empty.
    digest: u64,
}

impl fmt::Debug for BuildingTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuildingTable")
            .field("kinds", &self.stats.len())
            .field("digest", &format_args!("{:#018x}", self.digest))
            .finish()
    }
}

impl BuildingTable {
    /// The table of a world without building rules.
    pub fn empty() -> BuildingTable {
        BuildingTable::default()
    }

    /// Checks `stats`, sorts them by kind and each `requires` list ascending:
    /// the order of the input does not matter, any order gives an equal table
    /// with an equal digest.
    pub fn new(mut stats: Vec<BuildingStats>) -> Result<BuildingTable, BuildingsError> {
        stats.sort_by_key(|s| s.kind);
        for pair in stats.windows(2) {
            if pair[0].kind == pair[1].kind {
                return Err(BuildingsError::DuplicateKind(pair[0].kind));
            }
        }
        for s in &mut stats {
            s.requires.sort();
            s.requires.dedup();
        }
        for s in &stats {
            if s.footprint.width == 0 || s.footprint.height == 0 {
                return Err(BuildingsError::EmptyFootprint(s.kind));
            }
            match (s.exit_side, s.function.needs_exit()) {
                (None, true) => return Err(BuildingsError::NoExitSide(s.kind)),
                (Some(_), false) => return Err(BuildingsError::UselessExitSide(s.kind)),
                _ => {}
            }
            if s.cost.is_some() != (s.build_ticks > 0) {
                return Err(BuildingsError::CostAndTime(s.kind));
            }
            if !s.requires.is_empty() && s.cost.is_none() {
                return Err(BuildingsError::RequiresWithoutCost(s.kind));
            }
            for &p in &s.requires {
                if stats.binary_search_by_key(&p, |o| o.kind).is_err() {
                    return Err(BuildingsError::UnknownPrerequisite(s.kind, p));
                }
            }
        }
        if let Some(kind) = find_cycle(&stats) {
            return Err(BuildingsError::Cycle(kind));
        }
        let digest = if stats.is_empty() {
            0
        } else {
            digest_of(&stats)
        };
        Ok(BuildingTable { stats, digest })
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
    pub fn iter(&self) -> impl Iterator<Item = &BuildingStats> {
        self.stats.iter()
    }

    /// The stats of `kind`; `None` for a kind that has none.
    pub fn get(&self, kind: Kind) -> Option<&BuildingStats> {
        self.stats
            .binary_search_by_key(&kind, |s| s.kind)
            .ok()
            .map(|i| &self.stats[i])
    }

    /// A hash of every entry; zero for an empty table. Part of the world
    /// checksum.
    pub fn digest(&self) -> u64 {
        self.digest
    }
}

/// The first kind (lowest number) that lies on a cycle of `requires`, if any.
/// `stats` is sorted by kind and every prerequisite is in it. Kahn's algorithm:
/// what cannot be removed is on or behind a cycle; the lowest kind that still
/// needs one of the remaining kinds is reported.
fn find_cycle(stats: &[BuildingStats]) -> Option<Kind> {
    let mut left: Vec<bool> = vec![true; stats.len()];
    loop {
        let mut removed = false;
        for i in 0..stats.len() {
            if left[i]
                && stats[i].requires.iter().all(|&p| {
                    stats
                        .binary_search_by_key(&p, |o| o.kind)
                        .is_ok_and(|j| !left[j])
                })
            {
                left[i] = false;
                removed = true;
            }
        }
        if !removed {
            break;
        }
    }
    (0..stats.len()).find(|&i| left[i]).map(|i| stats[i].kind)
}

fn digest_of(stats: &[BuildingStats]) -> u64 {
    let mut h = Fnv1a64::new();
    h.write_bytes(DIGEST_TAG);
    h.write_u64(u64::try_from(stats.len()).unwrap_or(u64::MAX));
    for s in stats {
        h.write_bytes(&s.kind.0.to_le_bytes());
        h.write_u8(s.footprint.width);
        h.write_u8(s.footprint.height);
        h.write_u8(s.function.number());
        match s.exit_side {
            Some(side) => {
                h.write_u8(1);
                h.write_u8(side.number());
            }
            None => h.write_u8(0),
        }
        h.write_u32(s.build_ticks);
        match s.cost {
            Some(c) => {
                h.write_u8(1);
                h.write_u32(c);
            }
            None => h.write_u8(0),
        }
        h.write_bytes(&s.power.to_le_bytes());
        h.write_u8(s.factions);
        h.write_u64(u64::try_from(s.requires.len()).unwrap_or(u64::MAX));
        for r in &s.requires {
            h.write_bytes(&r.0.to_le_bytes());
        }
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(kind: u16, requires: &[u16]) -> BuildingStats {
        BuildingStats {
            kind: Kind(kind),
            footprint: Footprint {
                width: 2,
                height: 2,
            },
            function: BuildingFunction::None,
            exit_side: None,
            build_ticks: 100,
            cost: Some(100),
            power: 0,
            factions: 3,
            requires: requires.iter().map(|&k| Kind(k)).collect(),
        }
    }

    #[test]
    fn input_order_does_not_change_the_table() {
        let a = BuildingTable::new(vec![b(1, &[]), b(2, &[1]), b(3, &[2, 1])]).unwrap();
        let c = BuildingTable::new(vec![b(3, &[1, 2]), b(1, &[]), b(2, &[1])]).unwrap();
        assert_eq!(a, c);
        assert_eq!(a.digest(), c.digest());
        assert_ne!(a.digest(), 0);
        assert_eq!(BuildingTable::empty().digest(), 0);
    }

    #[test]
    fn every_field_is_in_the_digest() {
        let base = BuildingTable::new(vec![b(1, &[]), b(2, &[1])])
            .unwrap()
            .digest();
        type Mutant = Box<dyn Fn(&mut BuildingStats)>;
        let mutants: Vec<Mutant> = vec![
            Box::new(|s| s.footprint.width = 3),
            Box::new(|s| s.footprint.height = 3),
            Box::new(|s| s.function = BuildingFunction::Radar),
            Box::new(|s| s.build_ticks = 101),
            Box::new(|s| s.cost = Some(101)),
            Box::new(|s| s.power = -5),
            Box::new(|s| s.factions = 1),
            Box::new(|s| s.requires.clear()),
        ];
        for (n, m) in mutants.iter().enumerate() {
            let mut second = b(2, &[1]);
            m(&mut second);
            let digest = BuildingTable::new(vec![b(1, &[]), second])
                .unwrap()
                .digest();
            assert_ne!(digest, base, "mutant {n}");
        }
    }

    #[test]
    fn a_cycle_is_refused_and_a_chain_is_not() {
        assert_eq!(
            BuildingTable::new(vec![b(1, &[3]), b(2, &[1]), b(3, &[2])]).unwrap_err(),
            BuildingsError::Cycle(Kind(1))
        );
        assert_eq!(
            BuildingTable::new(vec![b(1, &[1])]).unwrap_err(),
            BuildingsError::Cycle(Kind(1))
        );
        assert!(BuildingTable::new(vec![b(1, &[]), b(2, &[1]), b(3, &[1, 2])]).is_ok());
    }

    #[test]
    fn each_kind_of_bad_entry_is_refused() {
        let with = |f: &dyn Fn(&mut BuildingStats)| {
            let mut x = b(2, &[]);
            f(&mut x);
            BuildingTable::new(vec![b(1, &[]), x]).unwrap_err()
        };
        assert_eq!(
            BuildingTable::new(vec![b(1, &[]), b(1, &[])]).unwrap_err(),
            BuildingsError::DuplicateKind(Kind(1))
        );
        assert_eq!(
            with(&|s| s.footprint.width = 0),
            BuildingsError::EmptyFootprint(Kind(2))
        );
        assert_eq!(
            with(&|s| s.footprint.height = 0),
            BuildingsError::EmptyFootprint(Kind(2))
        );
        for function in [
            BuildingFunction::InfantryFactory,
            BuildingFunction::VehicleFactory,
            BuildingFunction::VesselFactory,
            BuildingFunction::HarvesterDock,
        ] {
            assert_eq!(
                with(&|s| s.function = function),
                BuildingsError::NoExitSide(Kind(2)),
                "{function:?}"
            );
        }
        assert_eq!(
            with(&|s| s.exit_side = Some(Side::PosX)),
            BuildingsError::UselessExitSide(Kind(2))
        );
        assert_eq!(
            with(&|s| s.requires = vec![Kind(9)]),
            BuildingsError::UnknownPrerequisite(Kind(2), Kind(9))
        );
        assert_eq!(
            with(&|s| s.build_ticks = 0),
            BuildingsError::CostAndTime(Kind(2))
        );
        assert_eq!(
            with(&|s| s.cost = None),
            BuildingsError::CostAndTime(Kind(2))
        );
        assert_eq!(
            with(&|s| {
                s.cost = None;
                s.build_ticks = 0;
                s.requires = vec![Kind(1)];
            }),
            BuildingsError::RequiresWithoutCost(Kind(2))
        );
        // A factory with a side is fine.
        let mut ok = b(2, &[]);
        ok.function = BuildingFunction::VehicleFactory;
        ok.exit_side = Some(Side::NegY);
        assert!(BuildingTable::new(vec![b(1, &[]), ok]).is_ok());
    }

    #[test]
    fn the_kind_behind_a_cycle_is_named_and_lookups_work() {
        // 1 needs 2, and 2 and 3 need each other: 1 hangs behind the cycle.
        assert_eq!(
            BuildingTable::new(vec![b(1, &[2]), b(2, &[3]), b(3, &[2])]).unwrap_err(),
            BuildingsError::Cycle(Kind(1))
        );
        let t = BuildingTable::new(vec![b(5, &[]), b(7, &[5])]).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t.get(Kind(7)).unwrap().requires, vec![Kind(5)]);
        assert!(t.get(Kind(6)).is_none());
        assert_eq!(t.iter().map(|s| s.kind.0).collect::<Vec<_>>(), [5, 7]);
    }
}
