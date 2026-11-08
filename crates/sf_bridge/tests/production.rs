//! The production rules through the bridge (WP6.5): the data numbers arrive in the world unchanged,
//! each one as itself, and the limits of the data file are the simulation's.

use sf_bridge::{production_rules, production_table};
use sf_data::production::{self, ProductionSettings};
use sf_data::units::{
    DefFile, DefSources, EMBEDDED_CLASSES, EMBEDDED_UNITS, EMBEDDED_WEAPONS, UnitDefs,
};
use sf_sim::{BuildingFunction, Kind, ProductionRules, World, WorldConfig};

fn repo() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn each_production_number_arrives_as_itself() {
    // Distinct values, so a swapped or dropped field shows.
    let settings = ProductionSettings {
        queue_limit: 7,
        exit_radius: 3,
    };
    assert_eq!(
        production_rules(&settings),
        ProductionRules {
            queue_limit: 7,
            exit_radius: 3
        }
    );
}

#[test]
fn the_limits_of_the_data_file_are_the_limits_of_the_simulation() {
    assert_eq!(production::MAX_QUEUE_LIMIT, sf_sim::MAX_QUEUE_LIMIT);
    assert_eq!(production::MAX_EXIT_RADIUS, sf_sim::MAX_EXIT_RADIUS);
}

#[test]
fn each_unit_arrives_with_its_own_price_time_factory_and_factions() {
    let defs = UnitDefs::embedded();
    let table = production_table(&defs).unwrap();
    // Literal pins of roles whose numbers all differ (cost, ticks, function, factions), read from
    // the shipped file by hand: a field swapped between two roles or two columns shows.
    let pin = |kind: u16, function, cost, ticks, factions| {
        let b = table
            .get(Kind(kind))
            .unwrap_or_else(|| panic!("kind {kind}"));
        assert_eq!(
            (b.factory, b.cost, b.build_ticks, b.factions),
            (function, cost, ticks, factions),
            "kind {kind}"
        );
        assert!(b.requires.is_empty(), "kind {kind}");
    };
    // rifleman: 100 credits, 5 s; both factions (Meridian 1 + Veyran 2).
    pin(12, BuildingFunction::InfantryFactory, 100, 100, 3);
    // sniper: 600 credits, 20 s; Meridian only.
    pin(16, BuildingFunction::InfantryFactory, 600, 400, 1);
    // medic: 400 credits, 12 s; Veyran only.
    pin(17, BuildingFunction::InfantryFactory, 400, 240, 2);
    // light tank: 700 credits, 20 s.
    pin(19, BuildingFunction::VehicleFactory, 700, 400, 3);
    // heavy tank: 1400 credits, 40 s.
    pin(20, BuildingFunction::VehicleFactory, 1400, 800, 3);
    // harvester: 1100 credits, 30 s.
    pin(11, BuildingFunction::VehicleFactory, 1100, 600, 3);
    // gunboat: 1200 credits, 35 s; Veyran only.
    pin(28, BuildingFunction::VesselFactory, 1200, 700, 2);
    // Structures and objects are not in it, nor is anything without a price.
    for kind in [0u16, 2, 3, 4, 8, 29, 35, 42] {
        assert!(table.get(Kind(kind)).is_none(), "kind {kind}");
    }
    // Everything with a price and a mobile category is.
    let producible = defs
        .sim_units()
        .iter()
        .filter(|u| u.produced_by.is_some() && u.cost.is_some())
        .count();
    assert_eq!(table.iter().count(), producible);
    // The refinery's harvester: structure 2, unit 11 (the pair, not its reverse).
    assert_eq!(table.comes_with_all(), &[(Kind(2), Kind(11))]);
    assert_eq!(table.comes_with(Kind(2)), Some(Kind(11)));
    assert_eq!(table.comes_with(Kind(11)), None);
}

#[test]
fn a_requires_of_a_unit_arrives_in_the_table() {
    let (c, w) = (
        EMBEDDED_CLASSES.replace("\r\n", "\n"),
        EMBEDDED_WEAPONS.replace("\r\n", "\n"),
    );
    let units = EMBEDDED_UNITS.replace("\r\n", "\n").replacen(
        r#"kind: 12, id: "rifleman", factions: [Meridian, Veyran], category: Infantry,"#,
        r#"kind: 12, id: "rifleman", factions: [Meridian, Veyran], category: Infantry, requires: ["radar", "refinery"],"#,
        1,
    );
    let sources = DefSources {
        classes: DefFile {
            name: "classes.ron",
            text: &c,
        },
        weapons: DefFile {
            name: "weapons.ron",
            text: &w,
        },
        units: DefFile {
            name: "units.ron",
            text: &units,
        },
    };
    let defs = UnitDefs::from_sources(&sources, Some(&repo().join("assets"))).unwrap();
    let table = production_table(&defs).unwrap();
    // radar is kind 5, refinery kind 2; sorted ascending by the table.
    assert_eq!(
        table.get(Kind(12)).unwrap().requires,
        vec![Kind(2), Kind(5)]
    );
    assert!(table.get(Kind(13)).unwrap().requires.is_empty());
}

#[test]
fn the_shipped_production_reaches_the_world() {
    let defs = UnitDefs::embedded();
    let world = World::new(WorldConfig::new(1))
        .unwrap()
        .with_units(sf_bridge::unit_table(&defs).unwrap())
        .unwrap()
        .with_buildings(sf_bridge::building_table(&defs).unwrap())
        .unwrap()
        .with_production(
            production_rules(&ProductionSettings::embedded()),
            production_table(&defs).unwrap(),
        )
        .unwrap();
    // Literal pin of the shipped numbers, as the world sees them.
    assert_eq!(
        world.production_rules(),
        Some(ProductionRules {
            queue_limit: 5,
            exit_radius: 4
        })
    );
    assert_eq!(
        world
            .production_table()
            .unwrap()
            .get(Kind(12))
            .unwrap()
            .cost,
        100
    );
}
