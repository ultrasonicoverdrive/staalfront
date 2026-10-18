//! Every role of the **shipped** unit definitions (`assets/config/unit_defs.ron`)
//! can be spawned (WP3.2), with no checksum.
//!
//! The `units_spawn` scenario is pinned by a checksum and therefore uses a
//! frozen copy of the definitions (a balance tweak must not force a re-pin).
//! This test is the other half: the roles are placed automatically from
//! whatever the shipped file holds, so a role added or changed there is tried
//! without anyone editing a scenario or a pinned value.
//!
//! Placement: a structure goes on the first anchor (scanning the map row by
//! row) where its footprint and a cell of room around it are buildable and
//! free; a vessel on a water cell; every other role on a free cell of open
//! ground, two cells apart.

mod common;

use std::collections::BTreeSet;

use common::scenario_path;
use sf_data::terrain::TerrainTypes;
use sf_data::units::{Category, UnitDefs};
use sf_headless::scenario::Scenario;
use sf_math::{Fx, Vec2};
use sf_sim::{Cell, Command, Footprint, Kind, PlayerId, World, WorldConfig};

const SIDE: i32 = 256;

fn playfield() -> (World, UnitDefs) {
    let terrain = Scenario::load(&scenario_path("terrain_256.ron"))
        .unwrap()
        .terrain
        .expect("terrain_256 has a terrain layer");
    let defs = UnitDefs::embedded();
    let world = World::with_terrain(WorldConfig::new(9), terrain)
        .unwrap()
        .with_units(sf_bridge::unit_table(&defs).unwrap())
        .unwrap();
    (world, defs)
}

/// Schedules every role of `defs` on tick 0, placed automatically.
fn place_everything(world: &mut World, defs: &UnitDefs) {
    let water = TerrainTypes::embedded().id_of("water").unwrap();
    let all_cells = || (0..SIDE).flat_map(|y| (0..SIDE).map(move |x| (x, y)));
    let flags = |w: &World, x: i32, y: i32| {
        (x >= 0 && y >= 0)
            .then(|| w.terrain().flags_at(Cell::new(x as u32, y as u32)))
            .flatten()
    };
    let mut taken: BTreeSet<(i32, i32)> = BTreeSet::new();
    let send = |w: &mut World, c: Command| {
        w.schedule(w.tick(), PlayerId(0), c).unwrap();
    };
    for def in defs.units() {
        let role = defs.sim_unit(def.kind).unwrap();
        if let Some((fw, fh)) = role.footprint {
            let (fw, fh) = (i32::from(fw), i32::from(fh));
            let anchor = all_cells()
                .find(|&(x0, y0)| {
                    (y0 - 1..y0 + fh + 1).all(|y| {
                        (x0 - 1..x0 + fw + 1).all(|x| {
                            !taken.contains(&(x, y))
                                && flags(world, x, y).is_some_and(|f| f.buildable)
                        })
                    })
                })
                .unwrap_or_else(|| panic!("{}: no room for a {fw} x {fh} structure", def.id));
            for y in anchor.1 - 1..anchor.1 + fh + 1 {
                for x in anchor.0 - 1..anchor.0 + fw + 1 {
                    taken.insert((x, y));
                }
            }
            send(
                world,
                Command::PlaceStructure {
                    kind: Kind(def.kind),
                    cell: Cell::new(anchor.0 as u32, anchor.1 as u32),
                    owner: PlayerId(0),
                },
            );
        } else {
            let on_water = def.category == Category::Vessel;
            let cell = all_cells()
                .find(|&(x, y)| {
                    (x % 2, y % 2) == (0, 0)
                        && !taken.contains(&(x, y))
                        && if on_water {
                            world.terrain_at(Cell::new(x as u32, y as u32))
                                == Some(sf_sim::TerrainId(water))
                        } else {
                            flags(world, x, y).is_some_and(|f| f.foot && f.wheeled && f.tracked)
                        }
                })
                .unwrap_or_else(|| panic!("{}: no cell for it", def.id));
            taken.insert(cell);
            send(
                world,
                Command::SpawnUnit {
                    kind: Kind(def.kind),
                    pos: Vec2::new(
                        Fx::from_int(cell.0) + Fx::HALF,
                        Fx::from_int(cell.1) + Fx::HALF,
                    ),
                    owner: PlayerId(0),
                },
            );
        }
    }
}

#[test]
fn every_role_of_the_shipped_definitions_spawns() {
    let (mut world, defs) = playfield();
    place_everything(&mut world, &defs);
    let report = world.step();
    let roles = defs.units().len() as u32;
    assert_eq!(
        (report.commands_executed, report.commands_ignored),
        (roles, 0),
        "every role must be accepted"
    );
    assert_eq!(world.len() as u32, roles);

    let mut kinds = BTreeSet::new();
    let mut claimed = BTreeSet::new();
    for view in world.snapshot().iter() {
        let role = defs.sim_unit(view.kind.0).unwrap();
        let name = &defs.unit_by_kind(view.kind.0).unwrap().id;
        assert!(kinds.insert(view.kind.0), "{name} twice");
        assert!(view.has_stats, "{name}");
        assert_eq!(view.hp, role.hp, "{name}");
        match role.footprint {
            Some((w, h)) => {
                assert_eq!(
                    world.footprint_of(view.id),
                    Some(Footprint {
                        width: w,
                        height: h
                    }),
                    "{name}"
                );
                let anchor = world.cell_of(view.id).unwrap();
                for dy in 0..u32::from(h) {
                    for dx in 0..u32::from(w) {
                        let cell = Cell::new(anchor.x + dx, anchor.y + dy);
                        assert_eq!(world.occupant_at(cell), Some(view.id), "{name} {cell}");
                        claimed.insert(cell);
                    }
                }
            }
            None => assert_eq!(world.cell_of(view.id), None, "{name}"),
        }
    }
    assert_eq!(
        kinds,
        defs.units().iter().map(|u| u.kind).collect::<BTreeSet<_>>()
    );
    assert_eq!(world.occupied_cells(), claimed.len());
}

#[test]
fn the_automatic_placement_is_deterministic() {
    // Placement is deterministic: the same checksums twice.
    let run = || {
        let (mut world, defs) = playfield();
        place_everything(&mut world, &defs);
        for _ in 0..20 {
            world.step();
        }
        world.checksum_history().to_vec()
    };
    assert_eq!(run(), run());
}
