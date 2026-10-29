//! Money (WP7.4, `docs/rules/economy.md`, "Money"): the credits of a player and what a structure
//! costs while it is built.
//!
//! **The store is the money pool.** The ore a refinery takes off a harvester goes into the
//! player's store (WP7.3) as credits, `credits_per_ore` for each unit, and what is spent comes out
//! of the same number. So spending makes room, and a harvester waiting at a full refinery goes on
//! as soon as enough was spent. Starting credits are put into the store when the world is made;
//! the store may then hold more than its capacity (as when a refinery is lost), and nothing more
//! goes in until it is below it again.
//!
//! **A structure is paid for while it is built.** Of a structure of price `C` and `T` build ticks
//! the tick that takes it from `k` ticks done to `k + 1` costs `C * (k + 1) / T - C * k / T`
//! (integer division), so the whole price is paid exactly when the last tick is, and the cost of a
//! tick is a function of the tick's index alone: no state is kept next to `build_left`. If the
//! owner does not hold that much the structure does not advance in that tick (it is *stalled*;
//! [`World::stalled`]) and nothing is paid; it tries again next tick. Structures under
//! construction are served in ascending slot order from the same store, so with too little money
//! for all, the older slot is paid first.
//!
//! Nothing is refunded: a structure that is destroyed or removed half built keeps what was paid
//! for it. [`Command::PlaceStructure`](crate::Command::PlaceStructure) (mission objects) is not
//! built and costs nothing. A structure with no price, or with no build ticks (it is complete the
//! moment it is placed), is free. Units will cost money when production exists (WP6.5): it is to
//! call [`World::pay`], which takes the whole sum or nothing.

use crate::ids::{EntityId, PlayerId};
use crate::world::{ConfigError, World};

/// Most credits one player can be given at the start.
pub const MAX_STARTING_CREDITS: u64 = 1_000_000_000;
/// Most credits one unit of ore can be worth.
pub const MAX_CREDITS_PER_ORE: u32 = 1000;

/// The money rules of a world.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MoneyRules {
    /// Credits one unit of ore is worth when a refinery takes it: 1 to [`MAX_CREDITS_PER_ORE`].
    pub credits_per_ore: u32,
}

/// The state next to the store: the rules and what each player has spent so far (a statistic,
/// ascending by player, hashed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Money {
    pub(crate) rules: MoneyRules,
    pub(crate) spent: Vec<(PlayerId, u64)>,
}

/// What the tick that takes a structure of price `cost` and `total` build ticks from `done` to
/// `done + 1` ticks done costs. Zero when `total` is zero.
pub fn tick_cost(cost: u32, total: u32, done: u32) -> u64 {
    if total == 0 {
        return 0;
    }
    let (c, t) = (u64::from(cost), u64::from(total));
    let at = |k: u64| c * k / t;
    at(u64::from(done) + 1) - at(u64::from(done))
}

impl World {
    /// The same world with money (WP7.4): a unit of ore taken by a refinery is worth
    /// `rules.credits_per_ore` credits in the store, each `(player, credits)` of `starting`
    /// begins with that many, and the structures players build are paid for while they go up.
    /// Call it after [`World::with_storage`] (the store is the pool) and before the world starts.
    /// Refused with [`ConfigError::Money`] without storage rules, given twice, after the start,
    /// with a price outside `1..=MAX_CREDITS_PER_ORE`, a starting sum above
    /// [`MAX_STARTING_CREDITS`], or a player listed twice. A world without it builds for free and
    /// hashes as before.
    pub fn with_money(
        mut self,
        rules: MoneyRules,
        starting: &[(PlayerId, u64)],
    ) -> Result<World, ConfigError> {
        let mut starting = starting.to_vec();
        starting.sort_unstable_by_key(|&(p, _)| p);
        let bad = self.money.is_some()
            || self.tick != crate::ids::Tick::ZERO
            || self.entities.len() != 0
            || !self.queue.is_empty()
            || rules.credits_per_ore == 0
            || rules.credits_per_ore > MAX_CREDITS_PER_ORE
            || starting.windows(2).any(|p| p[0].0 == p[1].0)
            || starting.iter().any(|&(_, n)| n > MAX_STARTING_CREDITS);
        match self.harvest.as_deref_mut() {
            Some(h) if !bad && h.has_storage() => {
                h.set_money(rules.credits_per_ore, &starting);
                self.money = Some(Money {
                    rules,
                    spent: Vec::new(),
                });
                Ok(self)
            }
            _ => Err(ConfigError::Money),
        }
    }

    /// Whether the world was given money rules ([`World::with_money`]).
    pub fn has_money(&self) -> bool {
        self.money.is_some()
    }

    /// The money rules; `None` for a world without them.
    pub fn money_rules(&self) -> Option<MoneyRules> {
        self.money.as_ref().map(|m| m.rules)
    }

    /// The credits `player` holds: the number in its store. Zero without storage rules. With
    /// money rules this is the money; without, it is the ore stored, the same number
    /// ([`World::ore_stored`]).
    pub fn credits(&self, player: PlayerId) -> u64 {
        self.ore_stored(player)
    }

    /// The credits `player` has spent so far. Zero without money rules.
    pub fn credits_spent(&self, player: PlayerId) -> u64 {
        self.money.as_ref().map_or(0, |m| {
            m.spent
                .binary_search_by_key(&player, |&(p, _)| p)
                .map_or(0, |k| m.spent[k].1)
        })
    }

    /// The structures under construction that did not advance in the last step because their
    /// owner could not pay for the tick, in ascending slot order. Replaced by every step; empty
    /// without money rules.
    pub fn stalled(&self) -> &[EntityId] {
        &self.stalled
    }

    /// Whether the structure `id` is stalled for lack of money ([`World::stalled`]).
    pub fn is_stalled(&self, id: EntityId) -> bool {
        self.stalled.contains(&id)
    }

    /// Takes `n` credits from `player`: all of them and `true`, or nothing and `false` when the
    /// store holds less. Always `false` without money rules. The hook for the cost of units
    /// (WP6.5).
    pub(crate) fn pay(&mut self, player: PlayerId, n: u64) -> bool {
        let (Some(money), Some(harvest)) = (&mut self.money, self.harvest.as_deref_mut()) else {
            return false;
        };
        if !harvest.spend(player, n) {
            return false;
        }
        if n > 0 {
            match money.spent.binary_search_by_key(&player, |&(p, _)| p) {
                Ok(k) => money.spent[k].1 = money.spent[k].1.saturating_add(n),
                Err(k) => money.spent.insert(k, (player, n)),
            }
        }
        true
    }

    /// What the next tick of the structure in slot `i` costs: zero without money rules, without
    /// a price or without build ticks.
    pub(crate) fn construction_charge(&self, i: usize) -> u64 {
        if self.money.is_none() {
            return 0;
        }
        let Some(b) = self.buildings.get(self.entities.kind[i]) else {
            return 0;
        };
        let (Some(cost), total) = (b.cost, b.build_ticks) else {
            return 0;
        };
        let left = self.entities.build_left[i].min(total);
        tick_cost(cost, total, total - left)
    }

    /// Writes the money section (`M`) of the checksum.
    pub(crate) fn write_money_hash(&self, h: &mut sf_math::hash::Fnv1a64) {
        let Some(m) = &self.money else {
            return;
        };
        h.write_u32(m.rules.credits_per_ore);
        h.write_u64(m.spent.len() as u64);
        for &(player, n) in &m.spent {
            h.write_u8(player.0);
            h.write_u64(n);
        }
    }
}
