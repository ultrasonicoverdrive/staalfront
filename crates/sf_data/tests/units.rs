//! WP3.1: the unit, structure and weapon definitions.
//!
//! * the shipped files load, every reference resolves (weapons, models, icons);
//! * every role of the tables in `content/units.md` exists, with the cost,
//!   power, hit points, speed, weapon range and capacities that file states,
//!   and with the faction that its tech tree gives it;
//! * the structure footprints are the art brief's proposal;
//! * a role added to the data file needs no Rust change (quality rule 6);
//! * a bad file is refused with the file and the line;
//! * the conversion for the simulation is fixed point, in the sim's units.

use std::path::{Path, PathBuf};

use sf_data::units::{
    Category, Decimal, DefFile, DefSources, EMBEDDED_CLASSES, EMBEDDED_UNITS, EMBEDDED_WEAPONS,
    Faction, Projectile, UnitDefs,
};
use sf_math::Fx;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn shipped() -> UnitDefs {
    let root = repo();
    UnitDefs::load(&root.join("assets/config"), &root.join("assets"))
        .unwrap_or_else(|e| panic!("the shipped definitions load: {e}"))
}

/// The text with Unix line ends: a Windows checkout may turn the files into
/// CRLF, and the substitutions below match across lines.
fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Loads the shipped files with a substitution applied to one of them.
fn load_with(
    classes: impl Fn(&str) -> String,
    weapons: impl Fn(&str) -> String,
    units: impl Fn(&str) -> String,
) -> Result<UnitDefs, sf_data::units::DefError> {
    let (c, w, u) = (
        classes(&lf(EMBEDDED_CLASSES)),
        weapons(&lf(EMBEDDED_WEAPONS)),
        units(&lf(EMBEDDED_UNITS)),
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
    UnitDefs::from_sources(&sources, Some(&repo().join("assets")))
}

fn replace_units(from: &str, to: &str) -> Result<UnitDefs, sf_data::units::DefError> {
    assert!(
        lf(EMBEDDED_UNITS).contains(from),
        "test setup: {from:?} not found"
    );
    load_with(str::to_string, str::to_string, |t| t.replacen(from, to, 1))
}

// ---- the shipped data ---------------------------------------------------

#[test]
fn shipped_definitions_load_and_every_reference_resolves() {
    let defs = shipped();
    assert!(defs.units().len() >= 43);
    assert_eq!(defs.sim_units().len(), defs.units().len());
    assert_eq!(defs.sim_weapons().len(), defs.weapons().len());
    // Loading from disk checked that every model and icon file exists; the
    // embedded copy is the same text.
    let embedded = UnitDefs::embedded();
    assert_eq!(embedded.units(), defs.units());
    assert_eq!(embedded.weapons(), defs.weapons());
    assert_eq!(embedded.classes(), defs.classes());
    assert_eq!(embedded.sim_units(), defs.sim_units());
}

#[test]
fn all_six_movement_classes_exist() {
    let defs = shipped();
    for name in ["foot", "wheeled", "tracked", "hover", "water", "air"] {
        assert!(defs.classes().movement_kind(name).is_some(), "{name}");
    }
}

/// The kind numbers as WP3.1 shipped them: the role at position `n` has kind
/// `n`. Numbers are permanent (`docs/rules/units.md`), so this table only ever
/// grows at the end; a changed or reused number would break saves and scenarios.
const SHIPPED_KINDS: &[&str] = &[
    "construction_center",
    "power_plant",
    "refinery",
    "barracks",
    "vehicle_factory",
    "radar",
    "defence_tower",
    "advanced_defences",
    "shipyard",
    "decoy_emplacement",
    "fortified_bunker",
    "harvester",
    "rifleman",
    "rocket_trooper",
    "engineer",
    "scout",
    "sniper",
    "medic",
    "demolition_team",
    "light_tank",
    "heavy_tank",
    "siege_artillery",
    "repair_truck",
    "transport_truck",
    "light_transport",
    "amphibious_transport",
    "combat_engineer",
    "heavy_gun_team",
    "gunboat",
    "supply_truck",
    "civilian",
    "technician",
    "relay_transmitter",
    "fuel_depot",
    "archive_building",
    "evidence_crate",
    "ferry",
    "bridge_segment",
    "rail_spur",
    "automated_gun",
    "generator",
    "unmanned_artillery",
    "ore_field",
];

#[test]
fn kind_numbers_are_the_shipped_ones() {
    let defs = shipped();
    for (kind, id) in SHIPPED_KINDS.iter().enumerate() {
        assert_eq!(defs.kind_of(id), Some(kind as u16), "kind of {id}");
        assert_eq!(defs.unit_by_kind(kind as u16).unwrap().id, *id);
        assert_eq!(defs.sim_unit(kind as u16).unwrap().kind, kind as u16);
    }
    // Weapons and the small tables keep their numbers too.
    for (kind, id) in ["rifle", "rocket", "long_rifle", "pistol", "carbine"]
        .iter()
        .enumerate()
    {
        assert_eq!(defs.weapon(id).unwrap().kind, kind as u16, "{id}");
    }
    let classes = defs.classes();
    for (kind, name) in ["foot", "wheeled", "tracked", "hover", "water", "air"]
        .iter()
        .enumerate()
    {
        assert_eq!(classes.movement_kind(name), Some(kind as u16), "{name}");
    }
    assert_eq!(classes.armor_kind("structure"), Some(4));
    assert_eq!(classes.damage_kind("demolition"), Some(5));
}

#[test]
fn structure_footprints_follow_the_art_brief() {
    let defs = shipped();
    let expected = [
        ("construction_center", 4, 4),
        ("power_plant", 2, 3),
        ("refinery", 3, 4),
        ("barracks", 2, 3),
        ("vehicle_factory", 3, 4),
        ("radar", 2, 2),
        ("defence_tower", 1, 1),
        ("advanced_defences", 2, 2),
        ("shipyard", 4, 5),
        ("decoy_emplacement", 1, 1),
        ("fortified_bunker", 2, 2),
    ];
    for (id, w, h) in expected {
        let u = defs.unit(id).unwrap();
        let fp = u
            .footprint
            .unwrap_or_else(|| panic!("{id} has a footprint"));
        assert_eq!((fp.width, fp.height), (w, h), "{id}");
    }
}

#[test]
fn no_forbidden_names() {
    let defs = shipped();
    for banned in ["construction_yard", "naval_yard"] {
        assert!(defs.unit(banned).is_none());
        assert!(!EMBEDDED_UNITS.contains(banned));
    }
}

// ---- content/units.md ---------------------------------------------------

/// One data row of a table of `content/units.md`.
struct Row {
    section: String,
    cells: Vec<String>,
}

fn units_md_rows() -> Vec<Row> {
    let text = std::fs::read_to_string(repo().join("content/units.md")).expect("units.md");
    let mut section = String::new();
    let mut rows = Vec::new();
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            section = h.trim().to_string();
        } else if line.starts_with('|') {
            let cells: Vec<String> = line
                .trim()
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim().to_string())
                .collect();
            let first = cells[0].as_str();
            if first == "Role"
                || first == "After"
                || first.starts_with('-')
                || first.starts_with(':')
            {
                continue;
            }
            rows.push(Row {
                section: section.clone(),
                cells,
            });
        }
    }
    rows
}

const ROLE_SECTIONS: [&str; 3] = [
    "Shared structures",
    "Shared units",
    "Enemy-only / mission objects",
];

fn role_rows() -> Vec<Row> {
    units_md_rows()
        .into_iter()
        .filter(|r| ROLE_SECTIONS.contains(&r.section.as_str()))
        .collect()
}

/// The number in thousandths at the start of `text` (after an optional sign
/// or the unicode minus), and what follows it.
fn leading_number(text: &str) -> Option<(i64, &str)> {
    let t = text.trim_start();
    let (neg, t) = if let Some(r) = t.strip_prefix('\u{2212}').or_else(|| t.strip_prefix('-')) {
        (true, r)
    } else {
        (false, t.strip_prefix('+').unwrap_or(t))
    };
    let end = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(t.len());
    if end == 0 {
        return None;
    }
    let (num, rest) = t.split_at(end);
    let (whole, frac) = num.split_once('.').unwrap_or((num, ""));
    let mut milli = whole.parse::<i64>().ok()? * 1000;
    let frac3 = format!("{frac:0<3}");
    milli += frac3[..3].parse::<i64>().ok()?;
    Some((if neg { -milli } else { milli }, rest))
}

/// The number that follows `prefix` somewhere in `text`.
fn number_after(text: &str, prefix: &str) -> Option<i64> {
    let at = text.find(prefix)? + prefix.len();
    leading_number(&text[at..]).map(|(n, _)| n)
}

/// The roles in the tables of `content/units.md`, taken from the file itself.
fn roster() -> Vec<String> {
    role_rows()
        .into_iter()
        .map(|r| r.cells[0].clone())
        .collect()
}

#[test]
fn every_role_of_units_md_exists_in_the_defs() {
    let defs = shipped();
    let roles = roster();
    // The parser found the three tables.
    assert!(roles.len() >= 40, "found only {} roles", roles.len());
    let mut missing = Vec::new();
    for role in &roles {
        let valid = !role.is_empty()
            && role
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        assert!(valid, "unparsed role cell {role:?}");
        if defs.unit(role).is_none() {
            missing.push(role.clone());
        }
    }
    assert!(
        missing.is_empty(),
        "roles in units.md but not in the defs: {missing:?}"
    );
}

#[test]
fn every_id_in_the_defs_is_a_role_of_units_md() {
    // The reverse of the test above: a role deleted from units.md (or a typo in
    // the defs) must fail here, not pass unnoticed.
    let defs = shipped();
    let roles = roster();
    assert!(roles.len() >= 40, "found only {} roles", roles.len());
    let extra: Vec<&str> = defs
        .units()
        .iter()
        .map(|u| u.id.as_str())
        .filter(|id| !roles.iter().any(|r| r == id))
        .collect();
    assert!(
        extra.is_empty(),
        "ids in the defs but not in units.md: {extra:?}"
    );
    // Same count, so no role is listed twice on either side.
    assert_eq!(roles.len(), defs.units().len());
}

#[test]
fn defs_carry_the_stats_of_units_md() {
    let defs = shipped();
    for row in role_rows() {
        let role = &row.cells[0];
        let u = defs.unit(role).unwrap_or_else(|| panic!("{role}"));
        let all = row.cells.join(" | ");
        let cell = |i: usize| row.cells[i].as_str();
        let int = |s: &str| -> Option<i64> { leading_number(s).map(|(n, _)| n / 1000) };
        match row.section.as_str() {
            "Shared structures" => {
                // Role | Cost | Power | HP | Notes
                assert_eq!(u.cost.map(i64::from), int(cell(1)), "{role} cost");
                assert_eq!(i64::from(u.power), int(cell(2)).unwrap(), "{role} power");
                assert_eq!(i64::from(u.hp), int(cell(3)).unwrap(), "{role} hp");
                assert_eq!(u.category, Category::Structure, "{role}");
            }
            "Shared units" => {
                // Role | Cost | HP | Speed | Weapon | Notes
                assert_eq!(u.cost.map(i64::from), int(cell(1)), "{role} cost");
                assert_eq!(i64::from(u.hp), int(cell(2)).unwrap(), "{role} hp");
                let (speed, _) = leading_number(cell(3)).unwrap();
                assert_eq!(u.speed.milli(), speed, "{role} speed");
                if cell(3).contains("land /") {
                    let water = number_after(cell(3), "/ ").unwrap();
                    assert_eq!(
                        u.water_speed.map(Decimal::milli),
                        Some(water),
                        "{role} water"
                    );
                }
                assert_eq!(
                    cell(4) == "\u{2014}",
                    u.weapons.is_empty(),
                    "{role}: weapon column {:?} against weapons {:?}",
                    cell(4),
                    u.weapons
                );
            }
            _ => {
                // Role | HP | Notes
                assert_eq!(i64::from(u.hp), int(cell(1)).unwrap_or(0), "{role} hp");
                if let Some(speed) = number_after(cell(2), "Speed ") {
                    assert_eq!(u.speed.milli(), speed, "{role} speed");
                }
            }
        }
        // Weapon ranges, in the weapon or notes column.
        let mut longest = None;
        let mut shortest_min = None;
        let mut rest = all.as_str();
        while let Some(at) = rest.find("range ") {
            let is_min = rest[..at].ends_with("min ");
            if let Some((n, _)) = leading_number(&rest[at + 6..]) {
                if is_min {
                    shortest_min = Some(n);
                } else {
                    longest = Some(n);
                }
            }
            rest = &rest[at + 6..];
        }
        let weapons: Vec<_> = u.weapons.iter().map(|w| defs.weapon(w).unwrap()).collect();
        if let Some(range) = longest {
            let best = weapons.iter().map(|w| w.range.milli()).max();
            assert_eq!(best, Some(range), "{role}: longest weapon range");
        }
        if let Some(min) = shortest_min {
            assert!(
                weapons.iter().any(|w| w.min_range.milli() == min),
                "{role}: min range {min}"
            );
        }
        // Capacities and radii in the notes: both ways, so a flag that the notes
        // do not mention fails too (a missing phrase reads as zero).
        let milli_of = |phrase: &str| number_after(&all, phrase).unwrap_or(0);
        assert_eq!(
            i64::from(u.flags.transports) * 1000,
            milli_of("Carries "),
            "{role} carries"
        );
        assert_eq!(
            i64::from(u.flags.transports_vehicles) * 1000,
            milli_of("infantry or "),
            "{role} carries vehicles"
        );
        assert_eq!(
            i64::from(u.flags.garrison) * 1000,
            milli_of("Garrison "),
            "{role} garrison"
        );
        assert_eq!(
            u.flags.build_radius.milli(),
            milli_of("Build radius "),
            "{role} build radius"
        );
        assert_eq!(
            u.flags.heal_radius.milli(),
            milli_of("Heals infantry in radius "),
            "{role} heal radius"
        );
        assert_eq!(
            u.flags.repair_radius.milli(),
            milli_of("Repairs vehicles in radius "),
            "{role} repair radius"
        );
        assert_eq!(
            u.flags.death_blast.map_or(0, |b| b.radius.milli()),
            milli_of("damages radius "),
            "{role} blast radius"
        );
        // Sight is only stated for some roles; where it is, it must match.
        for phrase in ["Sight ", "reveals "] {
            if let Some(n) = number_after(&all, phrase) {
                assert_eq!(u.sight.milli(), n, "{role} {phrase}");
            }
        }
        // Relations between roles, in words: "Looks like X", "Comes with 1 X",
        // "build X in the field". Both ways.
        assert_eq!(
            u.flags.disguise_as.as_deref(),
            word_after(&all, "Looks like "),
            "{role} disguise"
        );
        assert_eq!(
            u.flags.comes_with.as_deref(),
            word_after(&all, "Comes with 1 "),
            "{role} comes with"
        );
        let builds: Vec<&str> = word_after(&all, "build ").into_iter().collect();
        let defined: Vec<&str> = u.flags.builds.iter().map(String::as_str).collect();
        assert_eq!(defined, builds, "{role} builds");
    }
}

/// The snake_case word that follows `prefix` somewhere in `text`.
fn word_after<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let at = text.find(prefix)? + prefix.len();
    let rest = &text[at..];
    let end = rest
        .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
        .unwrap_or(rest.len());
    (end > 0).then(|| &rest[..end])
}

#[test]
fn factions_follow_the_tech_tree_of_units_md() {
    let defs = shipped();
    let rows: Vec<Row> = units_md_rows()
        .into_iter()
        .filter(|r| r.section.starts_with("Tech tree"))
        .collect();
    assert_eq!(rows.len(), 10, "ten missions in the tech tree");
    let mut meridian: Vec<String> = Vec::new();
    let mut veyran: Vec<String> = Vec::new();
    let roles = |cell: &str| -> Vec<String> {
        if cell == "full roster" {
            return Vec::new();
        }
        cell.split(',')
            .filter_map(|t| t.split_whitespace().next())
            .map(str::to_string)
            .collect()
    };
    for row in &rows {
        let m = roles(&row.cells[1]);
        let v = if row.cells[2] == "same" {
            m.clone()
        } else {
            roles(&row.cells[2])
        };
        meridian.extend(m);
        veyran.extend(v);
    }
    let mut all: Vec<&String> = meridian.iter().chain(&veyran).collect();
    all.sort();
    all.dedup();
    assert!(all.len() >= 25, "parsed the tree: {all:?}");
    for role in all {
        let u = defs
            .unit(role)
            .unwrap_or_else(|| panic!("tech tree role {role} missing"));
        let mut want = Vec::new();
        if meridian.contains(role) {
            want.push(Faction::Meridian);
        }
        if veyran.contains(role) {
            want.push(Faction::Veyran);
        }
        let mut have = u.factions.clone();
        have.sort();
        assert_eq!(have, want, "factions of {role}");
    }
}

// ---- rule 6: adding a role is data only ---------------------------------

const EXTRA_UNIT: &str = r#"
        (
            kind: 43, id: "hover_scout", factions: [Meridian], category: Vehicle,
            cost: 450, build_seconds: 12, hp: 120, armor: "light",
            movement: "hover", speed: 3.5, water_speed: 3, turn_rate: 12, size: 0.9, sight: 9,
            weapons: ["rifle"],
            model: Shape(Cone), strategic_icon: Triangle,
        ),
"#;

/// Appends `extra` to the list of units of the shipped file.
fn with_extra(units: &str, extra: &str) -> String {
    let end = units.rfind("    ],").expect("end of the unit list");
    format!("{}{}{}", &units[..end], extra, &units[end..])
}

#[test]
fn a_role_added_to_the_file_needs_no_rust_change() {
    let base = shipped();
    let defs = load_with(str::to_string, str::to_string, |t| {
        with_extra(t, EXTRA_UNIT)
    })
    .expect("the file with one more unit loads");
    assert_eq!(defs.units().len(), base.units().len() + 1);
    let kind = defs.kind_of("hover_scout").unwrap();
    assert_eq!(kind, 43);
    // Every earlier kind kept its number and its numbers.
    assert_eq!(&defs.sim_units()[..base.units().len()], base.sim_units());
    let sim = defs.sim_unit(kind).unwrap();
    assert_eq!(sim.hp, 120);
    assert_eq!(sim.cost, Some(450));
    assert_eq!(sim.build_ticks, 240); // 12 s at 20 Hz
    assert_eq!(sim.speed, Fx::from_ratio(7, 2));
    assert_eq!(sim.water_speed, Fx::from_int(3));
    assert_eq!(sim.sight, Fx::from_int(9));
    assert_eq!(sim.movement_class, defs.classes().movement_kind("hover"));
    assert_eq!(sim.weapons, vec![0]); // "rifle" is the first weapon
    assert!(sim.available_to(Faction::Meridian));
    assert!(!sim.available_to(Faction::Veyran));
}

#[test]
fn a_new_weapon_damage_type_and_movement_class_are_data_too() {
    let base = shipped();
    let extra = r#"
        (
            kind: 43, id: "plasma_walker", factions: [Veyran], category: Vehicle,
            cost: 2000, build_seconds: 60, hp: 700, armor: "heavy",
            movement: "strider", speed: 0.9, turn_rate: 5, size: 1.6, sight: 8,
            weapons: ["plasma_lance"],
            model: Shape(Cylinder), strategic_icon: Triangle,
        ),
"#;
    let defs = load_with(
        |t| {
            // A movement class is a record with its terrain costs: one cost for
            // every terrain type (the walker climbs rock, slowly).
            t.replacen(
                "    ],\n    // The set of armor types",
                "        (kind: 6, name: \"strider\", terrain_costs: [(\"grass\", Speed(1000)), \
                 (\"road\", Speed(1000)), (\"water\", Impassable), (\"rock\", Speed(300)), \
                 (\"ore\", Speed(1000)), (\"bridge\", Speed(1000))]),\n    ],\n    \
                 // The set of armor types",
                1,
            )
            .replacen(
                "(kind: 5, name: \"demolition\"),",
                "(kind: 5, name: \"demolition\"), (kind: 6, name: \"plasma\"),",
                1,
            )
            // A new damage type needs its row in the armor table (the table is complete or refused).
            .replacen(
                "        (damage: \"demolition\", percent: [",
                "        (damage: \"plasma\", percent: [(\"none\", 0), (\"infantry\", 100), (\"light\", 100), \
                 (\"heavy\", 100), (\"structure\", 100)]),\n        (damage: \"demolition\", percent: [",
                1,
            )
        },
        |t| {
            with_extra(
                t,
                "        (kind: 15, id: \"plasma_lance\", range: 6, reload: 3, damage: 150, damage_type: \"plasma\", projectile: Falling, splash_radius: 2),\n",
            )
        },
        |t| with_extra(t, extra),
    );
    let defs = defs.expect("new weapon, damage type and movement class load");
    let walker = defs.unit("plasma_walker").unwrap();
    let sim = defs
        .sim_unit(defs.kind_of("plasma_walker").unwrap())
        .unwrap();
    assert_eq!(sim.movement_class, Some(6));
    let lance = defs.sim_weapon(sim.weapons[0]).unwrap();
    assert_eq!(lance.damage_type, 6);
    assert_eq!(lance.projectile, Projectile::Falling);
    assert_eq!(lance.reload_ticks, 60);
    assert_eq!(lance.splash_radius, Fx::from_int(2));
    assert_eq!(walker.weapons, ["plasma_lance"]);
    assert_eq!(defs.units().len(), base.units().len() + 1);
}

// ---- refusals with file and line ----------------------------------------

/// The 1-based line of the first occurrence of `needle` in `text`.
fn line_of(text: &str, needle: &str) -> usize {
    1 + text[..text.find(needle).unwrap()].matches('\n').count()
}

#[test]
fn unknown_weapon_is_refused_with_its_line() {
    let from = "weapons: [\"cannon\"]";
    let err = replace_units(from, "weapons: [\"cannon2\"]").unwrap_err();
    assert_eq!(err.file, "units.ron");
    assert!(err.message.contains("unknown weapon \"cannon2\""), "{err}");
    assert!(err.message.contains("light_tank"), "{err}");
    let text = EMBEDDED_UNITS.replacen(from, "weapons: [\"cannon2\"]", 1);
    assert_eq!(err.line, Some(line_of(&text, "\"cannon2\"")), "{err}");
    assert!(err.to_string().starts_with("units.ron:"), "{err}");
}

#[test]
fn duplicate_ids_are_refused_on_the_second_one() {
    let err = replace_units("id: \"barracks\"", "id: \"power_plant\"").unwrap_err();
    assert!(
        err.message.contains("\"power_plant\" appears twice"),
        "{err}"
    );
    let text = EMBEDDED_UNITS.replacen("id: \"barracks\"", "id: \"power_plant\"", 1);
    let second = text.match_indices("id: \"power_plant\"").nth(1).unwrap().0;
    let want = 1 + text[..second].matches('\n').count();
    assert_eq!(err.line, Some(want), "{err}");
}

#[test]
fn unresolved_references_are_refused() {
    let e = replace_units("armor: \"heavy\"", "armor: \"adamant\"").unwrap_err();
    assert!(e.message.contains("unknown armor type \"adamant\""), "{e}");
    let e = replace_units("movement: \"tracked\"", "movement: \"skate\"").unwrap_err();
    assert!(
        e.message.contains("unknown movement class \"skate\""),
        "{e}"
    );
    let e = replace_units("comes_with: \"harvester\"", "comes_with: \"harvestor\"").unwrap_err();
    assert!(e.message.contains("unknown unit \"harvestor\""), "{e}");
    let e = replace_units("builds: [\"fortified_bunker\"]", "builds: [\"rifleman\"]").unwrap_err();
    assert!(e.message.contains("not a Structure"), "{e}");
    let e = load_with(
        str::to_string,
        |t| t.replacen("damage_type: \"rocket\"", "damage_type: \"laser\"", 1),
        str::to_string,
    )
    .unwrap_err();
    assert!(e.message.contains("unknown damage type \"laser\""), "{e}");
    assert!(e.line.is_some(), "{e}");
}

#[test]
fn model_files_must_exist_and_stay_inside_assets() {
    let e = replace_units(
        "testpack_v1/models/lichte_tank.glb",
        "testpack_v1/models/no_such_tank.glb",
    )
    .unwrap_err();
    assert!(e.message.contains("does not exist"), "{e}");
    assert!(e.line.is_some());
    let e = replace_units("testpack_v1/models/lichte_tank.glb", "../Cargo.toml.glb").unwrap_err();
    assert!(e.message.contains("not a plain relative path"), "{e}");
    let e = replace_units(
        "testpack_v1/models/lichte_tank.glb",
        "testpack_v1/icons/icon_lichte_tank_64.png",
    )
    .unwrap_err();
    assert!(e.message.contains("should end with .glb"), "{e}");
}

/// Models per faction: a role keeps one generic `model` and may give a faction
/// its own entry in `models`. Fallback: the faction's entry, then `model`.
#[test]
fn a_role_can_have_a_model_per_faction() {
    use sf_data::units::Model;
    let defs = shipped();
    let get = |id: &str| defs.unit(id).unwrap();
    let glb = |p: &str| Model::Glb(p.to_owned());

    let rifleman = get("rifleman");
    assert_eq!(
        rifleman.model_for(Some(Faction::Meridian)),
        &glb("set_v3/meridian_rifleman.glb")
    );
    assert_eq!(
        rifleman.model_for(Some(Faction::Veyran)),
        &glb("set_v3/veyran_rifleman.glb")
    );
    // No entry for Neutral, and an owner that is no known side: the generic model.
    assert_eq!(rifleman.model_for(Some(Faction::Neutral)), &rifleman.model);
    assert_eq!(rifleman.model_for(None), &rifleman.model);
    assert_eq!(rifleman.all_models().count(), 3);

    // A role with no `models` is drawn with `model` whoever owns it.
    let sniper = get("sniper");
    assert!(sniper.models.is_empty());
    assert_eq!(sniper.model, glb("set_v3/meridian_sniper.glb"));
    for faction in [Some(Faction::Meridian), Some(Faction::Veyran), None] {
        assert_eq!(sniper.model_for(faction), &sniper.model);
    }

    // Every entry of every role is for a faction the role has.
    for def in defs.units() {
        for faction in def.models.keys() {
            assert!(def.factions.contains(faction), "{}: {faction:?}", def.id);
        }
    }
}

#[test]
fn models_per_faction_are_checked_like_the_generic_model() {
    // A faction entry may be a primitive too.
    let defs = replace_units(
        "icon: \"sniper\",",
        "models: {Meridian: Shape(Cube)},\n            icon: \"sniper\",",
    )
    .unwrap();
    let sniper = defs.unit("sniper").unwrap();
    assert_eq!(
        sniper.model_for(Some(Faction::Meridian)),
        &sf_data::units::Model::Shape(sf_data::units::ModelShape::Cube)
    );

    // A file that is not there.
    let e = replace_units(
        "set_v3/veyran_rifleman.glb",
        "set_v3/veyran_rifleman_v9.glb",
    )
    .unwrap_err();
    assert!(
        e.message.contains("models Veyran") && e.message.contains("does not exist"),
        "{e}"
    );
    assert!(e.line.is_some(), "{e}");
    // A path that leaves assets/.
    let e = replace_units("set_v3/veyran_rifleman.glb", "../veyran_rifleman.glb").unwrap_err();
    assert!(e.message.contains("not a plain relative path"), "{e}");
    // A faction that does not exist is a parse error with a line.
    let e = replace_units(
        "Veyran: Glb(\"set_v3/veyran_scout.glb\")",
        "Pirates: Shape(Cube)",
    )
    .unwrap_err();
    assert!(e.message.contains("Pirates"), "{e}");
    assert!(e.line.is_some(), "{e}");
    // A faction the role does not have (the sniper is Meridian's only).
    let e = replace_units(
        "icon: \"sniper\",",
        "models: {Veyran: Shape(Cube)},\n            icon: \"sniper\",",
    )
    .unwrap_err();
    assert!(
        e.message.contains("not one of the factions of this role"),
        "{e}"
    );
    assert!(e.line.is_some(), "{e}");
    // The same faction twice is refused, not silently resolved.
    let e = replace_units(
        "icon: \"sniper\",",
        "models: {Meridian: Shape(Cube), Meridian: Shape(Cone)},\n            icon: \"sniper\",",
    )
    .unwrap_err();
    assert!(e.message.contains("listed twice"), "{e}");
    assert!(e.line.is_some(), "{e}");
}

/// The text with every `Glb("...")` turned into a primitive.
fn strip_glbs(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("Glb(\"") {
        out.push_str(&rest[..at]);
        let after = &rest[at..];
        let end = after.find("\")").unwrap() + 2;
        out.push_str("Shape(Cube)");
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// A glTF model must lie in a pack of `config/models.ron`, or the renderer has
/// no axes to read it with: refused when the file is there to ask.
#[test]
fn a_glb_model_must_lie_in_a_model_pack() {
    let root = std::env::temp_dir().join(format!("sf_data_packs_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _cleanup = RemoveOnDrop(root.clone());
    std::fs::create_dir_all(root.join("config")).unwrap();
    std::fs::create_dir_all(root.join("pack_a")).unwrap();
    std::fs::create_dir_all(root.join("loose")).unwrap();
    std::fs::write(root.join("pack_a/a.glb"), b"x").unwrap();
    std::fs::write(root.join("loose/b.glb"), b"x").unwrap();
    std::fs::write(
        root.join("config/models.ron"),
        r#"(packs: {"a": (dir: "pack_a", axes: (front: PlusX, up: PlusY, metres_per_unit: 1.0), lod_px: [60.0], models: [])})"#,
    )
    .unwrap();
    let load = |generic: &str, faction: &str| {
        // The other roles' files are not in the temporary assets: they become primitives.
        let units = strip_glbs(&lf(EMBEDDED_UNITS).replacen(
            "model: Glb(\"set_v3/meridian_sniper.glb\")",
            "model: @G@, models: {Meridian: @F@}",
            1,
        ))
        .replace("@G@", &format!("Glb(\"{generic}\")"))
        .replace("@F@", &format!("Glb(\"{faction}\")"));
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
                text: &units,
            },
        };
        UnitDefs::from_sources(&sources, Some(&root))
    };
    load("pack_a/a.glb", "pack_a/a.glb").unwrap();
    let e = load("loose/b.glb", "pack_a/a.glb").unwrap_err();
    assert!(
        e.message.contains("is in no model pack") && e.message.contains("pack_a"),
        "{e}"
    );
    let e = load("pack_a/a.glb", "loose/b.glb").unwrap_err();
    assert!(
        e.message.contains("models Meridian") && e.message.contains("is in no model pack"),
        "{e}"
    );
}

/// Removes a directory when dropped, also when the test panics.
struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// WP3.6: `icon` is a name in the icon set, not a path. Only the spelling is
/// checked here (the renderer loads the set and falls back to the base shape
/// for a name it does not have), so an unknown name is not an error.
#[test]
fn icon_is_a_name_and_a_path_is_refused() {
    let defs = shipped();
    let tank = defs.units().iter().find(|u| u.id == "light_tank").unwrap();
    assert_eq!(tank.icon.as_deref(), Some("light_tank"));
    // A name that no icon file has is fine here.
    replace_units("icon: \"light_tank\"", "icon: \"no_such_icon_yet\"").unwrap();
    for bad in [
        "testpack_v1/icons/icon_lichte_tank_64.png",
        "../x",
        "Light_Tank",
        "light tank",
        "light_tank.png",
        "",
    ] {
        let e = replace_units("icon: \"light_tank\"", &format!("icon: \"{bad}\"")).unwrap_err();
        assert!(e.message.contains("a-z 0-9 _"), "{bad:?}: {e}");
        assert!(e.line.is_some(), "{bad:?}: {e}");
    }
    let long = "x".repeat(49);
    let e = replace_units("icon: \"light_tank\"", &format!("icon: \"{long}\"")).unwrap_err();
    assert!(e.message.contains("1 to 48"), "{e}");
}

/// Every shipped role's base shape is the one of its category (infantry a
/// circle, vessels a half-circle, and so on), so the fallback of a missing
/// icon is the base shape by category.
#[test]
fn the_base_shape_of_every_shipped_role_is_that_of_its_category() {
    use sf_data::units::{Category, IconShape};
    for u in shipped().units() {
        assert_eq!(
            u.strategic_icon,
            IconShape::of_category(u.category),
            "{} ({:?})",
            u.id,
            u.category
        );
    }
    assert_eq!(
        IconShape::of_category(Category::Vessel),
        IconShape::HalfCircle
    );
}

#[test]
fn numbers_must_be_exact_and_in_range() {
    let e = replace_units("speed: 1.2,", "speed: 1.2345,").unwrap_err();
    assert!(e.message.contains("three decimals"), "{e}");
    assert!(e.line.is_some(), "a parse error carries its line: {e}");
    // The decimals are counted on the text: these used to pass as 1.2 and 1.
    let e = replace_units("speed: 1.2,", "speed: 1.2000000001,").unwrap_err();
    assert!(e.message.contains("1.2000000001"), "{e}");
    let text = EMBEDDED_UNITS.replacen("speed: 1.2,", "speed: 1.2000000001,", 1);
    assert_eq!(e.line, Some(line_of(&text, "1.2000000001")), "{e}");
    let e = replace_units("speed: 1.2,", "speed: 1.2000,").unwrap_err();
    assert!(e.message.contains("three decimals"), "{e}");
    let e = replace_units("speed: 1.2,", "speed: 12e-1,").unwrap_err();
    assert!(e.message.contains("exponent"), "{e}");
    // The same in the weapons file.
    let e = load_with(
        str::to_string,
        |t| t.replacen("reload: 2.5,", "reload: 2.5000001,", 1),
        str::to_string,
    )
    .unwrap_err();
    assert!(e.message.contains("three decimals"), "{e}");
    assert_eq!(e.file, "weapons.ron");
    let e = replace_units("build_seconds: 15,", "build_seconds: 15.02,").unwrap_err();
    assert!(e.message.contains("whole number of ticks"), "{e}");
    let e = replace_units("hp: 400,", "hp: 0,").unwrap_err();
    assert!(e.message.contains("hp 0"), "{e}");
    let e = replace_units("footprint: (width: 2, height: 3), sight: 5,\n            model: Glb(\"testpack_v1/models/energiecentrale.glb\")", "sight: 5,\n            model: Glb(\"testpack_v1/models/energiecentrale.glb\")")
        .unwrap_err();
    assert!(e.message.contains("needs a footprint"), "{e}");
    let e = replace_units(
        "movement: \"foot\", speed: 0.7,",
        "movement: \"foot\", speed: 0,",
    )
    .unwrap_err();
    assert!(e.message.contains("speed 0"), "{e}");
}

#[test]
fn a_syntax_error_names_file_line_and_column() {
    let e = replace_units("cost: 700,", "cost: 700 hp: ,").unwrap_err();
    assert_eq!(e.file, "units.ron");
    assert!(e.line.is_some() && e.column.is_some(), "{e}");
    let text = EMBEDDED_UNITS.replacen("cost: 700,", "cost: 700 hp: ,", 1);
    let near = line_of(&text, "cost: 700 hp");
    assert!(
        e.line.unwrap().abs_diff(near) <= 1,
        "{e} (expected near line {near})"
    );
    // An unknown field is refused too.
    let e = replace_units("cost: 700,", "cost: 700, colour: 3,").unwrap_err();
    assert!(e.message.contains("colour"), "{e}");
}

#[test]
fn a_missing_file_is_reported() {
    let e = UnitDefs::load(&repo().join("assets/nonexistent"), &repo().join("assets")).unwrap_err();
    assert!(e.message.contains("cannot read"), "{e}");
    assert!(e.file.ends_with("unit_classes.ron"), "{e}");
}

// ---- the conversion for the simulation ----------------------------------

#[test]
fn conversion_is_fixed_point_in_the_sims_units() {
    let defs = shipped();
    let sim = |id: &str| defs.sim_unit(defs.kind_of(id).unwrap()).unwrap();
    let tank = sim("light_tank");
    assert_eq!(tank.speed, Fx::from_int(2));
    assert_eq!(tank.water_speed, tank.speed);
    assert_eq!(tank.hp, 400);
    assert_eq!(tank.cost, Some(700));
    assert_eq!(tank.build_ticks, 400); // 20 s
    assert_eq!(tank.turn_per_tick, 10);
    assert_eq!(tank.size, Fx::from_ratio(6, 5));
    assert_eq!(tank.footprint, None);
    let cannon = defs.sim_weapon(tank.weapons[0]).unwrap();
    assert_eq!(cannon.range, Fx::from_int(5));
    assert_eq!(cannon.reload_ticks, 40); // 2 s
    assert_eq!(cannon.projectile, Projectile::Straight);
    assert_eq!(sim("rifleman").speed, Fx::from_ratio(7, 10));
    let amph = sim("amphibious_transport");
    assert_eq!(amph.speed, Fx::from_ratio(3, 2));
    assert_eq!(amph.water_speed, Fx::from_ratio(6, 5));
    let howitzer = defs.sim_weapon(sim("siege_artillery").weapons[0]).unwrap();
    assert_eq!(howitzer.min_range, Fx::from_int(3));
    assert_eq!(howitzer.range, Fx::from_int(10));
    assert_eq!(howitzer.projectile, Projectile::Arc);
    let plant = sim("power_plant");
    assert_eq!((plant.power, plant.footprint), (100, Some((2, 3))));
    assert_eq!(plant.speed, Fx::ZERO);
    assert_eq!(plant.movement_class, None);
    assert_eq!(sim("refinery").power, -30);
    assert_eq!(sim("refinery").flags.comes_with, defs.kind_of("harvester"));
    assert_eq!(
        sim("decoy_emplacement").flags.disguise_as,
        defs.kind_of("defence_tower")
    );
    assert_eq!(sim("ore_field").hp, 0);
    assert_eq!(sim("ferry").speed, Fx::from_ratio(1, 5));
    // The mission document timers rest on these speeds (units.md).
    assert_eq!(sim("harvester").speed, Fx::from_ratio(6, 5));
    assert_eq!(sim("heavy_tank").speed, Fx::from_ratio(11, 10));
    assert_eq!(sim("light_transport").speed, Fx::from_ratio(5, 2));
}

#[test]
fn every_mobile_unit_moves_at_least_one_raw_unit_per_tick() {
    let defs = shipped();
    for u in defs.sim_units() {
        if u.movement_class.is_some() {
            assert!(u.speed_per_tick.raw() > 0, "kind {} stands still", u.kind);
            assert!(u.water_speed_per_tick.raw() > 0);
            assert!(u.turn_per_tick >= 1);
        } else {
            assert_eq!(u.speed_per_tick, Fx::ZERO);
        }
    }
}

/// The exact rounding of `milli / 1000 / 20` cells to 1/65536, to nearest with
/// ties away from zero, in plain integers (not the code under test).
fn exact_per_tick_raw(milli: i64) -> i64 {
    let numerator = milli * 65_536;
    let denominator = 1000 * 20;
    (2 * numerator + denominator) / (2 * denominator)
}

#[test]
fn speed_per_tick_is_rounded_once() {
    // heavy_tank, 1.1 cells per second: 3604.48 raw units per tick. Rounding
    // 1.1 to Fx first (72090) and dividing by 20 gave 3605.
    let defs = shipped();
    let tank = defs.sim_unit(defs.kind_of("heavy_tank").unwrap()).unwrap();
    assert_eq!(tank.speed_per_tick.raw(), 3604);
    assert_eq!(Decimal::from_milli(1100).to_fx_per_tick().raw(), 3604);
    assert_eq!(Decimal::from_milli(1100).to_fx().div_int(20).raw(), 3605);
    // The amphibious transport: water speed per tick is rounded once too.
    let amph = defs
        .sim_unit(defs.kind_of("amphibious_transport").unwrap())
        .unwrap();
    assert_eq!(amph.speed_per_tick.raw(), exact_per_tick_raw(1500));
    assert_eq!(amph.water_speed_per_tick.raw(), exact_per_tick_raw(1200));
    // Roles without a water speed use the land speed for water.
    assert_eq!(tank.water_speed_per_tick, tank.speed_per_tick);
}

#[test]
fn speed_per_tick_equals_the_exact_rounding_for_every_speed() {
    // Every speed the files may name: 0.001 to 20.000 cells per second.
    let mut double_rounding_differs = 0;
    for milli in 1..=20_000_i64 {
        let d = Decimal::from_milli(milli);
        let got = d.to_fx_per_tick().raw();
        assert_eq!(got, exact_per_tick_raw(milli), "{d} cells per second");
        if d.to_fx().div_int(20).raw() != got {
            double_rounding_differs += 1;
        }
    }
    // The test is meaningful: the two-step route is wrong for many speeds.
    assert!(double_rounding_differs > 400, "{double_rounding_differs}");
}

#[test]
fn shipped_speeds_per_tick_are_exact() {
    let defs = shipped();
    for (u, sim) in defs.units().iter().zip(defs.sim_units()) {
        assert_eq!(
            sim.speed_per_tick.raw(),
            exact_per_tick_raw(u.speed.milli())
        );
        let water = u.water_speed.unwrap_or(u.speed);
        assert_eq!(
            sim.water_speed_per_tick.raw(),
            exact_per_tick_raw(water.milli())
        );
    }
}

#[test]
fn the_tick_constants_are_the_20_hz_of_sf_math() {
    assert_eq!(sf_data::units::TICKS_PER_SECOND, sf_math::TICKS_PER_SECOND);
    assert_eq!(sf_data::units::TICK_MILLIS, i64::from(sf_math::TICK_MILLIS));
    assert_eq!(sf_math::TICKS_PER_SECOND, 20);
}

#[test]
fn the_conversion_module_names_no_float_type() {
    let source = include_str!("../src/units/sim.rs");
    for banned in ["f32", "f64", "HashMap", "HashSet"] {
        assert!(!source.contains(banned), "sim.rs mentions {banned}");
    }
}

// ---- permanent numbers ---------------------------------------------------

/// Reverses the order of the lines that start with `prefix`, keeping every
/// other line (comments, list brackets) where it is. With `per_run`, each run of
/// consecutive such lines is reversed on its own.
fn reverse_lines(text: &str, prefix: &str, per_run: bool) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let marked: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].starts_with(prefix))
        .collect();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for &i in &marked {
        match groups.last_mut() {
            Some(g) if per_run && g.last() == Some(&(i - 1)) => g.push(i),
            Some(g) if !per_run => g.push(i),
            _ => groups.push(vec![i]),
        }
    }
    let mut out: Vec<&str> = lines.clone();
    for g in groups {
        for (slot, &from) in g.iter().zip(g.iter().rev()) {
            out[*slot] = lines[from];
        }
    }
    out.join("\n")
}

/// Puts the multi-line records of the unit file in another order: reversed,
/// then rotated. Comments between the records are dropped.
fn shuffle_unit_records(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut records: Vec<Vec<&str>> = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    let (mut first, mut last_end) = (None, 0);
    for (i, line) in lines.iter().enumerate() {
        if *line == "        (" {
            current = Some(vec![*line]);
            first.get_or_insert(i);
        } else if let Some(c) = current.as_mut() {
            c.push(*line);
            if *line == "        )," {
                records.push(current.take().unwrap());
                last_end = i + 1;
            }
        }
    }
    assert_eq!(records.len(), 43);
    records.reverse();
    records.rotate_left(7);
    let mut out: Vec<&str> = lines[..first.unwrap()].to_vec();
    for r in &records {
        out.extend(r);
    }
    out.extend(&lines[last_end..]);
    out.join("\n")
}

#[test]
fn the_order_of_the_records_does_not_matter() {
    let base = shipped();
    let (c, w, u) = (
        reverse_lines(&lf(EMBEDDED_CLASSES), "        (kind:", true),
        reverse_lines(&lf(EMBEDDED_WEAPONS), "        (kind:", false),
        shuffle_unit_records(&lf(EMBEDDED_UNITS)),
    );
    // The texts really are different ...
    assert_ne!(c, lf(EMBEDDED_CLASSES));
    assert_ne!(w, lf(EMBEDDED_WEAPONS));
    assert_ne!(u, lf(EMBEDDED_UNITS));
    // ... and give identical definitions and identical sim definitions.
    let shuffled =
        load_with(|_| c.clone(), |_| w.clone(), |_| u.clone()).expect("a shuffled file loads");
    assert_eq!(shuffled.units(), base.units());
    assert_eq!(shuffled.weapons(), base.weapons());
    assert_eq!(shuffled.classes(), base.classes());
    assert_eq!(shuffled.sim_units(), base.sim_units());
    assert_eq!(shuffled.sim_weapons(), base.sim_weapons());
    for u in base.units() {
        assert_eq!(shuffled.kind_of(&u.id), Some(u.kind));
    }
}

fn with_retired(retired: &str) -> Result<UnitDefs, sf_data::units::DefError> {
    load_with(str::to_string, str::to_string, |t| {
        t.replacen(
            "retired_units: [],",
            &format!("retired_units: [{retired}],"),
            1,
        )
    })
}

#[test]
fn duplicate_kinds_are_refused_in_every_file() {
    // Units: radar (kind 5) takes the number of barracks (kind 3).
    let e = replace_units("kind: 5, id: \"radar\"", "kind: 3, id: \"radar\"").unwrap_err();
    assert!(
        e.message.contains("kind 3 is already used by \"barracks\""),
        "{e}"
    );
    assert!(e.line.is_some(), "{e}");
    // Weapons.
    let e = load_with(
        str::to_string,
        |t| t.replacen("(kind: 1, id: \"rocket\"", "(kind: 0, id: \"rocket\"", 1),
        str::to_string,
    )
    .unwrap_err();
    assert!(e.message.contains("already used by \"rifle\""), "{e}");
    assert_eq!(e.file, "weapons.ron");
    // Movement classes, armor types, damage types.
    for (from, to, what) in [
        (
            "(kind: 1, name: \"wheeled\", ground: true,",
            "(kind: 0, name: \"wheeled\", ground: true,",
            "movement class",
        ),
        (
            "(kind: 4, name: \"structure\")",
            "(kind: 3, name: \"structure\")",
            "armor type",
        ),
        (
            "(kind: 5, name: \"demolition\")",
            "(kind: 0, name: \"demolition\")",
            "damage type",
        ),
    ] {
        let e = load_with(|t| t.replacen(from, to, 1), str::to_string, str::to_string).unwrap_err();
        assert!(e.message.contains("already used by"), "{what}: {e}");
        assert!(e.message.contains(what), "{what}: {e}");
        assert!(e.line.is_some(), "{what}: {e}");
    }
}

#[test]
fn a_missing_kind_is_refused() {
    let e = replace_units("kind: 3, id: \"barracks\"", "id: \"barracks\"").unwrap_err();
    assert!(e.message.contains("kind"), "{e}");
    assert!(e.line.is_some(), "{e}");
}

#[test]
fn a_tombstone_keeps_its_number_taken() {
    // A tombstone for a number nobody uses is fine and is reported.
    let defs = with_retired("(kind: 90, name: \"old_scout\")").expect("an unused number");
    assert_eq!(defs.retired_units().len(), 1);
    assert_eq!(defs.retired_units()[0].kind, 90);
    assert!(defs.unit_by_kind(90).is_none());
    assert!(defs.sim_unit(90).is_none());
    // A live role on a retired number is refused ...
    let e = with_retired("(kind: 5, name: \"old_radar\")").unwrap_err();
    assert!(
        e.message.contains("kind 5 was retired by \"old_radar\""),
        "{e}"
    );
    // ... also a new role added later.
    let extra = r#"
        (
            kind: 90, id: "new_scout", factions: [Meridian], category: Infantry,
            hp: 20, armor: "infantry", movement: "foot", speed: 1, turn_rate: 16,
            size: 0.4, sight: 6, model: Shape(Cone), strategic_icon: Triangle,
        ),
"#;
    let e = load_with(str::to_string, str::to_string, |t| {
        with_extra(t, extra).replacen(
            "retired_units: [],",
            "retired_units: [(kind: 90, name: \"old_scout\")],",
            1,
        )
    })
    .unwrap_err();
    assert!(
        e.message.contains("kind 90 was retired by \"old_scout\""),
        "{e}"
    );
    assert!(e.line.is_some(), "{e}");
    // The same tombstone number twice is refused.
    let e = with_retired("(kind: 90, name: \"a\"), (kind: 90, name: \"b\")").unwrap_err();
    assert!(e.message.contains("already retired"), "{e}");
    // Tombstones of weapons and of the small tables.
    let e = load_with(
        str::to_string,
        |t| {
            t.replacen(
                "retired_weapons: [],",
                "retired_weapons: [(kind: 2, name: \"old_gun\")],",
                1,
            )
        },
        str::to_string,
    )
    .unwrap_err();
    assert!(
        e.message.contains("kind 2 was retired by \"old_gun\""),
        "{e}"
    );
    let e = load_with(
        |t| {
            t.replacen(
                "retired_armor_types: [],",
                "retired_armor_types: [(kind: 2, name: \"old_armor\")],",
                1,
            )
        },
        str::to_string,
        str::to_string,
    )
    .unwrap_err();
    assert!(e.message.contains("retired by \"old_armor\""), "{e}");
}

// ---- the turret (WP4.5) -------------------------------------------------------

#[test]
fn tanks_and_artillery_have_a_turret_that_turns_apart_from_the_hull() {
    let defs = shipped();
    let rate = |id: &str| {
        defs.sim_unit(defs.kind_of(id).unwrap())
            .unwrap()
            .turret_per_tick
    };
    assert_eq!(rate("light_tank"), 12);
    assert_eq!(rate("heavy_tank"), 8);
    assert_eq!(rate("siege_artillery"), 6);
    // Everything else has none: infantry, trucks, structures.
    for role in [
        "rifleman",
        "repair_truck",
        "harvester",
        "power_plant",
        "gunboat",
    ] {
        assert_eq!(rate(role), 0, "{role}");
    }
}

#[test]
fn a_turret_needs_a_movement_class() {
    // On a unit that moves, the field is just data.
    let defs = replace_units(
        "movement: \"tracked\", speed: 1, turn_rate: 5, turret_rate: 6,",
        "movement: \"tracked\", speed: 1, turn_rate: 5, turret_rate: 7,",
    )
    .unwrap();
    let kind = defs.kind_of("siege_artillery").unwrap();
    assert_eq!(defs.sim_unit(kind).unwrap().turret_per_tick, 7);
    // On a structure, which never moves, it is refused with the line.
    let e = replace_units(
        "hp: 600, armor: \"structure\", power: 100,",
        "hp: 600, armor: \"structure\", power: 100, turret_rate: 4,",
    )
    .unwrap_err();
    assert!(
        e.message.contains("turret_rate need a movement class"),
        "{e}"
    );
    assert!(e.line.is_some(), "{e}");
}
