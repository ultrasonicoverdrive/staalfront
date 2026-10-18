//! The `units_spawn` scenario (WP3.2): every role of its frozen unit definitions is
//! spawned on the 256 by 256 cell playfield, and the hit points, the footprints
//! in the occupancy grid and the refusals are asserted.
//!
//! The roles are those of the frozen copy in `assets/scenarios/units_spawn_defs/`
//! (43 of them: 19 structures, 22 mobile units and 2 objects, as the shipped file
//! had them in WP3.2; `shipped_roles.rs` covers the shipped file itself); the cells are those the
//! scenario file names, on the terrain layer `assets/maps/playfield256`.

mod common;

use std::collections::BTreeSet;

use common::scenario_path;
use sf_data::units::UnitDefs;
use sf_headless::scenario::{Scenario, ScenarioError};
use sf_math::{Fx, Vec2};
use sf_sim::{Cell, EntityId, Footprint, Kind, PlayerId, StepReport, TerrainId, UnitClass, World};

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("units_spawn.ron")).unwrap()
}

/// The world after `ticks` ticks and the report of every one of them.
fn run(ticks: u64) -> (World, Vec<StepReport>) {
    let mut world = scenario().build_world().unwrap();
    let reports = (0..ticks).map(|_| world.step()).collect();
    (world, reports)
}

/// The frozen definitions the scenario names (`unit_defs`), not the shipped
/// ones: the scenario's checksum must not move with a balance change.
fn defs() -> UnitDefs {
    let scenarios = scenario_path("");
    UnitDefs::load(&scenarios.join("units_spawn_defs"), &scenarios.join("..")).unwrap()
}

/// Slot of the entity of `kind` (the scenario spawns each role once).
fn id_of_kind(world: &World, kind: u16) -> EntityId {
    let found: Vec<EntityId> = world
        .snapshot()
        .iter()
        .filter(|v| v.kind == Kind(kind))
        .map(|v| v.id)
        .collect();
    assert_eq!(found.len(), 1, "kind {kind} should exist exactly once");
    found[0]
}

#[test]
fn the_scenario_is_the_playfield_with_the_shipped_definitions() {
    let s = scenario();
    assert_eq!((s.map.width, s.map.height), (256, 256));
    assert_eq!(s.unit_defs.as_deref(), Some("units_spawn_defs"));
    assert!(s.terrain.as_ref().unwrap().has_layer());
    let world = s.build_world().unwrap();
    assert_eq!(world.unit_table().len(), defs().units().len());
    assert_eq!(world.map_size(), (256, 256));
}

#[test]
fn every_role_of_the_definitions_is_spawned_once_with_its_hit_points() {
    let (world, _) = run(1);
    let defs = defs();
    let kinds_present: BTreeSet<u16> = world.snapshot().iter().map(|v| v.kind.0).collect();
    let kinds_defined: BTreeSet<u16> = defs.units().iter().map(|u| u.kind).collect();
    assert_eq!(
        kinds_present, kinds_defined,
        "units_spawn.ron must spawn every role of its frozen definitions \
         (assets/scenarios/units_spawn_defs)"
    );
    assert_eq!(world.len(), defs.units().len());
    for view in world.snapshot().iter() {
        let role = defs.sim_unit(view.kind.0).unwrap();
        let name = &defs.unit_by_kind(view.kind.0).unwrap().id;
        assert!(view.has_stats, "{name}");
        assert_eq!(view.hp, role.hp, "{name}");
        assert_eq!(world.hp_of(view.id), Some(role.hp), "{name}");
    }
}

#[test]
fn the_sim_has_the_stats_of_the_converted_roles() {
    let (world, _) = run(1);
    let table = sf_bridge::unit_table(&defs()).unwrap();
    assert_eq!(world.unit_table(), &table);
    let defs = defs();
    for role in defs.sim_units() {
        let id = id_of_kind(&world, role.kind);
        let stats = world.stats_of(id).unwrap();
        let name = &defs.unit_by_kind(role.kind).unwrap().id;
        assert_eq!(stats.kind, Kind(role.kind), "{name}");
        assert_eq!(stats.hp, role.hp, "{name}");
        assert_eq!(stats.movement_class, role.movement_class, "{name}");
        assert_eq!(stats.speed_per_tick, role.speed_per_tick, "{name}");
        assert_eq!(stats.turn_per_tick, role.turn_per_tick, "{name}");
        assert_eq!(stats.size, role.size, "{name}");
        assert_eq!(
            stats.footprint,
            role.footprint
                .map(|(width, height)| Footprint { width, height }),
            "{name}"
        );
        // The class follows the data: a movement class, a footprint or neither.
        let class = match (role.movement_class, role.footprint) {
            (Some(_), None) => UnitClass::Mobile,
            (None, Some(_)) => UnitClass::Structure,
            (None, None) => UnitClass::Object,
            (Some(_), Some(_)) => unreachable!("the definitions refuse it"),
        };
        assert_eq!(stats.class(), class, "{name}");
    }
}

#[test]
fn structures_occupy_exactly_the_cells_of_their_footprint() {
    let (world, _) = run(1);
    let defs = defs();
    let mut claimed = BTreeSet::new();
    for view in world.snapshot().iter() {
        let role = defs.sim_unit(view.kind.0).unwrap();
        let name = &defs.unit_by_kind(view.kind.0).unwrap().id;
        let Some((w, h)) = role.footprint else {
            // Units and objects occupy no cell.
            assert_eq!(world.cell_of(view.id), None, "{name}");
            assert_eq!(world.footprint_of(view.id), None, "{name}");
            continue;
        };
        let anchor = world.cell_of(view.id).expect(name);
        assert_eq!(
            world.footprint_of(view.id),
            Some(Footprint {
                width: w,
                height: h
            }),
            "{name}"
        );
        for dy in 0..u32::from(h) {
            for dx in 0..u32::from(w) {
                let cell = Cell::new(anchor.x + dx, anchor.y + dy);
                assert_eq!(world.occupant_at(cell), Some(view.id), "{name} {cell}");
                assert!(claimed.insert(cell), "{name}: {cell} is claimed twice");
                // Only buildable ground is built on.
                let flags = world.terrain().flags_at(cell).unwrap();
                assert!(flags.buildable, "{name} {cell}");
            }
        }
        // The middle of the footprint.
        let middle = Vec2::new(
            Fx::from_int(anchor.x as i32) + Fx::from_ratio(i64::from(w), 2),
            Fx::from_int(anchor.y as i32) + Fx::from_ratio(i64::from(h), 2),
        );
        assert_eq!(view.pos, middle, "{name}");
    }
    // Nothing else is occupied: not by a unit, not by a refused placement.
    assert_eq!(world.occupied_cells(), claimed.len());
    let footprint_cells: usize = defs
        .sim_units()
        .iter()
        .filter_map(|u| u.footprint)
        .map(|(w, h)| usize::from(w) * usize::from(h))
        .sum();
    assert_eq!(claimed.len(), footprint_cells);
    let mut occupied = 0;
    for y in 0..256 {
        for x in 0..256 {
            if world.is_occupied(Cell::new(x, y)) {
                occupied += 1;
            }
        }
    }
    assert_eq!(occupied, footprint_cells);
}

#[test]
fn the_refusals_create_nothing() {
    let (_, reports) = run(400);
    let at = |tick: usize| {
        (
            reports[tick].commands_executed,
            reports[tick].commands_ignored,
        )
    };
    // Tick 0 creates all 43 roles.
    assert_eq!(at(0), (43, 0));
    // Tick 1: the thirteen refusals, none of them changes anything.
    assert_eq!(at(1), (0, 13));
    // Tick 10: the two units are told to walk; tick 50 and 60: the barracks
    // goes and comes back; tick 70: the first construction center is in the way.
    assert_eq!(at(10), (1, 0));
    assert_eq!(at(50), (1, 0));
    assert_eq!(at(60), (1, 0));
    assert_eq!(at(70), (0, 1));
    let busy: usize = reports
        .iter()
        .enumerate()
        .filter(|(_, r)| r.commands_executed + r.commands_ignored > 0)
        .count();
    assert_eq!(busy, 6);

    // Right after tick 1: still the same 43 entities, and the cells the
    // refused commands aimed at are still free.
    let (world, _) = run(2);
    assert_eq!(world.len(), 43);
    assert_eq!(world.slot_count(), 43, "a refusal spends no slot");
    for (x, y) in [
        // the power plant over road, bridge and water ...
        (125, 125),
        (126, 125),
        (125, 126),
        (126, 126),
        (125, 127),
        (126, 127),
        // ... the tower on the ore field ...
        (199, 0),
        // ... the plant that overlapped the construction center, beyond it ...
        (12, 62),
        (12, 63),
        (11, 64),
        (12, 64),
        // ... and the free cells a rejected kind pointed at.
        (100, 100),
        (100, 112),
        (100, 116),
        (255, 100),
        (100, 254),
    ] {
        assert!(!world.is_occupied(Cell::new(x, y)), "({x}, {y})");
    }
    // The first construction center kept its cell and its owner against the
    // tower that was refused on its corner, and the unit that would have
    // appeared in it.
    let cc = EntityId::new(0, 0);
    assert_eq!(world.occupant_at(Cell::new(8, 60)), Some(cc));
    assert_eq!(world.get(cc).unwrap().owner, PlayerId(0));
    assert_eq!(world.get(cc).unwrap().kind, Kind(0));
}

#[test]
fn a_structure_that_goes_away_frees_all_its_cells() {
    let mut world = scenario().build_world().unwrap();
    for _ in 0..50 {
        world.step();
    }
    let barracks = EntityId::new(3, 0);
    let cells: Vec<Cell> = {
        let anchor = world.cell_of(barracks).unwrap();
        let fp = world.footprint_of(barracks).unwrap();
        (0..u32::from(fp.height))
            .flat_map(|dy| (0..u32::from(fp.width)).map(move |dx| (dx, dy)))
            .map(|(dx, dy)| Cell::new(anchor.x + dx, anchor.y + dy))
            .collect()
    };
    assert_eq!(cells.len(), 6, "a barracks covers 2 x 3 cells");
    assert!(
        cells
            .iter()
            .all(|&c| world.occupant_at(c) == Some(barracks))
    );
    // Tick 50 despawns it.
    world.step();
    assert!(!world.is_alive(barracks));
    assert!(cells.iter().all(|&c| !world.is_occupied(c)));
    // Tick 60: Veyran builds there; the slot comes back as generation 1.
    for _ in 51..=60 {
        world.step();
    }
    let again = EntityId::new(3, 1);
    assert!(
        cells.iter().all(|&c| world.occupant_at(c) == Some(again)),
        "all six cells belong to the new barracks"
    );
    let view = world.get(again).unwrap();
    assert_eq!((view.kind, view.owner), (Kind(3), PlayerId(1)));
    assert_eq!(view.hp, 700);
}

#[test]
fn units_walk_at_their_own_speed_and_stop_at_the_rock() {
    let (world, _) = run(400);
    let defs = defs();
    let rifleman = id_of_kind(&world, defs.kind_of("rifleman").unwrap());
    let tank = id_of_kind(&world, defs.kind_of("light_tank").unwrap());
    // Both are sent by the legacy MoveTo to the cell (136, 90), which is rock.
    // Terrain costs (WP4.1) made that a new rule for them: neither enters rock,
    // each stands at its edge with the move kept. (Before WP4.1 the tank, 2 cells
    // a second, arrived and the rifleman, 0.7, was on the way.)
    let target = Vec2::from_ints(136, 90);
    let rock = world.terrain_at(Cell::new(136, 90)).unwrap();
    assert_eq!(rock, TerrainId(3), "the target cell is rock");
    for id in [tank, rifleman] {
        assert_eq!(world.waypoint_of(id), Some(target), "the move is kept");
    }
    // The tank is at the south edge of the rock cell, less than a step short.
    let tank_pos = world.get(tank).unwrap().pos;
    assert_eq!(world.cell_at(tank_pos), Some(Cell::new(135, 89)));
    assert!(Fx::from_int(90) - tank_pos.y < sf_sim::per_tick(Fx::from_int(2)));
    // The rifleman walked into a rock patch on the way (columns 138 to 141, rows
    // 59 to 62) and stands at its east edge.
    let rifle = world.get(rifleman).unwrap();
    let cell = world.cell_at(rifle.pos).unwrap();
    assert_eq!(cell, Cell::new(142, 62));
    assert_eq!(world.terrain_at(Cell::new(141, 62)), Some(TerrainId(3)));
    assert!(rifle.pos.x - Fx::from_int(142) < sf_sim::per_tick(Fx::from_ratio(7, 10)));
    // Each walked at its own speed: the tank covered far more ground.
    let start_tank = Vec2::new(Fx::from_ratio(277, 2), Fx::from_ratio(129, 2));
    let start_rifle = Vec2::new(Fx::from_ratio(285, 2), Fx::from_ratio(121, 2));
    assert!((tank_pos - start_tank).length_squared() > (rifle.pos - start_rifle).length_squared());
    // Everything else stands where it was put: nobody wanders.
    for v in world.snapshot().iter() {
        if v.id != rifleman && v.id != tank {
            assert_eq!(v.pos, v.prev_pos, "{}", v.kind.0);
        }
    }
}

#[test]
fn the_same_scenario_gives_the_same_checksums() {
    let (a, ra) = run(400);
    let (b, rb) = run(400);
    assert_eq!(a, b);
    assert_eq!(
        ra.iter().map(|r| r.checksum).collect::<Vec<_>>(),
        rb.iter().map(|r| r.checksum).collect::<Vec<_>>()
    );
}

#[test]
fn the_unit_actions_need_the_definitions() {
    let text = "Scenario(seed: 1, ticks: 5, map: (width: 8, height: 8), commands: [\
        (tick: 0, command: SpawnUnit(kind: 12, owner: 0, cell: (1, 1))), \
        (tick: 0, command: PlaceStructure(kind: 3, owner: 0, cell: (2, 2))) ])";
    let Err(ScenarioError::Invalid { problems, .. }) = Scenario::from_ron_str(text) else {
        panic!("a scenario without unit_defs must be refused");
    };
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems.iter().all(|p| p.contains("unit_defs")),
        "{problems:?}"
    );
    // With the name it parses, but the files are read by Scenario::load only.
    let ok = text.replace(
        "seed: 1,",
        "seed: 1, unit_defs: Some(\"units_spawn_defs\"),",
    );
    let s = Scenario::from_ron_str(&ok).unwrap();
    let Err(ScenarioError::Invalid { problems, .. }) = s.build_world() else {
        panic!("the definitions cannot be read from text");
    };
    assert!(problems[0].contains("Scenario::load"), "{problems:?}");
    // The cells must be on the map, and the name is one directory.
    let outside = ok.replace("cell: (1, 1)", "cell: (8, 1)");
    assert!(Scenario::from_ron_str(&outside).is_err());
    for bad in ["a/b", "..", "C:x"] {
        let t = text.replace("seed: 1,", &format!("seed: 1, unit_defs: Some(\"{bad}\"),"));
        assert!(Scenario::from_ron_str(&t).is_err(), "{bad}");
    }
}

#[test]
fn a_missing_definitions_directory_is_an_error_naming_it() {
    let dir = std::env::temp_dir().join(format!("sf_units_defs_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("s.ron");
    std::fs::write(
        &file,
        "Scenario(seed: 1, ticks: 5, map: (width: 8, height: 8), unit_defs: Some(\"nope\"))",
    )
    .unwrap();
    let err = Scenario::load(&file).unwrap_err().to_string();
    std::fs::remove_dir_all(&dir).ok();
    assert!(err.contains("unit_defs \"nope\""), "{err}");
}
