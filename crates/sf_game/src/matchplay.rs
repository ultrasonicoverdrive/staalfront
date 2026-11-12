//! The match (WP7.6, `sf_game --match`): milestone M1 as a game mode. The match file
//! (`assets/matches/m1.ron`, [`sf_data::match_def`]) names the map, the players, their credits and the
//! objects they start with; this module reads it, hands the rules of the match to the world and
//! schedules the starting objects through the command queue (nothing writes the world directly).
//!
//! The player has the sandbox's controls: build with B and the mouse, produce with the WP6.5 keys,
//! order units, attack. The opponent is **static** unless the match file gives it a computer
//! controller (WP9.1d, [`ais`]): a static one does not build, produce or attack; its units stand where they
//! are and fire back on their own (WP5.4), and its harvester mines on its own (WP7.2). What is decided
//! when is `docs/rules/match.md`.

use std::path::{Path, PathBuf};

use sf_data::ai_profile::AiProfile;
use sf_data::match_def::{Controller, MatchDef};
use sf_data::units::UnitDefs;
use sf_sim::{PlayerId, World};

use crate::{playfield, sandbox};

/// The match file `--match` starts without a name, relative to the assets directory.
pub const DEFAULT_MATCH: &str = "matches/m1.ron";

/// The match file to read: the one named on the command line, or the default under `assets`. A bare name
/// (one component, no extension: `m1_ai`) is the file of that name under `assets/matches` (WP9.1d); anything
/// else is a path as given.
pub fn file_path(named: Option<&Path>, assets: &Path) -> PathBuf {
    match named {
        None => assets.join(DEFAULT_MATCH),
        Some(path) if path.components().count() == 1 && path.extension().is_none() => {
            assets.join("matches").join(path).with_extension("ron")
        }
        Some(path) => path.to_path_buf(),
    }
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
    if let Some(p) = def
        .players
        .iter()
        .find(|p| p.id == local.0 && p.controller != Controller::Standard)
    {
        return Err(format!(
            "player {} is the local player and takes its orders from the keyboard: it cannot have the controller {:?}",
            p.id, p.controller
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
    // WP6.4: the structures make and use power; a shortage slows building and production.
    let world = playfield::with_power(world);
    // WP6.6: a player can repair and sell its structures.
    let world = playfield::with_repair(world);
    // WP8.1: each player's sight is worked out (nothing reads it yet; the debug view shows it).
    let world = playfield::with_vision(world, defs);
    let world = playfield::with_movement(world, defs);
    let world = playfield::with_combat(world, defs);
    playfield::with_match(world, def)
}

/// The computer opponents of the match (WP9.1d), ascending by player id: one `sf_ai::Ai` for every player
/// whose controller is `Ai`, made the way `sf_headless` makes the AIs of a scenario (`Scenario::load_ais`:
/// the profile file is read and checked, resolved for the player's faction against the unit definitions,
/// and every AI is told the start sites of the match; copied here and not shared because that function is
/// private to the scenario loader and `sf_ai` is changed by another work package at the same time). The
/// profile is read from `<assets>/ai/<profile>.ron`; a profile that is not there, or does not fit, is an
/// error that names the player and the profile. The random seed of an AI is the match's `seed`.
pub fn ais(def: &MatchDef, defs: &UnitDefs, assets: &Path) -> Result<Vec<sf_ai::Ai>, String> {
    let sites = sf_ai::start_sites(def, defs);
    let mut ais = Vec::new();
    for p in &def.players {
        let Controller::Ai { profile } = &p.controller else {
            continue;
        };
        let at = format!("player {} (ai profile \"{profile}\")", p.id);
        let file = assets.join("ai").join(format!("{profile}.ron"));
        let text = std::fs::read_to_string(&file)
            .map_err(|e| format!("{at}: cannot read {}: {e}", file.display()))?;
        let parsed = AiProfile::from_ron(&text).map_err(|e| format!("{at}: {e}"))?;
        let resolved =
            sf_ai::Profile::resolve(&parsed, p.faction, defs).map_err(|e| format!("{at}: {e}"))?;
        ais.push(sf_ai::Ai::new(
            PlayerId(p.id),
            resolved,
            def.seed,
            sites.clone(),
        ));
    }
    ais.sort_by_key(sf_ai::Ai::player);
    Ok(ais)
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

    pub(super) fn assets() -> PathBuf {
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
    fn a_bare_name_is_a_match_under_assets_matches_and_anything_else_a_path() {
        let assets = Path::new("assets");
        let at = |name: &str| file_path(Some(Path::new(name)), assets);
        assert_eq!(at("m1_ai"), Path::new("assets/matches/m1_ai.ron"));
        assert_eq!(at("m1.ron"), Path::new("m1.ron"));
        assert_eq!(at("x/m1_ai"), Path::new("x/m1_ai"));
        // The shipped file the bare name finds loads and has the AI.
        let def = load(&file_path(
            Some(Path::new("m1_ai")),
            &super::tests::assets(),
        ))
        .unwrap();
        assert_eq!(def, MatchDef::embedded_ai());
    }

    #[test]
    fn the_local_player_cannot_be_an_ai() {
        let (def, defs, map) = m1();
        let mut ai = MatchDef::embedded_ai();
        check(&ai, &defs, map.size_in_cells(), PlayerId(0)).unwrap();
        ai.players[0].controller = Controller::Ai {
            profile: "normal".to_string(),
        };
        let e = check(&ai, &defs, map.size_in_cells(), PlayerId(0)).unwrap_err();
        assert!(e.contains("player 0") && e.contains("local player"), "{e}");
        // The default match is untouched by the rule.
        check(&def, &defs, map.size_in_cells(), PlayerId(0)).unwrap();
    }

    #[test]
    fn the_ais_come_from_the_controllers_and_a_missing_profile_is_named() {
        let (def, defs, _) = m1();
        // Standard players only: no AI.
        assert!(ais(&def, &defs, &assets()).unwrap().is_empty());
        let ai = MatchDef::embedded_ai();
        let made = ais(&ai, &defs, &assets()).unwrap();
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].player(), PlayerId(1));
        // A profile that is not under assets/ai: the error names the player and the profile.
        let mut other = ai.clone();
        other.players[1].controller = Controller::Ai {
            profile: "nonesuch".to_string(),
        };
        let e = ais(&other, &defs, &assets()).unwrap_err();
        assert!(e.contains("player 1") && e.contains("nonesuch"), "{e}");
    }

    #[test]
    fn the_match_seed_reaches_the_ai_and_a_profile_without_the_faction_is_refused() {
        let (_, defs, _) = m1();
        let digest = |seed: u64| {
            let mut def = MatchDef::embedded_ai();
            def.seed = seed;
            ais(&def, &defs, &assets()).unwrap()[0].digest()
        };
        // The random stream is part of the AI's digest: the seed of the file is the AI's seed.
        assert_eq!(digest(7), digest(7));
        assert_ne!(digest(7), digest(8));
        // Two AI players listed in descending order come out ascending.
        let mut two = MatchDef::embedded_ai();
        two.players[0].controller = Controller::Ai {
            profile: "normal".to_string(),
        };
        two.players.reverse();
        let made = ais(&two, &defs, &assets()).unwrap();
        assert_eq!(
            made.iter().map(|a| a.player().0).collect::<Vec<_>>(),
            [0, 1]
        );
        // A profile file with a section for Meridian only, for the Veyran player: refused, naming both.
        // A Windows checkout has CRLF line ends; the cut below looks for LF.
        let normal = std::fs::read_to_string(assets().join("ai/normal.ron"))
            .unwrap()
            .replace("\r\n", "\n");
        let cut = normal
            .find("        (\n            faction: Veyran")
            .unwrap();
        let end = normal.rfind("    ],\n)").unwrap();
        let only_meridian = format!("{}{}", &normal[..cut], &normal[end..]);
        let dir = std::env::temp_dir().join(format!("sf_matchplay_faction_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("ai")).unwrap();
        std::fs::write(dir.join("ai/only_meridian.ron"), only_meridian).unwrap();
        let mut def = MatchDef::embedded_ai();
        def.players[1].controller = Controller::Ai {
            profile: "only_meridian".to_string(),
        };
        let e = ais(&def, &defs, &dir).unwrap_err();
        std::fs::remove_dir_all(&dir).ok();
        assert!(
            e.contains("player 1")
                && e.contains("only_meridian")
                && e.contains("no section for Veyran"),
            "{e}"
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
        assert_eq!(start.objects, 12);
        let report = w.step();
        assert_eq!(report.commands_ignored, 0);
        assert_eq!(report.commands_executed, 12);
        // The credits of the file, not of money.ron (1000 each).
        assert_eq!(w.credits(PlayerId(0)), 4000);
        assert_eq!(w.credits(PlayerId(1)), 0);
        // The match has begun and nobody has lost.
        assert!(w.match_begun());
        assert_eq!(w.match_result(), None);
    }

    #[test]
    fn the_match_is_won_in_the_world_the_game_makes() {
        // WP7.7: removing everything of the opponent but its construction centre (slot 11) decides
        // nothing; removing the centre does, in the tick it went.
        let (def, defs, map) = m1();
        let mut w = world(&def, &defs, &map);
        schedule(&mut w, &def, &defs).unwrap();
        w.step();
        let tick = w.tick();
        for slot in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] {
            let id = sf_sim::EntityId::new(slot, 0);
            w.schedule(tick, PlayerId(0), sf_sim::Command::Despawn { id })
                .unwrap();
        }
        w.step();
        assert_eq!(w.match_result(), None, "the centre still stands");
        let tick = w.tick();
        let centre = sf_sim::EntityId::new(11, 0);
        w.schedule(tick, PlayerId(0), sf_sim::Command::Despawn { id: centre })
            .unwrap();
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
