//! Measures how many ticks per second the simulation runs with a crowd of
//! wanderers. Not a test and not part of the simulation: it uses the system
//! clock, which `sf_sim` itself never does.
//!
//! ```text
//! cargo run --release -p sf_sim --example ticks_per_second [entities] [ticks]
//! ```
//!
//! Defaults: 5000 wanderers, 10000 ticks (the headline scenario of the work
//! package). `sf_headless` (WP1.3) will replace this with a real runner.

use std::time::Instant;

use sf_math::{Fx, Rng, Vec2};
use sf_sim::{Command, Kind, PlayerId, Tick, World, WorldConfig};

fn arg(n: usize, default: u64) -> u64 {
    std::env::args()
        .nth(n)
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let entities = arg(1, 5_000);
    let ticks = arg(2, 10_000);

    let mut script = Rng::new(1);
    let mut world = World::new(WorldConfig::new(0x57A1_F407)).expect("valid config");
    for i in 0..entities {
        let pos = Vec2::new(
            script.fx_range(Fx::ZERO, Fx::from_int(256)),
            script.fx_range(Fx::ZERO, Fx::from_int(256)),
        );
        world
            .schedule(
                Tick(0),
                PlayerId(0),
                Command::Spawn {
                    kind: Kind((i % 3) as u16),
                    pos,
                    owner: PlayerId((i % 4) as u8),
                },
            )
            .expect("tick 0 has not run");
    }

    let start = Instant::now();
    for _ in 0..ticks {
        world.step();
    }
    // Integer arithmetic only: the float lints of this crate apply here too.
    let micros = start.elapsed().as_micros().max(1);

    println!("entities:         {entities}");
    println!("ticks:            {ticks}");
    println!("elapsed:          {} ms", micros / 1_000);
    println!(
        "ticks per second: {}",
        u128::from(ticks) * 1_000_000 / micros
    );
    println!(
        "entity updates:   {} per second",
        u128::from(entities) * u128::from(ticks) * 1_000_000 / micros
    );
    println!("per tick:         {} us", micros / u128::from(ticks.max(1)));
    println!("final checksum:   {:#018x}", world.checksum());
}
