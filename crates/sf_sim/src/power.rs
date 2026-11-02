//! Power (WP6.4, `docs/rules/power.md`): what a player's standing structures make and use, what a
//! shortage does to building and production, and when defences go dark.
//!
//! **Made and used.** Each structure kind has a `power` number in the building table: positive makes
//! power, negative uses it. A player's `made` is the sum of what its **standing** producers make, its
//! `used` the sum (as a positive number) of what its standing consumers use. A structure under
//! construction, or at zero hit points, counts for neither: a building draws power from the tick it
//! stands (`World::stands`, `construction.md`), not while it goes up.
//!
//! **Damage lowers generation.** A producer with `hp` of `max` hit points makes `power * hp / max`,
//! rounded down (one integer rule, no number to tune; a structure that cannot be damaged, `max` zero
//! in the table, makes all of it). Consumers use their full number whatever their health: a damaged
//! barracks is not cheaper to run.
//!
//! **Shortage.** With `used <= made` (or nothing used) the speed is [`FULL_SPEED_PERMILLE`]. With
//! `used > made` it is `made * 1000 / used`, rounded down, but not below the world's
//! `shortage_floor_permille` ([`PowerRules`]). At zero everything of that player stops: its
//! factories show [`FactoryStatus::Stopped`](crate::FactoryStatus) and nothing advances or is paid.
//! One speed for everything: the factories use it as their progress per tick
//! ([`World::production_speed_permille`]); a structure under construction advances in exactly
//! `speed` of every thousand ticks, on the ticks where `tick * speed / 1000` steps up (no state of
//! its own, [`advances_on`]), so what a slow build costs stays `C * (k + 1) / T - C * k / T` per
//! tick it does advance and adds up to exactly `C`.
//!
//! **Defences.** [`World::defences_powered`] is the hook WP6.7 reads: true while the speed is at or
//! above `defence_cutoff_permille` (shipped 1000: any shortage switches them off). Nothing reads it
//! yet.
//!
//! **When it is worked out.** The totals are recounted once at the very start of every step, from the
//! state the previous step left, and kept (and hashed, section `I`). So what happens during a tick
//! (a plant completes, a shell hits it, it is destroyed) counts from the next tick on, and what the
//! snapshot shows is what that tick's production and construction used. One scan of the slots per
//! tick, only in a world given power rules.

use sf_math::hash::Fnv1a64;

use crate::ids::{PlayerId, Tick};
use crate::production::FULL_SPEED_PERMILLE;
use crate::terrain::NO_CELL;
use crate::world::{ConfigError, World};

/// The tuning numbers of power (`assets/config/power.ron`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PowerRules {
    /// The lowest speed a shortage can bring things down to, in thousandths: 0 to 1000. Zero means a
    /// player with no power at all is stopped; a world should keep it above zero, or a base that
    /// lost its last plant could never build another.
    pub shortage_floor_permille: u32,
    /// Defences work while the speed is at least this: 0 to 1000. 1000 switches them off in any
    /// shortage; 0 never switches them off.
    pub defence_cutoff_permille: u32,
}

impl PowerRules {
    /// Whether the numbers are inside their bounds.
    pub fn is_valid(self) -> bool {
        self.shortage_floor_permille <= FULL_SPEED_PERMILLE
            && self.defence_cutoff_permille <= FULL_SPEED_PERMILLE
    }
}

/// What a player makes and uses, in power units.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct PowerTotals {
    /// Power made by the standing producers, damage taken into account.
    pub made: u64,
    /// Power used by the standing consumers.
    pub used: u64,
}

/// The state of power: the rules and the totals of the last recount (ascending by player, only
/// players that make or use something).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Power {
    pub(crate) rules: PowerRules,
    pub(crate) totals: Vec<(PlayerId, PowerTotals)>,
}

/// The speed for these totals under `rules`: see the module text.
pub fn speed_for(totals: PowerTotals, rules: PowerRules) -> u32 {
    if totals.used == 0 || totals.made >= totals.used {
        return FULL_SPEED_PERMILLE;
    }
    let ratio = (totals.made * u64::from(FULL_SPEED_PERMILLE) / totals.used) as u32;
    ratio.max(rules.shortage_floor_permille)
}

/// Whether a structure under construction at `speed` thousandths advances in the step of `tick`:
/// exactly `speed` times in every thousand consecutive ticks, spread as evenly as whole ticks allow.
pub fn advances_on(tick: Tick, speed: u32) -> bool {
    let speed = u64::from(speed.min(FULL_SPEED_PERMILLE));
    let t = tick.0;
    (t + 1) * speed / 1000 > t * speed / 1000
}

impl Power {
    pub(crate) fn write_hash(&self, h: &mut Fnv1a64) {
        h.write_u32(self.rules.shortage_floor_permille);
        h.write_u32(self.rules.defence_cutoff_permille);
        h.write_u64(self.totals.len() as u64);
        for &(player, t) in &self.totals {
            h.write_u8(player.0);
            h.write_u64(t.made);
            h.write_u64(t.used);
        }
    }
}

impl World {
    /// The same world with power (WP6.4, `docs/rules/power.md`). Call it after the unit and building
    /// tables and before the world starts, and **before** [`World::with_match`]. Refused with
    /// [`ConfigError::Power`] after the start, twice, without the tables, after the match rules, or
    /// with numbers out of bounds. A world without it has no power: nothing is made or used, the
    /// speed is always full and it hashes as before.
    pub fn with_power(mut self, rules: PowerRules) -> Result<World, ConfigError> {
        if self.tick != Tick::ZERO
            || self.entities.len() != 0
            || !self.queue.is_empty()
            || self.power.is_some()
            || self.outcome.is_some()
            || !rules.is_valid()
            || self.units.is_empty()
            || self.buildings.is_empty()
        {
            return Err(ConfigError::Power);
        }
        self.power = Some(Power {
            rules,
            totals: Vec::new(),
        });
        Ok(self)
    }

    /// The tuning numbers of power; `None` for a world without it.
    pub fn power_rules(&self) -> Option<PowerRules> {
        self.power.as_ref().map(|p| p.rules)
    }

    /// What `player` makes and uses, as counted at the start of the last step. `None` for a world
    /// without power rules; zeros for a player with no producers and no consumers.
    pub fn power_of(&self, player: PlayerId) -> Option<PowerTotals> {
        let p = self.power.as_ref()?;
        Some(
            p.totals
                .binary_search_by_key(&player, |&(q, _)| q)
                .map_or(PowerTotals::default(), |k| p.totals[k].1),
        )
    }

    /// How fast things of `player` go, in thousandths ([`speed_for`]): full in a world without power
    /// rules, and for a player that uses nothing or makes enough.
    pub fn power_speed_permille(&self, player: PlayerId) -> u32 {
        match (&self.power, self.power_of(player)) {
            (Some(p), Some(t)) => speed_for(t, p.rules),
            _ => FULL_SPEED_PERMILLE,
        }
    }

    /// Whether the defences of `player` have power: the hook of WP6.7 (nothing reads it yet). Always
    /// true in a world without power rules.
    pub fn defences_powered(&self, player: PlayerId) -> bool {
        match &self.power {
            Some(p) => self.power_speed_permille(player) >= p.rules.defence_cutoff_permille,
            None => true,
        }
    }

    /// Recounts made and used for every player, from the standing structures in slot order. The
    /// first thing a step does.
    pub(crate) fn refresh_power(&mut self) {
        let Some(mut p) = self.power.take() else {
            return;
        };
        p.totals.clear();
        for i in 0..self.entities.len() {
            if !self.entities.alive[i] || self.entities.cell[i] == NO_CELL || !self.is_standing(i) {
                continue;
            }
            let Some(b) = self.buildings.get(self.entities.kind[i]) else {
                continue;
            };
            if b.power == 0 {
                continue;
            }
            let owner = self.entities.owner[i];
            let k = match p.totals.binary_search_by_key(&owner, |&(q, _)| q) {
                Ok(k) => k,
                Err(k) => {
                    p.totals.insert(k, (owner, PowerTotals::default()));
                    k
                }
            };
            if b.power > 0 {
                p.totals[k].1.made += self.generated_at(i, b.power as u64);
            } else {
                p.totals[k].1.used += u64::from(b.power.unsigned_abs());
            }
        }
        self.power = Some(p);
    }

    /// What the producer in slot `i` makes of its `full` output at its present hit points.
    fn generated_at(&self, i: usize, full: u64) -> u64 {
        let max = self.stats_at(i).map_or(0, |s| u64::from(s.hp));
        let hp = u64::from(self.entities.hp[i]);
        if max == 0 || hp >= max {
            full
        } else {
            full * hp / max
        }
    }
}

#[cfg(test)]
mod tests;
