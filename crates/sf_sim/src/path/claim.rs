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

/// The most free slots one claim looks at.
const LOOK: usize = 400;

/// Units that may take a slot in one tick.
pub(crate) const CLAIMS_PER_TICK: u32 = 16;

/// The slots held, gathered for one claim: position and spacing of the lattice.
/// Scratch memory: two of them are always equal.
#[derive(Clone, Debug, Default)]
pub(crate) struct HeldBuf(Vec<(Vec2, Fx)>);

impl PartialEq for HeldBuf {
    fn eq(&self, _: &HeldBuf) -> bool {
        true
    }
}

impl Eq for HeldBuf {}

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
        let mut held = std::mem::take(&mut self.held.0);
        held.clear();
        let order = self.entries[fi].layout.order;
        for (oi, o) in self.entries.iter().enumerate() {
            if o.serial == 0
                || !o.slots_ready
                || (oi != fi && (order == 0 || o.layout.order != order))
            {
                continue;
            }
            for (k, &p) in o.slots.iter().enumerate() {
                if o.held(e, k) {
                    held.push((p, o.layout.spacing));
                }
            }
        }
        let f = &self.entries[fi];
        let from = e.pos[i];
        let keep = |other: Fx| slots::keep_apart(f.layout.spacing, other);
        let mut nearest: Option<(Fx, usize)> = None;
        let mut first_free: Option<usize> = None;
        let mut looked = 0;
        let mut found = None;
        for (k, &p) in f.slots.iter().enumerate() {
            if f.held(e, k) {
                continue;
            }
            // Free: clear of every slot held, of any class.
            if held
                .iter()
                .any(|&(q, other)| (p - q).length_squared() < keep(other).square())
            {
                continue;
            }
            first_free.get_or_insert(k);
            looked += 1;
            if looked > LOOK {
                break;
            }
            if !(los::short_enough(from, p) && los::thick_clear(&g.grid, from, p).0) {
                continue;
            }
            // Open: no slot held lies nearer the way than the two bodies keep apart.
            let v = p - from;
            let vv = v.length_squared();
            let open = held
                .iter()
                .all(|&(q, other)| segment_distance2(q, from, v, vv) >= keep(other).square());
            if open {
                found = Some(k);
                break;
            }
            if nearest.is_none_or(|(best, _)| vv < best) {
                nearest = Some((vv, k));
            }
        }
        self.held.0 = held;
        found.or(nearest.map(|(_, k)| k)).or(first_free)
    }
}
