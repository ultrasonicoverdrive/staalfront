//! The construction manager (WP9.1b-1, `docs/rules/ai.md`, "The construction turn"): the goal list, the
//! power rule, the money rule and the placement, once every 40 ticks (one think in four).
//!
//! One turn, in order:
//!
//! (1) **Verify.** The build sent at the last turn (`pending_build`) must stand (complete or going up) at
//!    its cell; if not, the simulation refused it (a unit stepped on the cell, the ground changed), and
//!    `(kind, cell)` goes into `refused` for `blocked_ticks`. Either way the latch is cleared. Expired
//!    entries of `refused` and `blocked_until` are dropped.
//! (2) **Parallel cap.** With `max_parallel_builds` own structures going up, nothing more starts.
//! (3) **Choose.** The ore silo when the economy turn asked for it (`emergency`, WP9.1b-2), else the first goal
//!    entry `(kind, count)` with fewer than `count` standing or going up, not blocked, whose `requires` all
//!    stand.
//! (4) **Power first.** With the nominal numbers of the table (a damaged plant counts in full): when the
//!    power made is below the power used plus what the choice uses plus `power_margin`, a power plant (the
//!    first of the goal list) is built instead, unless the plant is blocked (then the choice stands).
//! (5) **Afford.** The full price is saved first: free credits at least the cost, or at least
//!    `capacity * silo_percent / 100` when that is less (the store cannot hold more). An emergency starts
//!    unfunded: a refinery when none stands or goes up, a power plant when less power is made than used.
//!    With no harvester at all and a refinery standing, a normal build also needs its full price plus a
//!    harvester's (not capped: the memo's rule).
//! (6) **Place** ([`crate::placement::search`]). Found: `BuildStructure` is sent and latched. Nothing
//!    fits: the kind is blocked for `blocked_ticks` and the next entry gets its turn next time.

use sf_sim::{BuildingFunction, Command, Kind, Tick};

use crate::budget;
use crate::observe::{Lens, Observation};
use crate::placement;
use crate::state::{AiState, REFUSED_CAP};

/// One construction turn; see the module documentation.
pub fn turn(state: &mut AiState, obs: &Observation, lens: &mut Lens<'_>, out: &mut Vec<Command>) {
    let now = obs.tick;
    let blocked_ticks = u64::from(state.profile.economy.blocked_ticks);
    let later = Tick(now.0 + blocked_ticks);

    // (1) Verify the last build.
    if let Some((kind, cell, _)) = state.pending_build.take() {
        let stands = obs
            .own
            .iter()
            .any(|o| o.kind == kind && o.cell == cell && o.footprint.is_some());
        if !stands {
            refuse(state, kind, cell, later);
        }
    }
    state.refused.retain(|_, until| *until > now);
    state.blocked_until.retain(|_, until| *until > now);

    // (2) The parallel cap.
    let going_up = obs.own.iter().filter(|o| o.building.is_some()).count();
    if going_up >= usize::from(state.profile.economy.max_parallel_builds) {
        return;
    }

    // (3) Choose.
    let Some(chosen) = choose(state, obs, lens) else {
        return;
    };

    // (4) Power first.
    let power = nominal_power(obs, lens);
    let kind = power_first(state, obs, lens, chosen, power);

    // (5) Afford.
    if !affordable(state, obs, lens, kind, power) {
        return;
    }

    // (6) Place.
    match placement::search(state, obs, lens, kind) {
        Some(cell) => {
            out.push(Command::BuildStructure { kind, cell });
            state.pending_build = Some((kind, cell, now));
        }
        None => {
            state.blocked_until.insert(kind, later);
        }
    }
}

/// Puts `(kind, cell)` into `refused` until `until`; when that makes it longer than [`REFUSED_CAP`], the
/// entry that expires first (the smallest key among equals) goes.
pub fn refuse(state: &mut AiState, kind: Kind, cell: sf_sim::Cell, until: Tick) {
    state.refused.insert((kind, cell), until);
    while state.refused.len() > REFUSED_CAP {
        let Some(first) = state
            .refused
            .iter()
            .min_by_key(|(key, t)| (**t, **key))
            .map(|(key, _)| *key)
        else {
            break;
        };
        state.refused.remove(&first);
    }
}

/// How many of `kind` the player has standing or going up.
pub(crate) fn have(obs: &Observation, kind: Kind) -> usize {
    obs.own
        .iter()
        .filter(|o| {
            o.kind == kind
                && o.footprint.is_some()
                && !o.selling
                && (o.standing || o.building.is_some())
        })
        .count()
}

/// The kinds of the player's standing structures, ascending.
fn standing_kinds(obs: &Observation) -> Vec<Kind> {
    let mut kinds: Vec<Kind> = obs
        .own
        .iter()
        .filter(|o| o.footprint.is_some() && o.standing && !o.selling)
        .map(|o| o.kind)
        .collect();
    kinds.sort();
    kinds.dedup();
    kinds
}

fn blocked(state: &AiState, kind: Kind, now: Tick) -> bool {
    state.blocked_until.get(&kind).is_some_and(|t| *t > now)
}

fn requirements_stand(lens: &Lens<'_>, kind: Kind, standing: &[Kind]) -> bool {
    lens.building(kind)
        .is_some_and(|b| b.requires.iter().all(|r| standing.binary_search(r).is_ok()))
}

/// The economy's emergency (the ore silo) when it is not blocked and its prerequisites stand, else the
/// first goal entry that is not satisfied, not blocked and whose prerequisites stand.
fn choose(state: &AiState, obs: &Observation, lens: &Lens<'_>) -> Option<Kind> {
    let standing = standing_kinds(obs);
    if let Some(kind) = state.emergency
        && !blocked(state, kind, obs.tick)
        && requirements_stand(lens, kind, &standing)
    {
        return Some(kind);
    }
    state
        .profile
        .build_order
        .iter()
        .find(|&&(kind, count)| {
            have(obs, kind) < usize::from(count)
                && !blocked(state, kind, obs.tick)
                && requirements_stand(lens, kind, &standing)
        })
        .map(|&(kind, _)| kind)
}

/// Whether a goal entry is open: some `(kind, count)` of the list has fewer than `count` standing or going
/// up, blocked or not (WP9.1c-1: the silo rule and the production money rule read the same test).
pub fn goal_open(state: &AiState, obs: &Observation) -> bool {
    state
        .profile
        .build_order
        .iter()
        .any(|&(k, count)| have(obs, k) < usize::from(count))
}

/// What the construction turn would build now, before the money question: the choice and the power rule
/// (steps (3) and (4)). `None` when nothing is chosen (the list is done, or every open entry is blocked
/// or waits for a prerequisite).
pub fn next_build(state: &AiState, obs: &Observation, lens: &Lens<'_>) -> Option<Kind> {
    let chosen = choose(state, obs, lens)?;
    Some(power_first(
        state,
        obs,
        lens,
        chosen,
        nominal_power(obs, lens),
    ))
}

/// What the construction turn saves before it starts `kind`: its price, or what the store can hold when that
/// is less (the cap of step (5)).
pub fn saving_for(state: &AiState, obs: &Observation, lens: &Lens<'_>, kind: Kind) -> u64 {
    let cost = lens
        .building(kind)
        .and_then(|b| b.cost)
        .map_or(0, u64::from);
    if obs.storage_capacity > 0 {
        cost.min(obs.storage_capacity * u64::from(state.profile.economy.silo_percent) / 100)
    } else {
        cost
    }
}

/// `(made, used)` by the table: the `power` of every own structure standing or going up, a producer at
/// its full number whatever its damage. `None` in a game without power rules.
fn nominal_power(obs: &Observation, lens: &Lens<'_>) -> Option<(u64, u64)> {
    obs.power?;
    let (mut made, mut used) = (0u64, 0u64);
    for o in &obs.own {
        if o.footprint.is_none() || !(o.standing || o.building.is_some()) {
            continue;
        }
        if let Some(b) = lens.building(o.kind) {
            if b.power > 0 {
                made += u64::from(b.power.unsigned_abs());
            } else {
                used += u64::from(b.power.unsigned_abs());
            }
        }
    }
    Some((made, used))
}

/// The power rule: `chosen`, or the goal list's power plant when `chosen` would leave less than
/// `power_margin` spare.
fn power_first(
    state: &AiState,
    obs: &Observation,
    lens: &Lens<'_>,
    chosen: Kind,
    power: Option<(u64, u64)>,
) -> Kind {
    let Some((made, used)) = power else {
        return chosen;
    };
    let uses = lens.building(chosen).map_or(0, |b| {
        if b.power < 0 {
            u64::from(b.power.unsigned_abs())
        } else {
            0
        }
    });
    if made >= used + uses + u64::from(state.profile.economy.power_margin) {
        return chosen;
    }
    let plant = state
        .profile
        .build_order
        .iter()
        .map(|&(k, _)| k)
        .find(|&k| {
            lens.building(k)
                .is_some_and(|b| b.function == BuildingFunction::PowerPlant)
        });
    match plant {
        Some(p)
            if !blocked(state, p, obs.tick)
                && requirements_stand(lens, p, &standing_kinds(obs)) =>
        {
            p
        }
        _ => chosen,
    }
}

/// Whether one of the player's structures of `function` stands or goes up.
fn has_function(
    obs: &Observation,
    lens: &Lens<'_>,
    function: BuildingFunction,
    standing_only: bool,
) -> bool {
    obs.own.iter().any(|o| {
        o.footprint.is_some()
            && (o.standing || (!standing_only && o.building.is_some()))
            && lens
                .building(o.kind)
                .is_some_and(|b| b.function == function)
    })
}

/// The money rule (step (5) of the turn).
fn affordable(
    state: &AiState,
    obs: &Observation,
    lens: &Lens<'_>,
    kind: Kind,
    power: Option<(u64, u64)>,
) -> bool {
    let Some(b) = lens.building(kind) else {
        return false;
    };
    let emergency = match b.function {
        BuildingFunction::HarvesterDock => {
            !has_function(obs, lens, BuildingFunction::HarvesterDock, false)
        }
        BuildingFunction::PowerPlant => power.is_some_and(|(made, used)| made < used),
        _ => false,
    };
    if emergency {
        return true;
    }
    let cost = u64::from(b.cost.unwrap_or(0));
    let free = budget::free(obs, lens);
    // The full price, or what the store can hold when that is less.
    let need = saving_for(state, obs, lens, kind);
    if free < need {
        return false;
    }
    let harvesters = lens.harvester_kinds();
    let have_harvester = obs.own.iter().any(|o| {
        harvesters.contains(&o.kind)
            || o.factory
                .as_ref()
                .is_some_and(|f| f.queue.iter().any(|(k, _)| harvesters.contains(k)))
    });
    // The harvester reserve: the price and a harvester's, not capped (design memo section 2, step 5).
    if !have_harvester && has_function(obs, lens, BuildingFunction::HarvesterDock, true) {
        let harvester = harvesters
            .iter()
            .find_map(|&h| lens.unit_cost(h))
            .map_or(0, u64::from);
        return free >= cost + harvester;
    }
    true
}
