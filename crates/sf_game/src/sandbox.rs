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
//! moves until it is ordered to (WP3.4) or something comes into reach: since WP5.5 the game's
//! world has weapons and its units shoot at whatever of another owner is in reach, so in a world
//! that has a weapon table the sides are placed out of each other's reach ([`APART`]) and a fight
//! starts when the player sends units across (or `--fight` does).
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

/// How far from the middle of the map a base is, in cells, in a world without weapons.
const BASE_OFFSET: i32 = 14;
/// How many cells farther out each side's base stands in a world that has a weapon table. Since WP5.5 the
/// game's sandbox fights (weapons, units that pick their own targets, destruction), so the two sides
/// must stand far enough apart that nobody is in reach of the other when it starts: the units of a side
/// gather `FRONT` cells in front of its base, so at `BASE_OFFSET + APART` (24) the nearest units of the
/// two sides are about 2 * (24 - 7) = 34 cells apart less the width of a gathering (around ten), against
/// the longest reach of a mobile unit of 10 cells (the howitzer). At the old 14 the two gatherings were
/// six cells apart and would have fired on each other on the first tick. A world without weapons keeps the
/// old layout, which the tests of formations in a base (`tests/formation_base.rs`) are measured on. A
/// test checks that nothing fires in the first minute.
const APART: i32 = 10;
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

/// The faction that sits at `owner`'s seat in the sandbox, from [`SIDES`] (the
/// one table of seats); `None` for any other owner number. The roster uses it
/// to pick the model of a unit by its owner.
pub fn faction_of_owner(owner: u8) -> Option<Faction> {
    SIDES
        .iter()
        .find(|side| side.owner == owner)
        .map(|side| side.faction)
}

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
        let anchor = base_of(side, middle, !world.weapon_table().is_empty());
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

/// Where a side's base is: its offset from the middle of the map, `APART` cells farther out in a world
/// that has weapons.
fn base_of(side: &Side, middle: (i32, i32), armed: bool) -> (i32, i32) {
    let out = if armed { side.at.0.signum() * APART } else { 0 };
    (middle.0 + side.at.0 + out, middle.1 + side.at.1)
}

/// Sends every armed mobile unit of the two sides on attack-move to the other side's base (`--fight`,
/// WP5.5): the sandbox then fights without anyone giving an order, which is how the viewer shows shots,
/// deaths, wrecks and rubble. Call it after the sandbox has been stepped once (the units exist); the orders
/// are for the next tick. Returns how many units were sent.
pub fn start_fight(world: &mut World) -> u32 {
    let (width, height) = world.map_size();
    let middle = (width as i32 / 2, height as i32 / 2);
    let mut sent = 0;
    for side in SIDES.iter().filter(|s| s.toward != 0) {
        let ids: Vec<_> = world
            .snapshot()
            .iter()
            .filter(|v| {
                v.owner == PlayerId(side.owner)
                    && world.stats_of(v.id).is_some_and(|s| {
                        s.class() == sf_sim::UnitClass::Mobile && s.weapon_range > Fx::ZERO
                    })
            })
            .map(|v| v.id)
            .collect();
        if ids.is_empty() {
            continue;
        }
        sent += ids.len() as u32;
        // The other side's base: it is on the other side of the middle.
        let (bx, by) = base_of(side, middle, !world.weapon_table().is_empty());
        let target = Vec2::from_ints(2 * middle.0 - bx, 2 * middle.1 - by);
        send(
            world,
            side.owner,
            Command::Order {
                ids,
                order: sf_sim::Order::AttackMove(target),
                queued: false,
            },
        );
    }
    sent
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

    #[test]
    fn every_seat_of_the_table_has_its_faction_and_no_other_owner_has_one() {
        for side in &SIDES {
            assert_eq!(faction_of_owner(side.owner), Some(side.faction));
        }
        let seats: Vec<u8> = SIDES.iter().map(|s| s.owner).collect();
        for owner in 0..=u8::MAX {
            if !seats.contains(&owner) {
                assert_eq!(faction_of_owner(owner), None, "owner {owner}");
            }
        }
    }

    fn world_for(cells: u32, seed: u32) -> (World, UnitDefs) {
        let defs = UnitDefs::embedded();
        let map = generate_cells(cells, cells, seed);
        let config = WorldConfig::new(1).with_map_size(cells, cells);
        let world = make_world(config, &map)
            .unwrap()
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap();
        // With the movement costs, as the game runs it: the spawn refusal of
        // WP4.1 (a unit on ground its class cannot enter) is then in play.
        let world = crate::playfield::with_movement(world, &defs);
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
    fn the_sandbox_does_not_attack_itself_once_the_world_has_weapons() {
        // WP5.5: the game's world fires, chooses targets and destroys. Left alone, nothing of the
        // sandbox is in reach of anything of another side, for a minute of game time, on several maps.
        for seed in [1, 2, 7] {
            let (world, defs) = world_for(256, seed);
            let mut world = crate::playfield::with_combat(world, &defs);
            assert!(world.destruction_config().is_some(), "the world has combat");
            schedule(&mut world, &defs);
            world.step();
            let placed = world.snapshot().len();
            let armed = world
                .snapshot()
                .iter()
                .filter(|v| !world.weapon_table().loadout(v.kind).is_empty())
                .count();
            assert!(armed > 10, "there are armed units: {armed}");
            let mut shots = 0;
            for _ in 0..1200 {
                let report = world.step();
                shots += report.shots;
                assert_eq!(report.deaths, 0, "seed {seed}");
            }
            assert_eq!(shots, 0, "seed {seed}: nobody fires");
            assert_eq!(world.snapshot().len(), placed, "seed {seed}");
            assert!(world.wrecks().is_empty());
        }
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
