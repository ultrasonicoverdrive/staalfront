//! The simulation world of a map: its terrain handed over (WP2.1).
//!
//! `sf_sim` does no I/O, so the terrain types of a map are read by `sf_data`
//! (the `terrain.png` of the map directory, checked against
//! `assets/config/terrain_types.ron`) and handed to the simulation here, when
//! the game starts a map. A map without a terrain layer gives a world of open
//! ground, exactly as before WP2.1; the demo's generated maps carry a layer.
//!
//! The presentation does not use the terrain types yet: materials per type are
//! WP2.2, the minimap WP2.3 and the grid overlay WP2.4.
//!
//! Which cells a unit may enter is the simulation's (WP4.1): the movement costs
//! of the unit definitions are handed over with the unit table
//! ([`with_movement`]), and the cursor asks the world, like the units do.

use sf_data::map::Map;
use sf_data::movement::MovementCosts;
use sf_data::terrain::TerrainTypes;
use sf_data::units::UnitDefs;
use sf_sim::{Terrain, TerrainDef, World, WorldConfig};

/// The simulation's table of terrain types: ids and flags, no names. The
/// `ground` summary is derived from the shipped movement costs.
pub fn sim_definitions(types: &TerrainTypes) -> Vec<TerrainDef> {
    sf_bridge::terrain_defs(types, &MovementCosts::embedded())
}

/// `world` with the terrain costs of the movement classes of `defs`, checked
/// against the shipped terrain types (the types every map layer was checked
/// against). Call it with the unit table, before the world starts.
///
/// # Panics
///
/// If `defs` leave out a terrain type of the shipped table or the world has
/// started; the sandbox, its one caller, has neither.
pub fn with_movement(world: World, defs: &UnitDefs) -> World {
    let costs = MovementCosts::from_defs(defs, &TerrainTypes::embedded()).unwrap_or_else(|e| {
        eprintln!("sf_game: {e}; using the movement costs compiled in");
        MovementCosts::embedded()
    });
    let table = sf_bridge::movement_table(&costs).expect("the movement costs make a valid table");
    world
        .with_movement(table)
        .expect("a world that has not started takes its movement table")
}

/// The terrain of `map` for the simulation: `None` for a map without a
/// terrain layer. The layer was checked against the shipped table when the map
/// was loaded (`Map::load`) or generated, so an error here means the map was
/// made with another table.
pub fn sim_terrain(map: &Map) -> Result<Option<Terrain>, String> {
    let Some(layer) = map.terrain() else {
        return Ok(None);
    };
    let defs = sim_definitions(&TerrainTypes::embedded());
    Terrain::from_layer(layer.width(), layer.height(), layer.ids(), &defs)
        .map(Some)
        .map_err(|e| e.to_string())
}

/// The world a map starts: on its terrain layer if it has one, on open ground
/// if it has none. A layer the simulation refuses is an error: the map was made
/// with another terrain table (or is corrupt), and a game that quietly started
/// on open ground would let units cross water and build on rock. The caller
/// reports it and stops (`sf_game` exits with status 1).
pub fn make_world(config: WorldConfig, map: &Map) -> Result<World, String> {
    match sim_terrain(map).map_err(|e| format!("terrain layer refused: {e}"))? {
        Some(terrain) => {
            World::with_terrain(config, terrain).map_err(|e| format!("terrain layer refused: {e}"))
        }
        None => World::new(config).map_err(|e| format!("map size refused: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_data::map::generate::generate_cells;
    use sf_data::terrain::TerrainLayer;
    use sf_sim::{Cell, TerrainId};

    fn config_for(map: &Map) -> WorldConfig {
        let cells = map.size_in_cells();
        WorldConfig::new(1).with_map_size(cells.0, cells.1)
    }

    #[test]
    fn a_generated_map_hands_its_terrain_to_the_sim() {
        let map = generate_cells(32, 24, 3);
        let world = make_world(config_for(&map), &map).unwrap();
        assert!(world.terrain().has_layer());
        assert_eq!(world.map_size(), (32, 24));
        let layer = map.terrain().unwrap();
        for y in 0..24 {
            for x in 0..32 {
                assert_eq!(
                    world.terrain_at(Cell::new(x, y)),
                    layer.get(x, y).map(TerrainId)
                );
            }
        }
        // The flags come from the table: water is not buildable, grass is.
        let types = TerrainTypes::embedded();
        let flags = |name: &str| {
            world
                .terrain()
                .flags_of(TerrainId(types.id_of(name).unwrap()))
                .unwrap()
        };
        assert!(flags("grass").buildable && !flags("water").buildable);
        assert!(!flags("water").ground && flags("bridge").ground);
        assert!(flags("ore").harvestable);
    }

    #[test]
    fn the_committed_test_map_starts_a_world_with_terrain() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps/test64");
        let map = Map::load(&dir).unwrap();
        let world = make_world(config_for(&map), &map).unwrap();
        assert_eq!(world.map_size(), (16, 16));
        assert!(world.terrain().has_layer());
    }

    #[test]
    fn a_map_without_a_layer_starts_a_world_of_open_ground() {
        let bare = Map::from_parts(
            16,
            16,
            1.0,
            vec!["a".into()],
            vec![0; 17 * 17],
            vec![0; 256],
        )
        .unwrap();
        assert_eq!(sim_terrain(&bare), Ok(None));
        let world = make_world(config_for(&bare), &bare).unwrap();
        assert!(!world.terrain().has_layer());
        assert_eq!(world.map_size(), (4, 4));
        // Same as the world sf_game made before WP2.1.
        let plain = World::new(config_for(&bare)).unwrap();
        assert_eq!(world.checksum(), plain.checksum());
    }

    #[test]
    fn a_map_loaded_without_a_layer_file_still_loads() {
        // A map directory without terrain.png, as every map was before WP2.1.
        let dir = std::env::temp_dir().join(format!("sf_game_playfield_{}", std::process::id()));
        let bare = Map::from_parts(
            16,
            16,
            1.0,
            vec!["a".into()],
            vec![0; 17 * 17],
            vec![0; 256],
        )
        .unwrap();
        bare.save(&dir).unwrap();
        let loaded = Map::load(&dir);
        std::fs::remove_dir_all(&dir).ok();
        let map = loaded.unwrap();
        assert!(map.terrain().is_none());
        assert!(
            !make_world(config_for(&map), &map)
                .unwrap()
                .terrain()
                .has_layer()
        );
    }

    #[test]
    fn a_layer_the_sim_refuses_is_an_error_not_open_ground() {
        // A layer with an id the shipped table does not know can only come
        // from a map made with another table; the game does not start on it.
        let bare = Map::from_parts(8, 8, 1.0, vec!["a".into()], vec![0; 81], vec![0; 64]).unwrap();
        let layer = TerrainLayer::new(2, 2, vec![0, 1, 77, 0]).unwrap();
        let mut custom = TerrainTypes::embedded().iter().cloned().collect::<Vec<_>>();
        let mut extra = custom[0].clone();
        extra.id = 77;
        extra.name = "other".into();
        custom.push(extra);
        let map = bare
            .with_terrain(layer, &TerrainTypes::new(custom).unwrap())
            .unwrap();
        assert!(sim_terrain(&map).is_err());
        let err = make_world(config_for(&map), &map).unwrap_err();
        assert!(err.contains("terrain layer refused"), "{err}");
    }
}
