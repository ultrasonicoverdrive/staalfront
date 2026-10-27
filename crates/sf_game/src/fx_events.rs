//! The effects event queue (WP5.6a): what the picture needs to know about shots, hits and deaths, with none
//! of it lost.
//!
//! # Why a queue
//!
//! The world keeps a report of the last tick only (`World::shots`, `impacts`, `deaths` are replaced by every
//! step), and the sim thread publishes one snapshot per pass of up to `1 + max_catch_up` steps, to a slot
//! the main thread reads whenever it likes. So the shots and deaths of the earlier steps of a catch-up pass,
//! and of every snapshot the main thread did not read, reached no snapshot (follow-up 31 of
//! `docs/progress.md`; `SimSnapshot::deaths` is that lossy list and stays for the F1 panel). Here the sim
//! thread calls [`record`] right after **every** `World::step` and appends what it finds to an [`FxQueue`]
//! behind a mutex; the main thread takes the whole queue once per frame. Nothing is read from a snapshot.
//!
//! # What is in an event
//!
//! Everything the picture needs, resolved while the entities still exist: positions are metres (the
//! presentation's unit, converted once here), the aim is the facing of the weapon's mount, sizes are metres.
//! A [`FxEvent`] is 32 bytes. The effects themselves (which particles) are the main thread's business
//! (`crate::fx`, `assets/config/effects.ron`).
//!
//! # Bounds
//!
//! The queue holds at most [`FX_QUEUE_CAP`] events. When it is full the **oldest** is dropped and counted
//! ([`FxQueue::dropped`], `SimCounters::fx_dropped`, shown in the F1 panel). A tick makes at most
//! 5,000 x `MAX_WEAPON_SLOTS` shots, but a real fight makes tens to hundreds (docs/rules/effects.md has
//! the measured number), so the cap is a guard and not a limit anyone meets.
//!
//! An event whose attacker or target is gone *and* was not destroyed in the same tick has no position and is
//! counted in [`FxQueue::unresolved`] instead (a victim that died in the tick is found in the tick's deaths).
//!
//! `sf_sim` is untouched: this only reads the world.

use std::collections::VecDeque;

use sf_math::Facing;
use sf_math::render_bridge::{cells_to_metres, vec2_cells_to_metres};
use sf_sim::{EntityId, ImpactCause, Mount, ProjectileKind, UnitClass, World};

/// The most events the queue holds.
pub const FX_QUEUE_CAP: usize = 4096;

/// What happened.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FxKind {
    /// A weapon fired. `slot` is the attacker's weapon slot (a burst's shots go on counting from the
    /// attacker's side).
    Fire { slot: u8 },
    /// A hit: a projectile that went off, or an instant weapon that landed. `hit` is a direct hit on an
    /// entity, `splash` that its splash hurt something.
    Impact {
        cause: ImpactCause,
        hit: bool,
        splash: bool,
    },
    /// A unit or structure was destroyed. `wreck` is true when it left a wreck or rubble.
    Death { wreck: bool, structure: bool },
}

/// One thing that happened in one tick, for the picture.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FxEvent {
    /// The tick it happened in (the number `World::shots` and the others carry).
    pub tick: u64,
    /// What.
    pub kind: FxKind,
    /// The kind number of the weapon (zero for a death).
    pub weapon: u16,
    /// The player it concerns: the shooter, the owner of the shooter of a projectile if known, the owner of
    /// what died.
    pub player: u8,
    /// The ground position in metres, `[x, z]`: the shooter's middle for a `Fire`, where it went off for an
    /// `Impact`, where it stood for a `Death`.
    pub pos: [f32; 2],
    /// A `Fire`: the facing in steps (of 256) of the weapon's mount, hull or turret, that it was aimed along.
    /// A `Death`: the way the body faced. Zero for an `Impact`.
    pub facing: u8,
    /// A `Fire`: the length of the shooter's body, metres. An `Impact`: the splash radius, metres. A
    /// `Death`: the size of the body (or the longer side of the footprint), metres.
    pub size: f32,
}

/// The queue the sim thread fills and the main thread empties. See the module documentation.
#[derive(Debug, Default)]
pub struct FxQueue {
    events: VecDeque<FxEvent>,
    dropped: u64,
    unresolved: u64,
    recorded: u64,
}

impl FxQueue {
    /// An empty queue with room for [`FX_QUEUE_CAP`] events (the one allocation).
    pub fn new() -> FxQueue {
        FxQueue {
            events: VecDeque::with_capacity(FX_QUEUE_CAP),
            ..FxQueue::default()
        }
    }

    /// Appends `event`; when the queue is full the oldest event goes and is counted.
    pub fn push(&mut self, event: FxEvent) {
        if self.events.len() >= FX_QUEUE_CAP {
            self.events.pop_front();
            self.dropped += 1;
        }
        self.events.push_back(event);
        self.recorded += 1;
    }

    /// Counts events that had no position.
    pub fn add_unresolved(&mut self, n: u64) {
        self.unresolved += n;
    }

    /// Moves every event to the end of `out`, in the order they were recorded, and empties the queue.
    pub fn drain_into(&mut self, out: &mut Vec<FxEvent>) {
        out.extend(self.events.drain(..));
    }

    /// Events in the queue now.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Events dropped because the queue was full.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Events that could not be given a position.
    pub fn unresolved(&self) -> u64 {
        self.unresolved
    }

    /// Events recorded since the queue was made (dropped ones included).
    pub fn recorded(&self) -> u64 {
        self.recorded
    }
}

/// Where something was, for an event: its middle, the way it faced, its turret if it has one, its body
/// length in metres, its player.
struct Origin {
    pos: [f32; 2],
    hull: Facing,
    turret: Option<Facing>,
    length_m: f32,
    player: u8,
}

/// The longer side of a body or footprint in metres, of an entity that is alive.
fn length_of(world: &World, id: EntityId) -> f32 {
    world.stats_of(id).map_or(0.0, |s| {
        let cells = match s.footprint {
            Some(f) => f32::from(f.width.max(f.height)),
            None => cells_to_metres(s.size) / sf_math::render_bridge::METRES_PER_CELL,
        };
        cells * sf_math::render_bridge::METRES_PER_CELL
    })
}

/// Finds `id` in the world, or in the deaths of the tick it was destroyed in.
fn origin_of(world: &World, id: EntityId) -> Option<Origin> {
    if let Some(v) = world.get(id) {
        return Some(Origin {
            pos: vec2_cells_to_metres(v.pos),
            hull: v.facing,
            turret: v.turret.map(|t| t.facing),
            length_m: length_of(world, id),
            player: v.owner.0,
        });
    }
    let d = world.deaths().iter().find(|d| d.victim == id)?;
    Some(Origin {
        pos: vec2_cells_to_metres(d.pos),
        hull: d.facing,
        turret: None,
        length_m: death_size_m(d),
        player: d.owner.0,
    })
}

fn death_size_m(d: &sf_sim::Death) -> f32 {
    match d.footprint {
        Some(f) => f32::from(f.width.max(f.height)) * sf_math::render_bridge::METRES_PER_CELL,
        None => cells_to_metres(d.size),
    }
}

/// Appends the events of the tick the world has just run to `out`, in the order shots, impacts, deaths
/// (each in the world's own order). Returns how many events had no position (see the module documentation).
///
/// * A shot is a `Fire` at the shooter, along the facing of the weapon's mount. An **instant** weapon also
///   makes an `Impact` at the target (it has no projectile and so no impact of its own in the world).
/// * An impact of the world's own is an `Impact` where the projectile went off.
/// * A death is a `Death` where the body stood.
pub fn record(world: &World, out: &mut Vec<FxEvent>) -> u64 {
    let table = world.weapon_table();
    let mut unresolved = 0;
    for shot in world.shots() {
        let Some(weapon) = table.weapon(shot.weapon) else {
            continue;
        };
        let Some(from) = origin_of(world, shot.attacker) else {
            unresolved += 1;
            continue;
        };
        let aim = match weapon.mount {
            Mount::Turret => from.turret.unwrap_or(from.hull),
            Mount::Hull => from.hull,
        };
        out.push(FxEvent {
            tick: shot.tick.0,
            kind: FxKind::Fire { slot: shot.slot },
            weapon: shot.weapon,
            player: from.player,
            pos: from.pos,
            facing: aim.raw(),
            size: from.length_m,
        });
        if weapon.projectile == ProjectileKind::Instant {
            match origin_of(world, shot.target) {
                Some(at) => out.push(FxEvent {
                    tick: shot.tick.0,
                    kind: FxKind::Impact {
                        cause: ImpactCause::Proximity,
                        hit: true,
                        splash: false,
                    },
                    weapon: shot.weapon,
                    player: from.player,
                    pos: at.pos,
                    facing: 0,
                    size: 0.0,
                }),
                None => unresolved += 1,
            }
        }
    }
    for impact in world.impacts() {
        let splash_m = table
            .weapon(impact.weapon)
            .map_or(0.0, |w| cells_to_metres(w.splash_radius));
        out.push(FxEvent {
            tick: impact.tick.0,
            kind: FxKind::Impact {
                cause: impact.cause,
                hit: impact.victim.is_some(),
                splash: impact.splash_victims > 0,
            },
            weapon: impact.weapon,
            player: origin_of(world, impact.owner).map_or(u8::MAX, |o| o.player),
            pos: vec2_cells_to_metres(impact.at),
            facing: 0,
            size: splash_m,
        });
    }
    for d in world.deaths() {
        out.push(FxEvent {
            tick: d.tick.0,
            kind: FxKind::Death {
                wreck: d.wreck.is_some(),
                structure: d.class == UnitClass::Structure,
            },
            weapon: 0,
            player: d.owner.0,
            pos: vec2_cells_to_metres(d.pos),
            facing: d.facing.raw(),
            size: death_size_m(d),
        });
    }
    unresolved
}

/// A `Fire` of weapon kind zero at the origin in tick zero, every number zero. Tests build their events from
/// it with `..FxEvent::default()`, so a new field is added here once and not in every test.
impl Default for FxEvent {
    fn default() -> Self {
        FxEvent {
            tick: 0,
            kind: FxKind::Fire { slot: 0 },
            weapon: 0,
            player: 0,
            pos: [0.0; 2],
            facing: 0,
            size: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(tick: u64) -> FxEvent {
        FxEvent {
            tick,
            kind: FxKind::Fire { slot: 0 },
            weapon: 1,
            player: 0,
            pos: [0.0, 0.0],
            facing: 0,
            size: 0.0,
        }
    }

    #[test]
    fn an_event_is_32_bytes_or_less() {
        assert!(
            std::mem::size_of::<FxEvent>() <= 32,
            "{}",
            std::mem::size_of::<FxEvent>()
        );
    }

    /// 4,097 events into a queue of 4,096: the oldest goes, one is counted, and the rest keep their order.
    #[test]
    fn a_full_queue_drops_the_oldest_and_counts_it() {
        let mut q = FxQueue::new();
        assert_eq!(FX_QUEUE_CAP, 4096);
        for i in 0..4096 {
            q.push(event(i));
        }
        assert_eq!((q.len(), q.dropped()), (4096, 0));
        q.push(event(4096));
        assert_eq!((q.len(), q.dropped()), (4096, 1));
        let mut out = Vec::new();
        q.drain_into(&mut out);
        assert_eq!(out.len(), 4096);
        assert_eq!(out[0].tick, 1, "tick 0 was the oldest and went");
        assert_eq!(out[4095].tick, 4096);
        assert!(out.windows(2).all(|w| w[0].tick < w[1].tick), "in order");
        assert!(q.is_empty());
        assert_eq!(q.recorded(), 4097);
    }

    #[test]
    fn draining_twice_gives_each_event_once() {
        let mut q = FxQueue::new();
        q.push(event(1));
        q.push(event(2));
        let mut a = Vec::new();
        q.drain_into(&mut a);
        q.push(event(3));
        let mut b = Vec::new();
        q.drain_into(&mut b);
        assert_eq!(a.iter().map(|e| e.tick).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(b.iter().map(|e| e.tick).collect::<Vec<_>>(), [3]);
    }
}
