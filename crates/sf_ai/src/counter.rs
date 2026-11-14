//! The counter mix (WP9.2b-1, `docs/rules/ai.md`, "Counter mix (WP9.2b-1)"): the AI tallies the armed enemy units it
//! sees and leans its unit mix toward the roles that do best against them, per credit. Everything here is off with
//! `counter_permille` 0: nothing is tallied, the weights are the base weights scaled by 1,000, and the digest has the
//! layout it had before.
//!
//! * **Tally** ([`tally`], every think): per kind, the count of units in the observation's `visible` list that are
//!   armed and have no footprint, kept as a peak with the tick it was seen, and dropped `intel_ticks` after that
//!   tick. Unarmed units (harvesters), the scout kind of the profile (it carries a pistol but is no army), structures
//!   (remembered ones included) and the AI's own units never
//!   count.
//! * **Share** ([`shares`]): the value (count times price) of every tallied kind goes to the mix role that does best
//!   against it **per credit** ([`best_role`], the lens's [`Lens::counter_score`] cross-multiplied with the prices
//!   in `u128`); the earlier entry of the mix wins a tie, and nobody wins when every score is zero; a role's share is
//!   its value as thousandths of everything tallied.
//! * **Weight** ([`weights`]): `base * (1000 + counter_permille * share / 1000)`, at most `2000 * base`, and no lower
//!   clamp: a role that is not the counter keeps its base weight. [`crate::production::pick_kind`] reads the result.

use std::collections::BTreeMap;

use sf_sim::Kind;

use crate::observe::{Lens, Observation};
use crate::state::AiState;

/// The largest factor of a base weight, in thousandths: a role is never made more than twice as often as its base.
pub const WEIGHT_CLAMP: u32 = 2000;

/// Updates `state.intel` from this think's observation. Does nothing with the rule off.
pub fn tally(state: &mut AiState, obs: &Observation) {
    if !state.profile.army.counter_on() {
        return;
    }
    let keep = u64::from(state.profile.army.intel_ticks);
    // A peak is forgotten `intel_ticks` after its tick: still known at `tick + keep - 1`, gone at `tick + keep`.
    state
        .intel
        .retain(|_, (_, seen)| obs.tick.0 < seen.0 + keep);
    let mut now: BTreeMap<Kind, u32> = BTreeMap::new();
    for s in &obs.visible {
        // The scout kind carries a pistol (an armed kind in the public table) but a scout is no army: one of them
        // would move the shares of a small army by a tenth.
        if s.footprint.is_none()
            && s.armed
            && s.owner != obs.player
            && Some(s.kind) != state.profile.scout
        {
            *now.entry(s.kind).or_insert(0) += 1;
        }
    }
    for (kind, n) in now {
        match state.intel.get_mut(&kind) {
            // At least the peak: the count and the tick are fresh (the same count seen again keeps the peak alive).
            Some(peak) if n < peak.0 => {}
            Some(peak) => *peak = (n, obs.tick),
            None => {
                state.intel.insert(kind, (n, obs.tick));
            }
        }
    }
}

/// The entry of `mix` that does best against `enemy` per credit: the largest `score / price`, found by
/// cross-multiplying in `u128`; the earlier entry wins a tie; `None` when every score is 0 (or no role has a price).
pub fn best_role(lens: &Lens<'_>, mix: &[(Kind, u16)], enemy: Kind) -> Option<usize> {
    let mut best: Option<(usize, u128, u128)> = None;
    for (i, &(role, _)) in mix.iter().enumerate() {
        let price = u128::from(lens.unit_cost(role).unwrap_or(0));
        let score = u128::from(lens.counter_score(role, enemy));
        if price == 0 || score == 0 {
            continue;
        }
        best = match best {
            Some((_, bs, bp)) if score * bp <= bs * price => best,
            _ => Some((i, score, price)),
        };
    }
    best.map(|(i, _, _)| i)
}

/// The share of every entry of `mix` in thousandths (see the module documentation); all 0 with nothing tallied. A
/// tallied kind with no price cannot be valued and is left out; a kind nobody counters still counts in the total, so
/// the shares then add up to less than a thousand.
pub fn shares(state: &AiState, lens: &Lens<'_>) -> Vec<u32> {
    let mix = &state.profile.unit_mix;
    let mut value = vec![0u128; mix.len()];
    let mut total = 0u128;
    for (&enemy, &(n, _)) in &state.intel {
        let worth = u128::from(n) * u128::from(lens.unit_cost(enemy).unwrap_or(0));
        if worth == 0 {
            continue;
        }
        total += worth;
        if let Some(i) = best_role(lens, mix, enemy) {
            value[i] += worth;
        }
    }
    value
        .iter()
        .map(|&v| (v * 1000).checked_div(total).unwrap_or(0) as u32)
        .collect()
}

/// The weight of a role with `base` and `share` (thousandths) at `permille`: see the module documentation.
pub fn weight(base: u16, share: u32, permille: u16) -> u32 {
    let base = u64::from(base);
    let factor = 1000 + u64::from(permille) * u64::from(share) / 1000;
    (base * factor.min(u64::from(WEIGHT_CLAMP))) as u32
}

/// The weights `pick_kind` reads, one per entry of the unit mix, in thousandths of a base unit. With the rule off the
/// base weights times 1,000 (the same order as the base weights, whatever the counts).
pub fn weights(state: &AiState, lens: &Lens<'_>) -> Vec<(Kind, u32)> {
    let mix = &state.profile.unit_mix;
    let a = &state.profile.army;
    if !a.counter_on() || state.intel.is_empty() {
        return mix.iter().map(|&(k, w)| (k, u32::from(w) * 1000)).collect();
    }
    let share = shares(state, lens);
    mix.iter()
        .zip(share)
        .map(|(&(k, w), s)| (k, weight(w, s, a.counter_permille)))
        .collect()
}
