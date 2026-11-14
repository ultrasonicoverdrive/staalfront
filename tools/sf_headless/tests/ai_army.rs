//! The AI's army (WP9.1c-2, `docs/rules/ai.md`, "The army in WP9.1c-2"): the scenario `ai_vs_passive` (the
//! AI against m1's static opponent) with its checksum and AI digest pinned, the waves, the win, the command
//! set and rate; the scenario `ai_raid` (a scripted raid on the AI's base) for the threat rule; a defeated AI;
//! the fence; then each rule where it decides, on an observation that is the world's own with the entities the
//! test needs put in, so that every test runs the production code (`sf_ai::army::turn`, `army::watch`,
//! `production::turn`, `Ai::think_with`, the runner's own driver).
//!
//! Player 0 is the AI (Meridian, a construction centre at (12, 68), gather cell (24, 64)) on the first match's
//! map; both scenarios run on frozen copies of the shipped definitions and profile.

mod common;

use std::collections::BTreeMap;

use common::scenario_path;
use sf_ai::army::{self, ARMY_COMMANDS_PER_TURN};
use sf_ai::production;
use sf_ai::state::{GroupStage, Posture};
use sf_ai::{Ai, AiState, Known, Lens, Observation, Own, OwnFactory, Seen};
use sf_headless::cli::parse_checksum;
use sf_headless::run::{Finished, RunOptions, run_full, run_observed};
use sf_headless::scenario::{Action, Scenario, TimedCommand};
use sf_math::{Fx, Vec2};
use sf_sim::{
    Cell, Command, EntityId, FactoryStatus, Kind, Order, PlayerId, Tick, Verdict, Visibility, World,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const VS_EXPECTED: &str = include_str!("../../../assets/scenarios/ai_vs_passive.expected");
const RAID_EXPECTED: &str = include_str!("../../../assets/scenarios/ai_raid.expected");
/// `Ai::digest` of player 0 at the end of `ai_vs_passive` and of `ai_raid`.
const VS_DIGEST: u64 = 0x5817_c874_52d6_33f1;
const RAID_DIGEST: u64 = 0x13d1_a446_6b23_765e;

/// The kinds of the frozen definitions (`ai_vs_passive_defs/unit_defs.ron`, the shipped ones of 12-11-2026).
const CENTRE: Kind = Kind(0);
const PLANT: Kind = Kind(1);
const REFINERY: Kind = Kind(2);
const BARRACKS: Kind = Kind(3);
const FACTORY: Kind = Kind(4);
const HARVESTER: Kind = Kind(11);
const RIFLEMAN: Kind = Kind(12);
const TANK: Kind = Kind(19);

fn load(name: &str) -> Scenario {
    Scenario::load(&scenario_path(&format!("{name}.ron"))).unwrap()
}

fn go(s: &Scenario, ticks: u64) -> Finished {
    run_full(s, &RunOptions { ticks, every: 100 }).unwrap()
}

/// `s` with the AI's profile changed by `change`.
fn with_profile(mut s: Scenario, change: impl Fn(&mut sf_ai::Profile)) -> Scenario {
    let old = s.ais[0].state().clone();
    let mut profile = old.profile.clone();
    change(&mut profile);
    s.ais[0] = Ai::new(old.player, profile, 0xA1_0001, old.start_sites);
    s
}

fn middle(c: Cell) -> Vec2 {
    let half = Fx::from_ratio(1, 2);
    Vec2::new(
        Fx::from_i64(i64::from(c.x)) + half,
        Fx::from_i64(i64::from(c.y)) + half,
    )
}

// ---- ai_vs_passive -----------------------------------------------------------------------------------------

/// One attack as the AI sent it: the tick of the think that launched it, how many units and what they cost,
/// how many of them were alive when it ended (or at the end of the run) and when it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Wave {
    tick: u64,
    size: usize,
    value: u64,
    survivors: usize,
    ended: Option<u64>,
}

struct Story {
    finished: Finished,
    waves: Vec<Wave>,
    /// The tick the match named player 0 the winner.
    won: Option<u64>,
    /// Army units of player 0 within the gather radius of the gather cell at ticks 4,000, 8,000, ...
    at_gather: BTreeMap<u64, usize>,
}

fn story(s: &Scenario, ticks: u64) -> Story {
    let mut waves: Vec<Wave> = Vec::new();
    let mut won = None;
    let mut at_gather = BTreeMap::new();
    let mut launched = 0;
    let mut out_before = false;
    let finished = run_observed(s, &RunOptions { ticks, every: 100 }, |w, ais| {
        let st = ais[0].state();
        let t = w.tick().0;
        if st.attacks_launched > launched {
            launched = st.attacks_launched;
            let cost = |k: Kind| {
                u64::from(
                    w.production_table()
                        .and_then(|t| t.get(k))
                        .map_or(0, |u| u.cost),
                )
            };
            let value = w
                .snapshot()
                .iter()
                .filter(|v| st.attackers().contains(&v.id))
                .map(|v| cost(v.kind))
                .sum();
            waves.push(Wave {
                tick: t - 1,
                size: st.attackers().len(),
                value,
                survivors: st.attackers().len(),
                ended: None,
            });
        }
        let out = !st.attackers().is_empty();
        if let Some(wave) = waves.last_mut() {
            if out {
                wave.survivors = st.attackers().len();
            } else if out_before && wave.ended.is_none() {
                wave.ended = Some(t - 1);
            }
        }
        out_before = out;
        if won.is_none()
            && let Some(r) = w.match_result()
            && r.verdict == Verdict::Won(P0)
        {
            won = Some(r.tick.0);
        }
        if t.is_multiple_of(4000)
            && let Some(g) = st.gather
        {
            let r = u32::from(st.profile.army.gather_radius);
            let n = w
                .snapshot()
                .iter()
                .filter(|v| v.owner == P0 && w.footprint_of(v.id).is_none())
                .filter(|v| v.kind != HARVESTER && st.scout != Some(v.id))
                .filter_map(|v| w.cell_at(v.pos))
                .filter(|c| c.x.abs_diff(g.x).max(c.y.abs_diff(g.y)) <= r)
                .count();
            at_gather.insert(t, n);
        }
    })
    .unwrap();
    Story {
        finished,
        waves,
        won,
        at_gather,
    }
}

/// T2: the length of `ai_vs_passive`; the AI must have won before it.
const T2: u64 = 16_000;
/// The tick the match names the AI the winner (the static opponent's construction centre falls).
const WON: u64 = 13_839;
/// The waves of `ai_vs_passive`: (tick, units, value, survivors, ended).
const WAVES: [(u64, usize, u64, usize, Option<u64>); 1] = [(11_150, 6, 2000, 6, None)];
/// Army units at the gather cell at ticks 4,000, 8,000, 12,000 and 16,000 (the match is over at 13,839).
const VS_AT_GATHER: [usize; 4] = [0, 4, 0, 0];
/// The tick of the last `BuildStructure` before the win.
const VS_LAST_BUILD: u64 = 2_890;

#[test]
fn the_checksums_and_ai_digests_of_ai_vs_passive_and_ai_raid_are_pinned() {
    for (name, expected, digest) in [
        ("ai_vs_passive", VS_EXPECTED, VS_DIGEST),
        ("ai_raid", RAID_EXPECTED, RAID_DIGEST),
    ] {
        let s = load(name);
        let f = go(&s, s.ticks);
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
    assert_eq!(load("ai_vs_passive").ticks, T2);
}

/// WP9.1c-2's pins of `ai_vs_passive` (length 16,000): the checksum and the AI digest the scenario had before WP9.1c-3.
const WP9_1C_2_VS_CHECKSUM: u64 = 0x3b5a_509e_49e4_3343;
const WP9_1C_2_VS_DIGEST: u64 = 0x74e3_8ea1_d028_c406;

#[test]
fn the_repin_of_ai_vs_passive_in_wp9_1c_3_is_the_profile() {
    // Cause of the re-pin, as a test (like `ai_scout`'s): the shipped profile with the numbers WP9.1c-3 changed put
    // back to WP9.1c-2's (the goal list with the vehicle factory fifth and the first tower before the second
    // refinery, `attack_value` 3,000, `max_wait` 6,000) gives the old checksum and the old AI digest. So the move
    // is the profile and nothing else (the `closing` fix in `army::watch` moves neither).
    let s = with_profile(load("ai_vs_passive"), |p| {
        p.army.attack_value = 3000;
        p.army.max_wait = 6000;
        p.build_order = [
            (PLANT, 1),
            (REFINERY, 1),
            (BARRACKS, 1),
            (PLANT, 2),
            (FACTORY, 1),
            (Kind(6), 1),
            (REFINERY, 2),
            (Kind(5), 1),
            (Kind(6), 2),
            (Kind(7), 1),
        ]
        .to_vec();
    });
    let f = go(&s, s.ticks);
    assert_eq!(f.outcome.final_checksum, WP9_1C_2_VS_CHECKSUM);
    assert_eq!(f.outcome.ai_digests, [(0, WP9_1C_2_VS_DIGEST)]);
    // And it is a different run from the shipped one.
    assert_ne!(
        f.outcome.final_checksum,
        parse_checksum(VS_EXPECTED).unwrap()
    );
}

#[test]
fn the_ai_beats_the_static_opponent_of_m1_before_t2() {
    let s = load("ai_vs_passive");
    let st = story(&s, s.ticks);
    let builds: Vec<u64> = st
        .finished
        .ai_commands
        .iter()
        .filter(|c| matches!(c.2, Command::BuildStructure { .. }))
        .map(|c| c.0)
        .collect();
    println!("won at {:?} (T2 {T2})", st.won);
    for w in &st.waves {
        println!("wave {w:?}");
    }
    println!(
        "army at the gather cell {:?}, the last build at {:?}",
        st.at_gather,
        builds.last()
    );
    let won = st.won.expect("the AI wins");
    assert!(won < T2, "{won}");
    assert_eq!(won, WON);
    let waves: Vec<_> = st
        .waves
        .iter()
        .map(|w| (w.tick, w.size, w.value, w.survivors, w.ended))
        .collect();
    assert_eq!(waves, WAVES);
    // Only gathered units go, so every wave is at least `min_units` strong (the gathered filter: one unit
    // leaving alone would fail this).
    let min = usize::from(s.ais[0].state().profile.army.min_units);
    assert!(st.waves.iter().all(|w| w.size >= min), "{:?}", st.waves);
    assert_eq!(
        st.at_gather.values().copied().take(4).collect::<Vec<_>>(),
        VS_AT_GATHER
    );
    assert_eq!(builds.last(), Some(&VS_LAST_BUILD));
    // The defeated opponent: its construction centre (slot 11) is gone.
    assert!(
        !st.finished
            .world
            .snapshot()
            .iter()
            .any(|v| v.id == EntityId::new(11, 0))
    );
}

/// The commands the AI may send (decision of WP9.1c-2): construction, production, rally, repair, the rescue
/// sale and unit orders that move, attack-move or attack, never queued.
fn on_the_list(c: &Command) -> bool {
    match c {
        Command::BuildStructure { .. }
        | Command::QueueUnit { .. }
        | Command::SetRally { .. }
        | Command::Repair { .. }
        | Command::Sell { .. } => true,
        Command::Order { order, queued, .. } => {
            !queued
                && matches!(
                    order,
                    Order::Move(_) | Order::AttackMove(_) | Order::Attack(_)
                )
        }
        _ => false,
    }
}

/// The most commands of the AI in any 1,000 ticks starting on a multiple of 100.
fn worst_window(f: &Finished) -> usize {
    let end = f.outcome.ticks;
    (0..=end.saturating_sub(1000))
        .step_by(100)
        .map(|from| {
            f.ai_commands
                .iter()
                .filter(|(t, _, _)| *t >= from && *t < from + 1000)
                .count()
        })
        .max()
        .unwrap_or(0)
}

/// The bound on commands in 1,000 ticks: measured 12 in both runs below in WP9.1c-2, in the opening, when the barracks
/// queued unit after unit below the army floor (about one `QueueUnit` per production turn, 25 turns per 1,000 ticks,
/// but the money limits it to about 8), plus the four first builds, a rally point and the scout's leg (WP9.1c-3: the
/// first four builds now come with the harvester, the barracks stands at 2,890 and the units follow from 4,460; the
/// measured worst is printed by the test, 4 in `ai_vs_passive`). 15 is 12 and room for
/// one more army turn's worth of orders in the same window (an attack, a march, a defence: up to 3 on top of a
/// gather order), so the bound fails on a spam (an order every turn would be 25) but not on one more wave. (WP9.1c-1's
/// bound was 10 for 6 measured, before the floor queued units in the opening.)
const COMMANDS_PER_1000: usize = 15;

/// The most `Order` commands the AI sent at one tick (one army turn).
fn most_orders_per_turn(f: &Finished) -> usize {
    let mut per: BTreeMap<u64, usize> = BTreeMap::new();
    for (t, _, c) in &f.ai_commands {
        if matches!(c, Command::Order { .. }) {
            *per.entry(*t).or_default() += 1;
        }
    }
    per.values().copied().max().unwrap_or(0)
}

#[test]
fn the_ai_of_ai_vs_passive_sends_only_the_list_at_a_bounded_rate_and_nothing_after_the_win() {
    let s = load("ai_vs_passive");
    let f = go(&s, s.ticks);
    for (t, p, c) in &f.ai_commands {
        assert_eq!(*p, 0);
        assert!(on_the_list(c), "tick {t}: {c:?}");
    }
    let has = |pick: fn(&Command) -> bool| f.ai_commands.iter().any(|(_, _, c)| pick(c));
    assert!(has(|c| matches!(
        c,
        Command::Order {
            order: Order::AttackMove(_),
            ..
        }
    )));
    let worst = worst_window(&f);
    let most = most_orders_per_turn(&f);
    println!(
        "ai_vs_passive: {} commands, most in 1,000 ticks {worst}, most orders in one turn {most}",
        f.ai_commands.len()
    );
    assert!(worst <= COMMANDS_PER_1000, "{worst}");
    assert!(most <= ARMY_COMMANDS_PER_TURN, "{most}");
    assert_eq!(ARMY_COMMANDS_PER_TURN, 4);
    // The match is decided at `WON`: from then on the AI sends nothing (the defeated-AI rule, decision 5).
    let last = f.ai_commands.last().unwrap().0;
    assert!(last <= WON, "{last}");
}

#[test]
fn with_no_enemy_known_the_army_never_attacks_and_stays_under_the_rate_over_forty_thousand_ticks() {
    // The "no enemy reachable" run: no scout and no start site of the other player, so the target rule
    // finds nothing and the army only gathers (it is never attacked either). 40,000 ticks.
    let mut s = with_profile(load("ai_vs_passive"), |p| p.army.scouts_max = 0);
    let old = s.ais[0].state().clone();
    let sites: Vec<_> = old
        .start_sites
        .iter()
        .copied()
        .filter(|(p, _)| *p == P0)
        .collect();
    s.ais[0] = Ai::new(P0, old.profile.clone(), 0xA1_0001, sites);
    let f = go(&s, 40_000);
    for (t, _, c) in &f.ai_commands {
        assert!(on_the_list(c), "tick {t}: {c:?}");
        assert!(
            !matches!(
                c,
                Command::Order {
                    order: Order::AttackMove(_) | Order::Attack(_),
                    ..
                }
            ),
            "tick {t}: {c:?}"
        );
    }
    let worst = worst_window(&f);
    let army = f
        .world
        .snapshot()
        .iter()
        .filter(|v| {
            v.owner == P0
                && f.world.footprint_of(v.id).is_none()
                && ![HARVESTER.0, 15].contains(&v.kind.0)
        })
        .count();
    println!(
        "40,000 ticks, no enemy known: {} commands, most in 1,000 ticks {worst}, {army} army units, {} attacks",
        f.ai_commands.len(),
        f.ais[0].state().attacks_launched
    );
    assert_eq!(f.ais[0].state().attacks_launched, 0);
    assert!(worst <= COMMANDS_PER_1000, "{worst}");
    assert!(most_orders_per_turn(&f) <= ARMY_COMMANDS_PER_TURN);
    // Measured: 19 army units at 40,000 (the army floor, then the goal list, then units again).
    assert!(army >= 15, "{army}");
}

// ---- ai_raid -----------------------------------------------------------------------------------------------

/// The AI's first power plant (slot 3), which the raiders attack, and the raiders (slots 20 to 22).
const RAIDED: EntityId = EntityId::new(3, 0);
const RAIDERS: [u32; 3] = [20, 21, 22];
/// Measured: the tick of the first hit on the plant, the last hit, the last raider dead, the posture back to
/// `Gather`.
const RAID_TICKS: (u64, u64, u64, u64) = (4022, 4582, 4587, 4990);

#[test]
fn a_raid_is_answered_within_one_think_and_the_army_goes_back_to_gathering_after_defend_ticks() {
    let s = load("ai_raid");
    let defend = u64::from(s.ais[0].state().profile.army.defend_ticks);
    assert_eq!(defend, 400);
    let (mut hp, mut first_hit, mut last_hit, mut dead, mut defend_at, mut gather_at) =
        (None, None, None, None, None, None);
    let f = run_observed(&s, &RunOptions::for_scenario(&s), |w, ais| {
        let t = w.tick().0 - 1;
        let now = w.snapshot().iter().find(|v| v.id == RAIDED).map(|v| v.hp);
        if let (Some(a), Some(b)) = (hp, now)
            && b < a
        {
            first_hit.get_or_insert(t);
            last_hit = Some(t);
        }
        hp = now;
        let alive = w
            .snapshot()
            .iter()
            .filter(|v| v.owner == P1 && RAIDERS.contains(&v.id.index()))
            .count();
        if t > 4001 && alive == 0 {
            dead.get_or_insert(t);
        }
        let posture = ais[0].state().directive.posture;
        if posture == Posture::Defend {
            defend_at.get_or_insert(t);
        }
        if defend_at.is_some() && posture == Posture::Gather {
            gather_at.get_or_insert(t);
        }
    })
    .unwrap();
    let (first, last, dead, defend_at, gather_at) = (
        first_hit.unwrap(),
        last_hit.unwrap(),
        dead.expect("the raiders die"),
        defend_at.unwrap(),
        gather_at.unwrap(),
    );
    println!(
        "first hit {first}, Defend from {defend_at}, last hit {last}, raiders dead {dead}, Gather from {gather_at}"
    );
    // Within one think (10 ticks) of the first hit.
    assert!(defend_at >= first && defend_at <= first + 10, "{defend_at}");
    // Defenders ordered within 40 ticks (one army turn), with an `AttackMove`.
    let ordered = f
        .ai_commands
        .iter()
        .find(|(_, _, c)| {
            matches!(
                c,
                Command::Order {
                    order: Order::AttackMove(_),
                    ..
                }
            )
        })
        .map(|c| c.0)
        .unwrap();
    assert!(ordered >= first && ordered <= first + 40, "{ordered}");
    // Back to gathering at the first think `defend_ticks` after the last hit.
    assert!(
        gather_at >= last + defend && gather_at <= last + defend + 10,
        "{gather_at}"
    );
    // And no attack-move or attack after that: the only orders left are the walk back to the gather cell.
    for (t, _, c) in f.ai_commands.iter().filter(|c| c.0 >= gather_at) {
        assert!(
            !matches!(
                c,
                Command::Order {
                    order: Order::AttackMove(_) | Order::Attack(_),
                    ..
                }
            ),
            "tick {t}: {c:?}"
        );
    }
    assert_eq!((first, last, dead, gather_at), RAID_TICKS);
}

// ---- a defeated AI -------------------------------------------------------------------------------------------

#[test]
fn a_defeated_ai_sends_nothing_and_its_state_stands_still() {
    // The AI's construction centre (slot 0) is removed at tick 6501 (WP9.1c-3: was 3001, before the AI had a unit; now
    // its first units stand, queued from 4,460): player 0 is defeated (and with two players the match is decided).
    // From then on the AI sends nothing and its digest does not move, though it still has a barracks, units and
    // credits (and a goal list and an army turn that would send orders).
    let mut s = load("ai_vs_passive");
    s.commands.push(TimedCommand {
        tick: 6501,
        player: 0,
        command: Action::Despawn {
            ids: vec![sf_headless::scenario::IdSpec::Id(0, 0)],
        },
    });
    let mut digest_at = BTreeMap::new();
    let f = run_observed(
        &s,
        &RunOptions {
            ticks: 9000,
            every: 100,
        },
        |w, ais| {
            if [6600, 9000].contains(&w.tick().0) {
                digest_at.insert(w.tick().0, ais[0].digest());
            }
        },
    )
    .unwrap();
    assert_eq!(f.world.defeat_tick(P0), Some(Tick(6501)));
    let after: Vec<_> = f.ai_commands.iter().filter(|c| c.0 > 6501).collect();
    println!("defeated at 6501; commands after: {}", after.len());
    assert!(after.is_empty(), "{after:?}");
    assert_eq!(digest_at[&6600], digest_at[&9000]);
    let mine = |pick: fn(Kind) -> bool| {
        f.world
            .snapshot()
            .iter()
            .filter(|v| v.owner == P0 && pick(v.kind))
            .count()
    };
    assert!(mine(|k| k == BARRACKS) >= 1);
    assert!(mine(|k| [RIFLEMAN, Kind(13), TANK].contains(&k)) >= 1);
    // It was acting before the defeat.
    assert!(f.ai_commands.iter().any(|c| c.0 > 4_000 && c.0 <= 6_501));
    // Control: without the defeat the same AI does send commands in 6,501..=9,000, so the silence above is the
    // defeated-AI rule and not an idle stretch.
    let control = run_observed(
        &load("ai_vs_passive"),
        &RunOptions {
            ticks: 9000,
            every: 100,
        },
        |_, _| {},
    )
    .unwrap();
    assert!(
        control
            .ai_commands
            .iter()
            .any(|c| c.0 > 6_501 && c.0 <= 9_000)
    );
    // The observation says so.
    let obs = Observation::build(&f.world, P0, &BTreeMap::new());
    assert!(obs.out_of_play);
}

// ---- the fence -------------------------------------------------------------------------------------------------

/// `ai_vs_passive` with three Veyran riflemen standing at `cells`.
fn with_riflemen_at(cells: [(u32, u32); 3]) -> Scenario {
    let mut s = load("ai_vs_passive");
    for cell in cells {
        s.commands.push(TimedCommand {
            tick: 0,
            player: 1,
            command: Action::SpawnUnit {
                kind: 12,
                owner: 1,
                cell,
            },
        });
    }
    s
}

#[test]
fn a_unit_hidden_in_the_reach_changes_nothing_the_ai_does_also_an_army_beside_the_route() {
    // Three enemy riflemen beside the army's way from the gather cell (24, 64) to the enemy centre (72, 14),
    // against the same three in a far corner nothing of the AI reaches: until their cell comes into view the
    // AI's commands and digest must be equal. Twice: with the scout (which sees them first), and without one,
    // so that the attack marches past them before they are seen. (The straight line crosses the lake in the
    // middle of the map, x 38..55, y 36..53; the cells are open ground at its north-east shore.)
    // (WP9.1c-3: the cells moved from (58, 34) to (57, 28): the first wave now carries light tanks and leaves at 11,150,
    // and its way passes the lake's north-east shore at y 25 to 30, not at y 34.)
    let near = [(57, 28), (58, 28), (57, 29)];
    let probe = Cell::new(57, 28);
    for scouts in [2u8, 0] {
        let scout = |p: &mut sf_ai::Profile| p.army.scouts_max = scouts;
        let s = with_profile(with_riflemen_at(near), scout);
        let mut seen_at = None;
        run_observed(&s, &RunOptions::for_scenario(&s), |w, _| {
            if seen_at.is_none() && w.visibility(P0, probe) == Visibility::Visible {
                seen_at = Some(w.tick().0);
            }
        })
        .unwrap();
        let until = seen_at.expect("the riflemen come into view") - 1;
        let a = go(&s, until);
        let b = go(
            &with_profile(with_riflemen_at([(90, 90), (91, 90), (90, 91)]), scout),
            until,
        );
        let attack_moves = a
            .ai_commands
            .iter()
            .filter(|c| {
                matches!(
                    c.2,
                    Command::Order {
                        order: Order::AttackMove(_),
                        ..
                    }
                )
            })
            .count();
        println!(
            "scouts_max {scouts}: the riflemen come into view at tick {seen_at:?}, {attack_moves} attack-moves before"
        );
        // The same entities in both worlds (so the same ids for everything the AI makes), in other places.
        assert_eq!(a.world.len(), b.world.len());
        assert_ne!(a.world.checksum(), b.world.checksum());
        assert_eq!(a.ai_commands, b.ai_commands, "scouts_max {scouts}");
        assert_eq!(a.ais[0].digest(), b.ais[0].digest(), "scouts_max {scouts}");
        if scouts == 0 {
            // Without a scout the attack is out before they are seen.
            assert!(attack_moves >= 1);
        }
    }
}

// ---- the rules, one army turn on an observation the test builds -----------------------------------------------

/// The world of `ai_vs_passive` after one tick and the AI's state.
/// The ids (indexes) still waiting for the defence order of a centre recall, over all groups.
fn recalled_of(s: &AiState) -> Vec<u32> {
    s.groups
        .iter()
        .flat_map(|g| g.recalled.iter().map(|i| i.index()))
        .collect()
}

fn rig() -> (World, AiState) {
    let f = go(&load("ai_vs_passive"), 1);
    (*f.world, f.ais[0].state().clone())
}

fn structure_at(world: &World, id: u32, kind: Kind, x: u32, y: u32) -> Own {
    Own {
        id: EntityId::new(id, 0),
        kind,
        cell: Cell::new(x, y),
        footprint: world.building_table().get(kind).map(|b| b.footprint),
        hp: 1000,
        order: Order::Idle,
        standing: true,
        max_hp: 1000,
        building: None,
        repairing: false,
        factory: None,
        selling: false,
        armed: false,
        mobile: false,
    }
}

fn soldier(id: u32, x: u32, y: u32) -> Own {
    Own {
        id: EntityId::new(id, 0),
        kind: RIFLEMAN,
        cell: Cell::new(x, y),
        footprint: None,
        hp: 100,
        order: Order::Idle,
        standing: true,
        max_hp: 100,
        building: None,
        repairing: false,
        factory: None,
        selling: false,
        armed: true,
        mobile: true,
    }
}

fn gather() -> Cell {
    Cell::new(24, 64)
}

/// A base whose barracks rallies to the gather cell: centre 0, plants 1 and 5, refinery 2, barracks 3, two
/// harvesters 10 and 11 (`ai_production.rs` has the same).
fn base(world: &World) -> Vec<Own> {
    let mut barracks = structure_at(world, 3, BARRACKS, 12, 60);
    barracks.factory = Some(OwnFactory {
        queue: Vec::new(),
        front_paid: 0,
        status: FactoryStatus::Building,
        rally: Some(gather()),
    });
    let harvester = |id, x| {
        let mut o = soldier(id, x, 80);
        o.kind = HARVESTER;
        o.armed = false;
        o
    };
    vec![
        structure_at(world, 0, CENTRE, 12, 68),
        structure_at(world, 1, PLANT, 18, 60),
        structure_at(world, 2, REFINERY, 18, 74),
        barracks,
        structure_at(world, 5, PLANT, 8, 60),
        harvester(10, 15),
        harvester(11, 16),
    ]
}

fn observation(world: &World, tick: u64, own: Vec<Own>) -> Observation {
    let mut obs = Observation::build(world, P0, &BTreeMap::new());
    obs.tick = Tick(tick);
    obs.credits = 0;
    obs.own = own;
    obs.own.sort_by_key(|o| o.id);
    obs
}

fn army_turn(world: &World, state: &mut AiState, obs: &Observation) -> Vec<Command> {
    let mut lens = Lens::new(world, P0);
    let mut out = Vec::new();
    army::turn(state, obs, &mut lens, &mut out);
    out
}

/// A state with the gather cell set and no scout, at its start otherwise.
fn army_state() -> (World, AiState) {
    let (w, mut s) = rig();
    s.gather = Some(gather());
    s.profile.scout = None;
    (w, s)
}

/// `n` riflemen idle at the gather cell, ids from 20.
fn gathered_soldiers(n: u32) -> Vec<Own> {
    (0..n)
        .map(|i| soldier(20 + i, 24 + i % 4, 64 + i / 4))
        .collect()
}

fn rifleman_cost(world: &World) -> u64 {
    u64::from(Lens::new(world, P0).unit_cost(RIFLEMAN).unwrap())
}

fn attack_moves(out: &[Command]) -> Vec<(Vec<u32>, Vec2)> {
    out.iter()
        .filter_map(|c| match c {
            Command::Order {
                ids,
                order: Order::AttackMove(p),
                ..
            } => Some((ids.iter().map(|i| i.index()).collect(), *p)),
            _ => None,
        })
        .collect()
}

/// The enemy construction centre of `ai_vs_passive` in the AI's memory.
fn remember_enemy_centre(state: &mut AiState) {
    state.known_structures.insert(
        EntityId::new(11, 0),
        Known {
            kind: CENTRE,
            owner: P1,
            anchor: Cell::new(72, 14),
            width: 4,
            height: 4,
            seen_tick: Tick(100),
        },
    );
}

#[test]
fn the_attack_goes_at_attack_value_and_not_one_credit_below() {
    let (world, base_state) = army_state();
    let c = rifleman_cost(&world);
    println!("a rifleman costs {c}");
    let mut own = base(&world);
    own.extend(gathered_soldiers(6));
    let obs = observation(&world, 3000, own);
    // Six riflemen gathered: value 6 c. With attack_value one above, nothing; at it, the attack goes.
    let mut s = base_state.clone();
    s.profile.army.attack_value = (6 * c + 1) as u32;
    assert!(attack_moves(&army_turn(&world, &mut s, &obs)).is_empty());
    assert_eq!(s.attacks_launched, 0);
    let mut s = base_state.clone();
    s.profile.army.attack_value = (6 * c) as u32;
    let out = army_turn(&world, &mut s, &obs);
    assert_eq!(s.attacks_launched, 1);
    assert_eq!(s.directive.posture, Posture::Attack);
    assert_eq!(s.attackers().len(), 6);
    assert_eq!(s.last_attack_tick, Tick(3000));
    // One `AttackMove` for all six, to a cell beside the enemy's start site (72, 14) facing the gather cell.
    let moves = attack_moves(&out);
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].0, [20, 21, 22, 23, 24, 25]);
    let t = s.attack_target().unwrap();
    assert!(t.x.abs_diff(73) <= 3 && t.y.abs_diff(15) <= 3, "{t:?}");
    assert_eq!(moves[0].1, middle(t));
    // The shipped threshold, a literal: 2,000 (WP9.1c-3, was 3,000), equal to the army floor: the first wave is value-driven.
    assert_eq!(base_state.profile.army.attack_value, 2000);
    assert_eq!(
        base_state.profile.army.attack_value,
        base_state.profile.army.army_floor_value
    );
}

#[test]
fn never_before_first_attack_tick_and_max_wait_needs_min_units() {
    let (world, mut base_state) = army_state();
    base_state.profile.army.attack_value = 1_000_000;
    let a = base_state.profile.army;
    assert_eq!(
        (a.first_attack_tick, a.max_wait, a.min_units),
        (2400, 12_000, 4)
    );
    let run = |n: u32, tick: u64| {
        let mut s = base_state.clone();
        let mut own = base(&world);
        own.extend(gathered_soldiers(n));
        army_turn(&world, &mut s, &observation(&world, tick, own));
        s.attacks_launched
    };
    // `max_wait` after the last attack (none yet: tick 0) with `min_units` gathered: the attack goes; with one
    // fewer it does not; one tick before `max_wait` neither.
    assert_eq!(run(4, 12_000), 1);
    assert_eq!(run(3, 12_000), 0);
    assert_eq!(run(4, 11_999), 0);
    // Before `first_attack_tick` never, whatever the value.
    let mut s = base_state.clone();
    s.profile.army.attack_value = 1;
    s.profile.army.max_wait = 1;
    let mut own = base(&world);
    own.extend(gathered_soldiers(20));
    army_turn(&world, &mut s, &observation(&world, 2399, own.clone()));
    assert_eq!(s.attacks_launched, 0);
    army_turn(&world, &mut s, &observation(&world, 2400, own));
    assert_eq!(s.attacks_launched, 1);
}

#[test]
fn later_waves_are_max_wait_apart_unless_attack_value_is_reached() {
    // WP9.1c-3: `max_wait` is also the least gap between two waves (`attack_due`: `max_wait` after the last attack, with
    // `min_units`), so with `attack_value` equal to the army floor a second wave goes at once when the army is worth
    // 2,000 again, and otherwise only 12,000 ticks after the first. Production code: `army::attack_due`.
    let (_world, s0) = army_state();
    let mut s = s0.clone();
    s.last_attack_tick = Tick(5000);
    s.attacks_launched = 1;
    let (value, wait) = (
        u64::from(s.profile.army.attack_value),
        u64::from(s.profile.army.max_wait),
    );
    assert_eq!((value, wait), (2000, 12_000));
    let due = |now: u64, v: u64, n: usize| army::attack_due(&s, Tick(now), v, n);
    // Worth `attack_value`: at once, one credit less: not.
    assert!(due(5040, value, 6));
    assert!(!due(5040, value - 1, 6));
    // Below it: only `max_wait` after the last attack, and only with `min_units`.
    assert!(!due(5000 + wait - 1, value - 1, 4));
    assert!(due(5000 + wait, value - 1, 4));
    assert!(!due(5000 + wait, value - 1, 3));
}

#[test]
fn only_idle_units_within_the_gather_radius_count_and_go() {
    let (world, mut s) = army_state();
    let c = rifleman_cost(&world);
    s.profile.army.attack_value = (4 * c) as u32;
    let mut own = base(&world);
    own.extend(gathered_soldiers(3));
    // A fourth one nine cells from the gather cell (radius 8), and a fifth at the cell but walking.
    own.push(soldier(30, 33, 64));
    let mut walking = soldier(31, 25, 65);
    walking.order = Order::Move(middle(gather()));
    own.push(walking);
    army_turn(&world, &mut s, &observation(&world, 3000, own.clone()));
    assert_eq!(s.attacks_launched, 0);
    // Within the radius: now four.
    own.retain(|o| o.id.index() != 30);
    own.push(soldier(30, 32, 64));
    let out = army_turn(&world, &mut s, &observation(&world, 3040, own));
    assert_eq!(s.attacks_launched, 1);
    assert_eq!(
        s.attackers().iter().map(|i| i.index()).collect::<Vec<_>>(),
        [20, 21, 22, 30]
    );
    assert!(attack_moves(&out).iter().all(|(ids, _)| !ids.contains(&31)));
}

#[test]
fn attackers_are_ordered_only_when_idle_and_not_again_within_resend_ticks() {
    let (world, mut s) = army_state();
    s.profile.army.attack_value = 1;
    let mut own = base(&world);
    own.extend(gathered_soldiers(4));
    let out = army_turn(&world, &mut s, &observation(&world, 3000, own.clone()));
    assert_eq!(attack_moves(&out).len(), 1);
    // 40 ticks on, still idle where they were (the order not yet seen): within `resend_ticks` (120) nothing.
    let out = army_turn(&world, &mut s, &observation(&world, 3040, own.clone()));
    assert!(attack_moves(&out).is_empty(), "{out:?}");
    // Marching: never ordered, also after `resend_ticks`.
    let marching: Vec<Own> = own
        .iter()
        .cloned()
        .map(|mut o| {
            if o.armed {
                o.order = Order::AttackMove(middle(s.attack_target().unwrap()));
            }
            o
        })
        .collect();
    let out = army_turn(&world, &mut s, &observation(&world, 3200, marching));
    assert!(out.is_empty(), "{out:?}");
    // Idle again after `resend_ticks` far from the target (refused, or gave the route up): ordered again.
    let out = army_turn(&world, &mut s, &observation(&world, 3240, own));
    assert_eq!(attack_moves(&out).len(), 1);
    assert_eq!(s.profile.army.resend_ticks, 120);
}

/// An enemy structure of player 1 that the AI sees now.
fn seen_structure(world: &World, id: u32, kind: Kind, x: u32, y: u32) -> Seen {
    Seen {
        id: EntityId::new(id, 0),
        kind,
        owner: P1,
        cell: Cell::new(x, y),
        footprint: world.building_table().get(kind).map(|b| b.footprint),
        hp: 1000,
        armed: false,
    }
}

fn attacks(out: &[Command]) -> Vec<(Vec<u32>, EntityId)> {
    out.iter()
        .filter_map(|c| match c {
            Command::Order {
                ids,
                order: Order::Attack(v),
                ..
            } => Some((ids.iter().map(|i| i.index()).collect(), *v)),
            _ => None,
        })
        .collect()
}

/// A state with an attack out on the enemy centre (remembered at (72, 14)) and four attackers idle beside it.
fn at_the_target() -> (World, AiState, Vec<Own>) {
    let (world, mut s) = army_state();
    remember_enemy_centre(&mut s);
    s.profile.army.attack_value = 1;
    let mut own = base(&world);
    own.extend(gathered_soldiers(4));
    army_turn(&world, &mut s, &observation(&world, 3000, own.clone()));
    let t = s.attack_target().unwrap();
    for o in own.iter_mut().filter(|o| o.armed) {
        o.cell = Cell::new(t.x, t.y + (o.id.index() - 20));
    }
    (world, s, own)
}

#[test]
fn idle_attackers_at_the_target_finish_the_visible_centre_and_never_an_unseen_one() {
    let (world, s0, own) = at_the_target();
    // The finishing rule: the centre is visible, so one `Attack` on it for the four. A power plant nearer to
    // the target is not chosen: the centre goes first.
    let mut s = s0.clone();
    let mut obs = observation(&world, 3200, own.clone());
    obs.visible.push(seen_structure(&world, 4, PLANT, 70, 18));
    obs.visible.push(seen_structure(&world, 11, CENTRE, 72, 14));
    let out = army_turn(&world, &mut s, &obs);
    assert_eq!(
        attacks(&out),
        [(vec![20, 21, 22, 23], EntityId::new(11, 0))]
    );
    // Without the centre in sight, the nearest visible structure.
    let mut s = s0.clone();
    let mut obs = observation(&world, 3200, own.clone());
    obs.visible.push(seen_structure(&world, 4, PLANT, 70, 18));
    let out = army_turn(&world, &mut s, &obs);
    assert_eq!(attacks(&out), [(vec![20, 21, 22, 23], EntityId::new(4, 0))]);
    // The fence: the centre is remembered but not seen now: no `Attack` at all (an id from memory is never
    // attacked), and never a unit.
    let mut s = s0.clone();
    let mut obs = observation(&world, 3200, own);
    obs.visible.push(Seen {
        id: EntityId::new(40, 0),
        kind: RIFLEMAN,
        owner: P1,
        cell: Cell::new(73, 20),
        footprint: None,
        hp: 100,
        armed: true,
    });
    let out = army_turn(&world, &mut s, &obs);
    assert!(attacks(&out).is_empty(), "{out:?}");
}

#[test]
fn an_attack_ends_with_its_attackers_its_target_or_its_patience_and_the_survivors_go_home() {
    let (world, s0, own) = at_the_target();
    let moves_home = |out: &[Command]| {
        out.iter()
            .filter(|c| {
                matches!(c, Command::Order { order: Order::Move(p), .. } if *p == middle(gather()))
            })
            .count()
    };
    // Every attacker dead: back to gathering.
    let mut s = s0.clone();
    let base_only = base(&world);
    army_turn(&world, &mut s, &observation(&world, 3200, base_only));
    assert!(s.attackers().is_empty());
    assert_eq!(s.directive.posture, Posture::Gather);
    // No target left (the centre seen gone, the start site seen empty, nothing else known): home.
    let mut s = s0.clone();
    s.known_structures.clear();
    s.sites_cleared.push(P1);
    let out = army_turn(&world, &mut s, &observation(&world, 3200, own.clone()));
    assert_eq!(moves_home(&out), 1);
    assert_eq!(s.directive.posture, Posture::Gather);
    // `attack_patience` (2,400) with no attacker within `arrive_radius` (6) of the target: home.
    let far: Vec<Own> = own
        .iter()
        .cloned()
        .map(|mut o| {
            if o.armed {
                o.cell = Cell::new(40, 40);
                o.order = Order::AttackMove(middle(Cell::new(70, 16)));
            }
            o
        })
        .collect();
    let mut s = s0.clone();
    s.attack_started = Tick(3000);
    army_turn(&world, &mut s, &observation(&world, 5399, far.clone()));
    assert_eq!(s.attackers().len(), 4);
    let out = army_turn(&world, &mut s, &observation(&world, 5400, far));
    assert!(s.attackers().is_empty());
    assert_eq!(moves_home(&out), 1);
    assert_eq!(
        (
            s0.profile.army.attack_patience,
            s0.profile.army.arrive_radius
        ),
        (2400, 6)
    );
}

#[test]
fn the_target_is_the_known_centre_then_the_start_site_then_any_known_structure() {
    let (world, mut s) = army_state();
    let lens = Lens::new(&world, P0);
    // Nothing known: the start site of player 1, (72, 14), a cell beside its 4 x 4 facing the gather cell.
    let site = army::target(&s, &lens, gather()).unwrap();
    assert!(
        (71..=76).contains(&site.x) && (13..=18).contains(&site.y),
        "{site:?}"
    );
    assert!(!(72..76).contains(&site.x) || !(14..18).contains(&site.y));
    // A remembered plant elsewhere does not take it; a remembered centre does.
    s.known_structures.insert(
        EntityId::new(4, 0),
        Known {
            kind: PLANT,
            owner: P1,
            anchor: Cell::new(40, 40),
            width: 2,
            height: 2,
            seen_tick: Tick(9),
        },
    );
    assert_eq!(army::target(&s, &lens, gather()), Some(site));
    s.known_structures.insert(
        EntityId::new(30, 0),
        Known {
            kind: CENTRE,
            owner: P1,
            anchor: Cell::new(60, 30),
            width: 4,
            height: 4,
            seen_tick: Tick(9),
        },
    );
    let c = army::target(&s, &lens, gather()).unwrap();
    assert!(
        (59..=64).contains(&c.x) && (29..=34).contains(&c.y),
        "{c:?}"
    );
    // With no centre known and the start site seen empty: the nearest known structure, the plant.
    s.known_structures.remove(&EntityId::new(30, 0));
    s.sites_cleared.push(P1);
    let p = army::target(&s, &lens, gather()).unwrap();
    assert!(
        (37..=44).contains(&p.x) && (37..=44).contains(&p.y),
        "{p:?}"
    );
    assert!(
        !(40..42).contains(&p.x) || !(40..42).contains(&p.y),
        "{p:?}"
    );
    s.known_structures.clear();
    assert_eq!(army::target(&s, &lens, gather()), None);
}

// ---- the threat rule, at every think ----------------------------------------------------------------------------

fn watch(world: &World, state: &mut AiState, obs: &Observation) {
    army::watch(state, obs, &Lens::new(world, P0));
}

#[test]
fn a_hit_turns_the_posture_to_defend_a_repair_or_a_sale_does_not() {
    let (world, s0) = army_state();
    let own = base(&world);
    let mut s = s0.clone();
    watch(&world, &mut s, &observation(&world, 100, own.clone()));
    assert_eq!(s.hp_seen.len(), 5);
    assert_eq!(s.directive.posture, Posture::Gather);
    // A repair raises the plant's hit points: no hit. (Start lower first.)
    let mut low = own.clone();
    low[1].hp = 500;
    let mut s = s0.clone();
    s.hp_seen.clear();
    watch(&world, &mut s, &observation(&world, 100, low.clone()));
    watch(&world, &mut s, &observation(&world, 110, own.clone()));
    assert!(s.threat.is_none());
    // A sale: the economy drops the structure from `hp_seen` when it sends it; the structure being sold and
    // then gone is no hit.
    let mut s = s0.clone();
    watch(&world, &mut s, &observation(&world, 100, own.clone()));
    s.sales.push((EntityId::new(5, 0), Tick(100)));
    s.hp_seen.remove(&EntityId::new(5, 0));
    let mut selling = own.clone();
    selling[4].selling = true;
    watch(&world, &mut s, &observation(&world, 110, selling));
    let gone: Vec<Own> = own
        .iter()
        .filter(|o| o.id != EntityId::new(5, 0))
        .cloned()
        .collect();
    watch(&world, &mut s, &observation(&world, 120, gone.clone()));
    assert!(s.threat.is_none());
    // A structure gone without a sale is a hit, and so is one that lost hit points.
    let mut s = s0.clone();
    watch(&world, &mut s, &observation(&world, 100, own.clone()));
    watch(&world, &mut s, &observation(&world, 110, gone));
    assert_eq!(s.directive.posture, Posture::Defend);
    assert_eq!(s.threat.map(|t| t.1), Some(Tick(110)));
    let mut s = s0.clone();
    watch(&world, &mut s, &observation(&world, 100, own.clone()));
    watch(&world, &mut s, &observation(&world, 110, low));
    assert_eq!(s.directive.posture, Posture::Defend);
    // `defend_ticks` after the last hit, back to gathering; one tick before, not yet.
    watch(&world, &mut s, &observation(&world, 509, own.clone()));
    assert_eq!(s.directive.posture, Posture::Defend);
    watch(&world, &mut s, &observation(&world, 510, own));
    assert_eq!(s.directive.posture, Posture::Gather);
    assert!(s.threat.is_none());
}

#[test]
fn the_units_at_home_defend_and_only_a_hit_on_the_centre_calls_the_attack_back() {
    let (world, s0, own) = at_the_target();
    // Two more soldiers at home, at the edge of the gather area, more than `arrive_radius` from the plant.
    let mut own = own;
    own.push(soldier(40, 30, 70));
    own.push(soldier(41, 31, 70));
    let mut s = s0.clone();
    watch(&world, &mut s, &observation(&world, 3100, own.clone()));
    // The power plant (1) is hit: the units at home attack-move there, the attackers stay on the attack.
    let mut hit = own.clone();
    hit[1].hp = 900;
    watch(&world, &mut s, &observation(&world, 3110, hit.clone()));
    assert_eq!(s.directive.posture, Posture::Defend);
    assert_eq!(s.attackers().len(), 4);
    let out = army_turn(&world, &mut s, &observation(&world, 3120, hit.clone()));
    let moves = attack_moves(&out);
    assert_eq!(moves[0].0, [40, 41]);
    // The construction centre (0) is hit: the attack is called back and everyone defends.
    let mut s = s0.clone();
    watch(&world, &mut s, &observation(&world, 3100, own.clone()));
    let mut hit = own;
    hit[0].hp = 900;
    watch(&world, &mut s, &observation(&world, 3110, hit.clone()));
    assert!(s.attackers().is_empty());
    let out = army_turn(&world, &mut s, &observation(&world, 3120, hit));
    let moves = attack_moves(&out);
    assert_eq!(moves[0].0, [20, 21, 22, 23, 40, 41]);
    assert!(out.len() <= ARMY_COMMANDS_PER_TURN);
}

// ---- the army floor, in the production turn --------------------------------------------------------------------

fn production_turn(world: &World, state: &mut AiState, obs: &Observation) -> Vec<Command> {
    let mut lens = Lens::new(world, P0);
    let mut out = Vec::new();
    production::turn(state, obs, &mut lens, &mut out);
    out
}

/// A state whose goal list still wants a vehicle factory (1,500): production holds back the reserve and that
/// price, unless the floor says otherwise. Riflemen only.
fn floor_state() -> (World, AiState) {
    let (w, mut s) = army_state();
    s.profile.build_order = vec![(PLANT, 2), (REFINERY, 1), (BARRACKS, 1), (FACTORY, 1)];
    s.profile.unit_mix = vec![(RIFLEMAN, 1)];
    (w, s)
}

fn queued_riflemen(out: &[Command]) -> usize {
    out.iter()
        .filter(|c| matches!(c, Command::QueueUnit { kind, .. } if *kind == RIFLEMAN))
        .count()
}

#[test]
fn below_the_army_floor_production_holds_back_only_the_reserve() {
    let (world, s0) = floor_state();
    let c = rifleman_cost(&world);
    let floor = u64::from(s0.profile.army.army_floor_value);
    assert_eq!(floor, 2000);
    let reserve = u64::from(s0.profile.economy.reserve);
    // Credits for the reserve and one rifleman, far below the factory's 1,500.
    let credits = reserve + c;
    let obs = |own: Vec<Own>| {
        let mut o = observation(&world, 3000, own);
        o.credits = credits;
        o
    };
    // No army: below the floor, a rifleman is queued.
    let mut s = s0.clone();
    assert_eq!(
        queued_riflemen(&production_turn(&world, &mut s, &obs(base(&world)))),
        1
    );
    // An army worth the floor: the goal's price is held back again, nothing is queued.
    let n = floor.div_ceil(c) as u32;
    let mut own = base(&world);
    own.extend(gathered_soldiers(n));
    let mut s = s0.clone();
    assert_eq!(
        queued_riflemen(&production_turn(&world, &mut s, &obs(own.clone()))),
        0
    );
    // One rifleman short of the floor: one more.
    let mut short = base(&world);
    short.extend(gathered_soldiers(n - 1));
    let mut s = s0.clone();
    assert_eq!(
        queued_riflemen(&production_turn(&world, &mut s, &obs(short.clone()))),
        1
    );
    // After the first attack the floor is off (the guard of decision 1) ...
    let mut s = s0.clone();
    s.attacks_launched = 1;
    assert_eq!(
        queued_riflemen(&production_turn(&world, &mut s, &obs(short.clone()))),
        0
    );
    // ... unless the army defends.
    let mut s = s0.clone();
    s.attacks_launched = 1;
    s.directive.posture = Posture::Defend;
    assert_eq!(
        queued_riflemen(&production_turn(&world, &mut s, &obs(short))),
        1
    );
}

#[test]
fn the_army_value_counts_the_queue_and_what_this_turn_ordered() {
    let (world, s0) = floor_state();
    let c = rifleman_cost(&world);
    let floor = u64::from(s0.profile.army.army_floor_value);
    let n = floor.div_ceil(c) as u32;
    // Two barracks, money for two riflemen above the reserve, an army one rifleman short of the floor: only one
    // is queued (the second would overshoot the floor: the first one ordered this turn counts).
    let mut own = base(&world);
    let mut second = structure_at(&world, 6, BARRACKS, 4, 60);
    second.factory = own[3].factory.clone();
    own.push(second);
    own.extend(gathered_soldiers(n - 1));
    let mut obs = observation(&world, 3000, own.clone());
    obs.credits = u64::from(s0.profile.economy.reserve) + 2 * c;
    let mut s = s0.clone();
    assert_eq!(queued_riflemen(&production_turn(&world, &mut s, &obs)), 1);
    // A rifleman in a queue counts as army: with one queued and one short, nothing.
    own[3]
        .factory
        .as_mut()
        .unwrap()
        .queue
        .push((RIFLEMAN, false));
    let mut obs = observation(&world, 3000, own);
    obs.credits = u64::from(s0.profile.economy.reserve) + 2 * c;
    let mut s = s0.clone();
    let v = production::army_value(&s, &obs, &Lens::new(&world, P0));
    assert_eq!(v, u64::from(n) * c);
    assert_eq!(queued_riflemen(&production_turn(&world, &mut s, &obs)), 0);
    let _ = TANK;
}

// ---- fix round 1 (sweep of PR #129, decisions of 12-11-2026) ---------------------------------------------------

#[test]
fn a_hit_on_the_centre_calls_back_a_wave_that_is_still_marching() {
    // The attackers are on their `AttackMove`, half-way (not idle at the target): a hit on the construction centre
    // turns them round all the same, once, with the defence `AttackMove`.
    let (world, s0, own) = at_the_target();
    let marching: Vec<Own> = own
        .into_iter()
        .map(|mut o| {
            if o.armed {
                o.cell = Cell::new(48, 30);
                o.order = Order::AttackMove(middle(Cell::new(70, 16)));
            }
            o
        })
        .collect();
    let mut s = s0.clone();
    watch(&world, &mut s, &observation(&world, 3100, marching.clone()));
    let mut hit = marching.clone();
    hit[0].hp = 900;
    watch(&world, &mut s, &observation(&world, 3110, hit.clone()));
    assert!(s.attackers().is_empty());
    assert_eq!(recalled_of(&s), [20, 21, 22, 23]);
    let out = army_turn(&world, &mut s, &observation(&world, 3240, hit.clone()));
    let moves = attack_moves(&out);
    assert_eq!(moves.len(), 1, "{out:?}");
    assert_eq!(moves[0].0, [20, 21, 22, 23]);
    assert_eq!(moves[0].1, middle(s.threat.unwrap().0));
    // Once: now on the defence order, they are left alone.
    assert!(recalled_of(&s).is_empty());
    let defending: Vec<Own> = hit
        .into_iter()
        .map(|mut o| {
            if o.armed {
                o.order = Order::AttackMove(middle(s.threat.unwrap().0));
            }
            o
        })
        .collect();
    let out = army_turn(&world, &mut s, &observation(&world, 3400, defending));
    assert!(attack_moves(&out).is_empty(), "{out:?}");
}

#[test]
fn a_centre_recall_leaves_no_attacker_on_the_closing_list() {
    // WP9.1c-3 (the re-review of #129): `closing` holds the attackers that were given the finishing Attack; a
    // recall ends the attack without `end_attack`, which used to leave their ids on the list (the reviewer's
    // probe left 4).
    let (world, s0, own) = at_the_target();
    let mut s = s0.clone();
    watch(&world, &mut s, &observation(&world, 3100, own.clone()));
    let members = s.attackers();
    s.groups[0].closing = members;
    assert_eq!(s.groups[0].closing.len(), 4);
    let mut hit = own;
    hit[0].hp = 900;
    watch(&world, &mut s, &observation(&world, 3110, hit));
    assert!(s.attackers().is_empty());
    assert_eq!(recalled_of(&s).len(), 4);
    assert!(s.groups[0].closing.is_empty(), "{:?}", s.groups[0].closing);
}

/// `s` with its match file changed by `change` (the starting objects made again, as `Scenario::load` does).
fn with_match(mut s: Scenario, change: impl FnOnce(&mut sf_data::match_def::MatchDef)) -> Scenario {
    let mut def = s.match_def.clone().unwrap();
    change(&mut def);
    let defs =
        sf_data::units::UnitDefs::load(&scenario_path("ai_vs_passive_defs"), &scenario_path(".."))
            .unwrap();
    s.match_commands = sf_bridge::match_commands(&def, &defs).unwrap();
    s.match_def = Some(def);
    s
}

#[test]
fn a_defeated_ai_stops_also_while_the_match_goes_on() {
    // Three players: a third (a copy of player 1's construction centre in the south-east) keeps the match
    // undecided when player 0 is defeated. So only the defeat clause of `out_of_play` can stop the AI.
    let mut s = with_match(load("ai_vs_passive"), |def| {
        let mut third = def.players[1].clone();
        third.id = 2;
        third.objects.retain(|o| o.role == "construction_center");
        third.objects[0].cell = (80, 80);
        def.players.push(third);
    });
    s.commands.push(TimedCommand {
        tick: 6501,
        player: 0,
        command: Action::Despawn {
            ids: vec![sf_headless::scenario::IdSpec::Id(0, 0)],
        },
    });
    let mut digest_at = BTreeMap::new();
    let f = run_observed(
        &s,
        &RunOptions {
            ticks: 9000,
            every: 100,
        },
        |w, ais| {
            if [6600, 9000].contains(&w.tick().0) {
                digest_at.insert(w.tick().0, ais[0].digest());
            }
        },
    )
    .unwrap();
    assert_eq!(f.world.defeat_tick(P0), Some(Tick(6501)));
    assert_eq!(f.world.match_result(), None, "the match goes on");
    let obs = Observation::build(&f.world, P0, &BTreeMap::new());
    assert!(obs.out_of_play);
    let after: Vec<_> = f.ai_commands.iter().filter(|c| c.0 > 6501).collect();
    assert!(after.is_empty(), "{after:?}");
    assert_eq!(digest_at[&6600], digest_at[&9000]);
}

#[test]
fn the_army_floor_ends_at_max_wait_before_the_first_attack() {
    // Units lost at the gather cell (no structure hit, so no defence) keep the army below the floor and no attack
    // goes; the floor still lifts at `max_wait` (decision of 12-11-2026), so it never holds the goal list longer.
    let (world, s0) = floor_state();
    let c = rifleman_cost(&world);
    let credits = u64::from(s0.profile.economy.reserve) + c;
    let max_wait = u64::from(s0.profile.army.max_wait);
    assert_eq!(max_wait, 12_000);
    let at = |tick: u64| {
        let mut o = observation(&world, tick, base(&world));
        o.credits = credits;
        o
    };
    let mut s = s0.clone();
    assert_eq!(
        queued_riflemen(&production_turn(&world, &mut s, &at(max_wait - 40))),
        1
    );
    let mut s = s0.clone();
    assert_eq!(
        queued_riflemen(&production_turn(&world, &mut s, &at(max_wait))),
        0
    );
    // A defence brings it back.
    let mut s = s0.clone();
    s.directive.posture = Posture::Defend;
    assert_eq!(
        queued_riflemen(&production_turn(&world, &mut s, &at(max_wait + 4000))),
        1
    );
}

#[test]
fn an_attack_ends_when_the_target_stops_losing_hit_points_for_attack_patience() {
    // Four attackers stand at the visible enemy centre and it does not lose hit points: `attack_patience` (2,400)
    // after they got there the attack ends, although they are at the target all the time.
    let (world, s0, own) = at_the_target();
    let obs_at = |tick: u64, hp: u32| {
        let mut o = observation(&world, tick, own.clone());
        let mut centre = seen_structure(&world, 11, CENTRE, 72, 14);
        centre.hp = hp;
        o.visible.push(centre);
        o
    };
    let mut s = s0.clone();
    army_turn(&world, &mut s, &obs_at(3200, 2000));
    assert_eq!(
        s.strike().unwrap().target_hp,
        Some((EntityId::new(11, 0), 2000, Tick(3200)))
    );
    army_turn(&world, &mut s, &obs_at(5560, 2000));
    assert_eq!(s.attackers().len(), 4);
    army_turn(&world, &mut s, &obs_at(5600, 2000));
    assert!(
        s.attackers().is_empty(),
        "stuck at the target: the attack ends"
    );
    // The control: hit points that keep dropping keep the attack going.
    let mut s = s0.clone();
    for (i, t) in (3200..=6000).step_by(400).enumerate() {
        army_turn(&world, &mut s, &obs_at(t, 2000 - 10 * i as u32));
    }
    assert_eq!(s.attackers().len(), 4);
}

/// An armed rifleman of player 1 that the AI sees at `(x, y)`.
fn seen_raider(id: u32, x: u32, y: u32, armed: bool) -> Seen {
    Seen {
        id: EntityId::new(id, 0),
        kind: RIFLEMAN,
        owner: P1,
        cell: Cell::new(x, y),
        footprint: None,
        hp: 100,
        armed,
    }
}

#[test]
fn the_defence_goes_to_the_nearest_seen_armed_raider_near_the_hit_structure() {
    // The power plant (id 1, 2 x 2 at (18, 60)) is hit. The threat cell is the cell of the nearest seen armed unit
    // of another player within `gather_radius` (8) of its footprint; an unarmed one or one farther does not count,
    // and with none the cell is beside the plant.
    let (world, s0) = army_state();
    let own = base(&world);
    let mut hit = own.clone();
    hit[1].hp = 900;
    let cell_with = |visible: Vec<Seen>| {
        let mut s = s0.clone();
        watch(&world, &mut s, &observation(&world, 100, own.clone()));
        let mut o = observation(&world, 110, hit.clone());
        o.visible = visible;
        watch(&world, &mut s, &o);
        s.threat.unwrap().0
    };
    let beside = cell_with(vec![]);
    assert!(
        beside.x.abs_diff(19) <= 4 && beside.y.abs_diff(61) <= 4,
        "{beside:?}"
    );
    assert!(!(18..20).contains(&beside.x) || !(60..62).contains(&beside.y));
    // Two armed raiders, 3 and 6 cells off: the nearer.
    assert_eq!(
        cell_with(vec![
            seen_raider(40, 26, 61, true),
            seen_raider(41, 22, 61, true)
        ]),
        Cell::new(22, 61)
    );
    // Unarmed: ignored.
    assert_eq!(cell_with(vec![seen_raider(41, 22, 61, false)]), beside);
    // Armed but 9 cells beyond the footprint's edge (x 20 + 8 is the last cell in reach): ignored; at the edge: taken.
    assert_eq!(cell_with(vec![seen_raider(42, 28, 61, true)]), beside);
    assert_eq!(
        cell_with(vec![seen_raider(42, 27, 61, true)]),
        Cell::new(27, 61)
    );
}

#[test]
fn a_hidden_raider_in_reach_of_the_raided_plant_changes_nothing_until_it_is_seen() {
    // `ai_raid` with a fourth rifleman of player 1 put in fog within 8 cells of the raided plant at tick 4,003 (after the raiders,
    // so the slots the script names stay the same), against the same one in the far north-east. Until the AI sees
    // it the commands and the digest are equal: the threat cell uses only what is seen.
    let with_extra = |cell: (u32, u32)| {
        let mut s = load("ai_raid");
        s.commands.push(TimedCommand {
            tick: 4003,
            player: 1,
            command: Action::SpawnUnit {
                kind: 12,
                owner: 1,
                cell,
            },
        });
        s
    };
    // Open ground (the map's terrain): the first cell south-west of the base, and one in the far north-east.
    let (rig_world, _) = rig();
    let ground = |xs: std::ops::Range<u32>, ys: std::ops::Range<u32>| {
        ys.flat_map(|y| xs.clone().map(move |x| (x, y)))
            .find(|&(x, y)| {
                rig_world
                    .terrain()
                    .flags_at(Cell::new(x, y))
                    .is_some_and(|f| f.ground)
            })
            .unwrap()
    };
    // Fix round 2: `near` is a ground cell in fog at tick 4,003 within `threat_cell`'s reach (8 cells) of the
    // raided plant (2 x 2 at (18, 60)), the one farthest from it, so the visibility rule is what keeps it out.
    let base_run = go(&load("ai_raid"), 4003);
    let near = (52..70u32)
        .flat_map(|y| (10..28u32).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let c = Cell::new(x, y);
            rig_world.terrain().flags_at(c).is_some_and(|f| f.ground)
                && base_run.world.visibility(P0, c) != Visibility::Visible
        })
        .max_by_key(|&(x, y)| {
            (
                x.abs_diff(19).max(y.abs_diff(61)),
                std::cmp::Reverse((y, x)),
            )
        })
        .expect("a fogged cell beside the plant");
    assert!(
        near.0 + 8 >= 18 && near.0 < 28 && near.1 + 8 >= 60 && near.1 < 70,
        "{near:?}"
    );
    let far = ground(86..94, 2..8);
    let s = with_extra(near);
    // Seen: the cell the extra rifleman stands in (it may move) is visible to player 0.
    let mut seen_at = None;
    run_observed(&s, &RunOptions::for_scenario(&s), |w, _| {
        if seen_at.is_none()
            && let Some(v) = w
                .snapshot()
                .iter()
                .find(|v| v.owner == P1 && v.kind == RIFLEMAN && !RAIDERS.contains(&v.id.index()))
            && let Some(c) = w.cell_at(v.pos)
            && w.visibility(P0, c) == Visibility::Visible
        {
            seen_at = Some(w.tick().0);
        }
    })
    .unwrap();
    let until = seen_at.map_or(s.ticks, |t| t - 1);
    println!("the hidden raider at {near:?} beside the base comes into view at {seen_at:?}");
    // The raid (first hit 4,022) is answered before it is seen.
    assert!(until > 4100, "{until}");
    let a = go(&s, until);
    let b = go(&with_extra(far), until);
    assert_eq!(a.world.len(), b.world.len());
    assert_ne!(a.world.checksum(), b.world.checksum());
    assert!(a.ai_commands.iter().any(|c| matches!(
        c.2,
        Command::Order {
            order: Order::AttackMove(_),
            ..
        }
    )));
    assert_eq!(a.ai_commands, b.ai_commands);
    assert_eq!(a.ais[0].digest(), b.ais[0].digest());
}

#[test]
fn the_finishing_attack_is_sent_on_the_real_world_and_the_simulation_takes_it() {
    // The AI's own army turn on the real world of `ai_vs_passive`: four riflemen of player 0 put idle beside the
    // enemy centre (which they see), an attack out on it. The turn sends one `Attack` on the
    // centre (slot 11) and the simulation takes it: the units' order is that `Attack` after the step.
    let mut s = load("ai_vs_passive");
    for cell in [(77, 9), (78, 9), (77, 10), (78, 10)] {
        s.commands.push(TimedCommand {
            tick: 0,
            player: 0,
            command: Action::SpawnUnit {
                kind: 12,
                owner: 0,
                cell,
            },
        });
    }
    let f = go(&s, 2);
    let mut world = f.world;
    let mut state = f.ais[0].state().clone();
    let ours: Vec<EntityId> = world
        .snapshot()
        .iter()
        .filter(|v| v.owner == P0 && v.kind == RIFLEMAN)
        .map(|v| v.id)
        .collect();
    assert_eq!(ours.len(), 4);
    let first = Observation::build(&world, P0, &state.known_structures);
    let gather = army::ensure_gather(&mut state, &first, &Lens::new(&world, P0)).unwrap();
    state.profile.scout = None;
    // North-east of the centre, away from the opponent's units and tower (which would draw them into a fight of
    // their own), 7 to 8 cells from the target cell on its south-west side: an arrive radius that covers that.
    state.profile.army.arrive_radius = 12;
    let goal = army::target(&state, &Lens::new(&world, P0), gather).unwrap();
    state.add_group(
        ours.clone(),
        goal,
        GroupStage::Advancing,
        world.tick(),
        0,
        None,
        None,
    );
    state.attack_started = world.tick();
    let mut obs = Observation::build(&world, P0, &state.known_structures);
    sf_ai::memory::update(&mut state.known_structures, &obs);
    obs.tick = world.tick();
    let mut out = Vec::new();
    army::turn(&mut state, &obs, &mut Lens::new(&world, P0), &mut out);
    // The enemy centre (its slot comes after the four spawned units).
    let centre = world
        .snapshot()
        .iter()
        .find(|v| v.owner == P1 && v.kind == CENTRE)
        .unwrap()
        .id;
    let sent = attacks(&out);
    assert_eq!(sent.len(), 1, "{out:?}");
    assert_eq!(sent[0].1, centre);
    for c in out {
        world.schedule(world.tick(), P0, c).unwrap();
    }
    let report = world.step();
    assert_eq!(report.commands_ignored, 0);
    for id in ours {
        assert_eq!(world.order_of(id), Some(Order::Attack(centre)), "{id:?}");
    }
}

#[test]
fn the_finishing_attack_never_goes_to_an_own_structure() {
    // An own structure in `visible` (the observation never has one; this pins the owner filter): nearer to the
    // target than the enemy's, it is still not chosen.
    let (world, s0, own) = at_the_target();
    let mut s = s0.clone();
    let mut obs = observation(&world, 3200, own);
    let mut mine = seen_structure(&world, 5, CENTRE, 70, 16);
    mine.owner = P0;
    obs.visible.push(mine);
    obs.visible.push(seen_structure(&world, 11, CENTRE, 72, 14));
    obs.visible.sort_by_key(|v| v.id);
    let out = army_turn(&world, &mut s, &obs);
    assert_eq!(
        attacks(&out),
        [(vec![20, 21, 22, 23], EntityId::new(11, 0))]
    );
}

#[test]
fn the_goal_list_with_the_floor_and_no_attack_over_thirty_thousand_ticks() {
    // Fix round 1 (item 4): `ai_vs_passive` with the attack switched off (no win), to see when the goal list
    // completes with the army floor and how the army grows. Printed and pinned.
    let s = with_profile(load("ai_vs_passive"), |p| {
        p.army.attack_value = 1_000_000;
        p.army.max_wait = 72_000;
    });
    let st = story(&s, 30_000);
    let builds: Vec<u64> = st
        .finished
        .ai_commands
        .iter()
        .filter(|c| matches!(c.2, Command::BuildStructure { .. }))
        .map(|c| c.0)
        .collect();
    let army = |f: &Finished| {
        f.world
            .snapshot()
            .iter()
            .filter(|v| v.owner == P0 && [12, 13, 19].contains(&v.kind.0))
            .count()
    };
    println!(
        "ai_vs_passive, no attack: {} builds, the last at {:?}; army at the gather cell {:?}; {} army units at 30,000",
        builds.len(),
        builds.last(),
        st.at_gather,
        army(&st.finished)
    );
    assert!(st.waves.is_empty());
    // WP9.1c-3 (was: the list does not complete in 30,000 ticks, 9 builds, the vehicle factory at 11,650): with the
    // vehicle factory second and the second refinery before the first tower the list completes at 22,730 (11 builds:
    // a third power plant for the radar included); the army floor is reached at 10,687 (pinned in
    // `the_vehicle_factory_comes_before_...`). The target of 20,000 is NOT met here (met in the game seat, 19,777): the
    // ore of the map is the limit (docs/rules/ai.md "The opening").
    // 14-11-2026 (WP9.2a-2): superseded, 20,297 since WP9.2a-2, see "The start-site advantage"; target now 20,500.
    assert_eq!(
        builds,
        [
            10, 250, 810, 2170, 2890, 14_370, 18_410, 19_170, 20_650, 21_570, 22_730
        ]
    );
    // The last structure of the list, the advanced defences (kind 7), is the final build.
    let last_kind = st
        .finished
        .ai_commands
        .iter()
        .filter_map(|c| match &c.2 {
            Command::BuildStructure { kind, .. } => Some(kind.0),
            _ => None,
        })
        .next_back();
    assert_eq!(last_kind, Some(7));
}

#[test]
fn the_vehicle_factory_comes_before_the_barracks_and_the_second_harvester_before_the_first_army_unit()
 {
    // WP9.1c-3: the opening of `ai_vs_passive` (shipped profile, frozen copy): the goal list's third build is the
    // vehicle factory (kind 4), the barracks (kind 3) comes after it, the bought harvester is queued as soon as
    // the factory stands, the second harvester exists well before tick 4,000, and the first unit of the mix
    // (rifleman, rocket trooper, light tank) is queued only after it.
    let s = load("ai_vs_passive");
    let mut second_harvester = None;
    let mut first_unit_exists = None;
    let mut floor_reached = None;
    let f = run_observed(
        &s,
        &RunOptions {
            ticks: 13_000,
            every: 100,
        },
        |w, ais| {
            let t = w.tick().0;
            let mine = |pick: fn(u16) -> bool| {
                w.snapshot()
                    .iter()
                    .filter(|v| v.owner == P0 && pick(v.kind.0))
                    .count()
            };
            if second_harvester.is_none() && mine(|k| k == HARVESTER.0) >= 2 {
                second_harvester = Some(t);
            }
            if first_unit_exists.is_none() && mine(|k| [12, 13, 19].contains(&k)) >= 1 {
                first_unit_exists = Some(t);
            }
            let value: u64 = w
                .snapshot()
                .iter()
                .filter(|v| v.owner == P0 && [12, 13, 19].contains(&v.kind.0))
                .map(|v| {
                    u64::from(
                        w.production_table()
                            .and_then(|t| t.get(v.kind))
                            .map_or(0, |u| u.cost),
                    )
                })
                .sum();
            if floor_reached.is_none()
                && value >= u64::from(ais[0].state().profile.army.army_floor_value)
            {
                floor_reached = Some(t);
            }
        },
    )
    .unwrap();
    let builds: Vec<(u64, u16)> = f
        .ai_commands
        .iter()
        .filter_map(|c| match &c.2 {
            Command::BuildStructure { kind, .. } => Some((c.0, kind.0)),
            _ => None,
        })
        .collect();
    // power plant, refinery, vehicle factory, (the second power plant before the barracks: `power_first`), barracks.
    assert_eq!(
        builds[..5],
        [(10, 1), (250, 2), (810, 4), (2170, 1), (2890, 3)]
    );
    let queued = |pick: fn(u16) -> bool| {
        f.ai_commands.iter().find_map(|c| match &c.2 {
            Command::QueueUnit { kind, .. } if pick(kind.0) => Some(c.0),
            _ => None,
        })
    };
    let harvester_queued = queued(|k| k == HARVESTER.0).expect("a harvester is bought");
    let unit_queued = queued(|k| [12, 13, 19].contains(&k)).expect("the mix starts");
    assert_eq!(harvester_queued, 1480);
    assert_eq!(second_harvester, Some(1880));
    assert_eq!(unit_queued, 4460);
    assert_eq!(first_unit_exists, Some(4621));
    // The army floor (2,000 credits of units) is reached at 10,687, well before `max_wait` (12,000) ends it.
    assert_eq!(floor_reached, Some(10_687));
    let profile = load("ai_vs_passive").ais[0].state().profile.clone();
    assert!(floor_reached.unwrap() + 1000 < u64::from(profile.army.max_wait));
    // The acceptance bound of the WP (two harvesters by 4,000) and the order the opening exists for.
    assert!(second_harvester.unwrap() < 4000);
    assert!(harvester_queued < unit_queued);
}

// ---- fix round 2 (review of PR #129, decision orchestrator 12-11-2026) -----------------------------------------

#[test]
fn attackers_at_the_target_that_see_nothing_close_in_once_and_the_attack_still_ends() {
    // The reviewer's probe: four attackers idle within `arrive_radius` of the target, no enemy structure in
    // sight (`obs.visible` empty), an army turn every 40 ticks. Before fix round 2 the attack stayed out forever
    // (`attack_started` renewed on `near` alone, no order for the arrived attackers).
    let (world, s0, own) = at_the_target();
    let centre = middle(Cell::new(74, 16));
    let mut s = s0.clone();
    let mut closing = Vec::new();
    let mut ended_at = None;
    for t in (3200..=20_000).step_by(40) {
        let out = army_turn(&world, &mut s, &observation(&world, t, own.clone()));
        let to_centre: Vec<_> = attack_moves(&out)
            .into_iter()
            .filter(|(_, p)| *p == centre)
            .collect();
        if !to_centre.is_empty() {
            assert_eq!(to_centre, [(vec![20, 21, 22, 23], centre)], "tick {t}");
            println!("closing at {t}, attack {}", s.attacks_launched);
            closing.push(s.attacks_launched);
        }
        if ended_at.is_none() && s.attackers().is_empty() {
            ended_at = Some(t);
        }
    }
    // Nothing came into sight: the first attack ends when `attack_patience` (2,400) after its launch (3,000) runs
    // out. The units never move here, so the AI launches again from where they stand and each attack ends the
    // same way (seven in all up to 20,000).
    assert_eq!(ended_at, Some(5400));
    // The closing `AttackMove` onto the middle of the remembered centre: once per attack, not every turn.
    assert_eq!(closing, (1..=s.attacks_launched).collect::<Vec<_>>());
    assert_eq!(s.attacks_launched, 7);
}
