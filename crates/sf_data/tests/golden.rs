//! Golden hashes of the generated maps (follow-up 1 of phase 1).
//!
//! The generator is integer and `sf_math` fixed point only, so a map is the
//! same on every machine, in debug and in release. These hashes are pinned in
//! the source and CI runs the test on Linux and on Windows (debug in the
//! workspace run, release in its own step): the two systems agree because both
//! must match the numbers below.
//!
//! If the generator changes on purpose (a different look), the hashes change
//! with it: run the test, copy the printed values, and say so in the PR. A
//! change that nobody meant to make shows up here first, and the simulation
//! (which will read terrain from maps, WP2.1) must never see it unnoticed.

use sf_data::map::Map;
use sf_data::map::generate::generate_cells;
use sf_math::hash::Fnv1a64;

/// Hash of the width, the height, every vertex height and every material
/// index, as two numbers so a failure says which half changed.
fn hashes(map: &Map) -> (u64, u64) {
    let mut heights = Fnv1a64::new();
    heights.write_u32(map.width());
    heights.write_u32(map.height());
    for &h in map.heights() {
        heights.write_bytes(&h.to_le_bytes());
    }
    let mut cells = Fnv1a64::new();
    cells.write_bytes(map.cells());
    (heights.finish(), cells.finish())
}

fn check(cells: u32, seed: u32, expected: (u64, u64)) {
    let map = generate_cells(cells, cells, seed);
    let got = hashes(&map);
    println!(
        "{cells}x{cells} cells, seed {seed}: heights {:#018x}, materials {:#018x}",
        got.0, got.1
    );
    assert_eq!(
        got.0, expected.0,
        "{cells}x{cells} cells, seed {seed}: the heights changed"
    );
    assert_eq!(
        got.1, expected.1,
        "{cells}x{cells} cells, seed {seed}: the materials changed"
    );
}

/// 64 x 64 cells = 256 x 256 m, the smallest map size of PLAN.md 1.3.
#[test]
fn the_64_cell_map_is_pinned() {
    check(64, 1, (0x2358_d8fd_db4e_3826, 0x3fa7_2356_59d8_4c3e));
}

/// 256 x 256 cells = 1,024 x 1,024 m, the largest map and the demo map.
#[test]
fn the_256_cell_map_is_pinned() {
    check(256, 1, (0x3652_2a04_d73b_8be1, 0xed4e_8e43_a9c3_7b13));
}

/// Another seed, so a change that only touches a seed-dependent path (the
/// hills, the river phase) cannot hide behind seed 1.
#[test]
fn another_seed_is_pinned_too() {
    check(64, 7, (0x8e3f_3b41_1d30_2795, 0xc9e8_2e36_c67d_aefc));
}
