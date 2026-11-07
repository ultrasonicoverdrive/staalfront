//! The ore silo (WP7.3b): the `silo` scenario (a refinery that stores 600, a silo built at tick 2600
//! that stores 400 and is sold at tick 4400) with its checksum pinned. See
//! `assets/scenarios/silo.ron` and `docs/rules/economy.md` ("The ore silo").

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::run::{RunOptions, run};
use sf_headless::scenario::Scenario;
use sf_sim::{PlayerId, World};

const EXPECTED: &str = include_str!("../../../assets/scenarios/silo.expected");

const P0: PlayerId = PlayerId(0);

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("silo.ron")).unwrap()
}

/// Per tick after the step: (tick, stored, capacity).
fn run_scenario() -> (World, Vec<(u64, u64, u64)>) {
    let s = scenario();
    let mut world = s.build_world().unwrap();
    let mut rows = Vec::new();
    for _ in 0..s.ticks {
        world.step();
        rows.push((
            world.tick().0,
            world.ore_stored(P0),
            world.storage_capacity(P0),
        ));
    }
    (world, rows)
}

#[test]
fn the_final_checksum_is_the_pinned_value() {
    let expected = parse_checksum(EXPECTED).unwrap();
    let s = scenario();
    let outcome = run(&s, &RunOptions::for_scenario(&s)).unwrap();
    assert_eq!(outcome.final_checksum, expected);
}

#[test]
fn the_silo_adds_room_once_built_and_takes_it_away_when_sold_without_losing_ore() {
    let (world, rows) = run_scenario();
    let at = |t: u64| rows[(t - 1) as usize];
    // (1) The refinery alone: the store fills to 600 and stays there.
    let full = rows.iter().find(|r| r.1 == 600).unwrap().0;
    assert!(full < 2400, "full at {full}");
    for t in full..=2600 {
        assert_eq!((at(t).1, at(t).2), (600, 600), "tick {t}");
    }
    // (2) While the silo is under construction (12 seconds = 240 ticks) it holds nothing.
    let complete = rows.iter().find(|r| r.2 == 1000).unwrap().0;
    assert!(
        (2840..=2845).contains(&complete),
        "silo complete at {complete}"
    );
    for t in 2601..complete {
        assert_eq!((at(t).1, at(t).2), (600, 600), "under construction, tick {t}");
    }
    // (3) Standing: the capacity is 1000 and the store fills to it before the sale.
    assert_eq!(at(4399).1, 1000);
    // (4) Sold: the capacity is 600 again, the 1000 stored are kept and nothing more goes in.
    let sold = rows
        .iter()
        .skip(4399)
        .find(|r| r.2 == 600)
        .expect("the sale lowers the capacity")
        .0;
    assert!(sold <= 4400 + 120, "capacity back at 600 at {sold}");
    for t in sold..=6000 {
        assert_eq!((at(t).1, at(t).2), (1000, 600), "after the sale, tick {t}");
    }
    // Nothing spends: stored is what was delivered, and it never fell.
    assert_eq!(world.ore_delivered(P0), world.ore_stored(P0));
    for w in rows.windows(2) {
        assert!(w[1].1 >= w[0].1);
    }
}
