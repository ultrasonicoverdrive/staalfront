//! The production manager (WP9.1c-1, `docs/rules/ai.md`, "Production"): which units the AI buys, where they
//! gather, and the one scout. Once every 40 ticks (one think in four).
//!
//! One turn, in order:
//!
//! (1) **Verify.** The unit orders sent at the last turn (`units_sent`) must have raised the player's count of
//!    their kind (own units plus queue items): if not, the simulation refused them, and the kind goes into
//!    `blocked_until` for `blocked_ticks` (a refused scout is taken back from `scouts_sent`).
//! (2) **Rally** (P1). Every complete factory that makes no harvester and whose rally point is not the
//!    gather cell gets one `SetRally`;
//!    the observation (`OwnFactory.rally`) says whether it took, and a factory asked is left alone for
//!    `blocked_ticks` (`rally_backoff`).
//! (3) **Queue** (P3). Nothing while a harvester is wanted ([`economy::harvester_wanted`]; the economy comes
//!    first). Otherwise, one unit at a time, the scout when one is due (and nothing else before the first
//!    scout is ordered), else the kind of the mix furthest below its share, on the complete factory of its function
//!    with the shortest queue (the lowest id among equals), at most `max_queue` items in a factory and one
//!    order per factory per turn, while the free credits cover `holdback + price`. `holdback` is nothing when no
//!    goal entry is open, else `reserve` plus what the construction turn is saving for ([`base::next_build`]).

use sf_sim::{BuildingFunction, Cell, Command, EntityId, Kind, Tick};

use crate::army;
use crate::base;
use crate::budget;
use crate::economy::{self, complete, function_of};
use crate::observe::{Lens, Observation};
use crate::state::AiState;

/// One production turn; see the module documentation.
pub fn turn(state: &mut AiState, obs: &Observation, lens: &mut Lens<'_>, out: &mut Vec<Command>) {
    if !state.profile.makes_units() {
        return;
    }
    let later = Tick(obs.tick.0 + u64::from(state.profile.economy.blocked_ticks));
    verify(state, obs, later);
    if let Some(gather) = army::ensure_gather(state, obs, lens) {
        rally(state, obs, lens, out, gather, later);
    }
    queue(state, obs, lens, out);
}

/// How many units of `kind` the player has and how many are queued: the count a unit order must raise.
pub fn have(obs: &Observation, kind: Kind) -> u32 {
    let units = obs
        .own
        .iter()
        .filter(|o| o.footprint.is_none() && o.kind == kind)
        .count();
    let items = obs
        .own
        .iter()
        .filter_map(|o| o.factory.as_ref())
        .flat_map(|f| f.queue.iter())
        .filter(|&&(k, _)| k == kind)
        .count();
    (units + items) as u32
}

fn blocked(state: &AiState, kind: Kind, now: Tick) -> bool {
    state.blocked_until.get(&kind).is_some_and(|t| *t > now)
}

/// How many own units of `kind` stand.
pub fn units_of(obs: &Observation, kind: Kind) -> u32 {
    obs.own
        .iter()
        .filter(|o| o.footprint.is_none() && o.kind == kind)
        .count() as u32
}

/// How many items of `kind` the factory `id` has queued.
pub fn queued_in(obs: &Observation, id: EntityId, kind: Kind) -> u32 {
    obs.own
        .iter()
        .filter(|o| o.id == id)
        .filter_map(|o| o.factory.as_ref())
        .flat_map(|f| f.queue.iter())
        .filter(|&&(k, _)| k == kind)
        .count() as u32
}

/// Step (1). An order was accepted when the factory has more items of the kind queued than before it, or
/// the player has more units of the kind (the item came out within the 40 ticks). A unit of the kind that
/// died meanwhile is neither: the live count alone would read it as a refusal. A scout order that was
/// accepted is never taken back, so `scouts_max` is a hard cap on scouts that entered a queue.
fn verify(state: &mut AiState, obs: &Observation, later: Tick) {
    for (factory, kind, _, units, queued) in std::mem::take(&mut state.units_sent) {
        if queued_in(obs, factory, kind) > queued || units_of(obs, kind) > units {
            continue;
        }
        state.blocked_until.insert(kind, later);
        if state.profile.scout == Some(kind) {
            state.scouts_sent = state.scouts_sent.saturating_sub(1);
        }
    }
}

/// Whether the factories of `function` make a harvester kind. They get no rally point: a harvester sent to
/// the gather cell first mines later (measured on `ai_scout`: 15 percent less ore delivered in 20,000 ticks
/// with the rally point on the vehicle factory), and the units of such a factory are brought to the gather
/// cell by the gathering rule of the army turn instead.
fn makes_harvesters(lens: &Lens<'_>, function: BuildingFunction) -> bool {
    lens.harvester_kinds()
        .iter()
        .any(|&k| lens.unit_factory(k) == Some(function))
}

/// Step (2).
fn rally(
    state: &mut AiState,
    obs: &Observation,
    lens: &Lens<'_>,
    out: &mut Vec<Command>,
    gather: Cell,
    later: Tick,
) {
    let wrong = |o: &crate::observe::Own| {
        complete(o)
            && function_of(lens, o)
                .is_some_and(|f| f.is_factory() && !makes_harvesters(lens, f))
            // A factory with no production record (nothing queued yet) has no rally point.
            && o.factory.as_ref().and_then(|f| f.rally) != Some(gather)
    };
    // Drop what no longer needs the backoff: the rally point is right, the factory is gone or the time is up.
    state.rally_backoff.retain(|id, until| {
        *until > obs.tick
            && obs
                .own
                .binary_search_by_key(id, |o| o.id)
                .ok()
                .is_some_and(|i| wrong(&obs.own[i]))
    });
    for o in obs.own.iter().filter(|o| wrong(o)) {
        if state.rally_backoff.contains_key(&o.id) {
            continue;
        }
        out.push(Command::SetRally {
            factory: o.id,
            cell: Some(gather),
        });
        state.rally_backoff.insert(o.id, later);
    }
}

/// The unit kind to make next from `mix`, among the kinds that `usable` says a factory can take now: the
/// one furthest below its share of the army (`have * weight` against the others, by cross-multiplication),
/// the earlier in the list among equals. `have` is the player's count per entry of `mix`.
pub fn pick_kind(
    mix: &[(Kind, u16)],
    have: &[u32],
    usable: impl Fn(Kind) -> bool,
) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (i, &(kind, weight)) in mix.iter().enumerate() {
        if !usable(kind) {
            continue;
        }
        best = match best {
            None => Some(i),
            Some(j) => {
                let (hi, hj) = (u64::from(have[i]), u64::from(have[j]));
                let (wi, wj) = (u64::from(weight), u64::from(mix[j].1));
                if hi * wj < hj * wi { Some(i) } else { Some(j) }
            }
        };
    }
    best
}

/// The complete factory of `function` with room in its queue and no order this turn, on the shortest queue
/// (the lowest id among equals; `obs.own` ascends by id).
fn factory_for(
    obs: &Observation,
    lens: &Lens<'_>,
    function: BuildingFunction,
    max_queue: usize,
    used: &[EntityId],
) -> Option<EntityId> {
    obs.own
        .iter()
        .filter(|o| complete(o) && function_of(lens, o) == Some(function) && !used.contains(&o.id))
        .filter(|o| o.factory.as_ref().map_or(0, |f| f.queue.len()) < max_queue)
        .min_by_key(|o| o.factory.as_ref().map_or(0, |f| f.queue.len()))
        .map(|o| o.id)
}

/// Whether a scout is due: the profile has one, the match has not used `scouts_max`, none is alive or
/// queued, and it is the first (from `scout_tick`) or a replacement (`scout_retry` ticks after the last was
/// seen lost).
fn scout_due(state: &AiState, obs: &Observation, kind: Kind) -> bool {
    let a = &state.profile.army;
    if state.scouts_sent >= a.scouts_max || have(obs, kind) > 0 || used_of(state, kind) > 0 {
        return false;
    }
    if state.scouts_sent == 0 {
        return obs.tick.0 >= u64::from(a.scout_tick);
    }
    state
        .scout_gone_at
        .is_some_and(|t| obs.tick.0 >= t.0 + u64::from(a.scout_retry))
}

/// Step (3).
fn queue(state: &mut AiState, obs: &Observation, lens: &Lens<'_>, out: &mut Vec<Command>) {
    // The economy comes first. (An emergency build waiting for money needs no check of its own here: it is
    // stalled only with no free credits at all, and then nothing below can be paid.)
    if economy::harvester_wanted(state, obs, lens) {
        return;
    }
    let e = state.profile.economy;
    let goal_open = base::goal_open(state, obs);
    let reserve = if goal_open { u64::from(e.reserve) } else { 0 };
    let holdback = if goal_open {
        reserve
            + base::next_build(state, obs, lens)
                .map_or(0, |k| base::saving_for(state, obs, lens, k))
    } else {
        0
    };
    let max_queue = usize::from(e.max_queue);
    let mix = state.profile.unit_mix.clone();
    let mut have_k: Vec<u32> = mix.iter().map(|&(k, _)| have(obs, k)).collect();
    let mut free = budget::free(obs, lens);
    let mut used: Vec<EntityId> = Vec::new();
    // At most one order per factory per turn, so at most one per factory function in this loop's passes.
    loop {
        let now = obs.tick;
        // The scout first, when it is due; then the mix.
        let scout = state
            .profile
            .scout
            .filter(|&k| !blocked(state, k, now) && scout_due(state, obs, k));
        let mut choice: Option<(Kind, EntityId, Option<usize>)> = None;
        if let Some(kind) = scout
            && let Some(function) = lens.unit_factory(kind)
            && let Some(factory) = factory_for(obs, lens, function, max_queue, &used)
        {
            choice = Some((kind, factory, None));
        }
        if choice.is_none() {
            let usable = |k: Kind| {
                !blocked(state, k, now)
                    && lens
                        .unit_factory(k)
                        .is_some_and(|f| factory_for(obs, lens, f, max_queue, &used).is_some())
            };
            if let Some(i) = pick_kind(&mix, &have_k, usable)
                && let Some(function) = lens.unit_factory(mix[i].0)
                && let Some(factory) = factory_for(obs, lens, function, max_queue, &used)
            {
                choice = Some((mix[i].0, factory, Some(i)));
            }
        }
        // Until the first scout is ordered it is the first unit made: no unit of the mix goes before it (unless
        // the scout is blocked, a refused order that waits `blocked_ticks`).
        if choice.as_ref().is_none_or(|c| c.2.is_some())
            && let Some(kind) = state.profile.scout
            && state.profile.army.scouts_max > 0
            && state.scouts_sent == 0
            && have(obs, kind) == 0
            && !blocked(state, kind, now)
        {
            return;
        }
        let Some((kind, factory, index)) = choice else {
            return;
        };
        let price = u64::from(lens.unit_cost(kind).unwrap_or(0));
        // The scout (once per match, and a replacement) is held back by the reserve only: it must go while the
        // goal list is still open, which is when it is useful, (it delays the list by 1,960 ticks on `ai_scout`, measured).
        let held = if index.is_none() { reserve } else { holdback };
        if free < held + price {
            return;
        }
        free -= price;
        let (units, queued) = (units_of(obs, kind), queued_in(obs, factory, kind));
        out.push(Command::QueueUnit { factory, kind });
        state.units_sent.push((factory, kind, now, units, queued));
        used.push(factory);
        match index {
            Some(i) => have_k[i] += 1,
            None => state.scouts_sent += 1,
        }
        state.units_sent.sort_by_key(|u| u.0);
    }
}

/// How many orders of `kind` this turn has sent already (they are not in the observation yet).
fn used_of(state: &AiState, kind: Kind) -> u32 {
    state
        .units_sent
        .iter()
        .filter(|&&(_, k, _, _, _)| k == kind)
        .count() as u32
}
