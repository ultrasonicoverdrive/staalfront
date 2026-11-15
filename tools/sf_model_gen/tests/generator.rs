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
// WP11.6c: 137 files added (every structure's states, the mission objects, the heaps); the six
// files above kept their bytes (the faster AO pre-filter and the palette's new entries change none).
// Re-pinned 15-11-2026 (WP11.6c review): every stage at most its finished model's LOD0 share, no
// detail floor, stripes top-only; the finished models and the units unchanged.
// Re-pinned 15-11-2026 (WP11.6r, shape refinement): smooth normals on turned surfaces (crease
// 40°), no facet variation there, the kit's bevel rule on boxes, wider recipe bevels and at least
// 16 segments on round parts in LOD0. Every file moved. A one-off scratch check (not reproducible
// from the branch): with those four kit rules edited back by hand, `--check` against the previous
// set found all 157 files equal. Re-pinned again (WP11.6r review): the LOD0-only rules no longer
// reach the stages' far levels, the ferry and gunboat keep funnel and mast in LOD2, and the fallen
// roof planks carry `stages::PLANK_TONE`.
// Re-pinned 15-11-2026 (WP11.6p, realistic palette): the albedo palette of model_palette.ron, grime
// 0.30 -> 0.20 plus dust towards (118, 104, 82), highlight 0.16 -> 0.05, wear 0.35 -> 0.20 with the
// paint's tint, panel lines 560 -> 500; team vertices carry no grime. Every file moved; no recipe
// or kit geometry changed, so only COLOR_0 and the material factors differ (9 files are a few bytes
// longer or shorter because a factor prints with other digits).
const PINNED: [(&str, usize, u64); 143] = [
    ("meridian_advanced_defences.glb", 456824, 0x493c81a8a7cc5fc2),
    (
        "meridian_advanced_defences_damaged.glb",
        257040,
        0x6d2f685f4f52f6b5,
    ),
    (
        "meridian_advanced_defences_foundation.glb",
        88760,
        0x35e4074ace3504bf,
    ),
    (
        "meridian_advanced_defences_heavily_damaged.glb",
        262304,
        0xc1ce653f31033306,
    ),
    (
        "meridian_advanced_defences_scaffolding.glb",
        205364,
        0x4186440e08266044,
    ),
    ("meridian_barracks.glb", 400908, 0x8642c30be3e2861f),
    ("meridian_barracks_damaged.glb", 203460, 0xf5e2f04d405fa785),
    (
        "meridian_barracks_foundation.glb",
        93588,
        0x28845f529b4ed948,
    ),
    (
        "meridian_barracks_heavily_damaged.glb",
        189740,
        0x04259ce92ab0cdfa,
    ),
    (
        "meridian_barracks_scaffolding.glb",
        194648,
        0xff8687ff89546bcf,
    ),
    (
        "meridian_construction_center.glb",
        541156,
        0x706f58dc9b68ad53,
    ),
    (
        "meridian_construction_center_damaged.glb",
        428892,
        0xb9ffe3a6326325d1,
    ),
    (
        "meridian_construction_center_foundation.glb",
        114132,
        0x15ac667bd685375e,
    ),
    (
        "meridian_construction_center_heavily_damaged.glb",
        272104,
        0x2553c51c50c7a3a3,
    ),
    (
        "meridian_construction_center_scaffolding.glb",
        266532,
        0xba6552361fe0263b,
    ),
    ("meridian_decoy_emplacement.glb", 436352, 0xb9ea4190bd801ef6),
    (
        "meridian_decoy_emplacement_damaged.glb",
        209980,
        0x1e78975dae13d9d9,
    ),
    (
        "meridian_decoy_emplacement_foundation.glb",
        82136,
        0xc6029bffabbda327,
    ),
    (
        "meridian_decoy_emplacement_heavily_damaged.glb",
        240156,
        0x4d3944106525b5a4,
    ),
    (
        "meridian_decoy_emplacement_scaffolding.glb",
        175772,
        0x50ba284c4e4e9f22,
    ),
    ("meridian_defence_tower.glb", 368804, 0x1e4cc43003f324ab),
    (
        "meridian_defence_tower_damaged.glb",
        155752,
        0x41b8cd957c89d6ef,
    ),
    (
        "meridian_defence_tower_foundation.glb",
        82168,
        0xabc3ba8207b3b6a2,
    ),
    (
        "meridian_defence_tower_heavily_damaged.glb",
        182940,
        0xb39437517e9a1988,
    ),
    (
        "meridian_defence_tower_scaffolding.glb",
        114360,
        0xab887c949d00f456,
    ),
    ("meridian_harvester.glb", 290264, 0x0ebaaffbb59d7fc0),
    ("meridian_light_tank.glb", 281412, 0x31e34f09fd6cac6f),
    ("meridian_ore_silo.glb", 380516, 0x7dc0ee03967e5930),
    ("meridian_ore_silo_damaged.glb", 210944, 0x0f1bb123b2a015c4),
    (
        "meridian_ore_silo_foundation.glb",
        88752,
        0x10e21f88cd1ec98c,
    ),
    (
        "meridian_ore_silo_heavily_damaged.glb",
        212448,
        0x5a6f99a15b3b161f,
    ),
    (
        "meridian_ore_silo_scaffolding.glb",
        142684,
        0xb6a83a9c22a0bdea,
    ),
    ("meridian_power_plant.glb", 400008, 0x71e8fc0ba5e4b7ab),
    (
        "meridian_power_plant_damaged.glb",
        309500,
        0x22a7daf3f0fddab0,
    ),
    (
        "meridian_power_plant_foundation.glb",
        93540,
        0x9b955d1789f1e256,
    ),
    (
        "meridian_power_plant_heavily_damaged.glb",
        212572,
        0xaf5967a9447c2957,
    ),
    (
        "meridian_power_plant_scaffolding.glb",
        194192,
        0x726c43e7724caff1,
    ),
    ("meridian_radar.glb", 376508, 0x563580ac39c814af),
    ("meridian_radar_damaged.glb", 184096, 0x52a691aefe086a96),
    ("meridian_radar_foundation.glb", 88740, 0x5e1b5b4f4421bfe1),
    (
        "meridian_radar_heavily_damaged.glb",
        144796,
        0xcac8391d2b24697c,
    ),
    ("meridian_radar_scaffolding.glb", 193024, 0x9349604729143b1d),
    ("meridian_refinery.glb", 528924, 0xac3da79b471834b1),
    ("meridian_refinery_damaged.glb", 402484, 0x773f5f27597850e2),
    (
        "meridian_refinery_foundation.glb",
        107464,
        0x60d92bcfee9b8a45,
    ),
    (
        "meridian_refinery_heavily_damaged.glb",
        300440,
        0x19fd900f0a874fbf,
    ),
    (
        "meridian_refinery_scaffolding.glb",
        237840,
        0xf6213e9693228f58,
    ),
    ("meridian_shipyard.glb", 551080, 0xb2c13b56374a08fa),
    ("meridian_shipyard_damaged.glb", 406272, 0xf362119c750451f6),
    (
        "meridian_shipyard_foundation.glb",
        119544,
        0x6a6be0e8b8ddb832,
    ),
    (
        "meridian_shipyard_heavily_damaged.glb",
        301956,
        0x19ddd6287de8df9d,
    ),
    (
        "meridian_shipyard_scaffolding.glb",
        280832,
        0xb828e0c28813a7ad,
    ),
    ("meridian_vehicle_factory.glb", 409784, 0x0e0505c84ad07a66),
    (
        "meridian_vehicle_factory_damaged.glb",
        296972,
        0xb3d919d406a1809e,
    ),
    (
        "meridian_vehicle_factory_foundation.glb",
        107484,
        0x93608fc0067f6e71,
    ),
    (
        "meridian_vehicle_factory_heavily_damaged.glb",
        239396,
        0x117d0b53a7a2172c,
    ),
    (
        "meridian_vehicle_factory_scaffolding.glb",
        253960,
        0x01f73fe5366979d3,
    ),
    ("network_automated_gun.glb", 181556, 0x6a41fd491b68a57e),
    (
        "network_automated_gun_disabled.glb",
        181584,
        0x6bf22a36ef67e055,
    ),
    (
        "network_automated_gun_rubble.glb",
        84984,
        0x5a0548324f524658,
    ),
    ("network_unmanned_artillery.glb", 316536, 0x8cf8300c69729f49),
    (
        "network_unmanned_artillery_no_uplink.glb",
        205680,
        0xdce28987005311ae,
    ),
    (
        "network_unmanned_artillery_rubble.glb",
        182328,
        0x75ddd37525c9b42d,
    ),
    ("neutral_archive_building.glb", 275472, 0xcb3f0631306a3740),
    (
        "neutral_archive_building_damaged.glb",
        183748,
        0x07b7ae187e196b1a,
    ),
    (
        "neutral_archive_building_heavily_damaged.glb",
        120108,
        0xbe5d36378c48d546,
    ),
    ("neutral_bridge_segment.glb", 148732, 0x09480b1a028c1f29),
    (
        "neutral_bridge_segment_damaged.glb",
        93524,
        0x3de1e00bb74a3396,
    ),
    (
        "neutral_bridge_segment_destroyed.glb",
        87276,
        0x919c2b177b742d05,
    ),
    ("neutral_evidence_crate.glb", 32084, 0x5e9f12cb9b0f8f35),
    ("neutral_fuel_depot.glb", 262132, 0xf9b50325c9d20e62),
    (
        "neutral_fuel_depot_burnt_out.glb",
        144744,
        0xd15c84cafc3fa8ba,
    ),
    ("neutral_fuel_depot_damaged.glb", 209836, 0xbebf5d7e68b44bc1),
    ("neutral_generator.glb", 154904, 0x098cbb90337ffb6e),
    ("neutral_generator_off.glb", 153632, 0x62dfdc8c887d0422),
    ("neutral_generator_rubble.glb", 69420, 0x23ba34004e27c305),
    ("neutral_rail_spur.glb", 146276, 0x105ed4e5a93ccf31),
    ("neutral_rail_spur_destroyed.glb", 57648, 0x97441704b5edd91d),
    ("neutral_relay_transmitter.glb", 286012, 0x98143acb9f77047d),
    (
        "neutral_relay_transmitter_damaged.glb",
        230168,
        0x908f1a353caf253e,
    ),
    (
        "neutral_relay_transmitter_rubble.glb",
        112368,
        0xac2e2e119cd19e22,
    ),
    ("neutral_rubble_1x1.glb", 32180, 0x45c5eccc0aae6165),
    ("neutral_rubble_2x2.glb", 101676, 0x013d001e5eac9523),
    ("neutral_rubble_2x3.glb", 133432, 0xc560600fca05b6de),
    ("neutral_rubble_3x4.glb", 143888, 0xe39072efac688d3f),
    ("neutral_rubble_4x4.glb", 143892, 0x3bdd61aa97fcf817),
    ("neutral_rubble_4x5.glb", 143884, 0xfb70515fd318bb03),
    ("veyran_advanced_defences.glb", 411472, 0x36d78fb52036021c),
    (
        "veyran_advanced_defences_damaged.glb",
        242372,
        0x17f44515f9a54c22,
    ),
    (
        "veyran_advanced_defences_foundation.glb",
        88756,
        0x2b70ba68672121c8,
    ),
    (
        "veyran_advanced_defences_heavily_damaged.glb",
        259132,
        0x1510ccdd9589cd9c,
    ),
    (
        "veyran_advanced_defences_scaffolding.glb",
        169384,
        0xcc47943afc30353e,
    ),
    ("veyran_barracks.glb", 398852, 0x17af7b075a3fa47c),
    ("veyran_barracks_damaged.glb", 174524, 0xb398c6dc5ddd26f3),
    ("veyran_barracks_foundation.glb", 93584, 0x4f033a6d01c13807),
    (
        "veyran_barracks_heavily_damaged.glb",
        187476,
        0x55d87f98cc004182,
    ),
    (
        "veyran_barracks_scaffolding.glb",
        191256,
        0x5f59ea5c2c6aa046,
    ),
    ("veyran_construction_center.glb", 526384, 0x935aaa34ba07ee39),
    (
        "veyran_construction_center_damaged.glb",
        392128,
        0x7980676fdeb437e4,
    ),
    (
        "veyran_construction_center_foundation.glb",
        114120,
        0x74a941d11e2c3556,
    ),
    (
        "veyran_construction_center_heavily_damaged.glb",
        327524,
        0xbd334c4ad46e203f,
    ),
    (
        "veyran_construction_center_scaffolding.glb",
        236228,
        0xa931aef1474639c1,
    ),
    ("veyran_defence_tower.glb", 371336, 0x51dafef78f1d52a6),
    (
        "veyran_defence_tower_damaged.glb",
        235164,
        0xe55fea214e1f386e,
    ),
    (
        "veyran_defence_tower_foundation.glb",
        82176,
        0x112f3fd0c85a7516,
    ),
    (
        "veyran_defence_tower_heavily_damaged.glb",
        214988,
        0x71a7eb5097bf7fb8,
    ),
    (
        "veyran_defence_tower_scaffolding.glb",
        148564,
        0x188adee828b23f55,
    ),
    ("veyran_fortified_bunker.glb", 361356, 0xf18cfc0665708fb4),
    (
        "veyran_fortified_bunker_damaged.glb",
        158396,
        0x2aaa81176090f583,
    ),
    (
        "veyran_fortified_bunker_foundation.glb",
        88756,
        0x62c576fd9471e2be,
    ),
    (
        "veyran_fortified_bunker_heavily_damaged.glb",
        177532,
        0x309e26239047c342,
    ),
    (
        "veyran_fortified_bunker_scaffolding.glb",
        203760,
        0x665da2817ca622fa,
    ),
    ("veyran_light_tank.glb", 270056, 0x16cd830d5ce6379b),
    ("veyran_ore_silo.glb", 375396, 0x3cd004d5d9d85633),
    ("veyran_ore_silo_damaged.glb", 212528, 0xf9ed6a94c89a80c7),
    ("veyran_ore_silo_foundation.glb", 88748, 0x6d47948decf02e85),
    (
        "veyran_ore_silo_heavily_damaged.glb",
        211200,
        0x57ba7d477ab9d401,
    ),
    (
        "veyran_ore_silo_scaffolding.glb",
        139032,
        0x70c15ce208b26aaa,
    ),
    ("veyran_power_plant.glb", 496380, 0x51aa61fb52baa7ce),
    ("veyran_power_plant_damaged.glb", 395996, 0x4babb65e0836bf07),
    (
        "veyran_power_plant_foundation.glb",
        93536,
        0xb23ea94f10a19e3e,
    ),
    (
        "veyran_power_plant_heavily_damaged.glb",
        256580,
        0xe41e38e69b1f5936,
    ),
    (
        "veyran_power_plant_scaffolding.glb",
        201940,
        0x2baf41d686441c87,
    ),
    ("veyran_radar.glb", 388524, 0x5b4d05ac2eeb5104),
    ("veyran_radar_damaged.glb", 233460, 0xf4703e6c05456914),
    ("veyran_radar_foundation.glb", 88736, 0xeea4fb4557f0e08c),
    (
        "veyran_radar_heavily_damaged.glb",
        157712,
        0xe54908a59185bbfa,
    ),
    ("veyran_radar_scaffolding.glb", 210852, 0x521e9d4018225f16),
    ("veyran_refinery.glb", 495868, 0xe19fe6be478a070e),
    ("veyran_refinery_damaged.glb", 363884, 0x34f99204a30c674e),
    ("veyran_refinery_foundation.glb", 107464, 0x6d9d2b4ec3f751ae),
    (
        "veyran_refinery_heavily_damaged.glb",
        299788,
        0x06397d0a71e7311d,
    ),
    (
        "veyran_refinery_scaffolding.glb",
        244056,
        0x8fa20e414af69631,
    ),
    ("veyran_shipyard.glb", 505748, 0xd5fe6ad57c0aeb3d),
    ("veyran_shipyard_damaged.glb", 363492, 0x1a387aa6bf8da41e),
    ("veyran_shipyard_foundation.glb", 119540, 0xc066a3276fa8675d),
    (
        "veyran_shipyard_heavily_damaged.glb",
        304036,
        0xfe760c978a0ee3a4,
    ),
    (
        "veyran_shipyard_scaffolding.glb",
        283384,
        0xf1cd0e5aeb96d0b7,
    ),
    ("veyran_vehicle_factory.glb", 415336, 0x62181478f61a9d0a),
    (
        "veyran_vehicle_factory_damaged.glb",
        269260,
        0xeb7fc3e5f46fa9b0,
    ),
    (
        "veyran_vehicle_factory_foundation.glb",
        107480,
        0xf0d43724266a859a,
    ),
    (
        "veyran_vehicle_factory_heavily_damaged.glb",
        249800,
        0xfcb48d0a4f7e6e67,
    ),
    (
        "veyran_vehicle_factory_scaffolding.glb",
        258776,
        0xcadac361b83259b1,
    ),
];

/// The units of WP11.6b (vehicles, vessels, the heavy gun team), pinned the same way; see
// the first table for how to re-pin. Sorted by file name.
const PINNED_UNITS: [(&str, usize, u64); 14] = [
    (
        "meridian_amphibious_transport.glb",
        237748,
        0xbbb02a49f93442e3,
    ),
    ("meridian_heavy_tank.glb", 312352, 0x2d7dc2f0c8290285),
    ("meridian_light_transport.glb", 218072, 0xd946d9f6a0aa18d7),
    ("meridian_repair_truck.glb", 242956, 0xcc9fbbbc548dd93d),
    ("meridian_siege_artillery.glb", 308432, 0x31b3d24720ea62f6),
    ("neutral_ferry.glb", 208644, 0xae3854f34beb209a),
    ("neutral_supply_truck.glb", 287884, 0xdde680fd6205e776),
    ("veyran_gunboat.glb", 287436, 0x26d6f6764e37bfff),
    ("veyran_harvester.glb", 267916, 0x7a6be173be3a0d3e),
    ("veyran_heavy_gun_team.glb", 141092, 0x3ad70115bbb83d38),
    ("veyran_heavy_tank.glb", 325092, 0x9a1a2a06a35cde90),
    ("veyran_repair_truck.glb", 240216, 0x5b83ce0f03d24c03),
    ("veyran_siege_artillery.glb", 307484, 0x01640589a22c2c4e),
    ("veyran_transport_truck.glb", 224936, 0x210ef439eb47c08f),
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

/// One run of the generator, shared by the tests of this file (a run takes a while: 157 files).
fn first() -> &'static [(String, Vec<u8>)] {
    static F: std::sync::OnceLock<Vec<(String, Vec<u8>)>> = std::sync::OnceLock::new();
    F.get_or_init(files)
}

#[test]
fn two_runs_give_the_same_bytes_and_every_file_is_pinned() {
    let first = first();
    // Print the whole table first, so a deliberate change can be pasted in one go.
    for (name, bytes) in first {
        println!("    (\"{name}\", {}, {:#018x}),", bytes.len(), fnv1a(bytes));
    }
    let second = files();
    assert!(first == second.as_slice(), "two runs differ");
    // The units of WP11.6b are pinned in their own table; a file is in exactly one.
    assert_eq!(first.len(), PINNED.len() + PINNED_UNITS.len());
    for (name, bytes) in first {
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
    for (name, bytes) in first() {
        let on_disk = std::fs::read(set_dir().join(name))
            .unwrap_or_else(|e| panic!("{name} is not committed: {e}"));
        assert!(
            on_disk == *bytes,
            "{name} is out of date: run `cargo run -p sf_model_gen`"
        );
    }
}
