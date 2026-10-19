//! The route a unit keeps: bounded storage, nothing allocated.
//!
//! A route is the *how* of an order (the order holds the *where*, see
//! `docs/rules/orders.md`). It lives in the unit's own `route` column, is
//! reset whenever the order changes, and has two fixed-size parts:
//!
//! * the **ring** of the next waypoints, already refined to cells and pulled
//!   taut: the unit steers at the front one and takes it off when it arrives;
//! * the **abstract path** still to refine: the portal nodes of the sectors the
//!   route goes through, from which the next legs are refined lazily when the
//!   ring runs low.
//!
//! Both are rings of fixed capacity ([`RING_CAP`], [`ABS_CAP`]); a plan whose
//! abstract path is longer than [`ABS_CAP`] keeps the first part and is planned
//! again from where the unit is when that has been used up
//! ([`Route::truncated`]).

use core::ops::{Index, IndexMut};

use sf_math::hash::Fnv1a64;

use crate::path::replan::Overlay;

/// Waypoints the ring holds.
pub const RING_CAP: usize = 8;

/// Portal nodes of the abstract path a route keeps.
pub const ABS_CAP: usize = 64;

/// The ring refills when it holds this many waypoints or fewer.
pub(crate) const REFILL_AT: usize = 2;

/// What a unit's route is doing.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum RouteState {
    /// No route: the unit has no walking order, or has not asked yet.
    Idle,
    /// A route is asked for and not finished: queued behind others, or being
    /// worked on. The unit follows the waypoints it already has and, when it
    /// has none, waits where it stands.
    Wanted,
    /// A route exists. It may still be refined further (the ring refills as the
    /// unit walks).
    Active,
    /// The unit stands at the end of its route and the order's destination is
    /// not reached: the destination cannot be reached (the route led to the
    /// nearest cell, [`Route::unreachable`]), or the unit was refused a step on
    /// its route twice. The order is kept.
    Stranded,
}

impl RouteState {
    const fn tag(self) -> u8 {
        match self {
            RouteState::Idle => 0,
            RouteState::Wanted => 1,
            RouteState::Active => 2,
            RouteState::Stranded => 3,
        }
    }
}

/// A unit's route. Plain data, `Copy`, the same size for every unit. Two routes
/// are equal when they hold the same waypoints and portals in the same order,
/// wherever the rings happen to start.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Route {
    pub state: RouteState,
    /// The destination cannot be reached: the route leads to the reachable
    /// cell nearest to it ([`Route::goal`]).
    pub unreachable: bool,
    /// Every leg of the route is refined: the abstract path is used up and the
    /// ring ends at the goal. What follows the ring is the destination itself.
    pub complete: bool,
    /// The plan has more portals than [`ABS_CAP`]: when they are used up the
    /// route is planned again from where the unit stands.
    pub truncated: bool,
    /// Smoothing is off: the route follows cell to cell. Set after a step on a
    /// smoothed route was refused.
    pub cautious: bool,
    /// Numbers a request; a job working on an older number is dropped.
    pub ticket: u32,
    /// The graph epoch the route was made in (see `Pathing`).
    pub epoch: u32,
    /// The cell the destination was in when the route was asked for.
    pub want: u32,
    /// The cell the route leads to: `want`, or the nearest reachable cell.
    pub goal: u32,
    /// The cell the refined route ends in: the last waypoint of the ring, or
    /// where the unit started when the ring is empty.
    pub frontier: u32,
    /// The flowfield this unit follows (WP4.3): the serial of its group's
    /// field, zero for a unit that is routed on its own. A member keeps the
    /// state `Active` and `complete`, so the service never plans a route for it;
    /// the ring then holds at most the one waypoint it steers at.
    pub field_serial: u32,
    /// The unit's place among the members of its group, by ascending entity
    /// id: which arrival slot is its own.
    pub field_rank: u16,
    /// A field member whose next step is straight on to its arrival slot.
    pub final_leg: bool,
    /// The cells this request treats as blocked (WP4.7): the bodies that do not
    /// move round a stuck unit. Empty for every route but the one a stuck unit
    /// asks for; cleared when that request is over.
    pub overlay: Overlay,
    ring: [u32; RING_CAP],
    ring_head: u8,
    ring_len: u8,
    abs: [u32; ABS_CAP],
    abs_head: u8,
    abs_len: u8,
}

impl PartialEq for Route {
    fn eq(&self, o: &Route) -> bool {
        self.state == o.state
            && self.unreachable == o.unreachable
            && self.complete == o.complete
            && self.truncated == o.truncated
            && self.cautious == o.cautious
            && self.ticket == o.ticket
            && self.epoch == o.epoch
            && self.want == o.want
            && self.goal == o.goal
            && self.frontier == o.frontier
            && self.field_serial == o.field_serial
            && self.field_rank == o.field_rank
            && self.final_leg == o.final_leg
            && self.overlay == o.overlay
            && self.ring_len() == o.ring_len()
            && (0..self.ring_len()).all(|k| self.ring_get(k) == o.ring_get(k))
            && self.abs_len() == o.abs_len()
            && (0..self.abs_len()).all(|k| self.abs_get(k) == o.abs_get(k))
    }
}

impl Eq for Route {}

impl Route {
    /// A unit with no route.
    pub const IDLE: Route = Route {
        state: RouteState::Idle,
        unreachable: false,
        complete: false,
        truncated: false,
        cautious: false,
        ticket: 0,
        epoch: 0,
        want: 0,
        goal: 0,
        frontier: 0,
        field_serial: 0,
        field_rank: 0,
        final_leg: false,
        overlay: Overlay::EMPTY,
        ring: [0; RING_CAP],
        ring_head: 0,
        ring_len: 0,
        abs: [0; ABS_CAP],
        abs_head: 0,
        abs_len: 0,
    };

    // ---- the ring ---------------------------------------------------------

    pub fn ring_len(&self) -> usize {
        usize::from(self.ring_len)
    }

    /// The waypoint `k` places from the front.
    pub fn ring_get(&self, k: usize) -> Option<u32> {
        (k < self.ring_len()).then(|| self.ring[(usize::from(self.ring_head) + k) % RING_CAP])
    }

    pub fn ring_front(&self) -> Option<u32> {
        self.ring_get(0)
    }

    pub fn ring_back(&self) -> Option<u32> {
        self.ring_len()
            .checked_sub(1)
            .and_then(|k| self.ring_get(k))
    }

    /// Adds a waypoint at the back; false if the ring is full.
    pub fn ring_push(&mut self, cell: u32) -> bool {
        if self.ring_len() == RING_CAP {
            return false;
        }
        let at = (usize::from(self.ring_head) + self.ring_len()) % RING_CAP;
        self.ring[at] = cell;
        self.ring_len += 1;
        true
    }

    pub fn ring_pop_front(&mut self) -> Option<u32> {
        let cell = self.ring_front()?;
        self.ring_head = ((usize::from(self.ring_head) + 1) % RING_CAP) as u8;
        self.ring_len -= 1;
        Some(cell)
    }

    pub fn ring_pop_back(&mut self) -> Option<u32> {
        let cell = self.ring_back()?;
        self.ring_len -= 1;
        Some(cell)
    }

    pub fn ring_clear(&mut self) {
        self.ring_head = 0;
        self.ring_len = 0;
    }

    // ---- the abstract path --------------------------------------------------

    pub fn abs_len(&self) -> usize {
        usize::from(self.abs_len)
    }

    /// The portal node `k` places from the front.
    pub fn abs_get(&self, k: usize) -> Option<u32> {
        (k < self.abs_len()).then(|| self.abs[(usize::from(self.abs_head) + k) % ABS_CAP])
    }

    pub fn abs_front(&self) -> Option<u32> {
        self.abs_get(0)
    }

    pub fn abs_push(&mut self, node: u32) -> bool {
        if self.abs_len() == ABS_CAP {
            return false;
        }
        let at = (usize::from(self.abs_head) + self.abs_len()) % ABS_CAP;
        self.abs[at] = node;
        self.abs_len += 1;
        true
    }

    pub fn abs_pop_front(&mut self) -> Option<u32> {
        let node = self.abs_front()?;
        self.abs_head = ((usize::from(self.abs_head) + 1) % ABS_CAP) as u8;
        self.abs_len -= 1;
        Some(node)
    }

    pub fn abs_clear(&mut self) {
        self.abs_head = 0;
        self.abs_len = 0;
    }

    /// Forgets the plan, keeping nothing: what a new order does.
    pub fn reset(&mut self) {
        *self = Route::IDLE;
    }

    /// Forgets the waypoints and the abstract path, keeping the request.
    pub fn clear_plan(&mut self) {
        self.ring_clear();
        self.abs_clear();
        self.complete = false;
        self.truncated = false;
        self.unreachable = false;
    }

    /// Writes the route into a hash, the part of it the future depends on.
    pub(crate) fn write_hash(&self, h: &mut Fnv1a64) {
        h.write_u8(self.state.tag());
        h.write_u8(
            u8::from(self.unreachable)
                | u8::from(self.complete) << 1
                | u8::from(self.truncated) << 2
                | u8::from(self.cautious) << 3,
        );
        h.write_u32(self.ticket);
        h.write_u32(self.epoch);
        h.write_u32(self.want);
        h.write_u32(self.goal);
        h.write_u32(self.frontier);
        h.write_u8(self.ring_len);
        for k in 0..self.ring_len() {
            h.write_u32(self.ring_get(k).unwrap_or(0));
        }
        h.write_u8(self.abs_len);
        for k in 0..self.abs_len() {
            h.write_u32(self.abs_get(k).unwrap_or(0));
        }
        // A flowfield member (WP4.3). Nothing is written for the others, so a
        // world that never made a field hashes as before.
        if self.field_serial != 0 {
            h.write_u8(0xF1);
            h.write_u32(self.field_serial);
            h.write_u32(u32::from(self.field_rank));
            h.write_u8(u8::from(self.final_leg));
        }
        // The overlay of a request (WP4.7); nothing for the routes without one.
        if !self.overlay.is_empty() {
            h.write_u8(0xF2);
            h.write_u8(self.overlay.len() as u8);
            for &c in self.overlay.cells() {
                h.write_u32(c);
            }
        }
    }
}

/// What a slot without a route holds.
static IDLE_ROUTE: Route = Route::IDLE;

/// The routes of all slots, kept apart from the other columns because a route is
/// big (a few hundred bytes) and most slots never have one: the vector grows
/// only as far as the highest slot that has been asked for a route, and a slot
/// beyond it reads as [`Route::IDLE`]. Two sets are equal when every slot has
/// an equal route, however long the vectors are.
#[derive(Clone, Default)]
pub(crate) struct Routes(Vec<Route>);

impl core::fmt::Debug for Routes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Routes({} slots)", self.0.len())
    }
}

impl Routes {
    /// The number of slots that may have a route: every slot from here on is idle.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Makes slot `i` idle (no growth).
    pub fn reset(&mut self, i: usize) {
        if let Some(r) = self.0.get_mut(i) {
            r.reset();
        }
    }

    /// The slots whose route is not idle, ascending.
    pub fn iter_active(&self) -> impl Iterator<Item = (usize, &Route)> {
        self.0
            .iter()
            .enumerate()
            .filter(|(_, r)| r.state != RouteState::Idle)
    }
}

impl Index<usize> for Routes {
    type Output = Route;
    fn index(&self, i: usize) -> &Route {
        self.0.get(i).unwrap_or(&IDLE_ROUTE)
    }
}

impl IndexMut<usize> for Routes {
    fn index_mut(&mut self, i: usize) -> &mut Route {
        if self.0.len() <= i {
            self.0.resize(i + 1, Route::IDLE);
        }
        &mut self.0[i]
    }
}

impl PartialEq for Routes {
    fn eq(&self, other: &Routes) -> bool {
        (0..self.0.len().max(other.0.len())).all(|i| self[i] == other[i])
    }
}

impl Eq for Routes {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_is_first_in_first_out_bounded_and_wraps() {
        let mut r = Route::IDLE;
        for c in 0..RING_CAP as u32 {
            assert!(r.ring_push(c));
        }
        assert!(!r.ring_push(99), "full");
        assert_eq!(r.ring_pop_front(), Some(0));
        assert!(r.ring_push(100));
        assert_eq!(r.ring_len(), RING_CAP);
        assert_eq!(r.ring_back(), Some(100));
        assert_eq!(r.ring_pop_back(), Some(100));
        let rest: Vec<u32> = std::iter::from_fn(|| r.ring_pop_front()).collect();
        assert_eq!(rest, (1..RING_CAP as u32).collect::<Vec<_>>());
        assert_eq!(r.ring_front(), None);
    }

    #[test]
    fn the_abstract_path_is_bounded_and_wraps() {
        let mut r = Route::IDLE;
        for n in 0..ABS_CAP as u32 {
            assert!(r.abs_push(n * 10));
        }
        assert!(!r.abs_push(1));
        assert_eq!(r.abs_pop_front(), Some(0));
        assert_eq!(r.abs_pop_front(), Some(10));
        assert!(r.abs_push(7));
        assert_eq!(r.abs_get(ABS_CAP - 2), Some(7));
        assert_eq!(r.abs_front(), Some(20));
    }

    #[test]
    fn the_hash_ignores_where_the_ring_starts() {
        let mut a = Route::IDLE;
        let mut b = Route::IDLE;
        a.ring_push(1);
        a.ring_push(2);
        a.ring_pop_front();
        b.ring_push(2);
        let (mut ha, mut hb) = (Fnv1a64::new(), Fnv1a64::new());
        a.write_hash(&mut ha);
        b.write_hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    #[test]
    fn routes_grow_where_they_are_used_and_equal_ones_are_equal_whatever_the_length() {
        let mut a = Routes::default();
        let b = Routes::default();
        assert_eq!(a[5000], Route::IDLE);
        assert_eq!(a.len(), 0, "a read does not grow it");
        a[3].state = RouteState::Active;
        assert_eq!(a.len(), 4);
        assert!(a != b);
        a.reset(3);
        a.reset(900);
        assert_eq!(a.len(), 4, "a reset does not grow it");
        assert!(a == b, "only idle routes: equal to none");
        a[2].state = RouteState::Wanted;
        assert_eq!(a.iter_active().map(|(i, _)| i).collect::<Vec<_>>(), [2]);
    }

    #[test]
    fn a_reset_forgets_everything() {
        let mut r = Route::IDLE;
        r.state = RouteState::Active;
        r.ring_push(3);
        r.abs_push(4);
        r.unreachable = true;
        r.reset();
        assert_eq!(r, Route::IDLE);
    }
}
