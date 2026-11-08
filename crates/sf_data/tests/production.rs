//! WP6.5: which factory builds a unit, and what a unit needs standing.
//!
//! * the factory function follows from the category (infantry, vehicle, vessel) and nothing else
//!   has one;
//! * the shipped units need nothing but their factory (the tech tree is not changed here);
//! * a unit's `requires` is read, checked and converted to kind numbers;
//! * a unit nothing could build is refused when the file has factories;
//! * the frozen copies of the scenarios load unchanged.

use std::path::{Path, PathBuf};

use sf_data::units::{
    BuildingFunction, Category, DefError, DefFile, DefSources, EMBEDDED_CLASSES, EMBEDDED_UNITS,
    EMBEDDED_WEAPONS, UnitDefs, produced_by,
};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn shipped() -> UnitDefs {
    let root = repo();
    UnitDefs::load(&root.join("assets/config"), &root.join("assets"))
        .unwrap_or_else(|e| panic!("the shipped definitions load: {e}"))
}

fn change(from: &str, to: &str) -> Result<UnitDefs, DefError> {
    let units = lf(EMBEDDED_UNITS);
    assert!(units.contains(from), "test setup: {from:?} not found");
    let (c, w) = (lf(EMBEDDED_CLASSES), lf(EMBEDDED_WEAPONS));
    let text = units.replacen(from, to, 1);
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
            text: &text,
        },
    };
    UnitDefs::from_sources(&sources, Some(&repo().join("assets")))
}

fn refused(from: &str, to: &str, message: &str) {
    let e = change(from, to).expect_err("the change must be refused");
    assert!(e.message.contains(message), "{e}");
    assert_eq!(e.file, "units.ron");
    assert!(e.line.is_some(), "an error with a line: {e}");
}

const RIFLEMAN: &str =
    r#"kind: 12, id: "rifleman", factions: [Meridian, Veyran], category: Infantry,"#;
const BARRACKS: &str =
    r#"kind: 3, id: "barracks", factions: [Meridian, Veyran], category: Structure,"#;

#[test]
fn the_factory_function_follows_from_the_category() {
    // Three different answers for three categories, none for the rest: a swap shows.
    assert_eq!(
        produced_by(Category::Infantry),
        Some(BuildingFunction::InfantryFactory)
    );
    assert_eq!(
        produced_by(Category::Vehicle),
        Some(BuildingFunction::VehicleFactory)
    );
    assert_eq!(
        produced_by(Category::Vessel),
        Some(BuildingFunction::VesselFactory)
    );
    assert_eq!(produced_by(Category::Structure), None);
    assert_eq!(produced_by(Category::Object), None);
}

#[test]
fn every_shipped_unit_is_made_by_the_factory_of_its_category() {
    let defs = shipped();
    let sim = defs.sim_units();
    let expect = |kind: u16| sim.iter().find(|s| s.kind == kind).unwrap().produced_by;
    // By name, so the test reads the shipped roles and not the rule it tests.
    for (id, function) in [
        ("rifleman", BuildingFunction::InfantryFactory),
        ("medic", BuildingFunction::InfantryFactory),
        ("light_tank", BuildingFunction::VehicleFactory),
        ("harvester", BuildingFunction::VehicleFactory),
        ("amphibious_transport", BuildingFunction::VehicleFactory),
        ("gunboat", BuildingFunction::VesselFactory),
    ] {
        let u = defs.units().iter().find(|u| u.id == id).unwrap();
        assert_eq!(expect(u.kind), Some(function), "{id}");
    }
    for u in defs.units() {
        let want = match u.category {
            Category::Infantry => Some(BuildingFunction::InfantryFactory),
            Category::Vehicle => Some(BuildingFunction::VehicleFactory),
            Category::Vessel => Some(BuildingFunction::VesselFactory),
            Category::Structure | Category::Object => None,
        };
        assert_eq!(expect(u.kind), want, "{}", u.id);
    }
    // And every shipped unit that has a price has a factory of that function that someone can
    // build, in the tree.
    for u in defs.units().iter().filter(|u| u.cost.is_some()) {
        if let Some(function) = produced_by(u.category) {
            assert!(
                defs.units().iter().any(|s| s.cost.is_some()
                    && s.building.as_ref().is_some_and(|b| b.function == function)),
                "{}",
                u.id
            );
        }
    }
}

#[test]
fn the_shipped_units_need_nothing_but_their_factory() {
    for u in shipped().units() {
        assert!(u.requires.is_empty(), "{} has requires", u.id);
    }
    for s in shipped().sim_units() {
        assert!(s.requires.is_empty());
    }
}

#[test]
fn a_requires_of_a_unit_arrives_as_kind_numbers() {
    let radar = 5;
    let tower = 6;
    let defs = change(
        RIFLEMAN,
        &format!(r#"{RIFLEMAN} requires: ["advanced_defences", "radar"],"#),
    )
    .expect("a unit that needs structures loads");
    let rifleman = defs.sim_units().iter().find(|s| s.kind == 12).unwrap();
    // The order of the record, converted one by one: advanced_defences is kind 7, radar kind 5.
    assert_eq!(rifleman.requires, vec![7, radar]);
    assert_ne!(rifleman.requires, vec![tower]);
    // Nobody else got it.
    for s in defs.sim_units().iter().filter(|s| s.kind != 12) {
        assert!(s.requires.is_empty(), "{}", s.kind);
    }
}

#[test]
fn a_bad_requires_of_a_unit_is_refused_at_its_name() {
    let with = |list: &str| format!(r#"{RIFLEMAN} requires: [{list}],"#);
    refused(
        RIFLEMAN,
        &with(r#""nowhere""#),
        "unknown prerequisite \"nowhere\"",
    );
    refused(RIFLEMAN, &with(r#""harvester""#), "is not a Structure");
    refused(RIFLEMAN, &with(r#""radar", "radar""#), "listed twice");
    // The bunker is for Veyran alone; the rifleman is for both.
    refused(
        RIFLEMAN,
        &with(r#""fortified_bunker""#),
        "is not available to Meridian",
    );
    // A mission structure nobody can build.
    refused(
        RIFLEMAN,
        &with(r#""relay_transmitter""#),
        "can never be built",
    );
}

#[test]
fn requires_belongs_to_a_unit_with_a_price_and_a_structure_has_its_own() {
    // The supply truck has no cost.
    refused(
        r#"kind: 29, id: "supply_truck", factions: [Neutral], category: Vehicle,"#,
        r#"kind: 29, id: "supply_truck", factions: [Neutral], category: Vehicle, requires: ["radar"],"#,
        "requires on a unit that cannot be built",
    );
    refused(
        BARRACKS,
        &format!(r#"{BARRACKS} requires: ["power_plant"],"#),
        "requires on a Structure",
    );
}

#[test]
fn a_unit_nothing_can_build_is_refused() {
    // The shipyard is the only vessel factory; without a price (and so without its tree edge)
    // nobody can build it, and the gunboat could never be made.
    refused(
        concat!(
            "building: (function: VesselFactory, exit_side: PosX, requires: [\"vehicle_factory\"]),\n",
            "            cost: 1000, build_seconds: 40, hp: 900,"
        ),
        "building: (function: VesselFactory, exit_side: PosX),\n            hp: 900,",
        "no buildable VesselFactory for Veyran",
    );
}

#[test]
fn the_frozen_copies_of_the_scenarios_still_load() {
    let root = repo();
    let dir = root.join("assets/scenarios");
    let mut n = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let is_defs = path
            .file_name()
            .and_then(|f| f.to_str())
            .is_some_and(|f| f.ends_with("_defs"));
        if !is_defs || !path.join("unit_defs.ron").exists() {
            continue;
        }
        UnitDefs::load(&path, &root.join("assets"))
            .unwrap_or_else(|e| panic!("{} loads: {e}", path.display()));
        n += 1;
    }
    assert!(n >= 8, "found {n} frozen copies");
}
