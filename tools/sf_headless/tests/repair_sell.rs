//! The `repair_sell` scenario (WP6.6, `docs/rules/repair_sell.md`): two Veyran light tanks shoot a Meridian
//! power plant and barracks and are removed; the plant is repaired to full hit points with exact money, the
//! repair runs out of money in the middle (the owner has 20 credits) and goes on when the sale of the
//! damaged barracks pays; the opponent tries to repair and sell the plant and is refused; the opponent sells
//! its only structure and is defeated.
//!
//! The cast and the ticks are written in `assets/scenarios/repair_sell.ron` (numbers: 5 hit points per tick,
//! a full repair costs 400 thousandths of the price, a sale takes 60 ticks and pays 700 thousandths). The
//! amounts below are worked out by hand from those numbers and the prices of the definitions (power plant 300
//! credits and 600 hit points, barracks 400 credits and 700 hit points), not read back from the simulation.
//! The checksum is pinned in `assets/scenarios/repair_sell.expected` (CI runs it on Linux and Windows).

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::scenario::Scenario;
use sf_sim::{EntityId, PlayerId, RepairError, RepairStatus, World};

const EXPECTED: &str = include_str!("../../../assets/scenarios/repair_sell.expected");

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn id(slot: u32) -> EntityId {
    EntityId::new(slot, 0)
}

const PLANT: u32 = 1;
const BARRACKS: u32 = 2;
const OPPONENT_CENTRE: u32 = 4;

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("repair_sell.ron")).unwrap()
}

/// Runs the scenario to the end of tick `tick` (inclusive).
fn to(w: &mut World, tick: u64) {
    while w.tick().0 <= tick {
        w.step();
    }
}

/// The cumulative cost of a repair of the plant up to `h` hit points, by hand:
/// `300 * 400 * h / (600 * 1000)`, which is `h / 5` rounded down.
fn plant_cum(h: u32) -> u32 {
    h / 5
}

#[test]
fn the_tanks_hurt_the_plant_and_the_barracks_and_the_opponent_is_refused() {
    let mut w = scenario().build_world().unwrap();
    to(&mut w, 300);
    assert_eq!(w.hp_of(id(PLANT)), Some(432), "600 - 8 shots of 21");
    assert_eq!(w.hp_of(id(BARRACKS)), Some(532), "700 - 8 shots of 21");
    assert_eq!(w.credits(P0), 20);
    to(&mut w, 399);
    assert_eq!(
        w.hp_of(id(PLANT)),
        Some(432),
        "nothing changes without a repair"
    );
    // Tick 400: the opponent tries to repair and to sell a structure that is not its own.
    to(&mut w, 400);
    let refused: Vec<_> = w
        .repair_rejections()
        .iter()
        .map(|r| (r.player, r.structure, r.reason))
        .collect();
    assert_eq!(
        refused,
        vec![
            (P1, id(PLANT), RepairError::NotYours),
            (P1, id(PLANT), RepairError::NotYours)
        ]
    );
    assert!(!w.is_repairing(id(PLANT)));
    assert!(w.sale_of(id(PLANT)).is_none());
    assert_eq!(w.hp_of(id(PLANT)), Some(432));
}

#[test]
fn the_repair_is_paid_a_credit_per_five_hit_points_and_waits_when_the_money_is_gone() {
    let mut w = scenario().build_world().unwrap();
    to(&mut w, 410);
    // The command ran in tick 410: nothing gained yet.
    assert!(w.is_repairing(id(PLANT)));
    assert_eq!((w.hp_of(id(PLANT)), w.credits(P0)), (Some(432), 20));
    // From tick 411 on: 5 hit points a tick and the cumulative cost (hp / 5) taken tick by tick.
    for n in 1..=20u32 {
        w.step();
        let hp = 432 + 5 * n;
        assert_eq!(w.hp_of(id(PLANT)), Some(hp), "tick {}", 410 + n);
        assert_eq!(
            w.credits(P0),
            20 - (plant_cum(hp) - plant_cum(432)) as u64,
            "tick {}",
            410 + n
        );
    }
    // Tick 430: 532 hit points, the last credit spent. The next payment is a credit more.
    assert_eq!((w.hp_of(id(PLANT)), w.credits(P0)), (Some(532), 0));
    assert_eq!(w.repair_status(id(PLANT)), Some(RepairStatus::Repairing));
    w.step();
    assert_eq!(
        w.repair_status(id(PLANT)),
        Some(RepairStatus::WaitingForMoney)
    );
    for _ in 0..50 {
        w.step();
        assert_eq!((w.hp_of(id(PLANT)), w.credits(P0)), (Some(532), 0));
        assert!(w.is_repairing(id(PLANT)));
    }
}

#[test]
fn the_sale_of_the_damaged_barracks_pays_by_hit_points_and_the_repair_goes_on() {
    let mut w = scenario().build_world().unwrap();
    to(&mut w, 430);
    assert_eq!(
        w.power_of(P0).unwrap().used,
        50,
        "barracks 20 + refinery 30"
    );
    // Tick 480: the sale starts. It stands no more and is gone 60 ticks later.
    to(&mut w, 480);
    assert!(w.stands(id(PLANT)) && !w.stands(id(BARRACKS)));
    assert_eq!(w.sale_of(id(BARRACKS)).map(|s| s.left), Some(60));
    // The totals are counted at the start of the next step: no barracks, and the plant at 532 of 600.
    w.step();
    let power = w.power_of(P0).unwrap();
    assert_eq!(power.used, 30, "only the refinery now");
    assert_eq!(power.made, 88, "100 * 532 / 600, rounded down");
    to(&mut w, 539);
    assert!(w.hp_of(id(BARRACKS)).is_some());
    assert_eq!(w.credits(P0), 0);
    assert_eq!(w.sale_of(id(BARRACKS)).map(|s| s.left), Some(1));
    // Tick 540: gone. 400 * 700 * 532 / (700 * 1000) = 212.8: 212 credits, not the 280 of a whole barracks.
    w.step();
    assert_eq!(w.tick().0, 541);
    assert_eq!(w.hp_of(id(BARRACKS)), None);
    assert_eq!(w.credits(P0), 212);
    let sales = w.sales();
    assert_eq!(sales.len(), 1);
    assert_eq!((sales[0].tick.0, sales[0].refund), (540, 212));
    // The waiting repair takes the next tick that pays: 537 hit points, one credit.
    w.step();
    assert_eq!((w.hp_of(id(PLANT)), w.credits(P0)), (Some(537), 211));
    assert_eq!(w.repair_status(id(PLANT)), Some(RepairStatus::Repairing));
    // It ends at tick 554 on 600 hit points and the flag goes. The whole repair cost 120 - 86 = 34
    // credits: the 20 the owner had and 14 of the refund.
    to(&mut w, 553);
    assert_eq!(w.hp_of(id(PLANT)), Some(597));
    w.step();
    assert_eq!(w.hp_of(id(PLANT)), Some(600));
    assert!(!w.is_repairing(id(PLANT)));
    assert_eq!(w.credits(P0), 212 - 14);
    assert_eq!(
        20 + 212 - w.credits(P0),
        u64::from(plant_cum(600) - plant_cum(432))
    );
    // The plant makes all of its power again from the next count.
    w.step();
    assert_eq!(w.power_of(P0).unwrap().made, 100);
}

#[test]
fn the_opponent_sells_its_last_structure_and_is_defeated_in_the_tick_it_starts() {
    let mut w = scenario().build_world().unwrap();
    to(&mut w, 699);
    assert!(w.defeats().is_empty());
    assert!(w.match_result().is_none());
    to(&mut w, 700);
    // It counts as gone from the first tick, and the outcome rule sees it in that tick.
    assert_eq!(w.defeats(), &[(P1, sf_sim::Tick(700))]);
    let result = w.match_result().unwrap();
    assert_eq!(result.tick.0, 700);
    assert_eq!(result.verdict, sf_sim::Verdict::Won(P0));
    // The structure is still there while it is sold, and goes 60 ticks later without a refund (it has no price).
    assert!(w.hp_of(id(OPPONENT_CENTRE)).is_some());
    to(&mut w, 759);
    assert!(w.hp_of(id(OPPONENT_CENTRE)).is_some());
    to(&mut w, 760);
    assert!(w.hp_of(id(OPPONENT_CENTRE)).is_none());
    assert_eq!(w.sales().len(), 1);
    assert_eq!(w.sales()[0].refund, 0);
    assert_eq!(w.credits(P1), 0);
}

#[test]
fn the_pinned_checksum_is_the_one_of_the_run_and_the_run_replays() {
    let s = scenario();
    let run = || {
        let mut w = s.build_world().unwrap();
        let mut last = 0;
        for _ in 0..s.ticks {
            last = w.step().checksum;
        }
        last
    };
    let first = run();
    assert_eq!(first, run(), "the same scenario ends on the same checksum");
    assert_eq!(first, parse_checksum(EXPECTED).unwrap());
}
