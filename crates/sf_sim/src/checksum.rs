//! The state checksum.
//!
//! [`World::checksum`] folds the whole simulation state into one `u64` with
//! [`Fnv1a64`]: explicit little-endian integers, no
//! `std::hash`, nothing that depends on the platform, the pointer width or the
//! build profile. It is computed after every tick and kept in the world's
//! history, so a desync, a regression or a platform difference shows up at the
//! first tick where two runs disagree.
//!
//! # What is covered, in this order
//!
//! | Section | Contents |
//! |---|---|
//! | header | a format tag, the tick counter |
//! | config | seed, map size, speed, turn rate |
//! | spawn counter | how many entities were ever spawned (seeds the next one's stream) |
//! | slots, ascending index | the generation of every slot, a live flag, and for a live slot: kind, owner, position, facing, behaviour, target, wander area, and the full state of the entity's random stream |
//! | free list | its length and the indices in stack order, since that order decides which slot a later spawn gets |
//! | pending commands | their count and each one in execution order: tick, player, and every field of the command |
//! | terrain (`T`), only with a terrain layer | the terrain's digest: size, type definitions and the type of every cell, hashed once when the world is made |
//! | occupancy (`O`), only while a cell is occupied | the number of occupied cells, then for each in ascending cell order: the cell index, the occupant's slot and its generation |
//! | units (`U`), only with a unit table | the table's digest (every kind with its hit points, movement class, speed, turn rate, size and footprint), then the number of entities made from it and for each, in slot order: the slot and its current hit points |
//! | orders (`D`), only while some entity has an order or a queue | the table's weapon reach digest, then the number of such entities and for each, in slot order: the slot, its current order, the guard's walk switch and its queued orders (WP3.4) |
//!
//! Every variable-length part is preceded by its length and every section by a
//! tag byte, so two different states cannot produce the same byte stream.
//!
//! # Terrain and occupancy leave the old checksums alone
//!
//! The last two sections are written **only when they have content**, and after
//! all the others. A world made with `World::new` has no terrain layer, and
//! wanderers never occupy a cell, so for every world that existed before WP2.1
//! the hashed bytes are exactly what they were: the golden values of
//! `tests/golden.rs` and `wander_5000.expected` / `command_mix.expected` did not
//! change, and `FORMAT_TAG` stays. A world with a layer or a structure gets
//! the extra sections and so a checksum of its own, which is what the
//! `terrain_256` scenario pins. The terrain is hashed through a digest computed
//! once (it never changes), so a big map costs eight bytes per tick; occupancy
//! costs a scan of the grid only while at least one cell is occupied.
//!
//! The unit table section (WP3.2) follows the same rule: it is written only for
//! a world made with a non-empty `UnitTable`, after the others. The legacy
//! `Spawn` and `Place` commands do not consult the table, so every scenario
//! that does not use `SpawnUnit` or `PlaceStructure` and has no table hashes
//! exactly as before. A footprint of several cells is already in the occupancy
//! section (one entry per occupied cell). Which table entry an entity came from
//! is not hashed on its own: it follows from the kind and the table, and the
//! slots in the units section tell a table entity from a legacy one of the
//! same kind number.
//!
//! The orders section (WP3.4) follows the same rule: it is written only while at
//! least one live entity has an order other than idle or a queued order, so a
//! world that never received an order hashes as before. The weapon reach of the
//! unit table is hashed there and not in the table's digest, which the pinned
//! `units_spawn` scenario fixes. The new `Behavior::Ordered` has the tag three.
//!
//! # What is deliberately left out
//!
//! * `prev_pos` and `prev_facing`. They exist for render interpolation only
//!   and are derived (at the end of a tick they equal the state before that
//!   tick's step), so they cannot carry a divergence that `pos` and `facing`
//!   do not already show a tick later. A world rebuilt from a save does not
//!   need to restore them.
//! * The checksum history itself, the count of live entities (derived from the
//!   live flags) and the queue's insertion counter (only the resulting order
//!   matters, and that is covered).
//!
//! # Changing the format
//!
//! Any change here, or to what the simulation computes, changes every
//! checksum, including the golden values in `tests/golden.rs`. That is a
//! deliberate, reviewed act: say why in the PR. Change `FORMAT_TAG` when the
//! layout of the hashed bytes changes.

use sf_math::hash::Fnv1a64;
use sf_math::{Rng, Vec2};

use crate::behavior::Area;
use crate::command::Command;
use crate::entities::NO_DEF;
use crate::orders::Order;
use crate::world::World;

/// Written first. Bump the number when the layout of the hashed bytes changes.
const FORMAT_TAG: &[u8] = b"sf_sim/state/v1";

const SECTION_CONFIG: u8 = b'C';
const SECTION_SLOTS: u8 = b'S';
const SECTION_FREE: u8 = b'F';
const SECTION_QUEUE: u8 = b'Q';
const SECTION_TERRAIN: u8 = b'T';
const SECTION_OCCUPANCY: u8 = b'O';
const SECTION_UNITS: u8 = b'U';
const SECTION_ORDERS: u8 = b'D';

fn len64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

fn write_area(h: &mut Fnv1a64, a: Area) {
    h.write_vec2(a.min);
    h.write_vec2(a.max);
}

fn write_option_vec2(h: &mut Fnv1a64, v: Option<Vec2>) {
    match v {
        Some(v) => {
            h.write_u8(1);
            h.write_vec2(v);
        }
        None => h.write_u8(0),
    }
}

fn write_rng(h: &mut Fnv1a64, r: &Rng) {
    let s = r.state();
    h.write_u64(s.state);
    h.write_u64(s.inc);
}

fn write_command(h: &mut Fnv1a64, c: &Command) {
    match c {
        Command::Spawn { kind, pos, owner } => {
            h.write_u8(1);
            h.write_bytes(&kind.0.to_le_bytes());
            h.write_vec2(*pos);
            h.write_u8(owner.0);
        }
        Command::Place { kind, cell, owner } => {
            h.write_u8(5);
            h.write_bytes(&kind.0.to_le_bytes());
            h.write_u32(cell.x);
            h.write_u32(cell.y);
            h.write_u8(owner.0);
        }
        Command::SpawnUnit { kind, pos, owner } => {
            h.write_u8(6);
            h.write_bytes(&kind.0.to_le_bytes());
            h.write_vec2(*pos);
            h.write_u8(owner.0);
        }
        Command::PlaceStructure { kind, cell, owner } => {
            h.write_u8(7);
            h.write_bytes(&kind.0.to_le_bytes());
            h.write_u32(cell.x);
            h.write_u32(cell.y);
            h.write_u8(owner.0);
        }
        Command::Order { ids, order, queued } => {
            h.write_u8(8);
            h.write_u64(len64(ids.len()));
            for id in ids {
                h.write_u32(id.index());
                h.write_u32(id.generation());
            }
            order.write_hash(h);
            h.write_u8(u8::from(*queued));
        }
        Command::Stop { ids } => {
            h.write_u8(9);
            h.write_u64(len64(ids.len()));
            for id in ids {
                h.write_u32(id.index());
                h.write_u32(id.generation());
            }
        }
        Command::Despawn { id } => {
            h.write_u8(2);
            h.write_u32(id.index());
            h.write_u32(id.generation());
        }
        Command::SetWanderArea { ids, area } => {
            h.write_u8(3);
            h.write_u64(len64(ids.len()));
            for id in ids {
                h.write_u32(id.index());
                h.write_u32(id.generation());
            }
            write_area(h, *area);
        }
        Command::MoveTo { ids, target } => {
            h.write_u8(4);
            h.write_u64(len64(ids.len()));
            for id in ids {
                h.write_u32(id.index());
                h.write_u32(id.generation());
            }
            h.write_vec2(*target);
        }
    }
}

impl World {
    /// The checksum of the complete simulation state right now (see the
    /// [module documentation](crate::checksum) for exactly what is covered).
    ///
    /// Cost is linear in the number of slots. [`World::step`] computes it
    /// once per tick and stores it; calling this yourself between ticks gives
    /// the same value as the last history entry.
    pub fn checksum(&self) -> u64 {
        let mut h = Fnv1a64::new();
        h.write_bytes(FORMAT_TAG);
        h.write_u64(self.tick.0);

        h.write_u8(SECTION_CONFIG);
        h.write_u64(self.config.seed);
        h.write_u32(self.config.width);
        h.write_u32(self.config.height);
        h.write_fx(self.config.speed);
        h.write_u8(self.config.turn_per_tick);
        h.write_u64(self.spawn_serial);

        let e = &self.entities;
        let slots = e.len();
        h.write_u8(SECTION_SLOTS);
        h.write_u64(len64(slots));
        for i in 0..slots {
            h.write_u32(e.generation[i]);
            if !e.alive[i] {
                h.write_u8(0);
                continue;
            }
            h.write_u8(1);
            h.write_bytes(&e.kind[i].0.to_le_bytes());
            h.write_u8(e.owner[i].0);
            h.write_vec2(e.pos[i]);
            h.write_facing(e.facing[i]);
            h.write_u8(e.behavior[i].tag());
            write_option_vec2(&mut h, e.target[i]);
            write_area(&mut h, e.area[i]);
            write_rng(&mut h, &e.rng[i]);
        }

        h.write_u8(SECTION_FREE);
        h.write_u64(len64(self.free.len()));
        for &index in &self.free {
            h.write_u32(index);
        }

        h.write_u8(SECTION_QUEUE);
        h.write_u64(len64(self.queue.len()));
        for (tick, player, command) in self.queue.iter() {
            h.write_u64(tick.0);
            h.write_u8(player.0);
            write_command(&mut h, command);
        }

        // Terrain and occupancy (WP2.1). Each section is written only when
        // there is something in it, after everything above, so the byte stream
        // of a world without a terrain layer and without a structure is the one
        // it always was.
        if self.terrain.has_layer() {
            h.write_u8(SECTION_TERRAIN);
            h.write_u64(self.terrain.digest());
        }
        if self.occupancy.count() > 0 {
            h.write_u8(SECTION_OCCUPANCY);
            h.write_u64(len64(self.occupancy.count()));
            for (cell, slot) in self.occupancy.occupied() {
                h.write_u32(cell);
                h.write_u32(slot);
                h.write_u32(e.generation[slot as usize]);
            }
        }

        // The unit table (WP3.2): its digest, then the hit points of every
        // entity made from it. Written only for a world that has a table, so
        // the worlds above are untouched.
        if !self.units.is_empty() {
            h.write_u8(SECTION_UNITS);
            h.write_u64(self.units.digest());
            let made = (0..slots).filter(|&i| e.alive[i] && e.def[i] != NO_DEF);
            h.write_u64(len64(made.clone().count()));
            for i in made {
                h.write_u32(i as u32);
                h.write_u32(e.hp[i]);
            }
        }

        // Orders (WP3.4): the weapon reach of the unit table (what an attack
        // order walks to), then every entity that has an order or a queue.
        // Written only while at least one does, so every world that never
        // received an order hashes as before.
        let ordered =
            |i: usize| e.alive[i] && (e.order[i] != Order::Idle || !e.queue[i].is_empty());
        if (0..slots).any(ordered) {
            h.write_u8(SECTION_ORDERS);
            h.write_u64(self.units.reach_digest());
            h.write_u64(len64((0..slots).filter(|&i| ordered(i)).count()));
            for i in (0..slots).filter(|&i| ordered(i)) {
                h.write_u32(i as u32);
                e.order[i].write_hash(&mut h);
                h.write_u8(u8::from(e.guard_walking[i]));
                h.write_u8(e.queue[i].len() as u8);
                for queued in e.queue[i].iter() {
                    queued.write_hash(&mut h);
                }
            }
        }

        h.finish()
    }
}

#[cfg(test)]
mod tests;
