//! Ore storage (WP7.3) on the shipped harvest layout: the `storage` scenario (a refinery that
//! stores 600, two harvesters, a second refinery at tick 2600 that is removed at tick 4200) with
//! its checksum pinned. See `assets/scenarios/storage.ron` and `docs/rules/economy.md`
//! ("Storage").

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::run::{RunOptions, run};
use sf_headless::scenario::Scenario;
use sf_sim::{EntityId, HarvestStage, PlayerId, World};

const EXPECTED: &str = include_str!("../../../assets/scenarios/storage.expected");

const P0: PlayerId = PlayerId(0);

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("storage.ron")).unwrap()
}

/// Per tick after the step: (tick, stored, capacity, stage of slot 1, stage of slot 2, loads).
type Row = (
    u64,
    u64,
    u64,
    Option<HarvestStage>,
    Option<HarvestStage>,
    u16,
    u16,
);

fn run_scenario() -> (World, Vec<Row>) {
    let s = scenario();
    let mut world = s.build_world().unwrap();
    let mut rows = Vec::new();
    for _ in 0..s.ticks {
        world.step();
        let h = |slot| world.harvester_of(EntityId::new(slot, 0));
        rows.push((
            world.tick().0,
            world.ore_stored(P0),
            world.storage_capacity(P0),
            h(1).map(|v| v.stage),
            h(2).map(|v| v.stage),
            h(1).map_or(0, |v| v.load),
            h(2).map_or(0, |v| v.load),
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

fn at(rows: &[Row], tick: u64) -> &Row {
    &rows[(tick - 1) as usize]
}

#[test]
fn the_store_fills_blocks_grows_with_a_refinery_and_shrinks_without_losing_ore() {
    let (world, rows) = run_scenario();
    // Nothing is ever taken into a store that is full: every rise ends at or below the capacity.
    for w in rows.windows(2) {
        if w[1].1 > w[0].1 {
            assert!(w[1].1 <= w[1].2, "tick {}: stored {} over {}", w[1].0, w[1].1, w[1].2);
        }
        // Stored never falls (nothing spends, a smaller capacity keeps what is in).
        assert!(w[1].1 >= w[0].1);
    }
    // (1) One refinery: the store fills to 600 and the harvester in the bay is blocked, holding
    // its whole load, until the second refinery stands at tick 2600.
    let full = rows.iter().find(|r| r.1 == 600).unwrap().0;
    assert!(full < 2400, "full at {full}");
    for t in full..2600 {
        let r = at(&rows, t);
        assert_eq!((r.1, r.2), (600, 600), "tick {t}");
    }
    let blocked = (2400..2600).all(|t| {
        let r = at(&rows, t);
        r.4 == Some(HarvestStage::Unloading) && r.6 == 200
    });
    assert!(blocked, "slot 2 waits in the bay with its load while the store is full");
    // (2) The second refinery raises the capacity to 1200 and the store fills to it.
    assert_eq!(at(&rows, 2601).2, 1200);
    assert!(at(&rows, 2700).1 > 600, "the blocked harvester unloads once there is room");
    assert_eq!(at(&rows, 4199).1, 1200);
    // (3) Its removal lowers the capacity to 600; the 1200 stored are kept and nothing more is
    // taken: one harvester waits in the bay with a full load, the other queues behind it.
    for t in 4201..=6000 {
        let r = at(&rows, t);
        assert_eq!((r.1, r.2), (1200, 600), "tick {t}");
    }
    let last = at(&rows, 6000);
    assert_eq!(
        (last.3, last.5, last.4, last.6),
        (
            Some(HarvestStage::Unloading),
            200,
            Some(HarvestStage::Queued),
            200
        )
    );
    // With nothing spending, the store is what was delivered.
    assert_eq!(world.ore_delivered(P0), world.ore_stored(P0));
}
