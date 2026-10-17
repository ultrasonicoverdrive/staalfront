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
        assert!(defs.classes().movement_index(name).is_some(), "{name}");
    }
}

/// The order of the roles is the kind number. This is the roster as WP3.1
/// shipped it: new roles go after it, nothing moves. If this test fails because
/// of a reorder, saves and scenarios that name a kind number would break.
const ROSTER_ORDER: &[&str] = &[
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
fn kind_numbers_are_the_file_order() {
    let defs = shipped();
    assert!(defs.units().len() >= ROSTER_ORDER.len());
    for (kind, id) in ROSTER_ORDER.iter().enumerate() {
        assert_eq!(defs.units()[kind].id, *id, "kind {kind}");
        assert_eq!(defs.kind_of(id), Some(kind as u16));
        assert_eq!(usize::from(defs.sim_units()[kind].kind), kind);
    }
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
        // Capacities, radii and sight in the notes.
        if let Some(n) = number_after(&all, "Carries ") {
            assert_eq!(i64::from(u.flags.transports) * 1000, n, "{role} carries");
        }
        if let Some(n) = number_after(&all, "Garrison ") {
            assert_eq!(i64::from(u.flags.garrison) * 1000, n, "{role} garrison");
        }
        if let Some(n) = number_after(&all, "Build radius ") {
            assert_eq!(u.flags.build_radius.milli(), n, "{role} build radius");
        }
        if let Some(n) = number_after(&all, "Sight ") {
            assert_eq!(u.sight.milli(), n, "{role} sight");
        }
        if let Some(n) = number_after(&all, "reveals ") {
            assert_eq!(u.sight.milli(), n, "{role} reveals");
        }
        if let Some(n) = number_after(&all, "Heals infantry in radius ") {
            assert_eq!(u.flags.heal_radius.milli(), n, "{role} heal radius");
        }
        if let Some(n) = number_after(&all, "Repairs vehicles in radius ") {
            assert_eq!(u.flags.repair_radius.milli(), n, "{role} repair radius");
        }
        if let Some(n) = number_after(&all, "damages radius ") {
            let blast = u
                .flags
                .death_blast
                .unwrap_or_else(|| panic!("{role} blast"));
            assert_eq!(blast.radius.milli(), n, "{role} blast radius");
        }
    }
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
            id: "hover_scout", factions: [Meridian], category: Vehicle,
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
    assert_eq!(usize::from(kind), base.units().len());
    // Every earlier kind kept its number and its numbers.
    assert_eq!(&defs.sim_units()[..base.units().len()], base.sim_units());
    let sim = &defs.sim_units()[usize::from(kind)];
    assert_eq!(sim.hp, 120);
    assert_eq!(sim.cost, Some(450));
    assert_eq!(sim.build_ticks, 240); // 12 s at 20 Hz
    assert_eq!(sim.speed, Fx::from_ratio(7, 2));
    assert_eq!(sim.water_speed, Fx::from_int(3));
    assert_eq!(sim.sight, Fx::from_int(9));
    assert_eq!(sim.movement_class, defs.classes().movement_index("hover"));
    assert_eq!(sim.weapons, vec![0]); // "rifle" is the first weapon
    assert!(sim.available_to(Faction::Meridian));
    assert!(!sim.available_to(Faction::Veyran));
}

#[test]
fn a_new_weapon_damage_type_and_movement_class_are_data_too() {
    let base = shipped();
    let extra = r#"
        (
            id: "plasma_walker", factions: [Veyran], category: Vehicle,
            cost: 2000, build_seconds: 60, hp: 700, armor: "heavy",
            movement: "strider", speed: 0.9, turn_rate: 5, size: 1.6, sight: 8,
            weapons: ["plasma_lance"],
            model: Shape(Cylinder), strategic_icon: Triangle,
        ),
"#;
    let defs = load_with(
        |t| {
            t.replacen(
                "(name: \"air\"),",
                "(name: \"air\"), (name: \"strider\"),",
                1,
            )
            .replacen(
                "(name: \"demolition\"),",
                "(name: \"demolition\"), (name: \"plasma\"),",
                1,
            )
        },
        |t| {
            with_extra(
                t,
                "        (id: \"plasma_lance\", range: 6, reload: 3, damage: 150, damage_type: \"plasma\", projectile: Falling, splash_radius: 2),\n",
            )
        },
        |t| with_extra(t, extra),
    );
    let defs = defs.expect("new weapon, damage type and movement class load");
    let walker = defs.unit("plasma_walker").unwrap();
    let sim = &defs.sim_units()[usize::from(defs.kind_of("plasma_walker").unwrap())];
    assert_eq!(sim.movement_class, Some(6));
    let lance = &defs.sim_weapons()[usize::from(sim.weapons[0])];
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
fn model_and_icon_files_must_exist_and_stay_inside_assets() {
    let e = replace_units(
        "testpack_v1/models/lichte_tank.glb",
        "testpack_v1/models/no_such_tank.glb",
    )
    .unwrap_err();
    assert!(e.message.contains("does not exist"), "{e}");
    assert!(e.line.is_some());
    let e = replace_units(
        "testpack_v1/icons/icon_lichte_tank_64.png",
        "../Cargo.toml.png",
    )
    .unwrap_err();
    assert!(e.message.contains("not a plain relative path"), "{e}");
    let e = replace_units(
        "testpack_v1/models/lichte_tank.glb",
        "testpack_v1/icons/icon_lichte_tank_64.png",
    )
    .unwrap_err();
    assert!(e.message.contains("should end with .glb"), "{e}");
}

#[test]
fn numbers_must_be_exact_and_in_range() {
    let e = replace_units("speed: 1.2,", "speed: 1.2345,").unwrap_err();
    assert!(e.message.contains("more than three decimals"), "{e}");
    assert!(e.line.is_some(), "a parse error carries its line: {e}");
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
    let sim = |id: &str| &defs.sim_units()[usize::from(defs.kind_of(id).unwrap())];
    let tank = sim("light_tank");
    assert_eq!(tank.speed, Fx::from_int(2));
    assert_eq!(tank.water_speed, tank.speed);
    assert_eq!(tank.hp, 400);
    assert_eq!(tank.cost, Some(700));
    assert_eq!(tank.build_ticks, 400); // 20 s
    assert_eq!(tank.turn_per_tick, 10);
    assert_eq!(tank.size, Fx::from_ratio(6, 5));
    assert_eq!(tank.footprint, None);
    let cannon = &defs.sim_weapons()[usize::from(tank.weapons[0])];
    assert_eq!(cannon.range, Fx::from_int(5));
    assert_eq!(cannon.reload_ticks, 40); // 2 s
    assert_eq!(cannon.projectile, Projectile::Straight);
    assert_eq!(sim("rifleman").speed, Fx::from_ratio(7, 10));
    let amph = sim("amphibious_transport");
    assert_eq!(amph.speed, Fx::from_ratio(3, 2));
    assert_eq!(amph.water_speed, Fx::from_ratio(6, 5));
    let howitzer = &defs.sim_weapons()[usize::from(sim("siege_artillery").weapons[0])];
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
            let per_tick = sf_sim::per_tick(u.speed);
            assert!(per_tick.raw() > 0, "kind {} stands still", u.kind);
            assert!(u.turn_per_tick >= 1);
        }
    }
}

#[test]
fn tick_constants_match_the_simulation() {
    assert_eq!(sf_data::units::TICKS_PER_SECOND, sf_sim::TICKS_PER_SECOND);
    assert_eq!(sf_data::units::TICK_MILLIS, i64::from(sf_sim::TICK_MILLIS));
}

#[test]
fn the_conversion_module_names_no_float_type() {
    let source = include_str!("../src/units/sim.rs");
    for banned in ["f32", "f64", "HashMap", "HashSet"] {
        assert!(!source.contains(banned), "sim.rs mentions {banned}");
    }
}
