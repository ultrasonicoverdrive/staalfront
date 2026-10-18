//! Tests of the flowfield machinery below the world (WP4.3): the integration
//! against a plain Dijkstra, the directions, the region, the slots, the pool and
//! the rebuild. The world level (orders, the budget shared with routes, the
//! checksum) is in `tests/flowfield.rs`, the big crossing in the headless
//! scenario `bridge_200`.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use sf_math::{Facing, Fx, Rng, Vec2};

use super::claim;
use super::field::{
    Field, FieldState, Fields, GOAL, IDLE_TICKS, LOOKAHEAD, Layout, MAX_SLOTS, UNSET,
};
use super::grid::DIRS;
use super::tests::{FOOT, mid, movement, terrain};
use super::*;
use crate::behavior::{Area, Behavior};
use crate::entities::NewEntity;
use crate::ids::{Kind, PlayerId};
use crate::units::UnitStats;

const MAN: Kind = Kind(1);

fn units() -> UnitTable {
    UnitTable::new(vec![UnitStats {
        kind: MAN,
        hp: 10,
        movement_class: Some(FOOT),
        speed_per_tick: Fx::from_ratio(1, 20),
        turn_per_tick: 255,
        size: Fx::from_ratio(4, 10),
        footprint: None,
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    }])
    .unwrap()
}

/// A pathing service with units on a map: the pieces `World` holds, without
/// the world.
struct Rig {
    terrain: Terrain,
    occupancy: Occupancy,
    p: Pathing,
    e: Entities,
    units: UnitTable,
}

impl Rig {
    fn new(rows: &[&str], at: &[(u32, u32)]) -> Rig {
        Rig::with_budget(rows, at, DEFAULT_NODE_BUDGET)
    }

    fn with_budget(rows: &[&str], at: &[(u32, u32)], budget: u32) -> Rig {
        let terrain = terrain(rows);
        let occupancy = Occupancy::new(terrain.width(), terrain.height());
        // Two make a group here, so that the maps and the groups can be small.
        let p = Pathing::build(&terrain, &movement(), &occupancy, budget, 2);
        let mut e = Entities::default();
        for (n, &(x, y)) in at.iter().enumerate() {
            let i = e.push_free_slot();
            assert_eq!(i, n);
            e.fill(
                i,
                NewEntity {
                    kind: MAN,
                    owner: PlayerId(0),
                    pos: mid(x, y),
                    facing: Facing::default(),
                    behavior: Behavior::Idle,
                    area: Area::new(Vec2::ZERO, Vec2::ZERO),
                    rng: Rng::new(n as u64),
                    def: 0,
                    hp: 10,
                },
            );
        }
        Rig {
            terrain,
            occupancy,
            p,
            e,
            units: units(),
        }
    }

    fn everyone(&self) -> Vec<usize> {
        (0..self.e.len()).collect()
    }

    /// A group order for everybody to `goal`.
    fn order(&mut self, goal: (u32, u32)) {
        let started = self.everyone();
        self.p
            .group_order(&mut self.e, &self.units, &started, mid(goal.0, goal.1));
    }

    fn serve(&mut self) {
        self.p.serve(&mut self.e, &self.units);
    }

    /// Serves until no field is waiting to be built; the ticks it took.
    fn build(&mut self) -> u32 {
        for t in 1..=2000 {
            self.serve();
            assert!(self.p.stats.nodes_last_tick <= self.p.budget);
            if !self.p.fields.has_work() {
                return t;
            }
        }
        panic!("the field was not built in 2000 ticks");
    }

    fn field(&self) -> &Field {
        self.p
            .fields
            .entries
            .iter()
            .find(|f| f.serial != 0 && f.state == FieldState::Ready)
            .expect("a finished field")
    }

    fn graph(&self) -> &ClassGraph {
        &self.p.graphs[0]
    }
}

/// An open map with a few walls and a road (64 cells wide, 48 high).
fn walls() -> Vec<String> {
    let (w, h) = (64_usize, 48_usize);
    let mut rows = vec![vec![b'.'; w]; h];
    for row in rows.iter_mut().take(36).skip(8) {
        row[20] = b'#';
        row[21] = b'#';
    }
    for row in rows.iter_mut().skip(14) {
        row[40] = b'#';
    }
    rows[44][4..60].fill(b'r');
    rows[30][41] = b'#';
    rows[31][41] = b'#';
    rows.iter()
        .map(|r| String::from_utf8(r.clone()).unwrap())
        .collect()
}

fn as_refs(rows: &[String]) -> Vec<&str> {
    rows.iter().map(String::as_str).collect()
}

/// The cost to the goal of every cell of the field's region by a plain Dijkstra
/// over the grid, the cells outside the region treated as walls.
fn reference(f: &Field, g: &ClassGraph) -> BTreeMap<(u32, u32), u32> {
    let (gx, gy) = g.grid.xy(f.goal);
    let mut cost: BTreeMap<(u32, u32), u32> = BTreeMap::new();
    let mut heap = BinaryHeap::new();
    cost.insert((gx, gy), 0);
    heap.push(Reverse((0_u32, gx, gy)));
    while let Some(Reverse((c, x, y))) = heap.pop() {
        if cost[&(x, y)] < c {
            continue;
        }
        for &(dx, dy) in &DIRS {
            let (nx, ny) = (i64::from(x) + dx, i64::from(y) + dy);
            if nx < 0
                || ny < 0
                || nx >= i64::from(g.grid.width())
                || ny >= i64::from(g.grid.height())
            {
                continue;
            }
            let (nx, ny) = (nx as u32, ny as u32);
            if f.index(&g.sectors, nx, ny).is_none() || !g.grid.can_step(nx, ny, -dx, -dy) {
                continue;
            }
            let cand = c + g.grid.step_cost(nx, ny, -dx, -dy);
            if cost.get(&(nx, ny)).is_none_or(|&old| cand < old) {
                cost.insert((nx, ny), cand);
                heap.push(Reverse((cand, nx, ny)));
            }
        }
    }
    cost
}

fn region_cells(f: &Field, g: &ClassGraph) -> Vec<(u32, u32)> {
    let mut cells = Vec::new();
    for i in 0..f.dir.len() {
        let (x, y) = f.xy(&g.sectors, i);
        if x < g.grid.width() && y < g.grid.height() {
            cells.push((x, y));
        }
    }
    cells
}

// --- the integration and the directions ---

#[test]
fn the_costs_are_those_of_a_plain_dijkstra_over_the_region() {
    let rows = walls();
    let mut rig = Rig::new(&as_refs(&rows), &[(3, 3), (5, 40), (30, 20), (10, 10)]);
    rig.order((55, 5));
    rig.build();
    let (f, g) = (rig.field(), rig.graph());
    let want = reference(f, g);
    let mut compared = 0;
    for (x, y) in region_cells(f, g) {
        let i = f.index(&g.sectors, x, y).unwrap();
        match want.get(&(x, y)) {
            Some(&c) => {
                assert!(f.dir[i] != UNSET, "({x}, {y}) is integrated");
                assert_eq!(f.cost[i], c, "cost of ({x}, {y})");
                compared += 1;
            }
            None => assert_eq!(f.dir[i], UNSET, "({x}, {y}) cannot reach the goal"),
        }
    }
    assert!(compared > 500, "{compared}");
    assert_eq!(f.finalized as usize, want.len());
}

#[test]
fn every_direction_leads_down_to_the_goal_and_ties_go_to_the_first() {
    let rows = walls();
    let mut rig = Rig::new(&as_refs(&rows), &[(3, 3), (5, 40)]);
    rig.order((55, 5));
    rig.build();
    let (f, g) = (rig.field(), rig.graph());
    let (gx, gy) = g.grid.xy(f.goal);
    assert_eq!(f.dir_at(&g.sectors, gx, gy), GOAL);
    for (x, y) in region_cells(f, g) {
        let i = f.index(&g.sectors, x, y).unwrap();
        let d = f.dir[i];
        if d == UNSET || d == GOAL {
            continue;
        }
        // The steps that are allowed and lead to an integrated cell.
        let totals: Vec<Option<u64>> = DIRS
            .iter()
            .map(|&(dx, dy)| {
                if !g.grid.can_step(x, y, dx, dy) {
                    return None;
                }
                let (nx, ny) = ((i64::from(x) + dx) as u32, (i64::from(y) + dy) as u32);
                let m = f.index(&g.sectors, nx, ny)?;
                (f.dir[m] != UNSET)
                    .then(|| u64::from(f.cost[m]) + u64::from(g.grid.step_cost(x, y, dx, dy)))
            })
            .collect();
        let best = totals.iter().flatten().min().copied().unwrap();
        assert_eq!(best, u64::from(f.cost[i]), "the direction gives the cost");
        let first = totals.iter().position(|t| *t == Some(best)).unwrap();
        assert_eq!(usize::from(d), first, "tie-break at ({x}, {y})");
    }
    // Following the directions from a far cell gets to the goal, the cost
    // falling by the cost of each step.
    let (mut x, mut y) = (3_u32, 3_u32);
    let mut steps = 0;
    while (x, y) != (gx, gy) {
        let i = f.index(&g.sectors, x, y).unwrap();
        let d = usize::from(f.dir[i]);
        let (dx, dy) = DIRS[d];
        assert!(g.grid.can_step(x, y, dx, dy));
        let (nx, ny) = ((i64::from(x) + dx) as u32, (i64::from(y) + dy) as u32);
        let m = f.index(&g.sectors, nx, ny).unwrap();
        assert_eq!(f.cost[i], f.cost[m] + g.grid.step_cost(x, y, dx, dy));
        (x, y) = (nx, ny);
        steps += 1;
        assert!(steps < 500);
    }
}

#[test]
fn slices_of_the_budget_do_not_change_the_field() {
    let rows = walls();
    let at = [(3, 3), (5, 40), (30, 20)];
    let make = |budget: u32| {
        let mut rig = Rig::with_budget(&as_refs(&rows), &at, budget);
        rig.order((55, 5));
        let ticks = rig.build();
        (rig, ticks)
    };
    let (whole, t_whole) = make(1_000_000);
    let (sliced, t_sliced) = make(61);
    let (a, b) = (whole.field(), sliced.field());
    assert_eq!(a.sectors, b.sectors);
    assert_eq!(a.cost, b.cost);
    assert_eq!(a.dir, b.dir);
    assert_eq!(a.slots, b.slots);
    assert_eq!(a.digest, b.digest);
    assert!(t_whole <= 2 && t_sliced > 40, "{t_whole} {t_sliced}");
}

#[test]
fn a_blocked_cell_has_a_cost_and_leads_out_but_nothing_leads_in() {
    // A structure on a cell a unit stands on: the unit leaves it, nothing else
    // goes through it.
    let rows = ["................", "................", "................"];
    let mut rig = Rig::new(&rows, &[(2, 1), (3, 0)]);
    let index = rig.occupancy.index(Cell::new(2, 1)).unwrap();
    assert!(rig.occupancy.claim(index, 0));
    rig.p.refresh_cells(
        &rig.terrain,
        &movement(),
        &rig.occupancy,
        [Cell::new(2, 1)].into_iter(),
    );
    rig.order((12, 1));
    rig.build();
    let (f, g) = (rig.field(), rig.graph());
    let here = f.index(&g.sectors, 2, 1).unwrap();
    assert_ne!(f.dir[here], UNSET, "the unit can get out");
    let (dx, dy) = DIRS[usize::from(f.dir[here])];
    assert!(g.grid.can_step(2, 1, dx, dy));
    // No cell steps into it.
    for (x, y) in region_cells(f, g) {
        let i = f.index(&g.sectors, x, y).unwrap();
        if f.dir[i] == UNSET || f.dir[i] == GOAL || (x, y) == (2, 1) {
            continue;
        }
        let (dx, dy) = DIRS[usize::from(f.dir[i])];
        let next = ((i64::from(x) + dx) as u32, (i64::from(y) + dy) as u32);
        assert_ne!(next, (2, 1));
    }
}

// --- the region ---

#[test]
fn the_region_is_the_corridor_and_one_ring_of_sectors() {
    // Ten sectors across and six down, open. The group and the goal are in the
    // top row: the corridor is that row, and the margin is the row below.
    let rows: Vec<String> = (0..96).map(|_| ".".repeat(160)).collect();
    let mut rig = Rig::new(&as_refs(&rows), &[(2, 2), (3, 3), (4, 2)]);
    rig.order((150, 2));
    rig.build();
    let (f, g) = (rig.field(), rig.graph());
    assert_eq!(f.sectors.len(), 20, "{:?}", f.sectors);
    for &s in &f.sectors {
        let (_, sy) = g.sectors.coords(s);
        assert!(sy <= 1, "sector {s}");
    }
    // Cells outside the region are not integrated and have no place.
    assert!(f.index(&g.sectors, 5, 50).is_none());
    // The goal's own sector is in it.
    let (gx, gy) = g.grid.xy(f.goal);
    assert!(f.index(&g.sectors, gx, gy).is_some());
}

#[test]
fn units_in_one_sector_with_the_goal_need_no_corridor() {
    let rows: Vec<String> = (0..64).map(|_| ".".repeat(64)).collect();
    let mut rig = Rig::new(&as_refs(&rows), &[(2, 2), (3, 3)]);
    rig.order((10, 10));
    rig.build();
    let (f, g) = (rig.field(), rig.graph());
    // The goal's sector and its neighbours at most; the units are in the first sector.
    assert!(f.sectors.len() <= 4, "{:?}", f.sectors);
    assert!(f.sectors.contains(&g.sectors.of_cell(10, 10)));
}

#[test]
fn a_unit_cut_off_from_the_goal_is_not_in_the_field_and_goes_on_alone() {
    // A wall across the whole map; one unit on each side.
    let mut rows: Vec<String> = (0..32).map(|_| ".".repeat(48)).collect();
    for r in rows.iter_mut() {
        r.replace_range(24..25, "#");
    }
    let mut rig = Rig::new(&as_refs(&rows), &[(4, 4), (6, 6), (40, 10), (42, 12)]);
    // Ranks 0 and 1 are on the far side.
    rig.order((44, 20));
    rig.build();
    // The units of the far side follow the field.
    for i in 2..4 {
        let steer = rig.p.steer(&mut rig.e, i, mid(44, 20), Some(FOOT));
        assert!(matches!(steer, Steer::Go(_, _)), "{steer:?}");
        assert_ne!(rig.e.route[i].field_serial, 0);
    }
    // The others are not in the region: routed on their own, to the reachable
    // cell nearest the goal.
    for i in 0..2 {
        let steer = rig.p.steer(&mut rig.e, i, mid(44, 20), Some(FOOT));
        assert_eq!(steer, Steer::Wait, "waiting for its own route");
        assert_eq!(rig.e.route[i].field_serial, 0);
        assert_eq!(rig.e.route[i].state, RouteState::Wanted);
    }
}

#[test]
fn a_goal_the_class_cannot_enter_gets_no_field() {
    let rows = ["........", "...#....", "........"];
    let mut rig = Rig::new(&rows, &[(0, 0), (1, 0), (0, 2), (1, 2)]);
    rig.order((3, 1));
    assert!(!rig.p.fields.any());
    assert!(rig.e.route[0].field_serial == 0);
}

#[test]
fn fewer_units_than_the_threshold_get_no_field_and_zero_turns_them_off() {
    let rows: Vec<String> = (0..16).map(|_| ".".repeat(32)).collect();
    let at: Vec<(u32, u32)> = (0..7).map(|k| (k, 1)).collect();
    let mut rig = Rig::new(&as_refs(&rows), &at);
    rig.p.set_flowfield_threshold(DEFAULT_FLOWFIELD_THRESHOLD);
    rig.order((30, 14));
    assert!(!rig.p.fields.any(), "seven are not eight");
    let at: Vec<(u32, u32)> = (0..8).map(|k| (k, 1)).collect();
    let mut rig = Rig::new(&as_refs(&rows), &at);
    rig.p.set_flowfield_threshold(DEFAULT_FLOWFIELD_THRESHOLD);
    rig.order((30, 14));
    assert!(rig.p.fields.any(), "eight are");
    let mut rig = Rig::new(&as_refs(&rows), &at);
    rig.p.set_flowfield_threshold(0);
    rig.order((30, 14));
    assert!(!rig.p.fields.any(), "off");
}

#[test]
fn a_corridor_of_more_sectors_than_a_field_holds_gets_no_field() {
    // Twelve bands of sixteen sectors across, with a one cell wall between
    // them that has a gap at alternating ends: the way from the first band to
    // the last passes through every sector.
    let (w, h) = (256_usize, 192_usize);
    let mut rows = vec![vec![b'.'; w]; h];
    for band in 1..12 {
        let y = band * 16 - 1;
        rows[y].fill(b'#');
        let gap = if band % 2 == 1 { w - 3..w } else { 0..3 };
        rows[y][gap].fill(b'.');
    }
    let rows: Vec<String> = rows
        .iter()
        .map(|r| String::from_utf8(r.clone()).unwrap())
        .collect();
    let mut rig = Rig::new(&as_refs(&rows), &[(1, 1), (2, 1)]);
    rig.order((250, 190));
    assert!(rig.p.fields.any());
    for _ in 0..400 {
        rig.serve();
        assert!(rig.p.stats.nodes_last_tick <= rig.p.budget);
    }
    assert!(
        !rig.p.fields.any(),
        "dropped: the corridor needs more than FIELD_SECTORS"
    );
    const { assert!(super::FIELD_SECTORS < 192) };
    // The units are sent on alone.
    let steer = rig.p.steer(&mut rig.e, 0, mid(250, 190), Some(FOOT));
    assert_eq!(steer, Steer::Wait);
    assert_eq!(rig.e.route[0].field_serial, 0);
    assert_eq!(rig.e.route[0].state, RouteState::Wanted);
}

#[test]
fn a_group_with_no_unit_left_is_dropped_before_it_is_built() {
    let rows: Vec<String> = (0..16).map(|_| ".".repeat(32)).collect();
    let at: Vec<(u32, u32)> = (0..8).map(|k| (k, 1)).collect();
    let mut rig = Rig::new(&as_refs(&rows), &at);
    rig.order((30, 14));
    assert!(rig.p.fields.any());
    for r in 0..8 {
        rig.e.route.reset(r);
    }
    rig.serve();
    assert!(!rig.p.fields.any());
    assert!(!rig.p.fields.has_work());
}

// --- the slots ---

#[test]
fn each_rank_has_a_slot_of_its_own_in_sight_of_the_goal_and_inside_five_cells() {
    let rows: Vec<String> = (0..64).map(|_| ".".repeat(96)).collect();
    let at: Vec<(u32, u32)> = (0..200).map(|k| (2 + k % 20, 2 + k / 20)).collect();
    let mut rig = Rig::new(&as_refs(&rows), &at);
    rig.order((80, 40));
    rig.build();
    let (f, g) = (rig.field(), rig.graph());
    assert_eq!(f.slots.len(), 200);
    let (gx, gy) = g.grid.xy(f.goal);
    let centre = Vec2::new(
        Fx::from_i64(i64::from(gx)) + Fx::HALF,
        Fx::from_i64(i64::from(gy)) + Fx::HALF,
    );
    let mut sorted = f.slots.clone();
    sorted.sort_unstable_by_key(|p| (p.x.raw(), p.y.raw()));
    sorted.dedup();
    assert_eq!(sorted.len(), 200, "every slot is its own point");
    for &p in &f.slots {
        assert!((p - centre).length() < Fx::from_int(5));
        assert!(los::thick_clear(&g.grid, centre, p).0);
    }
    // And the units are given them by rank.
    for i in 0..200 {
        assert_eq!(rig.e.route[i].field_rank as usize, i);
        assert_eq!(rig.p.arrival_point(&rig.e, i), Some(f.slots[i]));
    }
}

#[test]
fn slots_keep_out_of_rock_and_out_of_sight_behind_it() {
    // The goal is in a notch of rock: points inside the rock and behind the
    // wall are not slots.
    let mut rows: Vec<String> = (0..32).map(|_| ".".repeat(48)).collect();
    for row in &mut rows[10..20] {
        row.replace_range(30..31, "#");
    }
    let mut rig = Rig::new(&as_refs(&rows), &[(2, 2), (3, 3), (4, 4), (5, 5)]);
    rig.order((29, 15));
    rig.build();
    let (f, g) = (rig.field(), rig.graph());
    assert_eq!(f.slots.len(), 4);
    for &p in &f.slots {
        let (x, y) = p.cell();
        assert!(g.grid.open(i64::from(x), i64::from(y)), "{p:?}");
        assert!(p.x < Fx::from_int(30), "not behind the wall: {p:?}");
    }
}

#[test]
fn a_group_bigger_than_the_slots_shares_them_round() {
    let rows: Vec<String> = (0..64).map(|_| ".".repeat(96)).collect();
    let n = MAX_SLOTS as u32 + 20;
    let at: Vec<(u32, u32)> = (0..n).map(|k| (2 + k % 30, 2 + k / 30)).collect();
    let mut rig = Rig::new(&as_refs(&rows), &at);
    rig.order((80, 40));
    rig.build();
    let f = rig.field();
    assert_eq!(f.slots.len(), MAX_SLOTS);
    let last = n as usize - 1;
    assert_eq!(
        rig.p.arrival_point(&rig.e, last),
        Some(f.slots[last % MAX_SLOTS])
    );
}

#[test]
fn a_group_order_gives_each_class_a_ring_and_the_slots_are_spaced_for_the_bodies() {
    // 40 riflemen in one class: a spacing of three tenths and a sixteenth, the
    // innermost ring.
    let rows: Vec<String> = (0..32).map(|_| ".".repeat(64)).collect();
    let at: Vec<(u32, u32)> = (0..40).map(|k| (1 + k % 10, 1 + k / 10)).collect();
    let mut rig = Rig::new(&as_refs(&rows), &at);
    rig.order((40, 20));
    let layout = rig.p.fields.entries[0].layout;
    assert_eq!(layout.rank, 0);
    assert_eq!(layout.inner, Fx::ZERO);
    assert_eq!(
        layout.spacing,
        Fx::from_ratio(3, 10) + super::slots::GAP,
        "three quarters of a rifleman's width and a gap"
    );
    assert_ne!(layout.order, 0);
    rig.build();
    let f = rig.field();
    // No two slots are closer than the spacing.
    for (a, &p) in f.slots.iter().enumerate() {
        for &q in &f.slots[a + 1..] {
            assert!((p - q).length() >= f.layout.spacing - Fx::from_ratio(1, 1000));
        }
    }
}

#[test]
fn the_ring_of_a_later_class_begins_where_the_earlier_ones_end_and_skips_their_slots() {
    // Two fields of one order (the second is the next class's): the later one
    // starts further out, and its points that are too near a slot of the earlier
    // one, or are the same point, are left out, though they are in sight.
    let rows: Vec<String> = (0..48).map(|_| ".".repeat(64)).collect();
    let at: Vec<(u32, u32)> = (0..12).map(|k| (1 + k % 6, 1 + k / 6)).collect();
    let mut rig = Rig::new(&as_refs(&rows), &at);
    rig.order((40, 24));
    rig.build();
    let first = rig.field().clone();
    let order = first.layout.order;
    let wide = super::slots::spacing_for(Fx::from_ratio(12, 10));
    // The next class of the same order: no inner radius at all, so that it would
    // start in the middle and run into the first one's slots if nothing stopped it.
    let sectors = rig.graph().sectors.count();
    let serial = rig.p.fields.create(
        0,
        first.goal,
        Layout {
            order,
            rank: 1,
            spacing: wide,
            inner: Fx::ZERO,
        },
        12,
        sectors,
    );
    assert_ne!(serial, 0);
    // The units follow the second field now (it needs members to be built).
    for i in 0..12 {
        rig.e.route[i].field_serial = serial;
    }
    rig.build();
    let second = rig
        .p
        .fields
        .entries
        .iter()
        .find(|f| f.serial == serial)
        .unwrap();
    assert_eq!(second.slots.len(), 12);
    for &p in &second.slots {
        assert!(!first.slots.contains(&p), "one point in two classes");
        for &q in &first.slots {
            let apart = (first.layout.spacing + wide).div_int(2) - super::slots::GAP;
            assert!((p - q).length() >= apart, "{p:?} too near {q:?}");
        }
    }
}

// --- following ---

#[test]
fn a_unit_steers_at_the_farthest_cell_down_its_chain_that_it_can_see() {
    let rows: Vec<String> = (0..48).map(|_| ".".repeat(96)).collect();
    let mut rig = Rig::new(&as_refs(&rows), &[(2, 20), (2, 21)]);
    rig.order((90, 20));
    rig.build();
    // Open ground: the chain is a straight row, and the farthest cell it
    // looks down is LOOKAHEAD steps on.
    let steer = rig.p.steer(&mut rig.e, 0, mid(90, 20), Some(FOOT));
    let Steer::Go(at, Aim::Waypoint) = steer else {
        panic!("{steer:?}")
    };
    assert_eq!(at.cell(), (2 + LOOKAHEAD as i32, 20), "{at:?}");
    assert_eq!(rig.e.route[0].ring_len(), 1);
    // Reached: the next one is chosen from where the unit stands.
    rig.e.pos[0] = at;
    rig.p.reached(&mut rig.e, 0);
    assert_eq!(rig.e.route[0].ring_len(), 0);
    let Steer::Go(next, _) = rig.p.steer(&mut rig.e, 0, mid(90, 20), Some(FOOT)) else {
        panic!()
    };
    assert_eq!(next.cell().0, at.cell().0 + LOOKAHEAD as i32);
}

#[test]
fn a_unit_waits_until_its_own_cell_is_integrated_and_the_nearest_ones_are_first() {
    let rows: Vec<String> = (0..48).map(|_| ".".repeat(96)).collect();
    // One unit near the goal and one far from it.
    let mut rig = Rig::with_budget(&as_refs(&rows), &[(84, 20), (4, 20)], 512);
    rig.order((90, 20));
    let mut near_at = None;
    let mut far_at = None;
    for t in 1..400 {
        rig.serve();
        for (i, at) in [(0_usize, &mut near_at), (1, &mut far_at)] {
            if at.is_none()
                && matches!(
                    rig.p.steer(&mut rig.e, i, mid(90, 20), Some(FOOT)),
                    Steer::Go(_, _)
                )
            {
                *at = Some(t);
            }
        }
        if near_at.is_some() && far_at.is_some() {
            break;
        }
    }
    let (near_at, far_at) = (near_at.unwrap(), far_at.unwrap());
    assert!(near_at < far_at, "{near_at} {far_at}");
}

#[test]
fn a_refused_step_goes_cell_by_cell_and_a_second_one_strands_the_unit() {
    let rows: Vec<String> = (0..48).map(|_| ".".repeat(96)).collect();
    let mut rig = Rig::new(&as_refs(&rows), &[(2, 20), (2, 21)]);
    rig.order((90, 20));
    rig.build();
    let Steer::Go(far, _) = rig.p.steer(&mut rig.e, 0, mid(90, 20), Some(FOOT)) else {
        panic!()
    };
    rig.p.blocked(&mut rig.e, 0);
    assert!(rig.e.route[0].cautious);
    let Steer::Go(near, _) = rig.p.steer(&mut rig.e, 0, mid(90, 20), Some(FOOT)) else {
        panic!()
    };
    assert!(near.x < far.x, "one cell on, not the farthest");
    assert_eq!(near.cell(), (3, 20));
    rig.p.blocked(&mut rig.e, 0);
    assert_eq!(
        rig.p.steer(&mut rig.e, 0, mid(90, 20), Some(FOOT)),
        Steer::Stranded
    );
}

// --- the pool ---

#[test]
fn the_least_recently_used_field_goes_when_the_pool_is_full_ties_to_the_lowest() {
    let mut fields = Fields::new(8);
    let serials: Vec<u32> = (0..super::FIELD_POOL)
        .map(|k| fields.create(0, k as u32, Layout::ALONE, 8, 4))
        .collect();
    assert_eq!(fields.live() as usize, super::FIELD_POOL);
    // All made at the same tick: the first goes.
    let ninth = fields.create(0, 99, Layout::ALONE, 8, 4);
    assert!(fields.find(serials[0]).is_none());
    assert!(fields.find(ninth).is_some());
    for &s in &serials[1..] {
        assert!(fields.find(s).is_some());
    }
    // Later two are used (the ninth took the place of the first): the unused
    // oldest, the lowest place of them, goes next.
    for _ in 0..10 {
        fields.tick();
    }
    fields.touch(serials[1]);
    fields.touch(ninth);
    let tenth = fields.create(0, 100, Layout::ALONE, 8, 4);
    assert!(fields.find(serials[1]).is_some(), "used lately");
    assert!(fields.find(serials[2]).is_none(), "the oldest unused");
    assert!(fields.find(tenth).is_some());
    assert_eq!(fields.live() as usize, super::FIELD_POOL);
}

#[test]
fn a_field_nobody_uses_expires_and_one_in_use_does_not() {
    let mut fields = Fields::new(8);
    let idle = fields.create(0, 1, Layout::ALONE, 8, 4);
    let busy = fields.create(0, 2, Layout::ALONE, 8, 4);
    for _ in 0..IDLE_TICKS {
        fields.tick();
        fields.touch(busy);
    }
    assert!(fields.find(idle).is_some(), "not yet");
    fields.tick();
    fields.touch(busy);
    assert!(fields.find(idle).is_none());
    assert!(fields.find(busy).is_some());
}

#[test]
fn a_field_that_is_being_built_expires_too_when_nobody_asks_for_it() {
    // The documented rule (the comment of `Fields::tick`): the build in flight is
    // dropped like any other field that nobody has asked for in IDLE_TICKS, and the
    // builder is free for the queue.
    let mut fields = Fields::new(8);
    let building = fields.create(0, 1, Layout::ALONE, 8, 4);
    let queued = fields.create(0, 2, Layout::ALONE, 8, 4);
    fields.touch(queued);
    // Start the first build by hand, as `advance` does.
    let entry = fields.find(building).unwrap();
    fields.entries[entry].state = FieldState::Building;
    fields.cur = Some(super::field::Cur::new(entry, fields.entries[entry].born));
    for _ in 0..=IDLE_TICKS {
        fields.tick();
        fields.touch(queued);
    }
    assert!(fields.find(building).is_none(), "dropped");
    assert!(
        fields.cur.is_none(),
        "and the build in flight ended with it"
    );
    assert!(fields.find(queued).is_some(), "the one in use stays");
}

#[test]
fn a_stranded_member_gets_another_try_when_the_map_changes_and_does_not_keep_its_field_alive() {
    let rows: Vec<String> = (0..48).map(|_| ".".repeat(96)).collect();
    let mut rig = Rig::new(&as_refs(&rows), &[(2, 20), (2, 21), (3, 20)]);
    rig.order((90, 20));
    rig.build();
    let serial = rig.e.route[0].field_serial;
    // Two refused steps strand unit 0 (the second goes cell by cell).
    assert!(matches!(
        rig.p.steer(&mut rig.e, 0, mid(90, 20), Some(FOOT)),
        Steer::Go(..)
    ));
    rig.p.blocked(&mut rig.e, 0);
    assert!(matches!(
        rig.p.steer(&mut rig.e, 0, mid(90, 20), Some(FOOT)),
        Steer::Go(..)
    ));
    rig.p.blocked(&mut rig.e, 0);
    assert_eq!(
        rig.p.steer(&mut rig.e, 0, mid(90, 20), Some(FOOT)),
        Steer::Stranded
    );
    // It stays stranded while nothing changes...
    assert_eq!(
        rig.p.steer(&mut rig.e, 0, mid(90, 20), Some(FOOT)),
        Steer::Stranded
    );
    // ...and a change of the map, anywhere, gives it another go: the epoch moved on,
    // so the leg is chosen again, smoothing allowed (this used to be checked after
    // the stranded state, which therefore never ended).
    place(&mut rig, &[(80, 2)], 3);
    assert!(
        matches!(
            rig.p.steer(&mut rig.e, 0, mid(90, 20), Some(FOOT)),
            Steer::Go(..)
        ),
        "it is on the move again"
    );
    assert!(!rig.e.route[0].cautious);
    // A stranded member does not touch its field: with every member stranded the
    // field expires after IDLE_TICKS and the members are routed on their own.
    rig.p.blocked(&mut rig.e, 0);
    rig.p.steer(&mut rig.e, 0, mid(90, 20), Some(FOOT));
    rig.p.blocked(&mut rig.e, 0);
    // Only unit 0 is a member now, and it is stranded.
    rig.e.route[1] = super::Route::IDLE;
    rig.e.route[2] = super::Route::IDLE;
    assert_eq!(rig.e.route[0].state, super::RouteState::Stranded);
    for _ in 0..=IDLE_TICKS + 1 {
        rig.serve();
        let _ = rig.p.steer(&mut rig.e, 0, mid(90, 20), Some(FOOT));
    }
    assert!(
        rig.p.fields.find(serial).is_none(),
        "the field of a stranded member expired"
    );
    // And the member found it gone and is routed on its own from where it stands.
    assert_eq!(rig.e.route[0].field_serial, 0);
}

// --- a change of the map ---

/// Puts a structure on `cells` and tells the service.
fn place(rig: &mut Rig, cells: &[(u32, u32)], slot: u32) {
    for &(x, y) in cells {
        let i = rig.occupancy.index(Cell::new(x, y)).unwrap();
        assert!(rig.occupancy.claim(i, slot));
    }
    let list: Vec<Cell> = cells.iter().map(|&(x, y)| Cell::new(x, y)).collect();
    rig.p
        .refresh_cells(&rig.terrain, &movement(), &rig.occupancy, list.into_iter());
}

#[test]
fn a_structure_in_the_region_makes_a_successor_that_replaces_the_field() {
    let rows: Vec<String> = (0..48).map(|_| ".".repeat(96)).collect();
    let mut rig = Rig::new(&as_refs(&rows), &[(2, 20), (2, 21), (3, 20)]);
    rig.order((90, 20));
    rig.build();
    let old = rig.field().clone();
    assert_eq!(old.version, 0);
    let before = rig.p.fields.built;
    // A wall of structures across the row, in the region.
    place(
        &mut rig,
        &[(40, 18), (40, 19), (40, 20), (40, 21), (40, 22)],
        7,
    );
    assert_eq!(
        rig.p
            .fields
            .entries
            .iter()
            .filter(|f| f.serial != 0)
            .count(),
        2
    );
    // Until it is built the old one is still the one in use.
    let in_use = rig.p.fields.find(old.serial).unwrap();
    assert_eq!(rig.p.fields.entries[in_use].version, 0);
    rig.build();
    assert_eq!(rig.p.fields.built, before + 1);
    let f = rig.field();
    assert_eq!(f.version, 1);
    assert_eq!(
        rig.p
            .fields
            .entries
            .iter()
            .filter(|f| f.serial != 0)
            .count(),
        1,
        "the old one is gone"
    );
    // The new field goes round the wall: the cell in front of it does not
    // point into it.
    let g = rig.graph();
    let i = f.index(&g.sectors, 39, 20).unwrap();
    let (dx, dy) = DIRS[usize::from(f.dir[i])];
    assert!(g.grid.can_step(39, 20, dx, dy));
    assert_ne!(dx, 1, "not straight into the wall");
    assert!(f.cost[i] > old.cost[old.index(&g.sectors, 39, 20).unwrap()]);
}

#[test]
fn the_old_field_is_followed_only_where_it_is_still_valid() {
    let rows: Vec<String> = (0..48).map(|_| ".".repeat(96)).collect();
    let mut rig = Rig::new(&as_refs(&rows), &[(36, 20), (36, 21), (3, 20)]);
    rig.order((90, 20));
    rig.build();
    place(
        &mut rig,
        &[(40, 18), (40, 19), (40, 20), (40, 21), (40, 22)],
        7,
    );
    // The rebuild has not run: the unit at (36, 20) sees a chain that runs into
    // the structure. It steers at a cell short of it, never at or through it.
    let steer = rig.p.steer(&mut rig.e, 0, mid(90, 20), Some(FOOT));
    let Steer::Go(at, Aim::Waypoint) = steer else {
        panic!("{steer:?}")
    };
    assert!(at.x < Fx::from_int(40), "{at:?}");
    // Standing right in front of it, with the step the old field says not
    // allowed: it waits for the new one.
    rig.e.pos[0] = mid(39, 20);
    rig.e.route[0].ring_clear();
    let steer = rig.p.steer(&mut rig.e, 0, mid(90, 20), Some(FOOT));
    assert!(
        steer == Steer::Wait || matches!(steer, Steer::Go(p, _) if p.x < Fx::from_int(40)),
        "{steer:?}"
    );
    // The new field is there after the build; the unit is led round.
    rig.build();
    rig.e.route[0].ring_clear();
    let Steer::Go(round, _) = rig.p.steer(&mut rig.e, 0, mid(90, 20), Some(FOOT)) else {
        panic!()
    };
    assert!(
        round.y < Fx::from_int(18) || round.y >= Fx::from_int(23),
        "{round:?}"
    );
}

#[test]
fn a_change_far_from_the_region_leaves_the_field_alone() {
    let rows: Vec<String> = (0..96).map(|_| ".".repeat(160)).collect();
    let mut rig = Rig::new(&as_refs(&rows), &[(2, 2), (3, 3), (4, 2)]);
    rig.order((150, 2));
    rig.build();
    let built = rig.p.fields.built;
    // The bottom row of sectors is two rows from the region.
    place(&mut rig, &[(100, 90), (101, 90)], 9);
    assert_eq!(
        rig.p
            .fields
            .entries
            .iter()
            .filter(|f| f.serial != 0)
            .count(),
        1
    );
    rig.serve();
    assert!(!rig.p.fields.has_work());
    assert_eq!(rig.p.fields.built, built);
}

#[test]
fn a_change_while_the_field_is_being_built_starts_the_build_again() {
    let rows: Vec<String> = (0..48).map(|_| ".".repeat(96)).collect();
    let mut rig = Rig::with_budget(&as_refs(&rows), &[(2, 20), (2, 21), (3, 20)], 512);
    rig.order((90, 20));
    for _ in 0..3 {
        rig.serve();
    }
    assert!(rig.p.fields.has_work());
    place(&mut rig, &[(60, 20)], 3);
    let cur = rig.p.fields.cur.as_ref().unwrap();
    assert_eq!(cur.stage, super::field::Stage::Gather, "back to the start");
    rig.build();
    let f = rig.field();
    let g = rig.graph();
    // Built on the grid as it is: the structure's cell is not walked into.
    let i = f.index(&g.sectors, 59, 20).unwrap();
    assert_ne!(f.dir[i], UNSET);
    let (dx, dy) = DIRS[usize::from(f.dir[i])];
    assert!(!(dx == 1 && dy == 0), "not into the structure");
}

// --- slots taken on approach (WP4.4b, `path/claim.rs`) ---

/// A rig of `n` units that are all in range of a goal at (80, 40) on open ground, standing in a
/// row west of it, one every third of a cell, with the field built.
fn near_the_goal(n: usize) -> Rig {
    let rows: Vec<String> = (0..64).map(|_| ".".repeat(96)).collect();
    let at: Vec<(u32, u32)> = (0..n)
        .map(|k| (2 + k as u32 % 20, 2 + k as u32 / 20))
        .collect();
    let mut rig = Rig::new(&as_refs(&rows), &at);
    rig.order((80, 40));
    rig.build();
    for i in 0..n {
        rig.e.pos[i] = Vec2::new(
            Fx::from_ratio(7600 + 33 * i as i64, 100),
            Fx::from_ratio(4050, 100),
        );
    }
    rig
}

/// Every unit asks for its next step once, in the order of the entities, as the world does.
fn everyone_steers(rig: &mut Rig) {
    for i in 0..rig.e.len() {
        let _ = rig.p.steer(&mut rig.e, i, mid(80, 40), Some(FOOT));
    }
}

#[test]
fn a_unit_in_range_takes_the_first_free_slot_it_can_walk_to_and_the_next_takes_another() {
    let mut rig = near_the_goal(3);
    everyone_steers(&mut rig);
    let f = rig.field();
    let slots: Vec<usize> = (0..3)
        .map(|i| {
            f.taken
                .iter()
                .position(|&h| h == i as u32)
                .expect("every unit took a slot")
        })
        .collect();
    // The first to ask had the whole spiral to choose from and took its first point...
    assert_eq!(slots[0], 0);
    // ...the others, a slot a body away from every slot held, nearer the goal than the rest.
    assert_ne!(slots[1], slots[0]);
    assert_ne!(slots[2], slots[1]);
    for a in 0..3 {
        for b in a + 1..3 {
            assert!(
                (f.slots[slots[a]] - f.slots[slots[b]]).length() >= Fx::from_ratio(3, 10),
                "slots {a} and {b} are too near"
            );
        }
    }
    // The slot a unit stands on is the one it took, not the one its rank gives.
    for (i, &k) in slots.iter().enumerate() {
        assert_eq!(rig.p.arrival_point(&rig.e, i), Some(f.slots[k]));
        assert_eq!(rig.p.claimed_point(&rig.e, i), Some(f.slots[k]));
    }
}

#[test]
fn a_unit_out_of_range_takes_no_slot_and_goes_by_its_rank() {
    let mut rig = near_the_goal(2);
    // The second is a long way off.
    rig.e.pos[1] = mid(10, 10);
    everyone_steers(&mut rig);
    assert!(rig.p.claimed_point(&rig.e, 0).is_some());
    assert_eq!(rig.p.claimed_point(&rig.e, 1), None);
    let f = rig.field();
    assert_eq!(rig.p.arrival_point(&rig.e, 1), Some(f.slots[1]));
}

#[test]
fn a_slot_is_free_again_when_its_holder_gets_another_order_or_is_removed() {
    let mut rig = near_the_goal(3);
    everyone_steers(&mut rig);
    let k0 = rig.field().taken.iter().position(|&h| h == 0).unwrap();
    let k1 = rig.field().taken.iter().position(|&h| h == 1).unwrap();
    assert!(rig.field().held(&rig.e, k0));
    assert!(rig.field().held(&rig.e, k1));
    // Another order: the claim is let go (`orders::begin`).
    rig.e.crowd[0].claim = crate::crowd::Claim::default();
    assert!(!rig.field().held(&rig.e, k0));
    // Removed: the slot of the entity is cleared with the rest of it.
    rig.e.clear(1);
    assert!(!rig.field().held(&rig.e, k1));
    // And the next to ask takes the first slot again.
    rig.e.pos[2] = Vec2::new(Fx::from_ratio(7650, 100), Fx::from_ratio(4050, 100));
    rig.e.crowd[2].claim = crate::crowd::Claim::default();
    let _ = rig.p.steer(&mut rig.e, 2, mid(80, 40), Some(FOOT));
    let f = rig.field();
    assert_eq!(f.taken.iter().position(|&h| h == 2), Some(k0.min(k1)));
}

#[test]
fn only_so_many_units_take_a_slot_in_a_tick() {
    let n = 40;
    let mut rig = near_the_goal(n);
    everyone_steers(&mut rig);
    let taken = |rig: &Rig| {
        (0..n)
            .filter(|&i| rig.p.claimed_point(&rig.e, i).is_some())
            .count()
    };
    assert_eq!(taken(&rig), claim::CLAIMS_PER_TICK as usize);
    // The next tick lets the same number more.
    rig.p.fields.tick();
    everyone_steers(&mut rig);
    assert_eq!(taken(&rig), 2 * claim::CLAIMS_PER_TICK as usize);
}

#[test]
fn a_rebuilt_field_has_no_holders_and_the_units_take_their_slots_again() {
    let mut rig = near_the_goal(3);
    everyone_steers(&mut rig);
    assert!((0..3).all(|i| rig.p.claimed_point(&rig.e, i).is_some()));
    // A structure inside the region makes a successor; until it is built the old field serves.
    place(
        &mut rig,
        &[(30, 13), (30, 14), (30, 15), (30, 16), (30, 17)],
        7,
    );
    rig.build();
    assert!(rig.field().taken.iter().all(|&h| h == claim::NO_HOLDER));
    assert!((0..3).all(|i| rig.p.claimed_point(&rig.e, i).is_none()));
    rig.p.fields.tick();
    everyone_steers(&mut rig);
    assert!((0..3).all(|i| rig.p.claimed_point(&rig.e, i).is_some()));
}

#[test]
fn a_unit_on_the_far_side_of_a_wall_takes_no_slot_until_it_can_walk_to_one() {
    // A wall two cells thick across the whole map but for the top two rows, the goal east
    // of it. The margin of a claim (six cells beyond the slots) reaches across it.
    let mut grid: Vec<Vec<u8>> = (0..64).map(|_| vec![b'.'; 96]).collect();
    for row in grid.iter_mut().skip(2) {
        row[62] = b'#';
        row[63] = b'#';
    }
    let rows: Vec<String> = grid
        .into_iter()
        .map(|r| String::from_utf8(r).unwrap())
        .collect();
    let mut rig = Rig::new(&as_refs(&rows), &[(2, 2), (3, 2), (4, 2)]);
    rig.order((66, 40));
    rig.build();
    let put = |rig: &mut Rig, i: usize, x: i64, y: i64| {
        rig.e.pos[i] = Vec2::new(Fx::from_ratio(x, 100), Fx::from_ratio(y, 100));
    };
    // Unit 0 stands against the west face, five cells from the goal; the others east of it.
    put(&mut rig, 0, 6150, 4050);
    put(&mut rig, 1, 6650, 4350);
    put(&mut rig, 2, 6650, 3750);
    let steer_all = |rig: &mut Rig| {
        for i in 0..3 {
            let _ = rig.p.steer(&mut rig.e, i, mid(66, 40), Some(FOOT));
        }
    };
    steer_all(&mut rig);
    assert!(rig.p.claimed_point(&rig.e, 1).is_some());
    assert!(rig.p.claimed_point(&rig.e, 2).is_some());
    // It sees no slot, so it holds none, not even a shared one (that would stop it looking
    // again), and it did not take a slot from the units that can walk to theirs.
    assert_eq!(rig.p.claimed_point(&rig.e, 0), None);
    assert_eq!(rig.e.crowd[0].claim, crate::crowd::Claim::default());
    let held = rig
        .field()
        .taken
        .iter()
        .filter(|&&h| h != claim::NO_HOLDER)
        .count();
    assert_eq!(held, 2);
    // Still no slot a few ticks on, while it stands there.
    for _ in 0..5 {
        rig.p.fields.tick();
        steer_all(&mut rig);
    }
    assert_eq!(rig.p.claimed_point(&rig.e, 0), None);
    // In sight of slots, it takes one.
    put(&mut rig, 0, 6650, 4050);
    rig.p.fields.tick();
    steer_all(&mut rig);
    assert!(rig.p.claimed_point(&rig.e, 0).is_some());
}
