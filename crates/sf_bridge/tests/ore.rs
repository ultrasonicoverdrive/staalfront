//! The ore rules through the bridge (WP7.1): the data numbers arrive in the world unchanged.

use sf_bridge::ore_rules;
use sf_data::ore::{MAX_REGROW_PERIOD, OreSettings};
use sf_sim::{Cell, OreRules, Terrain, TerrainDef, World, WorldConfig};

#[test]
fn the_sim_numbers_equal_the_data_numbers() {
    let rules = ore_rules(&OreSettings::embedded());
    assert_eq!(
        rules,
        OreRules {
            start: 120,
            max: 160,
            regrow_amount: 1,
            regrow_period: 200,
        }
    );
    assert!(rules.is_valid());
}

#[test]
fn the_period_limit_is_the_sims() {
    assert_eq!(MAX_REGROW_PERIOD, OreRules::MAX_PERIOD);
}

/// The shipped terrain table's ore type is the harvestable one: a world made from the shipped
/// data starts with ore exactly on the cells of that type.
#[test]
fn shipped_ore_cells_start_full() {
    let types = sf_data::terrain::TerrainTypes::embedded();
    let defs: Vec<TerrainDef> =
        sf_bridge::terrain_defs(&types, &sf_data::movement::MovementCosts::embedded());
    let ore_id = types.id_of("ore").unwrap();
    let grass = types.id_of("grass").unwrap();
    let ids = [grass, ore_id, ore_id, grass];
    let terrain = Terrain::from_layer(4, 1, &ids, &defs).unwrap();
    let world = World::with_terrain(WorldConfig::new(1).with_map_size(4, 1), terrain)
        .unwrap()
        .with_ore(ore_rules(&OreSettings::embedded()))
        .unwrap();
    let amounts: Vec<u16> = (0..4).map(|x| world.ore_at(Cell::new(x, 0))).collect();
    assert_eq!(amounts, [0, 120, 120, 0]);
}
