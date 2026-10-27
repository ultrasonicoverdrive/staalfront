//! The `ore_256` scenario (WP7.1): ore amounts per cell on the 256 by 256 cell playfield, taking,
//! depletion and slow regrowth, with the final checksum pinned. The rules are in
//! `docs/rules/economy.md`.

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::run::{RunOptions, run};
use sf_headless::scenario::Scenario;
use sf_sim::{Cell, World};

const EXPECTED: &str = include_str!("../../../assets/scenarios/ore_256.expected");

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("ore_256.ron")).unwrap()
}

fn after(ticks: u64) -> World {
    let mut world = scenario().build_world().unwrap();
    for _ in 0..ticks {
        world.step();
    }
    world
}

/// The ore cells within a box of the map.
fn in_box(world: &World, x: (u32, u32), y: (u32, u32)) -> Vec<Cell> {
    world
        .ore()
        .unwrap()
        .cells()
        .map(|(c, _)| c)
        .filter(|c| (x.0..x.1).contains(&c.x) && (y.0..y.1).contains(&c.y))
        .collect()
}

#[test]
fn the_final_checksum_is_the_pinned_value() {
    let expected = parse_checksum(EXPECTED).unwrap();
    let s = scenario();
    let outcome = run(&s, &RunOptions::for_scenario(&s)).unwrap();
    assert_eq!(outcome.final_checksum, expected);
}

#[test]
fn the_playfield_has_215_ore_cells_full_at_the_start() {
    let world = after(0);
    let ore = world.ore().unwrap();
    assert_eq!(ore.field_count(), 215);
    assert!(ore.cells().all(|(_, a)| a == 120));
    assert_eq!(ore.total(), 215 * 120);
    // Field cells are exactly the ore-typed cells of the terrain.
    let ore_type = sf_data::terrain::TerrainTypes::embedded()
        .id_of("ore")
        .unwrap();
    let typed = world
        .terrain()
        .ids()
        .iter()
        .filter(|&&i| i == ore_type)
        .count();
    assert_eq!(typed, 215);
}

/// Depletion and regrowth by the rule, worked out here from the rule alone: the cell at place
/// `j` of the field list has its turn in the ticks `t` with `t % 20 == j % 20`, and gains 2 up
/// to 160. Field A is emptied in tick 5, field B drops to 40 in tick 5, the rest is untouched.
#[test]
fn depletion_and_regrowth_follow_the_rule() {
    let end = 600_u64;
    let world = after(end);
    let list: Vec<Cell> = world.ore().unwrap().cells().map(|(c, _)| c).collect();
    let turns_from = |j: usize, from: u64| -> u64 {
        (from..end).filter(|t| t % 20 == (j as u64) % 20).count() as u64
    };
    let field_a = in_box(&world, (110, 125), (76, 86));
    let field_b = in_box(&world, (40, 52), (112, 122));
    assert_eq!((field_a.len(), field_b.len()), (28, 27));
    for (j, &c) in list.iter().enumerate() {
        let expected = if field_a.contains(&c) {
            // Emptied in tick 5 (the take runs before that tick's growth): growth from tick 5.
            (2 * turns_from(j, 5)).min(160)
        } else if field_b.contains(&c) {
            // A cell whose turn came before tick 5 had grown by 2 when the 80 were taken.
            (120 + 2 * (turns_from(j, 0) - turns_from(j, 5)) - 80 + 2 * turns_from(j, 5)).min(160)
        } else {
            // Untouched: 120, growing by 2 from tick 0, held at 160.
            (120 + 2 * turns_from(j, 0)).min(160)
        };
        assert_eq!(u64::from(world.ore_at(c)), expected, "cell {c} (place {j})");
    }
}

#[test]
fn half_way_field_a_is_nearly_empty_and_untouched_fields_have_grown() {
    let world = after(300);
    let field_a = in_box(&world, (110, 125), (76, 86));
    assert!(field_a.iter().all(|&c| world.ore_at(c) <= 2 * 15));
    // 120 + 2 * 15 turns = 150 by tick 300 on an untouched cell.
    let other = in_box(&world, (50, 60), (230, 240));
    assert!(!other.is_empty());
    assert!(other.iter().all(|&c| world.ore_at(c) == 150));
}

#[test]
fn every_take_command_counted() {
    let s = scenario();
    let outcome = run(&s, &RunOptions::for_scenario(&s)).unwrap();
    assert_eq!(outcome.commands_executed, 55);
    assert_eq!(outcome.commands_ignored, 0);
}
