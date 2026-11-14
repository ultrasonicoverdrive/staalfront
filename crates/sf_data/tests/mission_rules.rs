//! WP10.1a: every validation rule of the mission format refuses its bad case and names the field, the
//! goal semantics are pinned, and the canonical order is tested. One test per rule, so a mutation of a rule
//! fails exactly its test.

use sf_data::mission::{Goal, Goals, MissionDef};
use sf_data::units::{BuildingFunction, UnitDefs};

/// A mission with two players and `extra` (the text of more top-level fields, each ending in a comma);
/// player 0 can be given `goals0`, player 1 `goals1` and `objects1` replaces player 1's objects.
fn mission(extra: &str) -> String {
    mission_with("", "", "", extra)
}

fn mission_with(goals0: &str, goals1: &str, objects0_extra: &str, extra: &str) -> String {
    format!(
        "(name: \"t\", map: \"m1\", seed: 1, players: [\
         (id: 0, faction: Meridian, credits: 10, {goals0} objects: [\
            (role: \"construction_center\", cell: (4, 4), name: Some(\"hq\")){objects0_extra}]),\
         (id: 1, faction: Veyran, credits: 0, {goals1} objects: [\
            (role: \"construction_center\", cell: (60, 60)),\
            (role: \"power_plant\", cell: (70, 60), name: Some(\"plant\"))])], {extra})"
    )
}

fn refused(text: &str) -> String {
    match MissionDef::from_ron(text) {
        Ok(_) => panic!("accepted:\n{text}"),
        Err(e) => e.to_string(),
    }
}

fn refused_with(text: &str, parts: &[&str]) {
    let e = refused(text);
    for p in parts {
        assert!(e.contains(p), "error should contain {p:?}: {e}");
    }
}

const ZONE: &str = "zones: [(name: \"z\", min: (1, 1), max: (3, 3))],";
const VAR: &str = "variables: [(name: \"v\", start: 0)],";

fn trigger(parts: &str) -> String {
    format!("triggers: [(name: \"tr\", owner: 0, lifetime: Once, {parts})],")
}

#[test]
fn a_bare_match_file_gets_the_default_goals_and_nothing_else() {
    let m = MissionDef::from_ron(&mission("")).unwrap();
    for p in &m.players {
        assert_eq!(p.goals, Goals::default());
        assert_eq!(
            p.goals.defeat,
            vec![Goal::NoStanding {
                function: BuildingFunction::Anchor
            }]
        );
        assert_eq!(p.goals.victory, vec![Goal::LastInMatch]);
    }
    assert!(
        m.zones.is_empty() && m.variables.is_empty() && m.teams.is_empty() && m.triggers.is_empty()
    );
}

#[test]
fn one_goal_list_left_out_takes_its_own_default_only() {
    let m =
        MissionDef::from_ron(&mission_with("goals: (defeat: [NothingLeft]),", "", "", "")).unwrap();
    assert_eq!(m.players[0].goals.defeat, vec![Goal::NothingLeft]);
    assert_eq!(m.players[0].goals.victory, vec![Goal::LastInMatch]);
}

#[test]
fn defeat_is_any_and_victory_is_all() {
    let goals = Goals {
        defeat: vec![
            Goal::NothingLeft,
            Goal::NoStanding {
                function: BuildingFunction::Radar,
            },
        ],
        victory: vec![Goal::LastInMatch, Goal::NothingLeft],
    };
    // Defeat: any.
    assert!(goals.is_defeated(|g| *g == Goal::NothingLeft));
    assert!(goals.is_defeated(|g| matches!(g, Goal::NoStanding { .. })));
    assert!(!goals.is_defeated(|_| false));
    // Victory: all.
    assert!(goals.has_won(|_| true));
    assert!(!goals.has_won(|g| *g == Goal::LastInMatch));
    assert!(!goals.has_won(|g| *g == Goal::NothingLeft));
    // An empty list: no defeat; and no victory (never vacuously true).
    let none = Goals {
        defeat: vec![],
        victory: vec![],
    };
    assert!(!none.is_defeated(|_| true));
    assert!(!none.has_won(|_| true));
}

#[test]
fn goal_lists_are_sorted_and_without_doubles_and_triggers_keep_their_order() {
    let a = "goals: (defeat: [NothingLeft, NoStandingRoles(roles: [\"power_plant\", \"barracks\", \"barracks\"]), NoStanding(function: Anchor), NothingLeft]),";
    let b = "goals: (defeat: [NoStanding(function: Anchor), NoStandingRoles(roles: [\"barracks\", \"power_plant\"]), NothingLeft]),";
    let (ma, mb) = (
        MissionDef::from_ron(&mission_with("", a, "", "")).unwrap(),
        MissionDef::from_ron(&mission_with("", b, "", "")).unwrap(),
    );
    assert_eq!(
        ma.players[1].goals, mb.players[1].goals,
        "the order of the file means nothing"
    );
    assert_eq!(
        ma.players[1].goals.defeat,
        vec![
            Goal::NoStanding {
                function: BuildingFunction::Anchor
            },
            Goal::NoStandingRoles {
                roles: vec!["barracks".to_string(), "power_plant".to_string()]
            },
            Goal::NothingLeft,
        ]
    );
    // Triggers are evaluated in file order and keep it.
    let two = |first: &str, second: &str| {
        let t = |n: &str| {
            format!(
                "(name: \"{n}\", owner: 0, lifetime: Once, events: [Always], actions: [Lose(player: 0)])"
            )
        };
        MissionDef::from_ron(&mission(&format!(
            "triggers: [{}, {}],",
            t(first),
            t(second)
        )))
        .unwrap()
    };
    let names = |m: &MissionDef| {
        m.triggers
            .iter()
            .map(|t| t.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&two("b", "a")), ["b", "a"]);
    assert_eq!(names(&two("a", "b")), ["a", "b"]);
}

#[test]
fn last_in_match_is_a_victory_goal_only() {
    refused_with(
        &mission_with("goals: (defeat: [LastInMatch]),", "", "", ""),
        &["goals.defeat", "LastInMatch", "player 0"],
    );
    MissionDef::from_ron(&mission_with(
        "goals: (victory: [LastInMatch, NothingLeft]),",
        "",
        "",
        "",
    ))
    .unwrap();
}

#[test]
fn a_goal_with_empty_or_badly_named_roles_is_refused() {
    refused_with(
        &mission_with("goals: (defeat: [NoStandingRoles(roles: [])]),", "", "", ""),
        &["goals.defeat", "NoStandingRoles needs roles"],
    );
    refused_with(
        &mission_with(
            "goals: (defeat: [NoStandingRoles(roles: [\"Power Plant\"])]),",
            "",
            "",
            "",
        ),
        &["goals.defeat", "role", "Power Plant"],
    );
}

#[test]
fn an_unknown_role_is_refused_by_check_naming_the_field() {
    let defs = UnitDefs::embedded();
    let cells = (384, 384);
    // In a goal.
    let m = MissionDef::from_ron(&mission_with(
        "goals: (defeat: [NoStandingRoles(roles: [\"warp_gate\"])]),",
        "",
        "",
        "",
    ))
    .unwrap();
    let e = m.check(&defs, cells).unwrap_err().to_string();
    assert!(
        e.contains("goals") && e.contains("warp_gate") && e.contains("player 0"),
        "{e}"
    );
    // A goal naming a unit and not a structure.
    let m = MissionDef::from_ron(&mission_with(
        "goals: (defeat: [NoStandingRoles(roles: [\"rifleman\"])]),",
        "",
        "",
        "",
    ))
    .unwrap();
    let e = m.check(&defs, cells).unwrap_err().to_string();
    assert!(
        e.contains("rifleman") && e.contains("not a structure"),
        "{e}"
    );
    // In a team.
    let m = MissionDef::from_ron(&mission(
        "teams: [(name: \"t1\", owner: 1, members: [(\"warp_gate\", 1)])],",
    ))
    .unwrap();
    let e = m.check(&defs, cells).unwrap_err().to_string();
    assert!(
        e.contains("teams[0]") && e.contains("members[0]") && e.contains("warp_gate"),
        "{e}"
    );
    // A structure in a team.
    let m = MissionDef::from_ron(&mission(
        "teams: [(name: \"t1\", owner: 1, members: [(\"barracks\", 1)])],",
    ))
    .unwrap();
    let e = m.check(&defs, cells).unwrap_err().to_string();
    assert!(
        e.contains("members[0]") && e.contains("not infantry or a vehicle"),
        "{e}"
    );
    // In a Built event.
    let m = MissionDef::from_ron(&mission(&trigger(
        "events: [Built(player: 1, role: \"warp_gate\")], actions: [Lose(player: 0)]",
    )))
    .unwrap();
    let e = m.check(&defs, cells).unwrap_err().to_string();
    assert!(e.contains("events[0]") && e.contains("warp_gate"), "{e}");
    // A start object.
    let text = mission("").replace("\"power_plant\"", "\"warp_gate\"");
    let e = MissionDef::from_ron(&text)
        .unwrap()
        .check(&defs, cells)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("player 1 object 1") && e.contains("warp_gate"),
        "{e}"
    );
}

#[test]
fn a_player_must_start_with_what_its_defeat_goal_asks_for() {
    let defs = UnitDefs::embedded();
    let cells = (384, 384);
    // Default goals and no construction center: refused, as the match file does.
    let text = mission("").replace("(role: \"construction_center\", cell: (60, 60)),", "");
    let e = MissionDef::from_ron(&text)
        .unwrap()
        .check(&defs, cells)
        .unwrap_err()
        .to_string();
    assert!(e.contains("player 1") && e.contains("construction"), "{e}");
    // The same player with a goal on its power plant instead: accepted.
    let ok = text.replace(
        "id: 1, faction: Veyran, credits: 0,",
        "id: 1, faction: Veyran, credits: 0, goals: (defeat: [NoStandingRoles(roles: [\"power_plant\"])]),",
    );
    MissionDef::from_ron(&ok)
        .unwrap()
        .check(&defs, cells)
        .unwrap();
    // ... and refused if it does not start with a power plant either.
    let bad = ok.replace("(role: \"power_plant\"", "(role: \"barracks\"");
    let e = MissionDef::from_ron(&bad)
        .unwrap()
        .check(&defs, cells)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("player 1") && e.contains("NoStandingRoles"),
        "{e}"
    );
    // A player with no defeat goal at all needs no structure of a function.
    let free = text.replace(
        "id: 1, faction: Veyran, credits: 0,",
        "id: 1, faction: Veyran, credits: 0, goals: (defeat: []),",
    );
    MissionDef::from_ron(&free)
        .unwrap()
        .check(&defs, cells)
        .unwrap();
}

#[test]
fn a_zone_outside_the_map_is_refused_by_check() {
    let m = MissionDef::from_ron(&mission(
        "zones: [(name: \"z\", min: (1, 1), max: (400, 3))],",
    ))
    .unwrap();
    let e = m
        .check(&UnitDefs::embedded(), (384, 384))
        .unwrap_err()
        .to_string();
    assert!(e.contains("zones[0]") && e.contains("outside"), "{e}");
}

#[test]
fn a_dangling_zone_reference_is_refused() {
    refused_with(
        &mission(&trigger(
            "events: [EnterZone(zone: \"nowhere\")], actions: [Lose(player: 0)]",
        )),
        &["triggers[0]", "events[0]", "zone \"nowhere\""],
    );
    refused_with(
        &mission(&format!(
            "{ZONE} {}",
            trigger("events: [Always], actions: [CameraTo(zone: \"nowhere\")]")
        )),
        &["actions[0]", "zone \"nowhere\""],
    );
    refused_with(
        &mission(&format!(
            "{ZONE} teams: [(name: \"t1\", owner: 1, members: [(\"rifleman\", 1)], orders: [MoveTo(zone: \"nowhere\")])],"
        )),
        &["teams[0]", "orders[0]", "zone \"nowhere\""],
    );
}

#[test]
fn a_dangling_team_reference_is_refused() {
    refused_with(
        &mission(&format!(
            "{ZONE} {}",
            trigger("events: [Always], actions: [SpawnTeam(team: \"ghost\", zone: \"z\")]")
        )),
        &["actions[0]", "team \"ghost\""],
    );
    refused_with(
        &mission(&trigger(
            "events: [Always], conditions: [TeamAlive(team: \"ghost\")], actions: [Lose(player: 0)]",
        )),
        &["conditions[0]", "team \"ghost\""],
    );
}

#[test]
fn a_dangling_variable_reference_is_refused() {
    refused_with(
        &mission(&trigger(
            "events: [Always], actions: [SetVariable(variable: \"nope\", value: 1)]",
        )),
        &["actions[0]", "variable \"nope\""],
    );
    refused_with(
        &mission(&trigger(
            "events: [Always], conditions: [VariableIs(variable: \"nope\", value: 1)], actions: [Lose(player: 0)]",
        )),
        &["conditions[0]", "variable \"nope\""],
    );
    refused_with(
        &mission(&format!(
            "{ZONE} teams: [(name: \"t1\", owner: 1, members: [(\"rifleman\", 1)], orders: [SetVariable(variable: \"nope\", value: 1)])],"
        )),
        &["orders[0]", "variable \"nope\""],
    );
}

#[test]
fn a_dangling_object_reference_is_refused() {
    refused_with(
        &mission(&trigger(
            "events: [Destroyed(object: \"missing\")], actions: [Lose(player: 0)]",
        )),
        &["events[0]", "object \"missing\""],
    );
    refused_with(
        &mission(&trigger(
            "events: [Always], actions: [Destroy(object: \"missing\")]",
        )),
        &["actions[0]", "object \"missing\""],
    );
    // A name that exists is fine.
    MissionDef::from_ron(&mission(&trigger(
        "events: [Destroyed(object: \"plant\")], actions: [Lose(player: 0)]",
    )))
    .unwrap();
}

#[test]
fn a_dangling_trigger_reference_is_refused() {
    refused_with(
        &mission(&trigger(
            "events: [Always], actions: [ForceTrigger(trigger: \"other\")]",
        )),
        &["actions[0]", "trigger \"other\""],
    );
}

#[test]
fn a_dangling_player_reference_is_refused() {
    refused_with(
        &mission(&trigger("events: [Always], actions: [Win(player: 5)]")),
        &["actions[0]", "player 5"],
    );
    refused_with(
        &mission(&trigger(
            "events: [AllDestroyed(player: 5)], actions: [Lose(player: 0)]",
        )),
        &["events[0]", "player 5"],
    );
    refused_with(
        &mission(
            "triggers: [(name: \"tr\", owner: 9, lifetime: Once, events: [Always], actions: [Lose(player: 0)])],",
        ),
        &["triggers[0]", "owner 9"],
    );
    refused_with(
        &mission("teams: [(name: \"t1\", owner: 9, members: [(\"rifleman\", 1)])],"),
        &["teams[0]", "owner 9"],
    );
}

#[test]
fn duplicate_names_are_refused() {
    refused_with(
        &mission(
            "zones: [(name: \"z\", min: (1, 1), max: (2, 2)), (name: \"z\", min: (3, 3), max: (4, 4))],",
        ),
        &["zones[1]", "name \"z\"", "twice"],
    );
    refused_with(
        &mission("variables: [(name: \"v\"), (name: \"v\")],"),
        &["variables[1]", "name \"v\"", "twice"],
    );
    refused_with(
        &mission(
            "teams: [(name: \"t1\", owner: 1, members: [(\"rifleman\", 1)]), (name: \"t1\", owner: 1, members: [(\"rifleman\", 1)])],",
        ),
        &["teams[1]", "name \"t1\"", "twice"],
    );
    let t =
        "(name: \"tr\", owner: 0, lifetime: Once, events: [Always], actions: [Lose(player: 0)])";
    refused_with(
        &mission(&format!("triggers: [{t}, {t}],")),
        &["triggers[1]", "name \"tr\"", "twice"],
    );
    // Object names are unique across players.
    let text = mission("").replace("Some(\"plant\")", "Some(\"hq\")");
    refused_with(&text, &["player 1 object 1", "name \"hq\"", "twice"]);
    // A player id twice.
    refused_with(
        &mission("").replace("id: 1,", "id: 0,"),
        &["player 0", "twice"],
    );
}

#[test]
fn a_name_must_be_a_plain_name() {
    for bad in ["", "Z", "a b", "a-b", &"x".repeat(33)] {
        refused_with(
            &mission(&format!(
                "zones: [(name: \"{bad}\", min: (1, 1), max: (2, 2))],"
            )),
            &["zones[0]", "name"],
        );
    }
}

#[test]
fn ranges_outside_their_bounds_are_refused() {
    // A zone with min above max.
    refused_with(
        &mission("zones: [(name: \"z\", min: (5, 1), max: (2, 2))],"),
        &["zones[0]", "min", "max"],
    );
    // A variable start.
    refused_with(
        &mission("variables: [(name: \"v\", start: 1000001)],"),
        &["variables[0]", "start 1000001"],
    );
    // Ticks of a time event: 0 and above the limit.
    for t in ["0", "1000001"] {
        refused_with(
            &mission(&trigger(&format!(
                "events: [Time(ticks: {t})], actions: [Lose(player: 0)]"
            ))),
            &["events[0]", "ticks"],
        );
    }
    // Credits above the limit.
    refused_with(
        &mission(&trigger(
            "events: [MoneyAbove(player: 0, credits: 1000000001)], actions: [Lose(player: 0)]",
        )),
        &["events[0]", "credits"],
    );
    refused_with(
        &mission(&trigger(
            "events: [Always], conditions: [MoneyAtLeast(player: 0, credits: 1000000001)], actions: [Lose(player: 0)]",
        )),
        &["conditions[0]", "credits"],
    );
    // A value and a delta.
    refused_with(
        &mission(&format!(
            "{VAR} {}",
            trigger("events: [Always], actions: [SetVariable(variable: \"v\", value: 1000001)]")
        )),
        &["actions[0]", "value 1000001"],
    );
    refused_with(
        &mission(&format!(
            "{VAR} {}",
            trigger("events: [Always], actions: [AddVariable(variable: \"v\", delta: -1000001)]")
        )),
        &["actions[0]", "delta -1000001"],
    );
    // A message: empty and too long.
    for text in [String::new(), "x".repeat(201)] {
        refused_with(
            &mission(&trigger(&format!(
                "events: [Always], actions: [Message(text: \"{text}\")]"
            ))),
            &["actions[0]", "text length"],
        );
    }
    // A team: a count of 0 and of 65, no members, a guard of 0 ticks, a loop forward.
    refused_with(
        &mission("teams: [(name: \"t1\", owner: 1, members: [(\"rifleman\", 0)])],"),
        &["teams[0]", "members[0]", "count 0"],
    );
    refused_with(
        &mission("teams: [(name: \"t1\", owner: 1, members: [(\"rifleman\", 65)])],"),
        &["members[0]", "count 65"],
    );
    refused_with(
        &mission("teams: [(name: \"t1\", owner: 1, members: [])],"),
        &["teams[0]", "members"],
    );
    refused_with(
        &mission(
            "teams: [(name: \"t1\", owner: 1, members: [(\"rifleman\", 1)], orders: [Guard(ticks: 0)])],",
        ),
        &["orders[0]", "ticks 0"],
    );
    refused_with(
        &mission(
            "teams: [(name: \"t1\", owner: 1, members: [(\"rifleman\", 1)], orders: [Loop(to: 1)])],",
        ),
        &["orders[0]", "to 1"],
    );
    // A trigger without events or without actions.
    refused_with(
        &mission(&trigger("events: [], actions: [Lose(player: 0)]")),
        &["triggers[0]", "events has 0"],
    );
    refused_with(
        &mission(&trigger("events: [Always], actions: []")),
        &["triggers[0]", "actions has 0"],
    );
    // Too many zones.
    let zones: String = (0..65)
        .map(|i| format!("(name: \"z{i}\", min: (1, 1), max: (2, 2)),"))
        .collect();
    refused_with(&mission(&format!("zones: [{zones}],")), &["zones has 65"]);
}

#[test]
fn the_edges_of_every_range_are_accepted() {
    let t = trigger(
        "events: [Time(ticks: 1), Time(ticks: 1000000), MoneyAbove(player: 0, credits: 1000000000)], \
                     conditions: [VariableAtLeast(variable: \"v\", value: -1000000)], \
                     actions: [SetVariable(variable: \"v\", value: 1000000), AddVariable(variable: \"v\", delta: -1000000)]",
    );
    MissionDef::from_ron(&mission(&format!(
        "variables: [(name: \"v\", start: -1000000)], \
         teams: [(name: \"t1\", owner: 1, members: [(\"rifleman\", 1), (\"rifleman\", 64)], orders: [Guard(ticks: 1000000), Loop(to: 1)])], {t}"
    )))
    .unwrap();
}

#[test]
fn unknown_fields_and_the_match_file_checks_still_apply() {
    assert!(refused(&mission("flavour: 3,")).contains("flavour"));
    assert!(refused(&mission("").replace("seed: 1", "seed: 1, sneed: 2")).contains("sneed"));
    refused_with(
        &mission("").replace("map: \"m1\"", "map: \"a/b\""),
        &["map"],
    );
    refused_with(
        &mission("").replace("name: \"t\"", "name: \" \""),
        &["name"],
    );
    refused_with(
        &mission("").replace("faction: Veyran", "faction: Neutral"),
        &["player 1", "Neutral"],
    );
    refused_with(
        &mission("").replace("credits: 0", "credits: 2000000000"),
        &["player 1", "credits"],
    );
    refused_with(
        &mission("").replace(
            "id: 1, faction: Veyran, credits: 0,",
            "id: 1, faction: Veyran, credits: 0, controller: Ai(profile: \"../x\"),",
        ),
        &["player 1", "ai profile"],
    );
}

#[test]
fn unknown_fields_inside_a_variant_are_refused() {
    let cases: [(String, &str); 5] = [
        (
            mission_with(
                "goals: (defeat: [NoStanding(function: Anchor, roles: [\"power_plant\"])]),",
                "",
                "",
                "",
            ),
            "roles",
        ),
        (
            mission(&format!(
                "{ZONE} teams: [(name: \"t1\", owner: 1, members: [(\"rifleman\", 1)], orders: [MoveTo(zone: \"z\", speed: 2)])],"
            )),
            "speed",
        ),
        (
            mission(&trigger(
                "events: [Time(ticks: 5, tocks: 9)], actions: [Lose(player: 0)]",
            )),
            "tocks",
        ),
        (
            mission(&format!(
                "{VAR} {}",
                trigger(
                    "events: [Always], conditions: [VariableIs(variable: \"v\", value: 1, extra: 2)], actions: [Lose(player: 0)]"
                )
            )),
            "extra",
        ),
        (
            mission(&trigger(
                "events: [Always], actions: [Win(player: 0, zone: \"nope\")]",
            )),
            "zone",
        ),
    ];
    for (text, field) in cases {
        let e = refused(&text);
        assert!(e.contains(field), "error should name {field:?}: {e}");
    }
}

#[test]
fn a_start_cell_at_u32_max_is_refused_as_outside_the_map() {
    for cell in ["(4294967295, 60)", "(60, 4294967295)"] {
        let m = MissionDef::from_ron(&mission("").replace("(60, 60)", cell)).unwrap();
        let e = m
            .check(&UnitDefs::embedded(), (384, 384))
            .unwrap_err()
            .to_string();
        assert!(e.contains("lies outside the 384x384 map"), "{e}");
    }
}
