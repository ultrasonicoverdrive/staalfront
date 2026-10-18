//! The sandbox: what `sf_game` puts on the playfield when it starts (WP3.2).
//!
//! Every structure, unit and object of the unit definitions, once for each
//! side that can have it, spawned through the command queue on tick 0 with
//! `Command::PlaceStructure` and `Command::SpawnUnit`. Nothing about *which*
//! roles is written here: a side gets the roles whose `factions` name it, so a
//! role added to `assets/config/unit_defs.ron` shows up with no change to the
//! code (quality rule 6). Meridian (player 0) stands west of the middle of the
//! map, Veyran (player 1) east of it and the neutral roles (player 2) north of
//! it; those three seats are the only layout that is written here. Nothing
//! moves: orders are WP3.4.
//!
//! Where things go is decided by the map, deterministically: a structure takes
//! the free spot nearest to its base's anchor where every cell of its footprint
//! (and a cell of room around it) is buildable, a unit takes the free cell of
//! open ground nearest to the front of its base, on a lattice of two cells.
//! The same map always gives the same sandbox.
//!
//! Vessels are left out: they need water to stand on, and the sandbox does not
//! look for any (the sim could now answer, `World::passable`, WP4.1; a vessel
//! in the sandbox is a change of its own).

use std::collections::BTreeSet;

use sf_data::units::{Category, Faction, UnitDef, UnitDefs};
use sf_math::{Fx, Vec2};
use sf_sim::{Cell, Command, Kind, PlayerId, World};

/// The number of the player who owns Meridian's things.
pub const MERIDIAN: u8 = 0;
/// The number of the player who owns Veyran's things.
pub const VEYRAN: u8 = 1;
/// The owner of the neutral roles (colour three of the palette).
pub const NEUTRAL: u8 = 2;

/// How far from the middle of the map a base is, in cells.
const BASE_OFFSET: i32 = 14;
/// How far in front of its structures a side's units gather, in cells.
const FRONT: i32 = 7;

/// One side: its owner, the roles it gets and its place on the map.
struct Side {
    faction: Faction,
    owner: u8,
    /// Which way the front is: `1` east, `-1` west, `0` neither.
    toward: i32,
    /// Offset of the base anchor from the middle of the map, in cells.
    at: (i32, i32),
}

const SIDES: [Side; 3] = [
    Side {
        faction: Faction::Meridian,
        owner: MERIDIAN,
        toward: 1,
        at: (-BASE_OFFSET, 0),
    },
    Side {
        faction: Faction::Veyran,
        owner: VEYRAN,
        toward: -1,
        at: (BASE_OFFSET, 0),
    },
    Side {
        faction: Faction::Neutral,
        owner: NEUTRAL,
        toward: 0,
        at: (0, -16),
    },
];

/// Whether `side` gets this role: its faction is among the role's, and it is
/// not a vessel.
fn belongs_to(def: &UnitDef, side: &Side) -> bool {
    def.category != Category::Vessel && def.factions.contains(&side.faction)
}

/// How many structures and units (and objects) the definitions give the
/// sandbox: what [`schedule`] sets out to place on a map with room for it.
pub fn wanted(defs: &UnitDefs) -> (u32, u32) {
    let mut count = (0, 0);
    for side in &SIDES {
        for def in defs.units().iter().filter(|d| belongs_to(d, side)) {
            if def.category == Category::Structure {
                count.0 += 1;
            } else {
                count.1 += 1;
            }
        }
    }
    count
}

/// What was scheduled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// `PlaceStructure` commands scheduled.
    pub structures: u32,
    /// `SpawnUnit` commands scheduled.
    pub units: u32,
    /// Roles for which the map had no room: not scheduled.
    pub skipped: u32,
}

/// Cells the sandbox has already promised, structures with a cell of room.
struct Taken(BTreeSet<(i32, i32)>);

/// Schedules the sandbox on tick 0 of `world`, which must have been made with
/// the unit table of `defs`. Nothing runs until the world is stepped.
pub fn schedule(world: &mut World, defs: &UnitDefs) -> Plan {
    let (width, height) = world.map_size();
    let middle = (width as i32 / 2, height as i32 / 2);
    let mut taken = Taken(BTreeSet::new());
    let mut plan = Plan::default();
    for side in &SIDES {
        let anchor = (middle.0 + side.at.0, middle.1 + side.at.1);
        let front = (anchor.0 + side.toward * FRONT, anchor.1 + 2 + FRONT / 3);
        let roles = defs.units().iter().filter(|d| belongs_to(d, side));
        // Structures first, so units are placed around them; both in kind order.
        let (structures, others): (Vec<&UnitDef>, Vec<&UnitDef>) =
            roles.partition(|d| d.category == Category::Structure);
        for def in structures {
            let footprint = defs.sim_unit(def.kind).and_then(|u| u.footprint);
            let spot = footprint.and_then(|(w, h)| find_structure(world, &taken, anchor, w, h));
            let Some((cell, (w, h))) = spot.zip(footprint) else {
                plan.skipped += 1;
                continue;
            };
            // The footprint and a cell of room all around it.
            for y in cell.1 - 1..cell.1 + i32::from(h) + 1 {
                for x in cell.0 - 1..cell.0 + i32::from(w) + 1 {
                    taken.0.insert((x, y));
                }
            }
            send(
                world,
                side.owner,
                Command::PlaceStructure {
                    kind: Kind(def.kind),
                    cell: Cell::new(cell.0 as u32, cell.1 as u32),
                    owner: PlayerId(side.owner),
                },
            );
            plan.structures += 1;
        }
        for def in others {
            let Some(cell) = find_unit(world, &taken, front) else {
                plan.skipped += 1;
                continue;
            };
            taken.0.insert(cell);
            let pos = Vec2::new(
                Fx::from_int(cell.0) + Fx::HALF,
                Fx::from_int(cell.1) + Fx::HALF,
            );
            send(
                world,
                side.owner,
                Command::SpawnUnit {
                    kind: Kind(def.kind),
                    pos,
                    owner: PlayerId(side.owner),
                },
            );
            plan.units += 1;
        }
    }
    plan
}

fn send(world: &mut World, player: u8, command: Command) {
    world
        .schedule(world.tick(), PlayerId(player), command)
        .expect("the tick that is about to run is not in the past");
}

/// Cells at distance `r` (the chessboard distance) from `c`, in a fixed order.
fn ring(c: (i32, i32), r: i32) -> impl Iterator<Item = (i32, i32)> {
    let side = 2 * r + 1;
    (0..side * side)
        .map(move |i| (c.0 - r + i % side, c.1 - r + i / side))
        .filter(move |&(x, y)| (x - c.0).abs().max((y - c.1).abs()) == r)
}

/// The nearest spot for a `w` x `h` structure with a cell of room around it:
/// every cell buildable, on the map and not promised.
fn find_structure(
    world: &World,
    taken: &Taken,
    near: (i32, i32),
    w: u8,
    h: u8,
) -> Option<(i32, i32)> {
    let ok = |x0: i32, y0: i32| {
        (y0 - 1..y0 + i32::from(h) + 1).all(|y| {
            (x0 - 1..x0 + i32::from(w) + 1)
                .all(|x| !taken.0.contains(&(x, y)) && buildable(world, x, y))
        })
    };
    (0..64).find_map(|r| ring(near, r).find(|&(x, y)| ok(x, y)))
}

/// The nearest free cell of open ground on the two-cell lattice of `near`.
fn find_unit(world: &World, taken: &Taken, near: (i32, i32)) -> Option<(i32, i32)> {
    (0..64).find_map(|r| {
        ring(near, r).find(|&(x, y)| {
            (x - near.0) % 2 == 0
                && (y - near.1) % 2 == 0
                && !taken.0.contains(&(x, y))
                && open_ground(world, x, y)
        })
    })
}

fn flags_at(world: &World, x: i32, y: i32) -> Option<sf_sim::TerrainFlags> {
    let cell = Cell::new(u32::try_from(x).ok()?, u32::try_from(y).ok()?);
    world.terrain().flags_at(cell)
}

fn buildable(world: &World, x: i32, y: i32) -> bool {
    flags_at(world, x, y).is_some_and(|f| f.buildable)
}

/// Passable for every kind of ground unit: foot, wheeled and tracked (the
/// `ground` summary of the terrain, derived from the movement costs).
fn open_ground(world: &World, x: i32, y: i32) -> bool {
    flags_at(world, x, y).is_some_and(|f| f.ground)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playfield::make_world;
    use sf_data::map::generate::generate_cells;
    use sf_sim::WorldConfig;

    fn world_for(cells: u32, seed: u32) -> (World, UnitDefs) {
        let defs = UnitDefs::embedded();
        let map = generate_cells(cells, cells, seed);
        let config = WorldConfig::new(1).with_map_size(cells, cells);
        let world = make_world(config, &map)
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap();
        (world, defs)
    }

    #[test]
    fn the_sandbox_is_all_placed_and_nothing_is_refused() {
        for seed in [1, 2, 7] {
            let (mut world, defs) = world_for(256, seed);
            let plan = schedule(&mut world, &defs);
            assert_eq!(plan.skipped, 0, "seed {seed}");
            let (structures, units) = wanted(&defs);
            assert_eq!(
                (plan.structures, plan.units),
                (structures, units),
                "seed {seed}"
            );
            let report = world.step();
            assert_eq!(report.commands_ignored, 0, "seed {seed}");
            assert_eq!(report.commands_executed, structures + units, "seed {seed}");
            assert_eq!(world.len() as u32, structures + units);
        }
    }

    #[test]
    fn every_side_has_structures_and_units_of_its_own() {
        let (mut world, defs) = world_for(256, 1);
        schedule(&mut world, &defs);
        world.step();
        for owner in [MERIDIAN, VEYRAN, NEUTRAL] {
            let mine: Vec<_> = world
                .snapshot()
                .iter()
                .filter(|v| v.owner == PlayerId(owner))
                .collect();
            let structures = mine
                .iter()
                .filter(|v| world.footprint_of(v.id).is_some())
                .count();
            assert!(structures >= 3, "owner {owner}: {structures} structures");
            assert!(mine.len() - structures >= 5, "owner {owner}");
            assert!(mine.iter().all(|v| v.has_stats));
        }
    }

    #[test]
    fn the_sandbox_is_deterministic_and_stands_still() {
        let run = || {
            let (mut world, defs) = world_for(256, 1);
            schedule(&mut world, &defs);
            for _ in 0..40 {
                world.step();
            }
            world
        };
        let (a, b) = (run(), run());
        assert_eq!(a.checksum_history(), b.checksum_history());
        for v in a.snapshot().iter() {
            assert_eq!(v.pos, v.prev_pos, "nothing moves without orders");
        }
    }

    #[test]
    fn a_small_map_skips_what_does_not_fit_and_refuses_nothing() {
        let (mut world, defs) = world_for(32, 3);
        let plan = schedule(&mut world, &defs);
        let report = world.step();
        assert_eq!(report.commands_ignored, 0);
        assert_eq!(
            plan.structures + plan.units,
            report.commands_executed,
            "everything scheduled stands"
        );
        assert!(plan.structures + plan.units > 0);
    }

    #[test]
    fn units_stand_clear_of_structures() {
        let (mut world, defs) = world_for(256, 1);
        schedule(&mut world, &defs);
        world.step();
        for v in world.snapshot().iter() {
            if world.footprint_of(v.id).is_none() {
                let cell = world.cell_at(v.pos).unwrap();
                assert!(!world.is_occupied(cell), "{} in a structure", v.kind.0);
            }
        }
    }
}
