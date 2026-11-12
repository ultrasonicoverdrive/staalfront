//! The army manager (`docs/rules/ai.md`, "The army in WP9.1c-1" and "The army in WP9.1c-2"): the gather
//! cell, the scout, the gathering (WP9.1c-1), and the threat rule, the attack timing, the target, marching
//! and finishing (WP9.1c-2). The rules write [`AiState::directive`]; [`operations_minimal`] turns it into
//! orders (the seam that WP9.2 replaces).
//!
//! **Every think** ([`watch`], called by `Ai::think_with` before the manager of the turn): the own structures
//! are compared with what the last think saw (`hp_seen`). One whose hit points fell, or that is gone without
//! having been sold, is a hit: the posture becomes `Defend` at once, until `defend_ticks` after the last hit.
//! A hit on the construction centre also calls the attackers back.
//!
//! One army turn, every 40 ticks (one think in four), in order:
//!
//! (1) **Gather cell** ([`ensure_gather`]): worked out once and kept (P2).
//! (2) **Scout** ([`scout`]): a new own unit of the scout kind becomes the scout; it visits the waypoints
//!    ([`waypoints`]) one after the other, each leg ordered once, whatever its order is (the one exception to
//!    "only an idle unit is ordered": a waypoint on a structure is a `Move` that never ends); a lost scout is
//!    noted and a replacement is bought by the production turn after `scout_retry` ticks, at most
//!    `scouts_max` in a match.
//! (3) **Decide** ([`decide`]): the start sites seen empty; with nothing to defend and no attack out, the
//!    attack trigger (A4) on the units gathered; with an attack out, its end (no attacker left, no target, or
//!    `attack_patience` without an attacker at the target).
//! (4) **Orders** ([`operations_minimal`]): the defence, the march, the finishing `Attack`, the survivors
//!    home, the gathering; at most [`ARMY_COMMANDS_PER_TURN`] commands in the turn, the scout's included. A
//!    unit is ordered only when its observed order is `Idle` (or, once, when a defence began under its
//!    `Move`), and not again within `resend_ticks`.
//!
//! How the AI learns that an order had no effect is always the observation: a unit that is `Idle` again far
//! from where it was sent was refused or gave the route up (`docs/rules/collision.md`, `replan_max`), and the
//! `sent` bookkeeping caps what that costs in commands. It never targets a unit and never orders an `Attack`
//! on an id that is not in `obs.visible`.

use sf_math::{Fx, Vec2};
use sf_sim::{BuildingFunction, Cell, Command, EntityId, Kind, Order, gap};

use crate::observe::{Lens, Observation, Own};
use crate::placement;
use crate::state::{AiState, Directive, HpSeen, Posture};

/// The most gather commands one army turn sends (an anti-spam bound, not a tuning number: with
/// `gather_group` 19 it is 38 units per turn, and the rest follow at the next turn).
pub const GATHER_COMMANDS_PER_TURN: usize = 2;

/// The most commands one army turn sends, the scout's included (WP9.1c-2; an anti-spam bound, not a tuning
/// number: each command carries many ids).
pub const ARMY_COMMANDS_PER_TURN: usize = 4;

/// How far from the first choice the gather cell search looks: rings 0 to 3 round it, 49 cells at most.
const GATHER_RINGS: i64 = 3;

/// How far round a footprint [`beside`] looks for open ground: rings 1 to 3 outside it.
const BESIDE_RINGS: i64 = 3;

/// The scout counts as arrived within this many cells (Chebyshev) of its waypoint: the sight of the shipped
/// scout is 8, so from here it sees what stands on the waypoint.
const SCOUT_ARRIVED: u32 = 3;

/// One army turn; see the module documentation.
pub fn turn(state: &mut AiState, obs: &Observation, lens: &mut Lens<'_>, out: &mut Vec<Command>) {
    if !state.profile.makes_units() {
        return;
    }
    let gather = ensure_gather(state, obs, lens);
    let before = out.len();
    scout(state, obs, out);
    // `obs.own` ascends by id, so a binary search finds the live units.
    let live = |id: &EntityId| obs.own.binary_search_by_key(id, |o| o.id).is_ok();
    state.sent.retain(|id, _| live(id));
    state.attackers.retain(live);
    if let Some(gather) = gather {
        let home = decide(state, obs, lens, gather);
        let budget = ARMY_COMMANDS_PER_TURN.saturating_sub(out.len() - before);
        operations_minimal(state, obs, lens, out, gather, &home, budget);
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

/// The squared distance between two cells.
fn dist2(a: Cell, b: Cell) -> u64 {
    let (dx, dy) = (u64::from(a.x.abs_diff(b.x)), u64::from(a.y.abs_diff(b.y)));
    dx * dx + dy * dy
}

/// Whether `kind` is a construction centre (the building table's function `Anchor`).
fn is_anchor(lens: &Lens<'_>, kind: Kind) -> bool {
    lens.building(kind)
        .is_some_and(|b| b.function == BuildingFunction::Anchor)
}

/// A cell of open ground just outside the footprint `(at, width, height)`, the one nearest to `toward` by
/// squared distance (the first in the order ring, y, x among equals), rings 1 to 3 round the footprint; the
/// footprint's own corner when there is none. Where the army goes for a structure: a cell under it would be a
/// move that never ends.
pub fn beside(lens: &Lens<'_>, at: Cell, width: u32, height: u32, toward: Cell) -> Cell {
    let (x0, y0) = (i64::from(at.x), i64::from(at.y));
    let (x1, y1) = (x0 + i64::from(width.max(1)), y0 + i64::from(height.max(1)));
    for ring in 1..=BESIDE_RINGS {
        let mut best: Option<(u64, Cell)> = None;
        for y in y0 - ring..y1 + ring {
            for x in x0 - ring..x1 + ring {
                let on_ring = x == x0 - ring || x == x1 + ring - 1 || y == y0 - ring || y == y1 + ring - 1;
                if !on_ring || x < 0 || y < 0 {
                    continue;
                }
                let c = Cell::new(x as u32, y as u32);
                if !lens.ground(c) {
                    continue;
                }
                let d = dist2(c, toward);
                if best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, c));
                }
            }
        }
        if let Some((_, c)) = best {
            return c;
        }
    }
    at
}

/// The footprint size of a structure of `kind`; 1 x 1 for a kind not in the building table.
fn size_of(lens: &Lens<'_>, kind: Kind) -> (u32, u32) {
    lens.building(kind).map_or((1, 1), |b| {
        (u32::from(b.footprint.width), u32::from(b.footprint.height))
    })
}

/// The threat rule (A3), at every think: compares the own structures with `hp_seen`, sets the threat and
/// the posture, and refreshes `hp_seen`. A structure being sold is left out, and one the economy sold is not a
/// hit (it drops it from `hp_seen` when it sends the sale, and a structure in `sales` that is gone is not
/// one either); a repair only raises hit points. So neither trips it. Off for a profile that makes no units.
pub fn watch(state: &mut AiState, obs: &Observation, lens: &Lens<'_>) {
    if !state.profile.makes_units() {
        return;
    }
    let now = obs.tick;
    // The first hit in id order, a hit on a construction centre before any other.
    let mut hit: Option<(HpSeen, bool)> = None;
    for (&id, &(kind, cell, hp)) in &state.hp_seen {
        let was_hit = match obs.own.binary_search_by_key(&id, |o| o.id) {
            Ok(i) => obs.own[i].hp < hp && !obs.own[i].selling,
            Err(_) => !state.sales.iter().any(|&(s, _)| s == id),
        };
        if !was_hit {
            continue;
        }
        let anchor = is_anchor(lens, kind);
        if hit.is_none_or(|(_, a)| anchor && !a) {
            hit = Some(((kind, cell, hp), anchor));
        }
    }
    state.hp_seen = obs
        .own
        .iter()
        .filter(|o| o.footprint.is_some() && !o.selling)
        .map(|o| (o.id, (o.kind, o.cell, o.hp)))
        .collect();
    if let Some(((kind, cell, _), anchor)) = hit {
        let (w, h) = size_of(lens, kind);
        let toward = state.gather.unwrap_or(cell);
        let at = beside(lens, cell, w, h, toward);
        state.threat = Some((at, now));
        state.defend_since.get_or_insert(now);
        if anchor && !state.attackers.is_empty() {
            // The construction centre is the match: the attack is called back.
            state.attackers.clear();
            state.attack_target = None;
        }
        state.directive = Directive {
            posture: Posture::Defend,
            target: Some(at),
        };
        return;
    }
    if let Some((_, last)) = state.threat
        && now.0 >= last.0 + u64::from(state.profile.army.defend_ticks)
    {
        state.threat = None;
        state.defend_since = None;
        state.directive = match state.attack_target {
            Some(t) if !state.attackers.is_empty() => Directive {
                posture: Posture::Attack,
                target: Some(t),
            },
            _ => Directive::default(),
        };
    }
}

/// The army units within `gather_radius` of the gather cell that are `Idle` (and those given up on after
/// `gather_tries`, counted where they stand): who goes when an attack goes out (A4). Never an attacker.
pub fn gathered(state: &AiState, obs: &Observation, lens: &Lens<'_>, gather: Cell) -> Vec<EntityId> {
    let a = &state.profile.army;
    let radius = u32::from(a.gather_radius);
    obs.own
        .iter()
        .filter(|o| is_army(state.scout, lens, o) && o.order == Order::Idle)
        .filter(|o| !state.attackers.contains(&o.id))
        .filter(|o| {
            cheb(o.cell, gather) <= radius
                || state
                    .sent
                    .get(&o.id)
                    .is_some_and(|&(_, n)| n >= a.gather_tries)
        })
        .map(|o| o.id)
        .collect()
}

/// What the own units `ids` cost (`Lens::unit_cost`).
pub fn value_of(obs: &Observation, lens: &Lens<'_>, ids: &[EntityId]) -> u64 {
    obs.own
        .iter()
        .filter(|o| ids.contains(&o.id))
        .map(|o| u64::from(lens.unit_cost(o.kind).unwrap_or(0)))
        .sum()
}

/// The attack trigger (A4) at `now` for `count` gathered units worth `value`: never before
/// `first_attack_tick`; then at `attack_value`, or `max_wait` ticks after the last attack with at least
/// `min_units`.
pub fn attack_due(state: &AiState, now: sf_sim::Tick, value: u64, count: usize) -> bool {
    let a = &state.profile.army;
    if now.0 < u64::from(a.first_attack_tick) || count == 0 {
        return false;
    }
    value >= u64::from(a.attack_value)
        || (now.0 >= state.last_attack_tick.0 + u64::from(a.max_wait)
            && count >= usize::from(a.min_units))
}

/// The target cell (A5): beside ([`beside`], facing the gather cell) the construction centre of another
/// player the AI remembers (the nearest to the gather cell, the lowest id among equals); else beside the start
/// site of another player not seen empty (the nearest, the lowest player among equals); else beside the
/// nearest structure of another player it remembers; else `None`.
pub fn target(state: &AiState, lens: &Lens<'_>, gather: Cell) -> Option<Cell> {
    let me = state.player;
    let nearest = |anchors_only: bool| {
        let mut best: Option<(u64, Cell, u32, u32)> = None;
        for k in state.known_structures.values() {
            if k.owner == me || (anchors_only && !is_anchor(lens, k.kind)) {
                continue;
            }
            let d = dist2(k.anchor, gather);
            if best.is_none_or(|(b, ..)| d < b) {
                best = Some((d, k.anchor, u32::from(k.width), u32::from(k.height)));
            }
        }
        best
    };
    if let Some((_, at, w, h)) = nearest(true) {
        return Some(beside(lens, at, w, h, gather));
    }
    let (w, h) = lens
        .buildings()
        .find(|b| b.function == BuildingFunction::Anchor)
        .map_or((1, 1), |b| {
            (u32::from(b.footprint.width), u32::from(b.footprint.height))
        });
    let mut site: Option<(u64, Cell)> = None;
    for &(p, c) in &state.start_sites {
        if p == me || state.sites_cleared.contains(&p) {
            continue;
        }
        let d = dist2(c, gather);
        if site.is_none_or(|(b, _)| d < b) {
            site = Some((d, c));
        }
    }
    if let Some((_, c)) = site {
        return Some(beside(lens, c, w, h, gather));
    }
    nearest(false).map(|(_, at, w, h)| beside(lens, at, w, h, gather))
}

/// The start sites of other players whose cell is visible with no construction centre of theirs on it go
/// into `sites_cleared` (ascending, once).
fn clear_sites(state: &mut AiState, obs: &Observation, lens: &Lens<'_>) {
    for i in 0..state.start_sites.len() {
        let (p, c) = state.start_sites[i];
        if p == state.player || state.sites_cleared.contains(&p) || !lens.visible(c) {
            continue;
        }
        let there = obs.visible.iter().any(|s| {
            s.owner == p
                && is_anchor(lens, s.kind)
                && s.footprint.is_some_and(|f| {
                    c.x >= s.cell.x
                        && c.x < s.cell.x + u32::from(f.width)
                        && c.y >= s.cell.y
                        && c.y < s.cell.y + u32::from(f.height)
                })
        });
        if !there {
            state.sites_cleared.push(p);
            state.sites_cleared.sort();
        }
    }
}

/// Step (3): the attack trigger and the attack's end. Returns the survivors of an attack that ended, to be
/// sent home.
fn decide(state: &mut AiState, obs: &Observation, lens: &Lens<'_>, gather: Cell) -> Vec<EntityId> {
    let now = obs.tick;
    clear_sites(state, obs, lens);
    let a = state.profile.army;
    if !state.attackers.is_empty() {
        let tgt = target(state, lens, gather);
        if let Some(t) = tgt
            && obs.own.iter().any(|o| {
                state.attackers.contains(&o.id) && cheb(o.cell, t) <= u32::from(a.arrive_radius)
            })
        {
            state.attack_started = now;
        }
        let patience_out = now.0 >= state.attack_started.0 + u64::from(a.attack_patience);
        if tgt.is_none() || patience_out {
            let home = std::mem::take(&mut state.attackers);
            end_attack(state);
            return home;
        }
        state.attack_target = tgt;
        if state.threat.is_none() {
            state.directive = Directive {
                posture: Posture::Attack,
                target: tgt,
            };
        }
        return Vec::new();
    }
    if state.attack_target.is_some() {
        // Every attacker is gone.
        end_attack(state);
    }
    if state.threat.is_some() {
        return Vec::new();
    }
    let ids = gathered(state, obs, lens, gather);
    let value = value_of(obs, lens, &ids);
    if !attack_due(state, now, value, ids.len()) {
        return Vec::new();
    }
    let Some(tgt) = target(state, lens, gather) else {
        return Vec::new();
    };
    state.attackers = ids;
    state.attack_target = Some(tgt);
    state.last_attack_tick = now;
    state.attack_started = now;
    state.attacks_launched += 1;
    state.directive = Directive {
        posture: Posture::Attack,
        target: Some(tgt),
    };
    Vec::new()
}

fn end_attack(state: &mut AiState) {
    state.attackers.clear();
    state.attack_target = None;
    if state.threat.is_none() {
        state.directive = Directive::default();
    }
}

/// Whether `o` may be given an order now: `Idle` and not ordered within `resend_ticks`, or on a `Move` and not
/// ordered since `since` (the start of a defence). Any other order (`AttackMove`, `Attack`, `Pursue`, ...) is
/// left alone: the anti-spam rule.
fn orderable(state: &AiState, o: &Own, now: sf_sim::Tick, since: Option<sf_sim::Tick>) -> bool {
    let resend = u64::from(state.profile.army.resend_ticks);
    let last = state.sent.get(&o.id).map(|&(t, _)| t);
    match o.order {
        Order::Idle => last.is_none_or(|t| now.0 >= t.0 + resend),
        Order::Move(_) => since.is_some_and(|s| last.is_none_or(|t| t < s)),
        _ => false,
    }
}

/// Step (4): the directive turned into orders, at most `budget` commands: the defence (`AttackMove` to the
/// threat cell for the units at home), the march (`AttackMove` to the target for idle attackers away from it),
/// the finishing `Attack` (idle attackers at the target, on a visible enemy structure: [`finishing_target`]),
/// the survivors of an attack that ended (`home`, `Move` to the gather cell), then the gathering while nothing
/// is defended. One command each (the gathering up to [`GATHER_COMMANDS_PER_TURN`]), many ids in it.
pub fn operations_minimal(
    state: &mut AiState,
    obs: &Observation,
    lens: &Lens<'_>,
    out: &mut Vec<Command>,
    gather: Cell,
    home: &[EntityId],
    budget: usize,
) {
    let now = obs.tick;
    let arrive = u32::from(state.profile.army.arrive_radius);
    let mut left = budget;
    let mut send = |state: &mut AiState, out: &mut Vec<Command>, ids: Vec<EntityId>, order: Order| {
        if ids.is_empty() || left == 0 {
            return;
        }
        left -= 1;
        for id in &ids {
            let n = state.sent.get(id).map_or(0, |&(_, n)| n);
            state.sent.insert(*id, (now, n));
        }
        out.push(Command::Order {
            ids,
            order,
            queued: false,
        });
    };
    // The defence: every army unit at home.
    if let Some((cell, _)) = state.threat {
        let ids: Vec<EntityId> = obs
            .own
            .iter()
            .filter(|o| is_army(state.scout, lens, o) && !state.attackers.contains(&o.id))
            .filter(|o| !(o.order == Order::Idle && cheb(o.cell, cell) <= arrive))
            .filter(|o| orderable(state, o, now, state.defend_since))
            .map(|o| o.id)
            .collect();
        send(state, out, ids, Order::AttackMove(middle(cell)));
    }
    // The attack: march, then finish.
    if let Some(t) = state.attack_target
        && !state.attackers.is_empty()
    {
        let mut march = Vec::new();
        let mut arrived = Vec::new();
        for o in obs.own.iter().filter(|o| state.attackers.contains(&o.id)) {
            if !orderable(state, o, now, None) {
                continue;
            }
            if cheb(o.cell, t) > arrive {
                march.push(o.id);
            } else {
                arrived.push(o.id);
            }
        }
        send(state, out, march, Order::AttackMove(middle(t)));
        if !arrived.is_empty()
            && let Some(victim) = finishing_target(state, obs, lens, t)
        {
            send(state, out, arrived, Order::Attack(victim));
        }
    }
    // The survivors of an attack that ended.
    send(state, out, home.to_vec(), Order::Move(middle(gather)));
    // The gathering, while nothing is defended.
    if state.threat.is_none() {
        gather_units(state, obs, lens, out, gather, left.min(GATHER_COMMANDS_PER_TURN), home);
    }
}

/// The structure the attackers at the target finish (A6): a visible construction centre of another player,
/// the nearest to the target cell, else the nearest visible structure of another player; the lowest id among
/// equals. Only ids in `obs.visible`, only structures: never a unit, never something the AI does not see.
pub fn finishing_target(
    state: &AiState,
    obs: &Observation,
    lens: &Lens<'_>,
    at: Cell,
) -> Option<EntityId> {
    let pick = |anchors_only: bool| {
        let mut best: Option<(u64, EntityId)> = None;
        for s in &obs.visible {
            if s.owner == state.player
                || s.footprint.is_none()
                || (anchors_only && !is_anchor(lens, s.kind))
            {
                continue;
            }
            let d = dist2(s.cell, at);
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, s.id));
            }
        }
        best.map(|(_, id)| id)
    };
    pick(true).or_else(|| pick(false))
}

/// The gather cell (rule P2): kept while it is open ground; else worked out again. From the middle of the
/// own standing anchor (the lowest id) `gather_distance` cells towards the middle of the map (an integer
/// step on the larger axis), then the first cell, in the order ring, y, x over rings 0 to 3, that is open
/// ground, under no structure the AI knows, off the apron of an own factory or dock and more than `lane + 2`
/// cells from every own structure. `None` (and again at the next turn) when there is no anchor or no such cell.
pub fn ensure_gather(state: &mut AiState, obs: &Observation, lens: &Lens<'_>) -> Option<Cell> {
    if let Some(g) = state.gather
        && lens.ground(g)
        && !under_a_structure(state, obs, g)
    {
        return Some(g);
    }
    state.gather = find_gather(state, obs, lens);
    state.gather
}

/// Whether `cell` lies under a structure the AI knows: its own, a visible one or a remembered one.
fn under_a_structure(state: &AiState, obs: &Observation, cell: Cell) -> bool {
    let inside = |at: Cell, w: u32, h: u32| {
        cell.x >= at.x && cell.x < at.x + w && cell.y >= at.y && cell.y < at.y + h
    };
    let own = obs.own.iter().filter_map(|o| {
        o.footprint
            .map(|f| (o.cell, u32::from(f.width), u32::from(f.height)))
    });
    let seen = obs.visible.iter().filter_map(|s| {
        s.footprint
            .map(|f| (s.cell, u32::from(f.width), u32::from(f.height)))
    });
    let known = state
        .known_structures
        .values()
        .map(|k| (k.anchor, u32::from(k.width), u32::from(k.height)));
    own.chain(seen)
        .chain(known)
        .any(|(at, w, h)| inside(at, w, h))
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
            // A scout nobody ordered (the match starts with one) counts as the first.
            if state.scouts_sent == 0 {
                state.scouts_sent = 1;
            }
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

/// The gathering (A7), at most `max_commands` commands. Not the attackers and not `home` (sent home this
/// turn already).
fn gather_units(
    state: &mut AiState,
    obs: &Observation,
    lens: &Lens<'_>,
    out: &mut Vec<Command>,
    gather: Cell,
    max_commands: usize,
    home: &[EntityId],
) {
    let now = obs.tick;
    let army = &state.profile.army;
    let (radius, group, tries, resend) = (
        u32::from(army.gather_radius),
        usize::from(army.gather_group),
        army.gather_tries,
        u64::from(army.resend_ticks),
    );
    let scout = state.scout;
    let mut to_send = Vec::new();
    for o in obs.own.iter().filter(|o| {
        is_army(scout, lens, o) && !state.attackers.contains(&o.id) && !home.contains(&o.id)
    }) {
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
    for ids in to_send.chunks(group.max(1)).take(max_commands) {
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
