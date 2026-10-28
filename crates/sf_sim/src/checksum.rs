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
//! | movement (`M`), only with a terrain layer, a movement table **and** a live entity made from the unit table that has a movement class | the table's digest: every movement class with its cost on every terrain type (WP4.1) |
//! | paths (`P`), only while the world has pathfinding and some live entity has a route | the path budget and the epoch of the graphs, the job in flight, and for each entity with a route, in slot order: its state, flags, cells, ring of waypoints and portal path (WP4.2) |
//! | flowfields (`L`), only while the pool holds a field | the pool and the build in flight (WP4.3; since WP4.4 each field also has its group order, its ring, the spacing and the inner radius of its slots) |
//! | crowd (`K`), only while some live unit remembers being blocked, sidestepping, shoved or crowded | the collision settings, then each such unit in slot order: how long it has not come closer to its waypoint, its sidestep attempts and the best distance seen, a sidestep in progress, how far it has been shoved and for how long left alone, whether it has asked for a new route (WP4.4) |
//! | turning (`R`), only while some live unit has a turret or an unfinished arc turn | the turret rates of the unit table and the movement table's digest, then each such unit in slot order: the thousandths of a step it has yet to turn and, for a turret, its facing and its target (WP4.5) |
//! | weapons (`W`), only with a weapon table | the weapon table's digest, then each live entity with a weapon that is reloading or in a burst, in slot order: for each weapon slot the ticks it waits to fire and the shots left in its burst (WP5.1) |
//! | projectiles (`J`), only with a weapon table | the projectile store: its slot count, free list (stack order) and each projectile in flight in slot order, with its slot, generation, weapon, owner, target, aim point, place, height, heading, span and fuse (WP5.2) |
//! | targeting (`A`), only with a weapon table **and** targeting settings | the settings, then each live unit whose memory of the targets it chose is not at rest (WP5.4) |
//! | destruction (`X`), only with destruction settings | the settings, the wrecks, the tally of each player and the kills of each live unit (WP5.5) |
//! | buildings (`B`), only with a building table | the table's digest: every structure kind with its footprint, function, exit side, build time, price, power, factions and prerequisites (WP6.1), since WP6.2 the build radii and the faction of each player |
//! | construction (`N`), only while some structure is under construction | the number of such structures, then for each in slot order: the slot and the ticks still to go (WP6.3); before `E` |
//! | ore (`E`), only in a world given ore rules | the rules (start, maximum, regrowth amount and period), the number of field cells and a running sum of one mixed term per field cell and its amount (WP7.1). The sum is kept up to date in constant time per change, so the section costs 24 bytes of hashing per tick whatever the map size |
//! | harvest (`H`), only in a world given harvest rules | the rules and the harvester kinds, the ore delivered per player, then every harvester in slot order (load, phase, timer, claimed cell, search focus, skipped cell, refinery, the order it expects, tries, unread messages) and every dock (bay, queue, inbox) (WP7.2). It is the last section |
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
//! The movement section (WP4.1) follows the same rule and is written last. The
//! costs only matter on a map that has terrain (without a layer every cell is
//! open ground whatever the table says) and for an entity that has a movement
//! class, which is any mobile entity made from the unit table, whatever drives
//! it (an order, the legacy `MoveTo`, a wander). So it is written only on a map
//! with a layer, with a movement table, while some live entity made from the unit
//! table has a movement class. That keeps `terrain_256` (terrain, no table),
//! `orders` (a table, no terrain) and the wanderer scenarios on the values they
//! were pinned with; `units_spawn` (terrain, a table, units with classes) gets
//! the section, and its legacy walkers now obey the costs, which is why it was
//! re-pinned once at WP4.1. The three passable flags of the terrain types are
//! gone; their bits in the terrain digest are the derived
//! `TerrainFlags::ground`, so that digest, and `terrain_256`, did not change.
//!
//! The paths section (WP4.2) follows the same rule and is written after the movement
//! section: only while some live unit has a route. A route exists only for a unit that
//! walks an order and is bound by terrain on a map with terrain and a movement table, so
//! every scenario without such a unit (the wanderers, `terrain_256`, `units_spawn` with its
//! legacy walkers, `orders` on open ground) hashes as before. The graphs themselves are
//! not hashed: they follow from the terrain and the occupancy, which are, and the epoch
//! says how often they changed.
//!
//! The flowfield section (WP4.3, `L`) comes last and is written only while the pool holds a
//! field, which only a group order of at least the threshold makes: the threshold, the clock
//! that ages the fields, every field (identity, state, region, arrival slots, how many cells
//! are integrated and a running hash of them in the order they were) and the build in flight.
//! A unit that follows a field also has its group and rank in its route. The arrays of costs
//! and directions are not hashed whole (they would be hashed every tick); the running hash
//! covers each cell as it is integrated.
//!
//! The crowd section (WP4.4, `K`) comes last. A unit that never met a crowd (it was
//! never blocked for a tick, never shoved) is at rest and not in it, so a world
//! where nobody collides hashes as before, and a world with collision turned off
//! never has the section: the pinned wanderer scenarios, `units_spawn`,
//! `terrain_costs` and `pathing` (collision off in the file) did not move. The
//! scratch of a tick (which units walked, which were refused) is not state and is
//! not hashed.
//!
//! The turning section (WP4.5, `R`) comes last and is written only while some live
//! unit has a turret or an unfinished arc turn: the turret rates of the unit table,
//! the movement table's digest (which holds the turning profiles of the classes),
//! and for each such unit, in slot order, the fraction of a step it has yet to
//! turn, and for a turret its facing and what it aims at. `prev_turret` is derived
//! like `prev_facing` and is not hashed. The hull turning itself needs no section:
//! the facing is in the slots, and a class that turns as the walk always did (a
//! table without turning profiles) leaves every checksum alone.
//!
//! The formation section (WP4.6, `G`) comes last and is written only while a formation
//! exists: its number counter, and for each formation its goal, leader, heading, state and
//! patience, and each member with its offset in the lattice and the ticks it has been away
//! from its slot. What a tick works out from those (the slots, what each member does) is
//! not hashed. A world whose move orders never formed one hashes as before.
//!
//! The weapons section (WP5.1, `W`) comes last and is written for as long as the world has a weapon
//! table: the digest of the table (so peers with different tables differ from the first tick), then
//! each live entity with a weapon that is not at rest in slot order with, for each of its weapon
//! slots, the ticks it still waits to fire and the shots left in a running burst. A world that was
//! given no weapon table hashes as before: the pinned wanderer scenarios, `units_spawn`, `orders` and
//! the rest did not move. The shots themselves are not state (the hit points they took are,
//! in the units section).
//!
//! The projectile section (WP5.2, `J`) follows it under the same condition, the world having a weapon
//! table: the store of projectiles in flight (slot count, free list, every live projectile), so a shell
//! in the air is part of the state and two peers whose shells differ disagree at once. It is written even
//! when nothing flies (the slot count and free list are state too: they decide the id of the next
//! launch). A world without a weapon table cannot have a projectile and has no section, so no checksum of
//! an earlier phase moved. The impacts and shots listed for a tick are reports and are not hashed.
//!
//! The targeting section (WP5.4, `A`) follows them under one more condition, that the world was given
//! targeting settings (`World::with_targeting`): the settings, then each live unit whose memory of
//! the targets it chose is not at rest (its cooldown to the next scan, whether it follows its target,
//! the order it goes back to, where it began and where it last saw the target, the reach it walks
//! to). The `Pursue` order a unit gives itself is in the orders section. A world without those
//! settings, which is every pinned scenario of the earlier phases, hashes as before.
//!
//! The destruction section (WP5.5, `X`) comes after the targeting section and before the
//! buildings section (`B`), and is written only in a world that was given
//! destruction settings (`World::with_destruction`): the settings, every wreck and heap of rubble
//! (kept sorted by where it lies), the tally of each player (kills, assists, losses) and the kills
//! credited to each live unit. The deaths of the last tick are a report and are not hashed. A world
//! without those settings hashes as before.
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
use crate::crowd;
use crate::entities::NO_DEF;
use crate::orders::Order;
use crate::targeting;
use crate::weapons;
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
const SECTION_MOVEMENT: u8 = b'M';
const SECTION_PATHS: u8 = b'P';
const SECTION_FIELDS: u8 = b'L';
const SECTION_CROWD: u8 = b'K';
const SECTION_TURNING: u8 = b'R';
const SECTION_FORMATIONS: u8 = b'G';
const SECTION_WEAPONS: u8 = b'W';
const SECTION_PROJECTILES: u8 = b'J';
const SECTION_TARGETING: u8 = b'A';
const SECTION_DESTRUCTION: u8 = b'X';
const SECTION_BUILDINGS: u8 = b'B';
const SECTION_CONSTRUCTION: u8 = b'N';
const SECTION_ORE: u8 = b'E';
const SECTION_HARVEST: u8 = b'H';

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

fn write_option_id(h: &mut Fnv1a64, id: Option<crate::ids::EntityId>) {
    match id {
        Some(id) => {
            h.write_u8(1);
            h.write_u32(id.index());
            h.write_u32(id.generation());
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
        Command::BuildStructure { kind, cell } => {
            h.write_u8(11);
            h.write_bytes(&kind.0.to_le_bytes());
            h.write_u32(cell.x);
            h.write_u32(cell.y);
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
        Command::AimTurret { ids, target } => {
            h.write_u8(10);
            h.write_u64(len64(ids.len()));
            for id in ids {
                h.write_u32(id.index());
                h.write_u32(id.generation());
            }
            write_option_id(h, *target);
        }
        Command::TakeOre { cell, amount } => {
            h.write_u8(12);
            h.write_u32(cell.x);
            h.write_u32(cell.y);
            h.write_bytes(&amount.to_le_bytes());
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

        // Movement costs (WP4.1): only on a map with terrain, while some unit
        // of the unit table has a movement class for them to apply to.
        let classed = |i: usize| {
            e.alive[i]
                && e.def[i] != NO_DEF
                && self.units.at(e.def[i] as usize).movement_class.is_some()
        };
        if self.terrain.has_layer() && !self.movement.is_empty() && (0..slots).any(classed) {
            h.write_u8(SECTION_MOVEMENT);
            h.write_u64(self.movement.digest());
        }

        // Routes (WP4.2): the budget, the epoch of the graphs, the job being
        // served and every route that exists. Written only while the world has
        // pathfinding and some unit has a route, so a world that never routed
        // anything hashes as before.
        if self.pathing.any_route(e) {
            h.write_u8(SECTION_PATHS);
            self.pathing.write_hash(&mut h, e);
        }

        // Flowfields (WP4.3): only while the pool holds one.
        if self.pathing.any_fields() {
            h.write_u8(SECTION_FIELDS);
            self.pathing.write_field_hash(&mut h);
        }

        // The crowd (WP4.4): only while some live unit remembers being blocked,
        // sidestepping or pushed, so a world that never met a crowd hashes as before.
        if crowd::any_state(e) {
            h.write_u8(SECTION_CROWD);
            crowd::write_hash(&mut h, &self.crowd.config, e);
        }

        // Turning (WP4.5): the turret of every unit that has one and the unfinished
        // turn of every unit that turns in an arc. Only while one exists, so a
        // world without turrets and arcs hashes as before.
        let turning_state = |i: usize| {
            e.alive[i]
                && (e.turn_carry[i] != 0
                    || (e.def[i] != NO_DEF && self.units.at(e.def[i] as usize).has_turret()))
        };
        if (0..slots).any(turning_state) {
            h.write_u8(SECTION_TURNING);
            h.write_u64(self.units.turret_digest());
            h.write_u64(self.movement.digest());
            h.write_u64(len64((0..slots).filter(|&i| turning_state(i)).count()));
            for i in (0..slots).filter(|&i| turning_state(i)) {
                h.write_u32(i as u32);
                h.write_bytes(&e.turn_carry[i].to_le_bytes());
                let has_turret =
                    e.def[i] != NO_DEF && self.units.at(e.def[i] as usize).has_turret();
                h.write_u8(u8::from(has_turret));
                if has_turret {
                    h.write_facing(e.turret[i]);
                    write_option_id(&mut h, e.turret_target[i]);
                }
            }
        }

        // Formations (WP4.6): the groups that keep their shape and where each member
        // stands in the lattice. Only while one exists, so a world whose orders never
        // formed one hashes as before.
        if self.formations.any() {
            h.write_u8(SECTION_FORMATIONS);
            self.formations.write_hash(&mut h, e);
        }

        // Weapons (WP5.1): the table, and every weapon that is reloading or in a burst. The
        // section is there for as long as the world has a weapon table, so two worlds with
        // different tables differ from the first tick; a world without a table hashes as
        // before.
        if !self.arsenal.is_empty() {
            let busy = |i: usize| e.alive[i] && !weapons::at_rest(&e.weapon[i]);
            h.write_u8(SECTION_WEAPONS);
            h.write_u64(self.arsenal.table.digest());
            h.write_u64(len64((0..slots).filter(|&i| busy(i)).count()));
            for i in (0..slots).filter(|&i| busy(i)) {
                h.write_u32(i as u32);
                for slot in &e.weapon[i] {
                    h.write_u32(slot.wait);
                    h.write_u8(slot.burst_left);
                }
            }
            // Projectiles (WP5.2): the store and everything in flight, after the weapons. Written
            // for as long as the world has a weapon table, like the section above, so a world
            // without one hashes as before.
            h.write_u8(SECTION_PROJECTILES);
            self.arsenal.projectiles.write_hash(&mut h);
            // Targeting (WP5.4): the settings and what each unit remembers about the targets it chose.
            // Only in a world that was given settings, after the projectiles, so a world with weapons
            // that never chooses targets hashes as before.
            if let Some(config) = &self.targeting.config {
                h.write_u8(SECTION_TARGETING);
                targeting::write_hash(&mut h, config, e);
            }
        }
        // Destruction (WP5.5): the settings, the wrecks, the scores and the kills of each unit. Only in
        // a world that was given settings, so every world of the earlier phases hashes as before.
        if self.destruction.config.is_some() {
            h.write_u8(SECTION_DESTRUCTION);
            self.destruction.write_hash(&mut h, e);
        }

        // Buildings (WP6.1): the rules of the structure kinds. Only in a world that was
        // given a table, so every world without one hashes as before.
        if !self.buildings.is_empty() {
            h.write_u8(SECTION_BUILDINGS);
            h.write_u64(self.buildings.digest());
            // Placement (WP6.2): the build radii and who is which faction. After the digest of
            // WP6.1, which is unchanged.
            h.write_u64(self.buildings.placement_digest());
            h.write_u64(len64(self.placement.players.len()));
            for &(player, bit) in &self.placement.players {
                h.write_u8(player.0);
                h.write_u8(bit);
            }
        }

        // Construction (WP6.3): the ticks still to go of every structure under construction, in
        // slot order. Only while one exists, so a world that never built anything with
        // `BuildStructure` (and one whose builds are all complete) hashes as before. The total
        // is the table's `build_ticks`, hashed in section `B`.
        let building = |i: usize| e.alive[i] && e.build_left[i] != 0;
        if (0..slots).any(building) {
            h.write_u8(SECTION_CONSTRUCTION);
            h.write_u64(len64((0..slots).filter(|&i| building(i)).count()));
            for i in (0..slots).filter(|&i| building(i)) {
                h.write_u32(i as u32);
                h.write_u32(e.build_left[i]);
            }
        }

        // Construction (`N`, WP6.3), then ore (`E`, WP7.1: the rules, the number of field cells
        // and a running sum over the amounts), then harvest (`H`, WP7.2) last. Each is
        // gated: `N` only while something is under construction, `E` only in a world that was given
        // ore rules, `H` only with harvest rules. So every world without them hashes as before.
        if let Some(ore) = &self.ore {
            h.write_u8(SECTION_ORE);
            for word in ore.fingerprint() {
                h.write_u64(word);
            }
        }

        // Harvesters (WP7.2): the rules, the ore delivered, every harvester's state and every
        // dock's bay and queue. After the ore section, and only in a world that was given harvest
        // rules, so every world without them hashes as before.
        if let Some(harvest) = &self.harvest {
            h.write_u8(SECTION_HARVEST);
            harvest.write_hash(&mut h);
        }

        h.finish()
    }
}

#[cfg(test)]
mod tests;
