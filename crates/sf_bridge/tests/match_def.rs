//! The match file through the bridge (WP7.6): each number of the file arrives in the simulation as
//! itself, in the order of the file, and a structure and a unit become the two commands they should.

use sf_bridge::{match_commands, match_credits, match_factions, match_rules};
use sf_data::match_def::MatchDef;
use sf_data::units::UnitDefs;
use sf_math::{Fx, Vec2};
use sf_sim::{Cell, Command, Kind, PlayerId};

/// Distinct values everywhere, so a swapped or dropped field shows: players 5 and 2 (not in order),
/// factions Veyran and Meridian, credits 7 and 3, a structure and two units each.
fn def() -> MatchDef {
    MatchDef::from_ron(
        r#"(name: "t", map: "m1", seed: 9, players: [
            (id: 5, faction: Veyran, credits: 7, objects: [
                (role: "power_plant", cell: (30, 31)),
                (role: "rifleman", cell: (32, 33)),
                (role: "light_tank", cell: (34, 35)),
            ]),
            (id: 2, faction: Meridian, credits: 3, objects: [
                (role: "construction_center", cell: (10, 11)),
                (role: "harvester", cell: (12, 13)),
            ]),
        ])"#,
    )
    .unwrap()
}

#[test]
fn the_players_arrive_ascending_whatever_the_order_of_the_file() {
    let rules = match_rules(&def()).unwrap();
    assert_eq!(rules.players(), &[PlayerId(2), PlayerId(5)]);
}

#[test]
fn each_faction_and_credit_arrives_with_its_own_player() {
    // Meridian is bit 1 and Veyran bit 2 (units::Faction::bit); the file lists Veyran first.
    assert_eq!(
        match_factions(&def()),
        vec![(PlayerId(5), 2), (PlayerId(2), 1)]
    );
    assert_eq!(
        match_credits(&def()),
        vec![(PlayerId(5), 7), (PlayerId(2), 3)]
    );
}

#[test]
fn a_structure_is_placed_and_a_unit_is_spawned_for_its_owner_in_the_order_of_the_file() {
    let defs = UnitDefs::embedded();
    let kind = |role: &str| Kind(defs.kind_of(role).unwrap());
    let middle = |x: i32, y: i32| Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF);
    let commands = match_commands(&def(), &defs).unwrap();
    assert_eq!(
        commands,
        vec![
            (
                PlayerId(5),
                Command::PlaceStructure {
                    kind: kind("power_plant"),
                    cell: Cell::new(30, 31),
                    owner: PlayerId(5)
                }
            ),
            (
                PlayerId(5),
                Command::SpawnUnit {
                    kind: kind("rifleman"),
                    pos: middle(32, 33),
                    owner: PlayerId(5)
                }
            ),
            (
                PlayerId(5),
                Command::SpawnUnit {
                    kind: kind("light_tank"),
                    pos: middle(34, 35),
                    owner: PlayerId(5)
                }
            ),
            (
                PlayerId(2),
                Command::PlaceStructure {
                    kind: kind("construction_center"),
                    cell: Cell::new(10, 11),
                    owner: PlayerId(2)
                }
            ),
            (
                PlayerId(2),
                Command::SpawnUnit {
                    kind: kind("harvester"),
                    pos: middle(12, 13),
                    owner: PlayerId(2)
                }
            ),
        ]
    );
    // And the kinds are the permanent numbers of the shipped file, not whatever the table has.
    assert_eq!(
        [kind("power_plant").0, kind("rifleman").0, kind("light_tank").0],
        [1, 12, 19]
    );
    assert_eq!([kind("construction_center").0, kind("harvester").0], [0, 11]);
}

#[test]
fn a_role_that_is_not_there_is_an_error_not_a_skipped_object() {
    let mut d = def();
    d.players[0].objects[0].role = "nonesuch".into();
    assert!(
        match_commands(&d, &UnitDefs::embedded())
            .unwrap_err()
            .contains("nonesuch")
    );
}

#[test]
fn the_limits_of_the_file_are_the_limits_of_the_simulation() {
    assert_eq!(sf_data::match_def::MAX_PLAYERS, sf_sim::MAX_MATCH_PLAYERS);
    assert_eq!(sf_data::match_def::MAX_CREDITS, sf_sim::MAX_STARTING_CREDITS);
}
