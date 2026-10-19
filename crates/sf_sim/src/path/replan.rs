//! Replanning (WP4.7, `docs/rules/pathfinding.md`, "Replanning"): the settings, and the
//! blocked-cell overlay a request for a new route can carry.
//!
//! The destination is where a unit wants to be; the route is how it gets there, and the
//! route is made again when it stops being the way. Four things make a unit ask:
//!
//! * a **change of the map** that touches its route (a structure placed or removed);
//!   the graph is right, the unit only has to look again (`Pathing::steer`);
//! * a **target that moved** (an attack, a guard of an entity): the re-search of WP4.2,
//!   now behind a cooldown ([`ReplanConfig::retarget_cooldown`]);
//! * a unit that is **stuck** after its sidesteps (`crowd.rs`, the hook of WP4.4): a new
//!   route from where it stands that goes round the bodies that do not move, the
//!   [`Overlay`]; at most [`ReplanConfig::max`] times in an order, with
//!   [`ReplanConfig::cooldown`] ticks between two, and then the order is given up;
//! * a **target that is gone** ends the order (`orders::run`), a **target that cannot be
//!   reached** leaves the unit at the nearest cell it can reach (WP4.2).
//!
//! The overlay is the part that is new in kind. A unit on `HoldPosition` (or an enemy
//! standing still) is no part of the graph: the graph is the terrain and the structures,
//! shared by every request, and a body that stands in a gap today and is gone tomorrow does
//! not belong there. So a stuck unit's request carries the **cells those bodies fill** with
//! it, in the route record (a fixed, sorted list of at most [`OVERLAY_CAP`] cells), and the
//! service treats them as blocked **for the searches of that request only**: the grid is
//! shown to the job with those cells shut, and the job's slice is the only time it is (the
//! cells are put back before the service goes on to anything else). The graph's portals and
//! edges are not touched; an abstract path that crosses a sector the overlay cuts is found
//! out when the sector is refined, and the request ends without a route
//! ([`Progress::Failed`](super::job::Progress)) instead of one through the body.

use sf_math::Fx;

/// The settings of the replan, game data in `assets/config/sim.ron`. A world that is
/// given none uses [`ReplanConfig::DEFAULT`], the same numbers.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct ReplanConfig {
    /// Ticks between two replans of a stuck unit (it sidesteps meanwhile).
    pub cooldown: u16,
    /// Replans a unit may ask for in one order because it is stuck; the next time it
    /// would, the order is given up (the unit's next queued order starts, or it goes
    /// idle where it stands).
    pub max: u8,
    /// Ticks between two new searches for a target that keeps moving.
    pub retarget_cooldown: u16,
}

/// The default of [`ReplanConfig::cooldown`] (5 s).
pub const DEFAULT_REPLAN_COOLDOWN: u16 = 100;
/// The default of [`ReplanConfig::max`].
pub const DEFAULT_REPLAN_MAX: u8 = 3;
/// The default of [`ReplanConfig::retarget_cooldown`] (1 s).
pub const DEFAULT_RETARGET_COOLDOWN: u16 = 20;
/// The most ticks a cooldown may be: a bound against a typing mistake.
pub const MAX_REPLAN_COOLDOWN: u16 = 1000;
/// The most replans a unit may be given in one order.
pub const MAX_REPLAN_MAX: u8 = 100;

impl ReplanConfig {
    /// The numbers of the shipped `sim.ron`.
    pub const DEFAULT: ReplanConfig = ReplanConfig {
        cooldown: DEFAULT_REPLAN_COOLDOWN,
        max: DEFAULT_REPLAN_MAX,
        retarget_cooldown: DEFAULT_RETARGET_COOLDOWN,
    };

    /// Whether a world takes these numbers: cooldowns of at least a tick and at most
    /// [`MAX_REPLAN_COOLDOWN`], and between one and [`MAX_REPLAN_MAX`] replans.
    pub const fn is_valid(&self) -> bool {
        self.cooldown >= 1
            && self.cooldown <= MAX_REPLAN_COOLDOWN
            && self.retarget_cooldown >= 1
            && self.retarget_cooldown <= MAX_REPLAN_COOLDOWN
            && self.max >= 1
            && self.max <= MAX_REPLAN_MAX
    }
}

impl Default for ReplanConfig {
    fn default() -> ReplanConfig {
        ReplanConfig::DEFAULT
    }
}

/// The most cells one request's overlay holds. A body of the biggest shipped size shuts
/// about nine cells; the nearest to the unit are kept when there are more.
pub const OVERLAY_CAP: usize = 32;

/// How far from the stuck unit a body is looked for, in cells.
pub const OVERLAY_RANGE: Fx = Fx::from_int(8);

/// Clearance added to the two radii when the cells a body fills are worked out, in cells.
pub const OVERLAY_MARGIN: Fx = Fx::from_ratio(1, 4);

/// The cells one request treats as blocked: row-major indices of the class's grid in
/// ascending order, at most [`OVERLAY_CAP`] of them.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Overlay {
    cells: [u32; OVERLAY_CAP],
    len: u8,
}

impl Overlay {
    pub const EMPTY: Overlay = Overlay {
        cells: [0; OVERLAY_CAP],
        len: 0,
    };

    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The cells, ascending.
    pub fn cells(&self) -> &[u32] {
        &self.cells[..self.len()]
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Whether `cell` is in the overlay.
    #[cfg(test)]
    pub fn contains(&self, cell: u32) -> bool {
        self.cells().binary_search(&cell).is_ok()
    }
}

impl PartialEq for Overlay {
    fn eq(&self, o: &Overlay) -> bool {
        self.cells() == o.cells()
    }
}

impl Eq for Overlay {}

/// A builder of an [`Overlay`] that keeps the [`OVERLAY_CAP`] cells nearest to a point
/// (the squared distance, then the lower cell index): offered cells in any order, the
/// result does not depend on it.
pub(crate) struct OverlayBuilder {
    /// `(squared distance to the unit, cell)`, ascending.
    best: [(Fx, u32); OVERLAY_CAP],
    len: usize,
}

impl OverlayBuilder {
    pub fn new() -> OverlayBuilder {
        OverlayBuilder {
            best: [(Fx::ZERO, 0); OVERLAY_CAP],
            len: 0,
        }
    }

    /// Offers a cell at squared distance `d2` from the unit. A cell offered twice
    /// counts once.
    pub fn offer(&mut self, d2: Fx, cell: u32) {
        let key = (d2, cell);
        if self.best[..self.len].iter().any(|&(_, c)| c == cell) {
            return;
        }
        if self.len == OVERLAY_CAP {
            if key >= self.best[OVERLAY_CAP - 1] {
                return;
            }
            self.len -= 1;
        }
        let at = self.best[..self.len].partition_point(|&k| k < key);
        self.best.copy_within(at..self.len, at + 1);
        self.best[at] = key;
        self.len += 1;
    }

    /// The overlay: the cells kept, ascending by index.
    pub fn finish(self) -> Overlay {
        let mut overlay = Overlay::EMPTY;
        for (k, &(_, cell)) in self.best[..self.len].iter().enumerate() {
            overlay.cells[k] = cell;
        }
        overlay.len = self.len as u8;
        overlay.cells[..self.len].sort_unstable();
        overlay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_valid_and_bad_numbers_are_not() {
        assert!(ReplanConfig::DEFAULT.is_valid());
        let with = |cooldown, max, retarget_cooldown| ReplanConfig {
            cooldown,
            max,
            retarget_cooldown,
        };
        assert!(!with(0, 3, 20).is_valid());
        assert!(!with(100, 0, 20).is_valid());
        assert!(!with(100, 3, 0).is_valid());
        assert!(!with(MAX_REPLAN_COOLDOWN + 1, 3, 20).is_valid());
        assert!(!with(100, MAX_REPLAN_MAX + 1, 20).is_valid());
        assert!(with(1, 1, 1).is_valid());
    }

    #[test]
    fn the_builder_keeps_the_nearest_cells_whatever_the_order_they_are_offered_in() {
        let offers: Vec<(Fx, u32)> = (0..100_u32)
            .map(|k| (Fx::from_int(((k * 37) % 41) as i32), 1000 + k))
            .collect();
        let build = |order: &mut dyn Iterator<Item = &(Fx, u32)>| {
            let mut b = OverlayBuilder::new();
            for &(d, c) in order {
                b.offer(d, c);
                // Offering again changes nothing.
                b.offer(d, c);
            }
            b.finish()
        };
        let a = build(&mut offers.iter());
        let b = build(&mut offers.iter().rev());
        assert_eq!(a, b);
        assert_eq!(a.len(), OVERLAY_CAP);
        assert!(a.cells().windows(2).all(|w| w[0] < w[1]), "ascending");
        let mut sorted = offers.clone();
        sorted.sort();
        for &(_, c) in &sorted[..OVERLAY_CAP] {
            assert!(a.contains(c));
        }
    }

    #[test]
    fn a_small_overlay_is_all_of_what_was_offered() {
        let mut b = OverlayBuilder::new();
        b.offer(Fx::from_int(4), 9);
        b.offer(Fx::from_int(1), 30);
        b.offer(Fx::from_int(2), 5);
        let o = b.finish();
        assert_eq!(o.cells(), &[5, 9, 30]);
        assert!(!o.is_empty());
        assert_eq!(Overlay::EMPTY.len(), 0);
    }
}
