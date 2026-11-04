//! Repair and sale of structures (WP6.6, `docs/rules/repair_sell.md`).
//!
//! **Repair** is a process of the owner's structure. [`Command::Repair`](crate::Command::Repair)
//! `{ on: true }` sets a flag; while it is set the structure gains `hp_per_tick` hit points per tick
//! and its owner pays per tick. The price is one integer rule on the *absolute* hit points, the
//! same trick as construction (`C * (k + 1) / T - C * k / T`): the credits paid to bring a structure
//! of `max` hit points and price `C` up to `h` hit points are `cum(h) = C * permille * h / (max *
//! 1000)` (whole credits, rounded down), so a tick from `a` to `b` costs `cum(b) - cum(a)`. The
//! ticks of any repair therefore add up to exactly `cum(end) - cum(start)` whatever the pace, and a
//! full repair from nothing costs exactly `C * permille / 1000`. The flag clears by itself when the
//! structure is at full hit points, when it no longer stands, and on the stop command. A structure
//! whose owner cannot pay for the next tick *waits* (nothing is paid, no hit points are gained, the
//! flag stays) and goes on when the credits are there. A power shortage slows repair like it slows
//! construction: it advances only on the ticks [`advances_on`](crate::advances_on) names.
//!
//! **Sale** is a process too. [`Command::Sell`](crate::Command::Sell) starts a countdown of
//! `sell_ticks`; the structure counts as gone from that moment (`World::stands` is false: it gives
//! no power and no prerequisite, builds nothing, fires nothing, keeps no storage and is no
//! structure for the match). When the countdown reaches zero the structure is removed through the
//! ordinary removal (`World::despawn`: no wreck, no death in the tally) and the owner is paid
//! `price * refund_permille * hp / (max_hp * 1000)` for the hit points it has at that moment,
//! rounded down, once. A structure that is destroyed during the sale pays nothing.
//!
//! **When it happens.** Both advance in [`World::advance_repair_sell`], in ascending slot order,
//! right after the structures under construction and before the commands of the tick, so a repair
//! started in tick `t` gains its first hit points in tick `t + 1` and a sale started in tick `t`
//! with `n` ticks ends in the step of tick `t + n`.
//!
//! Integers only; the state is two columns of [`Entities`](crate::entities::Entities) and the
//! section `r` of the checksum, written only while some structure repairs or is being sold.

use sf_math::hash::Fnv1a64;

use crate::command::Command;
use crate::ids::{EntityId, Kind, PlayerId, Tick};
use crate::power::advances_on;
use crate::terrain::NO_CELL;
use crate::world::{ConfigError, World};

/// Most hit points per tick the rules accept.
pub const MAX_REPAIR_HP_PER_TICK: u16 = 1000;
/// Most ticks a sale may take (five minutes).
pub const MAX_SELL_TICKS: u32 = 6000;

/// The numbers of repair and sale (`assets/config/repair.ron`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RepairRules {
    /// Hit points a repairing structure gains in a tick at full speed: 1 to
    /// [`MAX_REPAIR_HP_PER_TICK`].
    pub hp_per_tick: u16,
    /// What a repair from 0 to full hit points costs, in thousandths of the structure's price: 0
    /// to 1000.
    pub full_cost_permille: u32,
    /// Ticks a sale takes: 1 to [`MAX_SELL_TICKS`].
    pub sell_ticks: u32,
    /// The part of the price paid back for a structure at full hit points, in thousandths: 0 to
    /// 1000.
    pub refund_permille: u32,
}

impl RepairRules {
    /// Whether the numbers are inside their bounds.
    pub fn is_valid(self) -> bool {
        (1..=MAX_REPAIR_HP_PER_TICK).contains(&self.hp_per_tick)
            && self.full_cost_permille <= 1000
            && (1..=MAX_SELL_TICKS).contains(&self.sell_ticks)
            && self.refund_permille <= 1000
    }
}

/// Why a repair or sell command was refused.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RepairError {
    /// The id is stale, or not a structure.
    NoSuchStructure,
    /// The structure is not the sender's (an enemy's or a neutral one).
    NotYours,
    /// The structure is still under construction.
    UnderConstruction,
    /// The structure is being sold.
    BeingSold,
    /// The structure has no hit points left (it is a wreck) or cannot be damaged.
    NotStanding,
    /// A repair was asked for a structure at full hit points, or one that cannot be damaged.
    FullHitPoints,
    /// A repair was asked for a structure that is already repairing.
    AlreadyRepairing,
    /// A stop was asked for a structure that is not repairing.
    NotRepairing,
}

/// A refused repair or sell command.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RepairRejection {
    /// The tick of the refusal.
    pub tick: Tick,
    /// Who sent the command.
    pub player: PlayerId,
    /// The structure it named.
    pub structure: EntityId,
    /// Why.
    pub reason: RepairError,
}

/// A sale that finished in the last step: the hook for a mission trigger.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Sale {
    /// The tick whose step finished it.
    pub tick: Tick,
    /// The structure that is gone (its id is stale now).
    pub id: EntityId,
    /// Its kind.
    pub kind: Kind,
    /// Its owner.
    pub owner: PlayerId,
    /// The credits paid to the owner (0 in a world without money rules).
    pub refund: u64,
}

/// What a structure being repaired is doing, for the presentation.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RepairStatus {
    /// Gaining hit points (or about to, on a tick power allows).
    Repairing,
    /// The owner cannot pay for the next tick; nothing is paid, nothing is gained.
    WaitingForMoney,
}

/// How far a sale is.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SaleProgress {
    /// Ticks still to go; at least 1.
    pub left: u32,
    /// Ticks in all (the rules' `sell_ticks`).
    pub total: u32,
}

impl SaleProgress {
    /// Progress in thousandths, rounded down; 0 in the tick the sale started, never 1000.
    pub fn permille(self) -> u16 {
        if self.total == 0 {
            return 0;
        }
        let done = u64::from(self.total.saturating_sub(self.left));
        (done * 1000 / u64::from(self.total)).min(999) as u16
    }
}

/// The state of repair and sale: the rules and what the last step did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RepairSell {
    pub(crate) rules: RepairRules,
    pub(crate) rejections: Vec<RepairRejection>,
    /// Structures that could not pay in the last step, ascending by slot; derived, not hashed.
    pub(crate) waiting: Vec<EntityId>,
    /// Sales finished in the last step; derived, not hashed.
    pub(crate) sold: Vec<Sale>,
}

/// What a repair from `from` to `to` hit points costs (whole credits): the cumulative cost at `to`
/// minus the one at `from`, where the cumulative cost at `h` is `price * full_cost_permille * h /
/// (max_hp * 1000)`. Zero for `max_hp` zero, and for `to <= from`.
pub fn repair_cost(price: u32, full_cost_permille: u32, max_hp: u32, from: u32, to: u32) -> u64 {
    if max_hp == 0 || to <= from {
        return 0;
    }
    let at = |h: u32| {
        u128::from(price) * u128::from(full_cost_permille) * u128::from(h.min(max_hp))
            / (u128::from(max_hp) * 1000)
    };
    (at(to) - at(from)) as u64
}

/// What selling a structure of price `price` with `hp` of `max_hp` hit points pays: `price *
/// refund_permille * hp / (max_hp * 1000)`, rounded down. A structure that cannot be damaged
/// (`max_hp` zero) pays the full refund.
pub fn sale_refund(price: u32, refund_permille: u32, hp: u32, max_hp: u32) -> u64 {
    let hp = if max_hp == 0 { 1 } else { hp.min(max_hp) };
    let max = if max_hp == 0 { 1 } else { max_hp };
    (u128::from(price) * u128::from(refund_permille) * u128::from(hp) / (u128::from(max) * 1000))
        as u64
}

impl RepairSell {
    pub(crate) fn write_hash(&self, h: &mut Fnv1a64) {
        h.write_bytes(&self.rules.hp_per_tick.to_le_bytes());
        h.write_u32(self.rules.full_cost_permille);
        h.write_u32(self.rules.sell_ticks);
        h.write_u32(self.rules.refund_permille);
    }
}

impl World {
    /// The same world with repair and sale (WP6.6, `docs/rules/repair_sell.md`). Call it after the
    /// unit and building tables and before the world starts. Refused with [`ConfigError::Repair`]
    /// after the start, twice, without the tables, or with numbers out of bounds. A world without
    /// it refuses `Repair` and `Sell` (they count as ignored) and hashes as before.
    pub fn with_repair(mut self, rules: RepairRules) -> Result<World, ConfigError> {
        if self.tick != Tick::ZERO
            || self.entities.len() != 0
            || !self.queue.is_empty()
            || self.repair.is_some()
            || !rules.is_valid()
            || self.units.is_empty()
            || self.buildings.is_empty()
        {
            return Err(ConfigError::Repair);
        }
        self.repair = Some(Box::new(RepairSell {
            rules,
            rejections: Vec::new(),
            waiting: Vec::new(),
            sold: Vec::new(),
        }));
        Ok(self)
    }

    /// The repair and sale numbers; `None` for a world without them.
    pub fn repair_rules(&self) -> Option<RepairRules> {
        self.repair.as_ref().map(|r| r.rules)
    }

    /// The repair and sell commands refused in the last step.
    pub fn repair_rejections(&self) -> &[RepairRejection] {
        self.repair.as_ref().map_or(&[], |r| &r.rejections)
    }

    /// The sales that finished in the last step.
    pub fn sales(&self) -> &[Sale] {
        self.repair.as_ref().map_or(&[], |r| &r.sold)
    }

    /// Whether the structure `id` is being repaired (the flag is set; it may be waiting for money).
    pub fn is_repairing(&self, id: EntityId) -> bool {
        self.live_index(id)
            .is_some_and(|i| self.entities.repairing[i])
    }

    /// What the repair of `id` is doing, or `None` when it is not repairing.
    pub fn repair_status(&self, id: EntityId) -> Option<RepairStatus> {
        let i = self.live_index(id)?;
        if !self.entities.repairing[i] {
            return None;
        }
        let waiting = self
            .repair
            .as_ref()
            .is_some_and(|r| r.waiting.contains(&id));
        Some(if waiting {
            RepairStatus::WaitingForMoney
        } else {
            RepairStatus::Repairing
        })
    }

    /// How far the sale of `id` is, or `None` when it is not being sold.
    pub fn sale_of(&self, id: EntityId) -> Option<SaleProgress> {
        let i = self.live_index(id)?;
        let left = self.entities.sell_left[i];
        let total = self.repair.as_ref()?.rules.sell_ticks;
        (left > 0).then_some(SaleProgress { left, total })
    }

    /// Every structure that repairs, ascending by slot.
    pub fn repairing(&self) -> impl Iterator<Item = EntityId> + '_ {
        (0..self.entities.len())
            .filter(|&i| self.entities.alive[i] && self.entities.repairing[i])
            .map(|i| EntityId::new(i as u32, self.entities.generation[i]))
    }

    /// Every structure that is being sold with its progress, ascending by slot.
    pub fn sales_in_progress(&self) -> impl Iterator<Item = (EntityId, SaleProgress)> + '_ {
        (0..self.entities.len()).filter_map(move |i| {
            if !self.entities.alive[i] || self.entities.sell_left[i] == 0 {
                return None;
            }
            let id = EntityId::new(i as u32, self.entities.generation[i]);
            self.sale_of(id).map(|s| (id, s))
        })
    }

    /// Whether any structure repairs or is being sold: the gate of the checksum section.
    pub(crate) fn repair_active(&self) -> bool {
        (0..self.entities.len()).any(|i| {
            self.entities.alive[i] && (self.entities.repairing[i] || self.entities.sell_left[i] > 0)
        })
    }

    /// Writes section `r`: the rules, then each structure that repairs or is being sold, in slot
    /// order.
    pub(crate) fn write_repair_hash(&self, h: &mut Fnv1a64) {
        let Some(r) = self.repair.as_deref() else {
            return;
        };
        r.write_hash(h);
        let e = &self.entities;
        let active = |i: usize| e.alive[i] && (e.repairing[i] || e.sell_left[i] > 0);
        let n = (0..e.len()).filter(|&i| active(i)).count();
        h.write_u64(n as u64);
        for i in (0..e.len()).filter(|&i| active(i)) {
            h.write_u32(i as u32);
            h.write_u8(u8::from(e.repairing[i]));
            h.write_u32(e.sell_left[i]);
        }
    }

    /// The price of the structure in slot `i`; `None` when it has none.
    fn price_of(&self, i: usize) -> Option<u32> {
        self.buildings
            .get(self.entities.kind[i])
            .and_then(|b| b.cost)
    }

    /// The maximum hit points of the structure in slot `i` (zero: cannot be damaged).
    fn max_hp_of(&self, i: usize) -> u32 {
        self.stats_at(i).map_or(0, |s| s.hp)
    }

    /// Checks a repair or sell command against the structure: its live index.
    fn repair_target(&self, player: PlayerId, id: EntityId) -> Result<usize, RepairError> {
        let i = self
            .live_index(id)
            .filter(|&i| self.entities.cell[i] != NO_CELL && self.stats_at(i).is_some())
            .ok_or(RepairError::NoSuchStructure)?;
        if self.entities.owner[i] != player {
            return Err(RepairError::NotYours);
        }
        if self.entities.build_left[i] > 0 {
            return Err(RepairError::UnderConstruction);
        }
        if self.entities.sell_left[i] > 0 {
            return Err(RepairError::BeingSold);
        }
        if !self.is_standing(i) {
            return Err(RepairError::NotStanding);
        }
        Ok(i)
    }

    fn repair_command(
        &mut self,
        player: PlayerId,
        id: EntityId,
        on: bool,
    ) -> Result<(), RepairError> {
        let i = self.repair_target(player, id)?;
        if on {
            let max = self.max_hp_of(i);
            if max == 0 || self.entities.hp[i] >= max {
                return Err(RepairError::FullHitPoints);
            }
            if self.entities.repairing[i] {
                return Err(RepairError::AlreadyRepairing);
            }
            self.entities.repairing[i] = true;
        } else {
            if !self.entities.repairing[i] {
                return Err(RepairError::NotRepairing);
            }
            self.entities.repairing[i] = false;
        }
        Ok(())
    }

    fn sell_command(&mut self, player: PlayerId, id: EntityId) -> Result<(), RepairError> {
        let i = self.repair_target(player, id)?;
        let ticks = self.repair.as_ref().map_or(0, |r| r.rules.sell_ticks);
        self.entities.repairing[i] = false;
        self.entities.sell_left[i] = ticks;
        // It stands no more from now on: the match, if there is one, looks again at the end of the tick.
        if let Some(outcome) = self.outcome.as_deref_mut() {
            outcome.dirty = true;
        }
        Ok(())
    }

    /// Runs a repair or sell command of `player`. False when there are no repair rules, and when
    /// it is refused (then it is listed in [`World::repair_rejections`]).
    pub(crate) fn apply_repair(&mut self, player: PlayerId, command: Command) -> bool {
        if self.repair.is_none() {
            return false;
        }
        let (structure, result) = match command {
            Command::Repair { structure, on } => {
                (structure, self.repair_command(player, structure, on))
            }
            Command::Sell { structure } => (structure, self.sell_command(player, structure)),
            _ => return false,
        };
        match result {
            Ok(()) => true,
            Err(reason) => {
                let tick = self.tick;
                if let Some(r) = self.repair.as_deref_mut() {
                    r.rejections.push(RepairRejection {
                        tick,
                        player,
                        structure,
                        reason,
                    });
                }
                false
            }
        }
    }

    /// The repair and sale tick: see the module text. Runs after construction and before the
    /// commands of the tick.
    pub(crate) fn advance_repair_sell(&mut self) {
        let Some(rules) = self.repair.as_ref().map(|r| r.rules) else {
            return;
        };
        if let Some(r) = self.repair.as_deref_mut() {
            r.waiting.clear();
            r.sold.clear();
        }
        for i in 0..self.entities.len() {
            if !self.entities.alive[i] {
                continue;
            }
            if self.entities.sell_left[i] > 0 {
                self.entities.sell_left[i] -= 1;
                if self.entities.sell_left[i] == 0 {
                    self.finish_sale(i, rules);
                }
                continue;
            }
            if self.entities.repairing[i] {
                self.repair_step(i, rules);
            }
        }
    }

    /// One tick of repair of the structure in slot `i`.
    fn repair_step(&mut self, i: usize, rules: RepairRules) {
        let max = self.max_hp_of(i);
        let hp = self.entities.hp[i];
        // Done, or no longer a standing structure: the flag goes.
        if max == 0 || hp >= max || !self.is_standing(i) {
            self.entities.repairing[i] = false;
            return;
        }
        let owner = self.entities.owner[i];
        // Power: in a shortage the repair advances on `speed` ticks of every thousand.
        if self.power.is_some() && !advances_on(self.tick, self.power_speed_permille(owner)) {
            return;
        }
        let to = (hp + u32::from(rules.hp_per_tick)).min(max);
        let charge = match self.price_of(i) {
            Some(price) if self.money.is_some() => {
                repair_cost(price, rules.full_cost_permille, max, hp, to)
            }
            _ => 0,
        };
        if charge > 0 && !self.pay(owner, charge) {
            let id = EntityId::new(i as u32, self.entities.generation[i]);
            if let Some(r) = self.repair.as_deref_mut() {
                r.waiting.push(id);
            }
            return;
        }
        self.entities.hp[i] = to;
        if to >= max {
            self.entities.repairing[i] = false;
        }
    }

    /// The countdown of the structure in slot `i` reached zero: pay the owner and remove it.
    fn finish_sale(&mut self, i: usize, rules: RepairRules) {
        let id = EntityId::new(i as u32, self.entities.generation[i]);
        let (kind, owner) = (self.entities.kind[i], self.entities.owner[i]);
        let refund = self.price_of(i).map_or(0, |price| {
            sale_refund(
                price,
                rules.refund_permille,
                self.entities.hp[i],
                self.max_hp_of(i),
            )
        });
        self.despawn(id);
        // Into the store whatever its capacity, after the removal: a sold refinery has already
        // lowered the capacity (`economy.md`), and the store may hold more than it.
        let paid = if self.money.is_some() { refund } else { 0 };
        if paid > 0 {
            if let Some(h) = self.harvest.as_deref_mut() {
                h.refund(owner, paid);
            }
        }
        let tick = self.tick;
        if let Some(r) = self.repair.as_deref_mut() {
            r.sold.push(Sale {
                tick,
                id,
                kind,
                owner,
                refund: paid,
            });
        }
    }
}

#[cfg(test)]
mod tests;
