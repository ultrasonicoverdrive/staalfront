//! Where the AI builds (WP9.1b-1, `docs/rules/ai.md`, "Placement").
//!
//! The search narrows the candidates with what the player knows and leaves the verdict to the
//! simulation: only an `Ok` of [`Lens::placement`] (the simulation's own rule, asked for visible ground
//! only) lets a build be sent. The narrowing is no second copy of the rule; it reads the same terrain flag
//! and keeps the AI's own spacing on top.
//!
//! * **Window.** Every anchor cell within the build reach of an own standing anchor: anchor x in
//!   `[ax - r - w, ax + aw + r]`, the same for y, clipped to the map (`r` the anchor's `build_radius`).
//! * **Mask.** A cell may carry the footprint when it is buildable, visible now, free of any structure the
//!   player knows (its own, and others' that are visible), free of any unit it sees (its own and visible
//!   ones), outside the **apron** of an own refinery or factory, and outside the **lane** round every own
//!   structure (`lane` free cells between two structures). An apron is `(width, depth)` cells out from the
//!   exit cell, centred on it (`refinery_apron`, `factory_apron`). A refinery or factory also needs its own
//!   apron on the map, open ground, visible and free of structures.
//! * **Refused.** `(kind, cell)` pairs the simulation refused recently are left out.
//! * **Score**, lowest first, ties by `(y, x)`, squared distances in half cells: a refinery by its exit
//!   cell to the nearest ore ground of the map (terrain, listed again at every search); a defence by
//!   its middle to the nearest start site of another player; everything else by its middle to the middle of
//!   the nearest own anchor. Without ore or other sites the anchor rule applies.
//! * **Preview.** Best first, at most `placement_cap` questions; a `Some(Err)` goes into `refused`; the
//!   first `Some(Ok)` wins. None: the caller blocks the kind.

use sf_sim::{BuildingFunction, Cell, Footprint, Kind, Side, Tick, exit_cell, gap};

use crate::base;
use crate::observe::{Lens, Observation};
use crate::state::AiState;

/// How far beyond the window the ore ground is listed, in cells.
pub const ORE_MARGIN: i64 = 16;

/// An own standing anchor: `(x, y, width, height, build_radius)`.
type Anchor = (i64, i64, i64, i64, i64);

fn anchors(obs: &Observation, lens: &Lens<'_>) -> Vec<Anchor> {
    obs.own
        .iter()
        .filter(|o| o.standing)
        .filter_map(|o| {
            let fp = o.footprint?;
            let b = lens.building(o.kind)?;
            (b.function == BuildingFunction::Anchor).then_some((
                i64::from(o.cell.x),
                i64::from(o.cell.y),
                i64::from(fp.width),
                i64::from(fp.height),
                i64::from(b.build_radius),
            ))
        })
        .collect()
}

/// The cells of an apron `(width, depth)` in front of the exit of a footprint anchored on `anchor`: the exit
/// cell ([`exit_cell`]) and the cells beyond it, `width` wide, centred on it.
pub fn apron(
    anchor: Cell,
    fp: Footprint,
    side: Side,
    (width, depth): (u8, u8),
) -> impl Iterator<Item = (i64, i64)> {
    let (ex, ey) = exit_cell(anchor, fp, side);
    let half = i64::from(width / 2);
    (0..i64::from(depth)).flat_map(move |d| {
        (-half..=half).map(move |k| match side {
            Side::PosX => (ex + d, ey + k),
            Side::NegX => (ex - d, ey + k),
            Side::PosY => (ex + k, ey + d),
            Side::NegY => (ex + k, ey - d),
        })
    })
}

/// The apron size of a structure function, if it has one.
fn apron_size(state: &AiState, function: BuildingFunction) -> Option<(u8, u8)> {
    let e = &state.profile.economy;
    if function == BuildingFunction::HarvesterDock {
        Some(e.refinery_apron)
    } else if function.is_factory() {
        Some(e.factory_apron)
    } else {
        None
    }
}

/// A rectangle of the map with three flags per cell.
struct Mask {
    x0: i64,
    y0: i64,
    w: i64,
    h: i64,
    /// The footprint may not cover the cell.
    foot: Vec<bool>,
    /// An apron may not cover the cell.
    apron: Vec<bool>,
}

impl Mask {
    fn index(&self, x: i64, y: i64) -> Option<usize> {
        let (dx, dy) = (x - self.x0, y - self.y0);
        (dx >= 0 && dy >= 0 && dx < self.w && dy < self.h).then(|| (dy * self.w + dx) as usize)
    }

    fn block_foot(&mut self, x: i64, y: i64) {
        if let Some(i) = self.index(x, y) {
            self.foot[i] = true;
        }
    }

    fn block_both(&mut self, x: i64, y: i64) {
        if let Some(i) = self.index(x, y) {
            self.foot[i] = true;
            self.apron[i] = true;
        }
    }

    fn foot_ok(&self, x: i64, y: i64) -> bool {
        self.index(x, y).is_some_and(|i| !self.foot[i])
    }

    fn apron_ok(&self, x: i64, y: i64) -> bool {
        self.index(x, y).is_some_and(|i| !self.apron[i])
    }
}

fn cell(x: i64, y: i64) -> Cell {
    Cell::new(x as u32, y as u32)
}

/// The ore ground round the anchors (map knowledge, the terrain only), ascending by (y, x). Listed again at
/// every search, so it follows the anchors the player has now (a few thousand terrain lookups).
fn ore_cells(lens: &Lens<'_>, anchors: &[Anchor], map: (i64, i64)) -> Vec<Cell> {
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for &(ax, ay, aw, ah, r) in anchors {
        x0 = x0.min(ax - r - ORE_MARGIN);
        y0 = y0.min(ay - r - ORE_MARGIN);
        x1 = x1.max(ax + aw + r + ORE_MARGIN);
        y1 = y1.max(ay + ah + r + ORE_MARGIN);
    }
    let mut cells = Vec::new();
    for y in y0.max(0)..=y1.min(map.1 - 1) {
        for x in x0.max(0)..=x1.min(map.0 - 1) {
            if lens.harvestable(cell(x, y)) {
                cells.push(cell(x, y));
            }
        }
    }
    cells
}

fn nearest(from: (i64, i64), to: impl Iterator<Item = (i64, i64)>) -> Option<u64> {
    to.map(|(x, y)| {
        let (dx, dy) = (from.0 - x, from.1 - y);
        (dx * dx + dy * dy) as u64
    })
    .min()
}

/// A place for `kind`: the best visible candidate the simulation says yes to, or `None`. Refused previews
/// go into `state.refused`.
pub fn search(
    state: &mut AiState,
    obs: &Observation,
    lens: &mut Lens<'_>,
    kind: Kind,
) -> Option<Cell> {
    let b = lens.building(kind)?;
    let fp = b.footprint;
    let (w, h) = (i64::from(fp.width), i64::from(fp.height));
    let map = (i64::from(obs.map.0), i64::from(obs.map.1));
    let anchors = anchors(obs, lens);
    if anchors.is_empty() || w > map.0 || h > map.1 {
        return None;
    }

    // The window of anchor cells.
    let (mut wx0, mut wy0, mut wx1, mut wy1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for &(ax, ay, aw, ah, r) in &anchors {
        wx0 = wx0.min(ax - r - w);
        wy0 = wy0.min(ay - r - h);
        wx1 = wx1.max(ax + aw + r);
        wy1 = wy1.max(ay + ah + r);
    }
    let (wx0, wy0) = (wx0.max(0), wy0.max(0));
    let (wx1, wy1) = (wx1.min(map.0 - w), wy1.min(map.1 - h));
    if wx0 > wx1 || wy0 > wy1 {
        return None;
    }

    // The mask, over the window grown by the footprint and the widest apron.
    let e = state.profile.economy;
    let lane = i64::from(e.lane);
    let pad = [e.refinery_apron, e.factory_apron]
        .iter()
        .map(|&(aw, ad)| i64::from(ad) + i64::from(aw / 2))
        .max()
        .unwrap_or(0)
        .max(lane)
        + 1;
    let (x0, y0) = ((wx0 - pad).max(0), (wy0 - pad).max(0));
    let (x1, y1) = (
        (wx1 + w - 1 + pad).min(map.0 - 1),
        (wy1 + h - 1 + pad).min(map.1 - 1),
    );
    let (mw, mh) = (x1 - x0 + 1, y1 - y0 + 1);
    let mut mask = Mask {
        x0,
        y0,
        w: mw,
        h: mh,
        foot: vec![false; (mw * mh) as usize],
        apron: vec![false; (mw * mh) as usize],
    };
    for y in y0..=y1 {
        for x in x0..=x1 {
            let c = cell(x, y);
            let seen = lens.visible(c);
            let i = mask.index(x, y).unwrap_or(0);
            mask.foot[i] = !(seen && lens.buildable(c));
            mask.apron[i] = !(seen && lens.ground(c));
        }
    }
    let structures = obs
        .own
        .iter()
        .map(|o| (o.kind, o.cell, o.footprint, true))
        .chain(
            obs.visible
                .iter()
                .map(|s| (s.kind, s.cell, s.footprint, false)),
        );
    for (skind, at, footprint, mine) in structures {
        let (sx, sy) = (i64::from(at.x), i64::from(at.y));
        let Some(sfp) = footprint else {
            // A unit: its cell carries no footprint.
            mask.block_foot(sx, sy);
            continue;
        };
        let (sw, sh) = (i64::from(sfp.width), i64::from(sfp.height));
        let grow = if mine { lane } else { 0 };
        for y in sy - grow..sy + sh + grow {
            for x in sx - grow..sx + sw + grow {
                let inside = x >= sx && x < sx + sw && y >= sy && y < sy + sh;
                if inside {
                    mask.block_both(x, y);
                } else {
                    mask.block_foot(x, y);
                }
            }
        }
        if mine
            && let Some(sb) = lens.building(skind)
            && let (Some(side), Some(size)) = (sb.exit_side, apron_size(state, sb.function))
        {
            for (x, y) in apron(at, sfp, side, size) {
                mask.block_foot(x, y);
            }
        }
    }

    // The candidates.
    let own_apron = b.exit_side.zip(apron_size(state, b.function));
    let site_targets: Vec<(i64, i64)> = state
        .start_sites
        .iter()
        .filter(|(p, _)| *p != obs.player)
        .map(|(_, c)| (2 * i64::from(c.x) + 1, 2 * i64::from(c.y) + 1))
        .collect();
    let anchor_targets: Vec<(i64, i64)> = anchors
        .iter()
        .map(|&(ax, ay, aw, ah, _)| (2 * ax + aw, 2 * ay + ah))
        .collect();
    let ore: Vec<(i64, i64)> = ore_cells(lens, &anchors, map)
        .iter()
        .map(|c| (2 * i64::from(c.x) + 1, 2 * i64::from(c.y) + 1))
        .collect();
    let mut candidates: Vec<(u64, i64, i64)> = Vec::new();
    for y in wy0..=wy1 {
        for x in wx0..=wx1 {
            let me = (x, y, w, h);
            if !anchors
                .iter()
                .any(|&(ax, ay, aw, ah, r)| gap((ax, ay, aw, ah), me) <= r)
            {
                continue;
            }
            if state.refused.contains_key(&(kind, cell(x, y))) {
                continue;
            }
            if !(y..y + h).all(|cy| (x..x + w).all(|cx| mask.foot_ok(cx, cy))) {
                continue;
            }
            if let Some((side, size)) = own_apron
                && !apron(cell(x, y), fp, side, size).all(|(ax, ay)| mask.apron_ok(ax, ay))
            {
                continue;
            }
            let middle = (2 * x + w, 2 * y + h);
            let score = match b.function {
                BuildingFunction::HarvesterDock if !ore.is_empty() => {
                    let (ex, ey) = own_apron
                        .map(|(side, _)| exit_cell(cell(x, y), fp, side))
                        .unwrap_or((x, y));
                    nearest((2 * ex + 1, 2 * ey + 1), ore.iter().copied())
                }
                BuildingFunction::Defence if !site_targets.is_empty() => {
                    nearest(middle, site_targets.iter().copied())
                }
                _ => nearest(middle, anchor_targets.iter().copied()),
            }
            .unwrap_or(0);
            candidates.push((score, y, x));
        }
    }
    candidates.sort_unstable();

    // The previews, best first.
    // The resumption: a search that used up its questions starts the next one after them, so a kind whose
    // first `placement_cap` candidates all fail reaches the ones behind (its refusals expire with its block,
    // so the list is the same again). Back to the start when the list is shorter.
    let cap = state.profile.economy.placement_cap as usize;
    let later = Tick(obs.tick.0 + u64::from(e.blocked_ticks));
    let skip = state.resume.get(&kind).map_or(0, |&n| n as usize);
    let skip = if skip >= candidates.len() { 0 } else { skip };
    for &(_, y, x) in candidates.iter().skip(skip).take(cap) {
        match lens.placement(kind, cell(x, y)) {
            Some(Ok(())) => {
                state.resume.remove(&kind);
                return Some(cell(x, y));
            }
            Some(Err(_)) => base::refuse(state, kind, cell(x, y), later),
            None => {}
        }
    }
    if candidates.len() - skip > cap {
        state.resume.insert(kind, (skip + cap) as u32);
    } else {
        state.resume.remove(&kind);
    }
    None
}
