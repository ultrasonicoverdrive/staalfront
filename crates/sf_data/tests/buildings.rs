//! WP6.1: the building definitions (function, exit side, prerequisites).
//!
//! * the shipped tech tree is the one `docs/rules/buildings.md` draws, and every structure
//!   of the shipped file has an explicit `building` block;
//! * every way a block can be wrong is refused with the file and the line;
//! * the frozen copies of the scenarios have no block and load unchanged;
//! * a structure added to the file is data only (quality rule 6);
//! * the conversion for the simulation keeps footprint, build time, price and power.

use std::path::{Path, PathBuf};

use sf_data::units::{
    BANNED_IDS, BuildingFunction, Category, DefError, DefFile, DefSources, EMBEDDED_CLASSES,
    EMBEDDED_UNITS, EMBEDDED_WEAPONS, Faction, Side, UnitDefs,
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

/// The shipped units file with one substitution, loaded.
fn change(from: &str, to: &str) -> Result<UnitDefs, DefError> {
    let units = lf(EMBEDDED_UNITS);
    assert!(units.contains(from), "test setup: {from:?} not found");
    load_text(&units.replacen(from, to, 1))
}

fn load_text(units: &str) -> Result<UnitDefs, DefError> {
    let (c, w) = (lf(EMBEDDED_CLASSES), lf(EMBEDDED_WEAPONS));
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
            text: units,
        },
    };
    UnitDefs::from_sources(&sources, Some(&repo().join("assets")))
}

/// The error of a change that must be refused: its message and its line.
fn refused(from: &str, to: &str, message: &str) -> DefError {
    let e = change(from, to).expect_err("the change must be refused");
    assert!(e.message.contains(message), "{e}");
    assert_eq!(e.file, "units.ron");
    assert!(e.line.is_some(), "an error with a line: {e}");
    e
}

// ---- the shipped data ----------------------------------------------------------------------------

#[test]
fn every_shipped_structure_has_an_explicit_building_block() {
    let defs = shipped();
    let mut n = 0;
    for u in defs
        .units()
        .iter()
        .filter(|u| u.category == Category::Structure)
    {
        assert!(u.building.is_some(), "{} has no building block", u.id);
        n += 1;
    }
    assert_eq!(n, 19);
    assert_eq!(defs.sim_buildings().len(), n);
    for u in defs
        .units()
        .iter()
        .filter(|u| u.category != Category::Structure)
    {
        assert!(u.building.is_none(), "{} is no structure", u.id);
    }
}

/// The tech tree of `docs/rules/buildings.md`: id, function, exit side, prerequisites.
#[allow(clippy::type_complexity)]
const TREE: &[(&str, BuildingFunction, Option<Side>, &[&str])] = &[
    ("construction_center", BuildingFunction::Anchor, None, &[]),
    ("power_plant", BuildingFunction::PowerPlant, None, &[]),
    (
        "refinery",
        BuildingFunction::HarvesterDock,
        Some(Side::PosX),
        &["power_plant"],
    ),
    (
        "barracks",
        BuildingFunction::InfantryFactory,
        Some(Side::PosX),
        &["power_plant"],
    ),
    (
        "vehicle_factory",
        BuildingFunction::VehicleFactory,
        Some(Side::PosX),
        &["refinery"],
    ),
    ("radar", BuildingFunction::Radar, None, &["vehicle_factory"]),
    (
        "defence_tower",
        BuildingFunction::Defence,
        None,
        &["barracks"],
    ),
    (
        "advanced_defences",
        BuildingFunction::Defence,
        None,
        &["defence_tower", "radar"],
    ),
    (
        "shipyard",
        BuildingFunction::VesselFactory,
        Some(Side::PosX),
        &["vehicle_factory"],
    ),
    (
        "decoy_emplacement",
        BuildingFunction::Decoy,
        None,
        &["barracks"],
    ),
    (
        "fortified_bunker",
        BuildingFunction::Defence,
        None,
        &["barracks"],
    ),
    ("relay_transmitter", BuildingFunction::None, None, &[]),
    ("fuel_depot", BuildingFunction::None, None, &[]),
    ("archive_building", BuildingFunction::None, None, &[]),
    ("bridge_segment", BuildingFunction::None, None, &[]),
    ("rail_spur", BuildingFunction::None, None, &[]),
    ("automated_gun", BuildingFunction::Defence, None, &[]),
    ("generator", BuildingFunction::PowerPlant, None, &[]),
    ("unmanned_artillery", BuildingFunction::Defence, None, &[]),
];

#[test]
fn the_shipped_tech_tree_is_the_documented_one() {
    let defs = shipped();
    assert_eq!(TREE.len(), defs.sim_buildings().len());
    for &(id, function, exit, requires) in TREE {
        let b = defs.unit(id).unwrap().building.as_ref().unwrap();
        assert_eq!(b.function, function, "{id} function");
        assert_eq!(b.exit_side, exit, "{id} exit side");
        let want: Vec<String> = requires.iter().map(|r| r.to_string()).collect();
        assert_eq!(b.requires, want, "{id} requires");
    }
}

/// The campaign mission after which `content/units.md` ("Tech tree per campaign") unlocks each
/// buildable structure; M01 for the roles of the first row, both factions.
const UNLOCKED_AFTER: &[(&str, u8)] = &[
    ("power_plant", 1),
    ("barracks", 1),
    ("refinery", 1),
    ("vehicle_factory", 3),
    ("defence_tower", 4),
    ("fortified_bunker", 4),
    ("decoy_emplacement", 5),
    ("shipyard", 6),
    ("radar", 8),
    ("advanced_defences", 9),
];

#[test]
fn no_structure_is_unlocked_by_the_campaign_before_its_prerequisites() {
    let defs = shipped();
    let after = |id: &str| {
        UNLOCKED_AFTER
            .iter()
            .find(|(i, _)| *i == id)
            .map(|&(_, m)| m)
    };
    // The table is the roster of buildable structures, no more and no less.
    let buildable: Vec<&str> = defs
        .units()
        .iter()
        .filter(|u| u.category == Category::Structure && u.cost.is_some())
        .map(|u| u.id.as_str())
        .collect();
    assert_eq!(buildable.len(), UNLOCKED_AFTER.len());
    for id in buildable {
        let mission = after(id).unwrap_or_else(|| panic!("{id} has no unlock mission"));
        for r in &defs.unit(id).unwrap().building.as_ref().unwrap().requires {
            // The anchor is there from the start.
            let needed = after(r).unwrap_or(0);
            assert!(needed <= mission, "{id} (M{mission}) needs {r} (M{needed})");
        }
    }
    // The table above is what the roster file says.
    let roster = std::fs::read_to_string(repo().join("content/units.md")).unwrap();
    let row = |m: u8| {
        roster
            .lines()
            .find(|l| l.starts_with(&format!("| M{m:02} ")))
            .unwrap_or_else(|| panic!("no row for M{m:02}"))
            .to_string()
    };
    for &(id, m) in UNLOCKED_AFTER {
        assert!(
            row(m).contains(id),
            "content/units.md: {id} is not in the row of M{m:02}"
        );
    }
}

#[test]
fn the_numbers_of_functions_and_sides_are_permanent() {
    use BuildingFunction as F;
    let functions = [
        F::None,
        F::Anchor,
        F::PowerPlant,
        F::HarvesterDock,
        F::InfantryFactory,
        F::VehicleFactory,
        F::VesselFactory,
        F::Radar,
        F::Defence,
        F::Decoy,
    ];
    for (n, f) in functions.iter().enumerate() {
        assert_eq!(f.number() as usize, n, "{f:?}");
    }
    for (n, s) in [Side::PosX, Side::NegX, Side::PosY, Side::NegY]
        .iter()
        .enumerate()
    {
        assert_eq!(s.number() as usize, n, "{s:?}");
    }
}

#[test]
fn a_prerequisite_is_available_to_every_faction_of_the_building() {
    let defs = shipped();
    for u in defs.units().iter().filter(|u| u.building.is_some()) {
        for r in &u.building.as_ref().unwrap().requires {
            let p = defs.unit(r).unwrap();
            for f in &u.factions {
                assert!(
                    p.factions.contains(f),
                    "{} needs {r}, not open to {f:?}",
                    u.id
                );
            }
        }
    }
    // The two single-faction buildings sit on shared ones.
    assert_eq!(
        defs.unit("decoy_emplacement").unwrap().factions,
        [Faction::Meridian]
    );
    assert_eq!(
        defs.unit("fortified_bunker").unwrap().factions,
        [Faction::Veyran]
    );
}

#[test]
fn every_buildable_structure_can_be_reached_from_the_anchor() {
    // Following the prerequisites down always ends in a structure with none: nothing hangs on a
    // cycle or on a building that nobody can build.
    let defs = shipped();
    for u in defs
        .units()
        .iter()
        .filter(|u| u.cost.is_some() && u.category == Category::Structure)
    {
        let mut open = vec![u.id.as_str()];
        let mut seen = Vec::new();
        while let Some(id) = open.pop() {
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            let d = defs.unit(id).unwrap();
            assert!(
                d.cost.is_some(),
                "{} needs {id}, which cannot be built",
                u.id
            );
            open.extend(
                d.building
                    .as_ref()
                    .unwrap()
                    .requires
                    .iter()
                    .map(String::as_str),
            );
        }
    }
}

#[test]
fn the_factories_and_the_dock_have_an_exit_side_and_nothing_else_has() {
    let defs = shipped();
    for u in defs.units().iter().filter(|u| u.building.is_some()) {
        let b = u.building.as_ref().unwrap();
        assert_eq!(b.exit_side.is_some(), b.function.needs_exit(), "{}", u.id);
    }
    // The art faces +x, so every shipped exit is on the +x side.
    for b in defs
        .sim_buildings()
        .iter()
        .filter(|b| b.exit_side.is_some())
    {
        assert_eq!(b.exit_side, Some(Side::PosX), "kind {}", b.kind);
    }
}

#[test]
fn the_conversion_keeps_footprint_time_cost_power_and_factions() {
    let defs = shipped();
    for b in defs.sim_buildings() {
        let u = defs.unit_by_kind(b.kind).unwrap();
        let fp = u.footprint.unwrap();
        assert_eq!(b.footprint, (fp.width, fp.height), "{}", u.id);
        assert_eq!(b.cost, u.cost, "{}", u.id);
        assert_eq!(b.power, u.power, "{}", u.id);
        assert_eq!(
            b.build_ticks,
            u.build_seconds.to_ticks().unwrap(),
            "{}",
            u.id
        );
        assert_eq!(
            b.build_ticks,
            defs.sim_unit(b.kind).unwrap().build_ticks,
            "{}",
            u.id
        );
        assert_eq!(
            b.factions,
            defs.sim_unit(b.kind).unwrap().factions,
            "{}",
            u.id
        );
        let mut want: Vec<u16> = u
            .building
            .as_ref()
            .unwrap()
            .requires
            .iter()
            .map(|r| defs.kind_of(r).unwrap())
            .collect();
        want.sort_unstable();
        assert_eq!(b.requires, want, "{}", u.id);
    }
    // 15 seconds at 20 ticks per second.
    let plant = defs
        .sim_building(defs.kind_of("power_plant").unwrap())
        .unwrap();
    assert_eq!(plant.build_ticks, 300);
    assert!(
        defs.sim_building(defs.kind_of("rifleman").unwrap())
            .is_none()
    );
}

#[test]
fn the_order_of_the_records_does_not_change_the_buildings() {
    // Move the whole structure block behind the units: the same table comes out.
    let text = lf(EMBEDDED_UNITS);
    let start = text.find("        // ---- Shared structures").unwrap();
    let end = text.find("        // ---- Shared units").unwrap();
    let structures = &text[start..end];
    let moved = format!(
        "{}{}",
        &text[..start],
        text[end..].replacen(
            "    ],\n    // Tombstones",
            &format!("{structures}    ],\n    // Tombstones"),
            1
        )
    );
    assert_ne!(moved, text, "the structures really moved");
    assert!(
        moved.find("kind: 1, id: \"power_plant\"").unwrap()
            > moved.find("kind: 11, id: \"harvester\"").unwrap()
    );
    let other = load_text(&moved).unwrap();
    assert_eq!(other.sim_buildings(), shipped().sim_buildings());
}

// ---- the frozen copies ---------------------------------------------------------------------------

/// The frozen copies that were made after WP6.1 and carry the building blocks of the shipped file
/// on purpose: the scenarios that build (`BuildStructure`, WP6.2) need the tech tree, the exit sides
/// and the base radius. Every other copy predates the block.
const COPIES_WITH_BLOCKS: [&str; 1] = ["placement_defs"];

#[test]
fn the_frozen_copies_of_the_scenarios_load_with_no_building_blocks() {
    let root = repo().join("assets");
    let mut checked = 0;
    for entry in std::fs::read_dir(root.join("scenarios")).unwrap() {
        let path = entry.unwrap().path();
        if !path.is_dir() || !path.to_string_lossy().ends_with("_defs") {
            continue;
        }
        if COPIES_WITH_BLOCKS.iter().any(|c| path.ends_with(c)) {
            continue;
        }
        let defs = UnitDefs::load(&path, &root).unwrap();
        for u in defs.units() {
            assert!(u.building.is_none(), "{path:?}: {}", u.id);
        }
        // Every structure is still there, as a building with no function and no prerequisites.
        for b in defs.sim_buildings() {
            assert_eq!(b.function, BuildingFunction::None);
            assert_eq!(b.exit_side, None);
            assert!(b.requires.is_empty());
        }
        assert!(!defs.sim_buildings().is_empty(), "{path:?}");
        checked += 1;
    }
    assert!(checked >= 8, "found {checked} frozen copies");
}

#[test]
fn the_copy_that_builds_keeps_the_blocks_it_was_made_with() {
    // `placement_defs` is a copy of the shipped file made for WP6.2. Its table is pinned in the
    // headless test of the scenario (`the_frozen_definitions_are_the_wp6_1_tree`); here only that
    // the blocks and the base radius are there.
    let root = repo().join("assets");
    let copy = UnitDefs::load(&root.join("scenarios/placement_defs"), &root).unwrap();
    let anchor = copy
        .sim_building(copy.kind_of("construction_center").unwrap())
        .unwrap();
    assert_eq!(anchor.function, BuildingFunction::Anchor);
    assert_eq!(anchor.build_radius.floor_int(), 8);
    assert!(copy.units().iter().filter(|u| u.building.is_some()).count() >= 10);
}

// ---- data only -------------------------------------------------------------------------------------

#[test]
fn a_new_structure_is_a_record_and_no_rust_change() {
    let text = lf(EMBEDDED_UNITS);
    let record = r#"
        (
            kind: 900, id: "training_hall", factions: [Meridian, Veyran], category: Structure,
            cost: 700, build_seconds: 30, hp: 800, armor: "structure", power: -25,
            footprint: (width: 3, height: 3), sight: 5,
            building: (function: InfantryFactory, exit_side: NegY, requires: ["barracks", "radar"]),
            model: Shape(Cube), strategic_icon: Square,
        ),
    ],
    // Tombstones"#;
    let added = text.replacen("    ],\n    // Tombstones", record, 1);
    let defs = load_text(&added).unwrap();
    let b = defs.sim_building(900).unwrap();
    assert_eq!(b.function, BuildingFunction::InfantryFactory);
    assert_eq!(b.exit_side, Some(Side::NegY));
    assert_eq!(
        b.requires,
        [
            defs.kind_of("barracks").unwrap(),
            defs.kind_of("radar").unwrap()
        ]
    );
    assert_eq!(b.build_ticks, 600);
}

// ---- refusals -----------------------------------------------------------------------------------------

#[test]
fn an_unknown_prerequisite_is_refused_at_its_name() {
    let e = refused(
        r#"requires: ["power_plant"]),
"#,
        r#"requires: ["power_plantt"]),
"#,
        "unknown prerequisite \"power_plantt\"",
    );
    assert!(
        e.message.contains("refinery") || e.message.contains("barracks"),
        "{e}"
    );
}

#[test]
fn a_prerequisite_must_be_a_structure() {
    refused(
        r#"function: Defence, requires: ["barracks"]),"#,
        r#"function: Defence, requires: ["rifleman"]),"#,
        "\"rifleman\" is not a Structure",
    );
}

#[test]
fn a_prerequisite_nobody_can_build_is_refused_unless_it_is_the_anchor() {
    refused(
        r#"function: Defence, requires: ["barracks"]),"#,
        r#"function: Defence, requires: ["rail_spur"]),"#,
        "can never be built",
    );
    // The anchor has no cost either, and is allowed.
    change(
        r#"function: Defence, requires: ["barracks"]),"#,
        r#"function: Defence, requires: ["construction_center"]),"#,
    )
    .unwrap();
}

#[test]
fn a_building_may_not_need_itself_or_list_a_prerequisite_twice() {
    let e = refused(
        r#"function: Radar, requires: ["vehicle_factory"]),"#,
        r#"function: Radar, requires: ["radar"]),"#,
        "requires itself",
    );
    // The line of `requires:`, not the `id:` line above it.
    let text = lf(EMBEDDED_UNITS);
    let id_line = text
        .lines()
        .position(|l| l.contains("id: \"radar\""))
        .unwrap()
        + 1;
    let req_line = id_line
        + text
            .lines()
            .skip(id_line)
            .position(|l| l.contains("function: Radar"))
            .unwrap()
        + 1;
    assert_eq!(e.line, Some(req_line), "{e}");
    refused(
        r#"function: Radar, requires: ["vehicle_factory"]),"#,
        r#"function: Radar, requires: ["vehicle_factory", "vehicle_factory"]),"#,
        "listed twice",
    );
}

#[test]
fn a_cycle_in_the_tech_tree_is_refused_and_named() {
    // power_plant needs refinery, which needs power_plant.
    let e = refused(
        "building: (function: PowerPlant),\n            cost: 300",
        "building: (function: PowerPlant, requires: [\"refinery\"]),\n            cost: 300",
        "tech tree cycle in requires: power_plant -> refinery -> power_plant",
    );
    assert!(e.line.unwrap() > 1);
    // A longer one: power_plant -> radar -> vehicle_factory -> refinery -> power_plant.
    refused(
        "building: (function: PowerPlant),\n            cost: 300",
        "building: (function: PowerPlant, requires: [\"radar\"]),\n            cost: 300",
        "power_plant -> radar -> vehicle_factory -> refinery -> power_plant",
    );
}

#[test]
fn a_prerequisite_must_be_open_to_every_faction_of_the_building() {
    // The shared tower may not need the Meridian-only decoy.
    refused(
        r#"function: Defence, requires: ["barracks"]),
            cost: 500"#,
        r#"function: Defence, requires: ["decoy_emplacement"]),
            cost: 500"#,
        "is not available to Veyran",
    );
}

#[test]
fn a_footprint_of_zero_or_too_big_is_refused() {
    let units = lf(EMBEDDED_UNITS);
    assert!(units.contains("footprint: (width: 2, height: 3), sight: 5,"));
    for (to, what) in [
        (
            "footprint: (width: 0, height: 3), sight: 5,",
            "footprint width 0",
        ),
        (
            "footprint: (width: 2, height: 0), sight: 5,",
            "footprint height 0",
        ),
        (
            "footprint: (width: 17, height: 3), sight: 5,",
            "footprint width 17",
        ),
        (
            "footprint: (width: 2, height: 99), sight: 5,",
            "footprint height 99",
        ),
    ] {
        refused("footprint: (width: 2, height: 3), sight: 5,", to, what);
    }
    // 16 is the largest allowed.
    change(
        "footprint: (width: 2, height: 3), sight: 5,",
        "footprint: (width: 16, height: 16), sight: 5,",
    )
    .unwrap();
}

#[test]
fn a_negative_build_time_is_refused() {
    refused(
        "build_seconds: 15,",
        "build_seconds: -15,",
        "build_seconds -15 is negative",
    );
    refused("build_seconds: 15,", "build_seconds: -0.05,", "is negative");
    // Not a multiple of a tick is a different mistake.
    refused(
        "build_seconds: 15,",
        "build_seconds: 15.01,",
        "whole number of ticks",
    );
}

#[test]
fn a_factory_function_without_an_exit_side_is_refused() {
    for (from, to) in [
        (
            "function: InfantryFactory, exit_side: PosX,",
            "function: InfantryFactory,",
        ),
        (
            "function: VehicleFactory, exit_side: PosX,",
            "function: VehicleFactory,",
        ),
        (
            "function: VesselFactory, exit_side: PosX,",
            "function: VesselFactory,",
        ),
        (
            "function: HarvesterDock, exit_side: PosX,",
            "function: HarvesterDock,",
        ),
    ] {
        refused(from, to, "needs an exit_side");
    }
}

#[test]
fn an_exit_side_on_a_building_that_nobody_enters_is_refused() {
    refused(
        "function: Radar,",
        "function: Radar, exit_side: NegX,",
        "exit_side NegX on a building with function Radar",
    );
}

#[test]
fn a_function_needs_the_data_it_acts_on() {
    // power_plant with its power taken away.
    refused(
        "hp: 600, armor: \"structure\", power: 100,",
        "hp: 600, armor: \"structure\",",
        "needs power above 0",
    );
    refused(
        "flags: (minimap: true),",
        "flags: (),",
        "needs the minimap flag",
    );
    refused(
        "flags: (needs_water: true),",
        "flags: (),",
        "needs the needs_water flag",
    );
    refused(
        "flags: (build_radius: 8),",
        "flags: (),",
        "needs a build_radius",
    );
    refused(
        "flags: (disguise_as: \"defence_tower\"),",
        "flags: (),",
        "needs a disguise_as",
    );
}

#[test]
fn a_building_block_is_for_a_structure_and_requires_for_something_buildable() {
    refused(
        "kind: 12, id: \"rifleman\", factions: [Meridian, Veyran], category: Infantry,",
        "kind: 12, id: \"rifleman\", factions: [Meridian, Veyran], category: Infantry, building: (),",
        "only a Structure has a building block",
    );
    refused(
        "building: (function: Anchor),",
        "building: (function: Anchor, requires: [\"power_plant\"]),",
        "cannot be built",
    );
}

#[test]
fn an_unknown_function_or_field_is_refused_with_its_line() {
    let e = change("function: Radar,", "function: Radarr,").unwrap_err();
    assert!(e.line.is_some(), "{e}");
    let e = change("function: Radar,", "function: Radar, speed: 3,").unwrap_err();
    assert!(e.line.is_some(), "{e}");
}

#[test]
fn the_banned_names_are_refused_as_ids() {
    assert_eq!(BANNED_IDS, ["construction_yard", "naval_yard"]);
    for (id, from) in [
        ("construction_yard", "id: \"construction_center\""),
        ("naval_yard", "id: \"shipyard\""),
    ] {
        let e = refused(from, &format!("id: \"{id}\""), "is a banned name");
        assert!(e.message.contains(id), "{e}");
    }
}

#[test]
fn a_duplicate_or_reused_id_or_number_is_refused() {
    refused("id: \"radar\"", "id: \"barracks\"", "appears twice");
    refused("kind: 5, id: \"radar\"", "kind: 3, id: \"radar\"", "kind 3");
    // A tombstone keeps its number and its name.
    refused(
        "retired_units: [],",
        "retired_units: [(kind: 5, name: \"old\")],",
        "retired",
    );
    refused(
        "retired_units: [],",
        "retired_units: [(kind: 77, name: \"radar\")],",
        "retired",
    );
}

/// The structures the player starts with in a mission, read from section "E. Starting setup" of
/// `content/missions/<mission>.md` (rows `| Player | <role> | <count> | ...`).
fn player_starting_structures(defs: &UnitDefs, mission: &str) -> Vec<String> {
    let path = repo().join(format!("content/missions/{mission}.md"));
    let text = lf(&std::fs::read_to_string(&path).unwrap());
    let section = text
        .split("## E. Starting setup")
        .nth(1)
        .unwrap_or_else(|| panic!("{mission}: no starting setup"));
    let section = section.split("\n## ").next().unwrap();
    section
        .lines()
        .filter_map(|l| {
            let cells: Vec<&str> = l.split('|').map(str::trim).collect();
            (cells.len() > 3 && cells[1] == "Player").then(|| cells[2].to_string())
        })
        .filter(|id| {
            defs.unit(id)
                .is_some_and(|u| u.category == Category::Structure)
        })
        .collect()
}

#[test]
fn f1_m05_can_build_the_decoy_from_its_starting_structures() {
    // F1-M05 teaches the decoy (MSG_04, "New to build") on a fixed budget of 2000 credits
    // and no ore; T_PROBE comes at 0:30. So the decoy must be buildable from the start.
    let defs = shipped();
    let start = player_starting_structures(&defs, "F1-M05");
    assert_eq!(
        start,
        [
            "construction_center",
            "power_plant",
            "barracks",
            "vehicle_factory"
        ],
        "the starting setup of F1-M05 changed; check the decoy against it"
    );
    let decoy = defs.unit("decoy_emplacement").unwrap();
    assert!(decoy.factions.contains(&Faction::Meridian));
    for r in &decoy.building.as_ref().unwrap().requires {
        assert!(
            start.contains(r),
            "decoy_emplacement needs {r}, which F1-M05 does not start with"
        );
    }
    let cost = decoy.cost.unwrap();
    assert!(cost * 4 <= 2000, "a few decoys fit the budget: {cost}");
}
