//! The army manager as far as WP9.1c-1 goes (`docs/rules/ai.md`, "The army, as far as 9.1c-1 goes"): the
//! gather cell, the scout and the gathering of the army. **Army units never attack here**; the army manager
//! of WP9.1c-2 (threat, attack timing, target, marching) slots into [`AiState::directive`].
//!
//! One turn, every 40 ticks (one think in four), in order:
//!
//! (1) **Gather cell** ([`ensure_gather`]): worked out once and kept (P2).
//! (2) **Scout** ([`scout`]): a new own unit of the scout kind becomes the scout; it visits the waypoints
//!    ([`waypoints`]) one after the other, each leg ordered once, whatever its order is (the one exception to
//!    "only an idle unit is ordered": a waypoint on a structure is a `Move` that never ends); a lost scout is noted and a replacement is
//!    bought by the production turn after `scout_retry` ticks, at most `scouts_max` in a match.
//! (3) **Gather** ([`gather_units`]): army units (armed, mobile, not a harvester, not the scout) that are `Idle`
//!    farther than `gather_radius` from the gather cell get one `Move` there, `gather_group` to a command, at
//!    most [`GATHER_COMMANDS_PER_TURN`] commands per turn. A unit is never ordered while it has an order, not
//!    again within `resend_ticks`, and given up on after `gather_tries` orders without arriving.
//!
//! How the AI learns that an order had no effect is always the observation: a unit that is `Idle` again far
//! from the gather cell was refused or gave the route up (`docs/rules/collision.md`, `replan_max`), and the
//! `sent` bookkeeping caps what that costs in commands.

use sf_math::{Fx, Vec2};
use sf_sim::{BuildingFunction, Cell, Command, EntityId, Order, gap};

use crate::observe::{Lens, Observation, Own};
use crate::placement;
use crate::state::{AiState, Directive};

/// The most gather commands one army turn sends (an anti-spam bound, not a tuning number: with
/// `gather_group` 19 it is 38 units per turn, and the rest follow at the next turn).
pub const GATHER_COMMANDS_PER_TURN: usize = 2;

/// How far from the first choice the gather cell search looks: rings 0 to 3 round it, 49 cells at most.
const GATHER_RINGS: i64 = 3;

/// The scout counts as arrived within this many cells (Chebyshev) of its waypoint: the sight of the shipped
/// scout is 8, so from here it sees what stands on the waypoint.
const SCOUT_ARRIVED: u32 = 3;

/// One army turn; see the module documentation.
pub fn turn(state: &mut AiState, obs: &Observation, lens: &mut Lens<'_>, out: &mut Vec<Command>) {
    if !state.profile.makes_units() {
        return;
    }
    // The seam: until WP9.1c-2 the army only gathers.
    state.directive = Directive::default();
    let gather = ensure_gather(state, obs, lens);
    scout(state, obs, out);
    if let Some(gather) = gather {
        gather_units(state, obs, lens, out, gather);
    }
}

/// The middle of `cell`, as a point for an order.
fn middle(cell: Cell) -> Vec2 {
    let half = Fx::from_ratio(1, 2);
    Vec2::new(
        Fx::from_i64(i64::from(cell.x)) + half,
        Fx::from_i64(i64::from(cell.y)) + half,
    )
}

/// The chessboard distance between two cells.
fn cheb(a: Cell, b: Cell) -> u32 {
    a.x.abs_diff(b.x).max(a.y.abs_diff(b.y))
}

/// The gather cell (rule P2): kept while it is open ground; else worked out again. From the middle of the
/// own standing anchor (the lowest id) `gather_distance` cells towards the middle of the map (an integer
/// step on the larger axis), then the first cell, in the order ring, y, x over rings 0 to 3, that is open
/// ground, under no structure the AI knows, off the apron of an own factory or dock and more than `lane + 2`
/// cells from every own structure. `None` (and again at the next turn) when there is no anchor or no such cell.
pub fn ensure_gather(state: &mut AiState, obs: &Observation, lens: &Lens<'_>) -> Option<Cell> {
    if let Some(g) = state.gather
        && lens.ground(g)
    {
        return Some(g);
    }
    state.gather = find_gather(state, obs, lens);
    state.gather
}

fn find_gather(state: &AiState, obs: &Observation, lens: &Lens<'_>) -> Option<Cell> {
    let anchor = obs.own.iter().find(|o| {
        o.standing
            && o.footprint.is_some()
            && lens
                .building(o.kind)
                .is_some_and(|b| b.function == BuildingFunction::Anchor)
    })?;
    let fp = anchor.footprint?;
    let (w, h) = (i64::from(obs.map.0), i64::from(obs.map.1));
    let mid = (
        i64::from(anchor.cell.x) + i64::from(fp.width) / 2,
        i64::from(anchor.cell.y) + i64::from(fp.height) / 2,
    );
    let (dx, dy) = (w / 2 - mid.0, h / 2 - mid.1);
    let dist = i64::from(state.profile.army.gather_distance);
    let first = if dx == 0 && dy == 0 {
        mid
    } else if dx.abs() >= dy.abs() {
        (mid.0 + dx.signum() * dist, mid.1 + dy * dist / dx.abs())
    } else {
        (mid.0 + dx * dist / dy.abs(), mid.1 + dy.signum() * dist)
    };
    let first = (first.0.clamp(0, w - 1), first.1.clamp(0, h - 1));

    // What the cell must keep clear of: footprints (own, seen, remembered), own aprons, a lane round own
    // structures.
    let lane = i64::from(state.profile.economy.lane) + 2;
    let mut footprints: Vec<(i64, i64, i64, i64)> = Vec::new();
    let mut own_structures: Vec<(i64, i64, i64, i64)> = Vec::new();
    let mut aprons: Vec<(i64, i64)> = Vec::new();
    for o in &obs.own {
        let Some(fp) = o.footprint else { continue };
        let rect = (
            i64::from(o.cell.x),
            i64::from(o.cell.y),
            i64::from(fp.width),
            i64::from(fp.height),
        );
        footprints.push(rect);
        own_structures.push(rect);
        if let Some(b) = lens.building(o.kind)
            && let (Some(side), Some(size)) =
                (b.exit_side, placement::apron_size(state, b.function))
        {
            aprons.extend(placement::apron(o.cell, fp, side, size));
        }
    }
    for s in &obs.visible {
        if let Some(fp) = s.footprint {
            footprints.push((
                i64::from(s.cell.x),
                i64::from(s.cell.y),
                i64::from(fp.width),
                i64::from(fp.height),
            ));
        }
    }
    for k in state.known_structures.values() {
        footprints.push((
            i64::from(k.anchor.x),
            i64::from(k.anchor.y),
            i64::from(k.width),
            i64::from(k.height),
        ));
    }
    let free = |x: i64, y: i64| -> bool {
        if x < 0 || y < 0 || x >= w || y >= h {
            return false;
        }
        let c = Cell::new(x as u32, y as u32);
        if !lens.ground(c) || aprons.contains(&(x, y)) {
            return false;
        }
        if footprints
            .iter()
            .any(|&(fx, fy, fw, fh)| x >= fx && x < fx + fw && y >= fy && y < fy + fh)
        {
            return false;
        }
        own_structures.iter().all(|&r| gap((x, y, 1, 1), r) > lane)
    };
    for ring in 0..=GATHER_RINGS {
        for y in first.1 - ring..=first.1 + ring {
            for x in first.0 - ring..=first.0 + ring {
                if (x - first.0).abs().max((y - first.1).abs()) == ring && free(x, y) {
                    return Some(Cell::new(x as u32, y as u32));
                }
            }
        }
    }
    None
}

/// The scout's legs: the start sites of the other players in ascending player id, then the centres of the
/// four map quarters (north-west, north-east, south-west, south-east).
pub fn waypoints(state: &AiState, obs: &Observation) -> Vec<Cell> {
    let mut legs: Vec<Cell> = state
        .start_sites
        .iter()
        .filter(|(p, _)| *p != state.player)
        .map(|&(_, c)| c)
        .collect();
    let (w, h) = obs.map;
    for (qx, qy) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
        legs.push(Cell::new(w * qx / 4, h * qy / 4));
    }
    legs
}

/// The scout rules (A2); see the module documentation.
fn scout(state: &mut AiState, obs: &Observation, out: &mut Vec<Command>) {
    let now = obs.tick;
    let Some(kind) = state.profile.scout else {
        return;
    };
    let has = |id| obs.own.iter().any(|o| o.id == id);
    if let Some(id) = state.scout
        && !has(id)
    {
        state.scout = None;
        state.scout_gone_at = Some(now);
    }
    if state.scout.is_none() {
        if let Some(o) = obs
            .own
            .iter()
            .find(|o| o.footprint.is_none() && o.kind == kind)
        {
            state.scout = Some(o.id);
            state.scout_leg = 0;
            state.scout_leg_sent = None;
            state.scout_gone_at = None;
        } else if state.scouts_sent > 0 && state.scout_gone_at.is_none() && !queued(obs, kind) {
            // Ordered, and neither a unit nor an item in a queue: lost before it came out.
            state.scout_gone_at = Some(now);
        }
    }
    let Some(me) = state
        .scout
        .and_then(|id| obs.own.iter().find(|o| o.id == id))
    else {
        return;
    };
    let legs = waypoints(state, obs);
    let patience = u64::from(state.profile.army.scout_patience);
    // At most one step per leg, so a short list ends the loop.
    for _ in 0..=legs.len() {
        let Some(&at) = legs.get(usize::from(state.scout_leg)) else {
            return;
        };
        if cheb(me.cell, at) <= SCOUT_ARRIVED {
            state.scout_leg += 1;
            state.scout_leg_sent = None;
            continue;
        }
        if let Some(sent) = state.scout_leg_sent {
            if now.0 >= sent.0 + patience {
                state.scout_leg += 1;
                state.scout_leg_sent = None;
                continue;
            }
            // Ordered: it is walking, or it gave up and waits for the patience to run out. Never ordered
            // twice to the same leg.
            return;
        }
        // Not only when idle: a move to a waypoint that lies on a structure never ends, and the scout stands
        // beside it with its `Move` for ever. The latch above is what keeps the rate down: one order per leg.
        out.push(Command::Order {
            ids: vec![me.id],
            order: Order::Move(middle(at)),
            queued: false,
        });
        state.scout_leg_sent = Some(now);
        return;
    }
}

/// Whether an own factory has an item of `kind` queued.
fn queued(obs: &Observation, kind: sf_sim::Kind) -> bool {
    obs.own
        .iter()
        .filter_map(|o| o.factory.as_ref())
        .any(|f| f.queue.iter().any(|&(k, _)| k == kind))
}

/// Whether `o` is an army unit (rule A1): a mobile armed unit that is no harvester and not the scout.
pub fn is_army(scout: Option<EntityId>, lens: &Lens<'_>, o: &Own) -> bool {
    o.footprint.is_none()
        && o.mobile
        && o.armed
        && !lens.harvester_kinds().contains(&o.kind)
        && scout != Some(o.id)
}

/// The gathering (A7); see the module documentation.
fn gather_units(
    state: &mut AiState,
    obs: &Observation,
    lens: &Lens<'_>,
    out: &mut Vec<Command>,
    gather: Cell,
) {
    let now = obs.tick;
    let army = &state.profile.army;
    let (radius, group, tries, resend) = (
        u32::from(army.gather_radius),
        usize::from(army.gather_group),
        army.gather_tries,
        u64::from(army.resend_ticks),
    );
    // `obs.own` ascends by id, so a binary search finds the live units.
    state
        .sent
        .retain(|id, _| obs.own.binary_search_by_key(id, |o| o.id).is_ok());
    let scout = state.scout;
    let mut to_send = Vec::new();
    for o in obs.own.iter().filter(|o| is_army(scout, lens, o)) {
        if cheb(o.cell, gather) <= radius {
            state.sent.remove(&o.id);
            continue;
        }
        if o.order != Order::Idle {
            continue;
        }
        if let Some(&(t, n)) = state.sent.get(&o.id)
            && (n >= tries || now.0 < t.0 + resend)
        {
            continue;
        }
        to_send.push(o.id);
    }
    for ids in to_send.chunks(group.max(1)).take(GATHER_COMMANDS_PER_TURN) {
        for id in ids {
            let n = state.sent.get(id).map_or(0, |&(_, n)| n);
            state.sent.insert(*id, (now, n + 1));
        }
        out.push(Command::Order {
            ids: ids.to_vec(),
            order: Order::Move(middle(gather)),
            queued: false,
        });
    }
}
