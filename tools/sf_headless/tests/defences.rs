//! The `defences` scenario (WP6.7, `docs/rules/defences.md`): a tower and an emplacement shoot the
//! units that come within their reach, with their turrets turning to them; three tanks shoot the power
//! plant down and the defences go silent while a second wave stands in their reach; a new plant gives them
//! power back and they shoot again; a weaponless bunker does nothing throughout.
//!
//! The cast and the ticks are written in `assets/scenarios/defences.ron`. The checksum is pinned in
//! `assets/scenarios/defences.expected` (CI runs it on Linux and Windows).

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::scenario::Scenario;
use sf_math::Facing;
use sf_sim::{EntityId, PlayerId, World};

const EXPECTED: &str = include_str!("../../../assets/scenarios/defences.expected");

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const TOWER: u32 = 0;
const EMPLACEMENT: u32 = 1;
const BUNKER: u32 = 2;
const PLANT: u32 = 3;

/// The tick the plant's output falls below what the two defences use (60), the plant having 348 of
/// its 600 hit points (the first count under 360), and the tick it is destroyed in.
const DARK: u64 = 134;
const DARK_PLANT_HP: u32 = 348;
const PLANT_DESTROYED: u64 = 371;
/// The tick the new plant is counted: the defences have power again.
const LIT: u64 = 901;

fn id(slot: u32) -> EntityId {
    EntityId::new(slot, 0)
}

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("defences.ron")).unwrap()
}

#[derive(Default)]
struct Run {
    /// `(tick, shooter slot)` of every shot of a structure of player 0.
    shots: Vec<(u64, u32)>,
    /// Ticks in which an enemy mobile unit died.
    kills: Vec<u64>,
    /// The tick `defences_powered(P0)` last changed, and to what; the plant's hit points then.
    flips: Vec<(u64, bool, Option<u32>)>,
    checksums: Vec<u64>,
    /// The tower's turret at each of its shots, with the bearing to the victim.
    aimed: Vec<(Facing, Facing)>,
    tower_hp_at_lit: u32,
    plant_gone: Option<u64>,
}

fn play(s: &Scenario) -> (World, Run) {
    let mut w = s.build_world().unwrap();
    let mut run = Run::default();
    let mut powered = true;
    for _ in 0..s.ticks {
        let plant_hp_before = w.hp_of(id(PLANT));
        let report = w.step();
        let t = report.tick.0;
        run.checksums.push(report.checksum);
        for sh in w.shots() {
            if sh.attacker.index() <= BUNKER && sh.attacker.generation() == 0 {
                run.shots.push((t, sh.attacker.index()));
                // (A victim that this very shot killed is gone from the snapshot: not measured.)
                if sh.attacker == id(TOWER)
                    && let Some(victim) = w.get(sh.target)
                {
                    let a = w.get(sh.attacker).unwrap().pos;
                    run.aimed.push((
                        w.turret_of(sh.attacker).unwrap(),
                        (victim.pos - a).facing().unwrap(),
                    ));
                }
            }
        }
        for d in w.deaths() {
            if d.owner == P1 && d.class == sf_sim::UnitClass::Mobile {
                run.kills.push(t);
            }
        }
        if run.plant_gone.is_none() && w.hp_of(id(PLANT)).is_none() {
            run.plant_gone = Some(t);
        }
        let now = w.defences_powered(P0);
        if now != powered {
            run.flips.push((t, now, plant_hp_before));
            powered = now;
        }
        if t == LIT - 1 {
            run.tower_hp_at_lit = w.hp_of(id(TOWER)).unwrap();
        }
    }
    (w, run)
}

fn shots_of(run: &Run, slot: u32, from: u64, to: u64) -> Vec<u64> {
    run.shots
        .iter()
        .filter(|&&(t, s)| s == slot && (from..=to).contains(&t))
        .map(|&(t, _)| t)
        .collect()
}

#[test]
fn the_defences_shoot_while_they_have_power() {
    let s = scenario();
    let (_, run) = play(&s);
    // The emplacement's turret is within its arc of the tank on the first tick; the tower's has to come
    // round to a rifleman first.
    assert_eq!(shots_of(&run, EMPLACEMENT, 0, 134).first(), Some(&0));
    assert_eq!(shots_of(&run, TOWER, 0, DARK).first(), Some(&7));
    // Every shot of the tower is fired with its turret within the arc (8 steps) of the victim.
    assert!(!run.aimed.is_empty());
    for (turret, bearing) in &run.aimed {
        assert!(turret.abs_diff(*bearing) <= 8, "{turret:?} {bearing:?}");
    }
    // The first wave: one rifleman falls to the tower on tick 103 (the second was shot at on 119 when the
    // power went), the tank is shot by the emplacement later (below).
    assert_eq!(run.kills[0], 103);
}

#[test]
fn the_defences_go_dark_when_the_plant_is_hurt_and_stay_dark_until_a_new_one_stands() {
    let s = scenario();
    let (w, run) = play(&s);
    // The three tanks hurt the plant: on tick 134 it has 348 of 600 (counted at the start of the step),
    // which makes 58 of the 60 the two defences use.
    assert_eq!(run.flips[0], (DARK, false, Some(DARK_PLANT_HP)));
    assert_eq!(run.flips[1].0, LIT);
    assert!(run.flips[1].1);
    assert_eq!(run.flips.len(), 2);
    // It is destroyed on tick 371.
    assert!(w.hp_of(id(PLANT)).is_none());
    assert_eq!(run.plant_gone, Some(PLANT_DESTROYED));
    // Neither defence fires from the tick it went dark to the tick it is lit again: the last shots were
    // the tower's on 119 and the emplacement's on 100, the next ones are on 907 and 901.
    assert!(shots_of(&run, TOWER, DARK, LIT - 1).is_empty());
    assert!(shots_of(&run, EMPLACEMENT, DARK, LIT - 1).is_empty());
    assert_eq!(shots_of(&run, TOWER, 0, DARK).last(), Some(&119));
    assert_eq!(shots_of(&run, EMPLACEMENT, 0, DARK).last(), Some(&100));
    assert_eq!(shots_of(&run, EMPLACEMENT, LIT, 1500).first(), Some(&901));
    assert_eq!(shots_of(&run, TOWER, LIT, 1500).first(), Some(&907));
    // While dark, units stood in reach and hurt the tower (it was not undamaged), and it did not answer.
    assert!(run.tower_hp_at_lit < 600, "{}", run.tower_hp_at_lit);
}

#[test]
fn the_second_wave_falls_once_the_power_is_back() {
    let s = scenario();
    let (w, run) = play(&s);
    // The tank on 954 (the emplacement), then the first wave's rifleman and the three of the second
    // wave in reach of the tower, one every 112 ticks: 7 shots of 8 damage on 50 hit points. The rifleman
    // at the bunker (out of the tower's reach) stays alive.
    assert_eq!(run.kills, vec![103, 954, 987, 1099, 1211, 1323]);
    let alive = w
        .snapshot()
        .iter()
        .filter(|v| v.owner == P1 && v.kind.0 == 12)
        .count();
    assert_eq!(alive, 1);
    assert_eq!(w.hp_of(id(TOWER)), Some(498));
    assert_eq!(w.hp_of(id(EMPLACEMENT)), Some(296));
}

#[test]
fn the_bunker_does_nothing() {
    let s = scenario();
    let (w, run) = play(&s);
    assert!(run.shots.iter().all(|&(_, slot)| slot != BUNKER));
    assert_eq!(w.turret_target_of(id(BUNKER)), None);
    assert!(w.turret_of(id(BUNKER)).is_none());
    // It was shot at by the rifleman beside it and stood.
    let hp = w.hp_of(id(BUNKER)).unwrap();
    assert!((1000..1200).contains(&hp), "{hp}");
    assert_eq!(hp, 1128);
}

#[test]
fn it_replays_the_same_and_matches_the_pinned_checksum() {
    let s = scenario();
    let (w, a) = play(&s);
    let (_, b) = play(&s);
    assert_eq!(a.checksums, b.checksums);
    assert_eq!(
        *a.checksums.last().unwrap(),
        parse_checksum(EXPECTED).unwrap()
    );
    assert_eq!(w.tick().0, 1500);
    // Every command was taken.
    let report =
        sf_headless::run::run(&s, &sf_headless::run::RunOptions::for_scenario(&s)).unwrap();
    assert_eq!(report.commands_ignored, 0);
}
