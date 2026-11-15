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
const PINNED: [(&str, usize, u64); 143] = [
    ("meridian_advanced_defences.glb", 457052, 0x0c0dc29c9e944e34),
    (
        "meridian_advanced_defences_damaged.glb",
        251568,
        0x2d64ee101a6cfd2c,
    ),
    (
        "meridian_advanced_defences_foundation.glb",
        88760,
        0x11938c31bfa6b0b0,
    ),
    (
        "meridian_advanced_defences_heavily_damaged.glb",
        256832,
        0xaa645d7e4c65bb4b,
    ),
    (
        "meridian_advanced_defences_scaffolding.glb",
        194420,
        0x98b645263feef906,
    ),
    ("meridian_barracks.glb", 400680, 0x40158c4754768874),
    ("meridian_barracks_damaged.glb", 199928, 0xbd3c180f0fab4dce),
    (
        "meridian_barracks_foundation.glb",
        93588,
        0x8f3e7b2ac72fa18f,
    ),
    (
        "meridian_barracks_heavily_damaged.glb",
        189740,
        0x0d41138aa577b697,
    ),
    (
        "meridian_barracks_scaffolding.glb",
        183700,
        0xb998cfbcc1b3c913,
    ),
    (
        "meridian_construction_center.glb",
        519268,
        0x703bf9b24e8e6b0f,
    ),
    (
        "meridian_construction_center_damaged.glb",
        407004,
        0xbf3b5e310f84b414,
    ),
    (
        "meridian_construction_center_foundation.glb",
        114132,
        0xa03b3a1331397263,
    ),
    (
        "meridian_construction_center_heavily_damaged.glb",
        262756,
        0x2d2f551381d13c41,
    ),
    (
        "meridian_construction_center_scaffolding.glb",
        251936,
        0xdebb2a373942da9c,
    ),
    ("meridian_decoy_emplacement.glb", 435896, 0x8f0d07d415a043ee),
    (
        "meridian_decoy_emplacement_damaged.glb",
        207244,
        0xbe57683daf8f1557,
    ),
    (
        "meridian_decoy_emplacement_foundation.glb",
        82136,
        0xa7d246d0ceec48a0,
    ),
    (
        "meridian_decoy_emplacement_heavily_damaged.glb",
        239700,
        0x778939e944224459,
    ),
    (
        "meridian_decoy_emplacement_scaffolding.glb",
        164824,
        0x2ffe0ec1b4ae1739,
    ),
    ("meridian_defence_tower.glb", 369488, 0xb6c78b3844246548),
    (
        "meridian_defence_tower_damaged.glb",
        153016,
        0xb2523f3d765cfaa3,
    ),
    (
        "meridian_defence_tower_foundation.glb",
        82168,
        0xad720c8aa0259a94,
    ),
    (
        "meridian_defence_tower_heavily_damaged.glb",
        182488,
        0xfd56b517d123418a,
    ),
    (
        "meridian_defence_tower_scaffolding.glb",
        107964,
        0x73f62dd3431d9706,
    ),
    ("meridian_harvester.glb", 251960, 0xfdc6d76c5edada0d),
    ("meridian_light_tank.glb", 242188, 0xa78c91f749af33da),
    ("meridian_ore_silo.glb", 380744, 0xe5b00b983cb5c144),
    ("meridian_ore_silo_damaged.glb", 205476, 0x007278d49b1443e5),
    (
        "meridian_ore_silo_foundation.glb",
        88752,
        0x3029ffa49705e0df,
    ),
    (
        "meridian_ore_silo_heavily_damaged.glb",
        212564,
        0x5684da87c8e1fc8f,
    ),
    (
        "meridian_ore_silo_scaffolding.glb",
        137436,
        0x4307c367904fb975,
    ),
    ("meridian_power_plant.glb", 400008, 0x4446ef96fb135c58),
    (
        "meridian_power_plant_damaged.glb",
        300380,
        0x455285003d68da89,
    ),
    (
        "meridian_power_plant_foundation.glb",
        93540,
        0x5be3273e1507bc72,
    ),
    (
        "meridian_power_plant_heavily_damaged.glb",
        207556,
        0x81733187675b1fb4,
    ),
    (
        "meridian_power_plant_scaffolding.glb",
        183244,
        0x49158cea21fca87d,
    ),
    ("meridian_radar.glb", 376052, 0xbd6463999840423e),
    ("meridian_radar_damaged.glb", 176800, 0xbfdeb86e7cd573a2),
    ("meridian_radar_foundation.glb", 88740, 0xb7e59f74c57ed46a),
    (
        "meridian_radar_heavily_damaged.glb",
        142060,
        0xcd7ed6164d203c69,
    ),
    ("meridian_radar_scaffolding.glb", 192340, 0x1fcbd9d525bbd9f5),
    ("meridian_refinery.glb", 522540, 0x77a1dc82be4b49ec),
    ("meridian_refinery_damaged.glb", 396100, 0xf4cabccd34c02c81),
    (
        "meridian_refinery_foundation.glb",
        107464,
        0x844663266279c842,
    ),
    (
        "meridian_refinery_heavily_damaged.glb",
        299296,
        0xe36a7c7a5384ea25,
    ),
    (
        "meridian_refinery_scaffolding.glb",
        226892,
        0x1463928a9b1116f9,
    ),
    ("meridian_shipyard.glb", 478568, 0xd66cbabc1a0347db),
    ("meridian_shipyard_damaged.glb", 355652, 0x7ce64a8e55fda190),
    (
        "meridian_shipyard_foundation.glb",
        119544,
        0xbf4edae0b12383df,
    ),
    (
        "meridian_shipyard_heavily_damaged.glb",
        284168,
        0xb401b05d20833804,
    ),
    (
        "meridian_shipyard_scaffolding.glb",
        261224,
        0xedad449fa565f9cd,
    ),
    ("meridian_vehicle_factory.glb", 410696, 0xb0c77f7186429420),
    (
        "meridian_vehicle_factory_damaged.glb",
        266528,
        0xc5e179bd9110b4cf,
    ),
    (
        "meridian_vehicle_factory_foundation.glb",
        107484,
        0xda6810797bf12ece,
    ),
    (
        "meridian_vehicle_factory_heavily_damaged.glb",
        232096,
        0x4e13a86f400c8c85,
    ),
    (
        "meridian_vehicle_factory_scaffolding.glb",
        235712,
        0xc9172b826fb01c4c,
    ),
    ("network_automated_gun.glb", 181108, 0x2b2c22d4cc261db2),
    (
        "network_automated_gun_disabled.glb",
        181136,
        0x8150ec2caf3697fa,
    ),
    (
        "network_automated_gun_rubble.glb",
        84988,
        0x50a73a36bac51a75,
    ),
    ("network_unmanned_artillery.glb", 315844, 0xbbea3d9e1a6f30d4),
    (
        "network_unmanned_artillery_no_uplink.glb",
        204992,
        0xff5aa0384e17d6bd,
    ),
    (
        "network_unmanned_artillery_rubble.glb",
        182216,
        0xa9a0da746d4dbbb3,
    ),
    ("neutral_archive_building.glb", 275476, 0xb8be02c0f8602a58),
    (
        "neutral_archive_building_damaged.glb",
        183752,
        0xa032ee01529ef9ea,
    ),
    (
        "neutral_archive_building_heavily_damaged.glb",
        120108,
        0xe62b437a5e61bc63,
    ),
    ("neutral_bridge_segment.glb", 148280, 0x54ddf03d83f7b952),
    (
        "neutral_bridge_segment_damaged.glb",
        94216,
        0x9d472ffc80663fec,
    ),
    (
        "neutral_bridge_segment_destroyed.glb",
        87732,
        0x6ffc8d03485f90e5,
    ),
    ("neutral_evidence_crate.glb", 32088, 0x2b4502f209c5868b),
    ("neutral_fuel_depot.glb", 248452, 0x21e728be44db6684),
    (
        "neutral_fuel_depot_burnt_out.glb",
        131292,
        0x635e95f48075b959,
    ),
    ("neutral_fuel_depot_damaged.glb", 197752, 0xe84b800096ff232a),
    ("neutral_generator.glb", 154904, 0xe29ff62acdcaaf95),
    ("neutral_generator_off.glb", 153640, 0x261ecfdcf4f01c6a),
    ("neutral_generator_rubble.glb", 70444, 0xbb5175cf289f761b),
    ("neutral_rail_spur.glb", 145356, 0x732a769f53d7bd1c),
    ("neutral_rail_spur_destroyed.glb", 57652, 0xc3df7550f4fdf297),
    ("neutral_relay_transmitter.glb", 286012, 0x6b0be1bdab50e282),
    (
        "neutral_relay_transmitter_damaged.glb",
        230172,
        0xea6407cbb08f323a,
    ),
    (
        "neutral_relay_transmitter_rubble.glb",
        112372,
        0x9d785f8eaa353bc7,
    ),
    ("neutral_rubble_1x1.glb", 32184, 0xadda98149b262038),
    ("neutral_rubble_2x2.glb", 101680, 0x4626359a143ad08f),
    ("neutral_rubble_2x3.glb", 133436, 0x82a8bf1603240835),
    ("neutral_rubble_3x4.glb", 143892, 0xf5e88eee7cb85278),
    ("neutral_rubble_4x4.glb", 143896, 0x41eadddf946c0b9f),
    ("neutral_rubble_4x5.glb", 143888, 0x035cea4bdbf4bbae),
    ("veyran_advanced_defences.glb", 411700, 0xe38c652a55a83052),
    (
        "veyran_advanced_defences_damaged.glb",
        242828,
        0x95cb6cda77aa7d7b,
    ),
    (
        "veyran_advanced_defences_foundation.glb",
        88756,
        0xe1a8299803d8aa15,
    ),
    (
        "veyran_advanced_defences_heavily_damaged.glb",
        259364,
        0x37425fbd137733d3,
    ),
    (
        "veyran_advanced_defences_scaffolding.glb",
        168704,
        0x3dc2e8a9e5c3ba81,
    ),
    ("veyran_barracks.glb", 399536, 0xc27b3381af6f0336),
    ("veyran_barracks_damaged.glb", 167684, 0x6a24a77e34823636),
    ("veyran_barracks_foundation.glb", 93584, 0x4728b4b206e41289),
    (
        "veyran_barracks_heavily_damaged.glb",
        187704,
        0x0191c3a6576dfa7d,
    ),
    (
        "veyran_barracks_scaffolding.glb",
        180304,
        0x5c8a0a26efe045e7,
    ),
    ("veyran_construction_center.glb", 473936, 0xbf8b5f7880446186),
    (
        "veyran_construction_center_damaged.glb",
        340364,
        0x7a8403f244813d0f,
    ),
    (
        "veyran_construction_center_foundation.glb",
        114120,
        0xbf3b4ac784642e75,
    ),
    (
        "veyran_construction_center_heavily_damaged.glb",
        313728,
        0x78db4e13bc70c901,
    ),
    (
        "veyran_construction_center_scaffolding.glb",
        220720,
        0x868b2662937f06db,
    ),
    ("veyran_defence_tower.glb", 343516, 0xe2b3fb24edd0e4d5),
    (
        "veyran_defence_tower_damaged.glb",
        207348,
        0x0d5ee2c6d4a58736,
    ),
    (
        "veyran_defence_tower_foundation.glb",
        82176,
        0x700ef19c7c38b1fd,
    ),
    (
        "veyran_defence_tower_heavily_damaged.glb",
        194460,
        0xe5a705dc2a558aff,
    ),
    (
        "veyran_defence_tower_scaffolding.glb",
        123004,
        0x402ad28e65c9a5da,
    ),
    ("veyran_fortified_bunker.glb", 361584, 0x12c917903a222eb2),
    (
        "veyran_fortified_bunker_damaged.glb",
        155204,
        0x023c8c37e3cc297d,
    ),
    (
        "veyran_fortified_bunker_foundation.glb",
        88756,
        0x095d8924d3507709,
    ),
    (
        "veyran_fortified_bunker_heavily_damaged.glb",
        177532,
        0xb0bb22abafc86f21,
    ),
    (
        "veyran_fortified_bunker_scaffolding.glb",
        192816,
        0xd1bdde02bd8ee19d,
    ),
    ("veyran_light_tank.glb", 223532, 0x520461c2447f5b49),
    ("veyran_ore_silo.glb", 376304, 0x92d839a170709e73),
    ("veyran_ore_silo_damaged.glb", 191428, 0x0dc14d1a4ec5bfee),
    ("veyran_ore_silo_foundation.glb", 88748, 0xd4e328911a0b7000),
    (
        "veyran_ore_silo_heavily_damaged.glb",
        210632,
        0x2ea33fc2e268f27e,
    ),
    (
        "veyran_ore_silo_scaffolding.glb",
        135152,
        0xde694bb7b3f1820e,
    ),
    ("veyran_power_plant.glb", 473580, 0xd0deb10bf3a80a22),
    ("veyran_power_plant_damaged.glb", 373196, 0x62e02f49e5cdaaf0),
    (
        "veyran_power_plant_foundation.glb",
        93536,
        0x4c79cc98696e38e4,
    ),
    (
        "veyran_power_plant_heavily_damaged.glb",
        240504,
        0x1c9001da5aef4726,
    ),
    (
        "veyran_power_plant_scaffolding.glb",
        190992,
        0xb40e956177d79bfc,
    ),
    ("veyran_radar.glb", 388296, 0xeaa285854203adbf),
    ("veyran_radar_damaged.glb", 233232, 0x51b596eaefb7f0cf),
    ("veyran_radar_foundation.glb", 88736, 0x62ec489e164c1c91),
    (
        "veyran_radar_heavily_damaged.glb",
        157256,
        0x541a0baf37a01f6c,
    ),
    ("veyran_radar_scaffolding.glb", 211308, 0x04d62c4d873b1eeb),
    ("veyran_refinery.glb", 459380, 0xcf55f48b59516ff3),
    ("veyran_refinery_damaged.glb", 329220, 0xc800861d804be608),
    ("veyran_refinery_foundation.glb", 107464, 0x21ba3e3a98597554),
    (
        "veyran_refinery_heavily_damaged.glb",
        289184,
        0x93aef1f0060c8ced,
    ),
    (
        "veyran_refinery_scaffolding.glb",
        226492,
        0xe8fdf68946e3ae15,
    ),
    ("veyran_shipyard.glb", 451476, 0x68cf9c9c079381df),
    ("veyran_shipyard_damaged.glb", 298044, 0xd08844195ae86239),
    ("veyran_shipyard_foundation.glb", 119540, 0x0448b26afe9b4f77),
    (
        "veyran_shipyard_heavily_damaged.glb",
        282144,
        0xbffef0b74692aefb,
    ),
    (
        "veyran_shipyard_scaffolding.glb",
        283612,
        0x795bd43e260b2181,
    ),
    ("veyran_vehicle_factory.glb", 416244, 0x4b25a30c31da70da),
    (
        "veyran_vehicle_factory_damaged.glb",
        249648,
        0xd99f6223107e6545,
    ),
    (
        "veyran_vehicle_factory_foundation.glb",
        107480,
        0x42629cadb7188ec0,
    ),
    (
        "veyran_vehicle_factory_heavily_damaged.glb",
        242504,
        0xb29126d33cdc6201,
    ),
    (
        "veyran_vehicle_factory_scaffolding.glb",
        241444,
        0x90512ce92988ba45,
    ),
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
