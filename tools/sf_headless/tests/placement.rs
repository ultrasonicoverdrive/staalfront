//! The `placement` scenario (WP6.2): two bases build with `BuildStructure`, one command
//! per tick, on the 256 by 256 cell playfield, and the rules refuse what they should.
//!
//! The cells are those the scenario file names; the definitions are the frozen copy in
//! `assets/scenarios/placement_defs/`, so a balance change does not move the checksum.

mod common;

use common::scenario_path;
use sf_headless::scenario::{Scenario, ScenarioError};
use sf_sim::{Cell, Kind, PlacementError, PlayerId, World};

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("placement.ron")).unwrap()
}

/// The ticks that refuse a build, with the player, the kind and the reason, from a run of the
/// whole scenario, plus the world at the end.
fn run() -> (World, Vec<(u64, u8, u16, PlacementError)>) {
    let (world, refused, _) = run_with_completions();
    (world, refused)
}

/// The same run, and the structures that became complete: `(tick, kind, owner)`.
fn run_with_completions() -> (
    World,
    Vec<(u64, u8, u16, PlacementError)>,
    Vec<(u64, u16, u8)>,
) {
    let mut world = scenario().build_world().unwrap();
    let mut refused = Vec::new();
    let mut done = Vec::new();
    for _ in 0..scenario().ticks {
        let report = world.step();
        assert_eq!(
            world.placement_rejections().len(),
            report.placements_rejected as usize
        );
        for r in world.placement_rejections() {
            refused.push((r.tick.0, r.player.0, r.kind.0, r.reason));
        }
        assert_eq!(
            world.completions().len(),
            report.structures_completed as usize
        );
        for c in world.completions() {
            done.push((c.tick.0, c.kind.0, c.owner.0));
        }
    }
    (world, refused, done)
}

fn stands(world: &World, x: u32, y: u32) -> bool {
    world.occupant_at(Cell::new(x, y)).is_some()
}

#[test]
fn every_rule_refuses_what_it_should_and_nothing_else() {
    let (world, refused) = run();
    // Plant (kind 1), refinery (2), barracks (3), vehicle factory (4), decoy (9), bunker (10).
    let power = Kind(1);
    let expected = vec![
        (2, 0, power.0, PlacementError::BadTerrain),
        (3, 0, power.0, PlacementError::Occupied),
        (4, 0, power.0, PlacementError::UnitInTheWay),
        (5, 0, power.0, PlacementError::TooFarFromBase),
        (6, 0, 4, PlacementError::MissingPrerequisite(Kind(2))),
        // The refinery needs the power plant, which is only going up (WP6.3): refused at tick 7
        // and at tick 300, one tick before the plant is complete; accepted at 301.
        (7, 0, 2, PlacementError::MissingPrerequisite(power)),
        (12, 0, 0, PlacementError::CannotBeBuilt),
        (14, 1, 3, PlacementError::MissingPrerequisite(power)),
        (15, 1, 9, PlacementError::WrongFaction),
        (17, 1, power.0, PlacementError::TooFarFromBase),
        (18, 1, power.0, PlacementError::OffMap),
        (300, 0, 2, PlacementError::MissingPrerequisite(power)),
        (703, 0, 10, PlacementError::WrongFaction),
        (704, 0, 3, PlacementError::ExitBlocked),
    ];
    assert_eq!(refused, expected);

    // What stands: Meridian's plant, refinery, factory, barracks and decoy, Veyran's plant,
    // barracks and bunker, and the two anchors, the rifleman and the placed decoy.
    for (x, y, owner) in [
        (22, 44, 0),
        (30, 40, 0),
        (22, 49, 0),
        (19, 40, 0),
        (26, 49, 0),
        (30, 48, 0),
        (100, 36, 1),
        (108, 36, 1),
        (104, 42, 1),
        (106, 42, 1),
        (60, 100, 1),
    ] {
        let id = world
            .occupant_at(Cell::new(x, y))
            .unwrap_or_else(|| panic!("nothing on ({x}, {y})"));
        assert_eq!(world.get(id).unwrap().owner, PlayerId(owner), "({x}, {y})");
    }
    // The refused ones left nothing behind.
    for (x, y) in [(16, 44), (31, 48), (46, 44), (34, 44)] {
        assert!(!stands(&world, x, y), "({x}, {y})");
    }
    // 11 structures + the rifleman.
    assert_eq!(world.len(), 12);
}

#[test]
fn the_vehicle_factory_is_refused_until_the_refinery_stands() {
    let (world, _) = run();
    // Same cells, ticks 6 and 1102: the first refused, the second stands.
    assert!(stands(&world, 19, 40));
}

#[test]
fn a_structure_is_complete_after_its_build_time_and_not_before() {
    let (world, _, done) = run_with_completions();
    // Build seconds of the frozen definitions at 20 ticks a second, from the tick of the command:
    // plant 300 (1 -> 301, 13 -> 313), barracks 400 (301 -> 701, 313 -> 713), decoy 160
    // (702 -> 862), refinery 800 (301 -> 1101), bunker 500 (714 -> 1214), factory 1000 (1102 -> 2102).
    assert_eq!(
        done,
        vec![
            (301, 1, 0),
            (313, 1, 1),
            (701, 3, 0),
            (713, 3, 1),
            (862, 9, 0),
            (1101, 2, 0),
            (1214, 10, 1),
            (2102, 4, 0),
        ]
    );
    assert_eq!(world.under_construction(), 0);
}

#[test]
fn a_placed_structure_of_another_faction_stands_and_a_scenario_needs_the_rules_to_build() {
    // The decoy placed at tick 20 (complete at once) is a Veyran decoy, Meridian-only to build: PlaceStructure
    // does not check it (a mission places what it likes).
    let (world, _) = run();
    let id = world.occupant_at(Cell::new(60, 100)).unwrap();
    assert_eq!(world.get(id).unwrap().owner, PlayerId(1));

    // A scenario that builds without `buildings` is refused when it is checked.
    let mut s = scenario();
    s.buildings = false;
    s.factions.clear();
    match s.build_world() {
        Err(ScenarioError::Invalid { problems, .. }) => {
            assert!(
                problems
                    .iter()
                    .any(|p| p.contains("BuildStructure needs `buildings`")),
                "{problems:?}"
            );
        }
        other => panic!("{other:?}"),
    }
    // And `factions` without `buildings` is refused too.
    let mut s = scenario();
    s.buildings = false;
    s.commands
        .retain(|c| !format!("{:?}", c.command).contains("BuildStructure"));
    match s.build_world() {
        Err(ScenarioError::Invalid { problems, .. }) => {
            assert!(
                problems
                    .iter()
                    .any(|p| p.contains("factions needs `buildings`")),
                "{problems:?}"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_run_is_deterministic() {
    let sums = |()| {
        let (world, _) = run();
        world.checksum_history().to_vec()
    };
    assert_eq!(sums(()), sums(()));
}

#[test]
fn the_frozen_definitions_are_the_wp6_1_tree() {
    // The copy the scenario runs on has the building table of WP6.1, whatever the shipped file
    // becomes: its digest is the one pinned in `sf_bridge/tests/buildings.rs`, and the base radius
    // is in the placement digest.
    let world = scenario().build_world().unwrap();
    assert_eq!(world.building_table().digest(), 0x53ae_b53a_b441_57d1);
    let anchor = world.building_table().get(Kind(0)).unwrap();
    assert_eq!(anchor.build_radius, 8);
    assert_eq!(world.player_factions().len(), 2);
}
