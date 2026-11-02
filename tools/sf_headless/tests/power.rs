//! Power (WP6.4) on the m1 map: the `power` scenario with its checksum pinned, and the story along the
//! run. See `assets/scenarios/power.ron` and `docs/rules/power.md`.
//!
//! Meridian (player 0): a construction centre, a power plant (slot 1, +100), a barracks (slot 2, -20)
//! and a refinery (slot 3, -30); four riflemen queued at tick 1 (100 ticks each at full speed). At
//! tick 150 a vehicle factory (slot 5, -40) and a radar (slot 6, -50) go up: used 140 of 100 made,
//! speed 714. A Veyran light tank (slot 7) shoots the plant from tick 160; its output falls with its
//! hit points (`100 * hp / 600`), the speed falls to the floor of 400, and the plant is destroyed at
//! tick 1451. At tick 1600 two new plants (slots 1 and 11) are built, at the floor speed: 750 ticks,
//! complete at 2350. Then made is 200 and everything is at full speed again.
//!
//! The numbers below are counted again here from the public API (the plants' hit points and the power
//! of each kind written out), not read from the code under test.

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::run::{RunOptions, run};
use sf_headless::scenario::Scenario;
use sf_sim::{EntityId, PlayerId, World};

const EXPECTED: &str = include_str!("../../../assets/scenarios/power.expected");

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const OLD_PLANT: EntityId = EntityId::new(1, 0);
const FLOOR: u64 = 400;
const PLANT_HP: u64 = 600;

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("power.ron")).unwrap()
}

/// The power of each structure kind of the frozen definitions, as written there.
fn power_of_kind(kind: u16) -> i64 {
    match kind {
        1 => 100,
        2 => -30,
        3 => -20,
        4 => -40,
        5 => -50,
        _ => 0,
    }
}

/// (made, used) of player 0 counted again from the standing structures.
fn recount(w: &World) -> (u64, u64) {
    let (mut made, mut used) = (0u64, 0u64);
    for v in w.snapshot().iter() {
        if v.owner != P0 || !w.stands(v.id) {
            continue;
        }
        let p = power_of_kind(v.kind.0);
        if p > 0 {
            made += p as u64 * u64::from(v.hp) / PLANT_HP;
        } else {
            used += p.unsigned_abs();
        }
    }
    (made, used)
}

/// The speed for these totals, written out: full when enough is made, else the share, not below the floor.
fn speed(made: u64, used: u64) -> u64 {
    if used == 0 || made >= used {
        1000
    } else {
        (made * 1000 / used).max(FLOOR)
    }
}

/// One row per tick, after the step: the tick, the totals, the speed, whether defences have power.
struct Row {
    tick: u64,
    made: u64,
    used: u64,
    speed: u64,
    defences: bool,
    counted: (u64, u64),
}

struct Run {
    rows: Vec<Row>,
    /// (tick, slot) of every unit that came out of a factory.
    produced: Vec<(u64, u32)>,
    /// (tick, slot) of every structure that became complete.
    completed: Vec<(u64, u32)>,
    /// The first tick the old plant was gone.
    plant_gone: Option<u64>,
    world: World,
}

fn play() -> Run {
    let s = scenario();
    let mut w = s.build_world().unwrap();
    let mut run = Run {
        rows: Vec::new(),
        produced: Vec::new(),
        completed: Vec::new(),
        plant_gone: None,
        world: s.build_world().unwrap(),
    };
    for _ in 0..s.ticks {
        // The totals of a step are counted from the state before it.
        let before = recount(&w);
        w.step();
        let t = w.tick().0;
        let p = w.power_of(P0).unwrap();
        run.rows.push(Row {
            tick: t,
            made: p.made,
            used: p.used,
            speed: u64::from(w.power_speed_permille(P0)),
            defences: w.defences_powered(P0),
            counted: before,
        });
        for u in w.produced() {
            run.produced.push((t, u.unit.index()));
        }
        for c in w.completions() {
            run.completed.push((t, c.id.index()));
        }
        if run.plant_gone.is_none() && t > 100 && w.hp_of(OLD_PLANT).is_none() {
            run.plant_gone = Some(t);
        }
    }
    run.world = w;
    run
}

fn row(run: &Run, tick: u64) -> &Row {
    run.rows.iter().find(|r| r.tick == tick).unwrap()
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
fn at_every_tick_the_totals_and_the_speed_follow_the_rules() {
    let r = play();
    for row in &r.rows {
        assert_eq!(
            (row.made, row.used),
            row.counted,
            "tick {}: the totals are those of the standing structures before the step",
            row.tick
        );
        assert_eq!(
            row.speed,
            speed(row.made, row.used),
            "tick {}: speed",
            row.tick
        );
        assert_eq!(row.defences, row.speed == 1000, "tick {}: defences", row.tick);
    }
}

#[test]
fn the_story_in_numbers() {
    let r = play();
    // A base with enough power.
    for t in [2, 50, 100, 150] {
        let x = row(&r, t);
        assert_eq!((x.made, x.used, x.speed, x.defences), (100, 50, 1000, true), "{t}");
    }
    // The radar and the vehicle factory are placed in the step of tick 150 (the tick counter says 151
    // after it): they count from the next step. 140 used of 100 made: 100 * 1000 / 140 = 714.
    assert_eq!(row(&r, 151).used, 50);
    for t in [152, 200, 300] {
        let x = row(&r, t);
        assert_eq!((x.made, x.used, x.speed, x.defences), (100, 140, 714, false), "{t}");
    }
    // The plant is shot: 100 * 558 / 600 = 93; 100 * 285 / 600 = 47 (47.5 down), already at the floor.
    let x = row(&r, 400);
    assert_eq!((x.made, x.used, x.speed), (93, 140, 664));
    let x = row(&r, 900);
    assert_eq!((x.made, x.used, x.speed), (47, 140, 400));
    // Destroyed: nothing is made, the floor holds.
    assert_eq!(r.plant_gone, Some(1451));
    for t in [1452, 1453, 1600, 2349] {
        let x = row(&r, t);
        assert_eq!((x.made, x.used, x.speed, x.defences), (0, 140, 400, false), "{t}");
    }
    // Two plants built at the floor speed: 300 ticks of work take 750.
    assert_eq!(r.completed, vec![(2350, 1), (2350, 11)]);
    assert_eq!(row(&r, 2350).made, 0, "counted from the next step");
    for t in [2351, 2400, 3200] {
        let x = row(&r, t);
        assert_eq!((x.made, x.used, x.speed, x.defences), (200, 140, 1000, true), "{t}");
    }
}

#[test]
fn the_riflemen_come_out_slower_in_the_shortage_and_at_the_normal_pace_after_it() {
    let r = play();
    let ticks: Vec<u64> = r.produced.iter().map(|p| p.0).collect();
    // 100 ticks at full speed; the second is half done when the shortage begins (714), the third
    // and fourth are built at 714 and then at the falling speed of the damaged plant; the two queued
    // at tick 2500 take 100 ticks each again.
    assert_eq!(ticks, vec![101, 222, 364, 523, 2600, 2700]);
    let slots: Vec<u32> = r.produced.iter().map(|p| p.1).collect();
    assert_eq!(slots, vec![4, 8, 9, 10, 12, 13]);
}

#[test]
fn the_price_of_everything_is_paid_exactly() {
    let r = play();
    // 4 + 2 riflemen at 100, two plants at 300: whatever the speed.
    assert_eq!(r.world.credits_spent(P0), 6 * 100 + 2 * 300);
    assert_eq!(r.world.credits(P0), 3000 - 1200);
}

#[test]
fn a_player_with_no_structures_is_not_in_a_shortage() {
    let r = play();
    assert_eq!(r.world.power_speed_permille(P1), 1000);
    assert!(r.world.defences_powered(P1));
    let t = r.world.power_of(P1).unwrap();
    assert_eq!((t.made, t.used), (0, 0));
}
