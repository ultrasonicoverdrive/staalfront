//! WP9.3a-1, escalation after a failed strike (R2) and the harvester threat (`docs/rules/ai.md`, "Escalation and the
//! harvester threat (WP9.3a-1)"), on two scenarios that run the AI of the game: `ai_bounce` (a strong static defence; every
//! strike fails and the next one must be bigger) and `ai_harvest_raid` (two raiders go for a harvester; the defence must be
//! called before any structure is hit). The rules themselves are tested one by one in `ai_groups.rs`.
//!
//! Both scenarios run on frozen copies of the shipped definitions and a frozen variant of the profile
//! (`ai_bounce_defs/normal.ron`, `ai_harvest_raid_defs/normal.ron`); the control rows switch the number off on the same
//! scenario with the same code.

mod common;

use common::scenario_path;
use sf_ai::{Ai, Observation};
use sf_headless::cli::parse_checksum;
use sf_headless::run::{Finished, RunOptions, run_full, run_observed};
use sf_headless::scenario::Scenario;
use sf_math::{Fx, Vec2};
use sf_sim::{Command, Order, PlayerId};

const BOUNCE_EXPECTED: &str = include_str!("../../../assets/scenarios/ai_bounce.expected");
const RAID_EXPECTED: &str = include_str!("../../../assets/scenarios/ai_harvest_raid.expected");
/// `Ai::digest` of player 0 at the end of `ai_bounce` and of `ai_harvest_raid`.
const BOUNCE_DIGEST: u64 = 0x7a52_94a2_ce47_6f73;
const RAID_DIGEST: u64 = 0xa195_add4_72a5_224b;

fn load(name: &str) -> Scenario {
    Scenario::load(&scenario_path(&format!("{name}.ron"))).unwrap()
}

fn go(s: &Scenario) -> Finished {
    run_full(
        s,
        &RunOptions {
            ticks: s.ticks,
            every: 100,
        },
    )
    .unwrap()
}

/// `s` with the AI's profile changed by `change`.
fn with_profile(mut s: Scenario, change: impl Fn(&mut sf_ai::Profile)) -> Scenario {
    let old = s.ais[0].state().clone();
    let mut profile = old.profile.clone();
    change(&mut profile);
    s.ais[0] = Ai::new(old.player, profile, 0xA1_0001, old.start_sites);
    s
}

// ---- ai_bounce -------------------------------------------------------------------------------------------------

/// What the AI did over the run: every launch as `(tick of the army turn, launch value)` and every failed strike as
/// `(tick, launch value of the strike, bounces after it)`.
struct Story {
    launches: Vec<(u64, u64)>,
    bounces: Vec<(u64, u64, u32)>,
    finished: Finished,
}

fn story(s: &Scenario) -> Story {
    let mut launches = Vec::new();
    let mut bounces = Vec::new();
    let (mut launched, mut bounced, mut last_bounce) = (0, 0, 0);
    let finished = run_observed(
        s,
        &RunOptions {
            ticks: s.ticks,
            every: 100,
        },
        |w, ais| {
            let st = ais[0].state();
            // The callback runs after the tick: the AI acted at the tick before.
            let t = w.tick().0 - 1;
            if st.attacks_launched > launched {
                launched = st.attacks_launched;
                let value = st.groups.last().map_or(0, |g| g.launch_value);
                launches.push((t, value));
            }
            if st.bounces != bounced || st.bounce_tick.0 != last_bounce {
                bounced = st.bounces;
                last_bounce = st.bounce_tick.0;
                bounces.push((st.bounce_tick.0, st.bounce_value, st.bounces));
            }
        },
    )
    .unwrap();
    Story {
        launches,
        bounces,
        finished,
    }
}

/// The waves of `ai_bounce`: `(tick, launch value)`; the first at the attack value 800, then each exactly the escalated
/// need (800 -> 1,200 -> 1,800: 500 permille more), 7,590 and 17,630 being the ticks the gathered units reached it.
const BOUNCE_LAUNCHES: [(u64, u64); 3] = [(1630, 800), (7590, 1200), (17630, 1800)];
/// The failed strikes: `(tick, launch value, bounces after it)`.
const BOUNCE_BOUNCES: [(u64, u64, u32); 3] = [(3790, 800, 1), (9870, 1200, 2), (20110, 1800, 3)];

#[test]
fn each_failed_strike_makes_the_next_launch_half_as_big_again_and_the_ticks_and_values_are_pinned()
{
    let s = story(&load("ai_bounce"));
    assert_eq!(s.launches, BOUNCE_LAUNCHES);
    assert_eq!(s.bounces, BOUNCE_BOUNCES);
}

#[test]
fn without_the_escalation_the_second_wave_leaves_at_once_with_less() {
    let s = story(&with_profile(load("ai_bounce"), |p| {
        p.army.escalate_permille = 0
    }));
    // The control: no ledger, no bounce; every wave leaves with 800 and the second leaves at 3,790, the turn the first ended,
    // 3,800 ticks before the escalated one.
    assert_eq!(
        s.launches,
        [
            (1630, 800),
            (3790, 800),
            (9710, 800),
            (14270, 800),
            (18750, 800)
        ]
    );
    assert!(s.bounces.is_empty());
}

#[test]
fn the_checksums_and_ai_digests_of_ai_bounce_and_ai_harvest_raid_are_pinned() {
    for (name, expected, digest) in [
        ("ai_bounce", BOUNCE_EXPECTED, BOUNCE_DIGEST),
        ("ai_harvest_raid", RAID_EXPECTED, RAID_DIGEST),
    ] {
        let s = load(name);
        let f = go(&s);
        println!(
            "{name}: {:#018x} digest {:#018x}",
            f.outcome.final_checksum,
            f.ais[0].digest()
        );
        assert_eq!(
            f.outcome.final_checksum,
            parse_checksum(expected).unwrap(),
            "{name}"
        );
        assert_eq!(f.outcome.ai_digests, [(0, digest)], "{name}");
    }
}

// ---- ai_harvest_raid -------------------------------------------------------------------------------------------

const RIFLEMAN: sf_sim::Kind = sf_sim::Kind(12);

fn middle(x: u32, y: u32) -> Vec2 {
    let half = Fx::from_ratio(1, 2);
    Vec2::new(
        Fx::from_i64(i64::from(x)) + half,
        Fx::from_i64(i64::from(y)) + half,
    )
}

/// What the raid run shows.
#[derive(Debug, PartialEq, Eq)]
struct Raid {
    /// The first tick the AI has a threat, its cell, and whether every own structure still had its full hit points then.
    threat: Option<(u64, (u32, u32), bool)>,
    /// The first tick no raider (a rifleman of player 1) is left.
    raiders_gone: Option<u64>,
    /// The hit points of harvester 4 (slot 4 of player 0) at the end of the run.
    harvester_hp: u32,
}

fn raid(s: &Scenario) -> (Raid, Finished) {
    let mut found = None;
    let mut gone = None;
    let mut hp = 0;
    let f = run_observed(
        s,
        &RunOptions {
            ticks: s.ticks,
            every: 100,
        },
        |w, ais| {
            let st = ais[0].state();
            if found.is_none()
                && let Some((cell, _)) = st.threat
            {
                let obs = Observation::build(w, PlayerId(0), &st.known_structures);
                let whole = obs
                    .own
                    .iter()
                    .filter(|o| o.footprint.is_some())
                    .all(|o| o.hp == o.max_hp);
                found = Some((w.tick().0 - 1, (cell.x, cell.y), whole));
            }
            let snap = w.snapshot();
            let raiders = snap
                .iter()
                .filter(|e| e.owner == PlayerId(1) && e.kind == RIFLEMAN)
                .count();
            if gone.is_none() && raiders == 0 {
                gone = Some(w.tick().0 - 1);
            }
            hp = snap
                .iter()
                .find(|e| e.owner == PlayerId(0) && e.id.index() == 4)
                .map_or(0, |e| e.hp);
        },
    )
    .unwrap();
    (
        Raid {
            threat: found,
            raiders_gone: gone,
            harvester_hp: hp,
        },
        f,
    )
}

fn attack_moves(f: &Finished) -> Vec<(u64, Vec2)> {
    f.ai_commands
        .iter()
        .filter_map(|(t, _, c)| match c {
            Command::Order {
                order: Order::AttackMove(p),
                ..
            } => Some((*t, *p)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_hit_on_a_harvester_raises_a_defence_call_before_any_structure_is_hit() {
    let (r, f) = raid(&load("ai_harvest_raid"));
    // The first hit on harvester 4 is seen at tick 1510; the threat cell is the raider's (the nearest armed enemy seen) and no
    // structure is hurt yet.
    assert_eq!(r.threat, Some((1510, (20, 80), true)));
    // The units at home are called the same turn: the first `AttackMove` of the AI goes to that cell, and the raiders are
    // dead before the harvester is.
    assert_eq!(attack_moves(&f)[0], (1510, middle(20, 80)));
    assert_eq!((r.raiders_gone, r.harvester_hp), (Some(1860), 788));
}

#[test]
fn the_control_without_the_switch_never_defends_and_the_harvester_keeps_losing_hit_points() {
    let (r, f) = raid(&with_profile(load("ai_harvest_raid"), |p| {
        p.army.harvester_threat = false
    }));
    assert_eq!(r.threat, None);
    // The raiders are alive at the end of the run and the harvester is down to 382 of 800.
    assert_eq!((r.raiders_gone, r.harvester_hp), (None, 382));
    assert!(attack_moves(&f).is_empty(), "no defence order");
}

// ---- the duel fixture ------------------------------------------------------------------------------------------

#[test]
fn the_frozen_duel_opponent_is_the_shipped_profile_after_wp9_2b_1_without_the_new_numbers() {
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/normal_9_2b1.ron");
    let old =
        sf_data::ai_profile::AiProfile::from_ron(&std::fs::read_to_string(fixture).unwrap())
            .unwrap();
    let mut new = sf_data::ai_profile::AiProfile::embedded();
    new.name = "normal_9_2b1".to_string();
    // The shipped numbers of WP9.3a-1 are the ones a profile that does not write them has.
    let a = &mut new.army;
    a.escalate_permille = 0;
    a.attack_value_max = 0;
    a.harvester_threat = false;
    assert_eq!(old, new);
    assert!(!old.army.escalate_on() && !old.army.harvester_threat);
    // The mix and the counter mix of the profile after #135.
    assert_eq!((old.army.counter_permille, old.army.intel_ticks), (0, 3000));
    let mix: Vec<(&str, u16)> = old.factions[0]
        .unit_mix
        .iter()
        .map(|(r, n)| (r.as_str(), *n))
        .collect();
    assert_eq!(
        mix,
        [("rifleman", 4), ("rocket_trooper", 2), ("light_tank", 3)]
    );
}
