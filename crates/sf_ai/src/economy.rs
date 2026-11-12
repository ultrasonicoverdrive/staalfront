//! The economy manager (WP9.1b-2, `docs/rules/ai.md`, "The economy turn"): harvesters, the ore silo and
//! repair, once every 40 ticks (one think in four). Rebuilding has no rule of its own: the goal list, the
//! power rule, the silo rule and the harvester rule each notice a loss again.
//!
//! One turn, in order:
//!
//! (1) **Harvesters.** `have` (own harvesters plus harvester items in own queues, the free one of a refinery
//!    included) below `want` (standing refineries times `harvesters_per_refinery`), no harvester queued,
//!    a standing factory of the right kind and no emergency build stalled for lack of money: one
//!    `QueueUnit` on the factory with the shortest queue (the lowest id among equals). The command is
//!    latched in `harvester_sent`; when the next turn finds no harvester in any queue, the simulation
//!    refused it and the kind is blocked for `blocked_ticks`. It ignores `reserve` (that is for the army).
//! (2) **Silo.** `full_turns` counts the turns in a row with the store at least `silo_percent` full. With
//!    two of them, no goal entry open, fewer silos (standing or going up) than standing refineries and a
//!    silo kind in the table, `emergency` is the silo kind and the construction turn builds it first;
//!    otherwise `emergency` is empty.
//! (3) **Repair.** Own standing structures below `repair_below_percent` of their hit points, not repairing
//!    and not in `repair_backoff`, the anchor first, then power plants and factories, then the harvester
//!    dock, then the rest, then by id; at most two `Repair { on: true }` per turn. The AI never stops a
//!    repair (the simulation does at full hit points). While an emergency build is stalled for lack of
//!    money only the anchor is repaired (and its repair may take credits the build waits for: not counted in
//!    `budget::committed`, see `ai.md`). A repair that is sent but never shows (a game without repair
//!    rules ignores it) is not sent again before `blocked_ticks` have passed.
//! (4) **Rescue.** Once per match, when an emergency build has been stalled for lack of money on two economy
//!    turns in a row and no income can arrive, the structures that cover what is owed on it are sold
//!    ([`pick_sales`], never the anchor, a dock or a power plant); see `ai.md`, "The rescue".

use sf_sim::{BuildingFunction, Command, EntityId, Kind, Tick};

use crate::base;
use crate::budget;
use crate::observe::{Lens, Observation, Own};
use crate::state::AiState;

/// The most repairs one economy turn starts.
pub const REPAIRS_PER_TURN: usize = 2;

/// One economy turn; see the module documentation.
pub fn turn(state: &mut AiState, obs: &Observation, lens: &mut Lens<'_>, out: &mut Vec<Command>) {
    let now = obs.tick;
    let later = Tick(now.0 + u64::from(state.profile.economy.blocked_ticks));
    harvesters(state, obs, lens, out, later);
    silo(state, obs, lens);
    rescue(state, obs, lens, out, later);
    repair(state, obs, lens, out, later);
}

/// Whether the own structure `o` is complete: it stands and is not going up.
pub(crate) fn complete(o: &Own) -> bool {
    o.footprint.is_some() && o.standing && o.building.is_none() && !o.selling
}

/// The function of the table entry of `o`'s kind.
pub(crate) fn function_of(lens: &Lens<'_>, o: &Own) -> Option<BuildingFunction> {
    lens.building(o.kind).map(|b| b.function)
}

/// Whether an emergency build is waiting for money: a structure going up that the base cannot do
/// without (the first harvester dock, a power plant while less is made than used) and nothing free to pay
/// for it. The simulation pays structures first, so what is queued after it only waits; the harvester
/// order and every repair but the anchor's keep their hands off the money until it stands. The anchor's
/// repair does not: it may take credits an emergency build is waiting for (see `repair`).
pub fn emergency_stalled(obs: &Observation, lens: &Lens<'_>) -> bool {
    stalled_structure(obs, lens).is_some()
}

/// The structure an emergency build is stalled on (see [`emergency_stalled`]): the first own structure going
/// up that the base cannot do without, when nothing is free.
fn stalled_structure<'o>(obs: &'o Observation, lens: &Lens<'_>) -> Option<&'o Own> {
    if budget::free(obs, lens) > 0 {
        return None;
    }
    let dock_stands = obs
        .own
        .iter()
        .any(|o| complete(o) && function_of(lens, o) == Some(BuildingFunction::HarvesterDock));
    let short = obs.power.is_some_and(|p| p.made < p.used);
    obs.own
        .iter()
        .filter(|o| o.footprint.is_some() && o.building.is_some())
        .find(|o| match function_of(lens, o) {
            Some(BuildingFunction::HarvesterDock) => !dock_stands,
            Some(BuildingFunction::PowerPlant) => short,
            _ => false,
        })
}

fn blocked(state: &AiState, kind: Kind, now: Tick) -> bool {
    state.blocked_until.get(&kind).is_some_and(|t| *t > now)
}

/// The harvester kind to queue and the standing factory to queue it on: the first harvester kind of the
/// table whose factory function the player has standing, on the factory with the shortest queue (the lowest
/// id among equals; `obs.own` ascends by id).
fn harvester_order(obs: &Observation, lens: &Lens<'_>) -> Option<(Kind, EntityId)> {
    lens.harvester_kinds().iter().find_map(|&kind| {
        let function = lens.unit_factory(kind)?;
        obs.own
            .iter()
            .filter(|o| complete(o) && function_of(lens, o) == Some(function))
            .min_by_key(|o| o.factory.as_ref().map_or(0, |f| f.queue.len()))
            .map(|o| (kind, o.id))
    })
}

/// Whether the economy turn would still buy a harvester: fewer than wanted (own harvesters plus harvester
/// items in queues against standing docks times `harvesters_per_refinery`), a factory to make it, the map has
/// ore ground and the kind is not blocked. Production leaves the money alone while this holds (WP9.1c-1):
/// the harvester comes first.
pub fn harvester_wanted(state: &AiState, obs: &Observation, lens: &Lens<'_>) -> bool {
    let kinds = lens.harvester_kinds();
    let queued = obs
        .own
        .iter()
        .filter_map(|o| o.factory.as_ref())
        .flat_map(|f| f.queue.iter())
        .filter(|(k, _)| kinds.contains(k))
        .count();
    let units = obs
        .own
        .iter()
        .filter(|o| o.footprint.is_none() && kinds.contains(&o.kind))
        .count();
    let docks = obs
        .own
        .iter()
        .filter(|o| complete(o) && function_of(lens, o) == Some(BuildingFunction::HarvesterDock))
        .count();
    let want = docks * usize::from(state.profile.economy.harvesters_per_refinery);
    units + queued < want
        && lens.has_ore_ground()
        && harvester_order(obs, lens).is_some_and(|(kind, _)| !blocked(state, kind, obs.tick))
}

fn harvesters(
    state: &mut AiState,
    obs: &Observation,
    lens: &Lens<'_>,
    out: &mut Vec<Command>,
    later: Tick,
) {
    let kinds = lens.harvester_kinds();
    let queued = obs
        .own
        .iter()
        .filter_map(|o| o.factory.as_ref())
        .flat_map(|f| f.queue.iter())
        .filter(|(k, _)| kinds.contains(k))
        .count();
    // Verify the last order: when nothing is queued the simulation refused it.
    if let Some((kind, _)) = state.harvester_sent.take()
        && queued == 0
    {
        state.blocked_until.insert(kind, later);
    }
    let units = obs
        .own
        .iter()
        .filter(|o| o.footprint.is_none() && kinds.contains(&o.kind))
        .count();
    let docks = obs
        .own
        .iter()
        .filter(|o| complete(o) && function_of(lens, o) == Some(BuildingFunction::HarvesterDock))
        .count();
    let want = docks * usize::from(state.profile.economy.harvesters_per_refinery);
    if units + queued >= want || queued > 0 || emergency_stalled(obs, lens) {
        return;
    }
    let Some((kind, factory)) = harvester_order(obs, lens) else {
        return;
    };
    if blocked(state, kind, obs.tick) {
        return;
    }
    // No ore ground on the map (map knowledge): a harvester would have nothing to mine.
    if !lens.has_ore_ground() {
        return;
    }
    out.push(Command::QueueUnit { factory, kind });
    state.harvester_sent = Some((kind, obs.tick));
}

/// One entry of the building table as the silo choice sees it: `(kind, what it stores, function, price,
/// faction mask)`.
pub type SiloCandidate = (Kind, u32, BuildingFunction, Option<u32>, u8);

/// The kind of the ore silo among `table` (ascending by kind): the first that stores ore, is no harvester
/// dock, can be bought (has a price) and is open to the player's faction `bit` (any faction when `None`).
pub fn pick_silo(mut table: impl Iterator<Item = SiloCandidate>, bit: Option<u8>) -> Option<Kind> {
    table
        .find(|&(_, stores, function, cost, factions)| {
            stores > 0
                && function != BuildingFunction::HarvesterDock
                && cost.is_some()
                && bit.is_none_or(|bit| factions & bit != 0)
        })
        .map(|c| c.0)
}

fn silo_kind(lens: &Lens<'_>) -> Option<Kind> {
    pick_silo(
        lens.buildings().map(|b| {
            (
                b.kind,
                lens.storage_of(b.kind),
                b.function,
                b.cost,
                b.factions,
            )
        }),
        lens.faction_bit(),
    )
}

fn silo(state: &mut AiState, obs: &Observation, lens: &Lens<'_>) {
    let full = obs.storage_capacity > 0
        && obs.ore_stored * 100
            >= obs.storage_capacity * u64::from(state.profile.economy.silo_percent);
    state.full_turns = if full { state.full_turns + 1 } else { 0 };
    state.emergency = None;
    let Some(kind) = silo_kind(lens) else {
        return;
    };
    let goal_open = base::goal_open(state, obs);
    let docks = obs
        .own
        .iter()
        .filter(|o| complete(o) && function_of(lens, o) == Some(BuildingFunction::HarvesterDock))
        .count();
    if state.full_turns >= 2 && !goal_open && base::have(obs, kind) < docks {
        state.emergency = Some(kind);
    }
}

/// The order repair goes in: 0 the anchor, 1 a power plant or a factory, 2 the harvester dock, 3 the rest.
fn repair_rank(function: Option<BuildingFunction>) -> u8 {
    match function {
        Some(BuildingFunction::Anchor) => 0,
        Some(f) if f == BuildingFunction::PowerPlant || f.is_factory() => 1,
        Some(BuildingFunction::HarvesterDock) => 2,
        _ => 3,
    }
}

fn repair(
    state: &mut AiState,
    obs: &Observation,
    lens: &Lens<'_>,
    out: &mut Vec<Command>,
    later: Tick,
) {
    let below = u64::from(state.profile.economy.repair_below_percent);
    let damaged = |o: &Own| complete(o) && u64::from(o.hp) * 100 < u64::from(o.max_hp) * below;
    // Drop what no longer needs the backoff.
    state.repair_backoff.retain(|id, until| {
        *until > obs.tick
            && obs
                .own
                .iter()
                .any(|o| o.id == *id && damaged(o) && !o.repairing)
    });
    let stalled = emergency_stalled(obs, lens);
    let mut list: Vec<(u8, &Own)> = obs
        .own
        .iter()
        .filter(|o| damaged(o) && !o.repairing && !state.repair_backoff.contains_key(&o.id))
        .map(|o| (repair_rank(function_of(lens, o)), o))
        .filter(|&(rank, _)| !stalled || rank == 0)
        .collect();
    // Stable sort: ties keep the ascending ids of `obs.own`.
    list.sort_by_key(|&(rank, _)| rank);
    for (_, o) in list.into_iter().take(REPAIRS_PER_TURN) {
        out.push(Command::Repair {
            structure: o.id,
            on: true,
        });
        state.repair_backoff.insert(o.id, later);
    }
}

/// Every kind `kind` needs, directly or through others (`requires` of the building table), ascending.
fn requires_closure(lens: &Lens<'_>, kind: Kind) -> Vec<Kind> {
    let mut found: Vec<Kind> = Vec::new();
    let mut todo = vec![kind];
    while let Some(k) = todo.pop() {
        for &r in lens.building(k).map_or(&[][..], |b| &b.requires[..]) {
            if !found.contains(&r) {
                found.push(r);
                todo.push(r);
            }
        }
    }
    found.sort();
    found
}

/// Whether a structure of this function may be sold in the rescue: never the anchor (selling it is defeat),
/// a harvester dock (income) or a power plant (the base stops without it).
pub fn sellable(function: BuildingFunction) -> bool {
    !matches!(
        function,
        BuildingFunction::Anchor | BuildingFunction::HarvesterDock | BuildingFunction::PowerPlant
    )
}

/// The position of `kind` in the goal list, the last entry that names it; a kind that is not in the list
/// ranks after all of them.
fn goal_position(state: &AiState, kind: Kind) -> usize {
    state
        .profile
        .build_order
        .iter()
        .rposition(|&(k, _)| k == kind)
        .unwrap_or(usize::MAX)
}

/// The structures to sell to cover `need` credits, from `eligible` `(refund, goal position, id)`: the one
/// with the smallest refund that covers it alone (ties: the latest in the goal list, then the lowest id);
/// else the smallest refunds in that order one after the other until their sum covers it; nothing when all of
/// them together fall short (a sale that does not finish the build buys nothing).
pub fn pick_sales(mut eligible: Vec<(u64, usize, EntityId)>, need: u64) -> Vec<EntityId> {
    eligible.sort_by_key(|&(refund, pos, id)| (refund, std::cmp::Reverse(pos), id));
    if let Some(&(_, _, id)) = eligible.iter().find(|&&(refund, ..)| refund >= need) {
        return vec![id];
    }
    let mut sum = 0u64;
    let mut chosen = Vec::new();
    for &(refund, _, id) in &eligible {
        chosen.push(id);
        sum += refund;
        if sum >= need {
            return chosen;
        }
    }
    Vec::new()
}

/// The rescue (decision of 12-11-2026, replacing "the AI never sells"): once per match, when an emergency
/// build has been stalled for lack of money on two economy turns in a row and no income can arrive (no
/// harvester, or no complete dock), the AI sells structures to pay what is still owed on it. See `ai.md`.
fn rescue(
    state: &mut AiState,
    obs: &Observation,
    lens: &Lens<'_>,
    out: &mut Vec<Command>,
    later: Tick,
) {
    // Verify the last sales: any that shows `selling` or is gone started, and the rescue is spent; when
    // the simulation refused every one it is tried again after `blocked_ticks`.
    if !state.sales.is_empty() {
        let sent = std::mem::take(&mut state.sales);
        let started = sent.iter().any(|(id, _)| {
            obs.own
                .iter()
                .find(|o| o.id == *id)
                .is_none_or(|o| o.selling)
        });
        if started {
            state.rescue_used = true;
        } else {
            state.rescue_after = later;
        }
    }
    let stalled = stalled_structure(obs, lens);
    state.stalled_turns = if stalled.is_some() {
        state.stalled_turns + 1
    } else {
        0
    };
    let Some(stalled) = stalled else {
        return;
    };
    if state.rescue_used || state.stalled_turns < 2 || obs.tick < state.rescue_after {
        return;
    }
    let kinds = lens.harvester_kinds();
    let harvester = obs
        .own
        .iter()
        .any(|o| o.footprint.is_none() && kinds.contains(&o.kind));
    let dock = obs
        .own
        .iter()
        .any(|o| complete(o) && function_of(lens, o) == Some(BuildingFunction::HarvesterDock));
    if harvester && dock {
        return;
    }
    // What is still owed on the stalled structure, as `budget::committed` counts it, less the credits.
    let Some(c) = stalled.building else {
        return;
    };
    let Some(cost) = lens.building(stalled.kind).and_then(|b| b.cost) else {
        return;
    };
    let owed = (u64::from(cost) * u64::from(c.left)).div_ceil(u64::from(c.total.max(1)));
    let need = owed.saturating_sub(obs.credits);
    if need == 0 {
        return;
    }
    let needed = requires_closure(lens, stalled.kind);
    let mut eligible = Vec::new();
    for o in &obs.own {
        if !complete(o) || o.id == stalled.id || needed.binary_search(&o.kind).is_ok() {
            continue;
        }
        if !function_of(lens, o).is_some_and(sellable) {
            continue;
        }
        if let Some(refund) = lens.sale_refund(o.kind, o.hp, o.max_hp) {
            eligible.push((refund, goal_position(state, o.kind), o.id));
        }
    }
    for id in pick_sales(eligible, need) {
        out.push(Command::Sell { structure: id });
        state.sales.push((id, obs.tick));
        // WP9.1c-2 (decision 6 of 12-11-2026): a structure sold is no hit for the threat rule.
        state.hp_seen.remove(&id);
    }
}
