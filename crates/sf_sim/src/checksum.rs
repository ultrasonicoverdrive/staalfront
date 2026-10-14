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
//!
//! Every variable-length part is preceded by its length and every section by a
//! tag byte, so two different states cannot produce the same byte stream.
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
use crate::world::World;

/// Written first. Bump the number when the layout of the hashed bytes changes.
const FORMAT_TAG: &[u8] = b"sf_sim/state/v1";

const SECTION_CONFIG: u8 = b'C';
const SECTION_SLOTS: u8 = b'S';
const SECTION_FREE: u8 = b'F';
const SECTION_QUEUE: u8 = b'Q';

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

        h.finish()
    }
}

#[cfg(test)]
mod tests;
