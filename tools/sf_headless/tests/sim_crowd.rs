//! The crowd of a scenario file is the crowd of the sf_sim golden test.
//!
//! `crates/sf_sim/tests/common/mod.rs` builds the golden scenario (seed
//! `0x57A1_F407`, 5000 wanderers, a script of commands). Its first commands are
//! the spawns of the crowd. The scenario file `wander_5000.ron` is meant to be
//! exactly that crowd without the rest of the script, so the headline
//! milestone scenario and the sf_sim golden describe the same world. This test
//! proves it by running both worlds side by side: the checksums must agree
//! before the first tick and after every tick that is run. (The final
//! checksums of the two scenarios differ on purpose: the golden one also
//! executes the command script, which `command_mix.ron` covers for the
//! headless runner.)
//!
//! The helper module is included from the sf_sim tests by path, so this test
//! follows that script if it ever changes.

#[path = "../../../crates/sf_sim/tests/common/mod.rs"]
mod sim_common;

mod common;

use common::scenario_path;
use sf_headless::scenario::Scenario;
use sf_sim::Command;

/// The first `n` commands of the sf_sim golden script, which must all be
/// spawns, as a world.
fn golden_crowd(s: &sim_common::Scenario, n: usize) -> sf_sim::World {
    let script = sim_common::script(s);
    assert!(
        script[..n]
            .iter()
            .all(|(_, _, c)| matches!(c, Command::Spawn { .. })),
        "the first {n} commands of the sf_sim script are no longer all spawns"
    );
    sim_common::world_with_commands(s, &script[..n])
}

fn assert_same_history(mut a: sf_sim::World, mut b: sf_sim::World, ticks: u64) {
    assert_eq!(a.checksum(), b.checksum(), "before the first tick");
    assert_eq!(a.pending_commands(), b.pending_commands());
    let ha = sim_common::run(&mut a, ticks);
    let hb = sim_common::run(&mut b, ticks);
    assert_eq!(ha, hb);
    assert_eq!(a.len(), b.len());
}

#[test]
fn wander_5000_is_the_crowd_of_the_full_golden_scenario() {
    let scenario = Scenario::load(&scenario_path("wander_5000.ron")).unwrap();
    let golden = sim_common::Scenario::full();
    assert_eq!(scenario.seed, golden.seed);
    assert_eq!(scenario.ticks, golden.ticks);
    assert_eq!(scenario.spawns[0].count as usize, golden.entities);
    // A few ticks are enough to show the worlds are the same; a debug build
    // steps 5000 entities slowly.
    assert_same_history(
        scenario.build_world().unwrap(),
        golden_crowd(&golden, golden.entities),
        10,
    );
}

#[test]
fn a_smaller_group_is_the_crowd_of_the_small_golden_scenario() {
    let golden = sim_common::Scenario::small();
    let scenario = Scenario::from_ron_str(&format!(
        "Scenario(seed: {}, ticks: {}, spawns: [(count: {}, kinds: [0, 1, 2], \
         owners: [0, 1, 2, 3], area: (min: (0, 0), max: (256, 256)))])",
        golden.seed, golden.ticks, golden.entities
    ))
    .unwrap();
    assert_same_history(
        scenario.build_world().unwrap(),
        golden_crowd(&golden, golden.entities),
        200,
    );
}
