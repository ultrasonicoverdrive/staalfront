//! Shared scenario builder for the integration tests.
//!
//! A scenario is a seed, a number of wanderers and a length in ticks, plus a
//! fixed *script* of commands generated from the seed with a separate random
//! stream (never the simulation's own). The script exercises every command and
//! the slot reuse machinery: a mass spawn, a wander area for a group, a
//! `MoveTo` for another, despawns, spawns that reuse the freed slots, commands
//! with stale ids, and a second wander area.
//!
//! The same script drives `determinism.rs` and `golden.rs`, so the committed
//! golden checksums describe exactly what the determinism tests run.
#![allow(dead_code)]

use sf_math::{Fx, Rng, Vec2};
use sf_sim::{Area, Command, EntityId, Kind, PlayerId, Tick, World, WorldConfig};

/// One scheduled command.
pub type Scheduled = (Tick, PlayerId, Command);

#[derive(Copy, Clone, Debug)]
pub struct Scenario {
    pub seed: u64,
    pub entities: usize,
    pub ticks: u64,
}

impl Scenario {
    /// A scenario small enough for a debug build.
    pub const fn small() -> Scenario {
        Scenario {
            seed: 0x57A1_F407,
            entities: 300,
            ticks: 1_500,
        }
    }

    /// The headline scenario of the work package: 5000 wanderers for 10000
    /// ticks.
    pub const fn full() -> Scenario {
        Scenario {
            seed: 0x57A1_F407,
            entities: 5_000,
            ticks: 10_000,
        }
    }

    /// The size to run in the current build profile: full in release builds,
    /// small in debug builds (where the full run would take minutes).
    pub const fn for_profile() -> Scenario {
        if cfg!(debug_assertions) {
            Scenario::small()
        } else {
            Scenario::full()
        }
    }
}

fn map_point(script: &mut Rng) -> Vec2 {
    Vec2::new(
        script.fx_range(Fx::ZERO, Fx::from_int(256)),
        script.fx_range(Fx::ZERO, Fx::from_int(256)),
    )
}

/// The commands of a scenario, in the order a player would issue them.
///
/// The initial spawns all come from player 0, so entity `i` gets the id
/// `{ i, 0 }` (spawns run in insertion order within a player); later ids are
/// named by index and generation exactly as a replay file would.
pub fn script(s: &Scenario) -> Vec<Scheduled> {
    let mut script = Rng::with_stream(s.seed, 0x5C21);
    let mut cmds: Vec<Scheduled> = Vec::new();
    let n = s.entities as u32;

    for i in 0..s.entities {
        cmds.push((
            Tick(0),
            PlayerId(0),
            Command::Spawn {
                kind: Kind((i % 3) as u16),
                pos: map_point(&mut script),
                owner: PlayerId((i % 4) as u8),
            },
        ));
    }

    let group = |from: u32, to: u32| -> Vec<EntityId> {
        (from..to.min(n)).map(|i| EntityId::new(i, 0)).collect()
    };
    let at = |num: u64, den: u64| Tick(s.ticks * num / den);

    // A tenth of the crowd is sent to wander in a corner.
    cmds.push((
        at(1, 20),
        PlayerId(1),
        Command::SetWanderArea {
            ids: group(0, n / 10),
            area: Area::from_cells(0, 0, 24, 24),
        },
    ));
    // Another tenth marches to the middle of the map.
    cmds.push((
        at(1, 10),
        PlayerId(2),
        Command::MoveTo {
            ids: group(n / 10, n / 5),
            target: Vec2::from_ints(128, 128),
        },
    ));
    // Every 17th entity is despawned ...
    let doomed: Vec<u32> = (0..n).filter(|i| i % 17 == 5).collect();
    for &i in &doomed {
        cmds.push((
            at(1, 5),
            PlayerId(0),
            Command::Despawn {
                id: EntityId::new(i, 0),
            },
        ));
    }
    // ... and twenty new ones take the freed slots, one tick later.
    for k in 0..20u16 {
        cmds.push((
            Tick(at(1, 5).0 + 1),
            PlayerId(3),
            Command::Spawn {
                kind: Kind(k % 3),
                pos: map_point(&mut script),
                owner: PlayerId((k % 4) as u8),
            },
        ));
    }
    // A command that names the dead and the living: the stale ids are skipped.
    cmds.push((
        at(1, 4),
        PlayerId(1),
        Command::MoveTo {
            ids: doomed
                .iter()
                .take(10)
                .map(|&i| EntityId::new(i, 0))
                .chain(group(n / 5, n / 5 + 10))
                .collect(),
            target: Vec2::from_ints(200, 40),
        },
    ));
    // A second wander area for the middle group, and a despawn of a stale id.
    cmds.push((
        at(1, 2),
        PlayerId(2),
        Command::SetWanderArea {
            ids: group(n / 10, n / 5),
            area: Area::from_cells(100, 100, 160, 140),
        },
    ));
    cmds.push((
        at(1, 2),
        PlayerId(0),
        Command::Despawn {
            id: EntityId::new(doomed.first().copied().unwrap_or(0), 0),
        },
    ));
    cmds
}

/// A world with the scenario's script scheduled, in script order.
pub fn world_with(s: &Scenario) -> World {
    world_with_commands(s, &script(s))
}

/// A world with the given commands scheduled, in the given order.
pub fn world_with_commands(s: &Scenario, cmds: &[Scheduled]) -> World {
    let mut w = World::new(WorldConfig::new(s.seed)).expect("valid config");
    for (tick, player, cmd) in cmds {
        w.schedule(*tick, *player, cmd.clone())
            .expect("future tick");
    }
    w
}

/// Runs `ticks` steps and returns the checksum of each.
pub fn run(world: &mut World, ticks: u64) -> Vec<u64> {
    (0..ticks).map(|_| world.step().checksum).collect()
}
