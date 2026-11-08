//! Shroud and fog of war (WP8.1) as the `vision` scenario shows it, with its checksum pinned. See
//! `assets/scenarios/vision.ron` and `docs/rules/vision.md`.
//!
//! Two players on a 64 by 64 open map. Player 0 has a defence tower (slot 0), a scout (1) that walks
//! east from tick 1 and back west from tick 700, and a light tank (2) that shoots the relay
//! transmitter of player 1 (slot 3, dead in the step of tick 1021) and then, from tick 1200, its
//! rifleman (slot 4, dead in the step of tick 1407). A second rifleman (slot 5) stands in the far
//! corner, where player 0 never looks.
//!
//! The ticks below are the tick that ran (`World::tick()` after the step). A death is picked up by the
//! vision pass of the next tick, so a cell loses its sight one tick after the entity is gone.

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::run::{RunOptions, run};
use sf_headless::scenario::{Scenario, VisionSpec};
use sf_sim::{Cell, EntityId, PlayerId, Visibility, World};

const EXPECTED: &str = include_str!("../../../assets/scenarios/vision.expected");

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const RELAY: EntityId = EntityId::new(3, 0);
const RIFLEMAN: EntityId = EntityId::new(4, 0);

/// A cell in the scout's corridor, far from everything at the start.
const CORRIDOR: Cell = Cell::new(30, 32);
/// A cell of the relay's footprint corner area, seen by player 1 only through the relay.
const BY_RELAY: Cell = Cell::new(44, 34);
/// A cell of the relay itself.
const RELAY_CELL: Cell = Cell::new(41, 35);
/// Where the second rifleman stands, in the far corner.
const CORNER: Cell = Cell::new(56, 52);
/// Where the first rifleman stands.
const RIFLEMAN_CELL: Cell = Cell::new(44, 26);

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("vision.ron")).unwrap()
}

fn see(w: &World, p: PlayerId, c: Cell) -> Visibility {
    w.visibility(p, c)
}

fn explored(w: &World, p: PlayerId) -> u32 {
    w.fog_words(p)
        .map_or(0, |(_, e)| e.iter().map(|x| x.count_ones()).sum())
}

fn visible(w: &World, p: PlayerId) -> u32 {
    w.fog_words(p)
        .map_or(0, |(v, _)| v.iter().map(|x| x.count_ones()).sum())
}

/// What a world shows of its entities, for comparing two runs.
fn picture(w: &World) -> Vec<(u32, u32, i64, i64, u32)> {
    w.snapshot()
        .iter()
        .map(|v| {
            (
                v.id.index(),
                v.id.generation(),
                v.pos.x.raw(),
                v.pos.y.raw(),
                v.hp,
            )
        })
        .collect()
}

/// `(tick, player 0's state, player 1's state)` of a probe cell when either changed.
type Change = (u64, Visibility, Visibility);

/// What a run showed.
struct Story {
    /// `(tick, p0, p1)` of each probe cell whenever it changed, per cell.
    changes: Vec<(Cell, Vec<Change>)>,
    gone: Vec<(u64, EntityId)>,
    /// Explored cells and visible cells of both players after each tick.
    explored: Vec<[u32; 2]>,
    visible: Vec<[u32; 2]>,
    /// `(tick, player)` whenever a cell that was explored the tick before no longer is: the sets are
    /// compared, not only their sizes.
    lost: Vec<(u64, usize)>,
}

fn play() -> (World, Story) {
    let s = scenario();
    let mut w = s.build_world().unwrap();
    let probes = [CORRIDOR, BY_RELAY, RELAY_CELL, CORNER, RIFLEMAN_CELL];
    let mut story = Story {
        changes: probes.iter().map(|&c| (c, Vec::new())).collect(),
        gone: Vec::new(),
        explored: Vec::new(),
        visible: Vec::new(),
        lost: Vec::new(),
    };
    let mut before: [Vec<u64>; 2] = [Vec::new(), Vec::new()];
    let mut last = vec![(Visibility::Shroud, Visibility::Shroud); probes.len()];
    let mut dead = [false; 6];
    for _ in 0..s.ticks {
        w.step();
        let t = w.tick().0;
        for (k, &c) in probes.iter().enumerate() {
            let now = (see(&w, P0, c), see(&w, P1, c));
            if now != last[k] {
                story.changes[k].1.push((t, now.0, now.1));
                last[k] = now;
            }
        }
        for i in 0..6u32 {
            let id = EntityId::new(i, 0);
            if !dead[i as usize] && w.hp_of(id).is_none() {
                dead[i as usize] = true;
                story.gone.push((t, id));
            }
        }
        for (p, prev) in before.iter_mut().enumerate() {
            let now = w
                .fog_words(PlayerId(p as u8))
                .map_or(Vec::new(), |(_, e)| e.to_vec());
            if prev.iter().zip(&now).any(|(a, b)| a & !b != 0) {
                story.lost.push((t, p));
            }
            *prev = now;
        }
        story.explored.push([explored(&w, P0), explored(&w, P1)]);
        story.visible.push([visible(&w, P0), visible(&w, P1)]);
    }
    (w, story)
}

fn changes_of(story: &Story, c: Cell) -> &[Change] {
    &story.changes.iter().find(|(x, _)| *x == c).unwrap().1
}

use Visibility::{Fog, Shroud, Visible};

#[test]
fn the_final_checksum_is_the_pinned_value() {
    let expected = parse_checksum(EXPECTED).unwrap();
    let s = scenario();
    let outcome = run(&s, &RunOptions::for_scenario(&s)).unwrap();
    assert_eq!(outcome.final_checksum, expected);
}

#[test]
fn the_scout_turns_shroud_into_visible_and_then_fog() {
    let (_, story) = play();
    // The corridor cell: shroud at the start, in the scout's sight from tick 175 (the scout is 8
    // cells away), out of it from tick 922 on the way back, and for a moment in sight again while the
    // tank passes (1202 to 1212). Player 1 never sees it.
    assert_eq!(
        changes_of(&story, CORRIDOR),
        &[
            (175, Visible, Shroud),
            (922, Fog, Shroud),
            (1202, Visible, Shroud),
            (1213, Fog, Shroud),
        ]
    );
}

#[test]
fn a_structure_shot_down_leaves_fog_for_its_owner_and_the_attacker_keeps_what_it_saw() {
    let (_, story) = play();
    assert_eq!(story.gone[0], (1021, RELAY));
    // The relay's own cell: player 1 sees it from the first tick; player 0 first at tick 425 (the
    // scout is close enough), then no more once the scout is back (fog from 737), and again while
    // the tank walks by to its next target. Player 1 loses it one tick after the relay is gone.
    assert_eq!(
        changes_of(&story, RELAY_CELL),
        &[
            (1, Shroud, Visible),
            (425, Visible, Visible),
            (737, Fog, Visible),
            (1022, Fog, Fog),
            (1202, Visible, Fog),
        ]
    );
    // A cell only the relay of player 1 watched: shroud for player 0 until the tank gets near
    // (1256), fog for player 1 from the tick after the relay is gone, for good.
    assert_eq!(
        changes_of(&story, BY_RELAY),
        &[
            (1, Shroud, Visible),
            (1022, Shroud, Fog),
            (1256, Visible, Fog)
        ]
    );
}

#[test]
fn a_unit_that_dies_takes_its_sight_with_it() {
    let (_, story) = play();
    assert_eq!(story.gone[1], (1407, RIFLEMAN));
    assert_eq!(
        changes_of(&story, RIFLEMAN_CELL),
        &[
            (1, Shroud, Visible),
            (1272, Visible, Visible),
            (1408, Visible, Fog),
        ]
    );
}

#[test]
fn a_corner_nobody_looks_into_stays_shroud_for_the_other_player() {
    let (w, story) = play();
    assert_eq!(changes_of(&story, CORNER), &[(1, Shroud, Visible)]);
    assert_eq!(see(&w, P0, CORNER), Shroud);
    assert_eq!(see(&w, P1, CORNER), Visible);
}

#[test]
fn explored_never_shrinks_and_visible_cells_come_and_go() {
    let (_, story) = play();
    assert!(
        story.lost.is_empty(),
        "explored cells were lost: {:?}",
        story.lost
    );
    for p in 0..2 {
        let mut last = 0;
        for (i, e) in story.explored.iter().enumerate() {
            assert!(
                e[p] >= last,
                "player {p} lost explored cells at tick {}",
                i + 1
            );
            last = e[p];
        }
    }
    // Visible cells do shrink (player 0 when the scout turns back, player 1 when its units die).
    assert!(story.visible.windows(2).any(|w| w[1][0] < w[0][0]));
    assert!(story.visible.windows(2).any(|w| w[1][1] < w[0][1]));
    // Every visible cell is explored, at every tick.
    for (v, e) in story.visible.iter().zip(&story.explored) {
        assert!(v[0] <= e[0] && v[1] <= e[1]);
    }
    // The end: what each player knows, counted from the run.
    assert_eq!(story.visible.last().unwrap(), &[459, 81]);
    assert_eq!(story.explored.last().unwrap(), &[1010, 259]);
}

#[test]
fn the_incremental_state_equals_a_recompute_every_tick() {
    let s = scenario();
    let mut w = s.build_world().unwrap();
    for _ in 0..s.ticks {
        w.step();
        let t = w.tick().0;
        w.vision_audit().unwrap_or_else(|e| panic!("tick {t}: {e}"));
        w.vision_audit_current()
            .unwrap_or_else(|e| panic!("tick {t}, from the entities: {e}"));
    }
}

#[test]
fn vision_changes_nothing_but_the_hash() {
    let with = scenario();
    let mut without = scenario();
    without.vision = None;
    let (mut a, mut b) = (with.build_world().unwrap(), without.build_world().unwrap());
    assert!(a.vision_rules().is_some());
    assert!(b.vision_rules().is_none());
    for _ in 0..with.ticks {
        a.step();
        b.step();
        assert_eq!(picture(&a), picture(&b), "tick {}", a.tick().0);
    }
    assert_ne!(a.checksum(), b.checksum());
    // Without vision everything is visible, to everyone, and there is no section.
    assert_eq!(see(&b, P0, CORNER), Visible);
    assert_eq!(b.vision_digest(), None);
}

#[test]
fn a_different_legacy_sight_changes_the_hash_and_a_different_scout_sight_changes_the_picture() {
    let base = scenario();
    let mut a = base.build_world().unwrap();
    let mut other = scenario();
    other.vision = Some(VisionSpec {
        legacy_sight_milli: 1000,
        ..VisionSpec::default()
    });
    let mut b = other.build_world().unwrap();
    for _ in 0..50 {
        a.step();
        b.step();
    }
    // Nothing here has a legacy sight, so the picture is the same; the rules are hashed.
    assert_eq!(a.vision_digest().is_some(), b.vision_digest().is_some());
    assert_ne!(a.vision_digest(), b.vision_digest());
    assert_eq!(a.fog_words(P0), b.fog_words(P0));
    // A scout that sees 3 instead of 8 sees less.
    let mut c = scenario();
    for (kind, sight) in &mut c.vision_sights {
        if kind.0 == 15 {
            *sight = 3000;
        }
    }
    let mut c = c.build_world().unwrap();
    for _ in 0..50 {
        c.step();
    }
    assert!(visible(&c, P0) < visible(&a, P0));
}

/// The same scenario, run through the runner twice: the checksums agree.
#[test]
fn two_runs_agree() {
    let s = scenario();
    let o = RunOptions::for_scenario(&s);
    assert_eq!(
        run(&s, &o).unwrap().final_checksum,
        run(&s, &o).unwrap().final_checksum
    );
}

/// The first match (`match.ron`, 96 by 96 cells, two players, an economy, a fight) played with vision on
/// is the same match: the same entities in the same places at every tick we look at, the same
/// verdict. Only the hash differs (the section `v`). Since WP8.2a the file has the rules itself (as
/// `sf_game --match` does); the "off" world is the same file with the section taken out, which is
/// the value `match.expected` had before WP8.2a (`match_m1.rs`).
#[test]
fn the_first_match_plays_the_same_with_vision_on() {
    let mut off = Scenario::load(&scenario_path("match.ron")).unwrap();
    let on = Scenario::load(&scenario_path("match.ron")).unwrap();
    assert!(on.vision.is_some(), "match.ron has the vision rules");
    off.vision = None;
    let (mut a, mut b) = (off.build_world().unwrap(), on.build_world().unwrap());
    assert!(a.vision_rules().is_none() && b.vision_rules().is_some());
    for _ in 0..off.ticks {
        a.step();
        b.step();
        let t = a.tick().0;
        if t % 10 == 0 {
            assert_eq!(picture(&a), picture(&b), "tick {t}");
        }
        assert_eq!(a.match_result(), b.match_result(), "tick {t}");
    }
    assert_eq!(picture(&a), picture(&b));
    assert_eq!(a.defeats(), b.defeats());
    assert!(a.match_result().is_some(), "the match was decided");
    assert_ne!(a.checksum(), b.checksum());
    b.vision_audit().unwrap();
    // Both sides saw something, and the human saw more than its starting ground.
    assert!(visible(&b, P0) > 0 && explored(&b, P0) > visible(&b, P0));
    assert!(explored(&b, P1) > 0);
}

/// A crowd of 5,000 wanderers with a legacy sight: the incremental state equals a recompute (from the
/// stamps and from the entities) at sample ticks, and the picture does not change the crowd.
#[test]
fn a_crowd_of_wanderers_keeps_its_counts_exact() {
    let base = Scenario::load(&scenario_path("wander_5000.ron")).unwrap();
    let mut on = Scenario::load(&scenario_path("wander_5000.ron")).unwrap();
    on.vision = Some(VisionSpec {
        legacy_sight_milli: 5000,
        ..VisionSpec::default()
    });
    let (mut a, mut b) = (base.build_world().unwrap(), on.build_world().unwrap());
    for _ in 0..300 {
        a.step();
        b.step();
        let t = b.tick().0;
        if t % 50 == 0 {
            assert_eq!(picture(&a), picture(&b), "tick {t}");
            b.vision_audit().unwrap_or_else(|e| panic!("tick {t}: {e}"));
            b.vision_audit_current()
                .unwrap_or_else(|e| panic!("tick {t}, from the entities: {e}"));
        }
    }
    // Four owners, each with its own disc count: 1,250 wanderers or so each.
    let total: u32 = (0..4).map(|p| b.vision_discs(PlayerId(p))).sum();
    assert_eq!(total, 5000);
}
