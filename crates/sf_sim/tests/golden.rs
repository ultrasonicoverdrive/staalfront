//! Cross-platform bit-identity proof for `sf_sim` (WP1.2 acceptance
//! criterion, PLAN.md section 6).
//!
//! # How this proves anything
//!
//! Each test builds a fixed scenario (a seed plus a script of commands, see
//! `common/mod.rs`), runs it, and compares the checksum history at chosen
//! ticks with values committed in this file. The values were produced once on
//! Linux. CI runs `cargo test` on Linux and on Windows, in debug and (for the
//! full-size scenario, which is too slow for debug) in release mode, so **the
//! same committed values passing on both operating systems is the proof that
//! the simulation is bit-identical everywhere**. A single wrong bit in any tick
//! (a float sneaking in, a different iteration order, an arithmetic
//! difference) changes every later checksum and fails the test on the platform
//! that disagrees.
//!
//! * `tiny_scenario_history`: every tick of a three-entity, twenty-tick run.
//!   When something breaks, start here: the first differing tick says where.
//! * `small_scenario_checkpoints`: 300 wanderers for 1500 ticks with the whole
//!   command script, in both profiles.
//! * `full_scenario_checkpoints`: the headline scenario of the work package,
//!   5000 wanderers for 10000 ticks. Release only (`cargo test --release -p
//!   sf_sim`), which CI runs on both operating systems.
//! * `small_scenario_interpolation_state`: the columns the checksum leaves out
//!   (previous position and facing), hashed from the snapshot.
//!
//! # Changing a golden value
//!
//! A golden value may only change when the *specified behaviour* of the
//! simulation changes on purpose (a new movement rule, a different checksum
//! layout, a new RNG use). That invalidates every saved game and replay, so it
//! is a deliberate, reviewed act, never a fix to make a test pass. State in
//! the PR that and why the checksums change. To regenerate after such a
//! change:
//!
//! ```text
//! cargo test --release -p sf_sim --test golden -- --ignored --nocapture print_golden_values
//! ```

mod common;

use common::{Scenario, run, world_with};
use sf_math::hash::Fnv1a64;
use sf_sim::{Command, Kind, PlayerId, Tick, World, WorldConfig};

/// `(history index, checksum)` pairs: entry `k` is the checksum after tick `k`.
type Checkpoints = [(usize, u64)];

fn check(history: &[u64], golden: &Checkpoints) {
    for &(k, want) in golden {
        assert_eq!(
            history[k], want,
            "checksum after tick {k} is {:#018x}, the committed value is {want:#018x}",
            history[k]
        );
    }
}

fn tiny_world() -> World {
    let mut w = World::new(WorldConfig::new(1).with_map_size(64, 64)).unwrap();
    for (n, tick) in [(0u16, 0u64), (1, 0), (2, 3)] {
        w.schedule(
            Tick(tick),
            PlayerId(n as u8),
            Command::Spawn {
                kind: Kind(n),
                pos: sf_math::Vec2::from_ints(8 + 20 * i32::from(n), 30),
                owner: PlayerId(n as u8 % 2),
            },
        )
        .unwrap();
    }
    w
}

fn snapshot_hash(w: &World) -> u64 {
    let mut h = Fnv1a64::new();
    h.write_u64(w.tick().0);
    for v in w.snapshot().iter() {
        h.write_u32(v.id.index());
        h.write_u32(v.id.generation());
        h.write_vec2(v.prev_pos);
        h.write_vec2(v.pos);
        h.write_facing(v.prev_facing);
        h.write_facing(v.facing);
    }
    h.finish()
}

// ---- committed values ---------------------------------------------------------

const TINY_HISTORY: [u64; 20] = [
    0xdeea_dbd1_6a8c_8bb4,
    0x523e_a845_1a4c_7a63,
    0xd32f_23d6_605f_d3f2,
    0x752f_8be6_dae4_d127,
    0xfd5a_0f90_0c97_841e,
    0x7199_79bb_1182_6c96,
    0x5fd9_ccdb_86ce_ab48,
    0x6ef3_81dc_0418_9415,
    0x9212_272e_c109_e57f,
    0x6d2f_c8a0_fc9b_299a,
    0x8ff5_7c13_2f87_204b,
    0x08ab_5f5b_24c6_952c,
    0x1199_2211_ff42_1add,
    0xca46_4142_790d_78b0,
    0x2870_c623_f534_0031,
    0x508d_134a_6bc0_b44e,
    0xcc2a_2903_87be_29b2,
    0xdac9_d23a_8056_af06,
    0x6f67_12e5_825b_74d4,
    0x9567_cd21_8485_59f4,
];

const SMALL_CHECKPOINTS: [(usize, u64); 10] = [
    (0, 0x6352_45b1_4520_341a),
    (9, 0x6cce_47cf_4154_d82d),
    (49, 0xf75a_60cf_da00_e533),
    (99, 0x33d0_1989_949a_baeb),
    (249, 0x7813_3cac_286f_afe5),
    (499, 0x090b_7412_e42c_5b39),
    (749, 0x1622_e381_5aaa_327f),
    (999, 0x5224_e4c3_7cc6_cc6a),
    (1_249, 0x4f8d_1458_b90f_3ca5),
    (1_499, 0x26fb_2911_2853_9e33),
];

const SMALL_SNAPSHOT_HASH: u64 = 0x8d50_fe56_180b_4914;

const FULL_CHECKPOINTS: [(usize, u64); 11] = [
    (0, 0x52a4_964f_24ae_7c56),
    (999, 0x4b30_81ed_8d83_08dd),
    (1_999, 0xfbe0_1f3e_6ab7_e36d),
    (2_999, 0x0967_bd81_5789_36a0),
    (3_999, 0x5701_ded2_144d_f515),
    (4_999, 0x1d4c_de4c_6bf2_4a30),
    (5_999, 0x26b8_52c5_2512_a101),
    (6_999, 0xc5b5_1a34_7468_8174),
    (7_999, 0x5977_ef82_fc36_ddfa),
    (8_999, 0x6c5c_e016_cb82_2d5e),
    (9_999, 0xf0bc_5e9a_6c67_35d9),
];

// ---- the tests ----------------------------------------------------------------

#[test]
fn tiny_scenario_history() {
    let mut w = tiny_world();
    let history = run(&mut w, 20);
    assert_eq!(history, TINY_HISTORY, "history: {history:#018x?}");
}

#[test]
fn small_scenario_checkpoints() {
    let s = Scenario::small();
    let mut w = world_with(&s);
    let history = run(&mut w, s.ticks);
    check(&history, &SMALL_CHECKPOINTS);
}

#[test]
fn small_scenario_interpolation_state() {
    let s = Scenario::small();
    let mut w = world_with(&s);
    run(&mut w, s.ticks);
    assert_eq!(snapshot_hash(&w), SMALL_SNAPSHOT_HASH);
}

#[test]
#[cfg_attr(debug_assertions, ignore = "full-size run: use --release")]
fn full_scenario_checkpoints() {
    let s = Scenario::full();
    let mut w = world_with(&s);
    let history = run(&mut w, s.ticks);
    check(&history, &FULL_CHECKPOINTS);
}

/// Prints fresh values to paste above. Not a test.
#[test]
#[ignore = "regeneration helper, run by hand"]
fn print_golden_values() {
    let mut w = tiny_world();
    println!(
        "const TINY_HISTORY: [u64; 20] = {:#018x?};",
        run(&mut w, 20)
    );

    let s = Scenario::small();
    let mut w = world_with(&s);
    let history = run(&mut w, s.ticks);
    println!("const SMALL_CHECKPOINTS: [(usize, u64); 10] = [");
    for k in [0, 9, 49, 99, 249, 499, 749, 999, 1_249, 1_499] {
        println!("    ({k}, {:#018x}),", history[k]);
    }
    println!("];");
    println!(
        "const SMALL_SNAPSHOT_HASH: u64 = {:#018x};",
        snapshot_hash(&w)
    );

    if cfg!(debug_assertions) {
        println!("(full scenario skipped in a debug build)");
        return;
    }
    let s = Scenario::full();
    let mut w = world_with(&s);
    let history = run(&mut w, s.ticks);
    println!("const FULL_CHECKPOINTS: [(usize, u64); 11] = [");
    for k in [
        0, 999, 1_999, 2_999, 3_999, 4_999, 5_999, 6_999, 7_999, 8_999, 9_999,
    ] {
        println!("    ({k}, {:#018x}),", history[k]);
    }
    println!("];");
}
