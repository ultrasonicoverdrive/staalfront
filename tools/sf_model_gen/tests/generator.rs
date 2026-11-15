//! The generator is deterministic and the committed set v5 files are its output (WP11.6a), as in
//! `tools/sf_sound_gen/tests/generator.rs`. Integer geometry and basic float operations only, so
//! these numbers hold on every platform; a recipe or palette change moves them on purpose.

use std::path::{Path, PathBuf};

use sf_model_gen::files;

/// (file, byte length, FNV-1a 64 of the bytes), pinned as literals, sorted by file name.
// Re-pinned 14-11-2026 (tuning round): team as bands and trim, stronger AO and grime, the top-edge
// highlight and edge wear (RGB vertex colour, a grey material factor), the building detail.
// Re-pinned 14-11-2026 (refinement round 2): narrow team stripes (<= 350 mm, more of them),
// Veyran's steel tones 25 % lighter and cooler, no edge wear on lathed surfaces (the dome).
const PINNED: [(&str, usize, u64); 6] = [
    (
        "meridian_construction_center.glb",
        519268,
        0x703bf9b24e8e6b0f,
    ),
    ("meridian_harvester.glb", 251960, 0xfdc6d76c5edada0d),
    ("meridian_light_tank.glb", 242188, 0xa78c91f749af33da),
    ("veyran_defence_tower.glb", 343516, 0xe2b3fb24edd0e4d5),
    ("veyran_light_tank.glb", 223532, 0x520461c2447f5b49),
    ("veyran_power_plant.glb", 473580, 0xd0deb10bf3a80a22),
];

/// The units of WP11.6b (vehicles, vessels, the heavy gun team), pinned the same way; see
// the first table for how to re-pin. Sorted by file name.
const PINNED_UNITS: [(&str, usize, u64); 14] = [
    (
        "meridian_amphibious_transport.glb",
        237748,
        0x5705da54c8d406d2,
    ),
    ("meridian_heavy_tank.glb", 253968, 0x772a76d76a58e293),
    ("meridian_light_transport.glb", 207124, 0x728c043dc7e556a3),
    ("meridian_repair_truck.glb", 232920, 0xe6fa17f290f5c1cf),
    ("meridian_siege_artillery.glb", 245488, 0xbb18db5651739bf4),
    ("neutral_ferry.glb", 189808, 0x5828524cb25235d2),
    ("neutral_supply_truck.glb", 251852, 0xe2a6f0d2ccbd3b73),
    ("veyran_gunboat.glb", 259836, 0x4542c9605d2926b6),
    ("veyran_harvester.glb", 224140, 0xb53490113107c676),
    ("veyran_heavy_gun_team.glb", 120972, 0x6420c1e02be053d4),
    ("veyran_heavy_tank.glb", 255764, 0x017c668898bb6c33),
    ("veyran_repair_truck.glb", 230184, 0x0ccef6d6d87e7390),
    ("veyran_siege_artillery.glb", 244544, 0x5241858478a4aed4),
    ("veyran_transport_truck.glb", 224936, 0xca886da6e95e8833),
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
    assert_eq!(first.len(), PINNED.len() + PINNED_UNITS.len());
    // Print the whole table first, so a deliberate change can be pasted in one go.
    for (name, bytes) in &first {
        println!("    (\"{name}\", {}, {:#018x}),", bytes.len(), fnv1a(bytes));
    }
    for (name, bytes) in &first {
        let (_, len, hash) = PINNED
            .iter()
            .chain(PINNED_UNITS.iter())
            .find(|p| p.0 == name)
            .unwrap_or_else(|| panic!("{name} is not pinned"));
        assert_eq!(bytes.len(), *len, "{name} length");
        assert_eq!(
            fnv1a(bytes),
            *hash,
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
