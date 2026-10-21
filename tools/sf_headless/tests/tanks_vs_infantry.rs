//! The `tanks_vs_infantry` scenario (WP5.4, `docs/rules/targeting.md`): ten light tanks against twenty infantry
//! (fourteen riflemen and six rocket troopers), both sides on automatic targeting and nobody given an attack
//! order. Each side attack-moves onto the other's ground; the units pick their own targets, stop to fight and
//! go on when it is over. This is the headless half of the phase-5 done-when: **the same outcome and the same
//! checksum every run**. The other half, that it looks that way in the game, comes with WP5.5 and WP5.6.
//!
//! The cast and the commands are written in `assets/scenarios/tanks_vs_infantry.ron` (which also lists the
//! entity ids used below), on a frozen copy of the shipped definitions (`tanks_vs_infantry_defs/`). The
//! checksum is pinned in `assets/scenarios/tanks_vs_infantry.expected` (see `pinned.rs`).
//!
//! Nothing is removed when a unit reaches zero hit points until WP5.5; the units at zero stay where they fell
//! (on hold), are nobody's target and fire no more, and the tests check that along the run.

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::scenario::Scenario;
use sf_math::Vec2;
use sf_sim::{EntityId, Order, Shot, World};

const EXPECTED: &str = include_str!("../../../assets/scenarios/tanks_vs_infantry.expected");

const ENTITIES: u32 = 30;
const TANKS: std::ops::Range<u32> = 0..10;
const INFANTRY: std::ops::Range<u32> = 10..30;
/// The kind numbers of the cast, as the scenario's frozen definitions give them.
const CANNON: u16 = 6;
const RIFLE: u16 = 0;
const ROCKET: u16 = 1;

fn id(slot: u32) -> EntityId {
    EntityId::new(slot, 0)
}

/// Everything a run reported, tick by tick.
struct Run {
    world: World,
    shots: Vec<Shot>,
    /// `hp[t][slot]` at the end of tick `t`.
    hp: Vec<Vec<u32>>,
    /// `pos[t][slot]` at the end of tick `t`.
    pos: Vec<Vec<Vec2>>,
    /// The checksum after each tick.
    checksums: Vec<u64>,
}

fn run() -> Run {
    let scenario = Scenario::load(&scenario_path("tanks_vs_infantry.ron")).unwrap();
    let mut world = scenario.build_world().unwrap();
    let (mut shots, mut hp, mut pos, mut checksums) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for _ in 0..scenario.ticks {
        let report = world.step();
        checksums.push(report.checksum);
        shots.extend_from_slice(world.shots());
        hp.push((0..ENTITIES).map(|s| world.hp_of(id(s)).unwrap()).collect());
        pos.push(
            (0..ENTITIES)
                .map(|s| world.get(id(s)).unwrap().pos)
                .collect(),
        );
    }
    Run {
        world,
        shots,
        hp,
        pos,
        checksums,
    }
}

impl Run {
    fn alive(&self, slots: std::ops::Range<u32>) -> Vec<u32> {
        slots
            .filter(|&s| self.world.hp_of(id(s)).unwrap() > 0)
            .collect()
    }

    /// The tick at the end of which `slot` first had no hit points.
    fn fell_at(&self, slot: u32) -> Option<usize> {
        self.hp.iter().position(|row| row[slot as usize] == 0)
    }
}

fn is_tank(e: EntityId) -> bool {
    TANKS.contains(&e.index())
}

// ---- the same outcome every run ---------------------------------------------------------------------------

#[test]
fn the_same_outcome_and_checksum_come_out_every_run() {
    let (a, b) = (run(), run());
    // Every tick of the run, not only the last: the checksum after each, the shots, the hit points and
    // the places of all thirty.
    assert_eq!(a.checksums, b.checksums);
    assert_eq!(a.shots, b.shots);
    assert_eq!(a.hp, b.hp);
    assert_eq!(a.pos, b.pos);
    assert_eq!(a.world.checksum(), b.world.checksum());
    // And it is the value committed next to the scenario, which CI requires on Linux and on Windows.
    assert_eq!(
        a.world.checksum(),
        parse_checksum(EXPECTED).expect("tanks_vs_infantry.expected is a checksum")
    );
}

#[test]
fn the_same_scenario_without_targeting_fires_nothing() {
    // Nobody has an attack order, so with the units not choosing their targets the two armies march
    // through each other's ground without a shot, and every unit keeps its hit points.
    let mut quiet = Scenario::load(&scenario_path("tanks_vs_infantry.ron")).unwrap();
    quiet.targeting = false;
    let mut world = quiet.build_world().unwrap();
    assert_eq!(world.targeting_config(), None);
    for _ in 0..quiet.ticks {
        world.step();
        assert!(world.shots().is_empty());
    }
    let full: u32 = TANKS.map(|s| world.hp_of(id(s)).unwrap()).sum();
    assert_eq!(full, 10 * 400);
    let people: u32 = INFANTRY.map(|s| world.hp_of(id(s)).unwrap()).sum();
    assert_eq!(people, 14 * 50 + 6 * 45);
    // And the scenario with it takes the settings of the simulation.
    let on = Scenario::load(&scenario_path("tanks_vs_infantry.ron")).unwrap();
    assert_eq!(
        on.build_world().unwrap().targeting_config(),
        Some(sf_sim::TargetingConfig::DEFAULT)
    );
}

#[test]
fn targeting_needs_combat() {
    let mut s = Scenario::load(&scenario_path("tanks_vs_infantry.ron")).unwrap();
    assert!(s.problems().is_empty());
    s.combat = false;
    let problems = s.problems();
    assert!(
        problems
            .iter()
            .any(|p| p.contains("targeting needs `combat`")),
        "{problems:?}"
    );
}

// ---- the outcome ------------------------------------------------------------------------------------------

#[test]
fn the_tanks_win_with_four_of_ten_left_and_every_infantryman_down() {
    let r = run();
    assert_eq!(
        r.alive(INFANTRY),
        Vec::<u32>::new(),
        "no infantryman is left standing"
    );
    assert_eq!(r.alive(TANKS), [1, 2, 3, 9]);
    // Hit points left, tank by tank (a light tank has 400): three untouched, one hurt.
    let hp = |s: u32| r.world.hp_of(id(s)).unwrap();
    assert_eq!([hp(1), hp(2), hp(3), hp(9)], [282, 400, 400, 400]);
    assert_eq!(TANKS.map(hp).sum::<u32>(), 1482);
    // Nothing was removed: all thirty are still in the world, six of them tanks at zero.
    assert!((0..ENTITIES).all(|s| r.world.is_alive(id(s))));
    assert_eq!(TANKS.filter(|&s| hp(s) == 0).count(), 6);
}

#[test]
fn the_fight_starts_at_tick_270_and_the_last_shot_is_at_tick_787() {
    let r = run();
    assert_eq!(r.shots.len(), 228);
    assert_eq!(r.shots.first().unwrap().tick.0, 270);
    assert_eq!(r.shots.last().unwrap().tick.0, 787);
    // The last shell was still in the air: the last infantryman falls at the end of tick 791.
    let last_fall = INFANTRY.filter_map(|s| r.fell_at(s)).max().unwrap();
    assert_eq!(last_fall, 791);
    // Nothing fires after that, and the world is at rest: no projectile in flight.
    assert_eq!(r.world.projectile_count(), 0);
    assert!(r.shots.iter().all(|s| s.tick.0 <= 787));
}

#[test]
fn who_fired_what() {
    let r = run();
    let by = |f: &dyn Fn(&Shot) -> bool| r.shots.iter().filter(|s| f(s)).count();
    // The tanks fire their cannons, all at infantry; the riflemen and the rocket troopers fire at tanks.
    assert_eq!(by(&|s| is_tank(s.attacker) && s.weapon == CANNON), 87);
    assert_eq!(by(&|s| is_tank(s.attacker)), 87);
    assert_eq!(by(&|s| is_tank(s.target)), 97 + 44);
    assert_eq!(by(&|s| !is_tank(s.attacker) && s.weapon == RIFLE), 97);
    assert_eq!(by(&|s| !is_tank(s.attacker) && s.weapon == ROCKET), 44);
    assert!(
        r.shots
            .iter()
            .all(|s| is_tank(s.attacker) != is_tank(s.target))
    );
}

// ---- what the rules say, along the run --------------------------------------------------------------------

#[test]
fn nobody_fires_at_its_own_side_and_nobody_at_zero_fires_or_is_fired_at() {
    let r = run();
    let owner = |e: EntityId| r.world.get(e).unwrap().owner;
    for s in &r.shots {
        assert_ne!(owner(s.attacker), owner(s.target), "{s:?}");
        // The hit points at the end of the tick before the shot: the start of the weapon stage.
        let t = s.tick.0 as usize;
        if t > 0 {
            assert!(
                r.hp[t - 1][s.attacker.index() as usize] > 0,
                "a unit at zero fired: {s:?}"
            );
            assert!(
                r.hp[t - 1][s.target.index() as usize] > 0,
                "a unit at zero was shot at: {s:?}"
            );
        }
    }
}

#[test]
fn a_unit_at_zero_stays_where_it_fell_on_hold() {
    let r = run();
    let last = r.hp.len() - 1;
    let mut fallen = 0;
    for slot in 0..ENTITIES {
        let Some(t) = r.fell_at(slot) else { continue };
        fallen += 1;
        for later in t..=last {
            assert_eq!(
                r.pos[later][slot as usize], r.pos[t][slot as usize],
                "unit {slot} moved after it fell at tick {t}"
            );
        }
        assert_eq!(
            r.world.order_of(id(slot)),
            Some(Order::HoldPosition),
            "unit {slot}"
        );
    }
    assert_eq!(fallen, 26, "six tanks and twenty infantrymen");
}

#[test]
fn the_tanks_that_are_left_go_on_to_the_ground_they_were_sent_to() {
    let r = run();
    let goal = Vec2::from_ints(56, 30) + Vec2::splat(sf_math::Fx::HALF);
    for slot in r.alive(TANKS) {
        let v = r.world.get(id(slot)).unwrap();
        assert_eq!(
            r.world.order_of(id(slot)),
            Some(Order::Idle),
            "tank {slot} has arrived"
        );
        assert!(
            (v.pos - goal).length() < sf_math::Fx::from_int(4),
            "tank {slot} stands at {:?}",
            v.pos
        );
        assert!(r.world.engagement_of(id(slot)).is_none());
    }
}
