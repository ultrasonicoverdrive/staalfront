//! The build speed of the shipped data (WP6.11): `production.ron` says 3 / 2, the world built from the
//! shipped definitions holds the scaled tick counts for every unit and structure, and every price is
//! still paid exactly. At 1 / 1 the world holds the definitions' own tick counts.

use sf_bridge::{building_table, production_rules, production_table, unit_table};
use sf_data::production::{self, ProductionSettings};
use sf_data::units::UnitDefs;
use sf_sim::{
    BuildSpeed, Kind, MAX_BUILD_SPEED_RATIO, MAX_BUILD_SPEED_TERM, World, WorldConfig, paid_at,
    tick_cost,
};

fn world(defs: &UnitDefs, speed: Option<BuildSpeed>) -> World {
    let mut rules = production_rules(&ProductionSettings::embedded());
    if let Some(s) = speed {
        rules.build_speed = s;
    }
    World::new(WorldConfig::new(1))
        .unwrap()
        .with_units(unit_table(defs).unwrap())
        .unwrap()
        .with_buildings(building_table(defs).unwrap())
        .unwrap()
        .with_production(rules, production_table(defs).unwrap())
        .unwrap()
}

#[test]
fn the_shipped_factor_is_three_halves_and_the_limits_are_the_simulations() {
    let rules = production_rules(&ProductionSettings::embedded());
    assert_eq!(rules.build_speed, BuildSpeed { num: 3, den: 2 });
    assert_eq!(production::MAX_BUILD_SPEED_TERM, MAX_BUILD_SPEED_TERM);
    assert_eq!(production::MAX_BUILD_SPEED_RATIO, MAX_BUILD_SPEED_RATIO);
}

#[test]
fn a_few_shipped_builds_in_ticks() {
    let defs = UnitDefs::embedded();
    let w = world(&defs, None);
    let units = w.production_table().unwrap();
    let buildings = w.building_table();
    // Unit: rifleman 5 s = 100 ticks -> 67 (66.67 up); harvester 30 s = 600 -> 400 (exact).
    assert_eq!(units.get(Kind(12)).unwrap().build_ticks, 67);
    assert_eq!(units.get(Kind(11)).unwrap().build_ticks, 400);
    // Structure: power plant 15 s = 300 -> 200; barracks 20 s = 400 -> 267 (266.67 up); refinery
    // 40 s = 800 -> 534 (533.33 up); silo 12 s = 240 -> 160.
    assert_eq!(buildings.get(Kind(1)).unwrap().build_ticks, 200);
    assert_eq!(buildings.get(Kind(3)).unwrap().build_ticks, 267);
    assert_eq!(buildings.get(Kind(2)).unwrap().build_ticks, 534);
    assert_eq!(buildings.get(Kind(43)).unwrap().build_ticks, 160);
}

/// Every shipped unit and structure: the world's ticks are the definitions' ticks times 2 / 3 rounded
/// up, and the payments add up to the price (no credit lost or made by the rounding).
#[test]
fn every_shipped_price_is_paid_exactly_in_the_scaled_ticks() {
    let defs = UnitDefs::embedded();
    let w = world(&defs, None);
    let mut checked = (0, 0);
    for role in defs.sim_units() {
        let Some(cost) = role.cost else { continue };
        if role.build_ticks == 0 {
            continue;
        }
        let ticks = if let Some(b) = w.production_table().unwrap().get(Kind(role.kind)) {
            checked.0 += 1;
            b.build_ticks
        } else {
            let b = w.building_table().get(Kind(role.kind)).unwrap();
            checked.1 += 1;
            b.build_ticks
        };
        assert_eq!(
            ticks,
            (role.build_ticks * 2).div_ceil(3),
            "kind {}",
            role.kind
        );
        let by_tick: u64 = (0..ticks).map(|k| tick_cost(cost, ticks, k)).sum();
        assert_eq!(by_tick, u64::from(cost), "kind {} per tick", role.kind);
        let by_progress: u64 = (0..ticks)
            .map(|k| paid_at(cost, ticks, (k + 1) * 1000) - paid_at(cost, ticks, k * 1000))
            .sum();
        assert_eq!(
            by_progress,
            u64::from(cost),
            "kind {} per progress",
            role.kind
        );
    }
    // Both kinds were covered: more than a handful of units and of structures.
    assert!(checked.0 >= 10 && checked.1 >= 8, "{checked:?}");
}

#[test]
fn at_one_over_one_the_world_holds_the_definitions_own_ticks() {
    let defs = UnitDefs::embedded();
    let w = world(&defs, Some(BuildSpeed::FULL));
    for role in defs.sim_units().iter().filter(|r| r.cost.is_some()) {
        let ticks = w
            .production_table()
            .unwrap()
            .get(Kind(role.kind))
            .map(|b| b.build_ticks)
            .or_else(|| {
                w.building_table()
                    .get(Kind(role.kind))
                    .map(|b| b.build_ticks)
            });
        assert_eq!(ticks, Some(role.build_ticks), "kind {}", role.kind);
    }
    // And the digests are those of the bridge's own tables: nothing was rebuilt.
    assert_eq!(
        w.building_table().digest(),
        building_table(&defs).unwrap().digest()
    );
    assert_eq!(
        w.production_table().unwrap().digest(),
        production_table(&defs).unwrap().digest()
    );
}
