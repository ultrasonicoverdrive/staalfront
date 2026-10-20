//! WP5.3: the armor table (`armor_table` in `assets/config/unit_classes.ron`) and the splash fields of a
//! weapon (`splash_inner`, `friendly_fire`).
//!
//! * the shipped table is complete and says what its comments say;
//! * a file without a table (the frozen copies of the scenarios) still loads, and a hit then does the
//!   weapon's damage;
//! * every way the table can be wrong is refused with the file and the line, one test each;
//! * the splash fields have defaults that change nothing, and are refused where they have no use.
//!
//! The rules are written in `docs/rules/armour.md`.

use std::path::{Path, PathBuf};

use sf_data::units::{
    DefError, DefFile, DefSources, EMBEDDED_CLASSES, EMBEDDED_UNITS, EMBEDDED_WEAPONS,
    MAX_ARMOR_PERCENT, UnitDefs,
};
use sf_math::Fx;

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

fn load(
    classes: impl Fn(&str) -> String,
    weapons: impl Fn(&str) -> String,
) -> Result<UnitDefs, DefError> {
    let (c, w, u) = (
        classes(&lf(EMBEDDED_CLASSES)),
        weapons(&lf(EMBEDDED_WEAPONS)),
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
    UnitDefs::from_sources(&sources, Some(&repo().join("assets")))
}

/// The classes file with the first `from` replaced by `to`.
fn change(from: &'static str, to: &'static str) -> impl Fn(&str) -> String {
    move |text: &str| {
        assert!(text.contains(from), "test setup: {from:?} not in the file");
        text.replacen(from, to, 1)
    }
}

/// The 1-based line of the `nth` (0-based) occurrence of `needle` in `text`.
fn line_of(text: &str, needle: &str, nth: usize) -> usize {
    let at = text.match_indices(needle).nth(nth).expect("test setup").0;
    1 + text[..at].matches('\n').count()
}

/// Loads with the classes file changed and expects it to be refused, naming the file, the message and
/// the line of `needle` (its `nth` occurrence in the changed file).
fn refused(edit: impl Fn(&str) -> String, message: &str, needle: &str, nth: usize) -> DefError {
    let e = load(&edit, str::to_string).unwrap_err();
    assert_eq!(e.file, "classes.ron", "{e}");
    assert!(e.message.contains(message), "want {message:?} in: {e}");
    let edited = edit(&lf(EMBEDDED_CLASSES));
    assert_eq!(
        e.line,
        Some(line_of(&edited, needle, nth)),
        "the error names the line of {needle:?}: {e}"
    );
    e
}

// ---- the shipped table -----------------------------------------------------------------------------

#[test]
fn the_shipped_table_is_complete_and_within_bounds() {
    let defs = shipped();
    let classes = defs.classes();
    assert!(classes.has_armor_table());
    assert_eq!(
        classes.armor_percents.len(),
        classes.damage_types.len() * classes.armor_types.len()
    );
    for d in &classes.damage_types {
        for a in &classes.armor_types {
            let p = classes
                .armor_percent(d.kind, a.kind)
                .unwrap_or_else(|| panic!("{} against {} has no percentage", d.name, a.name));
            assert!(p <= MAX_ARMOR_PERCENT, "{} against {}", d.name, a.name);
        }
    }
}

#[test]
fn the_shipped_table_says_what_its_comments_say() {
    let defs = shipped();
    let c = defs.classes();
    let p = |damage: &str, armor: &str| {
        c.armor_percent(c.damage_kind(damage).unwrap(), c.armor_kind(armor).unwrap())
            .unwrap()
    };
    // The armor that takes nothing takes nothing from anything.
    for d in &c.damage_types {
        assert_eq!(p(&d.name, "none"), 0, "{}", d.name);
    }
    // Rifles are poor against heavy armor, good against infantry.
    assert!(p("bullet", "heavy") < p("bullet", "light"));
    assert!(p("bullet", "light") < p("bullet", "infantry"));
    assert_eq!(p("bullet", "infantry"), 100);
    // The sniper hurts infantry only.
    assert_eq!(p("precision", "infantry"), 100);
    for armor in ["light", "heavy", "structure"] {
        assert_eq!(p("precision", armor), 0, "precision against {armor}");
    }
    // Rockets are good against vehicles.
    assert!(p("rocket", "light") > 100);
    assert!(p("rocket", "heavy") >= 100);
    assert!(p("rocket", "infantry") < 100);
    // Artillery is good against structures.
    assert!(p("shell", "structure") > 100);
    // A satchel takes down a structure: its damage is far above any structure's hit points.
    let satchel = defs.weapon("satchel_charge").unwrap();
    let most = defs
        .units()
        .iter()
        .filter(|u| u.armor == "structure")
        .map(|u| u.hp)
        .max()
        .unwrap();
    assert!(satchel.damage * u32::from(p("demolition", "structure")) / 100 > most);
}

#[test]
fn every_shipped_role_has_an_armor_type_and_every_weapon_a_damage_type_in_the_table() {
    let defs = shipped();
    let c = defs.classes();
    for role in defs.sim_units() {
        assert!(
            c.armor_types.iter().any(|a| a.kind == role.armor_type),
            "role {}",
            role.kind
        );
    }
    for w in defs.sim_weapons() {
        assert!(
            c.damage_types.iter().any(|d| d.kind == w.damage_type),
            "weapon {}",
            w.kind
        );
    }
}

#[test]
fn the_shipped_splash_weapons_are_the_artillery_and_have_a_plateau() {
    let defs = shipped();
    let splash: Vec<&str> = defs
        .weapons()
        .iter()
        .filter(|w| !w.splash_radius.is_zero())
        .map(|w| w.id.as_str())
        .collect();
    assert_eq!(splash, ["howitzer", "battery_shell"]);
    for id in splash {
        let w = defs.sim_weapon(defs.weapon(id).unwrap().kind).unwrap();
        assert_eq!(w.splash_radius, Fx::from_ratio(3, 2), "{id}");
        assert_eq!(w.splash_inner, Fx::HALF, "{id}");
        assert!(w.friendly_fire, "{id}: classic: splash hurts everyone");
    }
}

// ---- a file without a table ---------------------------------------------------------------------------

/// The classes file with the whole `armor_table` taken out.
fn without_the_table(text: &str) -> String {
    let start = text.find("    armor_table: [").expect("the shipped table");
    let end = text[start..].find("\n    ],").unwrap() + start + "\n    ],\n".len();
    format!("{}{}", &text[..start], &text[end..])
}

#[test]
fn a_file_without_a_table_loads_and_has_none() {
    let defs = load(without_the_table, str::to_string).unwrap();
    assert!(!defs.classes().has_armor_table());
    assert_eq!(defs.classes().armor_percent(0, 1), None);
}

#[test]
fn the_frozen_copies_of_the_scenarios_load_without_a_table_and_with_the_splash_defaults() {
    let root = repo().join("assets");
    for dir in [
        "orders_defs",
        "pathing_defs",
        "projectiles_defs",
        "terrain_costs_defs",
        "turning_defs",
        "units_spawn_defs",
        "weapons_defs",
    ] {
        let defs = UnitDefs::load(&root.join("scenarios").join(dir), &root).unwrap();
        assert!(!defs.classes().has_armor_table(), "{dir}");
        for w in defs.sim_weapons() {
            assert_eq!(w.splash_inner, Fx::ZERO, "{dir}, weapon {}", w.kind);
            assert!(w.friendly_fire, "{dir}, weapon {}", w.kind);
        }
    }
}

// ---- a table that is wrong ----------------------------------------------------------------------------

#[test]
fn a_row_for_an_unknown_damage_type_is_refused() {
    refused(
        change("(damage: \"bullet\"", "(damage: \"bulet\""),
        "unknown damage type \"bulet\"",
        "\"bulet\"",
        0,
    );
}

#[test]
fn a_damage_type_with_two_rows_is_refused_at_the_second() {
    refused(
        change("(damage: \"precision\"", "(damage: \"bullet\""),
        "damage type \"bullet\" has two rows",
        "damage: \"bullet\"",
        1,
    );
}

#[test]
fn a_damage_type_without_a_row_is_refused() {
    // The row of demolition is renamed away: the table is no longer complete (and a new damage type
    // needs its row the same way).
    let e = load(
        |t| {
            let start = t.find("        (damage: \"demolition\"").unwrap();
            let end = t[start..].find("]),\n").unwrap() + start + "]),\n".len();
            format!("{}{}", &t[..start], &t[end..])
        },
        str::to_string,
    )
    .unwrap_err();
    assert_eq!(e.file, "classes.ron");
    assert!(
        e.message
            .contains("damage type \"demolition\" has no row (the table has to be complete)"),
        "{e}"
    );
}

#[test]
fn an_armor_type_without_a_percentage_in_a_row_is_refused_at_the_row() {
    refused(
        change(
            "(\"none\", 0), (\"infantry\", 100), (\"light\", 40), (\"heavy\", 10), (\"structure\", 20),",
            "(\"none\", 0), (\"infantry\", 100), (\"light\", 40), (\"structure\", 20),",
        ),
        "row \"bullet\": armor type \"heavy\" has no percentage",
        "damage: \"bullet\"",
        0,
    );
}

#[test]
fn an_unknown_armor_type_in_a_row_is_refused_at_its_name() {
    refused(
        change("(\"heavy\", 10)", "(\"heavyy\", 10)"),
        "unknown armor type \"heavyy\"",
        "\"heavyy\"",
        0,
    );
}

#[test]
fn an_armor_type_listed_twice_in_a_row_is_refused() {
    refused(
        change("(\"heavy\", 10)", "(\"light\", 10)"),
        "row \"bullet\": armor type \"light\" is listed twice",
        "(\"light\", 10)",
        0,
    );
}

#[test]
fn a_negative_percentage_is_refused() {
    refused(
        change("(\"heavy\", 10)", "(\"heavy\", -10)"),
        "\"heavy\" is -10 percent, which must be 0 to 400",
        "(\"heavy\", -10)",
        0,
    );
}

#[test]
fn a_percentage_above_the_bound_is_refused_and_the_bound_itself_is_fine() {
    refused(
        change("(\"heavy\", 10)", "(\"heavy\", 401)"),
        "\"heavy\" is 401 percent, which must be 0 to 400",
        "(\"heavy\", 401)",
        0,
    );
    // A number that does not fit the type at all.
    refused(
        change("(\"heavy\", 10)", "(\"heavy\", 70000)"),
        "70000 percent",
        "(\"heavy\", 70000)",
        0,
    );
    let defs = load(
        change("(\"heavy\", 10)", "(\"heavy\", 400)"),
        str::to_string,
    )
    .unwrap();
    let c = defs.classes();
    assert_eq!(
        c.armor_percent(
            c.damage_kind("bullet").unwrap(),
            c.armor_kind("heavy").unwrap()
        ),
        Some(400)
    );
    assert_eq!(MAX_ARMOR_PERCENT, 400);
}

#[test]
fn a_percentage_with_a_decimal_point_is_refused_and_never_becomes_a_float() {
    let e = load(
        change("(\"heavy\", 10)", "(\"heavy\", 10.5)"),
        str::to_string,
    )
    .unwrap_err();
    assert_eq!(e.file, "classes.ron");
    assert!(e.line.is_some(), "{e}");
}

#[test]
fn an_unknown_field_in_a_row_is_a_syntax_error_with_its_line() {
    let e = load(change("percent: [", "percnt: ["), str::to_string).unwrap_err();
    assert_eq!(e.file, "classes.ron");
    assert!(e.line.is_some() && e.column.is_some(), "{e}");
}

#[test]
fn the_order_of_the_rows_and_of_the_cells_does_not_matter() {
    // The last row moved to the front: the same table.
    let a = shipped();
    let b = load(
        |t| {
            let start = t.find("        (damage: \"demolition\"").unwrap();
            let end = t[start..].find("]),\n").unwrap() + start + "]),\n".len();
            let row = t[start..end].to_string();
            let mut out = format!("{}{}", &t[..start], &t[end..]);
            let first = out.find("        (damage: \"bullet\"").unwrap();
            out.insert_str(first, &row);
            out
        },
        str::to_string,
    )
    .unwrap();
    assert_eq!(a.classes().armor_percents, b.classes().armor_percents);
}

// ---- the splash fields of a weapon ----------------------------------------------------------------------

/// The weapons file with `from` replaced by `to` on the line of the weapon `id`.
fn weapon_edit(id: &'static str, from: &'static str, to: &'static str) -> impl Fn(&str) -> String {
    move |text: &str| {
        let marker = format!("id: \"{id}\"");
        let mut out = String::new();
        let mut done = false;
        for line in text.split_inclusive('\n') {
            if !done && line.contains(&marker) {
                assert!(line.contains(from), "test setup: {from:?} not in {line:?}");
                out.push_str(&line.replacen(from, to, 1));
                done = true;
            } else {
                out.push_str(line);
            }
        }
        assert!(done, "test setup: weapon {id} not found");
        out
    }
}

fn weapon_refused(id: &'static str, from: &'static str, to: &'static str, message: &str) {
    let e = load(str::to_string, weapon_edit(id, from, to)).unwrap_err();
    assert_eq!(e.file, "weapons.ron", "{e}");
    assert!(e.message.contains(message), "want {message:?} in: {e}");
    let edited = weapon_edit(id, from, to)(&lf(EMBEDDED_WEAPONS));
    assert_eq!(
        e.line,
        Some(line_of(&edited, &format!("id: \"{id}\""), 0)),
        "the error names the record of {id}: {e}"
    );
}

#[test]
fn the_full_damage_radius_lies_inside_the_splash_radius() {
    weapon_refused(
        "howitzer",
        "splash_inner: 0.5,",
        "splash_inner: 1.5,",
        "splash_inner 1.5 must be below the splash_radius 1.5",
    );
    weapon_refused(
        "howitzer",
        "splash_inner: 0.5,",
        "splash_inner: 2,",
        "must be below the splash_radius",
    );
    weapon_refused(
        "howitzer",
        "splash_inner: 0.5,",
        "splash_inner: -0.5,",
        "splash_inner -0.5 must be 0 or more",
    );
    let defs = load(
        str::to_string,
        weapon_edit("howitzer", "splash_inner: 0.5,", "splash_inner: 1.499,"),
    )
    .unwrap();
    let w = defs
        .sim_weapon(defs.weapon("howitzer").unwrap().kind)
        .unwrap();
    assert_eq!(w.splash_inner, Fx::from_ratio(1499, 1000));
}

#[test]
fn a_splash_field_on_a_weapon_without_a_splash_is_refused() {
    weapon_refused(
        "rifle",
        "projectile: Instant)",
        "projectile: Instant, splash_inner: 1)",
        "splash_inner 1 is for weapons with a splash_radius",
    );
    weapon_refused(
        "rifle",
        "projectile: Instant)",
        "projectile: Instant, friendly_fire: false)",
        "friendly_fire: false is for weapons with a splash_radius",
    );
    // Saying the default out loud is fine.
    load(
        str::to_string,
        weapon_edit(
            "rifle",
            "projectile: Instant)",
            "projectile: Instant, friendly_fire: true, splash_inner: 0)",
        ),
    )
    .unwrap();
}

#[test]
fn a_weapon_may_spare_its_own_side() {
    let defs = load(
        str::to_string,
        weapon_edit(
            "howitzer",
            "splash_inner: 0.5,",
            "splash_inner: 0.5, friendly_fire: false,",
        ),
    )
    .unwrap();
    let w = defs
        .sim_weapon(defs.weapon("howitzer").unwrap().kind)
        .unwrap();
    assert!(!w.friendly_fire);
    let other = defs
        .sim_weapon(defs.weapon("battery_shell").unwrap().kind)
        .unwrap();
    assert!(other.friendly_fire, "only the one that says so");
}
