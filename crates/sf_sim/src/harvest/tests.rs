//! Tests that need to reach into the world: a structure at zero hit points does not stand, so it
//! is no refinery (`World::is_standing`, the rule WP6.3 extends with "complete"). The public API
//! cannot put a structure at zero hit points without a weapon, so these set the column.

use super::*;
use crate::buildings::{BuildingStats, BuildingTable};
use crate::ids::per_tick;
use crate::terrain::{Terrain, TerrainDef, TerrainFlags, TerrainId};
use crate::units::{Footprint, UnitStats, UnitTable};
use crate::world::WorldConfig;
use crate::{Command, Tick};

const REFINERY: Kind = Kind(2);
const HARVESTER: Kind = Kind(11);

const RULES: HarvestRules = HarvestRules {
    capacity: 100,
    mine_amount: 5,
    mine_period: 4,
    unload_amount: 20,
    unload_period: 3,
    search_radius: 40,
    retry_period: 20,
    call_range: 6,
    grant_timeout: 200,
};

fn world() -> World {
    let defs = vec![
        TerrainDef {
            id: TerrainId(0),
            flags: TerrainFlags::OPEN,
        },
        TerrainDef {
            id: TerrainId(4),
            flags: TerrainFlags {
                buildable: false,
                harvestable: true,
                ground: true,
            },
        },
    ];
    let ids: Vec<u8> = (0..48u32)
        .flat_map(|y| (0..64u32).map(move |x| (x, y)))
        .map(|(x, y)| u8::from((24..28).contains(&x) && (18..22).contains(&y)) * 4)
        .collect();
    let terrain = Terrain::from_layer(64, 48, &ids, &defs).unwrap();
    let units = UnitTable::new(vec![
        UnitStats {
            kind: REFINERY,
            hp: 900,
            movement_class: None,
            speed_per_tick: Fx::ZERO,
            turn_per_tick: 0,
            size: Fx::ZERO,
            footprint: Some(Footprint {
                width: 3,
                height: 4,
            }),
            weapon_range: Fx::ZERO,
            turret_per_tick: 0,
        },
        UnitStats {
            kind: HARVESTER,
            hp: 800,
            movement_class: Some(1),
            speed_per_tick: per_tick(Fx::from_ratio(12, 10)),
            turn_per_tick: 6,
            size: Fx::from_ratio(3, 2),
            footprint: None,
            weapon_range: Fx::ZERO,
            turret_per_tick: 0,
        },
    ])
    .unwrap();
    let buildings = BuildingTable::new(vec![BuildingStats {
        kind: REFINERY,
        footprint: Footprint {
            width: 3,
            height: 4,
        },
        function: BuildingFunction::HarvesterDock,
        exit_side: Some(Side::PosX),
        build_ticks: 800,
        cost: Some(1200),
        power: -30,
        factions: 3,
        requires: vec![],
        build_radius: 0,
    }])
    .unwrap();
    World::with_terrain(WorldConfig::new(7).with_map_size(64, 48), terrain)
        .unwrap()
        .with_units(units)
        .unwrap()
        .with_buildings(buildings)
        .unwrap()
        .with_ore(crate::OreRules {
            start: 100,
            max: 100,
            regrow_amount: 0,
            regrow_period: 10,
        })
        .unwrap()
        .with_harvest(RULES, &[HARVESTER])
        .unwrap()
}

fn id(slot: u32) -> EntityId {
    EntityId::new(slot, 0)
}

fn place(w: &mut World, cell: (u32, u32)) {
    let command = Command::PlaceStructure {
        kind: REFINERY,
        cell: Cell::new(cell.0, cell.1),
        owner: PlayerId(0),
    };
    w.schedule(Tick(0), PlayerId(0), command).unwrap();
}

/// Two refineries, slot 0 the nearer to the field and slot 1 the farther, and a harvester (slot 2)
/// that has mined a full load and is driving back.
fn returning() -> World {
    let mut w = world();
    place(&mut w, (6, 20)); // slot 0, dock point (9.5, 22.5)
    place(&mut w, (6, 36)); // slot 1, dock point (9.5, 38.5)
    let spawn = Command::SpawnUnit {
        kind: HARVESTER,
        pos: Vec2::new(Fx::from_ratio(29, 2), Fx::from_ratio(45, 2)),
        owner: PlayerId(0),
    };
    w.schedule(Tick(0), PlayerId(0), spawn).unwrap();
    w
}

fn until_returning(w: &mut World) {
    for _ in 0..1500 {
        w.step();
        if w.harvester_of(id(2))
            .is_some_and(|v| v.stage == HarvestStage::ToRefinery)
        {
            return;
        }
    }
    panic!("never started back");
}

#[test]
fn the_nearer_refinery_wins_while_it_stands() {
    let mut w = returning();
    until_returning(&mut w);
    assert_eq!(w.harvester_of(id(2)).unwrap().dock, Some(id(0)));
}

#[test]
fn a_refinery_at_zero_hit_points_is_not_a_refinery() {
    let mut w = returning();
    // Run to just before the harvester is full, then take the nearer refinery down.
    for _ in 0..40 {
        w.step();
    }
    w.entities.hp[0] = 0;
    until_returning(&mut w);
    assert_eq!(w.harvester_of(id(2)).unwrap().dock, Some(id(1)));
    // The dead one has no dock state at all.
    assert!(w.dock_of(id(0)).is_none());
    assert!(w.dock_of(id(1)).is_some());
}

#[test]
fn a_refinery_that_stops_standing_under_a_queued_harvester_sends_it_to_the_other() {
    let mut w = returning();
    until_returning(&mut w);
    // It is on its way to refinery 0; that one stops standing.
    w.entities.hp[0] = 0;
    w.step();
    assert_eq!(w.harvester_of(id(2)).unwrap().dock, Some(id(1)));
}

#[test]
fn a_structure_stands_while_it_has_hit_points() {
    let mut w = world();
    place(&mut w, (6, 20));
    w.step();
    assert!(w.is_standing(0));
    w.entities.hp[0] = 0;
    assert!(!w.is_standing(0));
}
