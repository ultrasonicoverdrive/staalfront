//! Money (WP7.4) on the storage layout: the `money` scenario with its checksum pinned. See
//! `assets/scenarios/money.ron` and `docs/rules/economy.md` ("Money").
//!
//! Player 0 starts with 300 credits and two power plants (300 credits over 300 ticks each, so one
//! credit a tick each) go up at once; a refinery of capacity 600 and two harvesters. At tick 4650,
//! with the store full, a barracks (400 over 400 ticks) is built.

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::run::{RunOptions, run};
use sf_headless::scenario::Scenario;
use sf_sim::{EntityId, HarvestStage, PlayerId, World};

const EXPECTED: &str = include_str!("../../../assets/scenarios/money.expected");

const P0: PlayerId = PlayerId(0);
const START: u64 = 300;
const PLANT_A: u32 = 4;
const PLANT_B: u32 = 5;
const BARRACKS: u32 = 6;
const BARRACKS_TICK: u64 = 4650;

/// One row per tick, after the step.
struct Row {
    tick: u64,
    credits: u64,
    spent: u64,
    delivered: u64,
    stalled: Vec<u32>,
    left: [u32; 3],
    /// Stage and load of the harvesters in slots 2 and 3.
    harvesters: [Option<(HarvestStage, u16)>; 2],
}

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("money.ron")).unwrap()
}

fn run_scenario() -> (World, Vec<Row>) {
    let s = scenario();
    let mut w = s.build_world().unwrap();
    let mut rows = Vec::new();
    for _ in 0..s.ticks {
        w.step();
        let left = |slot| {
            w.construction_of(EntityId::new(slot, 0))
                .map_or(0, |c| c.left)
        };
        let h = |slot| {
            w.harvester_of(EntityId::new(slot, 0))
                .map(|v| (v.stage, v.load))
        };
        rows.push(Row {
            tick: w.tick().0,
            credits: w.credits(P0),
            spent: w.credits_spent(P0),
            delivered: w.ore_delivered(P0),
            stalled: w.stalled().iter().map(|e| e.index()).collect(),
            left: [left(PLANT_A), left(PLANT_B), left(BARRACKS)],
            harvesters: [h(2), h(3)],
        });
    }
    (w, rows)
}

fn at(rows: &[Row], tick: u64) -> &Row {
    &rows[(tick - 1) as usize]
}

#[test]
fn the_final_checksum_is_the_pinned_value() {
    let expected = parse_checksum(EXPECTED).unwrap();
    let s = scenario();
    let outcome = run(&s, &RunOptions::for_scenario(&s)).unwrap();
    assert_eq!(outcome.final_checksum, expected);
}

#[test]
fn the_books_balance_and_every_credit_spent_is_a_tick_of_construction() {
    let (_, rows) = run_scenario();
    for r in &rows {
        // The store is the pool: what the player started with, plus the ore delivered (one credit
        // a unit), minus what was spent.
        assert_eq!(r.credits, START + r.delivered - r.spent, "tick {}", r.tick);
        // All three structures cost one credit for each tick they have gone up.
        let barracks = if r.tick <= BARRACKS_TICK {
            0
        } else if r.left[2] == 0 {
            400
        } else {
            u64::from(400 - r.left[2])
        };
        let plants = |left: u32| if left == 0 { 300 } else { u64::from(300 - left) };
        // A plant that is complete has left 0 as has one that is not yet placed (tick 0 only).
        let paid = barracks
            + if r.tick >= 2 {
                plants(r.left[0]) + plants(r.left[1])
            } else {
                0
            };
        assert_eq!(r.spent, paid, "tick {}", r.tick);
    }
}

#[test]
fn the_credits_drain_over_the_build_time_then_construction_pauses_at_zero() {
    let (_, rows) = run_scenario();
    // Placed at tick 1; the first payment is the next tick. Two plants at one credit a tick each.
    for t in 3..=152 {
        let r = at(&rows, t);
        let n = t - 2;
        assert_eq!(r.credits, START - 2 * n, "tick {t}");
        assert_eq!(r.left, [300 - n as u32, 300 - n as u32, 0], "tick {t}");
        assert!(r.stalled.is_empty(), "tick {t}");
    }
    assert_eq!(at(&rows, 152).credits, 0);
    // No ore has been delivered yet: both plants stand still at 150 ticks to go, not one tick
    // moves and not one credit, for as long as it takes the harvesters to bring the first load.
    let first_delivery = rows.iter().find(|r| r.delivered > 0).unwrap().tick;
    assert!(first_delivery > 800, "first ore at {first_delivery}");
    for t in 153..first_delivery {
        let r = at(&rows, t);
        assert_eq!((r.credits, r.spent), (0, 300), "tick {t}");
        assert_eq!(r.left, [150, 150, 0], "tick {t}");
        assert_eq!(r.stalled, [PLANT_A, PLANT_B], "tick {t}");
    }
}

#[test]
fn harvesting_raises_the_credits_and_construction_goes_on_in_bursts_and_completes() {
    let (_, rows) = run_scenario();
    let first_delivery = rows.iter().find(|r| r.delivered > 0).unwrap().tick;
    // The tick the ore arrives, the plants move again, from where they stopped.
    let moved = rows
        .iter()
        .find(|r| r.tick > 152 && r.left[0] < 150)
        .unwrap()
        .tick;
    assert!(moved >= first_delivery && moved <= first_delivery + 3);
    // They pause again when the load is used up (the credits are 0 and the plants are not
    // complete), and go on with the next load.
    let paused_again = rows
        .iter()
        .any(|r| r.tick > moved && r.credits == 0 && !r.stalled.is_empty());
    assert!(paused_again);
    // Both are complete after the second load, never before the first.
    let done = rows
        .iter()
        .find(|r| r.tick > 152 && r.left[0] == 0 && r.left[1] == 0)
        .unwrap()
        .tick;
    assert!(done > first_delivery && done < 1500, "complete at {done}");
    assert_eq!(at(&rows, done).spent, 600);
    // Credits only ever rise by a delivery or fall by a payment: never both in a way that hides
    // a payment (a tick with a payment has its spent counter up).
    for w in rows.windows(2) {
        if w[1].credits < w[0].credits {
            assert!(w[1].spent > w[0].spent, "tick {}", w[1].tick);
        }
        if w[1].credits > w[0].credits {
            assert!(w[1].delivered > w[0].delivered, "tick {}", w[1].tick);
        }
    }
}

#[test]
fn spending_makes_room_in_a_full_store_and_the_waiting_harvester_unloads() {
    let (world, rows) = run_scenario();
    // Before the barracks the store is full at the capacity and a harvester waits in the bay with
    // its load, not one unit moving for 50 ticks.
    let before = at(&rows, BARRACKS_TICK);
    assert_eq!(before.credits, 600);
    assert_eq!(before.harvesters[1], Some((HarvestStage::Unloading, 100)));
    assert_eq!(before.harvesters[0].map(|h| h.0), Some(HarvestStage::Queued));
    for t in BARRACKS_TICK - 50..=BARRACKS_TICK {
        let r = at(&rows, t);
        assert_eq!((r.credits, r.delivered), (600, before.delivered), "tick {t}");
        assert_eq!(r.harvesters[1], Some((HarvestStage::Unloading, 100)));
    }
    // The payments for the barracks free room and ore goes in while it is going up.
    let first = rows
        .iter()
        .find(|r| r.tick > BARRACKS_TICK && r.delivered > before.delivered)
        .unwrap();
    assert!(first.tick < BARRACKS_TICK + 20, "ore at {}", first.tick);
    assert!(first.left[2] > 0, "still going up");
    // The credits never exceed the capacity while it goes up (nothing is taken above it).
    for r in rows.iter().filter(|r| r.tick > BARRACKS_TICK) {
        assert!(r.credits <= 600, "tick {}", r.tick);
    }
    // It is paid in full and stands.
    let barracks = EntityId::new(BARRACKS, 0);
    assert!(world.stands(barracks));
    assert_eq!(world.credits_spent(P0), 1000);
}
