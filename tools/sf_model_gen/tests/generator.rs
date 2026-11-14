//! The generator is deterministic and the committed set v5 files are its output (WP11.6a), as in
//! `tools/sf_sound_gen/tests/generator.rs`. Integer geometry and basic float operations only, so
//! these numbers hold on every platform; a recipe or palette change moves them on purpose.

use std::path::{Path, PathBuf};

use sf_model_gen::files;

/// (file, byte length, FNV-1a 64 of the bytes), pinned as literals, sorted by file name.
// Re-pinned 14-11-2026 (tuning round): team as bands and trim, stronger AO and grime, the top-edge
// highlight and edge wear (RGB vertex colour, a grey material factor), the building detail.
const PINNED: [(&str, usize, u64); 6] = [
    (
        "meridian_construction_center.glb",
        467696,
        0x7cc84bc66bda2692,
    ),
    ("meridian_harvester.glb", 242380, 0x98f689bd8c9c7181),
    ("meridian_light_tank.glb", 242188, 0xb36968875a2cc372),
    ("veyran_defence_tower.glb", 343516, 0x3cda5c0218fec60a),
    ("veyran_light_tank.glb", 221024, 0xf5f43c70b26a35b6),
    ("veyran_power_plant.glb", 467204, 0xd1fdd6f5a27bd3af),
];

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in bytes {
        h = (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn set_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/set_v5")
}

#[test]
fn two_runs_give_the_same_bytes_and_every_file_is_pinned() {
    let first = files();
    let second = files();
    assert_eq!(first, second, "two runs differ");
    assert_eq!(first.len(), PINNED.len());
    // Print the whole table first, so a deliberate change can be pasted in one go.
    for (name, bytes) in &first {
        println!("    (\"{name}\", {}, {:#018x}),", bytes.len(), fnv1a(bytes));
    }
    for ((name, bytes), (pin_name, len, hash)) in first.iter().zip(PINNED) {
        assert_eq!(name, pin_name);
        assert_eq!(bytes.len(), len, "{name} length");
        assert_eq!(
            fnv1a(bytes),
            hash,
            "{name} hash (a recipe or the palette changed?)"
        );
    }
}

#[test]
fn the_committed_files_are_what_a_fresh_run_writes() {
    for (name, bytes) in files() {
        let on_disk = std::fs::read(set_dir().join(&name))
            .unwrap_or_else(|e| panic!("{name} is not committed: {e}"));
        assert!(
            on_disk == bytes,
            "{name} is out of date: run `cargo run -p sf_model_gen`"
        );
    }
}
