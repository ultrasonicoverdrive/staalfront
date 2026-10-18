//! Arrival slots taken on approach (WP4.4b, `docs/rules/collision.md`, "Arrival").
//!
//! WP4.3 handed the slots of a group out by rank: the unit that came n-th in the
//! ascending order of entity ids went to the n-th slot, whenever it arrived. That has
//! nothing to do with the order the units *reach* the goal in. The classes of a group
//! stand in rings (the fastest innermost), and the arrival order is not the order of
//! the classes: a queue on a bridge mixes them. A slow rifleman that came early stood
//! in the outer ring and walled it off, and the tank that came after it had to push
//! through to its slot in the ring inside; the crowd spread out, holes were filled by
//! others, and at the end units stood several cells from where they belonged.
//!
//! Now a unit **takes** its slot when it comes near:
//!
//! * every field keeps a table of holders (`Field::taken`, as long as its slots); a
//!   unit within [`CLAIM_MARGIN`] of the outermost slot of its class that holds none
//!   looks at the slots nobody holds, in the order of the spiral (nearest the goal
//!   first, which is how they are built);
//! * a slot is **free** when no slot already held, of its own class or of any class
//!   of the same group order, lies nearer than the two bodies keep apart
//!   (`slots::keep_apart`): the rings of two classes lie side by side, and a unit of one
//!   class that took a place in the other's ring leaves no room for the slot there;
//! * the unit takes the **first free slot it can walk to straight**: it sees it, and no
//!   slot already held lies nearer the segment to it than the two bodies keep apart.
//!   The crowd therefore fills from the middle outwards on the side the units come
//!   from, and what is left for the last unit is not behind the others;
//! * when none of the slots looked at is open it takes the nearest free one it can see,
//!   and when it can see none the first free one. When all are taken (a group bigger
//!   than [`MAX_SLOTS`](super::field::MAX_SLOTS)) it shares the slot its rank gives, as
//!   before, which is also where a unit goes before it has taken one.
//!
//! Units look in the order the world visits them, which is the order of their entity
//! ids, one at a time within the tick: a claim is a write that the next unit sees, so
//! two units that come into range in the same tick take their slots in id order, and a
//! permutation of the ids of a group order changes nothing. At most
//! [`CLAIMS_PER_TICK`] units take a slot in a tick; the others go on following the
//! field and take theirs in the next, which keeps the cost of a crowd that arrives all
//! at once bounded.
//!
//! Whether a slot is free, and whether the way to it is open, is a question put to every
//! slot held. For a claim the held slots are sorted into a grid ([`HeldBuf`]), so that
//! each question looks at the few round the point instead of at all of them (up to
//! [`MAX_SLOTS`](super::field::MAX_SLOTS) a class): the answers are exactly those of
//! looking at every one, and a claim costs a few thousand steps instead of tens of
//! thousands.
//!
//! A claim lives in the unit (`CrowdState::claim`: the field and the place) and in the
//! field's table, and counts only while both say so. It outlives the order: a unit
//! standing on its slot keeps it until it is given another order (`orders::begin`) or
//! is removed. A unit that gets stuck on its way to its slot lets go of it when it
//! sidesteps (`crowd.rs`) and takes another when it can. A rebuilt field starts with an
//! empty table; the units that were walking take their slots again, those that stand
//! are no longer counted.

use sf_math::{Fx, Vec2};

use crate::crowd::Claim;
use crate::entities::Entities;
use crate::path::field::{Field, Fields, centre_of};
use crate::path::graph::ClassGraph;
use crate::path::{los, slots};

/// The holder of a slot nobody holds.
pub(crate) const NO_HOLDER: u32 = u32::MAX;

/// How far outside its outermost slot a unit takes its own, in cells.
pub(crate) const CLAIM_MARGIN: Fx = Fx::from_int(6);

/// Units that may take a slot in one tick.
pub(crate) const CLAIMS_PER_TICK: u32 = 16;

/// The most cells along a side of the grid over the slots held (`HeldBuf`).
const GRID_SIDE: i64 = 24;

/// One slot already held, seen from the unit that is about to take one: where it is
/// and how far a slot of the unit's own lattice must keep from it
/// (`slots::keep_apart`), and the square of that.
#[derive(Copy, Clone, Debug)]
struct Held {
    at: Vec2,
    keep2: Fx,
}

/// The slots held, gathered for one claim and sorted into a grid, so that asking
/// whether a point is clear of all of them looks at the few round it and not at every
/// one (up to 256 slots a class, a claim looks at as many candidates). The cells are
/// at least as wide as the widest `keep`, so the slots that matter to a point lie in
/// the nine cells round it. A counting sort, in the order the slots were gathered:
/// no hashing, and the answers are those of looking at every slot. Scratch memory:
/// any two of them are always equal.
#[derive(Clone, Debug, Default)]
pub(crate) struct HeldBuf {
    held: Vec<Held>,
    /// Where the slots of cell `c` begin in `items` (and the next cell's end).
    start: Vec<u32>,
    /// The indexes into `held`, cell by cell.
    items: Vec<u32>,
    /// The corner of the grid and the width of a cell.
    min: Vec2,
    cell: Fx,
    cols: i64,
    rows: i64,
}

impl PartialEq for HeldBuf {
    fn eq(&self, _: &HeldBuf) -> bool {
        true
    }
}

impl Eq for HeldBuf {}

impl HeldBuf {
    fn clear(&mut self) {
        self.held.clear();
        self.cols = 0;
        self.rows = 0;
    }

    fn push(&mut self, at: Vec2, keep: Fx) {
        self.held.push(Held {
            at,
            keep2: keep.square(),
        });
    }

    /// Sorts what was pushed into the grid. `widest` is the greatest `keep` pushed.
    fn sort(&mut self, widest: Fx) {
        let Some(first) = self.held.first() else {
            return;
        };
        let (mut lo, mut hi) = (first.at, first.at);
        for h in &self.held {
            lo = Vec2::new(lo.x.min(h.at.x), lo.y.min(h.at.y));
            hi = Vec2::new(hi.x.max(h.at.x), hi.y.max(h.at.y));
        }
        let extent = (hi.x - lo.x).max(hi.y - lo.y);
        self.min = lo;
        self.cell = widest.max(extent.div_int(GRID_SIDE)).max(Fx::from_raw(1));
        self.cols = (hi.x - lo.x).raw() / self.cell.raw() + 1;
        self.rows = (hi.y - lo.y).raw() / self.cell.raw() + 1;
        let cells = (self.cols * self.rows) as usize;
        self.start.clear();
        self.start.resize(cells + 1, 0);
        for h in &self.held {
            let c = self.cell_of(h.at);
            self.start[c] += 1;
        }
        // The end of each cell's stretch; placing the slots from the last counts each
        // cursor down to the start of its stretch.
        let mut end = 0;
        for c in 0..cells {
            end += self.start[c];
            self.start[c] = end;
        }
        self.start[cells] = end;
        self.items.clear();
        self.items.resize(self.held.len(), 0);
        for (n, h) in self.held.iter().enumerate().rev() {
            let c = self.cell_of(h.at);
            self.start[c] -= 1;
            self.items[self.start[c] as usize] = n as u32;
        }
    }

    /// The cell of a point inside the grid.
    fn cell_of(&self, p: Vec2) -> usize {
        let gx = (p.x - self.min.x).raw() / self.cell.raw();
        let gy = (p.y - self.min.y).raw() / self.cell.raw();
        (gy * self.cols + gx) as usize
    }

    /// The held slots in the cells from `(x0, y0)` to `(x1, y1)` (cell numbers; the
    /// range is cut to the grid).
    fn within(&self, x0: i64, y0: i64, x1: i64, y1: i64) -> impl Iterator<Item = &Held> + '_ {
        let (x0, x1) = (x0.max(0), x1.min(self.cols - 1));
        let (y0, y1) = (y0.max(0), y1.min(self.rows - 1));
        // Nothing to look at when the range lies outside the grid.
        let (y0, y1) = if x0 > x1 { (1, 0) } else { (y0, y1) };
        (y0..=y1).flat_map(move |gy| {
            let row = gy * self.cols;
            let from = self.start[(row + x0) as usize] as usize;
            let to = self.start[(row + x1 + 1) as usize] as usize;
            self.items[from..to]
                .iter()
                .map(move |&n| &self.held[n as usize])
        })
    }

    /// The number of the cell a coordinate falls in, below zero and past the grid
    /// included.
    fn cell_number(&self, coordinate: Fx, from: Fx) -> i64 {
        (coordinate - from).raw().div_euclid(self.cell.raw())
    }

    /// Whether a slot at `p` would lie nearer a slot held than the two bodies keep
    /// apart.
    fn crowds(&self, p: Vec2) -> bool {
        if self.held.is_empty() {
            return false;
        }
        let (gx, gy) = (
            self.cell_number(p.x, self.min.x),
            self.cell_number(p.y, self.min.y),
        );
        self.within(gx - 1, gy - 1, gx + 1, gy + 1)
            .any(|h| (p - h.at).length_squared() < h.keep2)
    }

    /// Whether no slot held lies nearer the segment that starts at `from` and goes
    /// by `v` (`vv` is its squared length) than the two bodies keep apart.
    fn open(&self, from: Vec2, v: Vec2, vv: Fx) -> bool {
        if self.held.is_empty() {
            return true;
        }
        let (x0, x1) = (from.x.min(from.x + v.x), from.x.max(from.x + v.x));
        let (y0, y1) = (from.y.min(from.y + v.y), from.y.max(from.y + v.y));
        // A slot that matters is within a cell's width of the segment's box.
        self.within(
            self.cell_number(x0 - self.cell, self.min.x),
            self.cell_number(y0 - self.cell, self.min.y),
            self.cell_number(x1 + self.cell, self.min.x),
            self.cell_number(y1 + self.cell, self.min.y),
        )
        .all(|h| segment_distance2(h.at, from, v, vv) >= h.keep2)
    }
}

impl Field {
    /// Whether slot `k` is held: the table names a unit that is alive and says it
    /// holds this slot of this field.
    pub(crate) fn held(&self, e: &Entities, k: usize) -> bool {
        let Some(&h) = self.taken.get(k) else {
            return false;
        };
        h != NO_HOLDER
            && (h as usize) < e.len()
            && e.alive[h as usize]
            && e.crowd[h as usize].claim
                == Claim {
                    serial: self.serial,
                    slot: k as u16,
                }
    }

    /// Whether unit `i` holds a slot of this field, or shares one.
    fn has_claim(&self, e: &Entities, i: usize) -> bool {
        let c = e.crowd[i].claim;
        c.serial == self.serial
            && (c.slot == Claim::SHARED || self.taken.get(usize::from(c.slot)) == Some(&(i as u32)))
    }
}

/// The squared distance of point `b` from the segment that starts at `a` and goes by
/// `v` (`vv` is its squared length).
fn segment_distance2(b: Vec2, a: Vec2, v: Vec2, vv: Fx) -> Fx {
    let w = b - a;
    let t = w.dot(v);
    if t <= Fx::ZERO || vv <= Fx::ZERO {
        w.length_squared()
    } else if t >= vv {
        (b - (a + v)).length_squared()
    } else {
        w.cross(v).square().saturating_div(vv)
    }
}

impl Fields {
    /// The point of the slot unit `i` holds, if it holds one of a field that is still
    /// there.
    pub(crate) fn claimed_point(&self, e: &Entities, i: usize) -> Option<Vec2> {
        let c = e.crowd[i].claim;
        if c.serial == 0 || c.slot == Claim::SHARED {
            return None;
        }
        let f = &self.entries[self.find(c.serial)?];
        (f.taken.get(usize::from(c.slot)) == Some(&(i as u32)))
            .then(|| f.slots.get(usize::from(c.slot)).copied())
            .flatten()
    }

    /// Unit `i`, a member of the group of entry `fi`, takes its arrival slot if it is
    /// near enough and holds none.
    pub(crate) fn claim_if_due(
        &mut self,
        graphs: &[ClassGraph],
        e: &mut Entities,
        fi: usize,
        i: usize,
    ) {
        let f = &self.entries[fi];
        if !f.slots_ready || f.taken.len() != f.slots.len() || f.has_claim(e, i) {
            return;
        }
        let g = &graphs[f.graph];
        let range = f.reach + CLAIM_MARGIN;
        if (e.pos[i] - centre_of(g, f.goal)).length_squared() > range.square()
            || self.claims_left == 0
        {
            return;
        }
        self.claims_left -= 1;
        let pick = self.choose(g, e, fi, i);
        let f = &mut self.entries[fi];
        let slot = match pick {
            Some(k) => {
                f.taken[k] = i as u32;
                k as u16
            }
            None => Claim::SHARED,
        };
        e.crowd[i].claim = Claim {
            serial: f.serial,
            slot,
        };
    }

    /// The slot unit `i` should take: see the module documentation.
    fn choose(&mut self, g: &ClassGraph, e: &Entities, fi: usize, i: usize) -> Option<usize> {
        // The slots held by the group, of every class.
        let mut held = std::mem::take(&mut self.held);
        held.clear();
        let order = self.entries[fi].layout.order;
        let mut widest = Fx::ZERO;
        for (oi, o) in self.entries.iter().enumerate() {
            if o.serial == 0
                || !o.slots_ready
                || (oi != fi && (order == 0 || o.layout.order != order))
            {
                continue;
            }
            let keep = slots::keep_apart(self.entries[fi].layout.spacing, o.layout.spacing);
            for (k, &p) in o.slots.iter().enumerate() {
                if o.held(e, k) {
                    held.push(p, keep);
                    widest = widest.max(keep);
                }
            }
        }
        held.sort(widest);
        let f = &self.entries[fi];
        let from = e.pos[i];
        let mut nearest: Option<(Fx, usize)> = None;
        let mut first_free: Option<usize> = None;
        let mut found = None;
        for (k, &p) in f.slots.iter().enumerate() {
            if f.held(e, k) {
                continue;
            }
            // Free: clear of every slot held, of any class.
            if held.crowds(p) {
                continue;
            }
            first_free.get_or_insert(k);
            if !(los::short_enough(from, p) && los::thick_clear(&g.grid, from, p).0) {
                continue;
            }
            // Open: no slot held lies nearer the way than the two bodies keep apart.
            let v = p - from;
            let vv = v.length_squared();
            if held.open(from, v, vv) {
                found = Some(k);
                break;
            }
            if nearest.is_none_or(|(best, _)| vv < best) {
                nearest = Some((vv, k));
            }
        }
        self.held = held;
        found.or(nearest.map(|(_, k)| k)).or(first_free)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic stream of numbers for the tests.
    struct Stream(u64);

    impl Stream {
        /// A number from 0 to `n` (exclusive).
        fn below(&mut self, n: i64) -> i64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 33) % n as u64) as i64
        }

        /// A point in the square from `lo` to `hi` cells, to a hundredth.
        fn point(&mut self, lo: i64, hi: i64) -> Vec2 {
            let x = Fx::from_ratio(lo * 100 + self.below((hi - lo) * 100 + 1), 100);
            let y = Fx::from_ratio(lo * 100 + self.below((hi - lo) * 100 + 1), 100);
            Vec2::new(x, y)
        }
    }

    /// The slots held, built from `points` and `keeps`.
    fn buffer(points: &[Vec2], keeps: &[Fx]) -> HeldBuf {
        let mut held = HeldBuf::default();
        let mut widest = Fx::ZERO;
        for (&p, &k) in points.iter().zip(keeps) {
            held.push(p, k);
            widest = widest.max(k);
        }
        held.sort(widest);
        held
    }

    #[test]
    fn the_grid_answers_as_if_every_slot_held_were_looked_at() {
        let mut rnd = Stream(0x5EED);
        // Few and many slots, close together and far apart (a grid cell wider than
        // the widest keep), and all of them in one cell.
        for (count, spread) in [
            (1, 2),
            (5, 3),
            (40, 8),
            (200, 14),
            (256, 14),
            (60, 90),
            (30, 1),
        ] {
            let points: Vec<Vec2> = (0..count).map(|_| rnd.point(0, spread)).collect();
            let keeps: Vec<Fx> = (0..count)
                .map(|_| Fx::from_ratio(30 + rnd.below(111), 100))
                .collect();
            let held = buffer(&points, &keeps);
            // A point is crowded when some slot held lies nearer than it keeps.
            for _ in 0..600 {
                let p = rnd.point(-4, spread + 4);
                let brute = points
                    .iter()
                    .zip(&keeps)
                    .any(|(&q, &k)| (p - q).length_squared() < k.square());
                assert_eq!(held.crowds(p), brute, "{count} slots, point {p:?}");
            }
            // A way is open when no slot held lies nearer the segment than it keeps.
            for _ in 0..300 {
                let (from, to) = (rnd.point(-4, spread + 4), rnd.point(-4, spread + 4));
                let v = to - from;
                let vv = v.length_squared();
                let brute = points
                    .iter()
                    .zip(&keeps)
                    .all(|(&q, &k)| segment_distance2(q, from, v, vv) >= k.square());
                assert_eq!(
                    held.open(from, v, vv),
                    brute,
                    "{count} slots, {from:?} to {to:?}"
                );
            }
        }
    }

    #[test]
    fn with_no_slot_held_nothing_is_crowded_and_every_way_is_open() {
        let held = buffer(&[], &[]);
        let p = Vec2::new(Fx::from_int(3), Fx::from_int(4));
        assert!(!held.crowds(p));
        assert!(held.open(p, Vec2::new(Fx::from_int(5), Fx::ZERO), Fx::from_int(25)));
        // A buffer used again after a bigger one starts from nothing.
        let mut held = buffer(&[p], &[Fx::ONE]);
        assert!(held.crowds(p));
        held.clear();
        held.sort(Fx::ZERO);
        assert!(!held.crowds(p));
    }
}
