//! Saving and restoring the harvest state (WP13.1c-2, `docs/rules/save.md`, section `H`).
//!
//! Saved: every harvester record (`units`, by slot), every dock in the list of the last sync, the
//! ore delivered per player and, with storage rules, the store and the capacity of every player.
//! Derived: the claims on ore cells (the inverse of the harvesters' claimed cells) and `live` (the
//! bound slots, ascending). Rules: the numbers, the harvester and dock kinds, the storage table and
//! the price, in the rules digest ([`Harvest::write_rules`]).

use sf_math::bytes::{ByteReader, ByteWriter};

use super::*;
use crate::save::{
    SaveResult, ensure, get_id, get_ids, get_opt_id, get_order, get_pairs, get_vec2, order_point,
    put_id, put_ids, put_opt_id, put_order, put_pairs, put_vec2,
};

/// What a harvester's phase is saved as: [`Phase::tag`], the checksum's numbers.
fn phase_of(tag: u8) -> Option<Phase> {
    Some(match tag {
        0 => Phase::Search,
        1 => Phase::Park,
        2 => Phase::ToOre,
        3 => Phase::Mine,
        4 => Phase::NoDock,
        5 => Phase::ToDock(Call::Driving),
        6 => Phase::ToDock(Call::Called),
        7 => Phase::ToDock(Call::Queued),
        8 => Phase::ToDock(Call::Granted),
        9 => Phase::Unload,
        10 => Phase::Manual,
        _ => return None,
    })
}

fn put_reply(w: &mut ByteWriter, r: Reply) {
    match r {
        Reply::Granted => w.u8(0),
        Reply::Queued(p) => {
            w.u8(1);
            w.u8(p);
        }
        Reply::Revoked => w.u8(2),
        Reply::Unloaded(n) => {
            w.u8(3);
            w.u16(n);
        }
        Reply::Released => w.u8(4),
    }
}

fn msg_tag(m: Msg) -> u8 {
    match m {
        Msg::Request => 0,
        Msg::Arrived => 1,
        Msg::Cancel => 2,
    }
}

fn side_of(tag: u8) -> Option<Side> {
    Some(match tag {
        0 => Side::PosX,
        1 => Side::NegX,
        2 => Side::PosY,
        3 => Side::NegY,
        _ => return None,
    })
}

impl Harvest {
    /// The rules for the rules digest: every harvest number, the harvester kinds and, with storage,
    /// the capacity table and the price of a unit of ore. Destructured without `..`: a new rule
    /// fails to compile until it is hashed.
    pub(crate) fn write_rules(&self, h: &mut Fnv1a64) {
        let Harvest {
            rules,
            kinds,
            // Derived from the kinds and the unit and building tables (both in the digest).
            harvester_def: _,
            dock_def: _,
            // State (section `H`).
            units: _,
            docks: _,
            claims: _,
            delivered: _,
            live: _,
            storage,
        } = self;
        let HarvestRules {
            capacity,
            mine_amount,
            mine_period,
            unload_amount,
            unload_period,
            search_radius,
            retry_period,
            call_range,
            grant_timeout,
            call_timeout,
            max_tries,
            park_distance,
            park_spread,
            park_near_tenths,
            mine_start_tenths,
            mine_reach_tenths,
            mine_lose_tenths,
            dock_reach_tenths,
            travel_cost,
            queue_cost,
            switch_margin,
            recheck_period,
            queue_gap,
            queue_start,
            queue_step,
            queue_rows,
        } = *rules;
        for v in [
            capacity,
            mine_amount,
            mine_period,
            unload_amount,
            unload_period,
            search_radius,
            retry_period,
            call_range,
            grant_timeout,
            call_timeout,
            u16::from(max_tries),
            park_distance,
            park_spread,
            park_near_tenths,
            mine_start_tenths,
            mine_reach_tenths,
            mine_lose_tenths,
            dock_reach_tenths,
            travel_cost,
            queue_cost,
            switch_margin,
            recheck_period,
            queue_gap,
            queue_start,
            queue_step,
            queue_rows,
        ] {
            h.write_u32(u32::from(v));
        }
        h.write_u64(kinds.len() as u64);
        for k in kinds {
            h.write_u32(u32::from(k.0));
        }
        match storage {
            Some(s) => {
                let Storage {
                    table,
                    // Derived from the table and the unit table.
                    per_def: _,
                    // State.
                    stored: _,
                    price,
                    capacity: _,
                } = s;
                h.write_u8(1);
                h.write_u64(table.len() as u64);
                for &(k, n) in table {
                    h.write_u32(u32::from(k.0));
                    h.write_u32(n);
                }
                h.write_u32(*price);
            }
            None => h.write_u8(0),
        }
    }

    /// Section `H`: see the module text. Destructured without `..`.
    pub(crate) fn save(&self, w: &mut ByteWriter) -> SaveResult<()> {
        let Harvest {
            // Rules: `write_rules`.
            rules: _,
            kinds: _,
            harvester_def: _,
            dock_def: _,
            units,
            docks,
            // Derived: the inverse of the claimed cells of the bound harvesters.
            claims: _,
            delivered,
            // Derived: the bound slots in ascending order (what the sync of the last tick listed).
            live: _,
            storage,
        } = self;
        w.usize(units.len())?;
        for hv in units {
            let Hv { core, mail } = hv;
            let Core {
                bound,
                generation,
                load,
                phase,
                wait,
                cell,
                focus,
                bad,
                dock,
                own,
                tries,
                spot,
            } = *core;
            w.bool(bound);
            if !bound {
                // An unbound slot holds `Core::NONE` and no mail (`sync` writes both).
                continue;
            }
            w.u32(generation);
            w.u16(load);
            w.u8(phase.tag());
            w.u16(wait);
            w.u32(cell);
            w.u32(focus);
            w.u32(bad);
            put_opt_id(w, dock);
            put_order(w, own);
            w.u8(tries);
            w.u8(spot);
            w.usize(mail.len())?;
            for &r in mail {
                put_reply(w, r);
            }
        }
        w.usize(docks.len())?;
        for d in docks {
            let Dock {
                id,
                owner,
                point,
                exit,
                side,
                bay,
                queue,
                inbox,
            } = d;
            put_id(w, *id);
            w.u8(owner.0);
            put_vec2(w, *point);
            w.i64(exit.0);
            w.i64(exit.1);
            w.u8(side.number());
            match *bay {
                Bay::Free => w.u8(0),
                Bay::Reserved { who, left } => {
                    w.u8(1);
                    put_id(w, who);
                    w.u16(left);
                }
                Bay::Unloading { who, wait } => {
                    w.u8(2);
                    put_id(w, who);
                    w.u16(wait);
                }
            }
            put_ids(w, queue)?;
            w.usize(inbox.len())?;
            for m in inbox {
                let Mail { from, msg } = *m;
                put_id(w, from);
                w.u8(msg_tag(msg));
            }
        }
        put_pairs(w, delivered)?;
        if let Some(s) = storage {
            let Storage {
                // Rules.
                table: _,
                per_def: _,
                price: _,
                stored,
                capacity,
            } = s;
            put_pairs(w, stored)?;
            put_pairs(w, capacity)?;
        }
        Ok(())
    }

    /// Reads section `H` into this fresh harvest state. `w` is the world being restored, with its
    /// entities, ore and tables in place (the harvest state is taken out of it). Every bound is
    /// what the harvest stage leaves at the end of a tick (`docs/rules/save.md`).
    pub(crate) fn restore(&mut self, r: &mut ByteReader<'_>, w: &World) -> SaveResult<()> {
        let rules = self.rules;
        let (width, height) = (w.config.width, w.config.height);
        let (max_x, max_y) = (
            Fx::from_i64(i64::from(width)),
            Fx::from_i64(i64::from(height)),
        );
        let inside = |p: Vec2| p.x >= Fx::ZERO && p.y >= Fx::ZERO && p.x <= max_x && p.y <= max_y;
        let field = |c: u32| c == NO_CELL || w.ore.as_ref().is_some_and(|o| o.is_field_index(c));
        // Every value `wait` is set to (`decide`, `search`, `park`, `go_return`, `dock_walk_failed`,
        // `apply`); otherwise it only counts down.
        let max_wait = rules
            .retry_period
            .max(rules.recheck_period)
            .max(rules.call_timeout)
            .max(rules.mine_period - 1);

        let n = r.count(1)?;
        // `sync` grows the records to the slot count of its tick; slots never go away.
        ensure(
            n <= w.entities.len(),
            "more harvester records than entity slots",
        )?;
        let mut units = Vec::with_capacity(n);
        let mut claims = BTreeMap::new();
        let mut live = Vec::new();
        for slot in 0..n {
            if !r.bool()? {
                units.push(Hv {
                    core: Core::NONE,
                    mail: Vec::new(),
                });
                continue;
            }
            let generation = r.u32()?;
            let load = r.u16()?;
            let phase = phase_of(r.u8()?)
                .ok_or(crate::save::SaveError::Corrupt("unknown harvester phase"))?;
            let core = Core {
                bound: true,
                generation,
                load,
                phase,
                wait: r.u16()?,
                cell: r.u32()?,
                focus: r.u32()?,
                bad: r.u32()?,
                dock: get_opt_id(r)?,
                own: get_order(r)?,
                tries: r.u8()?,
                spot: r.u8()?,
            };
            // A bite takes at most the room left (`decide`, Mine); an unload only lowers the load.
            ensure(
                core.load <= rules.capacity,
                "a harvester carries more than it holds",
            )?;
            ensure(
                core.wait <= max_wait,
                "a harvester timer the harvest stage never sets",
            )?;
            // The claimed cell and the search focus are cells with ore when chosen (`search`, a
            // player's order onto a field), the skipped cell a claimed one (`decide`, ToOre).
            ensure(
                field(core.cell) && field(core.focus) && field(core.bad),
                "a harvester cell that is no ore field",
            )?;
            // Its own walks go to a cell centre, a dock or queue point (all on the map) or take
            // over the unit's current order (whose point `restore_entities` checked).
            ensure(
                order_point(core.own).is_none_or(inside),
                "a harvester walk off the map",
            )?;
            // `tries` reaches `max_tries` at most: at it the walk is given up or the counter reset
            // (`decide`, `queue_walk`, `dock_walk_failed`).
            ensure(
                core.tries <= rules.max_tries,
                "a harvester tried more often than it may",
            )?;
            // `queue_walk`: `1 + 2 * row + side`, `row < queue_rows`.
            ensure(
                u16::from(core.spot) <= 2 * rules.queue_rows,
                "a queue spot past the last row",
            )?;
            // `apply` clears the mail of every bound harvester, and the docks only answer bound
            // ones: between ticks it is empty.
            ensure(
                r.usize()? == 0,
                "a harvester's mail the harvest stage never leaves",
            )?;
            if core.cell != NO_CELL {
                ensure(
                    claims.insert(core.cell, slot as u32).is_none(),
                    "two harvesters claim one ore cell",
                )?;
            }
            live.push(slot as u32);
            units.push(Hv {
                core,
                mail: Vec::new(),
            });
        }

        let n = r.count(36)?;
        let mut docks: Vec<Dock> = Vec::with_capacity(n);
        for _ in 0..n {
            let id = get_id(r)?;
            let owner = PlayerId(r.u8()?);
            let point = get_vec2(r)?;
            let exit = (r.i64()?, r.i64()?);
            let side = side_of(r.u8()?).ok_or(crate::save::SaveError::Corrupt("unknown side"))?;
            // `new_dock`: the exit cell clamped onto the map, and its centre.
            ensure(
                (0..i64::from(width)).contains(&exit.0)
                    && (0..i64::from(height)).contains(&exit.1)
                    && point
                        == Vec2::new(
                            Fx::from_i64(exit.0) + Fx::HALF,
                            Fx::from_i64(exit.1) + Fx::HALF,
                        ),
                "a dock point off the map or off its cell",
            )?;
            ensure(
                docks.last().is_none_or(|d| d.id.index() < id.index()),
                "the docks are out of order",
            )?;
            let bay = match r.u8()? {
                0 => Bay::Free,
                1 => Bay::Reserved {
                    who: get_id(r)?,
                    left: r.u16()?,
                },
                2 => Bay::Unloading {
                    who: get_id(r)?,
                    wait: r.u16()?,
                },
                _ => return Err(crate::save::SaveError::Corrupt("unknown dock bay")),
            };
            // A grant sets `grant_timeout` and the next tick counts it down; an unload sets
            // `unload_period` and the same `run_dock` counts the first tick off.
            ensure(
                match bay {
                    Bay::Free => true,
                    Bay::Reserved { left, .. } => left <= rules.grant_timeout,
                    Bay::Unloading { wait, .. } => wait < rules.unload_period,
                },
                "a dock bay the harvest stage never leaves",
            )?;
            let queue = get_ids(r)?;
            // A harvester joins the queue only when it is neither in it nor the holder.
            let holder = match bay {
                Bay::Free => None,
                Bay::Reserved { who, .. } | Bay::Unloading { who, .. } => Some(who),
            };
            let distinct = queue
                .iter()
                .enumerate()
                .all(|(k, q)| Some(*q) != holder && !queue[..k].contains(q));
            ensure(distinct, "a dock queue the harvest stage never leaves")?;
            let m = r.count(6)?;
            let mut inbox = Vec::with_capacity(m);
            for _ in 0..m {
                let from = get_id(r)?;
                let msg = match r.u8()? {
                    0 => Msg::Request,
                    1 => Msg::Arrived,
                    2 => Msg::Cancel,
                    _ => return Err(crate::save::SaveError::Corrupt("unknown dock message")),
                };
                inbox.push(Mail { from, msg });
            }
            // The inbox holds what `apply` sent this tick: at most one message per harvester, in
            // slot order (`run_dock` empties it every tick).
            ensure(
                inbox
                    .windows(2)
                    .all(|p| p[0].from.index() < p[1].from.index()),
                "a dock inbox the harvest stage never leaves",
            )?;
            let dock = Dock {
                id,
                owner,
                point,
                exit,
                side,
                bay,
                queue,
                inbox,
            };
            // A dock whose refinery still stands in its slot is the one `new_dock` made for it
            // (owner and anchor never change). One that died after the harvest stage is dropped
            // by the next sync.
            if let Some(i) = w.live_index(id) {
                let made = self
                    .dock_def
                    .get(w.entities.def[i] as usize)
                    .copied()
                    .unwrap_or(false)
                    .then(|| self.new_dock(w, i, id))
                    .flatten();
                ensure(
                    made.is_some_and(|m| {
                        (m.owner, m.point, m.exit, m.side) == (owner, point, exit, side)
                    }),
                    "a dock that is not its refinery's",
                )?;
            }
            docks.push(dock);
        }
        self.delivered = get_pairs(r)?;
        if let Some(s) = &mut self.storage {
            s.stored = get_pairs(r)?;
            s.capacity = get_pairs(r)?;
        }
        self.units = units;
        self.docks = docks;
        self.claims = claims;
        self.live = live;
        Ok(())
    }
}

/// What the harvest state holds, for [`crate::save::SaveContents`].
#[derive(Copy, Clone, Default)]
pub(crate) struct HarvestCounts {
    pub(crate) harvesters: u32,
    pub(crate) harvesting: u32,
    pub(crate) returning: u32,
    pub(crate) unloading: u32,
    pub(crate) waiting: u32,
    pub(crate) carrying: u32,
    pub(crate) queued: u32,
    pub(crate) inbox: u32,
    pub(crate) reserved: u32,
    pub(crate) delivered: bool,
    pub(crate) stored: bool,
}

impl Harvest {
    pub(crate) fn counts(&self) -> HarvestCounts {
        let mut c = HarvestCounts::default();
        for hv in self.units.iter().filter(|hv| hv.core.bound) {
            let core = &hv.core;
            c.harvesters += 1;
            match core.phase {
                Phase::ToOre | Phase::Mine => c.harvesting += 1,
                Phase::ToDock(_) => c.returning += 1,
                Phase::Unload => c.unloading += 1,
                Phase::Park | Phase::NoDock => c.waiting += 1,
                Phase::Search | Phase::Manual => {}
            }
            c.carrying += u32::from(core.load > 0);
        }
        for d in &self.docks {
            c.queued += d.queue.len() as u32;
            c.inbox += d.inbox.len() as u32;
            c.reserved += u32::from(matches!(d.bay, Bay::Reserved { .. }));
        }
        c.delivered = self.delivered.iter().any(|&(_, n)| n > 0);
        c.stored = self
            .storage
            .as_ref()
            .is_some_and(|s| s.stored.iter().any(|&(_, n)| n > 0));
        c
    }

    /// The saved fields of section `H` that hold something other than their default, by name (the
    /// coverage record of WP13.1c-2, `World::save_coverage`).
    pub(crate) fn coverage(&self, out: &mut Vec<&'static str>) {
        let mut add = |name: &'static str, on: bool| {
            if on && !out.contains(&name) {
                out.push(name);
            }
        };
        for hv in self.units.iter() {
            let c = &hv.core;
            add("harvester.unbound", !c.bound);
            if !c.bound {
                continue;
            }
            add("harvester.bound", true);
            add("harvester.generation", c.generation != 0);
            add("harvester.load", c.load != 0);
            add(
                match c.phase {
                    Phase::Search => "harvester.phase.search",
                    Phase::Park => "harvester.phase.park",
                    Phase::ToOre => "harvester.phase.to_ore",
                    Phase::Mine => "harvester.phase.mine",
                    Phase::NoDock => "harvester.phase.no_dock",
                    Phase::ToDock(Call::Driving) => "harvester.phase.driving",
                    Phase::ToDock(Call::Called) => "harvester.phase.called",
                    Phase::ToDock(Call::Queued) => "harvester.phase.queued",
                    Phase::ToDock(Call::Granted) => "harvester.phase.granted",
                    Phase::Unload => "harvester.phase.unload",
                    Phase::Manual => "harvester.phase.manual",
                },
                true,
            );
            add("harvester.wait", c.wait != 0);
            add("harvester.cell", c.cell != NO_CELL);
            add("harvester.focus", c.focus != NO_CELL);
            add("harvester.bad", c.bad != NO_CELL);
            add("harvester.dock", c.dock.is_some());
            add("harvester.own", c.own != Order::Idle);
            add("harvester.tries", c.tries != 0);
            add("harvester.spot", c.spot != 0);
            add("harvester.mail", !hv.mail.is_empty());
        }
        for d in &self.docks {
            add("dock", true);
            add("dock.owner", d.owner != PlayerId(0));
            add("dock.side", d.side != Side::PosX);
            add(
                match d.bay {
                    Bay::Free => "dock.bay.free",
                    Bay::Reserved { .. } => "dock.bay.reserved",
                    Bay::Unloading { .. } => "dock.bay.unloading",
                },
                true,
            );
            add("dock.queue", !d.queue.is_empty());
            for m in &d.inbox {
                add(
                    match m.msg {
                        Msg::Request => "dock.inbox.request",
                        Msg::Arrived => "dock.inbox.arrived",
                        Msg::Cancel => "dock.inbox.cancel",
                    },
                    true,
                );
            }
        }
        add("delivered", self.delivered.iter().any(|&(_, n)| n > 0));
        if let Some(s) = &self.storage {
            add("stored", s.stored.iter().any(|&(_, n)| n > 0));
            add("capacity", s.capacity.iter().any(|&(_, n)| n > 0));
        }
    }
}
