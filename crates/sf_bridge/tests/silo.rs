//! WP7.3b: the proposed ore silo with the shipped numbers. Its storage reaches the world through the
//! same table as the refinery's (`storage_capacities`), it adds to the owner's capacity only while it
//! stands, and it needs a standing refinery (`requires`). The rules themselves are tested on a hand-made
//! table in `sf_sim/tests/harvest.rs`.

use sf_bridge::{
    building_table, harvest_rules, harvester_kinds, storage_capacities, terrain_defs, unit_table,
};
use sf_data::harvest::HarvestSettings;
use sf_data::movement::MovementCosts;
use sf_data::terrain::TerrainTypes;
use sf_data::units::{Faction, UnitDefs};
use sf_sim::{
    Cell, Command, EntityId, Kind, PlacementError, PlayerId, Terrain, World, WorldConfig,
};

const SIDE: u32 = 96;
const P0: PlayerId = PlayerId(0);
const REFINERY_STORES: u64 = 2000;
const SILO_STORES: u64 = 1500;

fn world(defs: &UnitDefs) -> World {
    let types = TerrainTypes::embedded();
    let costs = MovementCosts::embedded();
    let layer = vec![types.id_of("grass").unwrap(); (SIDE * SIDE) as usize];
    let terrain = Terrain::from_layer(SIDE, SIDE, &layer, &terrain_defs(&types, &costs)).unwrap();
    World::with_terrain(WorldConfig::new(3).with_map_size(SIDE, SIDE), terrain)
        .unwrap()
        .with_units(unit_table(defs).unwrap())
        .unwrap()
        .with_buildings(building_table(defs).unwrap())
        .unwrap()
        .with_harvest(
            harvest_rules(&HarvestSettings::embedded()),
            &harvester_kinds(defs),
        )
        .unwrap()
        .with_storage(&storage_capacities(defs))
        .unwrap()
        .with_player_factions(&[(P0, Faction::Meridian.bit())])
        .unwrap()
}

fn send(w: &mut World, command: Command) -> sf_sim::StepReport {
    let tick = w.tick();
    w.schedule(tick, P0, command).unwrap();
    w.step()
}

fn place(w: &mut World, defs: &UnitDefs, id: &str, x: u32, y: u32) {
    let kind = Kind(defs.kind_of(id).unwrap());
    let r = send(
        w,
        Command::PlaceStructure {
            kind,
            cell: Cell::new(x, y),
            owner: P0,
        },
    );
    assert_eq!(r.placements_rejected, 0, "{id}");
}

fn build_silo(w: &mut World, defs: &UnitDefs, x: u32, y: u32) -> sf_sim::StepReport {
    send(
        w,
        Command::BuildStructure {
            kind: Kind(defs.kind_of("ore_silo").unwrap()),
            cell: Cell::new(x, y),
        },
    )
}

#[test]
fn the_silo_needs_a_standing_refinery() {
    let defs = UnitDefs::embedded();
    let mut w = world(&defs);
    place(&mut w, &defs, "construction_center", 40, 40);
    let r = build_silo(&mut w, &defs, 45, 40);
    assert_eq!(r.placements_rejected, 1);
    assert_eq!(
        w.placement_rejections()[0].reason,
        PlacementError::MissingPrerequisite(Kind(defs.kind_of("refinery").unwrap()))
    );
    // With the refinery standing it is accepted.
    place(&mut w, &defs, "refinery", 40, 46);
    assert_eq!(build_silo(&mut w, &defs, 45, 40).placements_rejected, 0);
}

#[test]
fn the_silo_adds_its_storage_only_while_it_stands() {
    let defs = UnitDefs::embedded();
    let mut w = world(&defs);
    place(&mut w, &defs, "construction_center", 40, 40);
    place(&mut w, &defs, "refinery", 40, 46);
    w.step();
    assert_eq!(w.storage_capacity(P0), REFINERY_STORES);
    // Under construction (12 seconds, 240 ticks): nothing yet.
    assert_eq!(build_silo(&mut w, &defs, 45, 40).placements_rejected, 0);
    let silo = EntityId::new(2, 0);
    for _ in 0..10 {
        w.step();
    }
    assert!(w.construction_of(silo).is_some());
    assert_eq!(w.storage_capacity(P0), REFINERY_STORES);
    for _ in 0..300 {
        w.step();
    }
    assert!(w.stands(silo));
    assert_eq!(w.storage_capacity(P0), REFINERY_STORES + SILO_STORES);
    // Gone: the capacity falls back at the next sync.
    send(&mut w, Command::Despawn { id: silo });
    w.step();
    assert_eq!(w.storage_capacity(P0), REFINERY_STORES);
}
