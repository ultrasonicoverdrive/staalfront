//! WP5.1: the weapon definitions (`assets/config/weapon_defs.ron`).
//!
//! * every field of the shipped set, and the conversion to the simulation's units;
//! * a bad file is refused with the file and the line of the weapon, for each rule;
//! * the permanent numbers and the tombstones, ids included;
//! * a file written before WP5.1 (the frozen copies of the scenarios) still loads.
//!
//! The rules and the rounding are written in `docs/rules/weapons.md`.

use std::path::{Path, PathBuf};

use sf_data::units::{
    DefError, DefFile, DefSources, EMBEDDED_CLASSES, EMBEDDED_UNITS, EMBEDDED_WEAPONS, Projectile,
    UnitDefs, WeaponMount,
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
    weapons: impl Fn(&str) -> String,
    units: impl Fn(&str) -> String,
) -> Result<UnitDefs, DefError> {
    let (c, w, u) = (
        lf(EMBEDDED_CLASSES),
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

/// The weapons file with `from` replaced by `to` inside the record of weapon `id` only.
fn edit(id: &str, from: &str, to: &str) -> impl Fn(&str) -> String {
    let (id, from, to) = (id.to_string(), from.to_string(), to.to_string());
    move |text: &str| {
        let marker = format!("id: \"{id}\"");
        let mut out = String::new();
        let mut done = false;
        for line in text.split_inclusive('\n') {
            if !done && line.contains(&marker) {
                assert!(line.contains(&from), "test setup: {from:?} not in {line:?}");
                out.push_str(&line.replacen(&from, &to, 1));
                done = true;
            } else {
                out.push_str(line);
            }
        }
        assert!(done, "test setup: weapon {id} not found");
        out
    }
}

/// The 1-based line of the record of weapon `id` in the edited file.
fn record_line(text: &str, id: &str) -> usize {
    let marker = format!("id: \"{id}\"");
    1 + text
        .lines()
        .take_while(|l| !l.contains(&marker) || l.trim_start().starts_with("//"))
        .count()
}

/// Loads with one record of the weapons file edited and expects it to be refused, at that record.
fn refused(id: &str, from: &str, to: &str, message: &str) -> DefError {
    let e = load(edit(id, from, to), str::to_string).unwrap_err();
    assert_eq!(e.file, "weapons.ron", "{e}");
    assert!(e.message.contains(message), "want {message:?} in: {e}");
    let edited = edit(id, from, to)(&lf(EMBEDDED_WEAPONS));
    assert_eq!(
        e.line,
        Some(record_line(&edited, id)),
        "the error names the record of {id}: {e}"
    );
    e
}

// ---- the shipped set ---------------------------------------------------------------------------

#[test]
fn every_shipped_projectile_that_flies_has_a_speed_and_an_instant_one_has_none() {
    // The loader does not insist (the frozen copies of the scenarios predate the field), the
    // shipped set does.
    let defs = shipped();
    for w in defs.weapons() {
        match w.projectile {
            Projectile::Instant => assert!(w.projectile_speed.is_zero(), "{}", w.id),
            _ => assert!(!w.projectile_speed.is_zero(), "{} has no speed", w.id),
        }
        match w.projectile {
            Projectile::Arc | Projectile::Falling => {
                assert!(!w.flight_height.is_zero(), "{} has no height", w.id)
            }
            _ => assert!(w.flight_height.is_zero(), "{}", w.id),
        }
        assert_eq!(
            w.guide_turn != 0,
            w.projectile == Projectile::Guided,
            "{}",
            w.id
        );
    }
}

#[test]
fn the_shipped_weapons_in_the_sims_units() {
    let defs = shipped();
    let sim = |id: &str| defs.sim_weapon(defs.weapon(id).unwrap().kind).unwrap();

    let rifle = sim("rifle");
    assert_eq!(rifle.range, Fx::from_int(4));
    assert_eq!(rifle.reload_ticks, 20); // 1 s at 20 Hz
    assert_eq!((rifle.burst, rifle.burst_delay_ticks), (1, 0));
    assert_eq!(rifle.projectile, Projectile::Instant);
    assert_eq!(rifle.projectile_speed, Fx::ZERO);
    assert_eq!(rifle.projectile_speed_per_tick, Fx::ZERO);
    assert_eq!(rifle.mount, WeaponMount::Hull);
    assert_eq!(rifle.fire_arc, 8);

    // The cannon of the light tank is on the turret and its shell crosses 30 cells a second,
    // which is a step of 3/2 cells a tick.
    let cannon = sim("cannon");
    assert_eq!(cannon.mount, WeaponMount::Turret);
    assert_eq!(cannon.projectile_speed, Fx::from_int(30));
    assert_eq!(cannon.projectile_speed_per_tick, Fx::from_ratio(3, 2));

    // The twin cannon: two shells a quarter of a second apart in a reload of two seconds.
    let twin = sim("twin_cannon");
    assert_eq!(
        (twin.burst, twin.burst_delay_ticks, twin.reload_ticks),
        (2, 5, 40)
    );
    assert_eq!(twin.damage, 30);

    // The machine gun: four shots a tenth of a second apart, all round.
    let gun = sim("automated_gun");
    assert_eq!(
        (gun.burst, gun.burst_delay_ticks, gun.reload_ticks),
        (4, 2, 20)
    );
    assert_eq!(gun.fire_arc, 128);

    // The howitzer: an arc, a minimum range, a splash and a height.
    let howitzer = sim("howitzer");
    assert_eq!(howitzer.projectile, Projectile::Arc);
    assert_eq!(howitzer.min_range, Fx::from_int(3));
    assert_eq!(howitzer.flight_height, Fx::from_int(3));
    assert_eq!(howitzer.splash_radius, Fx::from_ratio(3, 2));

    // The rocket steers.
    let rocket = sim("rocket");
    assert_eq!(rocket.projectile, Projectile::Guided);
    assert_eq!(rocket.guide_turn_per_tick, 4);
}

#[test]
fn a_projectile_speed_per_tick_is_rounded_once_from_the_exact_decimal() {
    // 1.1 cells a second: dividing the fixed-point speed by 20 afterwards would round twice and
    // give 3605 raw units; rounding once gives 3604 (see `Decimal::to_fx_per_tick`).
    let defs = load(
        edit("cannon", "projectile_speed: 30", "projectile_speed: 1.1"),
        str::to_string,
    )
    .unwrap();
    let cannon = defs
        .sim_weapon(defs.weapon("cannon").unwrap().kind)
        .unwrap();
    assert_eq!(cannon.projectile_speed_per_tick, Fx::from_raw(3604));
    assert_eq!(
        cannon.projectile_speed_per_tick,
        Fx::from_ratio(1100, 20_000)
    );
    // Every shipped speed is the exact rounding.
    let defs = shipped();
    for w in defs.weapons() {
        assert_eq!(
            defs.sim_weapon(w.kind).unwrap().projectile_speed_per_tick,
            Fx::from_ratio(w.projectile_speed.milli(), 20_000),
            "{}",
            w.id
        );
    }
}

#[test]
fn every_weapon_of_every_kind_is_data_only() {
    let extra = "        (kind: 15, id: \"a_instant\", range: 6, reload: 1, damage: 5, damage_type: \"bullet\", projectile: Instant),\n\
        (kind: 16, id: \"b_straight\", range: 6, reload: 1, damage: 5, damage_type: \"bullet\", projectile: Straight, projectile_speed: 20),\n\
        (kind: 17, id: \"c_arc\", range: 6, reload: 1, damage: 5, damage_type: \"shell\", projectile: Arc, projectile_speed: 8, flight_height: 4),\n\
        (kind: 18, id: \"d_guided\", range: 6, reload: 1, damage: 5, damage_type: \"rocket\", projectile: Guided, projectile_speed: 8, guide_turn: 6, burst: 3, burst_delay: 0.2),\n\
        (kind: 19, id: \"e_falling\", range: 6, reload: 1, damage: 5, damage_type: \"shell\", projectile: Falling, projectile_speed: 12, flight_height: 8, mount: Hull, fire_arc: 0),\n";
    let defs = load(
        |t| {
            t.replacen(
                "    ],\n    // Tombstones",
                &format!("{extra}    ],\n    // Tombstones"),
                1,
            )
        },
        str::to_string,
    )
    .expect("five more weapons load");
    assert_eq!(defs.weapons().len(), shipped().weapons().len() + 5);
    let guided = defs.sim_weapon(18).unwrap();
    assert_eq!((guided.burst, guided.burst_delay_ticks), (3, 4));
    assert_eq!(guided.guide_turn_per_tick, 6);
    assert_eq!(defs.sim_weapon(19).unwrap().fire_arc, 0);
    assert_eq!(defs.sim_weapon(15).unwrap().projectile, Projectile::Instant);
}

#[test]
fn a_file_from_before_wp5_1_still_loads_with_the_defaults() {
    // The scenarios pin their checksums on frozen copies of the definitions written before the
    // fields existed; every new field has a default that changes nothing.
    let root = repo().join("assets");
    let defs = UnitDefs::load(&root.join("scenarios/orders_defs"), &root).unwrap();
    assert!(!defs.weapons().is_empty());
    for w in defs.sim_weapons() {
        assert_eq!((w.burst, w.burst_delay_ticks), (1, 0), "{}", w.kind);
        assert_eq!(w.projectile_speed_per_tick, Fx::ZERO);
        assert_eq!((w.flight_height, w.guide_turn_per_tick), (Fx::ZERO, 0));
        assert_eq!((w.mount, w.fire_arc), (WeaponMount::Hull, 8), "{}", w.kind);
    }
}

// ---- refusals with file and line ---------------------------------------------------------------

#[test]
fn the_reload_must_be_a_positive_whole_number_of_ticks() {
    refused("rifle", "reload: 1,", "reload: 0,", "at least one tick");
    refused("rifle", "reload: 1,", "reload: -1,", "must be above 0");
    refused(
        "rifle",
        "reload: 1,",
        "reload: 0.07,",
        "whole number of ticks",
    );
    refused("rifle", "reload: 1,", "reload: 601,", "more than 600");
    // The smallest and the largest are fine.
    load(edit("rifle", "reload: 1,", "reload: 0.05,"), str::to_string).unwrap();
    load(edit("rifle", "reload: 1,", "reload: 600,"), str::to_string).unwrap();
}

#[test]
fn the_range_must_be_in_bounds() {
    refused("rifle", "range: 4,", "range: 0,", "range 0 must be above 0");
    refused("rifle", "range: 4,", "range: -2,", "must be above 0");
    refused("rifle", "range: 4,", "range: 65,", "at most 64 cells");
    load(edit("rifle", "range: 4,", "range: 64,"), str::to_string).unwrap();
    // The minimum range is below the range, and not negative.
    refused(
        "howitzer",
        "min_range: 3,",
        "min_range: 10,",
        "below the range",
    );
    refused(
        "howitzer",
        "min_range: 3,",
        "min_range: 12,",
        "below the range",
    );
    refused("howitzer", "min_range: 3,", "min_range: -1,", "0 or more");
    refused(
        "howitzer",
        "splash_radius: 1.5,",
        "splash_radius: -1,",
        "splash_radius",
    );
    refused(
        "howitzer",
        "splash_radius: 1.5,",
        "splash_radius: 65,",
        "splash_radius",
    );
}

#[test]
fn the_damage_must_be_in_bounds() {
    refused("rifle", "damage: 6,", "damage: 0,", "damage must be 1 to");
    refused(
        "rifle",
        "damage: 6,",
        "damage: 1000001,",
        "damage must be 1 to",
    );
    load(
        edit("rifle", "damage: 6,", "damage: 1000000,"),
        str::to_string,
    )
    .unwrap();
}

#[test]
fn the_damage_type_must_exist() {
    refused(
        "rifle",
        "damage_type: \"bullet\"",
        "damage_type: \"laser\"",
        "unknown damage type \"laser\"",
    );
}

#[test]
fn ids_must_be_snake_case_names_and_unique() {
    for bad in ["Rifle", "", "rifle-2", "2_rifle_", "ri fle"] {
        let to = format!("id: \"{bad}\"");
        let e = load(edit("rifle", "id: \"rifle\"", &to), str::to_string).unwrap_err();
        assert_eq!(e.file, "weapons.ron", "{e}");
        assert!(e.message.contains("snake_case"), "{bad:?}: {e}");
        assert!(e.line.is_some(), "{bad:?}: {e}");
    }
    // The second weapon with an id is the one named.
    let e = load(
        edit("rocket", "id: \"rocket\"", "id: \"rifle\""),
        str::to_string,
    )
    .unwrap_err();
    assert!(e.message.contains("\"rifle\" appears twice"), "{e}");
    let edited = edit("rocket", "id: \"rocket\"", "id: \"rifle\"")(&lf(EMBEDDED_WEAPONS));
    let second = edited
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains("id: \"rifle\"") && !l.trim_start().starts_with("//"))
        .nth(1)
        .unwrap()
        .0
        + 1;
    assert_eq!(e.line, Some(second), "{e}");
}

#[test]
fn the_list_of_weapons_cannot_be_empty() {
    let e = load(
        |t| {
            let start = t.find("    weapons: [").unwrap();
            let end = t.find("    // Tombstones").unwrap();
            format!("{}    weapons: [\n    ],\n{}", &t[..start], &t[end..])
        },
        str::to_string,
    )
    .unwrap_err();
    assert!(e.message.contains("empty"), "{e}");
}

#[test]
fn a_syntax_error_an_unknown_field_and_an_unknown_projectile_name_the_line() {
    let e = load(
        edit("rifle", "projectile: Instant", "projectile: Lobbed"),
        str::to_string,
    )
    .unwrap_err();
    assert_eq!(e.file, "weapons.ron");
    assert!(e.message.contains("Lobbed"), "{e}");
    assert!(e.line.is_some() && e.column.is_some(), "{e}");
    let e = load(
        edit("rifle", "damage: 6,", "damage: 6, colour: 3,"),
        str::to_string,
    )
    .unwrap_err();
    assert!(e.message.contains("colour"), "{e}");
    assert!(e.line.is_some(), "{e}");
}

#[test]
fn the_new_numbers_must_be_in_bounds() {
    // The burst.
    refused(
        "twin_cannon",
        "burst: 2,",
        "burst: 0,",
        "burst 0 must be 1 to 16",
    );
    refused(
        "twin_cannon",
        "burst: 2,",
        "burst: 17,",
        "burst 17 must be 1 to 16",
    );
    refused(
        "twin_cannon",
        "burst_delay: 0.25",
        "burst_delay: 0.07",
        "burst_delay 0.07 s is not a whole number of ticks",
    );
    refused(
        "twin_cannon",
        "burst_delay: 0.25",
        "burst_delay: 0",
        "needs a burst_delay of at least one tick",
    );
    refused(
        "rifle",
        "projectile: Instant",
        "projectile: Instant, burst_delay: 0.25",
        "no use with a burst of 1 shot",
    );
    // A burst has to fit in the reload, with a rest after the last shot.
    refused(
        "twin_cannon",
        "burst_delay: 0.25",
        "burst_delay: 2",
        "not longer than the burst",
    );
    refused(
        "twin_cannon",
        "burst_delay: 0.25",
        "burst_delay: 3",
        "not longer than the burst",
    );
    load(
        edit("twin_cannon", "burst_delay: 0.25", "burst_delay: 1.95"),
        str::to_string,
    )
    .unwrap();
    // The firing arc.
    refused(
        "rifle",
        "projectile: Instant",
        "projectile: Instant, fire_arc: 129",
        "fire_arc 129",
    );
    load(
        edit(
            "rifle",
            "projectile: Instant",
            "projectile: Instant, fire_arc: 128",
        ),
        str::to_string,
    )
    .unwrap();
    // The projectile.
    refused(
        "cannon",
        "projectile_speed: 30",
        "projectile_speed: 201",
        "projectile_speed 201 must be 0 to 200",
    );
    refused(
        "cannon",
        "projectile_speed: 30",
        "projectile_speed: -1",
        "projectile_speed",
    );
    load(
        edit("cannon", "projectile_speed: 30", "projectile_speed: 200"),
        str::to_string,
    )
    .unwrap();
    refused(
        "howitzer",
        "flight_height: 3,",
        "flight_height: 33,",
        "flight_height 33 must be 0 to 32",
    );
    refused(
        "rocket",
        "guide_turn: 4",
        "guide_turn: 65",
        "guide_turn 65 must be 0 to 64",
    );
}

#[test]
fn a_parameter_belongs_to_the_projectiles_that_use_it() {
    refused(
        "rifle",
        "projectile: Instant",
        "projectile: Instant, projectile_speed: 10",
        "Instant weapon has no projectile",
    );
    refused(
        "cannon",
        "projectile: Straight",
        "projectile: Straight, flight_height: 2",
        "flight_height is for Arc and Falling projectiles, not Straight",
    );
    refused(
        "cannon",
        "projectile: Straight",
        "projectile: Straight, guide_turn: 3",
        "guide_turn is for Guided projectiles, not Straight",
    );
    refused(
        "howitzer",
        "projectile: Arc",
        "projectile: Arc, guide_turn: 3",
        "guide_turn is for Guided projectiles, not Arc",
    );
}

#[test]
fn a_turret_mount_needs_a_unit_with_a_turret() {
    // The rifle is carried by infantry: no turret.
    let e = load(
        edit(
            "rifle",
            "projectile: Instant",
            "projectile: Instant, mount: Turret",
        ),
        str::to_string,
    )
    .unwrap_err();
    assert_eq!(e.file, "units.ron");
    assert!(
        e.message
            .contains("weapon \"rifle\" is mounted on a turret but the unit has no turret_rate"),
        "{e}"
    );
    assert!(e.line.is_some(), "{e}");
    // The cannon is, and the light tank has a turret: that is the shipped file.
    let defs = shipped();
    let tank = defs.sim_unit(defs.kind_of("light_tank").unwrap()).unwrap();
    assert!(tank.turret_per_tick > 0);
    assert_eq!(
        defs.sim_weapon(tank.weapons[0]).unwrap().mount,
        WeaponMount::Turret
    );
    // A tank without a turret may not keep its turret weapon.
    let e = load(str::to_string, |t| {
        t.replacen("turn_rate: 10, turret_rate: 12,", "turn_rate: 10,", 1)
    })
    .unwrap_err();
    assert!(e.message.contains("no turret_rate"), "{e}");
}

#[test]
fn a_unit_may_not_name_an_unknown_or_a_repeated_weapon() {
    let units = |from: &'static str, to: &'static str| {
        move |t: &str| {
            assert!(t.contains(from), "test setup");
            t.replacen(from, to, 1)
        }
    };
    let e = load(
        str::to_string,
        units("weapons: [\"cannon\"]", "weapons: [\"cannon2\"]"),
    )
    .unwrap_err();
    assert_eq!(e.file, "units.ron");
    assert!(e.message.contains("unknown weapon \"cannon2\""), "{e}");
    assert!(e.line.is_some(), "{e}");
    let e = load(
        str::to_string,
        units("weapons: [\"cannon\"]", "weapons: [\"cannon\", \"cannon\"]"),
    )
    .unwrap_err();
    assert!(e.message.contains("listed twice"), "{e}");
    let e = load(
        str::to_string,
        units(
            "weapons: [\"cannon\"]",
            "weapons: [\"cannon\", \"rifle\", \"pistol\", \"carbine\", \"rocket\"]",
        ),
    )
    .unwrap_err();
    assert!(e.message.contains("more than 4 weapons"), "{e}");
    // Four are fine, in the order written (slot order).
    let defs = load(
        str::to_string,
        units(
            "weapons: [\"cannon\"]",
            "weapons: [\"cannon\", \"rifle\", \"pistol\", \"carbine\"]",
        ),
    )
    .unwrap();
    let tank = defs.sim_unit(defs.kind_of("light_tank").unwrap()).unwrap();
    assert_eq!(tank.weapons, [6, 0, 3, 4]);
}

// ---- permanent numbers and tombstones ----------------------------------------------------------

#[test]
fn a_weapon_keeps_its_permanent_number_wherever_it_stands() {
    let defs = shipped();
    for (id, kind) in [
        ("rifle", 0),
        ("rocket", 1),
        ("cannon", 6),
        ("twin_cannon", 7),
        ("howitzer", 8),
        ("automated_gun", 13),
        ("battery_shell", 14),
    ] {
        assert_eq!(defs.weapon(id).unwrap().kind, kind, "{id}");
        assert_eq!(defs.weapon_by_kind(kind).unwrap().id, id);
    }
    // Reversing the records changes nothing.
    let reversed = load(
        |t| {
            let lines: Vec<&str> = t.lines().collect();
            let (first, last) = (
                lines
                    .iter()
                    .position(|l| l.contains("(kind: 0, id: \"rifle\""))
                    .unwrap(),
                lines
                    .iter()
                    .position(|l| l.contains("(kind: 14, id: \"battery_shell\""))
                    .unwrap(),
            );
            let mut out: Vec<&str> = lines[..first].to_vec();
            // The comment lines between the records stay out of the way: only the records move.
            let mut records: Vec<&str> = lines[first..=last]
                .iter()
                .copied()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect();
            records.reverse();
            out.extend(records);
            out.extend(&lines[last + 1..]);
            out.join("\n")
        },
        str::to_string,
    )
    .unwrap();
    assert_eq!(reversed.sim_weapons(), defs.sim_weapons());
    assert_eq!(reversed.weapon("cannon").unwrap().kind, 6);
}

#[test]
fn a_retired_number_and_a_retired_id_are_never_reused() {
    let retire = |tombstones: &'static str| {
        move |t: &str| {
            t.replacen(
                "retired_weapons: [],",
                &format!("retired_weapons: [{tombstones}],"),
                1,
            )
        }
    };
    // A tombstone for a number nobody uses is fine and is reported.
    let defs = load(retire("(kind: 90, name: \"old_gun\")"), str::to_string).unwrap();
    assert_eq!(defs.retired_weapons().len(), 1);
    assert!(defs.weapon_by_kind(90).is_none());
    // A live weapon on the number...
    let e = load(retire("(kind: 6, name: \"old_cannon\")"), str::to_string).unwrap_err();
    assert!(
        e.message.contains("kind 6 was retired by \"old_cannon\""),
        "{e}"
    );
    assert_eq!(e.file, "weapons.ron");
    assert!(e.line.is_some(), "{e}");
    // ... and a new weapon that takes the id of a removed one under a fresh number.
    let e = load(retire("(kind: 90, name: \"cannon\")"), str::to_string).unwrap_err();
    assert!(e.message.contains("\"cannon\""), "{e}");
    assert!(
        e.message
            .contains("retired with kind 90 and is never reused"),
        "{e}"
    );
    assert!(e.line.is_some(), "{e}");
    // The same number retired twice, and a number out of range.
    let e = load(
        retire("(kind: 90, name: \"a\"), (kind: 90, name: \"b\")"),
        str::to_string,
    )
    .unwrap_err();
    assert!(e.message.contains("already retired"), "{e}");
    let e = load(retire("(kind: 5000, name: \"a\")"), str::to_string).unwrap_err();
    assert!(e.message.contains("above the largest"), "{e}");
    // Two live weapons with one number.
    let e = load(edit("rocket", "kind: 1,", "kind: 0,"), str::to_string).unwrap_err();
    assert!(
        e.message.contains("kind 0 is already used by \"rifle\""),
        "{e}"
    );
    let e = load(edit("rocket", "kind: 1,", "kind: 4096,"), str::to_string).unwrap_err();
    assert!(
        e.message.contains("above the largest allowed (4095)"),
        "{e}"
    );
    // A weapon with no number at all.
    let e = load(edit("rocket", "kind: 1, ", ""), str::to_string).unwrap_err();
    assert!(e.message.contains("kind"), "{e}");
}
