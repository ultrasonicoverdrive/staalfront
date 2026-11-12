//! The AI frame (WP9.1a): the `ai_fog` scenario with its checksum and AI digest pinned, and the proofs that
//! the AI never sees what its player has not seen. See `assets/scenarios/ai_fog.ron`, `docs/rules/ai.md`.
//!
//! Player 0 (the AI) has a construction centre (slot 0) and a scout (slot 1). Player 1 is static: a
//! construction centre (2), a power plant (3), a barracks (4) and a refinery (5), and an army in the far
//! corner (6 to 8). The scout walks to the power plant and the barracks and sees exactly those two, the
//! barracks is removed at tick 500, the scout walks home at tick 520.
//!
//! Every test calls the code the game uses: `run_full` drives the AIs like the runner does, and the
//! observation is built by `Observation::build`, the one function the AI reads the world through.

mod common;

use std::collections::BTreeMap;

use common::scenario_path;
use sf_ai::{Ai, Known, Observation};
use sf_data::ai_profile::AiProfile;
use sf_data::units::{Faction, UnitDefs};
use sf_headless::cli::parse_checksum;
use sf_headless::run::{Finished, RunOptions, run, run_full};
use sf_headless::scenario::{Action, IdSpec, Scenario, TimedCommand};
use sf_sim::{Cell, Command, EntityId, Kind, PlayerId, Tick};

const EXPECTED: &str = include_str!("../../../assets/scenarios/ai_fog.expected");
/// `Ai::digest` of player 0's AI at the end of the shipped scenario (800 ticks).
///
/// Re-pinned by WP9.1b-1 (was `0x993a_fc7c_d2c4_2cfd`, with the checksum `0xd369218db6025eff`): the AI now
/// builds (a power plant at tick 10, a refinery at tick 330) and its digest hashes the new state and the
/// profile's lane and aprons. Measured: with the construction manager switched off, the new state left out
/// of `AiState::digest` and the lane and aprons left out of `Profile::digest`, both old values come back.
///
/// Re-pinned by WP9.1b-2 to `0x4a4e_ffe3_e0fa_05d1`; the checksum `0x768b77651509db8c` did not move (no
/// harvester is ordered: the AI has no vehicle factory in 800 ticks, and nothing is damaged). The digest moved
/// because it now hashes the economy state (`harvester_sent`, `full_turns`, `emergency`, `repair_backoff`):
/// measured, with the economy manager off and those left out of `AiState::digest` the WP9.1b-1 value
/// `0x630c_0979_a8ae_4749` comes back (and with the manager off but the state hashed the digest stays `0x4a4e...`).
const AI_DIGEST: u64 = 0x4a4e_ffe3_e0fa_05d1;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const SCOUT: EntityId = EntityId::new(1, 0);
const CENTRE_1: EntityId = EntityId::new(2, 0);
const PLANT: EntityId = EntityId::new(3, 0);
const BARRACKS: EntityId = EntityId::new(4, 0);
const REFINERY: EntityId = EntityId::new(5, 0);
const ARMY: [EntityId; 3] = [
    EntityId::new(6, 0),
    EntityId::new(7, 0),
    EntityId::new(8, 0),
];

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("ai_fog.ron")).unwrap()
}

fn go(s: &Scenario, ticks: u64) -> Finished {
    run_full(s, &RunOptions { ticks, every: 100 }).unwrap()
}

fn known(f: &Finished) -> &BTreeMap<EntityId, Known> {
    &f.ais[0].state().known_structures
}

fn ids(map: &BTreeMap<EntityId, Known>) -> Vec<EntityId> {
    map.keys().copied().collect()
}

/// What player 0 can observe in `f`'s world, with no memory.
fn observe(f: &Finished) -> Observation {
    Observation::build(&f.world, P0, &BTreeMap::new())
}

fn visible_ids(o: &Observation) -> Vec<EntityId> {
    o.visible.iter().map(|s| s.id).collect()
}

/// The scenario without the scout's walk: the AI has nothing that sees player 1.
fn without_the_scout(mut s: Scenario) -> Scenario {
    s.commands
        .retain(|c| !matches!(c.command, Action::OrderMove { .. }));
    s
}

/// The scenario without the army of player 1 in the far corner.
fn without_the_army(mut s: Scenario) -> Scenario {
    s.commands
        .retain(|c| !matches!(c.command, Action::SpawnUnit { .. }));
    s
}

#[test]
fn the_final_checksum_and_the_ai_digest_are_the_pinned_values() {
    let expected = parse_checksum(EXPECTED).unwrap();
    let s = scenario();
    let outcome = run(&s, &RunOptions::for_scenario(&s)).unwrap();
    assert_eq!(outcome.final_checksum, expected);
    assert_eq!(outcome.ai_digests, vec![(0, AI_DIGEST)]);
}

#[test]
fn a_run_is_repeatable_including_the_ai() {
    let s = scenario();
    let a = run(&s, &RunOptions::for_scenario(&s)).unwrap();
    let b = run(&s, &RunOptions::for_scenario(&s)).unwrap();
    assert_eq!(a, b);
}

#[test]
fn the_scout_teaches_the_ai_the_two_structures_it_sees_and_only_those() {
    let f = go(&scenario(), 400);
    let k = known(&f);
    assert_eq!(ids(k), vec![PLANT, BARRACKS]);
    let plant = k[&PLANT];
    assert_eq!(
        (
            plant.kind,
            plant.owner,
            plant.anchor,
            plant.width,
            plant.height
        ),
        (Kind(1), P1, Cell::new(40, 60), 2, 3)
    );
    let barracks = k[&BARRACKS];
    assert_eq!(
        (barracks.kind, barracks.owner, barracks.anchor),
        (Kind(3), P1, Cell::new(30, 56))
    );
    // Seen at the latest think, which is tick 390 (the last one before the 400 run ends).
    assert_eq!(plant.seen_tick, Tick(390));
    // The construction centre, the refinery and the army were never in sight.
    for id in [CENTRE_1, REFINERY] {
        assert!(!k.contains_key(&id), "{id:?} is in the AI's memory");
    }
}

#[test]
fn a_structure_seen_gone_is_forgotten_and_one_out_of_sight_is_kept() {
    // The barracks is removed at tick 500 while the scout watches: forgotten at the next think. The scout
    // leaves at 520: the power plant is out of sight but stays in the memory with its old tick.
    let f = go(&scenario(), 800);
    let k = known(&f);
    assert_eq!(ids(k), vec![PLANT]);
    let seen = k[&PLANT].seen_tick;
    assert!(
        seen.0 >= 500 && seen.0 < 600,
        "last seen at {seen:?}: the scout walks away after tick 520"
    );
    // And it really is out of sight now, so the memory is all the AI has of it.
    assert!(!visible_ids(&observe(&f)).contains(&PLANT));
}

#[test]
fn a_structure_destroyed_out_of_sight_stays_in_memory() {
    // The power plant is removed at tick 700, long after the scout walked home at 520. The AI must not
    // learn it: `gone` is decided from the picture only, never from whether the entity is alive.
    let s = scenario();
    let mut removed = s.clone();
    removed.commands.push(TimedCommand {
        tick: 700,
        player: 1,
        command: Action::Despawn {
            ids: vec![IdSpec::Id(PLANT.index(), 0)],
        },
    });
    let with = go(&removed, 800);
    let without = go(&s, 800);
    // Control: the plant really is gone from the world.
    assert!(!with.world.is_alive(PLANT) && without.world.is_alive(PLANT));
    assert_eq!(ids(known(&with)), vec![PLANT]);
    assert_eq!(
        known(&with)[&PLANT].seen_tick,
        known(&without)[&PLANT].seen_tick
    );
    assert!(known(&with)[&PLANT].seen_tick.0 < 600);
    assert_eq!(with.ais[0].digest(), without.ais[0].digest());
}

#[test]
fn without_the_scout_the_ai_knows_nothing_of_the_enemy() {
    let f = go(&without_the_scout(scenario()), 800);
    assert!(known(&f).is_empty());
    assert!(visible_ids(&observe(&f)).is_empty());
    // Control: the enemy structures are in the world.
    assert!(f.world.is_alive(PLANT) && f.world.is_alive(REFINERY));
}

#[test]
fn the_observation_never_holds_what_player_0_has_not_seen() {
    let s = scenario();
    // Before the scout arrives, while it is there, and after the barracks is gone.
    let seen_at = |ticks: u64| visible_ids(&observe(&go(&s, ticks)));
    assert_eq!(seen_at(100), vec![]);
    assert_eq!(seen_at(400), vec![PLANT, BARRACKS]);
    assert_eq!(seen_at(510), vec![PLANT]);
    for ticks in [1, 100, 250, 400, 510, 800] {
        let visible = seen_at(ticks);
        for hidden in [CENTRE_1, REFINERY, ARMY[0], ARMY[1], ARMY[2]] {
            assert!(
                !visible.contains(&hidden),
                "tick {ticks}: {hidden:?} is in shroud but in the observation"
            );
        }
    }
}

#[test]
fn the_observation_holds_the_players_own_things_in_full() {
    let f = go(&scenario(), 400);
    let o = observe(&f);
    assert_eq!((o.player, o.tick, o.map), (P0, Tick(400), (96, 96)));
    // Since WP9.1b-1 the AI builds: a power plant (slot 9) and a refinery going up (slot 10).
    assert_eq!(o.credits, f.world.credits(P0));
    assert!(o.credits < 1500);
    assert_eq!(
        o.own
            .iter()
            .map(|e| (e.id.index(), e.kind.0))
            .collect::<Vec<_>>(),
        [(0, 0), (1, 15), (9, 1), (10, 2)]
    );
    let refinery = &o.own[3];
    assert!(!refinery.standing);
    assert_eq!(refinery.building, f.world.construction_of(refinery.id));
    assert_eq!(o.own[2].max_hp, 600);
    let centre = &o.own[0];
    assert_eq!((centre.kind, centre.cell), (Kind(0), Cell::new(8, 80)));
    assert!(centre.standing && centre.footprint.is_some() && centre.hp > 0);
    assert!(o.own[1].footprint.is_none() && o.own[1].id == SCOUT);
    assert_eq!(o.power, f.world.power_of(P0));
}

#[test]
fn an_invisible_army_changes_nothing_the_ai_sees_or_does() {
    let s = scenario();
    let with = go(&s, 800);
    let without = go(&without_the_army(s), 800);
    // Control: the worlds do differ.
    assert_ne!(with.world.checksum(), without.world.checksum());
    assert_eq!(with.world.len(), without.world.len() + 3);
    // The AI cannot tell: same observation, same memory, same commands, same digest. The handles of the
    // structures it builds are other slots in the world with the army (which took slots 6 to 8 first);
    // ids are opaque (docs/rules/ai.md), so they are compared by their order.
    assert_eq!(
        with_own_ids_in_order(observe(&with)),
        with_own_ids_in_order(observe(&without))
    );
    assert_eq!(with.ai_commands, without.ai_commands);
    assert!(!with.ai_commands.is_empty());
    assert_eq!(known(&with), known(&without));
    assert_eq!(with.ais[0].digest(), without.ais[0].digest());
    assert_eq!(with.outcome.ai_digests, without.outcome.ai_digests);
}

/// `o` with the ids of the player's own entities replaced by their place in the list.
fn with_own_ids_in_order(mut o: Observation) -> Observation {
    for (i, e) in o.own.iter_mut().enumerate() {
        e.id = EntityId::new(i as u32, 0);
    }
    o
}

#[test]
fn a_visible_army_does_change_the_observation() {
    // The positive control of the test above: a unit of player 1 inside the scout's sight (7 cells away) is
    // seen. An engineer, and out of the pistol's reach, so that the scout does not shoot it first.
    let mut s = without_the_army(scenario());
    s.commands.push(TimedCommand {
        tick: 0,
        player: 1,
        command: Action::SpawnUnit {
            kind: 14,
            owner: 1,
            cell: (34, 55),
        },
    });
    let f = go(&s, 400);
    let o = observe(&f);
    let engineer = o
        .visible
        .iter()
        .find(|e| e.kind == Kind(14))
        .expect("an engineer next to the scout is seen");
    assert_eq!((engineer.owner, engineer.footprint), (P1, None));
    assert!(engineer.hp > 0);
}

#[test]
fn the_ai_thinks_every_ten_ticks_one_manager_each() {
    let f = go(&scenario(), 800);
    let state = f.ais[0].state();
    // Ticks 0, 10, ..., 790.
    assert_eq!(state.thinks, 80);
    assert_eq!(state.last_think, Some(Tick(790)));
    // One manager per think, in rotation: every manager 20 times.
    assert_eq!(state.manager_runs, [20, 20, 20, 20]);
}

#[test]
fn two_ais_think_on_different_ticks() {
    let mut s = scenario();
    let defs = UnitDefs::embedded();
    let profile = sf_ai::Profile::resolve(&AiProfile::embedded(), Faction::Veyran, &defs).unwrap();
    let sites = s.ais[0].state().start_sites.clone();
    s.ais.push(Ai::new(P1, profile, 7, sites));
    let f = go(&s, 800);
    let (a, b) = (f.ais[0].state(), f.ais[1].state());
    assert_eq!((a.player, b.player), (P0, P1));
    // Player 0 at tick % 10 == 0, player 1 at tick % 10 == 7 ((t + 3) % 10 == 0): the last thinks of the
    // 800 ticks are 790 and 797, and each has thought 80 times.
    assert_eq!((a.thinks, a.last_think), (80, Some(Tick(790))));
    assert_eq!((b.thinks, b.last_think), (80, Some(Tick(797))));
}

#[test]
fn the_ai_sends_its_two_builds_and_nothing_else() {
    // WP9.1b-1: with 1500 credits the AI builds its power plant and its refinery (an emergency: none
    // stands), then has nothing left for the barracks.
    let s = scenario();
    let f = go(&s, s.ticks);
    let o = &f.outcome;
    assert_eq!(
        f.ai_commands,
        vec![
            (
                10,
                0,
                Command::BuildStructure {
                    kind: Kind(1),
                    cell: Cell::new(5, 80)
                }
            ),
            (
                330,
                0,
                Command::BuildStructure {
                    kind: Kind(2),
                    cell: Cell::new(13, 80)
                }
            ),
        ]
    );
    // Everything else the world ran was scheduled by the scenario.
    assert_eq!(
        o.commands_executed + o.commands_ignored,
        o.commands_scheduled as u64 + f.ai_commands.len() as u64
    );
    assert_eq!(o.commands_ignored, 0);
}

#[test]
fn the_scenario_makes_the_ai_of_its_faction_from_the_frozen_profile() {
    let s = scenario();
    assert_eq!(s.ais.len(), 1);
    let state = s.ais[0].state();
    assert_eq!(state.player, P0);
    assert_eq!(state.profile.name, "normal");
    // The start sites of the match file: both construction centres.
    assert_eq!(
        state.start_sites,
        vec![(P0, Cell::new(8, 80)), (P1, Cell::new(80, 8))]
    );
    // The Meridian section: kinds 1 power_plant, 2 refinery, 3 barracks, 4 vehicle_factory.
    let order: Vec<(u16, u16)> = state
        .profile
        .build_order
        .iter()
        .map(|&(k, n)| (k.0, n))
        .collect();
    assert_eq!(
        order,
        vec![
            (1, 1),
            (2, 1),
            (3, 1),
            (1, 2),
            (4, 1),
            (6, 1),
            (2, 2),
            (5, 1),
            (6, 2),
            (7, 1)
        ]
    );
}
