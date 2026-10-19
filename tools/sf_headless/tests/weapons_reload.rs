//! The `weapons_reload` scenario (WP5.1, `docs/rules/weapons.md`): units with an attack order
//! walk into reach and fire their weapons at the victim, an instant hit that takes the damage off
//! its hit points (the placeholder of WP5.1; WP5.2 to WP5.4 replace it). Asserted: one shot per
//! reload window to the tick, a burst of two a fixed gap apart, a minimum range, a victim shot
//! down to zero hit points and then left alone, a reload that an order change does not reset, and
//! that the same scenario without `combat` fires nothing. The checksum is pinned in
//! `assets/scenarios/weapons_reload.expected` (see `pinned.rs`).
//!
//! The cast and the commands are written in `assets/scenarios/weapons_reload.ron`, which also
//! lists the entity ids used below.

mod common;

use common::scenario_path;
use sf_headless::scenario::Scenario;
use sf_sim::{EntityId, Order, Shot, Tick, World};

const PLANT: EntityId = EntityId::new(0, 0);
const R1: EntityId = EntityId::new(1, 0);
const R2: EntityId = EntityId::new(2, 0);
const R3: EntityId = EntityId::new(3, 0);
const SCOUT: EntityId = EntityId::new(4, 0);
const R4: EntityId = EntityId::new(5, 0);
const D: EntityId = EntityId::new(6, 0);
const LIGHT: EntityId = EntityId::new(7, 0);
const HEAVY: EntityId = EntityId::new(8, 0);
const A2: EntityId = EntityId::new(9, 0);
const E: EntityId = EntityId::new(10, 0);
const A1: EntityId = EntityId::new(11, 0);
const V: EntityId = EntityId::new(12, 0);

/// The reloads of the frozen definitions, in ticks (20 a second).
const RIFLE_RELOAD: u64 = 20;
const CANNON_RELOAD: u64 = 40;
const HOWITZER_RELOAD: u64 = 100;
/// The gap between the two shells of the twin cannon's burst.
const BURST_GAP: u64 = 5;

struct Run {
    world: World,
    /// Every shot of the run, in order.
    shots: Vec<Shot>,
}

fn run_scenario(scenario: &Scenario) -> Run {
    let mut world = scenario.build_world().unwrap();
    let mut shots = Vec::new();
    for _ in 0..scenario.ticks {
        world.step();
        shots.extend_from_slice(world.shots());
    }
    Run { world, shots }
}

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("weapons_reload.ron")).unwrap()
}

fn run() -> Run {
    run_scenario(&scenario())
}

impl Run {
    /// The shots `attacker` fired.
    fn by(&self, attacker: EntityId) -> Vec<Shot> {
        self.shots
            .iter()
            .copied()
            .filter(|s| s.attacker == attacker)
            .collect()
    }

    fn ticks_of(&self, attacker: EntityId) -> Vec<u64> {
        self.by(attacker).iter().map(|s| s.tick.0).collect()
    }

    fn hp(&self, id: EntityId) -> u32 {
        self.world.get(id).expect("alive").hp
    }
}

/// The differences between consecutive numbers.
fn gaps(ticks: &[u64]) -> Vec<u64> {
    ticks.windows(2).map(|w| w[1] - w[0]).collect()
}

#[test]
fn a_weapon_fires_once_per_reload_window() {
    let r = run();
    // R1 and R2 fire every 20 ticks from the first shot to the end of the run.
    for rifleman in [R1, R2] {
        let ticks = r.ticks_of(rifleman);
        assert_eq!(ticks.len(), 27, "{rifleman:?}");
        assert!(
            gaps(&ticks).iter().all(|&g| g == RIFLE_RELOAD),
            "{rifleman:?}: {ticks:?}"
        );
        // The next one would come after the run ends: none was missed at the end.
        assert!(*ticks.last().unwrap() + RIFLE_RELOAD >= 600);
    }
    // The light tank: 35 hit points a shell every 40 ticks, until its victim is down.
    let ticks = r.ticks_of(LIGHT);
    assert_eq!(ticks.len(), 11);
    assert!(
        gaps(&ticks).iter().all(|&g| g == CANNON_RELOAD),
        "{ticks:?}"
    );
    assert!(r.by(LIGHT).iter().all(|s| s.damage == 35 && s.weapon == 6));
    // The howitzer: 90 every 100 ticks, six shells in the run.
    let ticks = r.ticks_of(A2);
    assert_eq!(ticks.len(), 6);
    assert!(
        gaps(&ticks).iter().all(|&g| g == HOWITZER_RELOAD),
        "{ticks:?}"
    );
    assert!(r.by(A2).iter().all(|s| s.damage == 90 && s.weapon == 8));
}

#[test]
fn a_burst_is_two_shells_five_ticks_apart() {
    let r = run();
    let ticks = r.ticks_of(HEAVY);
    // Nine bursts of two, until the victim is down.
    assert_eq!(ticks.len(), 18);
    // The gaps alternate: 5 inside a burst, then the rest of the 40 ticks of the reload (the
    // reload counts from the first shell of a burst to the first of the next).
    let want: Vec<u64> = (0..17)
        .map(|k| {
            if k % 2 == 0 {
                BURST_GAP
            } else {
                CANNON_RELOAD - BURST_GAP
            }
        })
        .collect();
    assert_eq!(gaps(&ticks), want, "{ticks:?}");
    assert!(r.by(HEAVY).iter().all(|s| s.damage == 30 && s.weapon == 7));
    // Every burst starts one reload after the one before.
    let starts: Vec<u64> = ticks.iter().step_by(2).copied().collect();
    assert!(gaps(&starts).iter().all(|&g| g == CANNON_RELOAD));
}

#[test]
fn the_burst_and_the_reload_count_down_in_the_unit() {
    let scenario = scenario();
    let mut world = scenario.build_world().unwrap();
    let mut seen = [false; 3];
    for _ in 0..scenario.ticks {
        world.step();
        for shot in world.shots().iter().filter(|s| s.attacker == HEAVY) {
            // Right after a burst's first shell: one to come in 5 ticks. After the second: no
            // burst, and 35 ticks of the reload of 40 left.
            let (wait, left) = (
                world.weapon_wait(HEAVY, 0).unwrap(),
                world.weapon_burst_left(HEAVY, 0).unwrap(),
            );
            match left {
                1 => {
                    assert_eq!(wait, 5);
                    seen[0] = true;
                }
                0 => {
                    assert_eq!(wait, 35);
                    seen[1] = true;
                }
                other => panic!("burst left {other} after {shot:?}"),
            }
        }
        for shot in world.shots().iter().filter(|s| s.attacker == LIGHT) {
            assert_eq!(world.weapon_wait(LIGHT, 0), Some(40), "{shot:?}");
            assert_eq!(world.weapon_burst_left(LIGHT, 0), Some(0));
            seen[2] = true;
        }
    }
    assert_eq!(seen, [true; 3]);
    // The unit has one weapon: the second slot is not there. Neither is any for a structure.
    assert_eq!(world.weapon_wait(HEAVY, 1), None);
    assert_eq!(world.weapon_wait(PLANT, 0), None);
    assert_eq!(world.weapon_wait(EntityId::new(99, 0), 0), None);
}

#[test]
fn the_hit_points_are_the_start_less_every_hit() {
    let r = run();
    // Three riflemen on the plant (600): 81 hits of 6.
    let on_plant: Vec<Shot> = r
        .shots
        .iter()
        .copied()
        .filter(|s| s.target == PLANT)
        .collect();
    assert_eq!(on_plant.len(), 81);
    assert_eq!(r.hp(PLANT), 600 - 6 * 81);
    for (k, shot) in on_plant.iter().enumerate() {
        assert_eq!(shot.hp_after, 600 - 6 * (k as u32 + 1), "{shot:?}");
    }
    // The howitzer on the second heavy tank (900): six shells of 90.
    assert_eq!(r.hp(E), 900 - 90 * 6);
    // Nothing else was hurt: the attackers and the rifleman inside the minimum range have all
    // the hit points of their role.
    for id in [R1, R2, R3, R4, LIGHT, HEAVY, A1, A2, V] {
        let full = r.world.stats_of(id).unwrap().hp;
        assert_eq!(r.hp(id), full, "{id:?}");
    }
}

#[test]
fn a_victim_at_zero_hit_points_is_left_alone() {
    let r = run();
    // R4 on the scout (25): 6, 6, 6, 6 and a fifth that takes the last one point, then nothing.
    let on_scout = r.by(R4);
    assert_eq!(
        on_scout
            .iter()
            .map(|s| (s.tick.0, s.hp_after))
            .collect::<Vec<_>>(),
        [(1, 19), (21, 13), (41, 7), (61, 1), (81, 0)]
    );
    assert_eq!(r.hp(SCOUT), 0);
    // The tanks on D (900): 11 shells of 35 and 18 of 30 are 925, and the last shell, the light
    // tank's at tick 423, takes it from 10 to zero (25 points over, not counted). The heavy tank's
    // next burst, due at tick 436, is not fired at a tank with no hit points.
    let last = r.by(LIGHT).last().copied().unwrap();
    assert_eq!((last.tick.0, last.hp_after), (423, 0));
    assert_eq!(r.hp(D), 0);
    let down_at = r
        .shots
        .iter()
        .find(|s| s.target == D && s.hp_after == 0)
        .map(|s| s.tick.0)
        .unwrap();
    assert!(
        r.shots
            .iter()
            .filter(|s| s.target == D)
            .all(|s| s.tick.0 <= down_at),
        "a shell was fired at a tank with no hit points"
    );
    // The orders stand: nothing removes a unit yet (WP5.5), the attackers keep their order.
    assert_eq!(r.world.order_of(R4), Some(Order::Attack(SCOUT)));
}

#[test]
fn inside_the_minimum_range_a_howitzer_does_not_fire() {
    let r = run();
    assert!(r.by(A1).is_empty());
    assert_eq!(r.hp(V), 50);
    // It is in reach (it did not walk) and the order stands; it is the minimum range that holds it.
    assert_eq!(r.world.order_of(A1), Some(Order::Attack(V)));
    let a1 = r.world.get(A1).unwrap();
    let v = r.world.get(V).unwrap();
    let gap = (v.pos - a1.pos).length();
    let weapon = r.world.weapon_table().weapon(8).unwrap();
    assert!(gap < weapon.min_range, "{gap:?}");
    // The howitzer that is far enough did fire.
    assert!(!r.by(A2).is_empty());
}

#[test]
fn the_reload_runs_on_while_a_unit_is_stopped_and_attacks_again() {
    let r = run();
    // R3 was stopped at tick 200 and given its attack again at tick 205. A reload that started
    // over then would fire at 205; it fires on its cadence, at 219, like R1 beside it.
    let r3 = r.ticks_of(R3);
    assert!(!r3.iter().any(|&t| (200..219).contains(&t)), "{r3:?}");
    assert_eq!(r3, r.ticks_of(R1));
    assert_eq!(r3.len(), 27);
}

#[test]
fn the_same_scenario_without_combat_fires_nothing() {
    let mut quiet = scenario();
    quiet.combat = false;
    quiet.weapon_table = None;
    let r = run_scenario(&quiet);
    assert!(r.shots.is_empty());
    assert_eq!(r.hp(PLANT), 600);
    assert_eq!(r.hp(D), 900);
    assert!(r.world.weapon_table().is_empty());
    // The units still walked up to their victims, so only the shots differ.
    assert_ne!(r.world.checksum(), run().world.checksum());
}

#[test]
fn the_run_is_the_same_every_time() {
    let (a, b) = (run(), run());
    assert_eq!(a.world.checksum_history(), b.world.checksum_history());
    assert_eq!(a.shots, b.shots);
    assert_eq!(a.world.tick(), Tick(600));
    // The history has a different value after the first shot than before it would have had.
    assert_ne!(
        a.world.checksum_after(Tick(0)),
        a.world.checksum_after(Tick(1))
    );
}
