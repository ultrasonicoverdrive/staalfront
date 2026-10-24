//! The `tanks_vs_infantry` scenario (WP5.4, `docs/rules/targeting.md`; since WP5.5 with destruction,
//! `docs/rules/destruction.md`): ten light tanks against twenty infantry (fourteen riflemen and six rocket
//! troopers), both sides on automatic targeting and nobody given an attack order. Each side attack-moves onto
//! the other's ground; the units pick their own targets, stop to fight and go on when it is over. This is the
//! headless half of the phase-5 done-when: **the same outcome and the same checksum every run**, now with the
//! dead removed from the world and the tanks leaving wrecks. The other half, that it looks that way in the
//! game, is `sf_game`'s `destruction_screenshot` test and `docs/screenshots/wp5.5/`.
//!
//! The cast and the commands are written in `assets/scenarios/tanks_vs_infantry.ron` (which also lists the
//! entity ids used below), on a frozen copy of the shipped definitions (`tanks_vs_infantry_defs/`). The
//! checksum is pinned in `assets/scenarios/tanks_vs_infantry.expected` (see `pinned.rs`).
//!
//! **Re-pinned in WP5.5** from `0x53d3bcb0110a9479` (WP5.4: the dead stayed in the world on hold) with
//! `destruction: true`. The cause is measured by the test `without_destruction_it_is_the_run_of_wp5_4`: the
//! same scenario with the flag switched off gives the old value again, shot for shot. What changed is only
//! that the dead are removed: their bodies no longer stand in the way of the units that go on, so the
//! survivors take other steps and the later part of the fight differs.

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::scenario::Scenario;
use sf_math::Vec2;
use sf_sim::{Death, EntityId, Order, PlayerId, Shot, Tick, UnitClass, World, WreckKind};

const EXPECTED: &str = include_str!("../../../assets/scenarios/tanks_vs_infantry.expected");

/// The checksum of the scenario before WP5.5 (the dead stay in the world), which it gives again with the
/// destruction switched off.
const WP5_4_CHECKSUM: u64 = 0x53d3_bcb0_110a_9479;

const ENTITIES: u32 = 30;
const TANKS: std::ops::Range<u32> = 0..10;
const INFANTRY: std::ops::Range<u32> = 10..30;
/// The kind numbers of the cast, as the scenario's frozen definitions give them.
const CANNON: u16 = 6;
const RIFLE: u16 = 0;
const ROCKET: u16 = 1;
const LIGHT_TANK: u16 = 19;
/// Ticks a vehicle wreck lies (the simulation's own settings).
const WRECK_TICKS: u64 = 600;

fn id(slot: u32) -> EntityId {
    EntityId::new(slot, 0)
}

/// Everything a run reported, tick by tick.
struct Run {
    world: World,
    shots: Vec<Shot>,
    deaths: Vec<Death>,
    /// `hp[t][slot]` at the end of tick `t`; zero for a unit that is not in the world.
    hp: Vec<Vec<u32>>,
    /// `gone[t][slot]`: whether the unit is not in the world at the end of tick `t`.
    gone: Vec<Vec<bool>>,
    /// `pos[t][slot]` at the end of tick `t` (where it was last, for a unit that is gone).
    pos: Vec<Vec<Vec2>>,
    /// The number of wrecks lying at the end of each tick.
    wrecks: Vec<usize>,
    /// The checksum after each tick.
    checksums: Vec<u64>,
}

fn run() -> Run {
    run_with(true)
}

fn run_with(destruction: bool) -> Run {
    let mut scenario = Scenario::load(&scenario_path("tanks_vs_infantry.ron")).unwrap();
    assert!(scenario.destruction, "the scenario destroys");
    scenario.destruction = destruction;
    let mut world = scenario.build_world().unwrap();
    let mut r = Run {
        world: World::new(sf_sim::WorldConfig::new(1)).unwrap(),
        shots: Vec::new(),
        deaths: Vec::new(),
        hp: Vec::new(),
        gone: Vec::new(),
        pos: Vec::new(),
        wrecks: Vec::new(),
        checksums: Vec::new(),
    };
    let mut last = vec![Vec2::ZERO; ENTITIES as usize];
    for _ in 0..scenario.ticks {
        let report = world.step();
        r.checksums.push(report.checksum);
        r.shots.extend_from_slice(world.shots());
        r.deaths.extend_from_slice(world.deaths());
        r.hp.push(
            (0..ENTITIES)
                .map(|s| world.hp_of(id(s)).unwrap_or(0))
                .collect(),
        );
        r.gone
            .push((0..ENTITIES).map(|s| !world.is_alive(id(s))).collect());
        for s in 0..ENTITIES {
            if let Some(v) = world.get(id(s)) {
                last[s as usize] = v.pos;
            }
        }
        r.pos.push(last.clone());
        r.wrecks.push(world.wrecks().len());
    }
    r.world = world;
    r
}

impl Run {
    fn alive(&self, slots: std::ops::Range<u32>) -> Vec<u32> {
        slots
            .filter(|&s| self.world.is_alive(id(s)) && self.world.hp_of(id(s)).unwrap() > 0)
            .collect()
    }

    /// The tick at the end of which `slot` first had no hit points.
    fn fell_at(&self, slot: u32) -> Option<usize> {
        self.hp.iter().position(|row| row[slot as usize] == 0)
    }

    /// The tick at the end of which `slot` was no longer in the world.
    fn removed_at(&self, slot: u32) -> Option<usize> {
        self.gone.iter().position(|row| row[slot as usize])
    }
}

fn is_tank(e: EntityId) -> bool {
    TANKS.contains(&e.index())
}

// ---- the same outcome every run ---------------------------------------------------------------------------

#[test]
fn the_same_outcome_and_checksum_come_out_every_run() {
    let (a, b) = (run(), run());
    // Every tick of the run, not only the last: the checksum after each, the shots, the deaths with their
    // credit, the hit points, the places and the wrecks of all thirty.
    assert_eq!(a.checksums, b.checksums);
    assert_eq!(a.shots, b.shots);
    assert_eq!(a.deaths, b.deaths);
    assert_eq!(a.hp, b.hp);
    assert_eq!(a.gone, b.gone);
    assert_eq!(a.pos, b.pos);
    assert_eq!(a.wrecks, b.wrecks);
    assert_eq!(a.world.checksum(), b.world.checksum());
    // And it is the value committed next to the scenario, which CI requires on Linux and on Windows.
    assert_eq!(
        a.world.checksum(),
        parse_checksum(EXPECTED).expect("tanks_vs_infantry.expected is a checksum")
    );
}

#[test]
fn without_destruction_it_is_the_run_of_wp5_4() {
    // The measured cause of the re-pin: switch the destruction off and the checksum of WP5.4 comes back,
    // and so does everything the WP5.4 test asserted (the dead stay, on hold, where they fell).
    let r = run_with(false);
    assert_eq!(r.world.checksum(), WP5_4_CHECKSUM);
    assert_eq!(r.alive(INFANTRY), Vec::<u32>::new());
    assert_eq!(r.alive(TANKS), [1, 2, 3, 9]);
    assert_eq!(r.shots.len(), 228);
    assert!(
        (0..ENTITIES).all(|s| r.world.is_alive(id(s))),
        "nothing was removed"
    );
    assert!(r.deaths.is_empty() && r.wrecks.iter().all(|&n| n == 0));
    for slot in 0..ENTITIES {
        let Some(t) = r.fell_at(slot) else { continue };
        assert_eq!(
            r.world.order_of(id(slot)),
            Some(Order::HoldPosition),
            "unit {slot}"
        );
        for later in t..r.hp.len() {
            assert_eq!(r.pos[later][slot as usize], r.pos[t][slot as usize]);
        }
    }
    // With the flag on it is the same run until the first removal (the checksums differ from the start: the
    // settings are part of them) and a different one after it.
    let on = run();
    let first_death = on.deaths.first().unwrap().tick.0 as usize;
    assert_eq!(first_death, 303);
    assert_eq!(on.pos[..first_death], r.pos[..first_death]);
    assert_eq!(on.hp[..first_death], r.hp[..first_death]);
    assert_ne!(on.checksums[0], r.checksums[0]);
    assert_ne!(on.pos.last(), r.pos.last());
}

#[test]
fn destruction_needs_combat() {
    let mut s = Scenario::load(&scenario_path("tanks_vs_infantry.ron")).unwrap();
    assert!(s.problems().is_empty());
    s.combat = false;
    let problems = s.problems();
    assert!(
        problems
            .iter()
            .any(|p| p.contains("destruction needs `combat`")),
        "{problems:?}"
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
    let world = on.build_world().unwrap();
    assert_eq!(
        world.targeting_config(),
        Some(sf_sim::TargetingConfig::DEFAULT)
    );
    assert_eq!(
        world.destruction_config(),
        Some(sf_sim::DestructionConfig::DEFAULT)
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
fn the_tanks_win_with_three_of_ten_left_and_every_infantryman_gone() {
    let r = run();
    assert_eq!(
        r.alive(INFANTRY),
        Vec::<u32>::new(),
        "no infantryman is left"
    );
    assert_eq!(r.alive(TANKS), [3, 4, 9]);
    // Hit points left, tank by tank (a light tank has 400): two untouched, one nearly, one hurt.
    let hp = |s: u32| r.world.hp_of(id(s)).unwrap();
    assert_eq!([hp(3), hp(4), hp(9)], [282, 388, 400]);
    // Nothing else is in the world: the other twenty-seven were removed, not left on hold.
    assert_eq!(r.world.snapshot().len(), 3);
    assert!(
        (0..ENTITIES)
            .filter(|s| ![3, 4, 9].contains(s))
            .all(|s| !r.world.is_alive(id(s)))
    );
    assert_eq!(r.deaths.len(), 27);
}

#[test]
fn the_fight_starts_at_tick_270_and_the_last_shot_is_at_tick_820() {
    let r = run();
    assert_eq!(r.shots.len(), 243);
    assert_eq!(r.shots.first().unwrap().tick.0, 270);
    assert_eq!(r.shots.last().unwrap().tick.0, 820);
    // The last shell was still in the air: the last infantryman falls on tick 823.
    let last_fall = r
        .deaths
        .iter()
        .filter(|d| INFANTRY.contains(&d.victim.index()))
        .map(|d| d.tick.0)
        .max()
        .unwrap();
    assert_eq!(last_fall, 823);
    // Nothing fires after that, and the world is at rest: no projectile in flight.
    assert_eq!(r.world.projectile_count(), 0);
    assert!(r.shots.iter().all(|s| s.tick.0 <= 820));
}

#[test]
fn who_fired_what() {
    let r = run();
    let by = |f: &dyn Fn(&Shot) -> bool| r.shots.iter().filter(|s| f(s)).count();
    // The tanks fire their cannons, all at infantry; the riflemen and the rocket troopers fire at tanks.
    assert_eq!(by(&|s| is_tank(s.attacker) && s.weapon == CANNON), 95);
    assert_eq!(by(&|s| is_tank(s.attacker)), 95);
    assert_eq!(by(&|s| is_tank(s.target)), 148);
    assert_eq!(by(&|s| !is_tank(s.attacker) && s.weapon == RIFLE), 98);
    assert_eq!(by(&|s| !is_tank(s.attacker) && s.weapon == ROCKET), 50);
    assert!(
        r.shots
            .iter()
            .all(|s| is_tank(s.attacker) != is_tank(s.target))
    );
}

// ---- what the rules say, along the run --------------------------------------------------------------------

#[test]
fn nobody_fires_at_its_own_side_and_nobody_gone_fires_or_is_fired_at() {
    let r = run();
    let owner = |e: EntityId| {
        // Owners by slot: the tanks are 0, the infantry 1.
        u8::from(!is_tank(e))
    };
    for s in &r.shots {
        assert_ne!(owner(s.attacker), owner(s.target), "{s:?}");
        // The state at the end of the tick before the shot: the start of the weapon stage.
        let t = s.tick.0 as usize;
        if t > 0 {
            assert!(
                !r.gone[t - 1][s.attacker.index() as usize],
                "a unit that is gone fired: {s:?}"
            );
            assert!(
                !r.gone[t - 1][s.target.index() as usize],
                "a unit that is gone was shot at: {s:?}"
            );
        }
    }
}

#[test]
fn a_unit_is_removed_on_the_tick_it_reaches_zero_and_nothing_ever_stands_at_zero() {
    let r = run();
    // At no tick end is anybody in the world with no hit points.
    for (t, (row, gone)) in r.hp.iter().zip(&r.gone).enumerate() {
        for (slot, (&hp, &away)) in row.iter().zip(gone).enumerate() {
            assert!(away || hp > 0, "unit {slot} stands at zero after tick {t}");
        }
    }
    // Every death is a unit that was in the world at the end of the tick before and is gone at the end of its
    // tick, and the tick of the report is that tick.
    assert_eq!(r.deaths.len(), 27);
    for d in &r.deaths {
        let (t, slot) = (d.tick.0 as usize, d.victim.index());
        assert!(t == 0 || !r.gone[t - 1][slot as usize], "{d:?}");
        assert!(r.gone[t][slot as usize], "{d:?}");
        assert_eq!(r.removed_at(slot), Some(t));
        assert!(!r.world.is_alive(d.victim));
        // The slot is not reused in this scenario, so the generation is still the first.
        assert_eq!(d.victim.generation(), 0);
    }
    // A unit that is gone never comes back and never moves.
    for slot in 0..ENTITIES {
        let Some(t) = r.removed_at(slot) else {
            continue;
        };
        for later in t..r.hp.len() {
            assert!(r.gone[later][slot as usize]);
            assert_eq!(r.pos[later][slot as usize], r.pos[t][slot as usize]);
        }
    }
}

#[test]
fn only_the_tanks_leave_wrecks_and_each_lies_for_600_ticks() {
    let r = run();
    // Seven tanks fell, twenty infantrymen: seven wrecks, none of the infantry.
    let tanks: Vec<&Death> = r.deaths.iter().filter(|d| d.kind.0 == LIGHT_TANK).collect();
    assert_eq!(tanks.len(), 7);
    for d in &r.deaths {
        let wreck = (d.kind.0 == LIGHT_TANK).then_some(WreckKind::Vehicle);
        assert_eq!(d.wreck, wreck, "{d:?}");
        assert_eq!(d.class, UnitClass::Mobile);
    }
    // Wrecks lying at the end of a tick = tanks fallen by then minus tanks whose 600 ticks are over.
    for (t, &n) in r.wrecks.iter().enumerate() {
        let t = t as u64;
        let lying = tanks
            .iter()
            .filter(|d| d.tick.0 <= t && t < d.tick.0 + WRECK_TICKS)
            .count();
        assert_eq!(n, lying, "tick {t}");
    }
    // The first at the end of tick 408, the seventh after tick 776, the last gone on tick 1376.
    assert_eq!(r.wrecks.iter().position(|&n| n == 1), Some(408));
    assert_eq!(r.wrecks.iter().position(|&n| n == 7), Some(776));
    assert_eq!(r.wrecks.iter().rposition(|&n| n > 0), Some(1375));
    assert!(r.world.wrecks().is_empty());
}

#[test]
fn the_credit_goes_to_an_enemy_and_the_tallies_add_up() {
    let r = run();
    let owner_of = |slot: u32| PlayerId(u8::from(!TANKS.contains(&slot)));
    let mut kills = [0_u32; 2];
    let mut assists = [0_u32; 2];
    for d in &r.deaths {
        let killer = d
            .killer
            .expect("every death here was done by the other side");
        assert_ne!(owner_of(killer.shooter.index()), d.owner, "{d:?}");
        assert_eq!(killer.owner, owner_of(killer.shooter.index()));
        kills[killer.owner.0 as usize] += 1;
        let mut previous = killer.damage;
        for a in &d.assists {
            assert_eq!(a.owner, killer.owner, "{d:?}");
            assert!(a.damage <= previous, "ranked by damage: {d:?}");
            previous = a.damage;
            assists[a.owner.0 as usize] += 1;
        }
        // Tanks fall to rockets of 59 (130 percent of 45), infantrymen to cannon shells of 14 (40 percent of 35).
        let rocket_or_shell = if d.kind.0 == LIGHT_TANK { 59 } else { 14 };
        assert_eq!(killer.damage, rocket_or_shell, "{d:?}");
    }
    let t = |p: u8| r.world.tally_of(PlayerId(p));
    assert_eq!((t(0).kills, t(0).assists, t(0).losses), (20, 17, 7));
    assert_eq!((t(1).kills, t(1).assists, t(1).losses), (7, 0, 20));
    assert_eq!(kills, [20, 7]);
    assert_eq!(assists, [17, 0]);
    // The kills of the three tanks that are left are theirs alone: 3, 1 and 1 of the twenty.
    let kills_of = |s: u32| r.world.kills_of(id(s)).unwrap();
    assert_eq!([kills_of(3), kills_of(4), kills_of(9)], [3, 1, 1]);
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

#[test]
fn the_report_of_a_tick_is_in_the_snapshot() {
    // `Snapshot::deaths` is the last tick's list, replaced by every step; here the world is at rest at the end.
    let r = run();
    assert!(r.world.snapshot().deaths().is_empty());
    let mut scenario = Scenario::load(&scenario_path("tanks_vs_infantry.ron")).unwrap();
    scenario.ticks = 409;
    let mut world = scenario.build_world().unwrap();
    for _ in 0..409 {
        world.step();
    }
    // Tick 408 was the one the first tank fell on.
    let deaths = world.snapshot().deaths();
    assert_eq!(deaths.len(), 1);
    assert_eq!((deaths[0].tick, deaths[0].kind.0), (Tick(408), LIGHT_TANK));
    assert_eq!(deaths[0].size, sf_math::Fx::from_ratio(12, 10));
    assert_eq!(world.snapshot().wrecks().count(), 1);
}
