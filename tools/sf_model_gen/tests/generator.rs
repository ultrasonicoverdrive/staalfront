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
const PINNED: [(&str, usize, u64); 143] = [
    ("meridian_advanced_defences.glb", 456824, 0xb0e314c2c3b77ccc),
    (
        "meridian_advanced_defences_damaged.glb",
        257040,
        0xbf8a4d3e2c99d0c1,
    ),
    (
        "meridian_advanced_defences_foundation.glb",
        88760,
        0x7569c02949d89a5e,
    ),
    (
        "meridian_advanced_defences_heavily_damaged.glb",
        262304,
        0x76b56fba9ae4325d,
    ),
    (
        "meridian_advanced_defences_scaffolding.glb",
        205364,
        0x9576bf424fc43781,
    ),
    ("meridian_barracks.glb", 400908, 0x20b428c0933c8670),
    ("meridian_barracks_damaged.glb", 203460, 0x33826857d688a147),
    (
        "meridian_barracks_foundation.glb",
        93588,
        0xdc00007ccc2f3ea7,
    ),
    (
        "meridian_barracks_heavily_damaged.glb",
        189740,
        0x480566718fc258f0,
    ),
    (
        "meridian_barracks_scaffolding.glb",
        194648,
        0xb0a94229b84c9686,
    ),
    (
        "meridian_construction_center.glb",
        541156,
        0x54b271d091b95e9b,
    ),
    (
        "meridian_construction_center_damaged.glb",
        428892,
        0x740ff3b7e5ac216d,
    ),
    (
        "meridian_construction_center_foundation.glb",
        114132,
        0x18a82eef2f2401e3,
    ),
    (
        "meridian_construction_center_heavily_damaged.glb",
        272104,
        0xeea8e292543e80a6,
    ),
    (
        "meridian_construction_center_scaffolding.glb",
        266532,
        0x2e34abd3834c5b31,
    ),
    ("meridian_decoy_emplacement.glb", 436352, 0xf850fe98a4193072),
    (
        "meridian_decoy_emplacement_damaged.glb",
        209980,
        0x67d6d413ae826c04,
    ),
    (
        "meridian_decoy_emplacement_foundation.glb",
        82136,
        0x917eff5f69bf256f,
    ),
    (
        "meridian_decoy_emplacement_heavily_damaged.glb",
        240156,
        0x473b0d1be89fc9d5,
    ),
    (
        "meridian_decoy_emplacement_scaffolding.glb",
        175772,
        0x92017632e9eade29,
    ),
    ("meridian_defence_tower.glb", 368804, 0x9150cd20bfc0f17f),
    (
        "meridian_defence_tower_damaged.glb",
        155752,
        0x543b54ff3f6601c3,
    ),
    (
        "meridian_defence_tower_foundation.glb",
        82168,
        0xd64a206f0e16839d,
    ),
    (
        "meridian_defence_tower_heavily_damaged.glb",
        182940,
        0x0603479ad95c30b3,
    ),
    (
        "meridian_defence_tower_scaffolding.glb",
        114360,
        0xccd238514d94ad0f,
    ),
    ("meridian_harvester.glb", 290264, 0x32a6a2485ae46b55),
    ("meridian_light_tank.glb", 281412, 0xf15f5bea148c1775),
    ("meridian_ore_silo.glb", 380516, 0x3b4c137c8ec45183),
    ("meridian_ore_silo_damaged.glb", 210944, 0xea97e950a1d323e5),
    (
        "meridian_ore_silo_foundation.glb",
        88752,
        0xb43eff22f4c6fb35,
    ),
    (
        "meridian_ore_silo_heavily_damaged.glb",
        212448,
        0xa2283b38f401f695,
    ),
    (
        "meridian_ore_silo_scaffolding.glb",
        142684,
        0x680ae2685c5cc8be,
    ),
    ("meridian_power_plant.glb", 400008, 0xe98d486d15a01951),
    (
        "meridian_power_plant_damaged.glb",
        309500,
        0x85034bf3c1f96d98,
    ),
    (
        "meridian_power_plant_foundation.glb",
        93540,
        0x14dd5123202e7763,
    ),
    (
        "meridian_power_plant_heavily_damaged.glb",
        212572,
        0x7fbdcbccfcecc137,
    ),
    (
        "meridian_power_plant_scaffolding.glb",
        194192,
        0x74210cf1115fa799,
    ),
    ("meridian_radar.glb", 376508, 0x042b4a4b4d701186),
    ("meridian_radar_damaged.glb", 184096, 0x2cdf9b28315afa9e),
    ("meridian_radar_foundation.glb", 88740, 0x0b678cae7c69465c),
    (
        "meridian_radar_heavily_damaged.glb",
        144796,
        0xa79aace6ee61d310,
    ),
    ("meridian_radar_scaffolding.glb", 193024, 0x5e0edfa9072e5028),
    ("meridian_refinery.glb", 528924, 0x2646912c3b362c51),
    ("meridian_refinery_damaged.glb", 402484, 0x701ccd7fb0936153),
    (
        "meridian_refinery_foundation.glb",
        107464,
        0xe8d8e48b415dbadf,
    ),
    (
        "meridian_refinery_heavily_damaged.glb",
        300440,
        0x168d21292a935730,
    ),
    (
        "meridian_refinery_scaffolding.glb",
        237840,
        0x16c67530d6e3c79d,
    ),
    ("meridian_shipyard.glb", 551080, 0x0076b5041ab0563a),
    ("meridian_shipyard_damaged.glb", 406272, 0xa680336609c3db01),
    (
        "meridian_shipyard_foundation.glb",
        119544,
        0x1cdb52624f9d7156,
    ),
    (
        "meridian_shipyard_heavily_damaged.glb",
        301956,
        0x72b5095c4e9f9793,
    ),
    (
        "meridian_shipyard_scaffolding.glb",
        280832,
        0x4cf1b277f578852e,
    ),
    ("meridian_vehicle_factory.glb", 409784, 0x7298f0c9e987e175),
    (
        "meridian_vehicle_factory_damaged.glb",
        296972,
        0xcc0b15d52e189e91,
    ),
    (
        "meridian_vehicle_factory_foundation.glb",
        107484,
        0x45a0253f16f4b5fb,
    ),
    (
        "meridian_vehicle_factory_heavily_damaged.glb",
        239396,
        0x7e1ab46107addf97,
    ),
    (
        "meridian_vehicle_factory_scaffolding.glb",
        253960,
        0xa184882f970b60dc,
    ),
    ("network_automated_gun.glb", 181560, 0x00cfff17db645608),
    (
        "network_automated_gun_disabled.glb",
        181588,
        0x87fca40c6c808049,
    ),
    (
        "network_automated_gun_rubble.glb",
        84988,
        0xee2a90825ebb91d7,
    ),
    ("network_unmanned_artillery.glb", 316540, 0x32f5c81f9f692cc3),
    (
        "network_unmanned_artillery_no_uplink.glb",
        205680,
        0xc2e46797c1351835,
    ),
    (
        "network_unmanned_artillery_rubble.glb",
        182332,
        0x77c51bae96a1ddc4,
    ),
    ("neutral_archive_building.glb", 275476, 0xf859f07fccae9d46),
    (
        "neutral_archive_building_damaged.glb",
        183752,
        0xeb3d078430c5045f,
    ),
    (
        "neutral_archive_building_heavily_damaged.glb",
        120112,
        0xd2979250997e6678,
    ),
    ("neutral_bridge_segment.glb", 148736, 0xdcabb485a48318ac),
    (
        "neutral_bridge_segment_damaged.glb",
        93528,
        0x90c5a0864193a4ac,
    ),
    (
        "neutral_bridge_segment_destroyed.glb",
        87276,
        0xb4cf06ea3f9094f4,
    ),
    ("neutral_evidence_crate.glb", 32088, 0x7f2ae9874b1eb80c),
    ("neutral_fuel_depot.glb", 262136, 0x6f6af91482d944f6),
    (
        "neutral_fuel_depot_burnt_out.glb",
        144748,
        0xb7dc3dce25655438,
    ),
    ("neutral_fuel_depot_damaged.glb", 209840, 0x81e80bf862f54fd1),
    ("neutral_generator.glb", 154904, 0xc51e66298fd717af),
    ("neutral_generator_off.glb", 153636, 0x8a099881dc2a2183),
    ("neutral_generator_rubble.glb", 69424, 0xdabb6a1e397af5f0),
    ("neutral_rail_spur.glb", 146276, 0xd604879033371aac),
    ("neutral_rail_spur_destroyed.glb", 57652, 0x261e0c1d1ae859d1),
    ("neutral_relay_transmitter.glb", 286012, 0x25661e75281a15e0),
    (
        "neutral_relay_transmitter_damaged.glb",
        230172,
        0x675a5700cd3b9c22,
    ),
    (
        "neutral_relay_transmitter_rubble.glb",
        112372,
        0xf45078a873b58f5e,
    ),
    ("neutral_rubble_1x1.glb", 32184, 0xd5874474ab7a4fe0),
    ("neutral_rubble_2x2.glb", 101680, 0xbf126b772060e0d9),
    ("neutral_rubble_2x3.glb", 133436, 0xbc8b1935696f9016),
    ("neutral_rubble_3x4.glb", 143892, 0xa04a6da3a48af561),
    ("neutral_rubble_4x4.glb", 143896, 0xf3d0306c44790346),
    ("neutral_rubble_4x5.glb", 143888, 0xdf02734d5a8487b4),
    ("veyran_advanced_defences.glb", 411472, 0x2f0d5047a68424b5),
    (
        "veyran_advanced_defences_damaged.glb",
        242372,
        0x73e70aa919605eef,
    ),
    (
        "veyran_advanced_defences_foundation.glb",
        88756,
        0xf2bed7ebe4cd005c,
    ),
    (
        "veyran_advanced_defences_heavily_damaged.glb",
        259132,
        0xa0ac0a7f69e366b2,
    ),
    (
        "veyran_advanced_defences_scaffolding.glb",
        169384,
        0xa9bd275ad957122f,
    ),
    ("veyran_barracks.glb", 398852, 0x98e73b5876532728),
    ("veyran_barracks_damaged.glb", 174524, 0xb13c5e99f88d235e),
    ("veyran_barracks_foundation.glb", 93584, 0xce279591aac115bd),
    (
        "veyran_barracks_heavily_damaged.glb",
        187476,
        0xd557a7c4c66af789,
    ),
    (
        "veyran_barracks_scaffolding.glb",
        191256,
        0x2cf911ee8489c848,
    ),
    ("veyran_construction_center.glb", 526384, 0x2bd97f5e21d90a92),
    (
        "veyran_construction_center_damaged.glb",
        392128,
        0x03fd4dc79fc9180d,
    ),
    (
        "veyran_construction_center_foundation.glb",
        114120,
        0x7dbeb558069f1cab,
    ),
    (
        "veyran_construction_center_heavily_damaged.glb",
        327524,
        0xbc91bf31e34ce27d,
    ),
    (
        "veyran_construction_center_scaffolding.glb",
        236228,
        0x89909a192f1494fb,
    ),
    ("veyran_defence_tower.glb", 371336, 0xe1b548638929ae3d),
    (
        "veyran_defence_tower_damaged.glb",
        235164,
        0x90af6b611a0c3607,
    ),
    (
        "veyran_defence_tower_foundation.glb",
        82176,
        0x8883c4e061795db6,
    ),
    (
        "veyran_defence_tower_heavily_damaged.glb",
        214988,
        0xffcc6483c391d953,
    ),
    (
        "veyran_defence_tower_scaffolding.glb",
        148564,
        0xfa38cf1fff6c08be,
    ),
    ("veyran_fortified_bunker.glb", 361356, 0x051f4c911b1a745f),
    (
        "veyran_fortified_bunker_damaged.glb",
        158396,
        0x6c10547f098b6e27,
    ),
    (
        "veyran_fortified_bunker_foundation.glb",
        88756,
        0x3a8d22b7082aa7bf,
    ),
    (
        "veyran_fortified_bunker_heavily_damaged.glb",
        177532,
        0xbc5a122163a2cd99,
    ),
    (
        "veyran_fortified_bunker_scaffolding.glb",
        203760,
        0xa0963ccb6cebcf81,
    ),
    ("veyran_light_tank.glb", 270056, 0x82087dce651c6037),
    ("veyran_ore_silo.glb", 375396, 0xd0765d6b23f84a89),
    ("veyran_ore_silo_damaged.glb", 212528, 0xd2587e9901e11667),
    ("veyran_ore_silo_foundation.glb", 88748, 0x0377c3825db3f9a9),
    (
        "veyran_ore_silo_heavily_damaged.glb",
        211200,
        0xe992938a6b40c197,
    ),
    (
        "veyran_ore_silo_scaffolding.glb",
        139032,
        0x327c6833d6f9396a,
    ),
    ("veyran_power_plant.glb", 496380, 0x61abee99b9f63680),
    ("veyran_power_plant_damaged.glb", 395996, 0x53c1cdc36cbc6e86),
    (
        "veyran_power_plant_foundation.glb",
        93536,
        0xbf1266a4d049297e,
    ),
    (
        "veyran_power_plant_heavily_damaged.glb",
        256580,
        0x8d0532bec7910dae,
    ),
    (
        "veyran_power_plant_scaffolding.glb",
        201940,
        0xaea8673ee2dd1c11,
    ),
    ("veyran_radar.glb", 388524, 0xbe1362130cdb780b),
    ("veyran_radar_damaged.glb", 233460, 0x4d5d3a252804c62d),
    ("veyran_radar_foundation.glb", 88736, 0xd085bbc9d0aa60b0),
    (
        "veyran_radar_heavily_damaged.glb",
        157712,
        0x59b8ff33f64df0b1,
    ),
    ("veyran_radar_scaffolding.glb", 210852, 0xaa78c4d58a3b49fd),
    ("veyran_refinery.glb", 495868, 0xb37ef58ecfa50f9c),
    ("veyran_refinery_damaged.glb", 363884, 0xed0adc5681ad8e9b),
    ("veyran_refinery_foundation.glb", 107464, 0x673e55d5d2962285),
    (
        "veyran_refinery_heavily_damaged.glb",
        299788,
        0xe7379320e0c54b3d,
    ),
    (
        "veyran_refinery_scaffolding.glb",
        244056,
        0xb17afa46a4c45e09,
    ),
    ("veyran_shipyard.glb", 505748, 0x2885f91af578eb4a),
    ("veyran_shipyard_damaged.glb", 363492, 0x67adcde365fee13f),
    ("veyran_shipyard_foundation.glb", 119540, 0x7582edfae84f9452),
    (
        "veyran_shipyard_heavily_damaged.glb",
        304036,
        0xa0ae1136519eed7f,
    ),
    (
        "veyran_shipyard_scaffolding.glb",
        283384,
        0x11abc92e9a4de035,
    ),
    ("veyran_vehicle_factory.glb", 415336, 0xbc4266f57880827c),
    (
        "veyran_vehicle_factory_damaged.glb",
        269260,
        0x19c2f19243dd0ab4,
    ),
    (
        "veyran_vehicle_factory_foundation.glb",
        107480,
        0x6b1b4d23ae5621e9,
    ),
    (
        "veyran_vehicle_factory_heavily_damaged.glb",
        249800,
        0x9dec84a6efe605c2,
    ),
    (
        "veyran_vehicle_factory_scaffolding.glb",
        258776,
        0x0683791e0963e9ce,
    ),
];

/// The units of WP11.6b (vehicles, vessels, the heavy gun team), pinned the same way; see
// the first table for how to re-pin. Sorted by file name.
const PINNED_UNITS: [(&str, usize, u64); 14] = [
    (
        "meridian_amphibious_transport.glb",
        237748,
        0x048d4027b4414d47,
    ),
    ("meridian_heavy_tank.glb", 312352, 0x8d40857fe9d8a2e1),
    ("meridian_light_transport.glb", 218072, 0x231943ce91240dba),
    ("meridian_repair_truck.glb", 242956, 0x5ac71e618e47ddb4),
    ("meridian_siege_artillery.glb", 308432, 0x82b656dc19fd072c),
    ("neutral_ferry.glb", 208644, 0xd7dda0c4dde16e42),
    ("neutral_supply_truck.glb", 287888, 0xf75227bb06af2425),
    ("veyran_gunboat.glb", 287436, 0x51c6810fe3d17d9c),
    ("veyran_harvester.glb", 267916, 0x94e7fdaf21cfee19),
    ("veyran_heavy_gun_team.glb", 141092, 0x48e68db040b26813),
    ("veyran_heavy_tank.glb", 325092, 0x429ac73782fec4b1),
    ("veyran_repair_truck.glb", 240216, 0x35c6bbf8bcd556ee),
    ("veyran_siege_artillery.glb", 307484, 0x927431b76166dea2),
    ("veyran_transport_truck.glb", 224936, 0xf5f055a5660620cb),
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
