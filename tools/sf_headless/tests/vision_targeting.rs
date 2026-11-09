//! The targeting gate and shared vision (WP8.2a) as the `vision_targeting` scenario shows them, with its
//! checksum pinned. See `assets/scenarios/vision_targeting.ron` and `docs/rules/vision.md`.
//!
//! Slots: 0 siege artillery of player 0 (reach 10, sight 6) with an enemy rifleman (3) 8 cells away;
//! 1 unmanned artillery of player 0 (a defence; reach 12, sight 6) with an enemy rifleman (4) 9 cells
//! from its lower row; 2 siege artillery of player 0 next to the relay transmitter of player 1 (5),
//! whose lower row is in its sight and whose anchor is not; 6 siege artillery of player 1 under an
//! explicit attack order on a light tank of player 0 (7) out of its own sight; 8 and 9 scouts of
//! player 2, which shares its sight with player 0 (one way) and walks up to the riflemen at ticks 300
//! and 600.
//!
//! The ticks are the tick that ran (`World::tick()` after the step).

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::run::{RunOptions, run};
use sf_headless::scenario::Scenario;
use sf_sim::{Cell, EntityId, PlayerId, Verdict, Visibility, World};

const EXPECTED: &str = include_str!("../../../assets/scenarios/vision_targeting.expected");

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);

fn id(slot: u32) -> EntityId {
    EntityId::new(slot, 0)
}

/// Where the enemy riflemen and player 1's artillery stand.
const RIFLEMAN_3: Cell = Cell::new(8, 16);
const RIFLEMAN_4: Cell = Cell::new(20, 50);
const ARTILLERY_6: Cell = Cell::new(40, 8);
/// The relay's footprint is (50..=51, 40..=41): the anchor, and the cell that is in sight.
const RELAY_ANCHOR: Cell = Cell::new(50, 40);
const RELAY_LOWER: Cell = Cell::new(50, 41);

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("vision_targeting.ron")).unwrap()
}

/// What a run showed of the targeting.
#[derive(Default)]
struct Story {
    /// `(tick, shooter slot, target slot)` of every shot.
    shots: Vec<(u64, u32, u32)>,
    /// The first tick each slot had an engagement (a unit that chose its target), by slot.
    engaged: [Option<u64>; 10],
    /// The first tick the defence (slot 1) had a turret target.
    defence_aimed: Option<u64>,
    /// The first tick player 0 saw the cell, in the world's answer and in its own bitset.
    seen: Vec<(Cell, Option<u64>, Option<u64>)>,
    /// The tick player 0's light tank (7) took its target, and what player 0 saw of it then.
    retaliated: Option<(u64, EntityId, Visibility)>,
    /// What player 1 saw of the tank at the tick player 1's artillery first fired at it.
    attacker_sight: Option<(u64, Visibility)>,
}

fn own_bit(w: &World, p: PlayerId, c: Cell) -> bool {
    let i = (c.y * 64 + c.x) as usize;
    w.fog_words(p)
        .is_some_and(|(v, _)| (v[i / 64] >> (i % 64)) & 1 == 1)
}

fn play(s: &Scenario) -> Story {
    let mut w = s.build_world().unwrap();
    let mut st = Story {
        seen: [
            RIFLEMAN_3,
            RIFLEMAN_4,
            RELAY_ANCHOR,
            RELAY_LOWER,
            ARTILLERY_6,
        ]
        .iter()
        .map(|&c| (c, None, None))
        .collect(),
        ..Story::default()
    };
    for _ in 0..s.ticks {
        w.step();
        let t = w.tick().0;
        for sh in w.shots() {
            st.shots.push((t, sh.attacker.index(), sh.target.index()));
            if sh.attacker == id(6) && st.attacker_sight.is_none() {
                st.attacker_sight = Some((t, w.visibility(P1, Cell::new(40, 17))));
            }
        }
        for slot in 0..10u32 {
            if st.engaged[slot as usize].is_none() && w.engagement_of(id(slot)).is_some() {
                st.engaged[slot as usize] = Some(t);
                if slot == 7 {
                    let e = w.engagement_of(id(7)).unwrap();
                    st.retaliated = Some((t, e.target, w.visibility(P0, ARTILLERY_6)));
                }
            }
        }
        if st.defence_aimed.is_none() && w.turret_target_of(id(1)).is_some() {
            st.defence_aimed = Some(t);
        }
        for (c, any, own) in &mut st.seen {
            if any.is_none() && w.visibility(P0, *c) == Visibility::Visible {
                *any = Some(t);
            }
            if own.is_none() && own_bit(&w, P0, *c) {
                *own = Some(t);
            }
        }
    }
    st
}

fn first_shot(st: &Story, shooter: u32) -> Option<(u64, u32)> {
    st.shots.iter().find(|s| s.1 == shooter).map(|s| (s.0, s.2))
}

fn with(f: impl FnOnce(&mut Scenario)) -> Scenario {
    let mut s = scenario();
    f(&mut s);
    s
}

fn seen_at(st: &Story, c: Cell) -> (Option<u64>, Option<u64>) {
    let e = st.seen.iter().find(|x| x.0 == c).unwrap();
    (e.1, e.2)
}

#[test]
fn the_final_checksum_is_the_pinned_value() {
    let expected = parse_checksum(EXPECTED).unwrap();
    let s = scenario();
    let outcome = run(&s, &RunOptions::for_scenario(&s)).unwrap();
    assert_eq!(outcome.final_checksum, expected);
}

/// The unit gate: the artillery (slot 0) has the rifleman (3) in reach from the start and does not
/// pick it; the scout of player 2 comes within sight of it at tick 539 and the artillery picks it at
/// the next scan (541) and fires at 546. Player 0's own bitset never shows the cell: the sight is
/// shared.
#[test]
fn a_unit_does_not_pick_what_it_cannot_see_until_a_friend_reveals_it() {
    let st = play(&scenario());
    assert_eq!(seen_at(&st, RIFLEMAN_3), (Some(539), None));
    assert_eq!(st.engaged[0], Some(541));
    assert_eq!(first_shot(&st, 0), Some((546, 3)));
    // Nothing of player 0's artillery fired at the rifleman before: not a single shot of slot 0.
    assert!(st.shots.iter().filter(|s| s.1 == 0).all(|s| s.0 >= 546));
}

/// The same file with the gate off: both artilleries pick their targets at the first scan.
#[test]
fn with_the_gate_off_the_same_units_fire_at_once() {
    let st = play(&with(|s| {
        s.vision.as_mut().unwrap().acquire_needs_sight = false
    }));
    assert_eq!(st.engaged[0], Some(1));
    assert_eq!(first_shot(&st, 0).map(|f| f.1), Some(3));
    assert!(first_shot(&st, 0).unwrap().0 <= 10);
    assert!(first_shot(&st, 1).unwrap().0 <= 10);
}

/// The defence gate: the unmanned artillery (slot 1) has the rifleman (4) in reach (12) from the
/// start and picks it only once the scout of player 2 reveals it at tick 654.
#[test]
fn a_defence_ignores_an_unseen_target_until_it_is_revealed() {
    let st = play(&scenario());
    assert_eq!(seen_at(&st, RIFLEMAN_4), (Some(654), None));
    assert!(
        st.defence_aimed.is_some_and(|t| t >= 654),
        "{:?}",
        st.defence_aimed
    );
    assert_eq!(first_shot(&st, 1), Some((661, 4)));
    assert!(st.shots.iter().filter(|s| s.1 == 1).all(|s| s.0 >= 661));
}

/// A structure is seen by any cell of its footprint: the relay's anchor is outside the sight of the
/// artillery (slot 2), its lower row inside, and the artillery picks it at the first scan.
#[test]
fn a_structure_target_is_seen_through_any_cell_of_its_footprint() {
    let st = play(&scenario());
    assert_eq!(seen_at(&st, RELAY_LOWER), (Some(1), Some(1)));
    assert_eq!(seen_at(&st, RELAY_ANCHOR), (None, None));
    assert_eq!(st.engaged[2], Some(1));
    assert_eq!(first_shot(&st, 2), Some((18, 5)));
}

/// Retaliation is never gated: the light tank (7) is shot by player 1's artillery from a cell it does
/// not see (the shell is fired at tick 19, it lands at 32) and takes its attacker at once. The shot
/// itself is an explicit attack order, which player 1 could not have picked itself either (it did
/// not see the tank), and was fired all the same.
#[test]
fn being_hit_from_the_fog_is_answered_and_an_attack_order_is_not_gated() {
    let st = play(&scenario());
    assert_eq!(first_shot(&st, 6), Some((19, 7)));
    assert_eq!(st.attacker_sight, Some((19, Visibility::Shroud)));
    assert_eq!(st.retaliated, Some((32, id(6), Visibility::Shroud)));
    // The attacker comes into the tank's sight only on the way (first seen at 65).
    assert_eq!(seen_at(&st, ARTILLERY_6), (Some(65), Some(65)));
}

/// Sharing is one way, from the sharer to the viewer: with the pair turned round, player 2 sees what
/// player 0 sees and player 0 gets nothing from player 2, so the artilleries stay silent for the
/// whole run. Without any pair the same.
#[test]
fn sharing_goes_from_the_sharer_to_the_viewer_only() {
    for shared in [vec![(0, 2)], vec![]] {
        let st = play(&with(|s| {
            s.vision.as_mut().unwrap().shared = shared.clone()
        }));
        assert_eq!(seen_at(&st, RIFLEMAN_3), (None, None), "{shared:?}");
        assert_eq!(seen_at(&st, RIFLEMAN_4), (None, None), "{shared:?}");
        assert!(first_shot(&st, 0).is_none(), "{shared:?}");
        assert!(first_shot(&st, 1).is_none(), "{shared:?}");
        // The rest of the scenario does not depend on it.
        assert_eq!(first_shot(&st, 2), Some((18, 5)));
        assert_eq!(first_shot(&st, 6), Some((19, 7)));
    }
}

/// The reverse pair shows on the other side: player 2 sees what player 0's artilleries see.
#[test]
fn the_viewer_sees_the_sharers_cells_in_fog_and_in_sight() {
    let s = with(|s| s.vision.as_mut().unwrap().shared = vec![(0, 2)]);
    let mut w = s.build_world().unwrap();
    for _ in 0..5 {
        w.step();
    }
    // The cell the artillery of player 0 stands on is its own and visible to it; player 2 gets it too.
    let c = Cell::new(8, 8);
    assert_eq!(w.visibility(P0, c), Visibility::Visible);
    assert_eq!(w.visibility(P2, c), Visibility::Visible);
    assert!(
        !own_bit(&w, P2, c),
        "shared sight is not stored in the viewer's own bits"
    );
    // Player 1 is not a viewer of anybody.
    assert_ne!(w.visibility(P1, c), Visibility::Visible);
}

/// The run is the same every time and the gate does not move the part of the story that has nothing
/// to do with it (slots 2 and 6 fire on the same ticks with the gate on and off, bar the one-tick
/// shift of the artillery under an order, which the gate-off run spends on a pick that the order
/// then replaces).
#[test]
fn the_gate_changes_only_what_it_gates() {
    let on = play(&scenario());
    let off = play(&with(|s| {
        s.vision.as_mut().unwrap().acquire_needs_sight = false
    }));
    assert_eq!(first_shot(&on, 2), first_shot(&off, 2));
    assert_eq!(
        first_shot(&on, 6).map(|f| f.1),
        first_shot(&off, 6).map(|f| f.1)
    );
    let again = play(&scenario());
    assert_eq!(on.shots, again.shots);
}

/// What the gate does to the first match (WP7.7: shipped on, with the sight of a target measured to its
/// edge). Measured with the new match file (the opponent has a construction centre, the match is
/// decided when it falls), three ways:
///
/// | | decided | units lost |
/// |---|---|---|
/// | gate off (this test, `acquire_needs_sight = false`) | 6228 | 6 |
/// | gate on, edge test (shipped) | 6238 | 6 |
/// | gate on, the old cell test (WP8.2a, measured while building WP7.7, no longer in the code) | 9189 | 8 |
///
/// The rocket trooper (slot 22, sight 5, reach 5) picks the opponent's defence tower (slot 4) at the
/// limit of its reach as soon as it would without the gate: the tower's cell is 26 (squared) from the
/// trooper's cell, outside the disc of 25, and the cell beside it is inside. With the cell test it did
/// not, and the worlds parted at 4282 (WP8.2a).
#[test]
fn the_gate_in_the_first_match_costs_ten_ticks_and_no_unit() {
    let load = |gate: bool| {
        let mut s = Scenario::load(&scenario_path("match.ron")).unwrap();
        assert!(
            s.vision.as_ref().unwrap().acquire_needs_sight,
            "shipped on (WP7.7)"
        );
        s.vision.as_mut().unwrap().acquire_needs_sight = gate;
        s.ticks = 9000;
        s
    };
    let tower = id(4);
    let trooper = id(22);
    // The tick the trooper first has the tower as its target, with and without the gate.
    let first_pick = |gate: bool| {
        let mut w = load(gate).build_world().unwrap();
        for _ in 0..5000 {
            w.step();
            if w.engagement_of(trooper).map(|e| e.target) == Some(tower) {
                return Some(w.tick().0);
            }
        }
        None
    };
    let (off, on) = (first_pick(false), first_pick(true));
    assert!(off.is_some());
    assert_eq!(on, off, "the gate does not hold the trooper back any more");

    // To the end of each match.
    let decided = |gate: bool| {
        let s = load(gate);
        let mut w = s.build_world().unwrap();
        let mut lost = 0;
        for _ in 0..s.ticks {
            w.step();
            lost += w.deaths().iter().filter(|d| d.owner == P0).count();
            if let Some(r) = w.match_result() {
                return (w.tick().0 - 1, r.verdict, lost); // the tick that ran
            }
        }
        panic!("not decided");
    };
    assert_eq!(decided(false), (6228, Verdict::Won(P0), 6));
    assert_eq!(decided(true), (6238, Verdict::Won(P0), 6));
}
