//! The world of `sf_game --match`, made without the game (WP1.16a).
//!
//! A recorded game (`sf_game --match --record FILE`) is a scenario with a `game` block. Its world is
//! not described by the scenario's rule fields; it is made the way the game makes it: the unit
//! definitions from `<assets>/config` (the compiled-in copy when they cannot be read, as in the game),
//! the match file and the map under `<assets>`, and every other rule from the settings compiled into
//! this build (`sf_data::*::embedded()`), in the order of `sf_game::matchplay::configure`. Then the
//! starting objects are scheduled and one tick is run, as `match_scene` does before the sim thread
//! starts. The result is the world the game's sim thread begins with: the test
//! `tests/record_replay.rs` of `sf_game` plays one in both and compares the checksum at every tick.
//!
//! This repeats the wiring of `sf_game::playfield` and `matchplay`, because `sf_headless` may not
//! depend on the game. The test above is what keeps the two in step.

use std::path::Path;

use sf_data::map::Map;
use sf_data::match_def::MatchDef;
use sf_data::movement::MovementCosts;
use sf_data::terrain::TerrainTypes;
use sf_data::units::UnitDefs;
use sf_math::Fx;
use sf_sim::{PlayerId, Terrain, Tick, World, WorldConfig};

use super::ScenarioError;

/// The `game` block of a scenario: where the assets are and which match was played.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameSpec {
    /// The asset directory the game ran with (`assets/`): an absolute path, or one relative to the
    /// scenario file.
    pub assets: String,
    /// The match file under `assets` (`matches/m1.ron`), or an absolute path.
    pub match_path: String,
    /// The world's walking speed as a raw fixed-point number (`WorldConfig::speed`; it is part of the
    /// checksum). The game's own is `sf_game::bench_scene::DEMO_SPEED`.
    pub speed_raw: i64,
}

/// What [`load_game`] reads for [`build_game_world`].
#[derive(Clone, Debug)]
pub struct LoadedGame {
    pub(crate) def: MatchDef,
    pub(crate) defs: UnitDefs,
    pub(crate) terrain: Terrain,
    pub(crate) commands: Vec<(PlayerId, sf_sim::Command)>,
}

impl PartialEq for LoadedGame {
    fn eq(&self, other: &Self) -> bool {
        self.def == other.def && self.commands == other.commands
    }
}

impl Eq for LoadedGame {}

fn resolve(base: &Path, p: &str) -> std::path::PathBuf {
    let p = Path::new(p);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

/// The unit definitions of `assets`: the same fallback as the game (`sf_game::runner::load_defs`), and the
/// same warning: a replay on the compiled-in definitions is a replay of different numbers, so it must not
/// pass unnoticed.
pub(crate) fn load_defs(assets: &Path) -> UnitDefs {
    UnitDefs::load(&assets.join("config"), assets).unwrap_or_else(|e| {
        eprintln!(
            "sf_headless: {}: {e}; using the unit definitions compiled in",
            assets.join("config").display()
        );
        UnitDefs::embedded()
    })
}

/// Reads the match file at `match_path`, the unit definitions of `assets` and the map the match names, for a
/// match that is played without a scenario file around it (`sf_headless tournament`, WP9.5a). Returns the
/// match and the size of its map in cells. The error is the whole message.
pub(crate) fn load_match_file(
    assets: &Path,
    match_path: &Path,
) -> Result<(LoadedGame, (u32, u32)), String> {
    let text = std::fs::read_to_string(match_path)
        .map_err(|e| format!("cannot read {}: {e}", match_path.display()))?;
    let def = MatchDef::from_ron(&text).map_err(|e| format!("{}: {e}", match_path.display()))?;
    let defs = load_defs(assets);
    let map_dir = assets.join("maps").join(&def.map);
    let map = Map::load(&map_dir).map_err(|e| format!("{}: {e}", map_dir.display()))?;
    let layer = map
        .terrain()
        .ok_or_else(|| format!("{}: the map has no terrain layer", map_dir.display()))?;
    let cells = (layer.width(), layer.height());
    def.check(&defs, cells)
        .map_err(|e| format!("{}: {e}", match_path.display()))?;
    let terrain_defs =
        sf_bridge::terrain_defs(&TerrainTypes::embedded(), &MovementCosts::embedded());
    let terrain = Terrain::from_layer(cells.0, cells.1, layer.ids(), &terrain_defs)
        .map_err(|e| e.to_string())?;
    let commands = sf_bridge::match_commands(&def, &defs)?;
    let loaded = LoadedGame {
        def,
        defs,
        terrain,
        commands,
    };
    Ok((loaded, cells))
}

impl LoadedGame {
    /// The same match with the players given other factions (`factions[i]` for the i-th player of the file)
    /// and the starting commands made again for them: the seat swap of the tournament. The objects stay
    /// where the file puts them (a faction sits on a start site), so the roles must exist for the new
    /// faction; the match is checked again against the definitions.
    pub(crate) fn with_factions(
        &self,
        factions: &[sf_data::units::Faction],
        cells: (u32, u32),
    ) -> Result<LoadedGame, String> {
        let mut def = self.def.clone();
        if factions.len() != def.players.len() {
            return Err(format!(
                "{} factions for {} players",
                factions.len(),
                def.players.len()
            ));
        }
        for (player, faction) in def.players.iter_mut().zip(factions) {
            player.faction = *faction;
        }
        def.check(&self.defs, cells).map_err(|e| e.to_string())?;
        let commands = sf_bridge::match_commands(&def, &self.defs)?;
        Ok(LoadedGame {
            def,
            defs: self.defs.clone(),
            terrain: self.terrain.clone(),
            commands,
        })
    }
}

/// Reads the match file, the unit definitions and the map of `game`, relative to `scenario_file`.
pub fn load_game(
    game: &GameSpec,
    scenario_file: &Path,
    cells: (u32, u32),
) -> Result<LoadedGame, ScenarioError> {
    let invalid = |problem: String| ScenarioError::Invalid {
        path: Some(scenario_file.to_path_buf()),
        problems: vec![format!("game: {problem}")],
    };
    let base = scenario_file.parent().unwrap_or(Path::new(""));
    let assets = resolve(base, &game.assets);
    let match_path = resolve(&assets, &game.match_path);
    let text = std::fs::read_to_string(&match_path)
        .map_err(|e| invalid(format!("{}: {e}", match_path.display())))?;
    let def = MatchDef::from_ron(&text).map_err(|e| invalid(e.to_string()))?;
    let defs = load_defs(&assets);
    def.check(&defs, cells)
        .map_err(|e| invalid(e.to_string()))?;
    let map_dir = assets.join("maps").join(&def.map);
    let map = Map::load(&map_dir).map_err(|e| invalid(format!("{}: {e}", map_dir.display())))?;
    let layer = map
        .terrain()
        .ok_or_else(|| invalid("the map has no terrain layer".into()))?;
    if (layer.width(), layer.height()) != cells {
        return Err(invalid(format!(
            "the map is {}x{} cells but the scenario's map is {}x{}",
            layer.width(),
            layer.height(),
            cells.0,
            cells.1
        )));
    }
    let terrain_defs =
        sf_bridge::terrain_defs(&TerrainTypes::embedded(), &MovementCosts::embedded());
    let terrain = Terrain::from_layer(cells.0, cells.1, layer.ids(), &terrain_defs)
        .map_err(|e| invalid(e.to_string()))?;
    let commands = sf_bridge::match_commands(&def, &defs).map_err(invalid)?;
    Ok(LoadedGame {
        def,
        defs,
        terrain,
        commands,
    })
}

/// The game's world after its first tick, as the sim thread receives it.
pub fn build_game_world(
    seed: u64,
    cells: (u32, u32),
    game: &GameSpec,
    loaded: &LoadedGame,
) -> Result<World, ScenarioError> {
    build_world_at_speed(seed, cells, Fx::from_raw(game.speed_raw), loaded)
}

/// [`build_game_world`] with the walking speed given directly: the world of a match played without a scenario
/// file (`sf_headless tournament`).
pub(crate) fn build_world_at_speed(
    seed: u64,
    cells: (u32, u32),
    speed: Fx,
    loaded: &LoadedGame,
) -> Result<World, ScenarioError> {
    let sim = |e: String| ScenarioError::Sim(e);
    let defs = &loaded.defs;
    let def = &loaded.def;
    let config = WorldConfig {
        speed,
        ..WorldConfig::new(seed).with_map_size(cells.0, cells.1)
    };
    // `playfield::make_world`.
    let world = World::with_terrain(config, loaded.terrain.clone())
        .map_err(|e| sim(e.to_string()))?
        .with_ore(sf_bridge::ore_rules(&sf_data::ore::OreSettings::embedded()))
        .map_err(|e| sim(e.to_string()))?;
    // `matchplay::configure`, in its order.
    let table = sf_bridge::unit_table(defs).map_err(sim)?;
    let world = world
        .with_units(table)
        .map_err(|e| sim(e.to_string()))?
        .with_buildings(sf_bridge::building_table(defs).map_err(sim)?)
        .map_err(|e| sim(e.to_string()))?
        .with_player_factions(&sf_bridge::match_factions(def))
        .map_err(|e| sim(e.to_string()))?
        .with_harvest(
            sf_bridge::harvest_rules(&sf_data::harvest::HarvestSettings::embedded()),
            &sf_bridge::harvester_kinds(defs),
        )
        .map_err(|e| sim(e.to_string()))?
        .with_storage(&sf_bridge::storage_capacities(defs))
        .map_err(|e| sim(e.to_string()))?
        .with_money(
            sf_bridge::money_rules(&sf_data::money::MoneySettings::embedded()),
            &sf_bridge::match_credits(def),
        )
        .map_err(|e| sim(e.to_string()))?
        .with_production(
            sf_bridge::production_rules(&sf_data::production::ProductionSettings::embedded()),
            sf_bridge::production_table(defs).map_err(sim)?,
        )
        .map_err(|e| sim(e.to_string()))?
        .with_power(sf_bridge::power_rules(
            &sf_data::power::PowerSettings::embedded(),
        ))
        .map_err(|e| sim(e.to_string()))?
        .with_repair(sf_bridge::repair_rules(
            &sf_data::repair::RepairSettings::embedded(),
        ))
        .map_err(|e| sim(e.to_string()))?
        .with_vision(sf_bridge::vision_rules(
            &sf_data::vision::VisionSettings::embedded(),
            defs,
        ))
        .map_err(|e| sim(e.to_string()))?;
    // `playfield::with_movement`.
    let costs = MovementCosts::from_defs(defs, &TerrainTypes::embedded())
        .unwrap_or_else(|_| MovementCosts::embedded());
    let settings = sf_data::sim_settings::SimSettings::embedded();
    let world = world
        .with_movement(sf_bridge::movement_table(&costs).map_err(sim)?)
        .map_err(|e| sim(e.to_string()))?
        .with_path_budget(sf_bridge::path_budget(&settings))
        .map_err(|e| sim(e.to_string()))?
        .with_flowfield_threshold(sf_bridge::flowfield_threshold(&settings))
        .map_err(|e| sim(e.to_string()))?
        .with_collision(sf_bridge::collision_config(&settings))
        .map_err(|e| sim(e.to_string()))?
        .with_replan(sf_bridge::replan_config(&settings))
        .map_err(|e| sim(e.to_string()))?
        .with_formation(sf_bridge::formation_config(&settings))
        .map_err(|e| sim(e.to_string()))?;
    // `playfield::with_combat`.
    let mut world = world
        .with_weapons(sf_bridge::weapon_table(defs).map_err(sim)?)
        .map_err(|e| sim(e.to_string()))?
        .with_targeting(sf_bridge::targeting_config(&settings))
        .map_err(|e| sim(e.to_string()))?
        .with_destruction(sf_bridge::destruction_config(&settings))
        .map_err(|e| sim(e.to_string()))?
        .with_match(sf_bridge::match_rules(def).map_err(sim)?)
        .map_err(|e| sim(e.to_string()))?;
    // `matchplay::schedule`, then the tick `match_scene` runs before the thread starts.
    for (player, command) in &loaded.commands {
        world
            .schedule(Tick(0), *player, command.clone())
            .map_err(|e| sim(e.to_string()))?;
    }
    world.step();
    Ok(world)
}
