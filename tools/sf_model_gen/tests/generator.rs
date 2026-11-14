//! The generator is deterministic and the committed set v5 files are its output (WP11.6a), as in
//! `tools/sf_sound_gen/tests/generator.rs`. Integer geometry and basic float operations only, so
//! these numbers hold on every platform; a recipe or palette change moves them on purpose.

use std::path::{Path, PathBuf};

use sf_model_gen::files;

/// (file, byte length, FNV-1a 64 of the bytes), pinned as literals, sorted by file name.
const PINNED: [(&str, usize, u64); 6] = [
    ("meridian_construction_center.glb", 0, 0),
    ("meridian_harvester.glb", 0, 0),
    ("meridian_light_tank.glb", 0, 0),
    ("veyran_defence_tower.glb", 0, 0),
    ("veyran_light_tank.glb", 0, 0),
    ("veyran_power_plant.glb", 0, 0),
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
    for ((name, bytes), (pin_name, len, hash)) in first.iter().zip(PINNED) {
        println!("    (\"{name}\", {}, {:#018x}),", bytes.len(), fnv1a(bytes));
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
