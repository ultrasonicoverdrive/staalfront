//! WP13.1d: saving and restoring the computer player's state (`docs/rules/save.md`, "The AI").
//!
//! * Round trip: in every AI scenario of CI, every 97 ticks, each AI's state is saved and restored onto a fresh
//!   AI; the restored AI equals the live one (`PartialEq`) and saves to the same bytes. The claimed states (a
//!   wave forming, a build or unit order in flight, an attack under way, a hit being answered, scouting
//!   memory) are counted and must all have been among the saved states.
//! * Continue-equals: the same scenarios run once uninterrupted and once with every AI replaced by its restored
//!   copy every 97 ticks (and, for `ai_raid`, at every tick, and once only at single ticks); the AI digests
//!   after every tick, the AI commands and the final world checksum are those of the uninterrupted run. The
//!   world itself is not saved here (WP13.1c is not done): it runs on untouched.
//! * Every restore check refuses its corrupt value with its own message, and the value at the bound passes.

mod common;

use common::scenario_path;
use sf_ai::save::SaveBounds;
use sf_ai::state::{GroupStage, Posture};
use sf_ai::{Ai, AiState};
use sf_headless::run::{Finished, RunOptions, run_hooked, run_observed};
use sf_headless::scenario::Scenario;
use sf_math::bytes::{ByteReader, ByteWriter};
use sf_sim::save::SaveError;
use sf_sim::{Cell, EntityId, Kind, PlayerId, Tick, World};

/// The AI scenarios of the CI headless job.
const SCENARIOS: [&str; 15] = [
    "ai_fog",
    "ai_base",
    "ai_noore",
    "ai_boxed",
    "ai_economy",
    "ai_rebuild",
    "ai_deadlock",
    "ai_scout",
    "ai_vs_passive",
    "ai_raid",
    "ai_raid_guard",
    "ai_bounce",
    "ai_harvest_raid",
    "ai_vs_passive_easy",
    "ai_vs_passive_hard",
];

/// The spacing of the save points, a prime.
const K: u64 = 97;

fn load(name: &str) -> Scenario {
    Scenario::load(&scenario_path(&format!("{name}.ron"))).unwrap()
}

fn options(s: &Scenario) -> RunOptions {
    RunOptions::for_scenario(s)
}

fn save_state(s: &AiState) -> Vec<u8> {
    let mut w = ByteWriter::new();
    s.save(&mut w).unwrap();
    w.finish()
}

fn fresh_of(s: &AiState) -> AiState {
    AiState::new(s.player, s.profile.clone(), 0, s.start_sites.clone())
}

fn restore_state(fresh: AiState, bytes: &[u8], b: &SaveBounds) -> Result<AiState, SaveError> {
    let mut r = ByteReader::new(bytes);
    let s = AiState::restore(fresh, &mut r, b)?;
    r.end()?;
    Ok(s)
}

/// Which of the claimed states a state shows.
#[derive(Default, Debug)]
struct Seen {
    wave_forming: u32,
    order_in_flight: u32,
    attack_under_way: u32,
    defending: u32,
    scouting_memory: u32,
    saves: u32,
    /// Per rare field (`RARE`), whether it was ever non-default at a save point.
    rare: [bool; RARE.len()],
}

/// The fields that are default in most states; the coverage record says which the scenarios reach.
const RARE: [&str; 18] = [
    "sales",
    "rescue_used",
    "stalled_turns",
    "emergency",
    "repair_backoff",
    "rally_backoff",
    "resume",
    "refused",
    "blocked_until",
    "jitter",
    "sites_cleared",
    "intel",
    "harv_seen",
    "merge_into",
    "target_hp",
    "recalled",
    "closing",
    "full_turns",
];

/// Which of `RARE` are non-default in `s`, in that order.
fn rare_of(s: &AiState) -> [bool; RARE.len()] {
    let g = |f: &dyn Fn(&sf_ai::state::Group) -> bool| s.groups.iter().any(f);
    [
        !s.sales.is_empty(),
        s.rescue_used,
        s.stalled_turns != 0,
        s.emergency.is_some(),
        !s.repair_backoff.is_empty(),
        !s.rally_backoff.is_empty(),
        !s.resume.is_empty(),
        !s.refused.is_empty(),
        !s.blocked_until.is_empty(),
        s.jitter.is_some(),
        !s.sites_cleared.is_empty(),
        !s.intel.is_empty(),
        !s.harv_seen.is_empty(),
        g(&|g| g.merge_into.is_some()),
        g(&|g| g.target_hp.is_some()),
        g(&|g| !g.recalled.is_empty()),
        g(&|g| !g.closing.is_empty()),
        s.full_turns != 0,
    ]
}

fn note(seen: &mut Seen, s: &AiState) {
    seen.saves += 1;
    for (r, now) in seen.rare.iter_mut().zip(rare_of(s)) {
        *r |= now;
    }
    if s.groups.iter().any(|g| g.stage == GroupStage::Staging) {
        seen.wave_forming += 1;
    }
    if s.pending_build.is_some() || !s.units_sent.is_empty() || s.harvester_sent.is_some() {
        seen.order_in_flight += 1;
    }
    if s.directive.posture == Posture::Attack
        && s.groups.iter().any(|g| g.stage != GroupStage::Staging)
    {
        seen.attack_under_way += 1;
    }
    if s.threat.is_some() && s.directive.posture == Posture::Defend {
        seen.defending += 1;
    }
    if s.scout.is_some() && !s.known_structures.is_empty() {
        seen.scouting_memory += 1;
    }
}

fn round_trip(names: &[&str]) -> Seen {
    let mut seen = Seen::default();
    for name in names {
        let s = load(name);
        let fresh = s.ais.clone();
        assert!(!fresh.is_empty(), "{name} has no AI");
        run_observed(&s, &options(&s), |world, ais| {
            if world.tick().0 % K != 0 {
                return;
            }
            for (ai, fresh) in ais.iter().zip(&fresh) {
                let bytes = ai.save().unwrap();
                let back = Ai::restore(fresh.clone(), &bytes, world)
                    .unwrap_or_else(|e| panic!("{name} at {:?}: {e}", world.tick()));
                assert_eq!(&back, ai, "{name} at {:?}", world.tick());
                assert_eq!(back.save().unwrap(), bytes, "{name} at {:?}", world.tick());
                note(&mut seen, ai.state());
            }
        })
        .unwrap();
    }
    seen
}

#[test]
fn every_ai_state_of_the_ci_games_survives_a_round_trip_and_resaves_byte_equal() {
    let seen = round_trip(&SCENARIOS);
    eprintln!("{seen:?}");
    assert!(seen.saves > 1000, "{seen:?}");
    assert!(seen.wave_forming > 0, "{seen:?}");
    assert!(seen.order_in_flight > 0, "{seen:?}");
    assert!(seen.attack_under_way > 0, "{seen:?}");
    assert!(seen.defending > 0, "{seen:?}");
    assert!(seen.scouting_memory > 0, "{seen:?}");
    let missed: Vec<&str> = RARE
        .iter()
        .zip(seen.rare)
        .filter(|(_, r)| !r)
        .map(|(n, _)| *n)
        .collect();
    eprintln!("rare fields never reached: {missed:?}");
    assert_eq!(
        missed, NOT_REACHED,
        "the rare fields the scenarios never reach"
    );
}

/// The rare fields no CI scenario holds at a save point; each has its own round trip in
/// `the_rare_fields_no_scenario_reaches_round_trip`.
const NOT_REACHED: [&str; 9] = [
    "repair_backoff",
    "resume",
    "refused",
    "jitter",
    "sites_cleared",
    "intel",
    "merge_into",
    "recalled",
    "closing",
];

#[test]
fn the_rare_fields_no_scenario_reaches_round_trip() {
    let (base, b) = rich();
    let kind = *b.kinds.first().unwrap();
    let id = base.groups[0].members[0];
    let t = base.last_think.unwrap();
    let other = base
        .start_sites
        .iter()
        .find(|(p, _)| *p != base.player)
        .unwrap()
        .0;
    let edits: [(&str, &dyn Fn(&mut AiState)); 9] = [
        ("repair_backoff", &|s| {
            s.repair_backoff.insert(id, t);
        }),
        ("resume", &|s| {
            s.resume.insert(kind, 3);
        }),
        ("refused", &|s| {
            s.refused.insert((kind, Cell::new(1, 2)), t);
        }),
        ("jitter", &|s| s.jitter = Some(0)),
        ("sites_cleared", &|s| s.sites_cleared = vec![other]),
        ("intel", &|s| {
            s.profile.army.counter_permille = 100;
            s.intel.insert(kind, (2, t));
        }),
        ("merge_into", &|s| {
            let mut g = s.groups[0].clone();
            g.id = s.next_group;
            g.members.clear();
            g.recalled.clear();
            g.closing.clear();
            g.merge_into = Some(s.groups[0].id);
            s.next_group += 1;
            s.groups.push(g);
        }),
        ("recalled", &|s| s.groups[0].recalled = vec![id]),
        ("closing", &|s| s.groups[0].closing = vec![id]),
    ];
    assert_eq!(
        edits.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        NOT_REACHED
    );
    for (name, edit) in edits {
        let mut s = base.clone();
        edit(&mut s);
        let i = RARE.iter().position(|n| *n == name).unwrap();
        assert!(rare_of(&s)[i], "{name} is not set");
        let bytes = save_state(&s);
        let back =
            restore_state(fresh_of(&s), &bytes, &b).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(back, s, "{name}");
        assert_eq!(save_state(&back), bytes, "{name}: resave");
    }
}

/// Per tick, `(tick, digest of every AI)`.
type Digests = Vec<(u64, Vec<u64>)>;

fn digests_of(world: &World, ais: &[Ai], out: &mut Digests) {
    out.push((world.tick().0, ais.iter().map(Ai::digest).collect()));
}

fn uninterrupted(s: &Scenario) -> (Finished, Digests) {
    let mut d = Vec::new();
    let f = run_observed(s, &options(s), |w, ais| digests_of(w, ais, &mut d)).unwrap();
    (f, d)
}

/// The run with every AI replaced by its restored copy before the thinks of each tick `at` says yes to.
fn interrupted(s: &Scenario, at: impl Fn(u64) -> bool) -> (Finished, Digests, u32) {
    let fresh = s.ais.clone();
    let mut d = Vec::new();
    let mut restores = 0;
    let f = run_hooked(
        s,
        &options(s),
        |world, ais| {
            if !at(world.tick().0) {
                return;
            }
            for (ai, fresh) in ais.iter_mut().zip(&fresh) {
                let bytes = ai.save().unwrap();
                *ai = Ai::restore(fresh.clone(), &bytes, world).unwrap();
                restores += 1;
            }
        },
        |w, ais| digests_of(w, ais, &mut d),
    )
    .unwrap();
    (f, d, restores)
}

fn continue_equals(name: &str, at: impl Fn(u64) -> bool) -> u32 {
    let s = load(name);
    let (a, da) = uninterrupted(&s);
    let (b, db, restores) = interrupted(&s, at);
    assert_eq!(da.len(), db.len(), "{name}");
    for (x, y) in da.iter().zip(&db) {
        assert_eq!(x, y, "{name}: the AI digests differ after tick {}", x.0);
    }
    assert_eq!(
        a.ai_commands, b.ai_commands,
        "{name}: the AI commands differ"
    );
    assert_eq!(
        a.outcome.final_checksum, b.outcome.final_checksum,
        "{name}: the world checksum differs"
    );
    assert_eq!(a.outcome.ai_digests, b.outcome.ai_digests, "{name}");
    assert!(restores > 0, "{name}: nothing was restored");
    restores
}

#[test]
fn a_restored_ai_continues_exactly_as_the_uninterrupted_one_every_97_ticks() {
    for name in SCENARIOS {
        continue_equals(name, |t| t > 0 && t % K == 0);
    }
}

#[test]
fn a_restored_ai_continues_exactly_as_the_uninterrupted_one_at_every_tick_of_ai_raid() {
    let n = continue_equals("ai_raid", |_| true);
    assert!(n >= 5600, "{n}");
}

#[test]
fn a_restored_ai_continues_exactly_as_the_uninterrupted_one_from_single_ticks() {
    for t in [101u64, 1009, 2503, 4001] {
        continue_equals("ai_raid", move |x| x == t);
    }
}

#[test]
fn a_restored_ai_continues_exactly_on_the_profiles_with_groups_and_escalation() {
    // The control: with a wrong state the comparison does fail (a restore one think off in the rotation).
    let s = load("ai_raid_guard");
    let (a, da) = uninterrupted(&s);
    let fresh = s.ais.clone();
    let mut db = Vec::new();
    let b = run_hooked(
        &s,
        &options(&s),
        |world, ais| {
            if world.tick().0 == 997 {
                for (ai, fresh) in ais.iter_mut().zip(&fresh) {
                    let mut st = ai.state().clone();
                    st.thinks += 1;
                    let bytes = save_state(&st);
                    *ai = Ai::restore(fresh.clone(), &bytes, world).unwrap();
                }
            }
        },
        |w, ais| digests_of(w, ais, &mut db),
    )
    .unwrap();
    assert_ne!(
        da, db,
        "a think lost in the rotation must show in the digests"
    );
    let _ = (a, b);
    continue_equals("ai_raid_guard", |t| t % 31 == 7);
    continue_equals("ai_bounce", |t| t % 389 == 11);
}

// --- corrupt cases ----------------------------------------------------------------------------------------------

/// A state from a real game with groups, known structures and a scout, and the bounds of its world.
fn rich() -> (AiState, SaveBounds) {
    let s = load("ai_raid_guard");
    let mut out = None;
    run_observed(&s, &options(&s), |world, ais| {
        if out.is_some() {
            return;
        }
        let st = ais[0].state();
        if !st.groups.is_empty()
            && !st.known_structures.is_empty()
            && !st.hp_seen.is_empty()
            && !st.sent.is_empty()
        {
            out = Some((st.clone(), SaveBounds::of(world)));
        }
    })
    .unwrap();
    out.expect("ai_raid_guard reaches a state with groups")
}

fn outcome(s: &AiState, b: &SaveBounds) -> Result<AiState, SaveError> {
    restore_state(fresh_of(s), &save_state(s), b)
}

/// `edit` makes the state corrupt with `what`; `ok` puts the value at its bound, which passes.
fn refuses(
    base: &(AiState, SaveBounds),
    what: &str,
    ok: impl Fn(&mut AiState, &SaveBounds),
    edit: impl Fn(&mut AiState, &SaveBounds),
) {
    let (s, b) = base;
    let mut good = s.clone();
    ok(&mut good, b);
    match outcome(&good, b) {
        Ok(back) => assert_eq!(back, good, "{what}: the bound value"),
        Err(e) => panic!("{what}: the bound value was refused: {e:?}"),
    }
    let mut bad = s.clone();
    edit(&mut bad, b);
    match outcome(&bad, b) {
        Err(SaveError::Corrupt(m)) => assert_eq!(m, what),
        other => panic!("{what}: got {other:?}"),
    }
}

fn noop(_: &mut AiState, _: &SaveBounds) {}

fn last(b: &SaveBounds) -> Tick {
    Tick(b.tick.0 - 1)
}

#[test]
fn every_restore_check_refuses_its_value_with_its_message_and_passes_the_bound() {
    let base = rich();
    let (s, b) = &base;
    assert_eq!(outcome(s, b).as_ref(), Ok(s), "the unchanged state");
    let id0 = *s.known_structures.keys().next().unwrap();
    let k0 = s.known_structures[&id0];
    let (w, h) = b.map;
    let kind = s.known_structures[&id0].kind;

    refuses(
        &base,
        "an entity id names a slot the world does not have",
        |s, b| s.scout = Some(EntityId::new(b.slots - 1, 7)),
        |s, b| s.scout = Some(EntityId::new(b.slots, 0)),
    );
    refuses(
        &base,
        "a cell lies outside the map",
        |s, b| s.gather = Some(Cell::new(b.map.0 - 1, b.map.1 - 1)),
        |s, b| s.gather = Some(Cell::new(b.map.0, 0)),
    );
    refuses(&base, "a cell lies outside the map", noop, |s, b| {
        s.gather = Some(Cell::new(0, b.map.1))
    });
    refuses(
        &base,
        "a kind is not in the unit table",
        |s, b| s.emergency = Some(*b.kinds.last().unwrap()),
        |s, b| s.emergency = Some(Kind(b.kinds.last().unwrap().0 + 1)),
    );
    refuses(
        &base,
        "more thinks than the world has run ticks",
        |s, b| {
            s.thinks = b.tick.0;
            s.manager_runs = runs(b.tick.0);
        },
        |s, b| {
            s.thinks = b.tick.0 + 1;
            s.manager_runs = runs(b.tick.0 + 1);
        },
    );
    refuses(
        &base,
        "the last think and the number of thinks disagree",
        noop,
        |s, _| s.last_think = None,
    );
    refuses(
        &base,
        "the last think is not before the world's tick",
        |s, b| s.last_think = Some(last(b)),
        |s, b| s.last_think = Some(b.tick),
    );
    refuses(
        &base,
        "a structure seen of the AI's own player",
        noop,
        |s, _| s.known_structures.get_mut(&id0).unwrap().owner = s.player,
    );
    refuses(
        &base,
        "a structure seen does not fit the map",
        |s, _| {
            let k = s.known_structures.get_mut(&id0).unwrap();
            k.anchor = Cell::new(w - u32::from(k.width), h - u32::from(k.height));
        },
        |s, _| {
            let k = s.known_structures.get_mut(&id0).unwrap();
            k.anchor = Cell::new(w - u32::from(k.width) + 1, 0);
        },
    );
    refuses(
        &base,
        "a structure seen does not fit the map",
        noop,
        |s, _| s.known_structures.get_mut(&id0).unwrap().width = 0,
    );
    let at_last = |s: &AiState| s.last_think.unwrap();
    refuses(
        &base,
        "a structure was seen after the last think",
        |s, _| s.known_structures.get_mut(&id0).unwrap().seen_tick = at_last(s),
        |s, _| s.known_structures.get_mut(&id0).unwrap().seen_tick = Tick(at_last(s).0 + 1),
    );
    refuses(
        &base,
        "a build was sent after the last think",
        |s, _| s.pending_build = Some((kind, k0.anchor, at_last(s))),
        |s, _| s.pending_build = Some((kind, k0.anchor, Tick(at_last(s).0 + 1))),
    );
    let horizon =
        |s: &AiState| s.last_think.unwrap().0 + u64::from(s.profile.economy.blocked_ticks);
    refuses(
        &base,
        "more refused places than the cap",
        |s, _| {
            for i in 0..sf_ai::REFUSED_CAP as u32 {
                s.refused.insert((kind, Cell::new(i % w, i / w)), Tick(0));
            }
            while s.refused.len() > sf_ai::REFUSED_CAP {
                let k = *s.refused.keys().last().unwrap();
                s.refused.remove(&k);
            }
        },
        |s, _| {
            for i in 0..=sf_ai::REFUSED_CAP as u32 {
                s.refused.insert((kind, Cell::new(i % w, i / w)), Tick(0));
            }
        },
    );
    refuses(
        &base,
        "a kind or place is blocked past what a think can set",
        |s, _| {
            s.blocked_until.insert(kind, Tick(horizon(s)));
        },
        |s, _| {
            s.blocked_until.insert(kind, Tick(horizon(s) + 1));
        },
    );
    refuses(
        &base,
        "a kind or place is blocked past what a think can set",
        noop,
        |s, _| {
            s.refused.insert((kind, k0.anchor), Tick(horizon(s) + 1));
        },
    );
    refuses(
        &base,
        "a harvester was sent after the last think",
        |s, _| s.harvester_sent = Some((kind, at_last(s))),
        |s, _| s.harvester_sent = Some((kind, Tick(at_last(s).0 + 1))),
    );
    refuses(
        &base,
        "a sale was sent after the last think",
        |s, _| s.sales = vec![(id0, at_last(s))],
        |s, _| s.sales = vec![(id0, Tick(at_last(s).0 + 1))],
    );
    refuses(
        &base,
        "the rescue waits past what a think can set",
        |s, _| s.rescue_after = Tick(horizon(s)),
        |s, _| s.rescue_after = Tick(horizon(s) + 1),
    );
    refuses(
        &base,
        "a backoff runs past what a think can set",
        |s, _| {
            s.repair_backoff.insert(id0, Tick(horizon(s)));
        },
        |s, _| {
            s.repair_backoff.insert(id0, Tick(horizon(s) + 1));
        },
    );
    refuses(
        &base,
        "a backoff runs past what a think can set",
        |s, _| {
            s.rally_backoff.insert(id0, Tick(horizon(s)));
        },
        |s, _| {
            s.rally_backoff.insert(id0, Tick(horizon(s) + 1));
        },
    );
    refuses(
        &base,
        "a unit order was sent after the last think",
        |s, _| s.units_sent = vec![(id0, kind, at_last(s), 0, 0)],
        |s, _| s.units_sent = vec![(id0, kind, Tick(at_last(s).0 + 1), 0, 0)],
    );
    refuses(
        &base,
        "the unit orders are not strictly ascending by factory",
        |s, _| {
            s.units_sent = vec![
                (EntityId::new(0, 0), kind, Tick(0), 0, 0),
                (id0, kind, Tick(0), 0, 0),
            ]
        },
        |s, _| s.units_sent = vec![(id0, kind, Tick(0), 0, 0), (id0, kind, Tick(0), 1, 0)],
    );
    let legs =
        |s: &AiState| (s.start_sites.iter().filter(|(p, _)| *p != s.player).count() + 4) as u8;
    refuses(
        &base,
        "the scout is past its last waypoint",
        |s, _| s.scout_leg = legs(s),
        |s, _| s.scout_leg = legs(s) + 1,
    );
    refuses(
        &base,
        "more scouts sent than the profile allows",
        |s, _| s.scouts_sent = s.profile.army.scouts_max.max(1),
        |s, _| s.scouts_sent = s.profile.army.scouts_max.max(1) + 1,
    );
    refuses(
        &base,
        "a scout time is after the last think",
        |s, _| s.scout_gone_at = Some(at_last(s)),
        |s, _| s.scout_leg_sent = Some(Tick(at_last(s).0 + 1)),
    );
    refuses(
        &base,
        "a hit is after the last think",
        |s, _| s.defend_since = Some(at_last(s)),
        |s, _| s.threat = Some((k0.anchor, Tick(at_last(s).0 + 1))),
    );
    refuses(
        &base,
        "more groups than the most at one time",
        |s, _| {
            let g = s.groups[0].clone();
            s.groups.clear();
            for i in 0..sf_ai::state::GROUPS_MAX as u32 {
                let mut g = g.clone();
                g.id = i;
                g.merge_into = None;
                g.members = if i == 0 { g.members } else { Vec::new() };
                s.groups.push(g);
            }
            s.next_group = s.next_group.max(sf_ai::state::GROUPS_MAX as u32);
        },
        |s, _| {
            let g = s.groups[0].clone();
            s.groups.clear();
            for i in 0..=sf_ai::state::GROUPS_MAX as u32 {
                let mut g = g.clone();
                g.id = i;
                g.members = Vec::new();
                s.groups.push(g);
            }
            s.next_group = 100;
        },
    );
    refuses(
        &base,
        "a group id is not below the next one",
        |s, _| s.next_group = s.groups.last().unwrap().id + 1,
        |s, _| s.next_group = s.groups.last().unwrap().id,
    );
    refuses(
        &base,
        "the groups are not strictly ascending by id",
        noop,
        |s, _| {
            let mut g = s.groups[0].clone();
            g.members = Vec::new();
            s.groups.push(g);
        },
    );
    refuses(
        &base,
        "a group entered its stage after the last think",
        |s, _| s.groups[0].since = at_last(s),
        |s, _| s.groups[0].since = Tick(at_last(s).0 + 1),
    );
    refuses(
        &base,
        "a group saw its target after the last think",
        |s, _| s.groups[0].target_hp = Some((id0, 1, at_last(s))),
        |s, _| s.groups[0].target_hp = Some((id0, 1, Tick(at_last(s).0 + 1))),
    );
    refuses(
        &base,
        "a group merges into a group that never was",
        |s, _| s.groups[0].merge_into = Some(s.next_group - 1).filter(|&m| m != s.groups[0].id),
        |s, _| s.groups[0].merge_into = Some(s.next_group),
    );
    refuses(
        &base,
        "a group merges into a group that never was",
        noop,
        |s, _| s.groups[0].merge_into = Some(s.groups[0].id),
    );
    refuses(
        &base,
        "a group recalls a unit that is not its member",
        |s, _| s.groups[0].recalled = s.groups[0].members.clone(),
        |s, _| {
            let g = &mut s.groups[0];
            g.recalled = g.members.clone();
            g.members.remove(0);
        },
    );
    refuses(
        &base,
        "a group closes in with a unit that is not its member",
        |s, _| s.groups[0].closing = s.groups[0].members.clone(),
        |s, _| {
            let g = &mut s.groups[0];
            g.closing = g.members.clone();
            g.members.remove(0);
        },
    );
    refuses(&base, "a unit is in two groups", noop, |s, _| {
        let mut g = s.groups[0].clone();
        g.id = s.next_group;
        s.next_group += 1;
        s.groups.push(g);
    });
    refuses(
        &base,
        "a list of entity ids is not strictly ascending",
        noop,
        |s, _| s.groups[0].recalled = vec![id0, id0],
    );
    refuses(
        &base,
        "an attack went out after the last think",
        |s, _| s.last_attack_tick = at_last(s),
        |s, _| s.last_attack_tick = Tick(at_last(s).0 + 1),
    );
    refuses(
        &base,
        "an attack started after the last think",
        |s, _| s.attack_started = at_last(s),
        |s, _| s.attack_started = Tick(at_last(s).0 + 1),
    );
    refuses(
        &base,
        "the jitter is outside the profile's spread",
        |s, _| s.jitter = Some(-i32::from(s.profile.army.timing_jitter_permille)),
        |s, _| s.jitter = Some(i32::from(s.profile.army.timing_jitter_permille) + 1),
    );
    refuses(
        &base,
        "the jitter is outside the profile's spread",
        |s, _| s.jitter = Some(i32::from(s.profile.army.timing_jitter_permille)),
        |s, _| s.jitter = Some(-i32::from(s.profile.army.timing_jitter_permille) - 1),
    );
    let other = s
        .start_sites
        .iter()
        .find(|(p, _)| *p != s.player)
        .unwrap()
        .0;
    refuses(
        &base,
        "a cleared site is not another player's start site",
        |s, _| s.sites_cleared = vec![other],
        |s, _| s.sites_cleared = vec![s.player],
    );
    refuses(
        &base,
        "a cleared site is not another player's start site",
        noop,
        |s, _| s.sites_cleared = vec![PlayerId(200)],
    );
    refuses(
        &base,
        "the cleared sites are not strictly ascending",
        noop,
        |s, _| s.sites_cleared = vec![other, other],
    );
    refuses(
        &base,
        "a unit was ordered after the last think",
        |s, _| {
            let t = at_last(s);
            s.sent.insert(id0, (t, 0));
        },
        |s, _| {
            let t = Tick(at_last(s).0 + 1);
            s.sent.insert(id0, (t, 0));
        },
    );
}

fn runs(thinks: u64) -> [u32; 4] {
    let mut r = [0u32; 4];
    for (m, x) in r.iter_mut().enumerate() {
        *x = (thinks / 4 + u64::from((m as u64) < thinks % 4)) as u32;
    }
    r
}

#[test]
fn the_rules_gated_parts_and_the_enum_tags_are_checked() {
    let base = rich();
    let (s, b) = &base;
    // These rules are off in the profile of `ai_raid_guard`; on, the same values pass (bounds above).
    assert!(!s.profile.army.counter_on());
    assert!(!s.profile.army.escalate_on());
    assert!(!s.profile.army.harvester_threat);
    let kind = *b.kinds.first().unwrap();
    refuses(&base, "a tally with the counter mix off", noop, |s, _| {
        s.intel.insert(kind, (1, Tick(0)));
    });
    refuses(
        &base,
        "a bounce ledger with the escalation off",
        noop,
        |s, _| s.bounces = 1,
    );
    refuses(
        &base,
        "a bounce ledger with the escalation off",
        noop,
        |s, _| s.bounce_value = 1,
    );
    refuses(
        &base,
        "harvesters seen with the harvester threat off",
        noop,
        |s, _| {
            s.harv_seen
                .insert(EntityId::new(0, 0), (kind, Cell::new(0, 0), 1));
        },
    );
    let mut on = s.clone();
    on.profile.army.counter_permille = 100;
    on.profile.army.escalate_permille = 100;
    on.profile.army.harvester_threat = true;
    on.intel.insert(kind, (1, Tick(0)));
    on.bounces = 2;
    on.bounce_value = 9;
    on.bounce_tick = on.last_think.unwrap();
    on.harv_seen
        .insert(EntityId::new(0, 0), (kind, Cell::new(0, 0), 1));
    assert_eq!(outcome(&on, b).as_ref(), Ok(&on));
    let mut late = on.clone();
    late.bounce_tick = Tick(on.last_think.unwrap().0 + 1);
    assert_eq!(
        outcome(&late, b).err(),
        Some(SaveError::Corrupt("a strike failed after the last think"))
    );
    let mut late = on.clone();
    late.intel
        .insert(kind, (1, Tick(on.last_think.unwrap().0 + 1)));
    assert_eq!(
        outcome(&late, b).err(),
        Some(SaveError::Corrupt(
            "the tally saw a peak after the last think"
        ))
    );

    // The enum tags, by writing an unknown number into their byte. The byte is found as the first one that
    // differs between two saves that differ only in that field.
    let bytes = save_state(s);
    let at = |edit: &dyn Fn(&mut AiState)| {
        let mut t = s.clone();
        edit(&mut t);
        let other = save_state(&t);
        bytes.iter().zip(&other).position(|(x, y)| x != y).unwrap()
    };
    let probe = |at: usize, v: u8| {
        let mut x = bytes.clone();
        x[at] = v;
        restore_state(fresh_of(s), &x, b).err()
    };
    let posture = at(&|t| {
        t.directive.posture = match t.directive.posture {
            Posture::Gather => Posture::Attack,
            _ => Posture::Gather,
        }
    });
    assert_eq!(bytes[posture], s.directive.posture as u8);
    assert_eq!(
        probe(posture, 3),
        Some(SaveError::Corrupt("an unknown posture"))
    );
    let stage = at(&|t| {
        t.groups[0].stage = match t.groups[0].stage {
            GroupStage::Staging => GroupStage::Engaged,
            _ => GroupStage::Staging,
        }
    });
    assert_eq!(bytes[stage], s.groups[0].stage as u8);
    assert_eq!(
        probe(stage, 4),
        Some(SaveError::Corrupt("an unknown group stage"))
    );
    // The random stream's increment: a live generator never has an even one (`Rng::from_state` would make it odd).
    let inc = at(&|t| {
        let mut st = t.rng.state();
        st.inc ^= 2;
        t.rng = sf_math::Rng::from_state(st);
    });
    assert_eq!(bytes[inc] & 1, 1);
    assert_eq!(
        probe(inc, bytes[inc] & !1),
        Some(SaveError::Corrupt(
            "the random stream has an even increment"
        ))
    );
    // The role follows the group's id (four bytes).
    let role = at(&|t| t.groups[0].id ^= 1) + 4;
    assert_eq!(bytes[role], 0, "the role of the first group (strike)");
    assert_eq!(
        probe(role, 1),
        Some(SaveError::Corrupt("an unknown group role"))
    );
}

#[test]
fn the_profile_the_player_the_sites_the_version_and_a_used_fresh_state_are_refused() {
    let (s, b) = rich();
    let bytes = save_state(&s);
    // Another profile: rules.
    let mut f = fresh_of(&s);
    f.profile.think_interval += 1;
    let mut other = fresh_of(&s);
    other.profile = f.profile.clone();
    assert_eq!(
        restore_state(fresh_of(&other), &bytes, &b).err(),
        Some(SaveError::Rules)
    );
    // Another player.
    let mut p = s.clone();
    p.player = PlayerId(s.player.0 + 1);
    assert_eq!(
        restore_state(fresh_of(&p), &bytes, &b).err(),
        Some(SaveError::Rules)
    );
    // Other start sites.
    let mut q = s.clone();
    q.start_sites[0].1 = Cell::new(q.start_sites[0].1.x + 1, q.start_sites[0].1.y);
    assert_eq!(
        restore_state(fresh_of(&q), &bytes, &b).err(),
        Some(SaveError::Rules)
    );
    // Another AI format version: the u32 after the tag and the length.
    let mut v = bytes.clone();
    v[5] = v[5].wrapping_add(1);
    assert_eq!(
        restore_state(fresh_of(&s), &v, &b).err(),
        Some(SaveError::Version(sf_ai::AI_FORMAT_VERSION + 1))
    );
    // Another tag.
    let mut t = bytes.clone();
    t[0] = b'B';
    assert_eq!(
        restore_state(fresh_of(&s), &t, &b).err(),
        Some(SaveError::Corrupt("a section has the wrong tag or length"))
    );
    // A fresh state that has already thought, or holds a memory.
    let mut used = fresh_of(&s);
    used.thinks = 1;
    assert_eq!(
        restore_state(used, &bytes, &b).err(),
        Some(SaveError::NotFresh)
    );
    let mut used = fresh_of(&s);
    used.gather = Some(Cell::new(1, 1));
    assert_eq!(
        restore_state(used, &bytes, &b).err(),
        Some(SaveError::NotFresh)
    );
    // Its own random stream is fine: the host seeds it as for a new match.
    let mut seeded = fresh_of(&s);
    seeded.rng = sf_math::Rng::new(99);
    assert_eq!(restore_state(seeded, &bytes, &b).as_ref(), Ok(&s));
    // Every cut short is truncated or corrupt, never a panic; trailing bytes are refused by `Ai::restore`.
    for n in 0..bytes.len() {
        assert!(
            restore_state(fresh_of(&s), &bytes[..n], &b).is_err(),
            "cut at {n}"
        );
    }
}

#[test]
fn a_dead_entity_the_ai_remembers_is_kept_as_the_live_ai_keeps_it() {
    // The AI remembers structures and units that die later and drops them at its next look. A remembered id
    // whose entity is dead in the world is restored unchanged. This test proves only the round trip; that the
    // run goes on as before is the continue-equals tests' part, whose save points include such states. A
    // recycled slot (same index, newer generation in the world) is a known gap, not tested here.
    let mut found = 0;
    for name in ["ai_vs_passive", "ai_raid_guard", "ai_bounce"] {
        let s = load(name);
        run_observed(&s, &options(&s), |world, ais| {
            for (ai, fresh) in ais.iter().zip(&s.ais) {
                let st = ai.state();
                let dead = st
                    .known_structures
                    .keys()
                    .chain(st.sent.keys())
                    .chain(st.groups.iter().flat_map(|g| g.members.iter()))
                    .any(|id| !world.is_alive(*id));
                if dead {
                    found += 1;
                    let back = Ai::restore(fresh.clone(), &ai.save().unwrap(), world).unwrap();
                    assert_eq!(&back, ai);
                }
            }
        })
        .unwrap();
    }
    assert!(found > 0, "no AI remembers a dead entity at a save point");
}

#[test]
fn trailing_bytes_after_the_ai_section_are_refused() {
    let s = load("ai_fog");
    let f = run_observed(
        &s,
        &RunOptions {
            ticks: 200,
            every: 100,
        },
        |_, _| {},
    )
    .unwrap();
    let mut bytes = f.ais[0].save().unwrap();
    bytes.push(0);
    assert_eq!(
        Ai::restore(s.ais[0].clone(), &bytes, &f.world).err(),
        Some(SaveError::Corrupt("a section has the wrong tag or length"))
    );
}

#[test]
fn save_bounds_take_the_worlds_tick_map_slots_and_sorted_kinds() {
    // A world from a scenario after 200 ticks: entities, so the slots are not zero.
    let s = load("ai_fog");
    let f = run_observed(
        &s,
        &RunOptions {
            ticks: 200,
            every: 100,
        },
        |_, _| {},
    )
    .unwrap();
    let b = SaveBounds::of(&f.world);
    assert_eq!(b.tick, f.world.tick());
    assert_eq!(b.tick, Tick(200));
    assert_eq!(b.map, f.world.map_size());
    assert!(f.world.slot_count() > 0);
    assert_eq!(b.slots as usize, f.world.slot_count());
    // A unit table handed over in descending order: the kinds come out ascending, once each.
    let embedded = sf_bridge::unit_table(&sf_data::units::UnitDefs::embedded()).unwrap();
    let mut stats: Vec<_> = embedded.iter().copied().collect();
    stats.reverse();
    assert!(stats.len() > 1 && stats[0].kind > stats[1].kind);
    let table = sf_sim::UnitTable::new(stats).unwrap();
    let mut world = World::new(sf_sim::WorldConfig::new(5).with_map_size(40, 30))
        .unwrap()
        .with_units(table)
        .unwrap();
    world.step();
    let b = SaveBounds::of(&world);
    let mut want: Vec<Kind> = embedded.iter().map(|u| u.kind).collect();
    want.sort();
    want.dedup();
    assert_eq!(b.kinds, want);
    assert!(b.kinds.windows(2).all(|p| p[0] < p[1]));
    assert_eq!((b.tick, b.map, b.slots), (Tick(1), (40, 30), 0));
}
