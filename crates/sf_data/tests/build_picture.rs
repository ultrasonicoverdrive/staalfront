//! WP11.1: the `build_picture` field of `unit_defs.ron`, the picture of a role in the sidebar's build
//! menu, separate from `icon` (the name of the strategic map icon since WP3.6).
//!
//! * every role with a price in the shipped data has a picture; the picture is its own, not the icon;
//! * the orders are unique within a category, the labels fit;
//! * a bad picture is refused with the record named (no price, bad label, repeated order);
//! * a role without a picture loads (the frozen copies of the scenarios do not have the field).

use std::path::{Path, PathBuf};

use sf_data::units::{
    Category, DefFile, DefSources, EMBEDDED_CLASSES, EMBEDDED_UNITS, EMBEDDED_WEAPONS,
    MAX_PICTURE_LABEL, MAX_PICTURE_ORDER, UnitDefs,
};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn load_units(units: &str) -> Result<UnitDefs, sf_data::units::DefError> {
    let (c, w) = (lf(EMBEDDED_CLASSES), lf(EMBEDDED_WEAPONS));
    UnitDefs::from_sources(
        &DefSources {
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
        },
        Some(&repo().join("assets")),
    )
}

fn change(from: &str, to: &str) -> Result<UnitDefs, sf_data::units::DefError> {
    let text = lf(EMBEDDED_UNITS);
    assert!(text.contains(from), "test setup: {from:?} not found");
    load_units(&text.replacen(from, to, 1))
}

fn refused(from: &str, to: &str, message: &str) {
    let e = change(from, to).expect_err("the change must be refused");
    assert!(e.message.contains(message), "{e}");
    assert!(e.line.is_some(), "an error with a line: {e}");
}

const RIFLEMAN_PICTURE: &str =
    r#"build_picture: (color: (90, 170, 100), label: "RIFLE", order: 10),"#;

#[test]
fn every_shipped_role_with_a_price_has_a_picture_of_its_own() {
    let defs = UnitDefs::embedded();
    let mut priced = 0;
    for d in defs.units() {
        match (d.cost, &d.build_picture) {
            (Some(_), Some(p)) => {
                priced += 1;
                assert!(
                    (1..=MAX_PICTURE_LABEL).contains(&p.label.chars().count()),
                    "{}: label {:?}",
                    d.id,
                    p.label
                );
                assert!(p.order <= MAX_PICTURE_ORDER, "{}", d.id);
            }
            (Some(_), None) => panic!("{} has a price but no build_picture", d.id),
            (None, Some(_)) => panic!("{} has a picture but cannot be built", d.id),
            (None, None) => {}
        }
    }
    // 10 structures, 9 infantry, 8 vehicles and 1 vessel can be built.
    assert_eq!(priced, 28);
}

#[test]
fn the_picture_is_not_the_strategic_icon() {
    // The two fields are separate: the icon names the map icon, the picture is the tile of the menu.
    let defs = UnitDefs::embedded();
    let plant = defs.unit("power_plant").unwrap();
    assert_eq!(plant.icon.as_deref(), Some("power_plant"));
    let p = plant.build_picture.as_ref().unwrap();
    assert_eq!(
        (p.label.as_str(), p.order, p.color),
        ("PWR", 10, (230, 190, 60))
    );
    let refinery = defs
        .unit("refinery")
        .unwrap()
        .build_picture
        .as_ref()
        .unwrap();
    assert_eq!(
        (refinery.label.as_str(), refinery.order, refinery.color),
        ("REF", 20, (200, 140, 60))
    );
}

#[test]
fn orders_and_labels_are_distinct_within_a_category() {
    let defs = UnitDefs::embedded();
    let all: Vec<_> = defs
        .units()
        .iter()
        .filter_map(|d| d.build_picture.as_ref().map(|p| (d.category, p, &d.id)))
        .collect();
    for (i, (cat, p, id)) in all.iter().enumerate() {
        for (cat2, p2, id2) in &all[..i] {
            if cat == cat2 {
                assert_ne!(p.order, p2.order, "{id} and {id2} share an order");
            }
            // A label that is shared would make two tiles look alike.
            assert_ne!(p.label, p2.label, "{id} and {id2} share a label");
        }
    }
    // The same order in two categories is fine (each tab has its own list): 10 is first everywhere.
    for cat in [
        Category::Structure,
        Category::Infantry,
        Category::Vehicle,
        Category::Vessel,
    ] {
        assert!(
            all.iter().any(|(c, p, _)| *c == cat && p.order == 10),
            "{cat:?} has no first item at order 10"
        );
    }
}

#[test]
fn a_picture_on_a_role_that_cannot_be_built_is_refused() {
    // The construction centre has no price: it is the anchor, nobody buys it.
    refused(
        r#"icon: "construction_center","#,
        r#"icon: "construction_center", build_picture: (color: (1, 2, 3), label: "CC", order: 1),"#,
        "no cost",
    );
}

#[test]
fn a_bad_label_is_refused() {
    for label in ["", "TOOLONG1", " RIFLE", "RIFLE ", "RIFL\u{e9}"] {
        let to = format!(r#"build_picture: (color: (90, 170, 100), label: "{label}", order: 10),"#);
        refused(RIFLEMAN_PICTURE, &to, "label");
    }
}

#[test]
fn an_order_above_the_limit_or_repeated_in_a_category_is_refused() {
    refused(
        RIFLEMAN_PICTURE,
        r#"build_picture: (color: (90, 170, 100), label: "RIFLE", order: 1000),"#,
        "order 1000",
    );
    // Rocket trooper takes the order of the rifleman: two infantry at one place.
    refused(
        r#"build_picture: (color: (70, 150, 120), label: "ROCKT", order: 20),"#,
        r#"build_picture: (color: (70, 150, 120), label: "ROCKT", order: 10),"#,
        "also used by",
    );
}

#[test]
fn a_role_without_a_picture_loads_and_a_picture_can_be_added_by_data_alone() {
    // The frozen copies of the definitions have no build_picture: it is optional.
    let defs = change(RIFLEMAN_PICTURE, "").expect("a role without a picture loads");
    assert!(defs.unit("rifleman").unwrap().build_picture.is_none());
    // A role may take another place in its tab by data alone.
    let defs = change(
        r#"build_picture: (color: (210, 150, 50), label: "HARV", order: 10),"#,
        r#"build_picture: (color: (210, 150, 50), label: "HARV", order: 15),"#,
    )
    .expect("a free order is accepted");
    assert_eq!(
        defs.unit("harvester")
            .unwrap()
            .build_picture
            .as_ref()
            .unwrap()
            .order,
        15
    );
}

#[test]
fn an_unknown_picture_field_is_refused() {
    assert!(
        change(
            RIFLEMAN_PICTURE,
            r#"build_picture: (color: (90, 170, 100), label: "RIFLE", order: 10, glow: 1),"#
        )
        .is_err()
    );
}
