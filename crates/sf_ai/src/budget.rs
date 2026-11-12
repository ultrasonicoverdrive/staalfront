//! The money budget (WP9.1b-1, `docs/rules/ai.md`, "The budget"): what the AI may still spend.
//!
//! The simulation never refuses a build or a unit for lack of money: it accepts the command and the work
//! waits for credits (`docs/rules/economy.md`). So the AI keeps its own count, and keeps it **stateless**:
//! every turn it is worked out again from the observation, so there is no ledger to drift. **Committed** is
//! the unpaid part of every own structure under construction (`cost * left / total`, rounded up) plus the
//! unpaid price of every queued unit that is not free (the front item minus what it has paid). **Free** is
//! the credits minus that, never below zero.

use crate::observe::{Lens, Observation};

/// The credits already promised to work that has started or is queued.
pub fn committed(obs: &Observation, lens: &Lens<'_>) -> u64 {
    let mut sum = 0u64;
    for o in &obs.own {
        if let Some(c) = o.building
            && let Some(cost) = lens.building(o.kind).and_then(|b| b.cost)
        {
            sum += (u64::from(cost) * u64::from(c.left)).div_ceil(u64::from(c.total.max(1)));
        }
        if let Some(f) = &o.factory {
            for (i, &(kind, free)) in f.queue.iter().enumerate() {
                if free {
                    continue;
                }
                let price = u64::from(lens.unit_cost(kind).unwrap_or(0));
                let paid = if i == 0 { f.front_paid } else { 0 };
                sum += price.saturating_sub(paid);
            }
        }
    }
    sum
}

/// The credits not promised yet: `credits - committed`, at least zero.
pub fn free(obs: &Observation, lens: &Lens<'_>) -> u64 {
    obs.credits.saturating_sub(committed(obs, lens))
}
