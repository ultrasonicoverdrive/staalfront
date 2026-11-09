//! Production (WP6.5) on the playfield: the `production` scenario with its checksum pinned. See
//! `assets/scenarios/production.ron` and `docs/rules/production.md`.
//!
//! Slot 1 is a barracks that is destroyed at tick 60, slot 2 the barracks (rifleman: 100 credits
//! over 100 ticks, exit cell (86, 81), rally (80, 92)), slot 3 the vehicle factory (light tank:
//! 700 credits over 400 ticks, exit cell (93, 76), rally (92, 92)). Player 0 starts with 400
//! credits; a power plant closes the barracks' exit from tick 250 to tick 1500.

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::run::{RunOptions, run};
use sf_headless::scenario::Scenario;
use sf_sim::{Cell, EntityId, FactoryStatus, PlayerId, World, tick_cost};

const EXPECTED: &str = include_str!("../../../assets/scenarios/production.expected");

const P0: PlayerId = PlayerId(0);
const START: u64 = 400;
const DOOMED: EntityId = EntityId::new(1, 0);
const BARRACKS: EntityId = EntityId::new(2, 0);
const FACTORY: EntityId = EntityId::new(3, 0);
const RIFLEMAN: u16 = 12;
const TANK: u16 = 19;
const BLOCK_FROM: u64 = 250;
const BLOCK_TO: u64 = 1500;

/// One row per tick, after the step.
struct Row {
    tick: u64,
    credits: u64,
    spent: u64,
    delivered: u64,
    /// Status, queue length and progress of the barracks and the vehicle factory.
    barracks: (FactoryStatus, usize, u16),
    factory: (FactoryStatus, usize, u16),
    /// Credits paid for the front item of each.
    front_paid: [u64; 2],
}

/// A unit that came out: the tick of the step, the factory slot, the kind, the unit and its cell.
#[derive(Clone, Copy, Debug)]
struct Out {
    tick: u64,
    factory: u32,
    kind: u16,
    unit: EntityId,
    cell: Cell,
}

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("production.ron")).unwrap()
}

fn run_scenario() -> (World, Vec<Row>, Vec<Out>) {
    let s = scenario();
    let mut w = s.build_world().unwrap();
    let mut rows = Vec::new();
    let mut out = Vec::new();
    for _ in 0..s.ticks {
        w.step();
        for p in w.produced() {
            out.push(Out {
                tick: p.tick.0,
                factory: p.factory.index(),
                kind: p.kind.0,
                unit: p.unit,
                cell: p.cell,
            });
        }
        let view = |id| {
            w.production_of(id)
                .map(|v| ((v.status, v.queue.len(), v.front_permille), v.front_paid))
                .unwrap_or(((FactoryStatus::Idle, 0, 0), 0))
        };
        let (b, bp) = view(BARRACKS);
        let (f, fp) = view(FACTORY);
        rows.push(Row {
            tick: w.tick().0,
            credits: w.credits(P0),
            spent: w.credits_spent(P0),
            delivered: w.ore_delivered(P0),
            barracks: b,
            factory: f,
            front_paid: [bp, fp],
        });
    }
    (w, rows, out)
}

/// The row after the step that ran tick `t - 1`, i.e. the world at tick `t`.
fn at(rows: &[Row], tick: u64) -> &Row {
    &rows[(tick - 1) as usize]
}

fn sum(cost: u32, total: u32, ticks: u64) -> u64 {
    (0..ticks as u32).map(|k| tick_cost(cost, total, k)).sum()
}

#[test]
fn the_final_checksum_is_the_pinned_value() {
    let expected = parse_checksum(EXPECTED).unwrap();
    let s = scenario();
    let outcome = run(&s, &RunOptions::for_scenario(&s)).unwrap();
    assert_eq!(outcome.final_checksum, expected);
    // Every command of the file was taken: nothing was refused.
    assert_eq!(outcome.commands_ignored, 0);
}

#[test]
fn the_books_balance_and_every_credit_spent_is_progress_on_a_unit() {
    let (_, rows, out) = run_scenario();
    let doomed = sum(100, 100, 59);
    for r in &rows {
        // The store is the pool: what the player started with, plus the ore delivered (a credit a
        // unit), minus what was spent.
        assert_eq!(r.credits, START + r.delivered - r.spent, "tick {}", r.tick);
        // And what was spent is the price of the units out, the progress on the two fronts, and
        // the 59 ticks of the destroyed barracks (not refunded).
        let done = |factory: u32, kind: u16| {
            out.iter()
                .filter(|o| o.tick < r.tick && o.factory == factory && o.kind == kind)
                .count() as u64
        };
        let paid = done(2, RIFLEMAN) * 100
            + done(3, TANK) * 700
            + r.front_paid[0]
            + r.front_paid[1]
            + if r.tick >= 2 {
                doomed.min(sum(100, 100, r.tick - 1))
            } else {
                0
            };
        // Units out of the barracks slot after the destruction (slot 1 is reused) are not its.
        assert_eq!(r.spent, paid, "tick {}", r.tick);
    }
}

#[test]
fn both_factories_pay_in_queues_of_their_own_from_the_first_tick() {
    let (_, rows, _) = run_scenario();
    // Three queues (the doomed barracks too) pay in each of the first 59 ticks, the way a
    // structure does: a rifleman 100 over 100, a light tank 700 over 400.
    for t in 2..=60 {
        let r = at(&rows, t);
        let n = t - 1;
        assert_eq!(r.spent, 2 * sum(100, 100, n) + sum(700, 400, n), "tick {t}");
        assert_eq!(r.barracks.1, 3, "the whole queue is still there");
        assert_eq!(r.factory.1, 2);
        assert_eq!(r.barracks.0, FactoryStatus::Building);
        assert_eq!(r.factory.0, FactoryStatus::Building);
        // Each front item has its own progress in thousandths of its own build time.
        assert_eq!(u64::from(r.barracks.2), n * 1000 / 100);
        assert_eq!(u64::from(r.factory.2), n * 1000 / 400);
    }
}

#[test]
fn the_destroyed_factory_loses_its_queue_and_its_money_is_not_refunded() {
    let (world, rows, out) = run_scenario();
    // Until tick 60 it has two riflemen queued and a rifleman 59 percent paid.
    // After it nothing of it is left: no record, nothing out of it, the 59 credits stay spent.
    assert!(world.production_of(DOOMED).is_none());
    assert!(world.factories().all(|f| f != DOOMED));
    assert!(out.iter().all(|o| o.factory != 1 || o.tick > 60));
    let r = at(&rows, 61);
    assert_eq!(
        r.spent,
        sum(100, 100, 59) + sum(100, 100, 60) + sum(700, 400, 60)
    );
    // Nothing of the waiting second rifleman was ever paid for: 59 ticks are 59 credits.
    assert_eq!(sum(100, 100, 59), 59);
}

#[test]
fn the_money_runs_out_and_production_waits_until_the_ore_comes() {
    let (_, rows, _) = run_scenario();
    // Gone by about tick 125; both factories then wait and nothing moves, not one tick and not one
    // credit, for as long as it takes the harvesters to bring the first load.
    let broke = rows.iter().find(|r| r.credits == 0).unwrap().tick;
    assert!(broke > 100 && broke < 200, "broke at {broke}");
    let first_delivery = rows.iter().find(|r| r.delivered > 0).unwrap().tick;
    assert!(first_delivery > 800, "first ore at {first_delivery}");
    let held = at(&rows, broke + 20);
    assert_eq!(held.barracks.0, FactoryStatus::WaitingForMoney);
    assert_eq!(held.factory.0, FactoryStatus::WaitingForMoney);
    for t in broke + 20..first_delivery {
        let r = at(&rows, t);
        assert_eq!((r.credits, r.spent), (0, held.spent), "tick {t}");
        assert_eq!(r.barracks.2, held.barracks.2, "tick {t}");
        assert_eq!(r.factory.2, held.factory.2, "tick {t}");
    }
    // The lower slot is served first: the barracks, not the vehicle factory, takes the first
    // credits that come in (its front progress moves before the factory's).
    let moved_b = rows
        .iter()
        .find(|r| r.tick > first_delivery && r.barracks.2 != held.barracks.2)
        .unwrap()
        .tick;
    let moved_f = rows
        .iter()
        .find(|r| r.tick > first_delivery && r.factory.2 != held.factory.2)
        .unwrap()
        .tick;
    assert!(moved_b <= moved_f, "{moved_b} {moved_f}");
    assert!(moved_b <= first_delivery + 5);
}

#[test]
fn a_unit_comes_out_on_the_exit_cell_and_drives_to_the_rally_point() {
    let (world, _, out) = run_scenario();
    let first = out.iter().find(|o| o.factory == 2).unwrap();
    // 100 ticks after the order, on the cell next to the middle of the +x side of (84, 80) 2 x 3.
    assert_eq!((first.tick, first.kind), (100, RIFLEMAN));
    assert_eq!(first.cell, Cell::new(86, 81));
    // It walked to (80, 92) and stands there; the one that came out of the vehicle factory
    // stands on its rally cell (92, 92).
    assert_eq!(
        world.cell_at(world.get(first.unit).unwrap().pos),
        Some(Cell::new(80, 92))
    );
    let tank = out.iter().find(|o| o.kind == TANK).unwrap();
    assert_eq!(tank.cell, Cell::new(93, 76));
    assert_eq!(
        world.cell_at(world.get(tank.unit).unwrap().pos),
        Some(Cell::new(92, 92))
    );
    // Everything that came out of the barracks was sent the same way.
    for o in out.iter().filter(|o| o.factory != 1 && o.kind == RIFLEMAN) {
        assert!(matches!(
            world.order_of(o.unit),
            Some(sf_sim::Order::Move(_)) | Some(sf_sim::Order::Idle)
        ));
    }
}

#[test]
fn a_blocked_exit_holds_the_queue_until_the_block_goes() {
    let (_, rows, out) = run_scenario();
    // The second rifleman is finished by the time the ore has paid for it, and finds the free
    // cells round the exit taken by the power plant: it waits inside, full, queue not lost.
    let waiting: Vec<u64> = rows
        .iter()
        .filter(|r| r.barracks.0 == FactoryStatus::WaitingForExit)
        .map(|r| r.tick)
        .collect();
    assert!(!waiting.is_empty());
    let (from, to) = (waiting[0], *waiting.last().unwrap());
    assert!(from > BLOCK_FROM && to <= BLOCK_TO, "{from}..{to}");
    // One unbroken stretch from finishing to the tick the block goes.
    assert_eq!(waiting.len() as u64, to - from + 1);
    assert_eq!(to, BLOCK_TO);
    for t in from..=to {
        let r = at(&rows, t);
        assert_eq!(r.barracks.1, 2, "tick {t}: the queue holds");
        assert_eq!(r.barracks.2, 1000, "tick {t}: the unit is finished");
    }
    // Nothing came out of the barracks in that time, and it paid nothing for the third one.
    assert!(
        out.iter()
            .all(|o| o.factory != 2 || o.tick < from || o.tick >= BLOCK_TO)
    );
    // The tick the block is despawned the waiting unit comes out and the next starts the tick after.
    let freed = out
        .iter()
        .find(|o| o.factory == 2 && o.tick >= BLOCK_TO)
        .unwrap();
    assert_eq!(freed.tick, BLOCK_TO);
    assert_eq!(freed.cell, Cell::new(86, 81));
    assert_eq!(at(&rows, BLOCK_TO + 1).barracks.1, 1);
}

#[test]
fn the_vehicle_factory_has_a_queue_of_its_own_and_finishes_a_tank() {
    let (_, rows, out) = run_scenario();
    let tanks: Vec<&Out> = out.iter().filter(|o| o.kind == TANK).collect();
    assert!(!tanks.is_empty());
    assert!(tanks.iter().all(|o| o.factory == 3));
    // It was never held up by the barracks' blocked exit.
    assert_eq!(at(&rows, 1200).factory.1, 2);
    assert!(rows.iter().any(|r| r.factory.1 == 1));
}
