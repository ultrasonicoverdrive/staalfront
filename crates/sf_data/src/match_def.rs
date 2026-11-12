//! The match file (WP7.6, `assets/matches/*.ron`): one map, the players that take part, the credits each
//! starts with and the objects each starts with. It is what M1 plays: `sf_game --match <file>` starts it,
//! the headless scenario `match` runs it, and `docs/rules/match.md` describes the rules it is played by.
//!
//! The file is data and not code (PLAN §5 rule 6). It is checked when it is read ([`MatchDef::from_ron`]:
//! the shape, the players, the numbers) and against the unit definitions and the map ([`MatchDef::check`]:
//! every role exists, belongs to the player's faction and is a structure or a unit, every object lies on
//! the map and no two structures overlap). Whether the ground under an object is buildable or passable is
//! the simulation's to say: a starting object enters the world through a command, and a command the world
//! refuses is reported by the caller (`sf_bridge::match_commands`, `sf_game`, `sf_headless`).
//!
//! The match file is the **minimum** that M1 needs. The scenario format of phase 10 (WP10.1) takes it
//! over: starting objects and players move into the scenario, and the fixed win and loss rule of
//! `sf_sim::outcome` becomes one goal among others.
//!
//! ```
//! use sf_data::match_def::MatchDef;
//!
//! let m1 = MatchDef::embedded();
//! assert_eq!(m1.players.len(), 2);
//! ```

use std::collections::BTreeSet;
use std::fmt;

use serde::Deserialize;

use crate::units::{BuildingFunction, Category, Faction, UnitDefs};

/// The shipped first match, compiled in.
pub const EMBEDDED_M1: &str = include_str!("../../../assets/matches/m1.ron");

/// The shipped match against a computer opponent (WP9.1d), compiled in.
pub const EMBEDDED_M1_AI: &str = include_str!("../../../assets/matches/m1_ai.ron");

/// The most players a match file can list (the simulation's limit, `sf_sim::MAX_MATCH_PLAYERS`).
pub const MAX_PLAYERS: usize = 8;
/// The most credits a player can start with (the money rules' limit, `sf_sim::MAX_STARTING_CREDITS`).
pub const MAX_CREDITS: u64 = 1_000_000_000;

/// Why a match file was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchError(pub String);

impl fmt::Display for MatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "match file: {}", self.0)
    }
}

impl std::error::Error for MatchError {}

fn err<T>(message: impl Into<String>) -> Result<T, MatchError> {
    Err(MatchError(message.into()))
}

/// One object a player starts with: a structure (its footprint is anchored on `cell`, its smallest x and
/// y) or a unit (it stands in the middle of `cell`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchObject {
    /// The id of a role in `unit_defs.ron`.
    pub role: String,
    /// The cell `(x, y)`.
    pub cell: (u32, u32),
}

/// Who plays a seat (WP9.1d). Left out of the file it is [`Controller::Standard`], which is what every
/// match file before WP9.1d meant.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub enum Controller {
    /// The rules of the first match: the player with the id of the game's local player is the human at the
    /// keyboard, every other player is static (it builds, produces and attacks nothing).
    #[default]
    Standard,
    /// A computer opponent (`sf_ai`) with the profile `assets/ai/<profile>.ron` (the name without `.ron`,
    /// for example `normal`). The game's sim thread thinks for it before every step it is due. That the
    /// name is a plain name is checked here; that the file exists and fits the player's faction where the
    /// game reads it (`sf_game::matchplay::ais`), which names the profile in its error.
    Ai {
        /// The profile's file name under `assets/ai/`, without `.ron`.
        profile: String,
    },
}

/// One player.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchPlayer {
    /// The player number.
    pub id: u8,
    /// Which faction the player is: decides what it may build.
    pub faction: Faction,
    /// The credits it starts with, 0 to [`MAX_CREDITS`].
    pub credits: u64,
    /// What it starts with, in the order the objects enter the world (which fixes their slots).
    pub objects: Vec<MatchObject>,
    /// Who plays it; `Standard` when the file does not say.
    #[serde(default)]
    pub controller: Controller,
}

/// The file as written.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchDef {
    /// A short name for the console and the window.
    pub name: String,
    /// The map: the name of a directory under `assets/maps`.
    pub map: String,
    /// The world seed.
    pub seed: u64,
    /// The players that take part, in the order their objects are made; the first is the human.
    pub players: Vec<MatchPlayer>,
}

impl MatchDef {
    /// Reads and checks the shape of a match file. The error says what is wrong.
    pub fn from_ron(text: &str) -> Result<MatchDef, MatchError> {
        let def: MatchDef = ron::from_str(text).map_err(|e| MatchError(e.to_string()))?;
        if def.name.trim().is_empty() {
            return err("name must not be empty");
        }
        crate::map::check_file_name("map", &def.map).map_err(|e| MatchError(e.to_string()))?;
        if def.map.contains('/') {
            return err(format!(
                "map \"{}\" must be the name of one map directory, without '/'",
                def.map
            ));
        }
        if !(2..=MAX_PLAYERS).contains(&def.players.len()) {
            return err(format!(
                "{} players listed, a match has 2 to {MAX_PLAYERS}",
                def.players.len()
            ));
        }
        let mut seen = BTreeSet::new();
        for p in &def.players {
            if !seen.insert(p.id) {
                return err(format!("player {} is listed twice", p.id));
            }
            if p.faction == Faction::Neutral {
                return err(format!(
                    "player {}: the faction cannot be Neutral (nobody plays it)",
                    p.id
                ));
            }
            if p.credits > MAX_CREDITS {
                return err(format!(
                    "player {}: {} credits is above the limit of {MAX_CREDITS}",
                    p.id, p.credits
                ));
            }
            if p.objects.is_empty() {
                return err(format!("player {} starts with nothing", p.id));
            }
            if let Controller::Ai { profile } = &p.controller
                && (profile.is_empty()
                    || !profile
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
            {
                return err(format!(
                    "player {}: ai profile \"{profile}\" must be a name of lower-case letters, digits and '_' \
                     (the file assets/ai/<name>.ron), like \"normal\"",
                    p.id
                ));
            }
        }
        Ok(def)
    }

    /// The shipped first match. Panics if the shipped file is wrong (a test reads it).
    pub fn embedded() -> MatchDef {
        MatchDef::from_ron(EMBEDDED_M1).unwrap_or_else(|e| panic!("assets/matches/m1.ron: {e}"))
    }

    /// The shipped match against a computer opponent (WP9.1d), compiled in.
    pub fn embedded_ai() -> MatchDef {
        MatchDef::from_ron(EMBEDDED_M1_AI)
            .unwrap_or_else(|e| panic!("assets/matches/m1_ai.ron: {e}"))
    }

    /// Checks the file against the unit definitions and the size of its map (in cells): every role
    /// exists, is a structure or a mobile unit (not a vessel: there is no water to stand on in the
    /// first match, and an object or a neutral role belongs to nobody), is open to the player's
    /// faction, and lies on the map; no two structures overlap; and every player starts with a
    /// construction centre (a structure of the building function `Anchor`): a player is defeated when
    /// it owns no standing one (WP7.7, `docs/rules/match.md`), so a player without one would be
    /// defeated before it began. A player that is to get its first construction centre later is not
    /// something this file can express.
    pub fn check(&self, defs: &UnitDefs, map_cells: (u32, u32)) -> Result<(), MatchError> {
        let mut taken = BTreeSet::new();
        for p in &self.players {
            let mut construction_centres = 0;
            for (i, o) in p.objects.iter().enumerate() {
                let at = format!("player {} object {i} ({})", p.id, o.role);
                let Some(def) = defs.unit(&o.role) else {
                    return err(format!("{at}: no such role in unit_defs.ron"));
                };
                if !def.factions.contains(&p.faction) {
                    return err(format!(
                        "{at}: the role is not open to the faction {:?}",
                        p.faction
                    ));
                }
                let (w, h) = match def.category {
                    Category::Structure => defs
                        .sim_unit(def.kind)
                        .and_then(|u| u.footprint)
                        .ok_or_else(|| {
                            MatchError(format!("{at}: a structure without a footprint"))
                        })?,
                    Category::Infantry | Category::Vehicle => (1, 1),
                    Category::Vessel | Category::Object => {
                        return err(format!(
                            "{at}: only structures, infantry and vehicles can start a match"
                        ));
                    }
                };
                if def.category == Category::Structure
                    && def
                        .building
                        .as_ref()
                        .is_some_and(|b| b.function == BuildingFunction::Anchor)
                {
                    construction_centres += 1;
                }
                let (x1, y1) = (o.cell.0 + u32::from(w), o.cell.1 + u32::from(h));
                if x1 > map_cells.0 || y1 > map_cells.1 {
                    return err(format!(
                        "{at}: cell {:?} with its footprint lies outside the {}x{} map",
                        o.cell, map_cells.0, map_cells.1
                    ));
                }
                if def.category == Category::Structure {
                    for y in o.cell.1..y1 {
                        for x in o.cell.0..x1 {
                            if !taken.insert((x, y)) {
                                return err(format!(
                                    "{at}: overlaps another structure on cell ({x}, {y})"
                                ));
                            }
                        }
                    }
                }
            }
            if construction_centres == 0 {
                return err(format!(
                    "player {} starts without a construction center: a player with none standing is \
                     defeated, so every player needs one",
                    p.id
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(role: &str, x: u32, y: u32) -> String {
        format!("(role: \"{role}\", cell: ({x}, {y}))")
    }

    /// A file whose second player starts with a construction center far from everything, and then with
    /// `second_objects` (which may be empty).
    fn file(second_objects: &str) -> String {
        let cc = object("construction_center", 60, 60);
        let objects = if second_objects.is_empty() {
            cc
        } else {
            format!("{cc}, {second_objects}")
        };
        file_with_second(&objects)
    }

    /// A file whose second player starts with exactly `second_objects`.
    fn file_with_second(second_objects: &str) -> String {
        format!(
            "(name: \"t\", map: \"m1\", seed: 1, players: [\
             (id: 0, faction: Meridian, credits: 10, objects: [{}]),\
             (id: 1, faction: Veyran, credits: 0, objects: [{second_objects}])])",
            object("construction_center", 4, 4)
        )
    }

    #[test]
    fn the_shipped_first_match_is_pinned() {
        // The literal values of assets/matches/m1.ron: a change to the file fails here, on purpose.
        let m = MatchDef::embedded();
        assert_eq!(
            (m.name.as_str(), m.map.as_str(), m.seed),
            ("m1", "m1", 458_758)
        );
        assert_eq!(m.players.len(), 2);
        let (p0, p1) = (&m.players[0], &m.players[1]);
        assert_eq!(
            (p0.id, p0.faction, p0.credits),
            (0, Faction::Meridian, 4000)
        );
        assert_eq!((p1.id, p1.faction, p1.credits), (1, Faction::Veyran, 0));
        let list = |p: &MatchPlayer| -> Vec<(String, u32, u32)> {
            p.objects
                .iter()
                .map(|o| (o.role.clone(), o.cell.0, o.cell.1))
                .collect()
        };
        assert_eq!(list(p0), vec![("construction_center".to_string(), 12, 68)]);
        assert_eq!(
            list(p1),
            vec![
                ("power_plant".to_string(), 76, 20),
                ("refinery".to_string(), 70, 26),
                ("barracks".to_string(), 80, 14),
                ("defence_tower".to_string(), 66, 22),
                ("harvester".to_string(), 74, 31),
                ("rifleman".to_string(), 64, 24),
                ("rifleman".to_string(), 64, 26),
                ("rifleman".to_string(), 66, 28),
                ("rifleman".to_string(), 68, 20),
                ("light_tank".to_string(), 62, 22),
                ("construction_center".to_string(), 72, 14),
            ]
        );
    }

    /// WP9.1d: a file that does not say who plays a player means what every file meant before.
    #[test]
    fn the_controller_defaults_to_standard_and_the_shipped_first_match_has_only_standard_players() {
        assert_eq!(Controller::default(), Controller::Standard);
        for p in &MatchDef::embedded().players {
            assert_eq!(p.controller, Controller::Standard, "player {}", p.id);
        }
        let m = MatchDef::from_ron(&file(&object("power_plant", 40, 40))).unwrap();
        assert!(
            m.players
                .iter()
                .all(|p| p.controller == Controller::Standard)
        );
        // Written out, it is the same.
        let text = file(&object("power_plant", 40, 40))
            .replace("credits: 0,", "credits: 0, controller: Standard,");
        assert_eq!(MatchDef::from_ron(&text).unwrap(), m);
    }

    #[test]
    fn the_shipped_match_with_an_ai_is_pinned() {
        // The literal values of assets/matches/m1_ai.ron.
        let m = MatchDef::embedded_ai();
        assert_eq!(
            (m.name.as_str(), m.map.as_str(), m.seed),
            ("m1_ai", "m1", 458_758)
        );
        assert_eq!(m.players.len(), 2);
        let (p0, p1) = (&m.players[0], &m.players[1]);
        assert_eq!(
            (p0.id, p0.faction, p0.credits),
            (0, Faction::Meridian, 4000)
        );
        assert_eq!((p1.id, p1.faction, p1.credits), (1, Faction::Veyran, 4000));
        assert_eq!(p0.controller, Controller::Standard);
        assert_eq!(
            p1.controller,
            Controller::Ai {
                profile: "normal".to_string()
            }
        );
        // The same start of the human as in m1, and one construction centre on the cell of m1's.
        assert_eq!(p0.objects, MatchDef::embedded().players[0].objects);
        assert_eq!(
            p1.objects,
            vec![MatchObject {
                role: "construction_center".to_string(),
                cell: (72, 14)
            }]
        );
        let map = crate::map::Map::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../assets/maps")
                .join(&m.map),
        )
        .unwrap();
        m.check(&UnitDefs::embedded(), map.size_in_cells()).unwrap();
        // The profile it names is shipped: the file under assets/ai.
        let profile =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/ai/normal.ron");
        assert!(profile.is_file());
    }

    #[test]
    fn an_ai_controller_is_read_and_its_profile_name_is_checked() {
        let ai = |profile: &str| {
            file(&object("power_plant", 40, 40)).replace(
                "credits: 0,",
                &format!("credits: 0, controller: Ai(profile: \"{profile}\"),"),
            )
        };
        let m = MatchDef::from_ron(&ai("normal")).unwrap();
        assert_eq!(
            m.players[1].controller,
            Controller::Ai {
                profile: "normal".to_string()
            }
        );
        assert_eq!(m.players[0].controller, Controller::Standard);
        // A name that could leave the directory, or is empty, is refused naming the profile.
        for bad in ["", "../x", "a/b", "n.ron", "N", "a b"] {
            let e = MatchDef::from_ron(&ai(bad)).unwrap_err().to_string();
            assert!(
                e.contains("ai profile") && e.contains("player 1"),
                "{bad:?}: {e}"
            );
        }
        // An unknown field of the controller, or a variant that is not one, is a read error.
        assert!(MatchDef::from_ron(&ai("normal").replace("profile:", "prof:")).is_err());
        let e = MatchDef::from_ron(
            &file(&object("power_plant", 40, 40))
                .replace("credits: 0,", "credits: 0, controller: Human,"),
        );
        assert!(e.is_err());
    }

    #[test]
    fn the_shipped_first_match_fits_the_shipped_definitions_and_its_map() {
        let m = MatchDef::embedded();
        let map = crate::map::Map::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../assets/maps")
                .join(&m.map),
        )
        .unwrap();
        assert_eq!(map.size_in_cells(), (96, 96));
        m.check(&UnitDefs::embedded(), map.size_in_cells()).unwrap();
    }

    #[test]
    fn a_small_file_parses_and_checks() {
        let text = file(&object("power_plant", 40, 40));
        let m = MatchDef::from_ron(&text).unwrap();
        m.check(&UnitDefs::embedded(), (96, 96)).unwrap();
    }

    #[test]
    fn the_shape_is_checked() {
        let bad = |text: String, what: &str| {
            let e = MatchDef::from_ron(&text).unwrap_err().to_string();
            assert!(e.contains(what), "{e:?} should mention {what:?}");
        };
        let ok = object("power_plant", 40, 40);
        bad(file(&ok).replace("id: 1", "id: 0"), "listed twice");
        bad(file(&ok).replace("Veyran", "Neutral"), "Neutral");
        bad(
            file(&ok).replace("credits: 0", "credits: 1000000001"),
            "above the limit",
        );
        bad(file_with_second(""), "starts with nothing");
        bad(file(&ok).replace("m1", "../x"), "map");
        bad(
            file(&ok).replace("map: \"m1\"", "map: \"a/b\""),
            "without '/'",
        );
        bad(file(&ok).replace("name: \"t\"", "name: \" \""), "name");
        bad(file(&ok).replace("seed: 1", "seed: 1, extra: 2"), "extra");
        // One player is not a match.
        bad(
            format!(
                "(name: \"t\", map: \"m1\", seed: 1, players: [(id: 0, faction: Meridian, credits: 0, objects: [{}])])",
                object("construction_center", 4, 4)
            ),
            "2 to 8",
        );
    }

    #[test]
    fn a_player_without_a_construction_center_is_a_load_error() {
        // WP7.7: a player is defeated when it owns no standing construction center, so a player that
        // starts without one could only ever lose.
        let defs = UnitDefs::embedded();
        let check = |text: String| MatchDef::from_ron(&text).unwrap().check(&defs, (96, 96));
        // The second player has a power plant and an army, but no construction center.
        let e = check(file_with_second(&format!(
            "{}, {}",
            object("power_plant", 60, 60),
            object("rifleman", 50, 50)
        )))
        .unwrap_err();
        assert!(
            e.0.contains("player 1") && e.0.contains("construction center"),
            "{e}"
        );
        // And the first.
        let text = file("").replace(
            "(role: \"construction_center\", cell: (4, 4))",
            "(role: \"rifleman\", cell: (4, 4))",
        );
        let e = check(text).unwrap_err();
        assert!(e.0.contains("player 0"), "{e}");
        // With one each it is fine, wherever in the list it stands.
        let ok = format!(
            "{}, {}",
            object("rifleman", 50, 50),
            object("construction_center", 60, 60)
        );
        assert!(check(file_with_second(&ok)).is_ok());
    }

    #[test]
    fn the_roles_are_checked_against_the_definitions() {
        let defs = UnitDefs::embedded();
        let check = |second: &str| {
            MatchDef::from_ron(&file(second))
                .unwrap()
                .check(&defs, (96, 96))
        };
        assert!(
            check(&object("nonesuch", 40, 40))
                .unwrap_err()
                .0
                .contains("no such role")
        );
        // A Meridian-only role for the Veyran player.
        assert!(
            check(&object("sniper", 40, 40))
                .unwrap_err()
                .0
                .contains("not open to the faction")
        );
        // A vessel cannot start a match, and an object (neutral) belongs to nobody.
        assert!(
            check(&object("ore_field", 40, 40))
                .unwrap_err()
                .0
                .contains("not open to the faction")
        );
        assert!(
            check(&object("gunboat", 40, 40))
                .unwrap_err()
                .0
                .contains("only structures")
        );
        // Outside the map, also by the footprint (a power plant is 2 by 3).
        assert!(
            check(&object("power_plant", 95, 10))
                .unwrap_err()
                .0
                .contains("outside")
        );
        assert!(
            check(&object("power_plant", 10, 94))
                .unwrap_err()
                .0
                .contains("outside")
        );
        assert!(check(&object("power_plant", 94, 93)).is_ok());
        // Two structures on one cell: the construction center of player 0 is 4 by 4 at (4, 4).
        assert!(
            check(&object("power_plant", 7, 6))
                .unwrap_err()
                .0
                .contains("overlaps")
        );
        assert!(check(&object("power_plant", 8, 6)).is_ok());
    }
}
