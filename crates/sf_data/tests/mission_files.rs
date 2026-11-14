//! WP10.1a: the mission format against the files that exist (every shipped and every frozen match file)
//! and against the example mission fixture.

use std::path::{Path, PathBuf};

use sf_data::map::Map;
use sf_data::match_def::MatchDef;
use sf_data::mission::{Action, Condition, Event, Goal, Goals, Lifetime, MissionDef, TeamOrder};
use sf_data::units::{BuildingFunction, UnitDefs};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn match_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(repo().join("assets/matches"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "ron"))
        .collect();
    for dir in std::fs::read_dir(repo().join("assets/scenarios")).unwrap() {
        let m = dir.unwrap().path().join("match.ron");
        if m.is_file() {
            files.push(m);
        }
    }
    files.sort();
    files
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

#[test]
fn the_files_to_compare_are_all_there() {
    // 3 shipped match files and 11 frozen copies at the time of WP10.1a; a vanished file must not make the
    // equality test below pass on nothing.
    let files = match_files();
    let frozen = files.iter().filter(|p| p.ends_with("match.ron")).count();
    assert_eq!(files.len() - frozen, 3, "assets/matches/*.ron");
    assert_eq!(frozen, 11, "assets/scenarios/*_defs/match.ron");
}

#[test]
fn every_shipped_and_frozen_match_file_loads_as_a_mission_and_equals_its_match_def() {
    for path in match_files() {
        let text = read(&path);
        let at = path.display().to_string();
        let old = MatchDef::from_ron(&text).unwrap_or_else(|e| panic!("{at}: {e}"));
        let new = MissionDef::from_ron(&text).unwrap_or_else(|e| panic!("{at}: {e}"));
        // Field by field.
        assert_eq!(new.name, old.name, "{at}: name");
        assert_eq!(new.map, old.map, "{at}: map");
        assert_eq!(new.seed, old.seed, "{at}: seed");
        assert_eq!(new.players.len(), old.players.len(), "{at}: players");
        for (n, o) in new.players.iter().zip(&old.players) {
            assert_eq!(n.id, o.id, "{at}: player id");
            assert_eq!(n.faction, o.faction, "{at}: player {} faction", o.id);
            assert_eq!(n.credits, o.credits, "{at}: player {} credits", o.id);
            assert_eq!(
                n.controller, o.controller,
                "{at}: player {} controller",
                o.id
            );
            assert_eq!(
                n.objects.len(),
                o.objects.len(),
                "{at}: player {} objects",
                o.id
            );
            for (i, (no, oo)) in n.objects.iter().zip(&o.objects).enumerate() {
                assert_eq!(no.role, oo.role, "{at}: player {} object {i} role", o.id);
                assert_eq!(no.cell, oo.cell, "{at}: player {} object {i} cell", o.id);
                assert_eq!(no.name, None, "{at}: player {} object {i} name", o.id);
            }
            // What a match file cannot say is the default.
            assert_eq!(n.goals, Goals::default(), "{at}: player {} goals", o.id);
            assert!(n.goals.is_default());
        }
        assert!(new.zones.is_empty(), "{at}: zones");
        assert!(new.variables.is_empty(), "{at}: variables");
        assert!(new.teams.is_empty(), "{at}: teams");
        assert!(new.triggers.is_empty(), "{at}: triggers");
        // And as a whole.
        assert_eq!(new.as_match_def(), old, "{at}: as_match_def");
    }
}

#[test]
fn the_shipped_match_files_pass_the_mission_check_like_the_match_check() {
    let defs = UnitDefs::embedded();
    for name in ["m1", "m1_ai", "m1_duel"] {
        let text = read(&repo().join(format!("assets/matches/{name}.ron")));
        let old = MatchDef::from_ron(&text).unwrap();
        let new = MissionDef::from_ron(&text).unwrap();
        let map = Map::load(&repo().join("assets/maps").join(&old.map)).unwrap();
        let cells = map.size_in_cells();
        old.check(&defs, cells).unwrap();
        new.check(&defs, cells)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn the_example_mission_parses_and_passes_check() {
    let text = read(&repo().join("crates/sf_data/tests/fixtures/mission_example.ron"));
    let m = MissionDef::from_ron(&text).unwrap();
    let map = Map::load(&repo().join("assets/maps").join(&m.map)).unwrap();
    m.check(&UnitDefs::embedded(), map.size_in_cells()).unwrap();

    // Literal pins of what it says.
    assert_eq!(
        (m.name.as_str(), m.map.as_str(), m.seed),
        ("example", "m1", 7)
    );
    assert_eq!(m.players[0].goals, Goals::default());
    let g = &m.players[1].goals;
    // Canonical order: sorted, whatever the file said.
    assert_eq!(
        g.defeat,
        vec![
            Goal::NoStanding {
                function: BuildingFunction::Anchor
            },
            Goal::NoStandingRoles {
                roles: vec!["barracks".to_string(), "power_plant".to_string()]
            },
        ]
    );
    assert_eq!(g.victory, vec![Goal::LastInMatch]);
    assert_eq!(m.players[0].objects[0].name.as_deref(), Some("hq"));
    assert_eq!(m.players[1].objects[1].name.as_deref(), Some("plant"));
    assert_eq!(m.zones.len(), 2);
    assert_eq!(
        (m.zones[0].name.as_str(), m.zones[0].min, m.zones[0].max),
        ("ford", (40, 40), (44, 46))
    );
    assert_eq!(
        m.variables
            .iter()
            .map(|v| (v.name.as_str(), v.start))
            .collect::<Vec<_>>(),
        vec![("alarm", 0), ("wave", 1)]
    );
    assert_eq!(m.teams.len(), 1);
    assert_eq!(
        m.teams[0].members,
        vec![("light_tank".to_string(), 2), ("rifleman".to_string(), 4)]
    );
    assert_eq!(m.teams[0].orders.len(), 5);
    assert_eq!(m.teams[0].orders[4], TeamOrder::Loop { to: 0 });
    // Triggers keep the order of the file (it is the order they are evaluated in).
    let names: Vec<&str> = m.triggers.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["first_alarm", "time_up", "give_up"]);
    assert_eq!(m.triggers[0].lifetime, Lifetime::Once);
    assert_eq!(m.triggers[1].lifetime, Lifetime::Persistent);
    assert_eq!(
        m.triggers[0].events,
        vec![
            Event::EnterZone {
                zone: "ford".to_string()
            },
            Event::Destroyed {
                object: "plant".to_string()
            }
        ]
    );
    assert_eq!(m.triggers[0].conditions.len(), 2);
    assert_eq!(
        m.triggers[0].conditions[0],
        Condition::VariableIs {
            variable: "alarm".to_string(),
            value: 0
        }
    );
    assert_eq!(
        m.triggers[2].actions,
        vec![Action::Win { player: 1 }, Action::Lose { player: 0 }]
    );
    assert!(m.triggers[2].conditions.is_empty());
    // Written out as a match file's subset, it is the match def it extends.
    let old = m.as_match_def();
    assert_eq!(old.players[1].objects.len(), 3);
}
