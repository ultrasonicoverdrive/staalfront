//! The AI builds its base (WP9.1b-1, `docs/rules/ai.md`): the scenarios `ai_base`, `ai_noore` and
//! `ai_boxed` with their checksums and AI digests pinned, and the rules of the construction manager seen
//! from outside: the goal list is built in time, power never runs short, nothing stalls, the simulation
//! refuses nothing, structures keep their lane and aprons, an AI without income or room does not hang, a
//! unit hidden in the shroud changes nothing, and the AI sends only the commands a player may.
//!
//! Every test runs the code the game uses: `run_watched`/`run_full` drive the AI like the runner does.
//! Player 0 is the AI (Meridian, a construction centre at (12, 68), 6000 credits) on the first match's map;
//! player 1 is a lone static construction centre.

mod common;

use std::collections::BTreeMap;

use common::scenario_path;
use sf_ai::placement::search;
use sf_ai::{Lens, Observation};
use sf_headless::cli::parse_checksum;
use sf_headless::run::{Finished, RunOptions, run, run_full, run_watched};
use sf_headless::scenario::{Action, IdSpec, Scenario, TimedCommand};
use sf_sim::{
    BuildingFunction, Cell, Command, EntityId, Footprint, Kind, PlacementError, PlayerId,
    Visibility, World, gap,
};

const P0: PlayerId = PlayerId(0);

const BASE_EXPECTED: &str = include_str!("../../../assets/scenarios/ai_base.expected");
const NOORE_EXPECTED: &str = include_str!("../../../assets/scenarios/ai_noore.expected");
const BOXED_EXPECTED: &str = include_str!("../../../assets/scenarios/ai_boxed.expected");
/// `Ai::digest` of player 0 at the end of each scenario.
const BASE_DIGEST: u64 = 0x11dd_e9a0_0dc1_4f23;
const NOORE_DIGEST: u64 = 0x4920_9649_c89c_7ba9;
const BOXED_DIGEST: u64 = 0x7814_85e5_5788_ab34;

/// The kinds of the frozen definitions (`ai_base_defs/unit_defs.ron`).
const PLANT: Kind = Kind(1);
const REFINERY: Kind = Kind(2);
const BARRACKS: Kind = Kind(3);
const FACTORY: Kind = Kind(4);
const RADAR: Kind = Kind(5);
const TOWER: Kind = Kind(6);
const ADVANCED: Kind = Kind(7);
const HARVESTER: Kind = Kind(11);

/// Measured: the last goal up to the vehicle factory stands at tick 1998, the last of the list at 8078 (6518
/// before WP9.1b-2: the two harvesters the economy buys cost 2,200 credits the list then waits for); the
/// bounds leave 20 percent.
const T1: u64 = 2400;
const T1B: u64 = 9700;

fn scenario(name: &str) -> Scenario {
    Scenario::load(&scenario_path(&format!("{name}.ron"))).unwrap()
}

fn go(s: &Scenario, ticks: u64) -> Finished {
    run_full(s, &RunOptions { ticks, every: 100 }).unwrap()
}

/// Player 0's structures in `world`: `(id, kind, anchor, width, height)`, ascending by id.
fn structures(world: &World) -> Vec<(EntityId, Kind, Cell, i64, i64)> {
    world
        .snapshot()
        .iter()
        .filter(|v| v.owner == P0)
        .filter_map(|v| {
            let fp = world.footprint_of(v.id)?;
            Some((
                v.id,
                v.kind,
                world.cell_of(v.id)?,
                i64::from(fp.width),
                i64::from(fp.height),
            ))
        })
        .collect()
}

/// The run of `s` with, per tick, a check of `ok`; the first tick that fails is reported.
fn first_failure(s: &Scenario, mut ok: impl FnMut(&World) -> bool) -> Option<u64> {
    let mut failed = None;
    run_watched(s, &RunOptions::for_scenario(s), |w| {
        if failed.is_none() && !ok(w) {
            failed = Some(w.tick().0);
        }
    })
    .unwrap();
    failed
}

#[test]
fn the_checksums_and_ai_digests_are_the_pinned_values() {
    for (name, expected, digest) in [
        ("ai_base", BASE_EXPECTED, BASE_DIGEST),
        ("ai_noore", NOORE_EXPECTED, NOORE_DIGEST),
        ("ai_boxed", BOXED_EXPECTED, BOXED_DIGEST),
    ] {
        let s = scenario(name);
        let outcome = run(&s, &RunOptions::for_scenario(&s)).unwrap();
        assert_eq!(
            outcome.final_checksum,
            parse_checksum(expected).unwrap(),
            "{name}"
        );
        assert_eq!(outcome.ai_digests, vec![(0, digest)], "{name}");
        // Twice the same, the AI included.
        assert_eq!(outcome, run(&s, &RunOptions::for_scenario(&s)).unwrap());
    }
}

#[test]
fn the_goal_list_stands_by_t1_up_to_the_vehicle_factory_and_by_t1b_in_full() {
    let s = scenario("ai_base");
    let mut stood: BTreeMap<EntityId, (Kind, u64)> = BTreeMap::new();
    let f = run_watched(&s, &RunOptions::for_scenario(&s), |w| {
        for (id, kind, ..) in structures(w) {
            if w.stands(id) {
                stood.entry(id).or_insert((kind, w.tick().0));
            }
        }
    })
    .unwrap();
    let list: Vec<(Kind, u64)> = stood.values().copied().collect();
    // In order: the centre, then the goal list with the third power plant the power rule puts before the
    // radar (190 used + 20 margin > 200 made).
    let kinds: Vec<Kind> = list.iter().map(|&(k, _)| k).collect();
    assert_eq!(
        kinds,
        [
            Kind(0),
            PLANT,
            REFINERY,
            BARRACKS,
            PLANT,
            FACTORY,
            TOWER,
            REFINERY,
            PLANT,
            RADAR,
            TOWER,
            ADVANCED
        ]
    );
    let factory = list.iter().find(|&&(k, _)| k == FACTORY).unwrap().1;
    let last = list.last().unwrap().1;
    println!("T1 (vehicle factory stands) = tick {factory}, T1b (whole list) = tick {last}");
    assert!(
        list[..6].iter().all(|&(_, t)| t <= T1),
        "by T1 = {T1}: {list:?}"
    );
    assert!(last <= T1B, "by T1b = {T1B}: {list:?}");
    // Exactly one build per structure, one harvester bought per refinery, nothing refused.
    let builds = f
        .ai_commands
        .iter()
        .filter(|c| matches!(c.2, Command::BuildStructure { .. }))
        .count();
    assert_eq!((builds, f.ai_commands.len()), (11, 13));
    assert_eq!(f.outcome.commands_ignored, 0);
}

#[test]
fn power_made_is_never_below_power_used() {
    let s = scenario("ai_base");
    let failed = first_failure(&s, |w| w.power_of(P0).is_none_or(|p| p.made >= p.used));
    assert_eq!(failed, None, "power short at tick {failed:?}");
}

#[test]
fn nothing_stalls_for_lack_of_money() {
    // The money rule: a build starts only with its price saved, so the simulation never waits for credits.
    for name in ["ai_base", "ai_noore", "ai_boxed"] {
        let failed = first_failure(&scenario(name), |w| w.stalled().is_empty());
        assert_eq!(failed, None, "{name}: a stall at tick {failed:?}");
    }
}

#[test]
fn the_simulation_refuses_no_build() {
    for name in ["ai_base", "ai_noore", "ai_boxed"] {
        let s = scenario(name);
        let failed = first_failure(&s, |w| w.placement_rejections().is_empty());
        assert_eq!(failed, None, "{name}: a refused build at tick {failed:?}");
        let o = run(&s, &RunOptions::for_scenario(&s)).unwrap();
        assert_eq!(o.commands_ignored, 0, "{name}");
    }
}

#[test]
fn no_two_structures_touch_and_every_apron_is_free() {
    let s = scenario("ai_base");
    let f = go(&s, s.ticks);
    let e = f.ais[0].state().profile.economy;
    let list = structures(&f.world);
    assert_eq!(list.len(), 12);
    // The lane: at least `lane` free cells between any two of the player's structures.
    for (i, a) in list.iter().enumerate() {
        for b in &list[i + 1..] {
            let g = gap(
                (i64::from(a.2.x), i64::from(a.2.y), a.3, a.4),
                (i64::from(b.2.x), i64::from(b.2.y), b.3, b.4),
            );
            assert!(g >= i64::from(e.lane), "{a:?} and {b:?}: gap {g}");
        }
    }
    // The aprons, from the AI's own function: no structure on any of their cells.
    let table = f.world.building_table();
    let mut aprons = 0;
    for &(_, kind, at, _, _) in &list {
        let b = table.get(kind).unwrap();
        let size = match b.function {
            BuildingFunction::HarvesterDock => e.refinery_apron,
            f if f.is_factory() => e.factory_apron,
            _ => continue,
        };
        aprons += 1;
        for (x, y) in sf_ai::placement::apron(at, b.footprint, b.exit_side.unwrap(), size) {
            for &(id, kind2, c, w, h) in &list {
                let inside = x >= i64::from(c.x)
                    && x < i64::from(c.x) + w
                    && y >= i64::from(c.y)
                    && y < i64::from(c.y) + h;
                assert!(
                    !inside,
                    "({x}, {y}) of the apron of {kind:?} at {at:?} is under {id:?} ({kind2:?})"
                );
            }
        }
    }
    // Two refineries, the barracks and the vehicle factory.
    assert_eq!(aprons, 4);
}

#[test]
fn four_harvesters_mine_and_bring_credits() {
    let s = scenario("ai_base");
    let f = go(&s, s.ticks);
    let harvesters = f
        .world
        .snapshot()
        .iter()
        .filter(|v| v.owner == P0 && v.kind == HARVESTER)
        .count();
    // One free with each refinery and one the economy buys for each (WP9.1b-2).
    assert_eq!(harvesters, 4);
    assert!(f.world.ore_delivered(P0) > 0);
}

#[test]
fn the_ai_sends_only_what_a_player_may_send() {
    // The whitelist of the design: building, queueing units and repairing.
    for name in ["ai_base", "ai_noore", "ai_boxed"] {
        let s = scenario(name);
        let f = go(&s, s.ticks);
        assert!(!f.ai_commands.is_empty(), "{name}");
        for (tick, player, c) in &f.ai_commands {
            assert_eq!(*player, 0);
            assert!(
                matches!(
                    c,
                    Command::BuildStructure { .. }
                        | Command::QueueUnit { .. }
                        | Command::Repair { .. }
                ),
                "{name}: tick {tick}: {c:?}"
            );
        }
    }
}

#[test]
fn without_income_the_ai_builds_what_it_can_pay_and_then_waits() {
    let s = scenario("ai_noore");
    let f = go(&s, s.ticks);
    assert_eq!(f.outcome.ticks, 40_000);
    assert_eq!(f.world.ore_delivered(P0), 0);
    // At most one build per construction turn (one every 40 ticks), and none once the money is gone.
    let mut per_thousand = [0u32; 40];
    for (tick, ..) in &f.ai_commands {
        per_thousand[(*tick / 1000) as usize] += 1;
    }
    assert!(per_thousand.iter().all(|&n| n <= 25), "{per_thousand:?}");
    let last = f.ai_commands.last().unwrap().0;
    assert!(last < 3000, "the last build at tick {last}");
    // What is left is less than the radar, the next goal.
    assert!(f.world.credits(P0) < 900);
    assert!(f.ai_commands.len() <= 11);
}

#[test]
fn boxed_in_the_refinery_is_blocked_and_later_goals_are_built() {
    let s = scenario("ai_boxed");
    let f = go(&s, s.ticks);
    let kinds: Vec<Kind> = structures(&f.world).iter().map(|s| s.1).collect();
    assert!(!kinds.contains(&REFINERY), "{kinds:?}");
    // The barracks, the second power plant and both towers come after the refinery in the list.
    for (kind, n) in [(PLANT, 2), (BARRACKS, 1), (TOWER, 2)] {
        assert_eq!(
            kinds.iter().filter(|&&k| k == kind).count(),
            n,
            "{kind:?} in {kinds:?}"
        );
    }
    let state = f.ais[0].state();
    assert!(state.blocked_until.contains_key(&REFINERY));
    // The search asks the simulation only about candidates its mask lets through: one question per build.
    let builds = f
        .ai_commands
        .iter()
        .filter(|c| matches!(c.2, Command::BuildStructure { .. }))
        .count();
    assert_eq!(state.previews, builds as u64);
}

#[test]
fn a_build_the_simulation_refuses_is_noticed_and_built_elsewhere() {
    // The first build of `ai_base` is the power plant at (9, 68), sent at tick 10. A unit of player 0 put
    // on that cell at the same tick, before the AI's command runs, makes the simulation refuse it; the unit
    // drives off at tick 20. The next construction turn (tick 50) sees no plant there, remembers the
    // refusal (the cell is free again, so only that memory keeps the AI from the same cell) and builds the
    // plant elsewhere.
    let mut s = scenario("ai_base");
    let plain = go(&s, 300);
    assert_eq!(
        plain.ai_commands[0],
        (
            10,
            0,
            Command::BuildStructure {
                kind: PLANT,
                cell: Cell::new(9, 68)
            }
        )
    );
    s.commands.push(TimedCommand {
        tick: 10,
        player: 0,
        command: Action::SpawnUnit {
            kind: 22,
            owner: 0,
            cell: (9, 68),
        },
    });
    s.commands.push(TimedCommand {
        tick: 20,
        player: 0,
        command: Action::OrderMove {
            ids: vec![IdSpec::Id(2, 0)],
            target: (6, 58),
            queued: false,
        },
    });
    let at_50 = go(&s, 50);
    let truck = at_50.world.cell_of(EntityId::new(2, 0)).or_else(|| {
        at_50
            .world
            .snapshot()
            .iter()
            .find(|v| v.id == EntityId::new(2, 0))
            .and_then(|v| at_50.world.cell_at(v.pos))
    });
    let truck = truck.unwrap();
    assert!(
        !((9..11).contains(&truck.x) && (68..71).contains(&truck.y)),
        "the truck is still on the plant's cells at tick 50: {truck:?}"
    );
    let f = go(&s, 2500);
    assert_eq!(f.outcome.commands_ignored, 1);
    // The first plant (the lowest id) stands elsewhere; (9, 68) may be used again once the refusal has
    // expired (`blocked_ticks` later).
    let plants: Vec<Cell> = structures(&f.world)
        .iter()
        .filter(|s| s.1 == PLANT)
        .map(|s| s.2)
        .collect();
    assert_ne!(plants[0], Cell::new(9, 68), "{plants:?}");
    assert_eq!(f.ai_commands[1].0, 50, "{:?}", f.ai_commands);
    // The base goes on: the vehicle factory stands.
    assert!(
        structures(&f.world)
            .iter()
            .any(|s| s.1 == FACTORY && f.world.stands(s.0))
    );
}

/// A repair truck of player 1 (unarmed) at `cell` from tick 0.
fn with_hidden_truck(mut s: Scenario, cell: (u32, u32)) -> Scenario {
    s.commands.push(TimedCommand {
        tick: 0,
        player: 1,
        command: Action::SpawnUnit {
            kind: 22,
            owner: 1,
            cell,
        },
    });
    s
}

/// A corner of the build reach (x 4..23, y 60..79 for a 1 x 1) that player 0 does not see at the start.
const CORNER: (u32, u32) = (4, 60);

#[test]
fn a_unit_hidden_in_the_reach_changes_nothing_the_ai_does() {
    // Until the corner first comes into view the AI must not be able to tell the two worlds apart.
    let s = with_hidden_truck(scenario("ai_base"), CORNER);
    let corner = Cell::new(CORNER.0, CORNER.1);
    let mut seen_at = None;
    run_watched(&s, &RunOptions::for_scenario(&s), |w| {
        if seen_at.is_none() && w.visibility(P0, corner) == Visibility::Visible {
            seen_at = Some(w.tick().0);
        }
    })
    .unwrap();
    let until = seen_at.unwrap_or(s.ticks + 1) - 1;
    println!("the corner {corner:?} comes into view at tick {seen_at:?}");
    // Most of the base is built by then.
    assert!(until >= T1, "seen at {until}");
    let with = go(&s, until);
    // The other world has the same truck far away (also in the shroud), not no truck: an entity takes an
    // id, the ids of the AI's own things shift by one, and a `QueueUnit` names a factory by its id.
    let without = go(&with_hidden_truck(scenario("ai_base"), (92, 92)), until);
    assert_ne!(with.world.checksum(), without.world.checksum());
    assert_eq!(with.ai_commands, without.ai_commands);
    assert!(with.ai_commands.len() >= 6);
    assert_eq!(with.ais[0].digest(), without.ais[0].digest());
}

#[test]
fn the_lens_does_not_ask_about_ground_the_player_does_not_see() {
    // The fence of the placement rule: the simulation's verdict on a footprint in the shroud would tell
    // the AI that a unit stands there. The lens answers nothing for it; on visible ground it asks.
    let s = with_hidden_truck(scenario("ai_base"), CORNER);
    let f = go(&s, 1);
    let corner = Cell::new(CORNER.0, CORNER.1);
    assert_eq!(f.world.visibility(P0, corner), Visibility::Shroud);
    // Control: the simulation itself does see the truck.
    assert_eq!(
        f.world.check_placement(PLANT, corner, P0),
        Err(PlacementError::UnitInTheWay)
    );
    let mut lens = Lens::new(&f.world, P0);
    assert_eq!(lens.placement(PLANT, corner), None);
    assert_eq!(lens.previews(), 0);
    // Visible ground next to the centre: asked, and allowed.
    assert_eq!(lens.placement(PLANT, Cell::new(9, 68)), Some(Ok(())));
    assert_eq!(lens.previews(), 1);
    // A footprint with one cell in the shroud is not asked either.
    let edge = (0..96)
        .map(|x| Cell::new(x, 60))
        .find(|&c| f.world.visibility(P0, c) == Visibility::Visible)
        .unwrap();
    let left = Cell::new(edge.x - 1, 60);
    assert_eq!(f.world.visibility(P0, left), Visibility::Shroud);
    assert_eq!(lens.placement(PLANT, left), None);
    assert_eq!(lens.previews(), 1);
}

#[test]
fn the_search_keeps_off_the_apron_of_an_own_refinery() {
    // The placement search of the AI on the world of `ai_base` after one tick. Alone, the best place for a
    // power plant is (9, 68), west of the centre. The same search with a refinery the AI believes it has at
    // (5, 67) (exit side +x, so its apron is x 8..12, y 68..70) must keep the plant off that apron; the
    // world has no refinery there, so only the AI's own rule can keep it off.
    let f = go(&scenario("ai_base"), 1);
    let mut obs = Observation::build(&f.world, P0, &BTreeMap::new());
    let mut lens = Lens::new(&f.world, P0);
    let mut state = f.ais[0].state().clone();
    assert_eq!(
        search(&mut state.clone(), &obs, &mut lens, PLANT),
        Some(Cell::new(9, 68))
    );
    let mut refinery = obs.own[0].clone();
    refinery.id = EntityId::new(1000, 0);
    refinery.kind = REFINERY;
    refinery.cell = Cell::new(5, 67);
    refinery.footprint = Some(Footprint {
        width: 3,
        height: 4,
    });
    obs.own.push(refinery);
    let got = search(&mut state, &obs, &mut lens, PLANT).unwrap();
    let b = f.world.building_table().get(REFINERY).unwrap();
    let e = state.profile.economy;
    for (x, y) in sf_ai::placement::apron(
        Cell::new(5, 67),
        b.footprint,
        b.exit_side.unwrap(),
        e.refinery_apron,
    ) {
        let inside = x >= i64::from(got.x)
            && x < i64::from(got.x) + 2
            && y >= i64::from(got.y)
            && y < i64::from(got.y) + 3;
        assert!(
            !inside,
            "the plant at {got:?} covers ({x}, {y}) of the apron"
        );
    }
}

#[test]
fn the_new_scenarios_maps_are_what_the_generator_draws() {
    for (name, drawn) in [
        ("m1_noore", gen_ai_maps::draw_noore()),
        ("m1_boxed", gen_ai_maps::draw_boxed()),
    ] {
        let committed =
            sf_data::map::Map::load(&scenario_path(&format!("../maps/{name}"))).unwrap();
        assert_eq!(
            committed.terrain().unwrap().ids(),
            drawn.terrain().unwrap().ids(),
            "re-run: cargo run -p sf_headless --example gen_ai_maps -- assets/maps"
        );
    }
    // No ore on m1_noore; the rock box of m1_boxed with its pocket (rock 3, grass 0, literal ids).
    let types = sf_data::terrain::TerrainTypes::embedded();
    let ore = types.id_of("ore").unwrap();
    assert!(
        !gen_ai_maps::draw_noore()
            .terrain()
            .unwrap()
            .ids()
            .contains(&ore)
    );
    let boxed = gen_ai_maps::draw_boxed();
    let layer = boxed.terrain().unwrap();
    assert_eq!(layer.get(4, 56), Some(3));
    assert_eq!(layer.get(27, 83), Some(3));
    assert_eq!(layer.get(5, 66), Some(0));
    assert_eq!(layer.get(17, 73), Some(0));
    assert_eq!(
        (gen_ai_maps::BOX, gen_ai_maps::POCKET),
        ((4, 56, 27, 83), (5, 66, 17, 73))
    );
}

#[allow(dead_code)]
#[path = "../examples/gen_ai_maps.rs"]
mod gen_ai_maps;
