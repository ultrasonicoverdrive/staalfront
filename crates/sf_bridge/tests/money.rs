//! The money rules through the bridge (WP7.4): the data numbers arrive in the world unchanged, and
//! the limits of the data file are the simulation's.

use sf_bridge::{harvest_rules, money_rules, starting_credits};
use sf_data::harvest::HarvestSettings;
use sf_data::money::{self, MoneySettings};
use sf_data::units::UnitDefs;
use sf_sim::{MoneyRules, PlayerId, World, WorldConfig};

#[test]
fn each_money_number_arrives_as_itself() {
    // All fields distinct, so a swapped or dropped field shows.
    let settings = MoneySettings {
        credits_per_ore: 7,
        starting_credits: 12_345,
    };
    assert_eq!(money_rules(&settings), MoneyRules { credits_per_ore: 7 });
    let players = [PlayerId(3), PlayerId(0)];
    assert_eq!(
        starting_credits(&settings, &players),
        vec![(PlayerId(3), 12_345), (PlayerId(0), 12_345)]
    );
    assert!(starting_credits(&settings, &[]).is_empty());
}

#[test]
fn the_shipped_money_reaches_the_world() {
    let defs = UnitDefs::embedded();
    let settings = MoneySettings::embedded();
    let players = [PlayerId(0), PlayerId(1)];
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
        .with_storage(&sf_bridge::storage_capacities(&defs))
        .unwrap()
        .with_money(
            money_rules(&settings),
            &starting_credits(&settings, &players),
        )
        .unwrap();
    // Literal pin of the shipped numbers, as the world sees them.
    assert_eq!(world.money_rules(), Some(MoneyRules { credits_per_ore: 1 }));
    assert_eq!(
        (world.credits(PlayerId(0)), world.credits(PlayerId(1))),
        (1000, 1000)
    );
}

#[test]
fn the_limits_of_the_data_file_are_the_limits_of_the_simulation() {
    assert_eq!(money::MAX_CREDITS_PER_ORE, sf_sim::MAX_CREDITS_PER_ORE);
    assert_eq!(money::MAX_STARTING_CREDITS, sf_sim::MAX_STARTING_CREDITS);
}
