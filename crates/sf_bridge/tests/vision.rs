//! The vision rules through the bridge (WP8.1): the `sight` of each role arrives as thousandths of a
//! cell for its own kind, the legacy sight from `vision.ron`, and a world takes the shipped rules.

use sf_bridge::{sight_milli, unit_table, vision_rules};
use sf_data::units::UnitDefs;
use sf_data::vision::VisionSettings;
use sf_math::Fx;
use sf_sim::{Kind, World, WorldConfig};

#[test]
fn a_sight_in_cells_becomes_thousandths_exactly() {
    for milli in [
        0i64, 1, 100, 500, 699, 700, 1000, 4500, 5500, 12_345, 64_000,
    ] {
        // How the data crate makes the number: a decimal with three places, divided into a Fx.
        let fx = Fx::from_ratio(milli, 1000);
        assert_eq!(i64::from(sight_milli(fx)), milli, "{milli}");
    }
    // Above the bound it is cut to 64 cells, below zero to 0.
    assert_eq!(sight_milli(Fx::from_int(100)), 64_000);
    assert_eq!(sight_milli(Fx::from_int(-3)), 0);
}

#[test]
fn each_role_gets_the_sight_of_its_own_entry() {
    let defs = UnitDefs::embedded();
    let settings = VisionSettings {
        legacy_sight_cells: 7,
    };
    let rules = vision_rules(&settings, &defs);
    assert_eq!(rules.legacy_sight_milli, 7000);
    assert_eq!(rules.sights.len(), defs.sim_units().len());
    for role in defs.sim_units() {
        let got = rules
            .sights
            .iter()
            .find(|&&(k, _)| k == Kind(role.kind))
            .unwrap()
            .1;
        assert_eq!(got, sight_milli(role.sight), "{}", role.kind);
    }
    // The shipped data has more than one sight, so a swap between kinds shows.
    let mut distinct: Vec<u32> = rules.sights.iter().map(|&(_, s)| s).collect();
    distinct.sort_unstable();
    distinct.dedup();
    assert!(distinct.len() >= 4, "{distinct:?}");
    assert!(rules.is_valid());
}

#[test]
fn a_world_takes_the_shipped_vision_rules() {
    let defs = UnitDefs::embedded();
    let world = World::new(WorldConfig::new(1).with_map_size(32, 32))
        .unwrap()
        .with_units(unit_table(&defs).unwrap())
        .unwrap()
        .with_vision(vision_rules(&VisionSettings::embedded(), &defs))
        .unwrap();
    assert!(world.vision_rules().is_some());
}
