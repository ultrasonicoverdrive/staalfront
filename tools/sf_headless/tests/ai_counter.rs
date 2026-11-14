//! WP9.2b-1, the counter mix (`docs/rules/ai.md`, "Counter mix (WP9.2b-1)"): the tally of the armed enemy units the
//! AI sees, the share per armour type against the shipped tables, the weights `pick_kind` reads, the old order with the
//! rule off, the fog twin, the digest, and the duel fixture. Every test calls the code the game runs
//! (`counter::tally`, `counter::weights`, `Lens::counter_score`, `production::pick_kind`, `Ai::think_with`); the
//! observations are the world's own with `Seen` entries cloned from a real one (no struct literal of a shared type).

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::scenario_path;
use sf_ai::counter::{self, WEIGHT_CLAMP};
use sf_ai::production;
use sf_ai::{Ai, AiState, Lens, Observation, Seen};
use sf_data::ai_profile::AiProfile;
use sf_data::units::{Faction, UnitDefs};
use sf_headless::run::{Finished, RunOptions, run_full};
use sf_headless::scenario::{Action, Scenario, TimedCommand};
use sf_headless::tournament::{self, Prepared};
use sf_sim::{Footprint, Kind, PlayerId, Tick, World};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const RIFLEMAN: Kind = Kind(12);
const ROCKET: Kind = Kind(13);
const SCOUT: Kind = Kind(15);
const SNIPER: Kind = Kind(16);
const HARVESTER: Kind = Kind(11);
const TANK: Kind = Kind(19);
const HEAVY: Kind = Kind(20);
const TOWER: Kind = Kind(6);

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

/// The shipped world of `m1_duel` (the shipped tables) and the AI of seat 0, its profile with the counter mix on.
fn shipped(permille: u16, intel: u32) -> (Box<World>, AiState) {
    let setup = tournament::load_setup(&assets(), &assets().join("matches/m1_duel.ron")).unwrap();
    let Prepared { world, ais, .. } =
        tournament::prepare(&setup, [Faction::Meridian, Faction::Veyran], 458_758, 1).unwrap();
    let mut state = ais[0].state().clone();
    // The counter tests are written for the mix of WP9.2a-2 (4 : 2 : 3); the shipped mix has been 4 : 4 : 3 since
    // WP9.2b-1 (the static row that beat the counter mix), so the test puts the old one back.
    state.profile.unit_mix = vec![(RIFLEMAN, 4), (ROCKET, 2), (TANK, 3)];
    state.profile.army.counter_permille = permille;
    state.profile.army.intel_ticks = intel;
    (world, state)
}

/// A real observation and a real `Seen` in it (an engineer in the scout's sight in `ai_fog`), to clone.
struct Template {
    obs: Observation,
    seen: Seen,
}

fn template() -> Template {
    let mut s = Scenario::load(&scenario_path("ai_fog.ron")).unwrap();
    s.commands
        .retain(|c| !matches!(c.command, Action::SpawnUnit { .. }));
    s.commands.push(TimedCommand {
        tick: 0,
        player: 1,
        command: Action::SpawnUnit {
            kind: 14,
            owner: 1,
            cell: (34, 55),
        },
    });
    let f: Finished = run_full(
        &s,
        &RunOptions {
            ticks: 400,
            every: 100,
        },
    )
    .unwrap();
    let obs = Observation::build(&f.world, P0, &BTreeMap::new());
    let seen = obs
        .visible
        .iter()
        .find(|e| e.kind == Kind(14))
        .cloned()
        .expect("the engineer next to the scout is seen");
    Template { obs, seen }
}

/// `n` units of `kind` of player 1, armed or not, from ids `first` on.
fn units(t: &Template, kind: Kind, armed: bool, first: u32, n: u32) -> Vec<Seen> {
    (0..n)
        .map(|i| {
            let mut s = t.seen.clone();
            s.id = sf_sim::EntityId::new(first + i, 0);
            s.kind = kind;
            s.owner = P1;
            s.footprint = None;
            s.armed = armed;
            s
        })
        .collect()
}

/// The template's observation at `tick` with nothing of the player's own and `visible` in sight.
fn obs_with(t: &Template, tick: u64, visible: Vec<Seen>) -> Observation {
    let mut o = t.obs.clone();
    o.tick = Tick(tick);
    o.own.clear();
    o.gone.clear();
    o.visible = visible;
    o
}

fn mix_of(state: &AiState) -> Vec<(u16, u16)> {
    state
        .profile
        .unit_mix
        .iter()
        .map(|&(k, w)| (k.0, w))
        .collect()
}

// ---- the tally -------------------------------------------------------------------------------------------

#[test]
fn the_tally_keeps_the_peak_and_forgets_it_intel_ticks_after_its_tick_and_not_one_tick_earlier() {
    let t = template();
    let (_, mut state) = shipped(1000, 3000);
    // Five riflemen seen at tick 1,000.
    counter::tally(
        &mut state,
        &obs_with(&t, 1000, units(&t, RIFLEMAN, true, 100, 5)),
    );
    assert_eq!(state.intel.get(&RIFLEMAN), Some(&(5, Tick(1000))));
    // Fewer later: the peak and its tick stay.
    counter::tally(
        &mut state,
        &obs_with(&t, 1010, units(&t, RIFLEMAN, true, 100, 3)),
    );
    assert_eq!(state.intel.get(&RIFLEMAN), Some(&(5, Tick(1000))));
    // The same count again: the tick is fresh (a peak seen again must not age out while it stands there).
    counter::tally(
        &mut state,
        &obs_with(&t, 1020, units(&t, RIFLEMAN, true, 100, 5)),
    );
    assert_eq!(state.intel.get(&RIFLEMAN), Some(&(5, Tick(1020))));
    // More: a new peak.
    counter::tally(
        &mut state,
        &obs_with(&t, 1030, units(&t, RIFLEMAN, true, 100, 7)),
    );
    assert_eq!(state.intel.get(&RIFLEMAN), Some(&(7, Tick(1030))));
    // Nothing visible any more: still known at tick + intel_ticks - 1, gone at tick + intel_ticks.
    counter::tally(&mut state, &obs_with(&t, 1030 + 2999, Vec::new()));
    assert_eq!(state.intel.get(&RIFLEMAN), Some(&(7, Tick(1030))));
    counter::tally(&mut state, &obs_with(&t, 1030 + 3000, Vec::new()));
    assert!(state.intel.is_empty(), "{:?}", state.intel);
    // A kind whose entry has just expired and that is seen again starts afresh.
    counter::tally(
        &mut state,
        &obs_with(&t, 9000, units(&t, RIFLEMAN, true, 100, 2)),
    );
    counter::tally(
        &mut state,
        &obs_with(&t, 9000 + 3000, units(&t, RIFLEMAN, true, 100, 1)),
    );
    assert_eq!(state.intel.get(&RIFLEMAN), Some(&(1, Tick(12000))));
}

#[test]
fn the_tally_follows_intel_ticks_of_the_profile() {
    let t = template();
    for keep in [100u32, 777] {
        let (_, mut state) = shipped(1000, keep);
        counter::tally(
            &mut state,
            &obs_with(&t, 50, units(&t, ROCKET, true, 100, 2)),
        );
        counter::tally(
            &mut state,
            &obs_with(&t, 50 + u64::from(keep) - 1, Vec::new()),
        );
        assert!(state.intel.contains_key(&ROCKET), "keep {keep}");
        counter::tally(&mut state, &obs_with(&t, 50 + u64::from(keep), Vec::new()));
        assert!(state.intel.is_empty(), "keep {keep}");
    }
}

#[test]
fn the_tally_counts_armed_units_of_others_only() {
    let t = template();
    let (_, mut state) = shipped(1000, 3000);
    let mut seen = Vec::new();
    // Unarmed units (a harvester, an engineer) and the scout kind (armed with a pistol, but no army): never.
    seen.extend(units(&t, SCOUT, true, 100, 4));
    seen.extend(units(&t, Kind(14), false, 105, 2));
    seen.extend(units(&t, Kind(11), false, 110, 4));
    // Structures, armed or not, remembered or seen: never (a footprint makes it one).
    let mut tower = units(&t, TOWER, true, 120, 3);
    for s in &mut tower {
        s.footprint = Some(Footprint {
            width: 2,
            height: 2,
        });
    }
    seen.extend(tower);
    // One's own units in the list (it never holds them, but a slip must not count): never.
    let mut own = units(&t, RIFLEMAN, true, 130, 6);
    for s in &mut own {
        s.owner = P0;
    }
    seen.extend(own);
    // Armed units of another player, and of nobody: counted.
    seen.extend(units(&t, TANK, true, 140, 2));
    let mut neutral = units(&t, TANK, true, 150, 1);
    neutral[0].owner = PlayerId(7);
    seen.extend(neutral);
    counter::tally(&mut state, &obs_with(&t, 500, seen));
    let got: Vec<(u16, (u32, Tick))> = state.intel.iter().map(|(k, v)| (k.0, *v)).collect();
    assert_eq!(got, [(TANK.0, (3, Tick(500)))]);
}

#[test]
fn with_the_rule_off_nothing_is_tallied_and_the_digest_does_not_see_the_army() {
    let t = template();
    let (_, mut state) = shipped(0, 3000);
    let before = state.digest();
    counter::tally(
        &mut state,
        &obs_with(&t, 500, units(&t, RIFLEMAN, true, 100, 9)),
    );
    assert!(state.intel.is_empty());
    assert_eq!(state.digest(), before);
    // With the rule on the tally is in the digest.
    state.profile.army.counter_permille = 1;
    let on = state.digest();
    counter::tally(
        &mut state,
        &obs_with(&t, 500, units(&t, RIFLEMAN, true, 100, 9)),
    );
    assert_ne!(state.digest(), on);
}

// ---- the share per armour type against the shipped tables ---------------------------------------------------

#[test]
fn the_shipped_roles_have_the_kinds_the_tests_assume() {
    let defs = UnitDefs::embedded();
    for (role, kind) in [
        ("rifleman", RIFLEMAN),
        ("rocket_trooper", ROCKET),
        ("scout", SCOUT),
        ("light_tank", TANK),
        ("heavy_tank", HEAVY),
        ("defence_tower", TOWER),
    ] {
        assert_eq!(defs.kind_of(role), Some(kind.0), "{role}");
    }
}

#[test]
fn the_lens_scores_every_role_against_every_armour_type_as_pinned() {
    let (world, _) = shipped(1000, 3000);
    let lens = Lens::new(&world, P0);
    // Raw `weapon_score` (damage x burst x percent x 1000 / reload ticks) of the best weapon of the role against the
    // armour of the enemy kind, from the shipped tables: infantry (the rifleman and the rocket trooper wear it), light
    // (light tank), heavy (heavy tank), structure (defence tower). Pinned as literals.
    let got: Vec<[u64; 4]> = [RIFLEMAN, ROCKET, TANK]
        .map(|role| [ROCKET, TANK, HEAVY, TOWER].map(|e| lens.counter_score(role, e)))
        .to_vec();
    println!("{got:?}");
    assert_eq!(got, SCORES);
    // A unit with no weapon scores 0 (the harvester).
    assert_eq!(lens.counter_score(HARVESTER, TANK), 0);
    // The lens asks the table, not the list: the rifleman against itself is the infantry cell.
    assert_eq!(
        lens.counter_score(RIFLEMAN, RIFLEMAN),
        lens.counter_score(RIFLEMAN, ROCKET)
    );
}

/// `[against infantry (rocket trooper), light (light tank), heavy (heavy tank), structure (defence tower)]` per role.
const SCORES: [[u64; 4]; 3] = [
    [30_000, 12_000, 3_000, 6_000],
    [36_000, 117_000, 90_000, 63_000],
    [35_000, 87_500, 74_375, 52_500],
];

#[test]
fn per_credit_the_rifleman_counters_infantry_and_the_rocket_trooper_light_heavy_and_structure() {
    let (world, state) = shipped(1000, 3000);
    let lens = Lens::new(&world, P0);
    let mix = &state.profile.unit_mix;
    assert_eq!(mix_of(&state), [(12, 4), (13, 2), (19, 3)]);
    let best = |enemy| counter::best_role(&lens, mix, enemy).map(|i| mix[i].0.0);
    assert_eq!(best(RIFLEMAN), Some(12));
    assert_eq!(best(ROCKET), Some(12));
    assert_eq!(best(TANK), Some(13));
    assert_eq!(best(HEAVY), Some(13));
    assert_eq!(best(TOWER), Some(13));
    // Nobody counters a kind with no armour entry and no weapon matter: the scout is infantry-like, so it is the
    // rifleman too (the question is about the armour, not about whether the enemy is armed; the tally filters).
    // The light tank is best against nothing.
    for e in [RIFLEMAN, ROCKET, TANK, HEAVY, TOWER] {
        assert_ne!(best(e), Some(19), "{e:?}");
    }
}

#[test]
fn raw_scores_would_pick_the_rocket_trooper_against_everything_per_credit_does_not() {
    // The reason the comparison is per credit: the cross-multiplied choice differs from the raw one against infantry.
    let (world, state) = shipped(1000, 3000);
    let lens = Lens::new(&world, P0);
    let mix = &state.profile.unit_mix;
    let raw = |role: Kind| lens.counter_score(role, ROCKET);
    assert!(raw(ROCKET) > raw(RIFLEMAN), "the rocket hits harder");
    let cost = |k: Kind| u64::from(lens.unit_cost(k).unwrap());
    assert!(raw(RIFLEMAN) * cost(ROCKET) > raw(ROCKET) * cost(RIFLEMAN));
    assert_eq!(counter::best_role(&lens, mix, ROCKET), Some(0));
}

#[test]
fn a_tie_goes_to_the_earlier_entry_and_a_kind_nobody_hurts_to_nobody() {
    let (world, _) = shipped(1000, 3000);
    let lens = Lens::new(&world, P0);
    // The same role twice: the earlier index wins every enemy.
    let twice = [(RIFLEMAN, 1u16), (RIFLEMAN, 1u16)];
    for e in [RIFLEMAN, TANK, TOWER] {
        assert_eq!(counter::best_role(&lens, &twice, e), Some(0));
    }
    // A mix of unarmed roles: nobody wins.
    assert_eq!(counter::best_role(&lens, &[(HARVESTER, 1)], TANK), None);
    // An empty mix: nobody.
    assert_eq!(counter::best_role(&lens, &[], TANK), None);
}

#[test]
fn a_tallied_kind_nobody_in_the_mix_counters_still_counts_in_the_total() {
    // The sniper (kind 16) hurts infantry and scores 0 against the light tank (shipped tables), so with the sniper as
    // the only role, a seen light tank is valued but nobody's: it stays in the total and the shares add up to less
    // than 1,000 (riflemen 10 x 100 = 1,000 of 1,000 + 700 credits = 588 thousandths).
    let (world, mut state) = shipped(1000, 3000);
    let lens = Lens::new(&world, P0);
    assert!(lens.counter_score(SNIPER, RIFLEMAN) > 0);
    assert_eq!(lens.counter_score(SNIPER, TANK), 0);
    state.profile.unit_mix = vec![(SNIPER, 1)];
    with_intel(&mut state, &[(RIFLEMAN, 10), (TANK, 1)]);
    let share = counter::shares(&state, &lens);
    assert_eq!(share, [588]);
    assert!(share.iter().sum::<u32>() < 1000);
    // Only the countered kind: the whole share.
    with_intel(&mut state, &[(RIFLEMAN, 10)]);
    assert_eq!(counter::shares(&state, &lens), [1000]);
}

// ---- the weights -------------------------------------------------------------------------------------------

fn weights(world: &World, state: &AiState) -> Vec<u32> {
    let lens = Lens::new(world, P0);
    counter::weights(state, &lens)
        .into_iter()
        .map(|(_, w)| w)
        .collect()
}

fn with_intel(state: &mut AiState, list: &[(Kind, u32)]) {
    state.intel.clear();
    for &(k, n) in list {
        state.intel.insert(k, (n, Tick(100)));
    }
}

#[test]
fn the_weights_move_toward_the_counter_and_stop_at_twice_the_base() {
    let (world, mut state) = shipped(1000, 3000);
    // Base 4, 2, 3 (rifleman, rocket trooper, light tank); thousandths.
    assert_eq!(weights(&world, &state), [4000, 2000, 3000], "nothing seen");
    // Only riflemen seen: the rifleman counters them, share 1000, so at 1000 permille its weight doubles.
    with_intel(&mut state, &[(RIFLEMAN, 10)]);
    assert_eq!(weights(&world, &state), [8000, 2000, 3000]);
    // At 500 permille a half more.
    state.profile.army.counter_permille = 500;
    assert_eq!(weights(&world, &state), [6000, 2000, 3000]);
    // At 2000 the clamp holds it at twice the base, as at 1000.
    state.profile.army.counter_permille = 2000;
    assert_eq!(weights(&world, &state), [8000, 2000, 3000]);
    // Only tanks seen: the rocket trooper is the counter.
    with_intel(&mut state, &[(TANK, 4)]);
    state.profile.army.counter_permille = 1000;
    assert_eq!(weights(&world, &state), [4000, 4000, 3000]);
    // Half the value infantry, half tanks (a rifleman costs 100, a light tank 700: 7 riflemen = 1 tank).
    with_intel(&mut state, &[(RIFLEMAN, 7), (TANK, 1)]);
    assert_eq!(weights(&world, &state), [4000 + 2000, 2000 + 1000, 3000]);
    // A unit nobody hurts changes no weight (the scout is unarmed and never tallied; a made-up kind 99 has no price).
    with_intel(&mut state, &[(Kind(99), 50)]);
    assert_eq!(weights(&world, &state), [4000, 2000, 3000]);
}

#[test]
fn the_weight_formula_is_pinned_over_its_corners() {
    // base * (1000 + permille * share / 1000), at most 2000 * base, no lower clamp.
    assert_eq!(counter::weight(4, 0, 2000), 4000);
    assert_eq!(counter::weight(4, 1000, 0), 4000);
    assert_eq!(counter::weight(4, 500, 1000), 4 * 1500);
    assert_eq!(counter::weight(4, 1000, 1000), 8000);
    assert_eq!(counter::weight(4, 1000, 2000), 8000);
    assert_eq!(counter::weight(4, 750, 2000), 8000);
    assert_eq!(counter::weight(4, 333, 1000), 4 * 1333);
    // A small base: the thousandths keep what a u16 would round away (permille 1, share 1 is under one thousandth).
    assert_eq!(counter::weight(1, 999, 1), 1000);
    assert_eq!(counter::weight(1, 1000, 3), 1003);
    assert_eq!(WEIGHT_CLAMP, 2000);
    // The largest base the profile allows does not overflow.
    assert_eq!(
        counter::weight(u16::MAX, 1000, 2000),
        u32::from(u16::MAX) * 2000
    );
}

#[test]
fn with_the_rule_off_the_pick_order_is_the_old_one_over_a_sweep_of_counts_and_sightings() {
    let (world, mut state) = shipped(0, 3000);
    let lens = Lens::new(&world, P0);
    let base: Vec<(Kind, u32)> = state
        .profile
        .unit_mix
        .iter()
        .map(|&(k, w)| (k, u32::from(w)))
        .collect();
    // Seen armies of every shape with the rule off: the intel is empty (nothing is tallied), so the weights are the
    // base ones scaled by 1,000 and the order is the old one.
    let t = template();
    for seen in [
        Vec::new(),
        units(&t, RIFLEMAN, true, 100, 20),
        units(&t, TANK, true, 100, 5),
    ] {
        counter::tally(&mut state, &obs_with(&t, 100, seen));
        let w = counter::weights(&state, &lens);
        assert!(
            w.iter()
                .zip(&state.profile.unit_mix)
                .all(|(a, b)| a.1 == u32::from(b.1) * 1000)
        );
        for a in 0..9u32 {
            for b in 0..9u32 {
                for c in 0..9u32 {
                    let have = [a, b, c];
                    assert_eq!(
                        production::pick_kind(&w, &have, |_| true),
                        production::pick_kind(&base, &have, |_| true),
                        "{have:?}"
                    );
                    // And with one kind unusable, whichever.
                    for off in 0..3usize {
                        let usable = |k: Kind| k != base[off].0;
                        assert_eq!(
                            production::pick_kind(&w, &have, usable),
                            production::pick_kind(&base, &have, usable)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn with_the_rule_on_the_pick_leans_toward_the_counter() {
    let (world, mut state) = shipped(1000, 3000);
    with_intel(&mut state, &[(TANK, 6)]);
    let w = counter::weights(&state, &Lens::new(&world, P0));
    // Make 300 units by the picks, each made one counted at once: the shares follow the weights 4 : 4 : 3 against the
    // base 4 : 2 : 3.
    let mut have = [0u32; 3];
    for _ in 0..300 {
        let i = production::pick_kind(&w, &have, |_| true).unwrap();
        have[i] += 1;
    }
    // Integer shares in thousandths: rocket trooper 4/11, rifleman 4/11, tank 3/11 (+- one unit).
    let per = |n: u32| n * 1000 / 300;
    assert_eq!([per(have[0]), per(have[1]), per(have[2])], [363, 363, 273]);
    // The control, the same picks with the base weights: 4/9, 2/9, 3/9.
    let base: Vec<(Kind, u32)> = state
        .profile
        .unit_mix
        .iter()
        .map(|&(k, w)| (k, u32::from(w)))
        .collect();
    let mut have = [0u32; 3];
    for _ in 0..300 {
        let i = production::pick_kind(&base, &have, |_| true).unwrap();
        have[i] += 1;
    }
    assert_eq!([per(have[0]), per(have[1]), per(have[2])], [443, 223, 333]);
}

#[test]
fn composition_collapse_the_worst_tally_keeps_the_tank_at_its_clamp_floor() {
    // The most the other two roles can take from the tank: both at twice their base, 8 : 4 : 3, so the tank share is
    // 3 / 15 = 20 %. Swept over every composition of the four tallied kinds at 0, 1, 10 and 100 units and over
    // every strength of the rule from 0 to 2000: the tank weight is never lower than its base and its share of the
    // total weight never under 3/15.
    let (world, mut state) = shipped(2000, 3000);
    let counts = [0u32, 1, 10, 100];
    let mut worst = (u32::MAX, 0u32);
    for permille in [0u16, 1, 250, 500, 1000, 1500, 2000] {
        state.profile.army.counter_permille = permille;
        for &a in &counts {
            for &b in &counts {
                for &c in &counts {
                    for &d in &counts {
                        with_intel(
                            &mut state,
                            &[(RIFLEMAN, a), (ROCKET, b), (TANK, c), (HEAVY, d)],
                        );
                        let w = weights(&world, &state);
                        let total: u32 = w.iter().sum();
                        assert!(w[2] >= 3000, "the tank never goes below its base");
                        assert!(w[2] * 15 >= total * 3, "{w:?} at {permille}");
                        assert!(w[0] <= 8000 && w[1] <= 4000 && w[2] <= 6000);
                        if w[2] * 1000 / total < worst.0 {
                            worst = (w[2] * 1000 / total, u32::from(permille));
                        }
                    }
                }
            }
        }
    }
    println!(
        "lowest tank weight share {} thousandths at permille {}",
        worst.0, worst.1
    );
    // The floor is reached: 3000 of 15000 when infantry and tanks are both countered by the infantry roles.
    // (Heavy tanks are countered by the rocket trooper, light tanks too, infantry by the rifleman.)
    with_intel(&mut state, &[(RIFLEMAN, 100), (HEAVY, 1)]);
    state.profile.army.counter_permille = 2000;
    let w = weights(&world, &state);
    assert!(w[2] * 1000 / w.iter().sum::<u32>() >= 200, "{w:?}");
    // By count over 1,500 picks of the worst case: the tank share is at least 19 %.
    with_intel(&mut state, &[(RIFLEMAN, 70), (TANK, 1)]);
    let w = counter::weights(&state, &Lens::new(&world, P0));
    let mut have = [0u32; 3];
    for _ in 0..1500 {
        let i = production::pick_kind(&w, &have, |_| true).unwrap();
        have[i] += 1;
    }
    assert!(have[2] * 100 >= 1500 * 19, "{have:?}");
}

// ---- the profile and the digest ------------------------------------------------------------------------------------

#[test]
fn the_shipped_profile_has_the_counter_mix_as_measured() {
    let a = AiProfile::embedded().army;
    // Set from the 96-game duels of docs/rules/ai.md, "Counter mix (WP9.2b-1)".
    assert_eq!(
        (a.counter_permille, a.intel_ticks),
        (SHIPPED_PERMILLE, 3000)
    );
    assert_eq!(a.counter_on(), SHIPPED_PERMILLE != 0);
}

/// The shipped `counter_permille`.
const SHIPPED_PERMILLE: u16 = 0;

#[test]
fn the_numbers_of_the_counter_mix_are_in_the_digest_only_when_it_is_on() {
    let (_, base) = shipped(0, 3000);
    let off = base.digest();
    // Off: `intel_ticks` is not read and not hashed.
    let mut s = base.clone();
    s.profile.army.intel_ticks = 9000;
    assert_eq!(s.digest(), off);
    assert_eq!(s.profile.digest(), base.profile.digest());
    // On: both numbers are hashed, and so is the tally.
    let mut on = base.clone();
    on.profile.army.counter_permille = 1000;
    let d = on.digest();
    assert_ne!(d, off);
    let mut s = on.clone();
    s.profile.army.counter_permille = 1001;
    assert_ne!(s.digest(), d);
    let mut s = on.clone();
    s.profile.army.intel_ticks = 3001;
    assert_ne!(s.digest(), d);
    let mut s = on.clone();
    s.intel.insert(RIFLEMAN, (1, Tick(5)));
    let one = s.digest();
    assert_ne!(one, d);
    for change in [
        |s: &mut AiState| {
            s.intel.insert(RIFLEMAN, (2, Tick(5)));
        },
        |s: &mut AiState| {
            s.intel.insert(RIFLEMAN, (1, Tick(6)));
        },
        |s: &mut AiState| {
            s.intel.insert(ROCKET, (1, Tick(5)));
        },
        |s: &mut AiState| {
            s.intel.remove(&RIFLEMAN);
        },
    ] {
        let mut t = s.clone();
        change(&mut t);
        assert_ne!(t.digest(), one);
    }
}

// ---- the fog twin ------------------------------------------------------------------------------------------

#[test]
fn an_invisible_army_changes_no_command_and_no_digest_with_the_counter_mix_on() {
    // The twin of `ai_fog`'s test, with the rule on in both: the army in the far corner is out of sight.
    let on = |mut s: Scenario| {
        let old = s.ais[0].state().clone();
        let mut profile = old.profile.clone();
        profile.army.counter_permille = 1000;
        s.ais[0] = Ai::new(old.player, profile, 0xA1_0001, old.start_sites);
        s
    };
    let with = Scenario::load(&scenario_path("ai_fog.ron")).unwrap();
    let mut without = with.clone();
    without
        .commands
        .retain(|c| !matches!(c.command, Action::SpawnUnit { .. }));
    let go = |s: Scenario| {
        run_full(
            &s,
            &RunOptions {
                ticks: 800,
                every: 100,
            },
        )
        .unwrap()
    };
    let (a, b) = (go(on(with.clone())), go(on(without)));
    assert_ne!(a.world.checksum(), b.world.checksum(), "the worlds differ");
    assert!(!a.ai_commands.is_empty());
    assert_eq!(a.ai_commands, b.ai_commands);
    assert_eq!(a.ais[0].digest(), b.ais[0].digest());
    assert!(
        a.ais[0].state().intel.is_empty(),
        "{:?}",
        a.ais[0].state().intel
    );
    // The positive control: the same army where the scout sees it is in the tally and moves the digest.
    let mut seen = with;
    seen.commands
        .retain(|c| !matches!(c.command, Action::SpawnUnit { .. }));
    seen.commands.push(TimedCommand {
        tick: 0,
        player: 1,
        command: Action::SpawnUnit {
            kind: 12,
            owner: 1,
            cell: (34, 55),
        },
    });
    let c = go(on(seen));
    assert!(c.ais[0].state().intel.contains_key(&RIFLEMAN));
    assert_ne!(c.ais[0].digest(), b.ais[0].digest());
}

// ---- the duel fixture ----------------------------------------------------------------------------------------

#[test]
fn the_frozen_duel_opponent_is_the_shipped_profile_with_the_counter_mix_off() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/normal_9_2a2.ron");
    let old = AiProfile::from_ron(&std::fs::read_to_string(fixture).unwrap()).unwrap();
    let mut new = AiProfile::embedded();
    new.name = "normal_9_2a2".to_string();
    new.army.counter_permille = 0;
    new.army.intel_ticks = old.army.intel_ticks;
    // The shipped mix has had the rocket trooper at 4 since WP9.2b-1; the frozen opponent keeps the 2 of WP9.2a-2.
    for f in &mut new.factions {
        for (role, w) in &mut f.unit_mix {
            if role == "rocket_trooper" {
                assert_eq!(*w, 4);
                *w = 2;
            }
        }
    }
    assert_eq!(old, new);
    assert!(!old.army.counter_on());
    assert_eq!(old.army.intel_ticks, 3000);
}

// ---- the wiring: the production turn reads the weights ------------------------------------------------------

/// The kinds the production turn queues, in order, for the state `st` on the observation `obs` of `world`.
fn queued_kinds(world: &World, obs: &Observation, st: &AiState) -> Vec<u16> {
    let mut st = st.clone();
    let mut lens = Lens::new(world, P0);
    let mut out = Vec::new();
    production::turn(&mut st, obs, &mut lens, &mut out);
    out.iter()
        .filter_map(|c| match c {
            sf_sim::Command::QueueUnit { kind, .. } => Some(kind.0),
            _ => None,
        })
        .collect()
}

#[test]
fn the_production_turn_queues_what_the_weights_say_and_the_old_mix_with_the_rule_off() {
    let s = Scenario::load(&scenario_path("ai_vs_passive.ron")).unwrap();
    let f = run_full(
        &s,
        &RunOptions {
            ticks: 9000,
            every: 100,
        },
    )
    .unwrap();
    let world = &f.world;
    let base = f.ais[0].state().clone();
    let mut obs = Observation::build(world, P0, &base.known_structures);
    obs.credits = 100_000;
    // The control: the rule off.
    let mut off = base.clone();
    off.profile.army.counter_permille = 0;
    let old = queued_kinds(world, &obs, &off);
    assert!(!old.is_empty(), "the turn queues units");
    // With the rule on but nothing seen: the same orders.
    let mut on = base.clone();
    on.profile.army.counter_permille = 2000;
    assert_eq!(queued_kinds(world, &obs, &on), old);
    // With armies seen the first choice follows `pick_kind` on the weights (the same call the turn makes), and for some
    // army it is not the old one.
    let mut differs = false;
    let lens = Lens::new(world, P0);
    for army in [
        vec![(RIFLEMAN, 60)],
        vec![(TANK, 30)],
        vec![(HEAVY, 30)],
        vec![(RIFLEMAN, 20), (TANK, 20)],
    ] {
        on.intel.clear();
        for &(k, n) in &army {
            on.intel.insert(k, (n, Tick(8900)));
        }
        let got = queued_kinds(world, &obs, &on);
        let w = counter::weights(&on, &lens);
        let have: Vec<u32> = w.iter().map(|&(k, _)| production::have(&obs, k)).collect();
        let first = production::pick_kind(&w, &have, |_| true).map(|i| w[i].0.0);
        assert_eq!(got.first().copied(), first, "{army:?}");
        differs |= got != old;
    }
    assert!(differs, "no seen army changed what the turn queues");
}
