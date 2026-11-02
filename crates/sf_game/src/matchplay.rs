//! The match (WP7.6, `sf_game --match`): milestone M1 as a game mode. The match file
//! (`assets/matches/m1.ron`, [`sf_data::match_def`]) names the map, the players, their credits and the
//! objects they start with; this module reads it, hands the rules of the match to the world and
//! schedules the starting objects through the command queue (nothing writes the world directly).
//!
//! The player has the sandbox's controls: build with B and the mouse, produce with the WP6.5 keys,
//! order units, attack. The opponent is **static**: no AI before phase 9 (it does not build, produce
//! or attack); its units stand where they are and fire back on their own (WP5.4), and its harvester
//! mines on its own (WP7.2). What is decided when is `docs/rules/match.md`.

use std::path::{Path, PathBuf};

use sf_data::match_def::MatchDef;
use sf_data::units::UnitDefs;
use sf_sim::{PlayerId, World};

use crate::{playfield, sandbox};

/// The match file `--match` starts without a name, relative to the assets directory.
pub const DEFAULT_MATCH: &str = "matches/m1.ron";

/// The match file to read: the one named on the command line, or the default under `assets`.
pub fn file_path(named: Option<&Path>, assets: &Path) -> PathBuf {
    named.map_or_else(|| assets.join(DEFAULT_MATCH), Path::to_path_buf)
}

/// Reads and checks the shape of the match file at `path`.
pub fn load(path: &Path) -> Result<MatchDef, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read match file {}: {e}", path.display()))?;
    MatchDef::from_ron(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The directory of the match's map under `assets`.
pub fn map_dir(def: &MatchDef, assets: &Path) -> PathBuf {
    assets.join("maps").join(&def.map)
}

/// Where the local player's view starts: the first object of the local player, in metres on the
/// ground, for the camera. `None` if the local player has no object (the file is checked against that
/// by [`check`]).
pub fn focus_metres(def: &MatchDef, local: PlayerId) -> Option<(f64, f64)> {
    let first = def
        .players
        .iter()
        .find(|p| p.id == local.0)?
        .objects
        .first()?;
    // A cell is 4 m (B12); the view looks at the middle of the cell.
    let metres = |c: u32| (f64::from(c) + 0.5) * f64::from(sf_math::render_bridge::METRES_PER_CELL);
    Some((metres(first.cell.0), metres(first.cell.1)))
}

/// Checks the match against what the game can play: the local player takes part, every seat has the
/// faction the sandbox's seat has (the models of a unit follow its owner), and the file fits the
/// definitions and the map (`cells`).
pub fn check(
    def: &MatchDef,
    defs: &UnitDefs,
    cells: (u32, u32),
    local: PlayerId,
) -> Result<(), String> {
    if !def.players.iter().any(|p| p.id == local.0) {
        return Err(format!(
            "the match has no player {}: the local player must take part",
            local.0
        ));
    }
    for p in &def.players {
        if sandbox::faction_of_owner(p.id) != Some(p.faction) {
            return Err(format!(
                "player {}: the game draws the models of seat {} as {:?}, the file says {:?}",
                p.id,
                p.id,
                sandbox::faction_of_owner(p.id),
                p.faction
            ));
        }
    }
    def.check(defs, cells).map_err(|e| e.to_string())
}

/// `world` (made on the map of the match) with every rule a match is played by: the unit table, the
/// building rules, the economy (harvest, storage and money with the credits of the file), production,
/// movement, combat and, last, the match rules. The world has not started; [`schedule`] puts the
/// starting objects in.
pub fn configure(world: World, def: &MatchDef, defs: &UnitDefs) -> Result<World, String> {
    let table = sf_bridge::unit_table(defs).map_err(|e| format!("unit table: {e}"))?;
    let world = world
        .with_units(table)
        .map_err(|e| format!("unit table refused: {e}"))?;
    let world = playfield::with_buildings(world, defs);
    let world = playfield::with_harvest(world, defs);
    let world = playfield::with_storage(world, defs);
    let world = playfield::with_money_for(world, &sf_bridge::match_credits(def));
    let world = playfield::with_production(world, defs);
    let world = playfield::with_movement(world, defs);
    let world = playfield::with_combat(world, defs);
    playfield::with_match(world, def)
}

/// What [`schedule`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Start {
    /// Commands scheduled: one per starting object.
    pub objects: u32,
}

/// Schedules the starting objects of the match for the tick about to run.
pub fn schedule(world: &mut World, def: &MatchDef, defs: &UnitDefs) -> Result<Start, String> {
    let commands = sf_bridge::match_commands(def, defs)?;
    let objects = commands.len() as u32;
    for (player, command) in commands {
        world
            .schedule(world.tick(), player, command)
            .map_err(|e| format!("cannot schedule a starting object: {e}"))?;
    }
    Ok(Start { objects })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_data::map::Map;
    use sf_sim::{Verdict, WorldConfig};

    fn assets() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
    }

    fn m1() -> (MatchDef, UnitDefs, Map) {
        let def = load(&file_path(None, &assets())).unwrap();
        let map = Map::load(&map_dir(&def, &assets())).unwrap();
        (def, UnitDefs::embedded(), map)
    }

    fn world(def: &MatchDef, defs: &UnitDefs, map: &Map) -> World {
        let cells = map.size_in_cells();
        let world = playfield::make_world(
            WorldConfig::new(def.seed).with_map_size(cells.0, cells.1),
            map,
        )
        .unwrap();
        configure(world, def, defs).unwrap()
    }

    #[test]
    fn the_default_is_the_first_match_under_assets() {
        let path = file_path(None, Path::new("assets"));
        assert_eq!(path, Path::new("assets").join("matches/m1.ron"));
        assert_eq!(DEFAULT_MATCH, "matches/m1.ron");
        let named = file_path(Some(Path::new("x/y.ron")), Path::new("assets"));
        assert_eq!(named, Path::new("x/y.ron"));
        // The file on disk is the one compiled in.
        assert_eq!(
            load(&file_path(None, &assets())).unwrap(),
            MatchDef::embedded()
        );
    }

    #[test]
    fn the_camera_opens_on_the_first_object_of_the_local_player() {
        let (def, ..) = m1();
        // The construction centre on cell (12, 68): the middle of the cell, 4 m to a cell.
        assert_eq!(focus_metres(&def, PlayerId(0)), Some((50.0, 274.0)));
        // The opponent's first object, the power plant on (76, 20).
        assert_eq!(focus_metres(&def, PlayerId(1)), Some((306.0, 82.0)));
        assert_eq!(focus_metres(&def, PlayerId(7)), None);
    }

    #[test]
    fn the_shipped_match_is_playable_and_the_world_takes_every_starting_object() {
        let (def, defs, map) = m1();
        check(&def, &defs, map.size_in_cells(), PlayerId(0)).unwrap();
        let mut w = world(&def, &defs, &map);
        assert_eq!(w.match_players(), &[PlayerId(0), PlayerId(1)]);
        let start = schedule(&mut w, &def, &defs).unwrap();
        assert_eq!(start.objects, 11);
        let report = w.step();
        assert_eq!(report.commands_ignored, 0);
        assert_eq!(report.commands_executed, 11);
        // The credits of the file, not of money.ron (1000 each).
        assert_eq!(w.credits(PlayerId(0)), 4000);
        assert_eq!(w.credits(PlayerId(1)), 0);
        // The match has begun and nobody has lost.
        assert!(w.match_begun());
        assert_eq!(w.match_result(), None);
    }

    #[test]
    fn the_match_is_won_in_the_world_the_game_makes() {
        // Remove the opponent's armed units and structures (slots 1 to 10 except the harvester, 5):
        // the human wins, in the tick they went.
        let (def, defs, map) = m1();
        let mut w = world(&def, &defs, &map);
        schedule(&mut w, &def, &defs).unwrap();
        w.step();
        let tick = w.tick();
        for slot in [1, 2, 3, 4, 6, 7, 8, 9, 10] {
            let id = sf_sim::EntityId::new(slot, 0);
            w.schedule(tick, PlayerId(0), sf_sim::Command::Despawn { id })
                .unwrap();
        }
        // Before: nothing in the snapshot the HUD reads; after: the result of the world.
        assert_eq!(
            crate::sim_snapshot::SimSnapshot::capture(&w).match_result(),
            None
        );
        w.step();
        let result = w.match_result().expect("decided");
        assert_eq!(result.tick, tick);
        assert_eq!(result.verdict, Verdict::Won(PlayerId(0)));
        assert_eq!(
            crate::sim_snapshot::SimSnapshot::capture(&w).match_result(),
            Some(result)
        );
    }

    #[test]
    fn a_structure_still_going_up_does_not_keep_its_owner_in_the_match() {
        // The human builds a power plant next to its construction centre; while it goes up the centre
        // is lost: the plant under construction is not something that stands, so the human is
        // defeated, and the opponent wins in that tick.
        let (def, defs, map) = m1();
        let mut w = world(&def, &defs, &map);
        schedule(&mut w, &def, &defs).unwrap();
        w.step();
        let tick = w.tick();
        let plant = sf_sim::Kind(defs.kind_of("power_plant").unwrap());
        w.schedule(
            tick,
            PlayerId(0),
            sf_sim::Command::BuildStructure {
                kind: plant,
                cell: sf_sim::Cell::new(8, 62),
            },
        )
        .unwrap();
        w.step();
        assert_eq!(w.under_construction(), 1, "the plant goes up");
        assert_eq!(w.match_result(), None, "the centre stands");
        let tick = w.tick();
        let centre = sf_sim::EntityId::new(0, 0);
        w.schedule(tick, PlayerId(0), sf_sim::Command::Despawn { id: centre })
            .unwrap();
        w.step();
        assert_eq!(w.under_construction(), 1, "the plant is still going up");
        assert_eq!(w.defeat_tick(PlayerId(0)), Some(tick));
        assert_eq!(
            w.match_result().map(|r| r.verdict),
            Some(Verdict::Won(PlayerId(1)))
        );
    }

    #[test]
    fn a_match_the_game_cannot_play_is_refused_with_the_reason() {
        let (def, defs, map) = m1();
        let cells = map.size_in_cells();
        // The local player must take part.
        let mut other = def.clone();
        other.players[0].id = 4;
        assert!(
            check(&other, &defs, cells, PlayerId(0))
                .unwrap_err()
                .contains("no player 0")
        );
        // The models follow the seat: seat 1 is drawn as Veyran.
        let mut swapped = def.clone();
        swapped.players[1].faction = sf_data::units::Faction::Meridian;
        let e = check(&swapped, &defs, cells, PlayerId(0)).unwrap_err();
        assert!(e.contains("player 1") && e.contains("Meridian"), "{e}");
        // A file that does not fit the map is the data's error.
        assert!(check(&def, &defs, (40, 40), PlayerId(0)).is_err());
        // A file that is not there.
        assert!(
            load(Path::new("no/such/match.ron"))
                .unwrap_err()
                .contains("cannot read")
        );
    }
}
