//! WP4.1: terrain costs per movement class.
//!
//! * the shipped table is complete (every class names every terrain type), and
//!   its numbers are pinned: changing one is a balance decision;
//! * the rules the table stands for (air ignores terrain, vessels keep to water
//!   and pass under a bridge, rock stops everything on the ground);
//! * kind numbers of the classes are permanent;
//! * a table with a hole, a typo, a double entry or a wild number is refused
//!   with the file and the line, and a class or a terrain type added as data
//!   needs no Rust change.

use sf_data::movement::{MovementCosts, MovementError};
use sf_data::terrain::{TerrainType, TerrainTypes};
use sf_data::units::{
    ClassTables, DefFile, DefSources, EMBEDDED_CLASSES, EMBEDDED_UNITS, EMBEDDED_WEAPONS,
    TerrainCost, TurnMode, UnitDefs,
};

use TerrainCost::{Impassable, Speed};

/// The terrain types in the order of the shipped table.
const TERRAINS: [&str; 6] = ["grass", "road", "water", "rock", "ore", "bridge"];

fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn classes_text() -> String {
    lf(EMBEDDED_CLASSES)
}

/// The shipped definitions with a substitution applied to `unit_classes.ron`.
fn load_classes(edit: impl Fn(&str) -> String) -> Result<UnitDefs, sf_data::units::DefError> {
    let (c, w, u) = (
        edit(&classes_text()),
        lf(EMBEDDED_WEAPONS),
        lf(EMBEDDED_UNITS),
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
            text: &u,
        },
    };
    UnitDefs::from_sources(&sources, None)
}

// ---- the shipped table ---------------------------------------------------

#[test]
fn every_class_has_a_cost_for_every_terrain_type() {
    let costs = MovementCosts::embedded();
    let types = TerrainTypes::embedded();
    assert_eq!(costs.classes().len(), 6);
    assert_eq!(types.len(), 6);
    for class in costs.classes() {
        assert_eq!(class.costs().len(), types.len(), "{}", class.name);
        for t in types.iter() {
            assert!(
                class.cost_on(t.id).is_some() && class.cost_on_name(&t.name).is_some(),
                "{} on {}",
                class.name,
                t.name
            );
        }
    }
    assert_eq!(costs.terrain_ids(), [0, 1, 2, 3, 4, 5]);
}

/// The table of `docs/rules/movement.md`, in thousandths of the unit's speed.
/// Order of the columns: grass, road, water, rock, ore, bridge. `None` is
/// impassable.
const SHIPPED: [(&str, u16, [Option<u16>; 6]); 6] = [
    (
        "foot",
        0,
        [Some(1000), Some(1100), None, None, Some(800), Some(1100)],
    ),
    (
        "wheeled",
        1,
        [Some(800), Some(1200), None, None, Some(600), Some(1200)],
    ),
    (
        "tracked",
        2,
        [Some(1000), Some(1200), None, None, Some(800), Some(1200)],
    ),
    (
        "hover",
        3,
        [
            Some(1000),
            Some(1100),
            Some(800),
            None,
            Some(900),
            Some(1100),
        ],
    ),
    ("water", 4, [None, None, Some(1000), None, None, Some(1000)]),
    (
        "air",
        5,
        [
            Some(1000),
            Some(1000),
            Some(1000),
            Some(1000),
            Some(1000),
            Some(1000),
        ],
    ),
];

#[test]
fn the_shipped_cost_table_is_pinned() {
    // Kind numbers are permanent (docs/rules/units.md), and so is what a
    // reviewed balance decision put here: a change is deliberate.
    let costs = MovementCosts::embedded();
    for (name, kind, row) in SHIPPED {
        let class = costs.class(kind).unwrap_or_else(|| panic!("class {kind}"));
        assert_eq!(class.name, name, "kind {kind}");
        for (terrain, want) in TERRAINS.iter().zip(row) {
            let want = want.map_or(Impassable, Speed);
            assert_eq!(
                class.cost_on_name(terrain),
                Some(want),
                "{name} on {terrain}"
            );
        }
    }
}

#[test]
fn the_rules_the_table_stands_for() {
    let costs = MovementCosts::embedded();
    let on = |class: &str, terrain: &str| {
        costs
            .class_by_name(class)
            .and_then(|c| c.cost_on_name(terrain))
            .unwrap()
    };
    // Air ignores terrain: normal speed everywhere.
    for t in TERRAINS {
        assert_eq!(on("air", t), Speed(1000), "air on {t}");
    }
    // Vessels keep to water, and pass under a bridge (a bridge is a road over
    // water; docs/rules/movement.md).
    for t in TERRAINS {
        let enters = matches!(t, "water" | "bridge");
        assert_eq!(on("water", t).is_passable(), enters, "water class on {t}");
    }
    // Rock stops everything on the ground, water everything that cannot swim.
    for class in ["foot", "wheeled", "tracked", "hover", "water"] {
        assert_eq!(on(class, "rock"), Impassable, "{class} on rock");
    }
    for class in ["foot", "wheeled", "tracked"] {
        assert_eq!(on(class, "water"), Impassable, "{class} on water");
    }
    assert!(on("hover", "water").is_passable());
    // A bridge is crossed by every class: land over it, water under it, air.
    for class in ["foot", "wheeled", "tracked", "hover", "water", "air"] {
        assert!(on(class, "bridge").is_passable(), "{class} on the bridge");
    }
    // A road is quicker than grass for every class that is slowed by neither
    // (the vessels and the aircraft do not use it).
    for class in ["foot", "wheeled", "tracked"] {
        let (Speed(road), Speed(grass)) = (on(class, "road"), on(class, "grass")) else {
            panic!("{class}: road and grass are passable");
        };
        assert!(road > grass, "{class}: road {road}, grass {grass}");
    }
    // Ore slows every ground class.
    for class in ["foot", "wheeled", "tracked"] {
        let (Speed(ore), Speed(grass)) = (on(class, "ore"), on(class, "grass")) else {
            panic!("{class}: ore and grass are passable");
        };
        assert!(ore < grass, "{class}: ore {ore}, grass {grass}");
    }
}

#[test]
fn the_ground_classes_are_marked_in_the_data() {
    // `ground: true` in unit_classes.ron (not a list of names in Rust) says which
    // classes make a terrain type open ground.
    let costs = MovementCosts::embedded();
    for class in costs.classes() {
        let ground = matches!(class.name.as_str(), "foot" | "wheeled" | "tracked");
        assert_eq!(class.ground, ground, "{}", class.name);
    }
    let types = TerrainTypes::embedded();
    for (name, open) in [
        ("grass", true),
        ("road", true),
        ("ore", true),
        ("bridge", true),
        ("water", false),
        ("rock", false),
    ] {
        assert_eq!(
            costs.ground_passable(types.id_of(name).unwrap()),
            open,
            "{name}"
        );
    }
    // The flag is optional (false), so older files keep loading; marking another
    // class changes the summary without a Rust change.
    let marked =
        load_classes(|t| t.replacen("name: \"hover\",", "name: \"hover\", ground: true,", 1))
            .unwrap();
    let costs = MovementCosts::from_defs(&marked, &types).unwrap();
    assert!(costs.class_by_name("hover").unwrap().ground);
    assert!(!costs.ground_passable(types.id_of("water").unwrap()));
    assert!(!costs.ground_passable(types.id_of("rock").unwrap()));
    let unmarked = load_classes(|t| t.replacen("ground: true, ", "", 3)).unwrap();
    let costs = MovementCosts::from_defs(&unmarked, &types).unwrap();
    assert!(costs.classes().iter().all(|c| !c.ground));
    assert!(
        costs.ground_passable(types.id_of("water").unwrap()),
        "no class marked: no restriction"
    );
}

#[test]
fn the_hover_cost_on_water_is_the_amphibious_transports_water_speed() {
    // `water_speed` of the role (1.2 against 1.5 on land) and the hover class's
    // cost on water say the same thing; the simulation reads the cost, so the
    // two must agree.
    let defs = UnitDefs::embedded();
    let amphibious = defs
        .sim_unit(defs.kind_of("amphibious_transport").unwrap())
        .unwrap();
    let costs = MovementCosts::embedded();
    let Some(Speed(water)) = costs.class_by_name("hover").unwrap().cost_on_name("water") else {
        panic!("hover crosses water");
    };
    assert_eq!(
        amphibious.speed.mul_int(i64::from(water)).div_int(1000),
        amphibious.water_speed
    );
}

#[test]
fn the_kind_numbers_of_the_classes_are_permanent() {
    let defs = UnitDefs::embedded();
    let classes = defs.classes();
    for (name, kind, _) in SHIPPED {
        assert_eq!(classes.movement_kind(name), Some(kind), "{name}");
        assert_eq!(classes.movement_class(kind).unwrap().name, name);
    }
    // A shuffled file (classes in another order, each class's costs in another
    // order) gives the same costs.
    let shuffled = load_classes(|t| {
        let mut lines: Vec<&str> = t.lines().collect();
        // Reverse the six cost lines of every class: they are consecutive.
        let mut out = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            if lines[i].trim_start().starts_with("(\"grass\"") {
                let mut j = i;
                while lines[j].trim_start().starts_with("(\"") {
                    j += 1;
                }
                lines[i..j].reverse();
                out.extend_from_slice(&lines[i..j]);
                i = j;
            } else {
                out.push(lines[i]);
                i += 1;
            }
        }
        out.join("\n")
    })
    .expect("the costs listed in another order load");
    let types = TerrainTypes::embedded();
    assert_eq!(
        MovementCosts::from_defs(&shuffled, &types).unwrap(),
        MovementCosts::embedded()
    );
}

// ---- refusals -----------------------------------------------------------------

#[test]
fn a_class_without_a_cost_for_a_terrain_type_is_refused() {
    let defs = load_classes(|t| t.replacen("            (\"ore\", Speed(800)),\n", "", 1))
        .expect("the file alone does not know the terrain types");
    let err = MovementCosts::from_defs(&defs, &TerrainTypes::embedded()).unwrap_err();
    assert_eq!(
        err,
        MovementError::MissingCost {
            class: "foot".into(),
            terrain: "ore".into()
        }
    );
    assert!(
        err.to_string().contains("no cost for terrain \"ore\""),
        "{err}"
    );
}

#[test]
fn a_cost_for_a_terrain_type_that_does_not_exist_is_refused() {
    let defs =
        load_classes(|t| t.replacen("(\"ore\", Speed(800))", "(\"orre\", Speed(800))", 1)).unwrap();
    // The typo leaves "ore" without a cost too; the unknown name is named first.
    let err = MovementCosts::from_defs(&defs, &TerrainTypes::embedded()).unwrap_err();
    assert_eq!(
        err,
        MovementError::UnknownTerrain {
            class: "foot".into(),
            terrain: "orre".into()
        }
    );
}

#[test]
fn a_new_terrain_type_needs_a_cost_from_every_class() {
    let mut types: Vec<TerrainType> = TerrainTypes::embedded().iter().cloned().collect();
    types.push(TerrainType {
        id: 6,
        name: "marsh".into(),
        buildable: false,
        harvestable: false,
    });
    let types = TerrainTypes::new(types).unwrap();
    let defs = UnitDefs::embedded();
    let err = MovementCosts::from_defs(&defs, &types).unwrap_err();
    assert!(
        matches!(&err, MovementError::MissingCost { terrain, .. } if terrain == "marsh"),
        "{err}"
    );
    // With a line in every class (data only) it is complete.
    let with_marsh = load_classes(|t| {
        let mut out = String::new();
        for line in t.lines() {
            out.push_str(line);
            out.push('\n');
            if line.trim_start().starts_with("(\"bridge\"") {
                out.push_str("            (\"marsh\", Speed(500)),\n");
            }
        }
        out
    })
    .unwrap();
    let costs = MovementCosts::from_defs(&with_marsh, &types).unwrap();
    assert_eq!(costs.cost(0, 6), Some(Speed(500)), "foot on marsh");
    assert_eq!(costs.terrain_ids(), [0, 1, 2, 3, 4, 5, 6]);
}

#[test]
fn a_bad_list_is_refused_with_the_file_and_the_line() {
    let line_of =
        |text: &str, needle: &str| 1 + text[..text.find(needle).unwrap()].matches('\n').count();
    let text = classes_text();
    // A terrain listed twice in one class.
    let e = load_classes(|t| {
        t.replacen(
            "(\"road\", Speed(1100)),",
            "(\"road\", Speed(1100)), (\"road\", Speed(900)),",
            1,
        )
    })
    .unwrap_err();
    assert!(e.message.contains("\"road\" is listed twice"), "{e}");
    assert_eq!(e.file, "classes.ron");
    assert_eq!(e.line, Some(line_of(&text, "name: \"foot\"")), "{e}");
    // A speed outside 100..=2000, and the 0 that should be Impassable.
    for bad in ["Speed(0)", "Speed(99)", "Speed(2001)", "Speed(65535)"] {
        let e = load_classes(|t| t.replacen("Speed(1100)", bad, 1)).unwrap_err();
        assert!(
            e.message.contains("must be between 100 and 2000"),
            "{bad}: {e}"
        );
        assert!(e.message.contains("Impassable"), "{bad}: {e}");
        assert!(e.line.is_some(), "{bad}: {e}");
    }
    // A terrain name that cannot be one.
    let e = load_classes(|t| t.replacen("(\"road\", Speed(1100))", "(\"Road\", Speed(1100))", 1))
        .unwrap_err();
    assert!(e.message.contains("not snake_case"), "{e}");
    // Not a cost at all, a negative number, a decimal.
    for bad in ["Fast(3)", "Speed(-5)", "Speed(1.5)", "Speed(12e2)"] {
        assert!(
            load_classes(|t| t.replacen("Speed(1100)", bad, 1)).is_err(),
            "{bad}"
        );
    }
    // The old file shape, a class with no costs at all, no longer loads.
    let old = "(movement_classes: [(kind: 0, name: \"foot\")], armor_types: [(kind: 0, name: \"none\")], \
               damage_types: [(kind: 0, name: \"bullet\")])";
    assert!(ClassTables::from_ron("old.ron", old).is_err());
}

#[test]
fn the_costs_of_a_class_are_part_of_the_class() {
    // The class API: `cost_on` by terrain name.
    let defs = UnitDefs::embedded();
    let tracked = defs.classes().movement_class(2).unwrap();
    assert_eq!(tracked.cost_on("road"), Some(Speed(1200)));
    assert_eq!(tracked.cost_on("lava"), None);
    assert_eq!(Speed(1200).permille(), 1200);
    assert_eq!(Impassable.permille(), 0);
    assert!(Speed(100).is_passable() && !Impassable.is_passable());
}

// ---- how a class turns (WP4.5) ---------------------------------------------

#[test]
fn the_shipped_classes_turn_as_the_rules_say() {
    let defs = UnitDefs::embedded();
    let turning = |name: &str| {
        let kind = defs.classes().movement_kind(name).unwrap();
        defs.classes().movement_class(kind).unwrap().turning
    };
    // Infantry turn at once; tanks pivot; everything else has to move to turn.
    assert_eq!(turning("foot").mode, TurnMode::Instant);
    assert_eq!(turning("tracked").mode, TurnMode::Pivot);
    for name in ["wheeled", "hover", "water"] {
        let t = turning(name);
        assert_eq!(t.mode, TurnMode::Arc, "{name}");
        assert!(t.min_radius.milli() >= 250, "{name}: a radius");
        assert!((1..=1000).contains(&t.pivot_permille), "{name}");
    }
    // A class that says nothing turns as the walk did before WP4.5: pivot, 16 steps.
    let air = turning("air");
    assert_eq!((air.mode, air.drive_tolerance), (TurnMode::Pivot, 16));
    // The numbers are the bridge's to copy, in cells and steps.
    let costs = MovementCosts::embedded();
    assert_eq!(
        costs.class_by_name("wheeled").unwrap().turning,
        turning("wheeled")
    );
}

#[test]
fn a_bad_turning_block_is_refused_with_the_file_and_the_line() {
    let line_of =
        |text: &str, needle: &str| 1 + text[..text.find(needle).unwrap()].matches('\n').count();
    let text = classes_text();
    let tracked = "turning: (mode: Pivot, drive_tolerance: 16)";
    assert!(text.contains(tracked), "test setup");
    let with = |block: &str| load_classes(|t| t.replacen(tracked, block, 1));
    // The block may be partly written: the rest is the old walk.
    assert!(with("turning: (mode: Pivot)").is_ok());
    assert!(with("turning: (drive_tolerance: 8)").is_ok());
    assert!(with("turning: (mode: Arc, min_radius: 1.25, pivot_permille: 400)").is_ok());
    for (bad, why) in [
        ("turning: (mode: Pivot, drive_tolerance: 65)", "at most 64"),
        ("turning: (mode: Arc)", "min_radius between"),
        (
            "turning: (mode: Arc, min_radius: 0.2)",
            "min_radius between",
        ),
        ("turning: (mode: Arc, min_radius: 33)", "min_radius between"),
        (
            "turning: (mode: Arc, min_radius: 2, pivot_permille: 0)",
            "pivot_permille 0",
        ),
        (
            "turning: (mode: Arc, min_radius: 2, pivot_permille: 1001)",
            "pivot_permille 1001",
        ),
        ("turning: (mode: Pivot, min_radius: 2)", "for mode Arc only"),
        (
            "turning: (mode: Instant, pivot_permille: 500)",
            "for mode Arc only",
        ),
        ("turning: (mode: Slide)", "Slide"),
        ("turning: (speed: 3)", "speed"),
    ] {
        let e = with(bad).unwrap_err();
        assert!(e.message.contains(why), "{bad}: {e}");
        assert_eq!(e.file, "classes.ron", "{bad}");
        assert!(e.line.is_some(), "{bad}: {e}");
    }
    // A check on the class names the class, on the line of its record.
    let e = with("turning: (mode: Pivot, drive_tolerance: 65)").unwrap_err();
    assert!(e.message.contains("\"tracked\""), "{e}");
    assert_eq!(e.line, Some(line_of(&text, "name: \"tracked\"")), "{e}");
}
