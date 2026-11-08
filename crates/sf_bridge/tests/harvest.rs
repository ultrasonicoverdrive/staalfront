//! The harvest rules through the bridge (WP7.2): the data numbers arrive in the world unchanged,
//! the harvester kinds are the roles with the `harvests` flag, and the shipped refinery is the dock.

use sf_bridge::{building_table, harvest_rules, harvester_kinds, unit_table};
use sf_data::harvest::HarvestSettings;
use sf_data::units::UnitDefs;
use sf_sim::{BuildingFunction, HarvestRules, Kind, World, WorldConfig};

#[test]
fn the_sim_numbers_equal_the_data_numbers() {
    let rules = harvest_rules(&HarvestSettings::embedded());
    assert_eq!(
        rules,
        HarvestRules {
            capacity: 200,
            mine_amount: 4,
            mine_period: 8,
            unload_amount: 10,
            unload_period: 4,
            search_radius: 40,
            retry_period: 100,
            call_range: 6,
            grant_timeout: 400,
            call_timeout: 10,
            max_tries: 3,
            park_distance: 4,
            park_spread: 5,
            park_near_tenths: 20,
            mine_start_tenths: 10,
            mine_reach_tenths: 15,
            mine_lose_tenths: 25,
            dock_reach_tenths: 20,
            travel_cost: 16,
            queue_cost: 100,
            switch_margin: 100,
            recheck_period: 20,
            queue_gap: 3,
            queue_start: 1,
            queue_step: 2,
            queue_rows: 8,
        }
    );
    assert!(rules.is_valid());
}

#[test]
fn the_harvester_is_the_role_with_the_flag() {
    let defs = UnitDefs::embedded();
    assert_eq!(
        harvester_kinds(&defs),
        [Kind(defs.kind_of("harvester").unwrap())]
    );
    assert_eq!(harvester_kinds(&defs), [Kind(11)]);
}

#[test]
fn the_shipped_refinery_is_the_dock() {
    let defs = UnitDefs::embedded();
    let table = building_table(&defs).unwrap();
    let docks: Vec<Kind> = table
        .iter()
        .filter(|b| b.function == BuildingFunction::HarvesterDock)
        .map(|b| b.kind)
        .collect();
    assert_eq!(docks, [Kind(defs.kind_of("refinery").unwrap())]);
}

#[test]
fn a_world_from_the_shipped_data_takes_the_harvest_rules() {
    let defs = UnitDefs::embedded();
    let world = World::new(WorldConfig::new(1).with_map_size(32, 32))
        .unwrap()
        .with_units(unit_table(&defs).unwrap())
        .unwrap()
        .with_buildings(building_table(&defs).unwrap())
        .unwrap()
        .with_harvest(
            harvest_rules(&HarvestSettings::embedded()),
            &harvester_kinds(&defs),
        )
        .unwrap();
    assert_eq!(world.harvester_kinds(), [Kind(11)]);
    assert_eq!(world.harvest_rules().unwrap().capacity, 200);
}

/// The rules say the grant outlasts the drive from the call range; the shipped harvester is slow
/// enough that this holds with room to spare (`sf_data/tests/harvest.rs` holds the same against the
/// raw numbers; this one goes through the world's own table).
#[test]
fn the_shipped_grant_outlasts_the_drive_from_the_call_range() {
    let defs = UnitDefs::embedded();
    let table = unit_table(&defs).unwrap();
    let speed = table.get(Kind(11)).unwrap().speed_per_tick;
    let rules = harvest_rules(&HarvestSettings::embedded());
    // The ticks a straight drive of `call_range` cells takes.
    let drive = (1..=10_000_i64)
        .find(|&t| speed.mul_int(t).floor_int() >= i64::from(rules.call_range))
        .unwrap();
    assert!(i64::from(rules.grant_timeout) > 2 * drive, "{drive}");
}

/// Every field gets its own value, so a swap or a field fed from the wrong source fails here
/// (the shipped numbers repeat: 20, 10 and 4 occur more than once).
#[test]
fn each_field_maps_to_its_own_field() {
    let settings = HarvestSettings {
        capacity: 101,
        mine_amount: 102,
        mine_period: 103,
        unload_amount: 104,
        unload_period: 105,
        search_radius: 106,
        retry_period: 107,
        call_range: 108,
        grant_timeout: 109,
        call_timeout: 110,
        max_tries: 111,
        park_distance: 112,
        park_spread: 113,
        park_near_tenths: 114,
        mine_start_tenths: 115,
        mine_reach_tenths: 116,
        mine_lose_tenths: 117,
        dock_reach_tenths: 118,
        travel_cost: 119,
        queue_cost: 120,
        switch_margin: 121,
        recheck_period: 122,
        queue_gap: 123,
        queue_start: 124,
        queue_step: 125,
        queue_rows: 126,
    };
    let rules = harvest_rules(&settings);
    assert_eq!(rules.capacity, 101);
    assert_eq!(rules.mine_amount, 102);
    assert_eq!(rules.mine_period, 103);
    assert_eq!(rules.unload_amount, 104);
    assert_eq!(rules.unload_period, 105);
    assert_eq!(rules.search_radius, 106);
    assert_eq!(rules.retry_period, 107);
    assert_eq!(rules.call_range, 108);
    assert_eq!(rules.grant_timeout, 109);
    assert_eq!(rules.call_timeout, 110);
    assert_eq!(rules.max_tries, 111);
    assert_eq!(rules.park_distance, 112);
    assert_eq!(rules.park_spread, 113);
    assert_eq!(rules.park_near_tenths, 114);
    assert_eq!(rules.mine_start_tenths, 115);
    assert_eq!(rules.mine_reach_tenths, 116);
    assert_eq!(rules.mine_lose_tenths, 117);
    assert_eq!(rules.dock_reach_tenths, 118);
    assert_eq!(rules.travel_cost, 119);
    assert_eq!(rules.queue_cost, 120);
    assert_eq!(rules.switch_margin, 121);
    assert_eq!(rules.recheck_period, 122);
    assert_eq!(rules.queue_gap, 123);
    assert_eq!(rules.queue_start, 124);
    assert_eq!(rules.queue_step, 125);
    assert_eq!(rules.queue_rows, 126);
}

// ---- storage (WP7.3) ---------------------------------------------------------------------------

#[test]
fn the_shipped_storage_reaches_the_world() {
    let defs = UnitDefs::embedded();
    let table = sf_bridge::storage_capacities(&defs);
    // Literal pin: only the refinery (kind 2) stores, 2000 units.
    assert_eq!(table, vec![(Kind(2), 2000)]);
    let world = World::new(WorldConfig::new(1))
        .unwrap()
        .with_units(sf_bridge::unit_table(&defs).unwrap())
        .unwrap()
        .with_buildings(sf_bridge::building_table(&defs).unwrap())
        .unwrap()
        .with_harvest(
            harvest_rules(&HarvestSettings::embedded()),
            &sf_bridge::harvester_kinds(&defs),
        )
        .unwrap()
        .with_storage(&table)
        .unwrap();
    assert!(world.has_storage());
}
