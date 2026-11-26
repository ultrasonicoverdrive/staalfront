use sf_data::ai_profile::AiProfile;
use sf_data::units::{Faction, UnitDefs};
use sf_sim::{Cell, PlayerId};

use super::*;
use crate::{AiState, Profile};

const W: u32 = 96;
const H: u32 = 96;

/// The half turn of a cell on a map of `W` by `H`.
fn turn(c: Cell) -> Cell {
    Cell::new(W - 1 - c.x, H - 1 - c.y)
}

/// The half turn of a doubled point.
fn turn2(p: (i64, i64)) -> (i64, i64) {
    (2 * i64::from(W) - p.0, 2 * i64::from(H) - p.1)
}

/// The half turn of a cell given as a pair.
fn turn_xy((x, y): (i64, i64)) -> (i64, i64) {
    (i64::from(W) - 1 - x, i64::from(H) - 1 - y)
}

/// The two start sites of a map that a half turn maps onto itself (the south-west and the north-east base).
fn sites() -> Vec<(PlayerId, Cell)> {
    let sw = Cell::new(12, 68);
    vec![(PlayerId(0), sw), (PlayerId(1), turn(sw))]
}

fn state(player: u8) -> AiState {
    let profile = Profile::resolve(
        &AiProfile::embedded(),
        Faction::Meridian,
        &UnitDefs::embedded(),
    )
    .unwrap();
    AiState::new(PlayerId(player), profile, 1, sites())
}

fn both() -> (Facing, Facing) {
    (
        Facing::of(&state(0), (W, H)).unwrap(),
        Facing::of(&state(1), (W, H)).unwrap(),
    )
}

#[test]
fn the_facing_of_the_two_seats_is_a_half_turn_of_each_other() {
    let (a, b) = both();
    assert_eq!(a.origin, (25, 137));
    assert_eq!(a.dir, (2 * 83 + 1 - 25, 2 * 27 + 1 - 137));
    assert_eq!(b.origin, turn2(a.origin));
    assert_eq!(b.dir, (-a.dir.0, -a.dir.1));
}

#[test]
fn no_start_site_or_a_target_on_it_is_no_facing() {
    let mut s = state(0);
    s.start_sites.clear();
    assert_eq!(Facing::of(&s, (W, H)), None);
    // Alone: the middle of the map is the target.
    let mut s = state(0);
    s.start_sites.truncate(1);
    let f = Facing::of(&s, (W, H)).unwrap();
    assert_eq!(f.dir, (i64::from(W) - 25, i64::from(H) - 137));
}

#[test]
fn the_rank_of_a_point_is_the_same_after_a_half_turn_and_never_ties() {
    let (a, b) = both();
    let mut seen = std::collections::BTreeSet::new();
    for y in 0..H {
        for x in 0..W {
            let c = Cell::new(x, y);
            let r = a.rank(centre(c));
            assert_eq!(r, b.rank(centre(turn(c))), "cell {x},{y}");
            assert!(seen.insert(r), "two cells rank {r:?}");
        }
    }
}

#[test]
fn the_rank_prefers_the_cell_toward_the_enemy() {
    let (a, _) = both();
    // From the south-west base the enemy lies north-east: a cell north-east of another ranks first.
    assert!(a.rank(centre(Cell::new(30, 60))) < a.rank(centre(Cell::new(20, 70))));
    // Equal dot, mirrored about the line to the enemy: the nearer the line first, here both equal, so by side.
    let (d, o) = (a.dir, a.origin);
    let left = (o.0 + d.0 + d.1, o.1 + d.1 - d.0);
    let right = (o.0 + d.0 - d.1, o.1 + d.1 + d.0);
    let (rl, rr) = (a.rank(left), a.rank(right));
    assert_eq!((rl.0, rl.1), (rr.0, rr.1));
    assert_ne!(rl.2, rr.2);
}

#[test]
fn the_gather_point_turns_with_the_map() {
    // Every anchor of 2 to 5 cells on a grid of places, every distance: the point of the half-turned anchor
    // is the half turn of the point.
    let map = (i64::from(W), i64::from(H));
    for fw in 2..=5 {
        for fh in 2..=5 {
            for ax in (3..80).step_by(7) {
                for ay in (3..80).step_by(5) {
                    for dist in 1..=12 {
                        let p = gather_point((ax, ay), (fw, fh), map, dist);
                        let q =
                            gather_point((map.0 - ax - fw, map.1 - ay - fh), (fw, fh), map, dist);
                        assert_eq!(q, p.map(turn_xy), "anchor {ax},{ay} {fw}x{fh} dist {dist}");
                    }
                }
            }
        }
    }
}

#[test]
fn the_gather_point_is_dist_cells_from_the_anchor_toward_the_middle() {
    // South-west anchor of 4 x 4 at (12, 68): doubled centre (28, 140), the middle of the map (96, 96): the
    // longer axis is x (68 against 44), so the point is 8 cells east of the anchor's middle, in proportion north.
    let p = gather_point((12, 68), (4, 4), (96, 96), 8).unwrap();
    // x: 28 + 16 = 44 is a border, taken on the anchor's side (cell 21); y: 140 - 44 * 16 / 68 = 130, a border too, taken on the anchor's side (cell 65).
    assert_eq!(p, (21, 65));
    // On the middle of the map: none.
    assert_eq!(gather_point((46, 46), (4, 4), (96, 96), 8), None);
}

#[test]
fn a_ring_in_the_old_order_is_by_row_then_column_and_with_a_facing_by_rank() {
    let old = ring_cells((10, 10), 1, None);
    assert_eq!(
        old,
        [
            (9, 9),
            (10, 9),
            (11, 9),
            (9, 10),
            (11, 10),
            (9, 11),
            (10, 11),
            (11, 11)
        ]
    );
    let (a, b) = both();
    for ring in 0..=10 {
        let first = (30, 50);
        let ra = ring_cells(first, ring, Some(&a));
        let rb = ring_cells(turn_xy(first), ring, Some(&b));
        let mapped: Vec<(i64, i64)> = ra.iter().map(|&c| turn_xy(c)).collect();
        assert_eq!(mapped, rb, "ring {ring}");
        let mut sorted = ra.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ra.len(), "ring {ring} has a cell twice");
        assert_eq!(ra.len(), if ring == 0 { 1 } else { 8 * ring as usize });
    }
    // The first cell tried from the south-west is the one nearest the enemy: north-east of the point.
    assert_eq!(ring_cells((30, 50), 1, Some(&a))[0], (31, 49));
}

#[test]
fn the_quarters_in_the_old_order_and_the_facing_order_turn_with_the_map() {
    let old = quarters((W, H), None);
    assert_eq!(
        old,
        [
            Cell::new(24, 24),
            Cell::new(72, 24),
            Cell::new(24, 72),
            Cell::new(72, 72)
        ]
    );
    let (a, b) = both();
    let from_a = centre(sites()[1].1);
    let from_b = centre(sites()[0].1);
    let qa = quarters((W, H), Some((&a, from_a)));
    let qb = quarters((W, H), Some((&b, from_b)));
    let turned: Vec<Cell> = qa.iter().map(|&c| turn(c)).collect();
    assert_eq!(turned, qb);
    // The four distinct quarters, the one of the enemy (north-east) first, the one of the AI (south-west) last.
    let mut all = qa.clone();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 4);
    assert_eq!(qa[0], Cell::new(71, 24));
    assert_eq!(qa[3], Cell::new(24, 71));
    assert_eq!(qb[0], Cell::new(24, 71));
}

#[test]
fn the_candidates_of_a_footprint_turn_with_the_map_when_the_placement_has_a_facing() {
    use crate::placement::sort_candidates;
    let (a, b) = both();
    let (w, h) = (2i64, 3i64);
    let turn_fp = |&(s, y, x): &(u64, i64, i64)| (s, i64::from(H) - y - h, i64::from(W) - x - w);
    // All equal scores: only the tie order decides.
    let mut ca: Vec<(u64, i64, i64)> = Vec::new();
    for y in 55..70 {
        for x in 8..24 {
            ca.push((7, y, x));
        }
    }
    let mut cb: Vec<(u64, i64, i64)> = ca.iter().map(turn_fp).collect();
    sort_candidates(&mut ca, (w, h), Some(&a));
    sort_candidates(&mut cb, (w, h), Some(&b));
    let turned: Vec<(u64, i64, i64)> = ca.iter().map(turn_fp).collect();
    assert_eq!(turned, cb);
    // The old order for contrast: (y, x).
    let mut old = ca.clone();
    sort_candidates(&mut old, (w, h), None);
    assert_eq!(old[0], (7, 55, 8));
    // A lower score still comes first with a facing.
    let mut mixed = vec![(9, 60, 10), (3, 70, 20), (3, 60, 10)];
    sort_candidates(&mut mixed, (w, h), Some(&a));
    assert_eq!(mixed[2], (9, 60, 10));
    assert_eq!(mixed[0].0, 3);
}
