//! Vision (WP8.1): the discs, what stamps what, clipping, the transitions shroud / visible / fog,
//! the incremental bookkeeping against a recompute from scratch, the cap, and the hash. Inside the
//! crate because the audit reads the stamps and a few tests set a column directly. Every kind has
//! its own sight and every player its own cells, so that a swapped mapping shows.

use sf_math::{Fx, Vec2};

use super::*;
use crate::command::Command;
use crate::ids::{EntityId, Kind, PlayerId};
use crate::terrain::Cell;
use crate::units::{Footprint, UnitStats, UnitTable};
use crate::world::{World, WorldConfig};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const SCOUT: Kind = Kind(1); // mobile, sight 3
const RUNNER: Kind = Kind(2); // mobile, sight 5, fast
const BLIND: Kind = Kind(3); // mobile, sight 0
const HALL: Kind = Kind(4); // structure 2 x 2, sight 3
const TOWER: Kind = Kind(5); // structure 1 x 1, sight 10
const BIG: Kind = Kind(6); // structure 4 x 4, sight 4500 thousandths

const SIGHTS: [(Kind, u32); 6] = [
    (SCOUT, 3000),
    (RUNNER, 5000),
    (BLIND, 0),
    (HALL, 3000),
    (TOWER, 10_000),
    (BIG, 4500),
];

fn stats(kind: Kind, structure: Option<(u8, u8)>, fast: bool) -> UnitStats {
    UnitStats {
        kind,
        hp: 100,
        movement_class: structure.is_none().then_some(1),
        speed_per_tick: if structure.is_none() {
            Fx::from_ratio(1, if fast { 2 } else { 4 })
        } else {
            Fx::ZERO
        },
        turn_per_tick: if structure.is_none() { 16 } else { 0 },
        size: if structure.is_none() {
            Fx::from_ratio(4, 10)
        } else {
            Fx::ZERO
        },
        footprint: structure.map(|(width, height)| Footprint { width, height }),
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    }
}

fn table() -> UnitTable {
    UnitTable::new(vec![
        stats(SCOUT, None, false),
        stats(RUNNER, None, true),
        stats(BLIND, None, false),
        stats(HALL, Some((2, 2)), false),
        stats(TOWER, Some((1, 1)), false),
        stats(BIG, Some((4, 4)), false),
    ])
    .unwrap()
}

fn rules() -> VisionRules {
    VisionRules::new(2000, SIGHTS.to_vec())
}

fn plain(side: u32) -> World {
    World::new(WorldConfig::new(7).with_map_size(side, side))
        .unwrap()
        .with_units(table())
        .unwrap()
}

fn world(side: u32) -> World {
    plain(side).with_vision(rules()).unwrap()
}

fn at(x: i32, y: i32) -> Vec2 {
    // The middle of the cell.
    Vec2::new(
        Fx::from_int(x) + Fx::from_ratio(1, 2),
        Fx::from_int(y) + Fx::from_ratio(1, 2),
    )
}

fn schedule(w: &mut World, owner: PlayerId, c: Command) {
    let t = w.tick();
    w.schedule(t, owner, c).unwrap();
}

fn unit(w: &mut World, owner: PlayerId, kind: Kind, x: i32, y: i32) -> EntityId {
    schedule(
        w,
        owner,
        Command::SpawnUnit {
            kind,
            pos: at(x, y),
            owner,
        },
    );
    w.step();
    w.snapshot().iter().last().unwrap().id
}

fn structure(w: &mut World, owner: PlayerId, kind: Kind, x: u32, y: u32) -> EntityId {
    schedule(
        w,
        owner,
        Command::PlaceStructure {
            kind,
            cell: Cell::new(x, y),
            owner,
        },
    );
    w.step();
    w.occupant_at(Cell::new(x, y)).unwrap()
}

fn seen(w: &World, p: PlayerId, x: u32, y: u32) -> Visibility {
    w.visibility(p, Cell::new(x, y))
}

fn count(w: &World, p: PlayerId, which: Visibility) -> usize {
    let (side_w, side_h) = w.map_size();
    let mut n = 0;
    for y in 0..side_h {
        for x in 0..side_w {
            if seen(w, p, x, y) == which {
                n += 1;
            }
        }
    }
    n
}

/// The cells of the disc of `sight_milli` round `(cx, cy)` that are inside a `side` square map,
/// counted from the definition (integer arithmetic, no table).
fn brute(sight_milli: i64, cx: i64, cy: i64, side: i64) -> usize {
    let mut n = 0;
    for y in 0..side {
        for x in 0..side {
            let (dx, dy) = (1000 * (x - cx), 1000 * (y - cy));
            if dx * dx + dy * dy <= sight_milli * sight_milli {
                n += 1;
            }
        }
    }
    n
}

// ---- the discs ----------------------------------------------------------------------------------

#[test]
fn the_cell_counts_of_the_discs_are_pinned() {
    // Counted by hand for 1 and 3 (row halves 1,0 and 3,2,2,0), brute-forced for the rest.
    for (sight, cells) in [
        (0, 0),
        (500, 1),
        (1000, 5),
        (1414, 5),
        (1415, 9),
        (3000, 29),
        (5000, 81),
        (10_000, 317),
    ] {
        assert_eq!(Mask::new(sight).cells(), cells, "sight {sight}");
        if sight > 0 {
            assert_eq!(
                brute(i64::from(sight), 50, 50, 101),
                cells as usize,
                "sight {sight} by definition"
            );
        }
    }
}

#[test]
fn the_edge_of_the_disc_is_inside() {
    // (3, 4) is exactly 5 cells away: 9 + 16 is 25, and so is 5 times 5
    assert!(Mask::new(5000).contains(3, 4));
    assert!(Mask::new(5000).contains(-4, -3));
    assert!(!Mask::new(4999).contains(3, 4));
    assert!(Mask::new(5000).contains(5, 0));
    assert!(!Mask::new(5000).contains(5, 1));
    assert!(!Mask::new(5000).contains(4, 4));
}

#[test]
fn a_disc_is_symmetric_in_both_axes_and_across_the_diagonal() {
    for sight in [500, 1000, 3000, 4500, 5000, 5500, 10_000] {
        let m = Mask::new(sight);
        for dy in -12..=12 {
            for dx in -12..=12 {
                let inside = m.contains(dx, dy);
                assert_eq!(inside, m.contains(-dx, dy), "sight {sight} ({dx},{dy})");
                assert_eq!(inside, m.contains(dx, -dy), "sight {sight} ({dx},{dy})");
                assert_eq!(inside, m.contains(dy, dx), "sight {sight} ({dx},{dy})");
            }
        }
    }
}

#[test]
fn sight_zero_has_no_cells_and_any_sight_above_zero_has_the_centre() {
    assert_eq!(Mask::new(0).cells(), 0);
    assert!(!Mask::new(0).contains(0, 0));
    assert!(Mask::new(1).contains(0, 0));
    assert_eq!(Mask::new(1).cells(), 1);
}

// ---- what is seen -------------------------------------------------------------------------------

#[test]
fn a_unit_sees_its_disc_and_no_other_player_does() {
    let mut w = world(40);
    unit(&mut w, P0, SCOUT, 20, 20);
    assert_eq!(count(&w, P0, Visibility::Visible), 29);
    assert_eq!(count(&w, P1, Visibility::Visible), 0);
    assert_eq!(seen(&w, P0, 23, 20), Visibility::Visible);
    assert_eq!(seen(&w, P0, 24, 20), Visibility::Shroud);
    assert_eq!(seen(&w, P0, 22, 22), Visibility::Visible); // 4 + 4 = 8 <= 9
    assert_eq!(seen(&w, P0, 23, 22), Visibility::Shroud); // 9 + 4 = 13 > 9
    assert_eq!(seen(&w, P1, 20, 20), Visibility::Shroud);
}

#[test]
fn each_kind_has_the_sight_of_its_own_entry() {
    // Distinct sights, distinct cell counts: a swap of two kinds shows.
    for (kind, sight) in [(SCOUT, 3000u32), (RUNNER, 5000)] {
        let mut w = world(40);
        unit(&mut w, P0, kind, 20, 20);
        assert_eq!(
            count(&w, P0, Visibility::Visible),
            Mask::new(sight).cells() as usize
        );
    }
    let mut w = world(60);
    structure(&mut w, P1, TOWER, 30, 30);
    // A tower of one cell: the full disc of 10 cells
    assert_eq!(count(&w, P1, Visibility::Visible), 317);
}

#[test]
fn a_unit_that_sees_nothing_still_knows_its_own_cell() {
    let mut w = world(40);
    unit(&mut w, P0, BLIND, 10, 10);
    assert_eq!(count(&w, P0, Visibility::Visible), 0);
    assert_eq!(seen(&w, P0, 10, 10), Visibility::Fog);
    assert_eq!(count(&w, P0, Visibility::Fog), 1);
    assert_eq!(seen(&w, P0, 11, 10), Visibility::Shroud);
}

#[test]
fn a_structure_sees_from_its_whole_footprint() {
    let mut w = world(40);
    structure(&mut w, P0, HALL, 10, 10); // cells 10..=11 by 10..=11, sight 3
    // Along the middle rows the reach is 3 beyond the footprint: columns 7 to 14 in all
    assert_eq!(seen(&w, P0, 7, 10), Visibility::Visible);
    assert_eq!(seen(&w, P0, 14, 11), Visibility::Visible);
    assert_eq!(seen(&w, P0, 6, 10), Visibility::Shroud);
    assert_eq!(seen(&w, P0, 15, 11), Visibility::Shroud);
    assert_eq!(seen(&w, P0, 10, 7), Visibility::Visible);
    assert_eq!(seen(&w, P0, 11, 14), Visibility::Visible);
    assert_eq!(seen(&w, P0, 10, 6), Visibility::Shroud);
    // A corner: two cells beyond in both directions is inside (4 + 4 <= 9), three is not.
    assert_eq!(seen(&w, P0, 13, 13), Visibility::Visible);
    assert_eq!(seen(&w, P0, 14, 14), Visibility::Shroud);
    assert_eq!(seen(&w, P0, 8, 8), Visibility::Visible);
    assert_eq!(seen(&w, P0, 7, 7), Visibility::Shroud);
    // Counted: the two footprint rows are 2 + 2 * 3 = 8 wide; the three rows beyond, above and below,
    // have half-widths 2, 2 and 0 round the footprint of two: 6, 6 and 2 wide
    assert_eq!(count(&w, P0, Visibility::Visible), 2 * 8 + 2 * (6 + 6 + 2));
}

#[test]
fn a_big_structure_is_not_a_disc_round_its_anchor() {
    let mut w = world(40);
    structure(&mut w, P0, BIG, 10, 10); // 4 x 4, sight 4500 thousandths
    // Four cells beyond the far edge are in; from the anchor that would be 7 away.
    assert_eq!(seen(&w, P0, 17, 12), Visibility::Visible);
    assert_eq!(seen(&w, P0, 18, 12), Visibility::Shroud);
    assert_eq!(seen(&w, P0, 6, 11), Visibility::Visible);
    assert_eq!(seen(&w, P0, 5, 11), Visibility::Shroud);
}

#[test]
fn a_structure_under_construction_sees_nothing_but_knows_its_footprint() {
    let mut w = world(40);
    let id = structure(&mut w, P0, HALL, 10, 10);
    assert_eq!(count(&w, P0, Visibility::Visible), 2 * 8 + 2 * 14);
    let i = id.index() as usize;
    w.entities.build_left[i] = 50;
    w.step();
    assert_eq!(count(&w, P0, Visibility::Visible), 0);
    assert_eq!(count(&w, P0, Visibility::Fog), 2 * 8 + 2 * 14);
    assert_eq!(w.vision_discs(P0), 0);
    w.entities.build_left[i] = 0;
    w.step();
    assert_eq!(count(&w, P0, Visibility::Visible), 2 * 8 + 2 * 14);
    assert_eq!(w.vision_discs(P0), 1);
}

#[test]
fn a_structure_being_sold_stops_seeing() {
    let mut w = world(40);
    let id = structure(&mut w, P0, HALL, 10, 10);
    w.entities.sell_left[id.index() as usize] = 30;
    w.step();
    assert_eq!(count(&w, P0, Visibility::Visible), 0);
}

#[test]
fn a_legacy_wanderer_sees_the_legacy_sight_and_a_legacy_structure_only_knows_its_cell() {
    let mut w = world(40);
    schedule(
        &mut w,
        P0,
        Command::Spawn {
            kind: Kind(77),
            pos: at(20, 20),
            owner: P0,
        },
    );
    schedule(
        &mut w,
        P1,
        Command::Place {
            kind: Kind(78),
            cell: Cell::new(5, 5),
            owner: P1,
        },
    );
    w.step();
    // Legacy sight 2: 13 cells (rows 2, 1, 1, ... = 5 + 2 * 3 + 2 * 1).
    assert_eq!(count(&w, P0, Visibility::Visible), 13);
    assert_eq!(count(&w, P1, Visibility::Visible), 0);
    assert_eq!(seen(&w, P1, 5, 5), Visibility::Fog);
}

// ---- clipping -----------------------------------------------------------------------------------

#[test]
fn a_disc_is_clipped_at_the_edges_and_corners() {
    for (x, y, sight, kind) in [
        (0, 0, 5000i64, RUNNER),
        (39, 39, 5000, RUNNER),
        (0, 39, 5000, RUNNER),
        (39, 0, 5000, RUNNER),
        (0, 20, 5000, RUNNER),
        (20, 39, 5000, RUNNER),
        (1, 1, 3000, SCOUT),
        (0, 0, 3000, SCOUT),
    ] {
        let mut w = world(40);
        unit(&mut w, P0, kind, x, y);
        assert_eq!(
            count(&w, P0, Visibility::Visible),
            brute(sight, i64::from(x), i64::from(y), 40),
            "({x},{y}) sight {sight}"
        );
        w.vision_audit().unwrap();
    }
    // A quarter of the disc of 3 with its axes: 4 + 3 + 3 + 1 cells
    let mut w = world(40);
    unit(&mut w, P0, SCOUT, 0, 0);
    assert_eq!(count(&w, P0, Visibility::Visible), 11);
}

#[test]
fn a_tower_beside_the_edge_does_not_wrap_to_the_other_side() {
    let mut w = world(40);
    structure(&mut w, P0, TOWER, 0, 20);
    for y in 0..40 {
        // Column 39 is far from sight 10 and must be untouched.
        assert_eq!(seen(&w, P0, 39, y), Visibility::Shroud);
    }
    assert_eq!(seen(&w, P0, 10, 20), Visibility::Visible);
    assert_eq!(seen(&w, P0, 0, 10), Visibility::Visible);
}

// ---- shroud, visible, fog ----------------------------------------------------------------------

#[test]
fn a_unit_that_walks_away_leaves_fog_and_explored_never_shrinks() {
    let mut w = world(60);
    let id = unit(&mut w, P0, RUNNER, 10, 10);
    assert_eq!(seen(&w, P0, 10, 10), Visibility::Visible);
    schedule(
        &mut w,
        P0,
        Command::MoveTo {
            ids: vec![id],
            target: at(45, 10),
        },
    );
    let mut last_explored = 0u32;
    let mut was_fog = false;
    for _ in 0..400 {
        w.step();
        let (_, explored) = w.fog_words(P0).unwrap();
        let n: u32 = explored.iter().map(|x| x.count_ones()).sum();
        assert!(
            n >= last_explored,
            "explored shrank from {last_explored} to {n}"
        );
        last_explored = n;
        was_fog |= seen(&w, P0, 10, 10) == Visibility::Fog;
        w.vision_audit().unwrap();
    }
    assert!(was_fog, "the start cell never turned to fog");
    assert_eq!(seen(&w, P0, 10, 10), Visibility::Fog);
    assert_eq!(seen(&w, P0, 45, 10), Visibility::Visible);
    // Along the way: the whole corridor was explored, nothing above or below it.
    assert_eq!(seen(&w, P0, 30, 10), Visibility::Fog);
    assert_eq!(seen(&w, P0, 30, 17), Visibility::Shroud);
    assert_eq!(seen(&w, P0, 60 - 1, 59), Visibility::Shroud);
}

#[test]
fn a_dead_unit_takes_its_sight_away_and_the_explored_cells_stay() {
    let mut w = world(40);
    let a = unit(&mut w, P0, RUNNER, 10, 10);
    unit(&mut w, P0, SCOUT, 30, 30);
    assert_eq!(count(&w, P0, Visibility::Visible), 81 + 29);
    schedule(&mut w, P0, Command::Despawn { id: a });
    w.step();
    assert_eq!(count(&w, P0, Visibility::Visible), 29);
    assert_eq!(count(&w, P0, Visibility::Fog), 81);
    assert_eq!(w.vision_discs(P0), 1);
    w.vision_audit().unwrap();
}

#[test]
fn a_dying_structure_frees_its_cells_but_a_neighbour_keeps_the_overlap() {
    let mut w = world(40);
    let a = structure(&mut w, P0, TOWER, 10, 10);
    structure(&mut w, P0, TOWER, 14, 10);
    let both = count(&w, P0, Visibility::Visible);
    assert!(both > 317 && both < 2 * 317);
    schedule(&mut w, P0, Command::Despawn { id: a });
    w.step();
    // The overlap (visible to both) is still visible; what only the dead one saw is fog.
    assert_eq!(count(&w, P0, Visibility::Visible), 317);
    assert_eq!(seen(&w, P0, 12, 10), Visibility::Visible);
    assert_eq!(seen(&w, P0, 2, 10), Visibility::Fog); // only the dead one reached column 2
    assert_eq!(w.vision_discs(P0), 1);
    w.vision_audit().unwrap();
}

#[test]
fn a_slot_taken_over_in_the_same_tick_unstamps_the_old_owner_and_sight() {
    let mut w = world(40);
    let a = unit(&mut w, P0, RUNNER, 10, 10); // owner 0, sight 5
    // Despawn and spawn in one tick: the commands run in order, so the new unit takes the slot.
    schedule(&mut w, P0, Command::Despawn { id: a });
    schedule(
        &mut w,
        P1,
        Command::SpawnUnit {
            kind: SCOUT,
            pos: at(30, 30),
            owner: P1,
        },
    );
    w.step();
    let b = w.snapshot().iter().last().unwrap().id;
    assert_eq!(b.index(), a.index(), "the slot was reused");
    assert_eq!(
        count(&w, P0, Visibility::Visible),
        0,
        "player 0 still sees through the old unit"
    );
    assert_eq!(w.vision_discs(P0), 0);
    assert_eq!(count(&w, P1, Visibility::Visible), 29);
    assert_eq!(seen(&w, P1, 10, 10), Visibility::Shroud);
    assert_eq!(seen(&w, P0, 10, 10), Visibility::Fog);
    w.vision_audit().unwrap();
}

#[test]
fn the_fog_version_changes_only_when_the_picture_does() {
    let mut w = world(40);
    let id = unit(&mut w, P0, RUNNER, 10, 10);
    w.step();
    let v = w.fog_version(P0).unwrap();
    assert!(v > 0);
    for _ in 0..10 {
        w.step();
        assert_eq!(w.fog_version(P0), Some(v), "nothing moved");
    }
    schedule(
        &mut w,
        P0,
        Command::MoveTo {
            ids: vec![id],
            target: at(30, 10),
        },
    );
    for _ in 0..30 {
        w.step();
    }
    assert!(w.fog_version(P0).unwrap() > v);
    assert_eq!(w.fog_version(P1), Some(0));
    assert_eq!(plain(40).fog_version(P0), None);
}

// ---- a world without vision ---------------------------------------------------------------------

#[test]
fn a_world_without_vision_rules_sees_everything_and_hashes_as_a_plain_world() {
    let run = |with: bool| {
        let mut w = if with { world(40) } else { plain(40) };
        unit(&mut w, P0, SCOUT, 20, 20);
        for _ in 0..20 {
            w.step();
        }
        w
    };
    let off = run(false);
    let on = run(true);
    assert_eq!(off.vision_digest(), None);
    assert!(off.vision_rules().is_none());
    assert_eq!(off.fog_words(P0), None);
    assert_eq!(seen(&off, P1, 0, 0), Visibility::Visible);
    assert_ne!(
        off.checksum(),
        on.checksum(),
        "the section is written with rules"
    );
    // The same run twice: the plain one is itself.
    assert_eq!(off.checksum(), run(false).checksum());
    assert_eq!(on.checksum(), run(true).checksum());
}

#[test]
fn the_rules_are_refused_when_they_do_not_fit() {
    let mk = |sights: Vec<(Kind, u32)>, legacy| VisionRules::new(legacy, sights);
    let bad = |w: World, r: VisionRules| matches!(w.with_vision(r), Err(ConfigError::Vision));
    assert!(bad(plain(8), mk(vec![(Kind(99), 1000)], 0)), "unknown kind");
    assert!(
        bad(plain(8), mk(vec![(SCOUT, 1000), (SCOUT, 1000)], 0)),
        "twice"
    );
    assert!(
        bad(plain(8), mk(vec![(RUNNER, 1000), (SCOUT, 1000)], 0)),
        "order"
    );
    assert!(
        bad(plain(8), mk(vec![(SCOUT, MAX_SIGHT_MILLI + 1)], 0)),
        "too far"
    );
    assert!(
        bad(plain(8), mk(vec![], MAX_SIGHT_MILLI + 1)),
        "legacy too far"
    );
    assert!(bad(world(8), rules()), "given twice");
    // Without a unit table the rules may list no kinds (everything is legacy); one kind is unknown.
    let no_table = || World::new(WorldConfig::new(1).with_map_size(8, 8)).unwrap();
    assert!(no_table().with_vision(mk(vec![], 3000)).is_ok());
    assert!(
        bad(no_table(), mk(vec![(SCOUT, 1000)], 0)),
        "no table, a kind"
    );
    assert!(
        matches!(world(8).with_units(table()), Err(ConfigError::Vision)),
        "a table after the rules"
    );
    let mut started = plain(8);
    started.step();
    let started = started.with_vision(mk(vec![], 0));
    // A started world with no entities and an empty queue is still accepted by the other rules
    // only before the first tick: here the tick has run.
    assert!(
        matches!(started, Err(ConfigError::Vision)),
        "after the start"
    );
    assert!(
        plain(8)
            .with_vision(mk(vec![(SCOUT, MAX_SIGHT_MILLI)], MAX_SIGHT_MILLI))
            .is_ok()
    );
}

// ---- the bookkeeping against a recompute --------------------------------------------------------

/// A busy world: wanderers and table units on the move, structures going up and coming down, units
/// spawned and despawned. Every tick the counts are checked against a recompute from the stamps, and
/// on a clone against a recompute from the entities as they are.
#[test]
fn the_incremental_counts_equal_a_recompute_every_tick() {
    let mut w = world(48);
    let mut ids: Vec<EntityId> = Vec::new();
    for k in 0..12 {
        let owner = PlayerId((k % 3) as u8);
        schedule(
            &mut w,
            owner,
            Command::Spawn {
                kind: Kind(50 + k as u16),
                pos: at(4 + 3 * k, 5 + 2 * k),
                owner,
            },
        );
    }
    w.step();
    for k in 0..9 {
        let kind = [SCOUT, RUNNER, BLIND][k % 3];
        ids.push(unit(
            &mut w,
            PlayerId((k % 2) as u8),
            kind,
            3 + 4 * k as i32,
            40 - 3 * k as i32,
        ));
    }
    structure(&mut w, P0, HALL, 20, 20);
    let tower = structure(&mut w, P1, TOWER, 30, 10);
    let big = structure(&mut w, P1, BIG, 5, 30);
    schedule(
        &mut w,
        P0,
        Command::MoveTo {
            ids: ids.clone(),
            target: at(24, 24),
        },
    );
    for tick in 0..260u32 {
        match tick {
            20 => schedule(&mut w, P0, Command::Despawn { id: ids[1] }),
            40 => schedule(&mut w, P1, Command::Despawn { id: tower }),
            41 => {
                schedule(
                    &mut w,
                    P1,
                    Command::SpawnUnit {
                        kind: RUNNER,
                        pos: at(2, 2),
                        owner: P1,
                    },
                );
                schedule(
                    &mut w,
                    P0,
                    Command::SpawnUnit {
                        kind: SCOUT,
                        pos: at(44, 44),
                        owner: P0,
                    },
                );
            }
            60 => w.entities.build_left[big.index() as usize] = 40,
            100 => w.entities.build_left[big.index() as usize] = 0,
            120 => schedule(&mut w, P1, Command::Despawn { id: ids[4] }),
            121 => schedule(
                &mut w,
                P1,
                Command::SpawnUnit {
                    kind: TOWER,
                    pos: at(9, 9),
                    owner: P0,
                },
            ),
            150 => schedule(
                &mut w,
                P1,
                Command::MoveTo {
                    ids: ids.clone(),
                    target: at(3, 44),
                },
            ),
            _ => {}
        }
        w.step();
        w.vision_audit()
            .unwrap_or_else(|e| panic!("tick {tick}: {e}"));
        let mut c = w.clone();
        c.vision_resync();
        c.vision_audit_current()
            .unwrap_or_else(|e| panic!("tick {tick}, from the entities: {e}"));
    }
}

#[test]
fn the_picture_does_not_depend_on_the_order_the_units_were_made() {
    // Stationary units, made in three different orders: the slots differ, the arrays must not.
    let mut specs: Vec<(PlayerId, Kind, i32, i32)> = vec![
        (P0, SCOUT, 5, 5),
        (P0, RUNNER, 9, 7),
        (P1, SCOUT, 6, 6),
        (P1, RUNNER, 20, 20),
        (P0, BLIND, 12, 12),
        (P1, BLIND, 3, 30),
        (P0, SCOUT, 7, 5),
    ];
    let build = |specs: &[(PlayerId, Kind, i32, i32)]| {
        let mut w = world(40);
        for &(owner, kind, x, y) in specs {
            schedule(
                &mut w,
                owner,
                Command::SpawnUnit {
                    kind,
                    pos: at(x, y),
                    owner,
                },
            );
        }
        w.step();
        structure(&mut w, P0, HALL, 30, 30);
        w.step();
        w
    };
    let a = build(&specs);
    specs.reverse();
    let b = build(&specs);
    specs.rotate_left(3);
    let c = build(&specs);
    for other in [&b, &c] {
        for p in [P0, P1] {
            assert_eq!(a.fog_words(p), other.fog_words(p));
            assert_eq!(a.vision_discs(p), other.vision_discs(p));
        }
        assert_eq!(a.vision_digest(), other.vision_digest());
    }
    assert_eq!(
        count(&a, P0, Visibility::Visible),
        count(&b, P0, Visibility::Visible)
    );
    assert!(count(&a, P1, Visibility::Visible) > 0);
}

#[test]
fn different_pictures_have_different_digests() {
    let digest = |x: i32, owner: PlayerId, kind: Kind| {
        let mut w = world(40);
        unit(&mut w, owner, kind, x, 20);
        w.vision_digest().unwrap()
    };
    let base = digest(10, P0, SCOUT);
    assert_ne!(base, digest(11, P0, SCOUT), "a cell further");
    assert_ne!(base, digest(10, P1, SCOUT), "another player");
    assert_ne!(base, digest(10, P0, RUNNER), "another sight");
    assert_eq!(base, digest(10, P0, SCOUT));
}

#[test]
fn the_digest_follows_visible_and_explored_separately() {
    let mut w = world(40);
    let a = unit(&mut w, P0, SCOUT, 10, 10);
    let d1 = w.vision_digest().unwrap();
    schedule(&mut w, P0, Command::Despawn { id: a });
    w.step();
    let d2 = w.vision_digest().unwrap();
    assert_ne!(d1, d2, "the same cells, now fog");
    let b = unit(&mut w, P0, SCOUT, 10, 10);
    assert_ne!(b, a);
    assert_eq!(w.vision_digest().unwrap(), d1, "back to what it was");
}

// ---- the cap ------------------------------------------------------------------------------------

#[test]
fn a_player_cannot_have_more_discs_than_a_count_can_hold() {
    let mut v = Vision::new(8, 8, rules(), vec![3000, 5000, 0, 3000, 10_000, 4500]);
    let src = Source {
        owner: P0,
        x: 4,
        y: 4,
        w: 1,
        h: 1,
        sight_milli: 3000,
    };
    let n = MAX_DISCS_PER_PLAYER as usize + 5;
    for slot in 0..n {
        v.sync(slot, Some(src));
    }
    v.finish_pass();
    assert_eq!(v.discs(P0), MAX_DISCS_PER_PLAYER);
    assert_eq!(v.player(P0).unwrap().count[4 * 8 + 4], u16::MAX);
    v.audit().unwrap();
    // Removing ten makes room for the five that were refused, which get their disc on the next pass.
    for slot in 0..10 {
        v.sync(slot, None);
    }
    for slot in 10..n {
        v.sync(slot, Some(src));
    }
    v.finish_pass();
    assert_eq!(v.discs(P0), MAX_DISCS_PER_PLAYER - 10 + 5);
    assert_eq!(
        u32::from(v.player(P0).unwrap().count[4 * 8 + 4]),
        MAX_DISCS_PER_PLAYER - 5
    );
    v.audit().unwrap();
    // And everything taken away leaves nothing.
    for slot in 0..n {
        v.sync(slot, None);
    }
    v.finish_pass();
    assert_eq!(v.discs(P0), 0);
    assert!(v.player(P0).unwrap().count.iter().all(|&c| c == 0));
    assert_eq!(v.player(P0).unwrap().visible_cells, 0);
    v.audit().unwrap();
}
