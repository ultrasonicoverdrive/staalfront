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
    check(64, 1, (0x872e_74f4_80f8_c64d, 0xa2ea_ee42_c7f7_4e8c));
}

/// 256 x 256 cells = 1,024 x 1,024 m, the largest map and the demo map.
#[test]
fn the_256_cell_map_is_pinned() {
    check(256, 1, (0xdeab_ab98_9866_e939, 0xae6b_97c9_9e37_0857));
}

/// Another seed, so a change that only touches a seed-dependent path (the
/// hills, the river phase) cannot hide behind seed 1.
#[test]
fn another_seed_is_pinned_too() {
    check(64, 7, (0x9915_8215_647e_8d16, 0xcd4c_0943_d878_0a37));
}

// --- the terrain layer (WP2.1) ---

/// Hash of the terrain layer's size and every cell's type id, with the count
/// of each type (road, water, rock, ore, bridge; grass is the rest), so a
/// failure shows what moved.
fn terrain_summary(map: &Map) -> (u64, [u32; 5]) {
    let layer = map.terrain().expect("generated maps have a terrain layer");
    let mut hash = Fnv1a64::new();
    hash.write_u32(layer.width());
    hash.write_u32(layer.height());
    hash.write_bytes(layer.ids());
    let h = layer.histogram();
    (hash.finish(), [h[1], h[2], h[3], h[4], h[5]])
}

fn check_terrain(cells: u32, seed: u32, expected: (u64, [u32; 5])) {
    let got = terrain_summary(&generate_cells(cells, cells, seed));
    println!(
        "{cells}x{cells} cells, seed {seed}: terrain {:#018x}, {:?}",
        got.0, got.1
    );
    assert_eq!(
        got, expected,
        "{cells}x{cells} cells, seed {seed}: the terrain layer changed"
    );
}

/// The layer is the part of a generated map the simulation reads, so it is
/// pinned like the heights and the materials. Counts: road, water, rock, ore,
/// bridge.
#[test]
fn the_64_cell_terrain_layer_is_pinned() {
    check_terrain(64, 1, (0x1678_1ca8_7864_1041, [101, 141, 517, 51, 2]));
}

#[test]
fn the_256_cell_terrain_layer_is_pinned() {
    check_terrain(256, 1, (0xaee0_042c_3575_1af9, [407, 563, 7283, 215, 2]));
}

#[test]
fn another_seed_terrain_layer_is_pinned_too() {
    check_terrain(64, 7, (0x0f67_ce2f_8562_8214, [101, 139, 474, 53, 2]));
}
