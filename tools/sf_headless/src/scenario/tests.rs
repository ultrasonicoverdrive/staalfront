use super::*;
use sf_sim::PlayerId;

fn parse(text: &str) -> Result<Scenario, ScenarioError> {
    Scenario::from_ron_str(text)
}

/// The problems of a scenario that parses but cannot run.
fn problems(text: &str) -> Vec<String> {
    match parse(text) {
        Err(ScenarioError::Invalid { problems, .. }) => problems,
        other => panic!("expected an invalid scenario, got {other:?}"),
    }
}

fn syntax_error(text: &str) -> String {
    match parse(text) {
        Err(e @ ScenarioError::Syntax { .. }) => e.to_string(),
        other => panic!("expected a syntax error, got {other:?}"),
    }
}

fn has(problems: &[String], needle: &str) -> bool {
    problems.iter().any(|p| p.contains(needle))
}

const GROUP: &str =
    "(count: 4, kinds: [0, 1], owners: [0, 1, 2], area: (min: (0, 0), max: (8, 8)))";

fn with_spawns(spawns: &str) -> String {
    format!("Scenario(seed: 1, map: (width: 16, height: 16), ticks: 10, spawns: [{spawns}])")
}

fn with_command(command: &str) -> String {
    format!(
        "Scenario(seed: 1, map: (width: 16, height: 16), ticks: 10, spawns: [{GROUP}], \
         commands: [{command}])"
    )
}

#[test]
fn the_smallest_scenario_gets_defaults() {
    let s = parse("Scenario(seed: 7, ticks: 3)").unwrap();
    assert_eq!(s.seed, 7);
    assert_eq!(s.ticks, 3);
    assert_eq!(s.map, MapSize::default());
    assert_eq!((s.map.width, s.map.height), (256, 256));
    assert!(s.spawns.is_empty() && s.commands.is_empty());
    assert_eq!(s.display_name(), "unnamed");
}

#[test]
fn numbers_may_be_hex_with_underscores() {
    let s = parse("Scenario(seed: 0x57A1_F407, ticks: 10_000)").unwrap();
    assert_eq!(s.seed, 0x57A1_F407);
    assert_eq!(s.ticks, 10_000);
}

#[test]
fn spawn_and_command_defaults() {
    let s = parse(&with_command(
        "(tick: 3, command: Despawn(ids: [Id(0, 0)]))",
    ))
    .unwrap();
    assert_eq!((s.spawns[0].tick, s.spawns[0].player), (0, 0));
    assert_eq!(s.commands[0].player, 0);
}

#[test]
fn the_shipped_scenarios_are_valid() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenarios");
    for name in ["wander_5000", "command_mix", "terrain_256", "units_spawn"] {
        let s = Scenario::load(&dir.join(format!("{name}.ron"))).unwrap();
        assert_eq!(s.name, name);
    }
}

// ---- errors ----------------------------------------------------------------------

#[test]
fn unknown_fields_are_errors_that_name_the_field() {
    let top = syntax_error("Scenario(seed: 1, ticks: 10, tikcs: 5)");
    assert!(top.contains("tikcs"), "{top}");
    let group = syntax_error(
        "Scenario(seed: 1, ticks: 10, spawns: [(count: 1, kinds: [0], owners: [0], \
         area: (min: (0, 0), max: (1, 1)), speed: 3)])",
    );
    assert!(group.contains("speed"), "{group}");
    let map = syntax_error("Scenario(seed: 1, ticks: 10, map: (width: 8, depth: 8))");
    assert!(map.contains("depth"), "{map}");
    let area = syntax_error(&with_command(
        "(tick: 1, command: SetWanderArea(ids: [Id(0, 0)], area: (min: (0, 0), max: (2, 2)), \
         speed: 3))",
    ));
    assert!(area.contains("speed"), "{area}");
}

#[test]
fn unknown_commands_and_missing_fields_are_syntax_errors() {
    let e = syntax_error(&with_command(
        "(tick: 1, command: Teleport(ids: [Id(0, 0)]))",
    ));
    assert!(e.contains("Teleport"), "{e}");
    let e = syntax_error("Scenario(seed: 1)");
    assert!(e.contains("ticks"), "{e}");
    let e = syntax_error("Scenario(seed: 1, ticks: -4)");
    assert!(e.contains("1:"), "{e}");
}

#[test]
fn syntax_errors_carry_line_and_column() {
    let e = syntax_error("Scenario(\n    seed: 1,\n    ticks: oops,\n)");
    assert!(e.starts_with("invalid scenario: 3:"), "{e}");
}

#[test]
fn a_file_name_leads_the_message() {
    let e = ScenarioError::Syntax {
        path: Some("a/b.ron".into()),
        message: "1:1: boom".into(),
    };
    assert_eq!(e.to_string(), "a/b.ron: invalid scenario: 1:1: boom");
}

#[test]
fn zero_ticks_is_rejected() {
    let p = problems("Scenario(seed: 1, ticks: 0)");
    assert_eq!(p, vec!["ticks must be at least 1".to_string()]);
}

#[test]
fn a_command_after_the_last_tick_is_rejected() {
    // ticks: 10 means ticks 0..=9 run.
    let p = problems(&with_command(
        "(tick: 10, command: Despawn(ids: [Id(0, 0)]))",
    ));
    assert_eq!(p.len(), 1, "{p:?}");
    assert!(
        p[0].contains("commands[0]") && p[0].contains("tick 10"),
        "{p:?}"
    );
    assert!(p[0].contains("last tick (9)"), "{p:?}");
    // The last tick itself is fine.
    parse(&with_command(
        "(tick: 9, command: Despawn(ids: [Id(0, 0)]))",
    ))
    .unwrap();
}

#[test]
fn a_spawn_after_the_last_tick_is_rejected() {
    let p = problems(&with_spawns(
        "(tick: 40, count: 1, kinds: [0], owners: [0], area: (min: (0, 0), max: (1, 1)))",
    ));
    assert!(has(&p, "spawns[0]") && has(&p, "never run"), "{p:?}");
}

#[test]
fn spawn_groups_need_a_count_kinds_owners_and_a_real_area() {
    let p = problems(&with_spawns(
        "(count: 0, kinds: [], owners: [], area: (min: (4, 4), max: (4, 9)))",
    ));
    assert!(has(&p, "count must be at least 1"), "{p:?}");
    assert!(has(&p, "kinds must not be empty"), "{p:?}");
    assert!(has(&p, "owners must not be empty"), "{p:?}");
    assert!(has(&p, "empty or reversed"), "{p:?}");
    let reversed = problems(&with_spawns(
        "(count: 1, kinds: [0], owners: [0], area: (min: (9, 0), max: (2, 5)))",
    ));
    assert!(has(&reversed, "empty or reversed"), "{reversed:?}");
}

#[test]
fn areas_and_targets_must_lie_inside_the_map() {
    let p = problems(&with_spawns(
        "(count: 1, kinds: [0], owners: [0], area: (min: (0, 0), max: (17, 8)))",
    ));
    assert!(has(&p, "outside the 16x16 map"), "{p:?}");
    let p = problems(&with_spawns(
        "(count: 1, kinds: [0], owners: [0], area: (min: (-1, 0), max: (8, 8)))",
    ));
    assert!(has(&p, "outside the 16x16 map"), "{p:?}");
    let p = problems(&with_command(
        "(tick: 1, command: SetWanderArea(ids: [Id(0, 0)], area: (min: (0, 0), max: (16, 17))))",
    ));
    assert!(has(&p, "outside the 16x16 map"), "{p:?}");
    let p = problems(&with_command(
        "(tick: 1, command: MoveTo(ids: [Id(0, 0)], target: (16, 3)))",
    ));
    assert!(has(&p, "target (16, 3) lies outside"), "{p:?}");
    // The whole map and its last cell are fine.
    parse(&with_command(
        "(tick: 1, command: MoveTo(ids: [Id(0, 0)], target: (15, 15)))",
    ))
    .unwrap();
    parse(&with_command(
        "(tick: 1, command: SetWanderArea(ids: [Id(0, 0)], area: (min: (0, 0), max: (16, 16))))",
    ))
    .unwrap();
}

#[test]
fn the_map_size_is_checked() {
    let p = problems("Scenario(seed: 1, ticks: 1, map: (width: 0, height: 5))");
    assert!(has(&p, "between 1 and 1024"), "{p:?}");
    let p = problems("Scenario(seed: 1, ticks: 1, map: (width: 2000, height: 5))");
    assert!(has(&p, "between 1 and 1024"), "{p:?}");
    // Width alone is enough; the other side keeps its default.
    let s = parse("Scenario(seed: 1, ticks: 1, map: (width: 64))").unwrap();
    assert_eq!((s.map.width, s.map.height), (64, 256));
}

#[test]
fn ids_are_checked() {
    let p = problems(&with_command("(tick: 1, command: Despawn(ids: []))"));
    assert!(has(&p, "ids must not be empty"), "{p:?}");
    let p = problems(&with_command(
        "(tick: 1, command: Despawn(ids: [Range(5, 5)]))",
    ));
    assert!(has(&p, "Range(5, 5) is empty"), "{p:?}");
    let p = problems(&with_command(
        "(tick: 1, command: Despawn(ids: [Range(9, 3)]))",
    ));
    assert!(has(&p, "Range(9, 3) is empty"), "{p:?}");
    let p = problems(&with_command(
        "(tick: 1, command: Despawn(ids: [Range(0, 5000000)]))",
    ));
    assert!(has(&p, "past the limit"), "{p:?}");
    let p = problems(&with_command(
        "(tick: 1, command: Despawn(ids: [Id(4000000, 0)]))",
    ));
    assert!(has(&p, "not a valid slot"), "{p:?}");
}

#[test]
fn too_many_entities_are_rejected() {
    let p = problems(&with_spawns(
        "(count: 1000000, kinds: [0], owners: [0], area: (min: (0, 0), max: (4, 4))), \
         (count: 1000000, kinds: [0], owners: [0], area: (min: (0, 0), max: (4, 4)))",
    ));
    assert!(has(&p, "2000000 entities in total"), "{p:?}");
}

#[test]
fn every_problem_is_reported_at_once() {
    let p = problems(
        "Scenario(seed: 1, ticks: 5, map: (width: 8, height: 8), spawns: [\
         (count: 0, kinds: [0], owners: [0], area: (min: (0, 0), max: (9, 9)))], \
         commands: [(tick: 5, command: Despawn(ids: []))])",
    );
    assert!(p.len() >= 4, "{p:?}");
    let text = ScenarioError::Invalid {
        path: Some("x.ron".into()),
        problems: p,
    }
    .to_string();
    assert!(text.starts_with("x.ron: invalid scenario:\n  - "), "{text}");
}

#[test]
fn build_world_validates_a_scenario_edited_in_memory() {
    let mut s = parse("Scenario(seed: 1, ticks: 4)").unwrap();
    s.ticks = 0;
    assert!(matches!(
        s.build_world(),
        Err(ScenarioError::Invalid { .. })
    ));
}

// ---- loading ---------------------------------------------------------------------

#[test]
fn load_names_the_scenario_after_the_file() {
    let dir = std::env::temp_dir().join(format!("sf_headless_unit_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("my_run.ron");
    std::fs::write(&path, "Scenario(seed: 1, ticks: 2)").unwrap();
    let s = Scenario::load(&path).unwrap();
    assert_eq!(s.name, "my_run");
    // An explicit name wins.
    std::fs::write(&path, "Scenario(name: \"given\", seed: 1, ticks: 2)").unwrap();
    assert_eq!(Scenario::load(&path).unwrap().name, "given");
    // Errors name the file.
    std::fs::write(&path, "Scenario(seed: 1, ticks: 0)").unwrap();
    let e = Scenario::load(&path).unwrap_err().to_string();
    assert!(
        e.contains("my_run.ron") && e.contains("ticks must be at least 1"),
        "{e}"
    );
    let missing = Scenario::load(&dir.join("nope.ron")).unwrap_err();
    assert!(matches!(missing, ScenarioError::Io { .. }));
    assert!(missing.to_string().contains("nope.ron"), "{missing}");
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---- building the world ------------------------------------------------------------

#[test]
fn spawn_groups_cycle_kinds_and_owners_and_stay_in_their_area() {
    let s = parse(
        "Scenario(seed: 5, map: (width: 32, height: 32), ticks: 1, spawns: [\
         (count: 7, kinds: [10, 20, 30], owners: [1, 2], area: (min: (4, 8), max: (12, 20)))])",
    )
    .unwrap();
    let mut world = s.build_world().unwrap();
    assert_eq!(world.pending_commands().len(), 7);
    assert_eq!(world.len(), 0);
    world.step();
    assert_eq!(world.len(), 7);
    for i in 0..7u32 {
        let e = world.get(EntityId::new(i, 0)).unwrap();
        assert_eq!(e.kind, Kind([10, 20, 30][i as usize % 3]));
        assert_eq!(e.owner, PlayerId([1, 2][i as usize % 2]));
        // The entity has already taken one step from its spawn point (at most
        // a cell), so allow that much slack around the area.
        let slack = Fx::ONE;
        assert!(e.pos.x >= Fx::from_int(4) - slack && e.pos.x <= Fx::from_int(12) + slack);
        assert!(e.pos.y >= Fx::from_int(8) - slack && e.pos.y <= Fx::from_int(20) + slack);
    }
}

#[test]
fn spawn_positions_are_drawn_before_stepping_from_the_placement_stream() {
    // Check the positions exactly, in a world where nothing has stepped: the
    // pending Spawn commands carry them.
    let s = parse(
        "Scenario(seed: 9, map: (width: 64, height: 64), ticks: 1, spawns: [\
         (count: 3, kinds: [0], owners: [0], area: (min: (10, 20), max: (30, 40))), \
         (count: 2, kinds: [0], owners: [0], area: (min: (0, 0), max: (64, 64)))])",
    )
    .unwrap();
    let world = s.build_world().unwrap();
    let mut rng = Rng::with_stream(9, PLACEMENT_STREAM);
    let mut expected = Vec::new();
    for (x0, y0, x1, y1, n) in [(10, 20, 30, 40, 3), (0, 0, 64, 64, 2)] {
        for _ in 0..n {
            let x = rng.fx_range(Fx::from_int(x0), Fx::from_int(x1));
            let y = rng.fx_range(Fx::from_int(y0), Fx::from_int(y1));
            expected.push(Vec2::new(x, y));
        }
    }
    let spawned: Vec<Vec2> = world
        .pending_commands()
        .iter()
        .map(|(_, _, c)| match c {
            Command::Spawn { pos, .. } => *pos,
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(spawned, expected);
}

#[test]
fn commands_become_sim_commands_in_file_order() {
    let s = parse(&with_command(
        "(tick: 2, player: 1, command: Despawn(ids: [Id(0, 0), Range(2, 4)]))",
    ))
    .unwrap();
    let world = s.build_world().unwrap();
    // 4 spawns, then one Despawn per id: 1 + 2.
    assert_eq!(world.pending_commands().len(), 4 + 3);
    let despawned: Vec<(u64, u8, u32)> = world
        .pending_commands()
        .iter()
        .filter_map(|(t, p, c)| match c {
            Command::Despawn { id } => Some((t.0, p.0, id.index())),
            _ => None,
        })
        .collect();
    assert_eq!(despawned, vec![(2, 1, 0), (2, 1, 2), (2, 1, 3)]);
}

#[test]
fn move_to_and_wander_commands_change_the_behaviour() {
    let s = parse(
        "Scenario(seed: 3, map: (width: 64, height: 64), ticks: 6, spawns: [\
         (count: 2, kinds: [0], owners: [0], area: (min: (0, 0), max: (64, 64)))], commands: [\
         (tick: 2, command: MoveTo(ids: [Id(0, 0)], target: (60, 60))), \
         (tick: 3, command: SetWanderArea(ids: [Id(1, 0)], area: (min: (0, 0), max: (4, 4))))])",
    )
    .unwrap();
    let mut world = s.build_world().unwrap();
    for _ in 0..6 {
        world.step();
    }
    // The sim exposes behaviour only through its checksum, so compare with
    // the same scenario without the two commands.
    let mut plain = parse(
        "Scenario(seed: 3, map: (width: 64, height: 64), ticks: 6, spawns: [\
         (count: 2, kinds: [0], owners: [0], area: (min: (0, 0), max: (64, 64)))])",
    )
    .unwrap()
    .build_world()
    .unwrap();
    for _ in 0..6 {
        plain.step();
    }
    assert_ne!(world.checksum(), plain.checksum());
}

// ---- terrain and Place (WP2.1) ----------------------------------------------------

fn temp_tree(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sf_headless_terrain_{label}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("scenarios")).unwrap();
    dir
}

#[test]
fn place_becomes_a_sim_command_in_file_order() {
    let s = parse(&with_command(
        "(tick: 2, player: 1, command: Place(kind: 5, owner: 3, cell: (3, 4)))",
    ))
    .unwrap();
    let world = s.build_world().unwrap();
    let placed: Vec<_> = world
        .pending_commands()
        .iter()
        .filter_map(|(t, p, c)| match c {
            Command::Place { kind, cell, owner } => Some((t.0, p.0, kind.0, *cell, owner.0)),
            _ => None,
        })
        .collect();
    assert_eq!(placed, vec![(2, 1, 5, Cell::new(3, 4), 3)]);
}

#[test]
fn a_place_cell_must_lie_inside_the_map() {
    for cell in ["(16, 0)", "(0, 16)", "(99, 99)"] {
        let p = problems(&with_command(&format!(
            "(tick: 2, command: Place(kind: 1, owner: 0, cell: {cell}))"
        )));
        assert!(has(&p, "outside the 16x16 map"), "{cell}: {p:?}");
    }
    assert!(
        parse(&with_command(
            "(tick: 2, command: Place(kind: 1, owner: 0, cell: (15, 15)))"
        ))
        .is_ok()
    );
}

#[test]
fn a_map_name_follows_the_map_file_name_rules() {
    let with_name = |name: &str| format!("Scenario(seed: 1, ticks: 2, map_name: Some({name:?}))");
    assert!(parse(&with_name("playfield256")).is_ok());
    for bad in ["../x", "a/b", "C:\\x", "/etc", "CON", "", "a\\b"] {
        let p = problems(&with_name(bad));
        assert!(has(&p, "map_name"), "{bad:?}: {p:?}");
    }
}

#[test]
fn a_scenario_with_a_map_name_loads_the_terrain_of_the_map_next_to_it() {
    let root = temp_tree("load");
    // 16 x 16 m = 4 x 4 cells, with a terrain layer.
    let map = sf_data::map::generate::generate_cells(4, 4, 1);
    map.save(&root.join("maps").join("small")).unwrap();
    let file = root.join("scenarios").join("small.ron");
    std::fs::write(
        &file,
        "Scenario(seed: 1, map: (width: 4, height: 4), map_name: Some(\"small\"), ticks: 3)",
    )
    .unwrap();
    let s = Scenario::load(&file).unwrap();
    let terrain = s.terrain.as_ref().expect("terrain read");
    assert_eq!((terrain.width(), terrain.height()), (4, 4));
    assert_eq!(terrain.ids(), map.terrain().unwrap().ids());
    let world = s.build_world().unwrap();
    assert!(world.terrain().has_layer());
    assert_eq!(world.terrain().digest(), terrain.digest());
    // Without the name the scenario has no layer and another checksum.
    std::fs::write(
        &file,
        "Scenario(seed: 1, map: (width: 4, height: 4), ticks: 3)",
    )
    .unwrap();
    let plain = Scenario::load(&file).unwrap();
    assert!(plain.terrain.is_none());
    assert_ne!(
        plain.build_world().unwrap().checksum(),
        world.checksum(),
        "the terrain is in the checksum"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_map_that_does_not_fit_the_scenario_is_reported_with_the_file() {
    let root = temp_tree("mismatch");
    let maps = root.join("maps");
    // A map with a layer, but 6 x 6 cells.
    sf_data::map::generate::generate_cells(6, 6, 1)
        .save(&maps.join("six"))
        .unwrap();
    // A map without a layer.
    let bare = sf_data::map::Map::from_parts(
        16,
        16,
        1.0,
        vec!["a".into()],
        vec![0; 17 * 17],
        vec![0; 256],
    )
    .unwrap();
    bare.save(&maps.join("bare")).unwrap();
    let file = root.join("scenarios").join("s.ron");
    let load = |name: &str| {
        std::fs::write(
            &file,
            format!(
                "Scenario(seed: 1, map: (width: 4, height: 4), map_name: Some({name:?}), ticks: 3)"
            ),
        )
        .unwrap();
        Scenario::load(&file).unwrap_err().to_string()
    };
    let e = load("six");
    assert!(
        e.contains("s.ron") && e.contains("6x6 cells") && e.contains("4x4"),
        "{e}"
    );
    let e = load("bare");
    assert!(e.contains("no terrain layer"), "{e}");
    let e = load("missing");
    assert!(e.contains("missing") && e.contains("cannot read"), "{e}");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn the_order_actions_are_checked_like_the_other_commands() {
    // Targets are cells inside the map; the last cell is fine.
    for action in ["OrderMove", "OrderAttackMove", "OrderGuardPoint"] {
        let p = problems(&with_command(&format!(
            "(tick: 1, command: {action}(ids: [Id(0, 0)], target: (16, 3)))"
        )));
        assert!(has(&p, "target (16, 3) lies outside"), "{action}: {p:?}");
        let p = problems(&with_command(&format!(
            "(tick: 1, command: {action}(ids: [], target: (3, 3)))"
        )));
        assert!(has(&p, "ids must not be empty"), "{action}: {p:?}");
        parse(&with_command(&format!(
            "(tick: 1, command: {action}(ids: [Id(0, 0)], target: (15, 15), queued: true))"
        )))
        .unwrap();
    }
    // An entity target is a slot below the entity limit.
    for action in ["OrderAttack", "OrderGuardEntity"] {
        let p = problems(&with_command(&format!(
            "(tick: 1, command: {action}(ids: [Id(0, 0)], target: (2000000, 0)))"
        )));
        assert!(has(&p, "is not a valid slot"), "{action}: {p:?}");
        parse(&with_command(&format!(
            "(tick: 1, command: {action}(ids: [Id(0, 0)], target: (3, 0)))"
        )))
        .unwrap();
    }
    let p = problems(&with_command("(tick: 1, command: Stop(ids: []))"));
    assert!(has(&p, "ids must not be empty"), "{p:?}");
    let p = problems(&with_command("(tick: 1, command: OrderScatter(ids: []))"));
    assert!(has(&p, "ids must not be empty"), "{p:?}");
    let p = problems(&with_command("(tick: 1, command: OrderHold(ids: []))"));
    assert!(has(&p, "ids must not be empty"), "{p:?}");
    parse(&with_command(
        "(tick: 1, command: OrderHold(ids: [Id(0, 0)], queued: true))",
    ))
    .unwrap();
    // `queued` is optional and a field that does not exist is an error.
    parse(&with_command(
        "(tick: 1, command: OrderScatter(ids: [Id(0, 0)]))",
    ))
    .unwrap();
    let e = syntax_error(&with_command(
        "(tick: 1, command: OrderScatter(ids: [Id(0, 0)], urgent: true))",
    ));
    assert!(e.contains("urgent"), "{e}");
}

#[test]
fn aim_turret_is_checked_and_becomes_the_command() {
    let p = problems(&with_command("(tick: 1, command: AimTurret(ids: []))"));
    assert!(has(&p, "ids must not be empty"), "{p:?}");
    let p = problems(&with_command(
        "(tick: 1, command: AimTurret(ids: [Id(0, 0)], target: Some((4294967295, 0))))",
    ));
    assert!(has(&p, "is not a valid slot"), "{p:?}");
    let s = parse(&with_command(
        "(tick: 2, command: AimTurret(ids: [Range(0, 2)], target: Some((7, 1))))",
    ))
    .unwrap();
    let world = s.build_world().unwrap();
    let aims: Vec<_> = world
        .pending_commands()
        .iter()
        .filter(|(tick, _, _)| *tick == Tick(2))
        .collect();
    assert_eq!(aims.len(), 1);
    assert_eq!(
        *aims[0].2,
        Command::AimTurret {
            ids: vec![EntityId::new(0, 0), EntityId::new(1, 0)],
            target: Some(EntityId::new(7, 1)),
        }
    );
    // No target lets the turret go.
    let s = parse(&with_command(
        "(tick: 2, command: AimTurret(ids: [Id(0, 0)]))",
    ))
    .unwrap();
    let world = s.build_world().unwrap();
    let released = world
        .pending_commands()
        .iter()
        .any(|(_, _, c)| matches!(c, Command::AimTurret { target: None, .. }));
    assert!(released);
}

#[test]
fn the_order_actions_become_order_commands_for_the_cells_middle() {
    let s = parse(&with_command(
        "(tick: 2, player: 1, command: OrderMove(ids: [Range(0, 3)], target: (5, 6), queued: true))",
    ))
    .unwrap();
    let world = s.build_world().unwrap();
    let pending: Vec<_> = world
        .pending_commands()
        .iter()
        .filter(|(tick, _, _)| *tick == Tick(2))
        .collect();
    assert_eq!(pending.len(), 1);
    let (_, player, command) = pending[0];
    assert_eq!(player, PlayerId(1));
    assert_eq!(
        *command,
        Command::Order {
            ids: (0..3).map(|i| EntityId::new(i, 0)).collect(),
            order: Order::Move(Vec2::new(
                Fx::from_int(5) + Fx::HALF,
                Fx::from_int(6) + Fx::HALF
            )),
            queued: true,
        }
    );
    let s = parse(&with_command("(tick: 2, command: Stop(ids: [Id(4, 0)]))")).unwrap();
    let world = s.build_world().unwrap();
    assert!(world.pending_commands().iter().any(|(_, _, c)| *c
        == Command::Stop {
            ids: vec![EntityId::new(4, 0)]
        }));
}

/// Every field of a scenario's harvest block gets its own value, so a swap or a field fed from the
/// wrong source in `HarvestSpec::rules` fails here.
#[test]
fn each_harvest_field_maps_to_its_own_rule() {
    let spec = HarvestSpec {
        harvesters: vec![1],
        capacity: 101,
        mine_amount: 102,
        mine_period: 103,
        unload_amount: 104,
        unload_period: 105,
        search_radius: 106,
        retry_period: 107,
        call_range: 108,
        grant_timeout: 109,
        call_timeout: 110,
        max_tries: 111,
        park_distance: 112,
        park_spread: 113,
        park_near_tenths: 114,
        mine_start_tenths: 115,
        mine_reach_tenths: 116,
        mine_lose_tenths: 117,
        dock_reach_tenths: 118,
    };
    let rules = spec.rules();
    assert_eq!(rules.capacity, 101);
    assert_eq!(rules.mine_amount, 102);
    assert_eq!(rules.mine_period, 103);
    assert_eq!(rules.unload_amount, 104);
    assert_eq!(rules.unload_period, 105);
    assert_eq!(rules.search_radius, 106);
    assert_eq!(rules.retry_period, 107);
    assert_eq!(rules.call_range, 108);
    assert_eq!(rules.grant_timeout, 109);
    assert_eq!(rules.call_timeout, 110);
    assert_eq!(rules.max_tries, 111);
    assert_eq!(rules.park_distance, 112);
    assert_eq!(rules.park_spread, 113);
    assert_eq!(rules.park_near_tenths, 114);
    assert_eq!(rules.mine_start_tenths, 115);
    assert_eq!(rules.mine_reach_tenths, 116);
    assert_eq!(rules.mine_lose_tenths, 117);
    assert_eq!(rules.dock_reach_tenths, 118);
}
