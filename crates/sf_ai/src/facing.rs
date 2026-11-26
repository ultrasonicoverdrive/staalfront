//! Direction-free orderings (WP9.6a, `docs/rules/ai.md`, "Direction-free rules").
//!
//! Three rules of the AI broke a tie by the compass: the placement search ordered equal scores by `(y, x)`, the
//! gather search took the cells of a ring by `(y, x)`, and the scout visited the quarters north-west first. On a
//! map whose two start sites are a half turn of each other that favours the site that happens to be in the
//! north. Each rule has a profile switch (default off) that replaces the compass by a [`Facing`]: the start site of
//! the AI itself, and the direction to the start site of its enemy.
//!
//! All numbers are integers, in **doubled** coordinates: the centre of a cell `(x, y)` is `(2x + 1, 2y + 1)`, the
//! middle of a footprint at `(x, y)` of `w` by `h` cells is `(2x + w, 2y + h)`, the middle of the map is
//! `(width, height)`. A half turn of the map about its middle maps a doubled point `p` onto `(2 * width - p.x,
//! 2 * height - p.y)` and a cell onto `(width - 1 - x, height - 1 - y)`; it turns the direction to the enemy round
//! with the rest, so everything that is computed only from `dot`, `|cross|` and `cross` of a point against the
//! direction (see [`Facing::rank`]) is the same for the two players of a map that a half turn maps onto itself.

use sf_sim::Cell;

use crate::state::AiState;

/// The doubled centre of a cell.
pub fn centre(c: Cell) -> (i64, i64) {
    (2 * i64::from(c.x) + 1, 2 * i64::from(c.y) + 1)
}

/// The start site of the AI itself and the direction to its enemy, in doubled coordinates.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Facing {
    /// The AI's own start site (the centre of its cell).
    pub origin: (i64, i64),
    /// From the origin to the target: the nearest start site of another player (the lowest player id among equal
    /// distances), or the middle of the map when there is no other site. Never `(0, 0)`.
    pub dir: (i64, i64),
}

impl Facing {
    /// The facing of `state`'s player on a map of `map` cells; `None` when the AI has no start site or the
    /// target lies on it (the callers then keep the compass order).
    pub fn of(state: &AiState, map: (u32, u32)) -> Option<Facing> {
        let own = state
            .start_sites
            .iter()
            .find(|(p, _)| *p == state.player)
            .map(|&(_, c)| centre(c))?;
        let dist = |p: (i64, i64)| {
            let (dx, dy) = (p.0 - own.0, p.1 - own.1);
            dx * dx + dy * dy
        };
        let target = state
            .start_sites
            .iter()
            .filter(|(p, _)| *p != state.player)
            .map(|&(_, c)| centre(c))
            .min_by_key(|&p| dist(p)) // the first of equal distances: ascending player id
            .unwrap_or((i64::from(map.0), i64::from(map.1)));
        let dir = (target.0 - own.0, target.1 - own.1);
        (dir != (0, 0)).then_some(Facing { origin: own, dir })
    }

    /// The order of a doubled point: first the one farthest toward the enemy (largest `dot` of its offset from the
    /// origin with `dir`), then the one nearest to the line to the enemy (smallest `|cross|`), then by the sign of
    /// `cross`. `dot` and `cross` do not change when the whole situation is turned half a turn, and the map from
    /// a point to `(dot, cross)` is one-to-one while `dir` is not zero, so two different points never rank
    /// equal. (`-dot` is returned so that smaller sorts first.)
    pub fn rank(&self, p: (i64, i64)) -> (i64, i64, i64) {
        let (vx, vy) = (p.0 - self.origin.0, p.1 - self.origin.1);
        let dot = self.dir.0 * vx + self.dir.1 * vy;
        let cross = self.dir.0 * vy - self.dir.1 * vx;
        (-dot, cross.abs(), cross)
    }

    /// The target the direction points at: a doubled point.
    pub fn target(&self) -> (i64, i64) {
        (self.origin.0 + self.dir.0, self.origin.1 + self.dir.1)
    }
}

/// The squared distance between two doubled points.
pub fn dist2(a: (i64, i64), b: (i64, i64)) -> i64 {
    let (dx, dy) = (a.0 - b.0, a.1 - b.1);
    dx * dx + dy * dy
}

/// The cell that holds the doubled coordinate `p` on one axis. An odd `p` is inside one cell. An even `p` is a
/// border between two cells: the one on the side of `from` (the anchor's doubled coordinate) is taken, and when
/// `from` is on the border too, the one on the side of the map middle, `toward` (its sign). Both choices turn
/// round with a half turn of the map, which the plain integer division does not.
fn cell_on_axis(p: i64, from: i64, toward: i64) -> i64 {
    if p % 2 != 0 {
        return p.div_euclid(2);
    }
    let high = p / 2;
    match from.cmp(&p) {
        std::cmp::Ordering::Less => high - 1,
        std::cmp::Ordering::Greater => high,
        std::cmp::Ordering::Equal => {
            if toward < 0 {
                high - 1
            } else {
                high
            }
        }
    }
}

/// The first cell of the gather search with `gather_facing` on: the same point as before (`dist` cells from the
/// middle of the anchor toward the middle of the map along the longer axis, the other axis in proportion), worked
/// out in doubled coordinates and rounded toward the anchor, so that it turns round with the map. `anchor` is the
/// anchor's cell and `(fw, fh)` its footprint. `None` when the anchor is on the middle of the map.
pub fn gather_point(
    anchor: (i64, i64),
    (fw, fh): (i64, i64),
    map: (i64, i64),
    dist: i64,
) -> Option<(i64, i64)> {
    let c2 = (2 * anchor.0 + fw, 2 * anchor.1 + fh);
    let d = (map.0 - c2.0, map.1 - c2.1);
    if d == (0, 0) {
        return None;
    }
    let (px, py) = if d.0.abs() >= d.1.abs() {
        (
            c2.0 + d.0.signum() * 2 * dist,
            c2.1 + d.1 * 2 * dist / d.0.abs(),
        )
    } else {
        (
            c2.0 + d.0 * 2 * dist / d.1.abs(),
            c2.1 + d.1.signum() * 2 * dist,
        )
    };
    let x = cell_on_axis(px, c2.0, d.0);
    let y = cell_on_axis(py, c2.1, d.1);
    Some((x.clamp(0, map.0 - 1), y.clamp(0, map.1 - 1)))
}

/// The cells of the ring `ring` (Chebyshev distance) round `first`, in the order the gather search tries them: by
/// `(y, x)` as before, or, with a `facing`, by [`Facing::rank`] of their centres.
pub fn ring_cells(first: (i64, i64), ring: i64, facing: Option<&Facing>) -> Vec<(i64, i64)> {
    let mut cells = Vec::new();
    for y in first.1 - ring..=first.1 + ring {
        for x in first.0 - ring..=first.0 + ring {
            if (x - first.0).abs().max((y - first.1).abs()) == ring {
                cells.push((x, y));
            }
        }
    }
    if let Some(f) = facing {
        cells.sort_unstable_by_key(|&(x, y)| f.rank((2 * x + 1, 2 * y + 1)));
    }
    cells
}

/// The four quarter centres of a map in the order the scout visits them after the start sites. Without a
/// `facing` the old order: north-west, north-east, south-west, south-east, at `size * 1/4` and `size * 3/4`. With
/// one the centres sit at `size / 4` and `size - 1 - size / 4` (the same cells when the size is a multiple of four,
/// minus one at the far side, so that a half turn maps the set onto itself) and are taken nearest first to
/// `from`, the last site leg (the start site of the AI itself when it has none), ties by [`Facing::rank`].
pub fn quarters(map: (u32, u32), facing: Option<(&Facing, (i64, i64))>) -> Vec<Cell> {
    let (w, h) = map;
    let Some((f, from)) = facing else {
        return [(1, 1), (3, 1), (1, 3), (3, 3)]
            .iter()
            .map(|&(qx, qy)| Cell::new(w * qx / 4, h * qy / 4))
            .collect();
    };
    let at = |q: u32, size: u32| {
        if q == 1 {
            size / 4
        } else {
            size - 1 - size / 4
        }
    };
    let mut cells: Vec<Cell> = [(1, 1), (3, 1), (1, 3), (3, 3)]
        .iter()
        .map(|&(qx, qy)| Cell::new(at(qx, w), at(qy, h)))
        .collect();
    cells.sort_by_key(|&c| (dist2(centre(c), from), f.rank(centre(c))));
    cells
}

#[cfg(test)]
mod tests;
