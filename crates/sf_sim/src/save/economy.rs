//! Sections `C` (money), `U` (production) and `I` (power) of the save (WP13.1c-2,
//! `docs/rules/save.md`). The ore (`O`) and the harvesters (`H`) are saved next to their private
//! state (`ore.rs`, `harvest/save.rs`).

use sf_math::bytes::{ByteReader, ByteWriter};
use sf_math::hash::Fnv1a64;

use super::{SaveError, SaveResult, ensure, get_id, get_pairs, put_id, put_pairs};
use crate::ids::{Kind, PlayerId};
use crate::money::{Money, MoneyRules};
use crate::power::{Power, PowerRules, PowerTotals};
use crate::production::{
    BuildSpeed, Factory, FactoryStatus, Production, ProductionRules, QueueItem,
};
use crate::terrain::Cell;

// ---- money --------------------------------------------------------------------------------------

/// The money rules for the rules digest. Destructured without `..`.
pub(crate) fn write_money_rules(m: &Money, h: &mut Fnv1a64) {
    let Money {
        rules,
        // State (section `C`).
        spent: _,
    } = m;
    let MoneyRules { credits_per_ore } = *rules;
    h.write_u32(credits_per_ore);
}

/// Section `C`: what every player has spent, ascending by player.
pub(crate) fn save_money(w: &mut ByteWriter, m: &Money) -> SaveResult<()> {
    let Money { rules: _, spent } = m;
    put_pairs(w, spent)?;
    Ok(())
}

pub(crate) fn restore_money(r: &mut ByteReader<'_>, m: &mut Money) -> SaveResult<()> {
    m.spent = get_pairs(r)?;
    Ok(())
}

// ---- power --------------------------------------------------------------------------------------

/// The power rules for the rules digest. Destructured without `..`.
pub(crate) fn write_power_rules(p: &Power, h: &mut Fnv1a64) {
    let Power {
        rules,
        // State (section `I`).
        totals: _,
    } = p;
    let PowerRules {
        shortage_floor_permille,
        defence_cutoff_permille,
    } = *rules;
    h.write_u32(shortage_floor_permille);
    h.write_u32(defence_cutoff_permille);
}

/// Section `I`: the totals of the last recount, ascending by player. Saved, not recounted: they
/// are what the start of the last step counted, and a plant that fell after that is no longer in a
/// recount, so the host's readout (and `PartialEq`) would differ for the tick after a load.
pub(crate) fn save_power(w: &mut ByteWriter, p: &Power) -> SaveResult<()> {
    let Power { rules: _, totals } = p;
    w.usize(totals.len())?;
    for &(player, t) in totals {
        let PowerTotals { made, used } = t;
        w.u8(player.0);
        w.u64(made);
        w.u64(used);
    }
    Ok(())
}

/// Refused: players out of order (`refresh_power` inserts them sorted, each once).
pub(crate) fn restore_power(r: &mut ByteReader<'_>, p: &mut Power) -> SaveResult<()> {
    let n = r.count(17)?;
    let mut totals = Vec::with_capacity(n);
    for _ in 0..n {
        let player = PlayerId(r.u8()?);
        let made = r.u64()?;
        let used = r.u64()?;
        totals.push((player, PowerTotals { made, used }));
    }
    ensure(
        totals.windows(2).all(|t| t[0].0 < t[1].0),
        "power totals out of order",
    )?;
    p.totals = totals;
    Ok(())
}

// ---- production ---------------------------------------------------------------------------------

/// The production rules and table for the rules digest. Destructured without `..`; the table enters
/// by its stored digest, which `ProductionTable::new` takes over every field of every entry.
pub(crate) fn write_production_rules(p: &Production, h: &mut Fnv1a64) {
    let Production {
        rules,
        table,
        // State (section `U`) and the reports of the last step.
        factories: _,
        rejections: _,
        produced: _,
    } = p;
    let ProductionRules {
        queue_limit,
        exit_radius,
        build_speed,
    } = *rules;
    let BuildSpeed { num, den } = build_speed;
    h.write_u8(queue_limit);
    h.write_u8(exit_radius);
    h.write_u32(num);
    h.write_u32(den);
    h.write_u64(table.rules_digest());
}

/// The tag of a factory status in the file.
fn status_tag(s: FactoryStatus) -> (u8, Option<Kind>) {
    match s {
        FactoryStatus::Idle => (0, None),
        FactoryStatus::Building => (1, None),
        FactoryStatus::WaitingForMoney => (2, None),
        FactoryStatus::WaitingForExit => (3, None),
        FactoryStatus::MissingPrerequisite(k) => (4, Some(k)),
        FactoryStatus::Stopped => (5, None),
        FactoryStatus::NotStanding => (6, None),
    }
}

/// Section `U`: every factory record, ascending by slot. The rejections and the units put out are
/// the reports of the last step (cleared by the next before anything reads them).
pub(crate) fn save_production(w: &mut ByteWriter, p: &Production) -> SaveResult<()> {
    let Production {
        rules: _,
        table: _,
        factories,
        // Reports: `World::step` clears the rejections before the commands, and
        // `advance_production` the units put out before it fills them.
        rejections: _,
        produced: _,
    } = p;
    w.usize(factories.len())?;
    for f in factories {
        let Factory {
            id,
            rally,
            repeat,
            items,
            done,
            // Saved although no step reads it before writing it: it is the readout of the factory
            // between ticks (`World::production_of`) and part of `PartialEq`.
            status,
        } = f;
        put_id(w, *id);
        w.bool(rally.is_some());
        if let Some(c) = rally {
            w.u32(c.x);
            w.u32(c.y);
        }
        w.bool(*repeat);
        w.usize(items.len())?;
        for item in items {
            let QueueItem { kind, free } = *item;
            w.u16(kind.0);
            w.bool(free);
        }
        w.u32(*done);
        let (tag, kind) = status_tag(*status);
        w.u8(tag);
        if let Some(k) = kind {
            w.u16(k.0);
        }
    }
    Ok(())
}

/// Reads section `U`. Each bound is what the production stage and the production commands leave
/// (`production.rs`: `queue_unit`, `cancel_unit`, `set_rally`, `set_repeat`, `advance_production`).
pub(crate) fn restore_production(
    r: &mut ByteReader<'_>,
    p: &mut Production,
    map: (u32, u32),
) -> SaveResult<()> {
    let n = r.count(19)?;
    let mut factories: Vec<Factory> = Vec::with_capacity(n);
    let limit = usize::from(p.rules.queue_limit);
    for _ in 0..n {
        let id = get_id(r)?;
        // Kept sorted by slot for a binary search, one record per slot (`get_or_insert`).
        ensure(
            factories.last().is_none_or(|f| f.id.index() < id.index()),
            "the factories are out of order",
        )?;
        let rally = if r.bool()? {
            let c = Cell::new(r.u32()?, r.u32()?);
            // `set_rally` refuses a cell off the map.
            ensure(c.x < map.0 && c.y < map.1, "a rally point off the map")?;
            Some(c)
        } else {
            None
        };
        let repeat = r.bool()?;
        let m = r.count(3)?;
        // `queue_unit` and the repeat refuse a full queue; the free unit of a finished structure
        // goes into the empty queue of a structure that could take no order before (`limit >= 1`).
        ensure(m <= limit, "a queue longer than its limit")?;
        let mut items = Vec::with_capacity(m);
        for k in 0..m {
            let kind = Kind(r.u16()?);
            let free = r.bool()?;
            // A paid item is a unit of the table (`queue_unit`); a free one the unit of a
            // `comes_with` pair, put into an empty queue, so it is always first.
            let known = if free {
                k == 0 && p.table.comes_with_all().iter().any(|&(_, u)| u == kind)
            } else {
                p.table.get(kind).is_some()
            };
            ensure(known, "a queue item the production stage never holds")?;
            items.push(QueueItem { kind, free });
        }
        let done = r.u32()?;
        // Progress belongs to a paid front item and stops at its build time; it is zero on an
        // empty queue and on a free unit (both reset it or never move it).
        let most = match items.first() {
            Some(front) if !front.free => p
                .table
                .get(front.kind)
                .map_or(0, |b| b.build_ticks.saturating_mul(1000)),
            _ => 0,
        };
        ensure(done <= most, "progress the production stage never leaves")?;
        let status = match r.u8()? {
            0 => FactoryStatus::Idle,
            1 => FactoryStatus::Building,
            2 => FactoryStatus::WaitingForMoney,
            3 => FactoryStatus::WaitingForExit,
            4 => FactoryStatus::MissingPrerequisite(Kind(r.u16()?)),
            5 => FactoryStatus::Stopped,
            6 => FactoryStatus::NotStanding,
            _ => return Err(SaveError::Corrupt("unknown factory status")),
        };
        factories.push(Factory {
            id,
            rally,
            repeat,
            items,
            done,
            status,
        });
    }
    p.factories = factories;
    Ok(())
}
