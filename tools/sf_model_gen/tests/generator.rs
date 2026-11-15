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
        120436,
        0xff9ce653b2cbcb1e,
    ),
    (
        "meridian_advanced_defences_heavily_damaged.glb",
        257288,
        0x7124623e2f1b0446,
    ),
    (
        "meridian_advanced_defences_scaffolding.glb",
        194192,
        0xd8e5273a325c371a,
    ),
    ("meridian_barracks.glb", 400680, 0x40158c4754768874),
    ("meridian_barracks_damaged.glb", 200612, 0xfb2077770d257b7e),
    (
        "meridian_barracks_foundation.glb",
        120932,
        0xf1b0eb5a9d1de6b4,
    ),
    (
        "meridian_barracks_heavily_damaged.glb",
        188828,
        0x58cfc59ea3fff546,
    ),
    (
        "meridian_barracks_scaffolding.glb",
        182788,
        0x9e1f173ac0dd32a2,
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
        123436,
        0x9d5959ef75b4774b,
    ),
    (
        "meridian_construction_center_heavily_damaged.glb",
        261612,
        0x8518f0e872995521,
    ),
    (
        "meridian_construction_center_scaffolding.glb",
        250792,
        0xa7ad0207466b6559,
    ),
    ("meridian_decoy_emplacement.glb", 435896, 0x8f0d07d415a043ee),
    (
        "meridian_decoy_emplacement_damaged.glb",
        207248,
        0xf6a16706cc0e591d,
    ),
    (
        "meridian_decoy_emplacement_foundation.glb",
        119976,
        0x923b9d592a6b0f62,
    ),
    (
        "meridian_decoy_emplacement_heavily_damaged.glb",
        229212,
        0x1a1845660f1102e4,
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
        120008,
        0x3d0a3155685c3e1f,
    ),
    (
        "meridian_defence_tower_heavily_damaged.glb",
        189100,
        0x8505d1b4666a2cc4,
    ),
    (
        "meridian_defence_tower_scaffolding.glb",
        107964,
        0x73f62dd3431d9706,
    ),
    ("meridian_harvester.glb", 251960, 0xfdc6d76c5edada0d),
    ("meridian_light_tank.glb", 242188, 0xa78c91f749af33da),
    ("meridian_ore_silo.glb", 380744, 0xe5b00b983cb5c144),
    ("meridian_ore_silo_damaged.glb", 206044, 0x0b32116de47cecf2),
    (
        "meridian_ore_silo_foundation.glb",
        120428,
        0x1e60406bc0c0acc1,
    ),
    (
        "meridian_ore_silo_heavily_damaged.glb",
        212572,
        0x28dc603aa41ad9f0,
    ),
    (
        "meridian_ore_silo_scaffolding.glb",
        137208,
        0x3e4f40a513ce1cbd,
    ),
    ("meridian_power_plant.glb", 400008, 0x4446ef96fb135c58),
    (
        "meridian_power_plant_damaged.glb",
        300380,
        0x55477fb4a45ee497,
    ),
    (
        "meridian_power_plant_foundation.glb",
        120884,
        0x18e2fd369b94d675,
    ),
    (
        "meridian_power_plant_heavily_damaged.glb",
        206876,
        0x714361ca8e8c06c7,
    ),
    (
        "meridian_power_plant_scaffolding.glb",
        182332,
        0x10df36914238b1da,
    ),
    ("meridian_radar.glb", 376052, 0xbd6463999840423e),
    ("meridian_radar_damaged.glb", 176800, 0xbfdeb86e7cd573a2),
    ("meridian_radar_foundation.glb", 120416, 0x209d993e86330f7c),
    (
        "meridian_radar_heavily_damaged.glb",
        141832,
        0x68c7f3b68137c1bf,
    ),
    ("meridian_radar_scaffolding.glb", 192340, 0xbf46f0ff197c5dd2),
    ("meridian_refinery.glb", 522540, 0x77a1dc82be4b49ec),
    ("meridian_refinery_damaged.glb", 396100, 0xf4cabccd34c02c81),
    (
        "meridian_refinery_foundation.glb",
        121336,
        0xe93c5b943ea7c9a0,
    ),
    (
        "meridian_refinery_heavily_damaged.glb",
        299068,
        0x8fd6c7bee045d66c,
    ),
    (
        "meridian_refinery_scaffolding.glb",
        226436,
        0x603f078b6b1ce524,
    ),
    ("meridian_shipyard.glb", 478568, 0xd66cbabc1a0347db),
    ("meridian_shipyard_damaged.glb", 355652, 0x7ce64a8e55fda190),
    (
        "meridian_shipyard_foundation.glb",
        123840,
        0xa772d07a3bf3407d,
    ),
    (
        "meridian_shipyard_heavily_damaged.glb",
        284168,
        0xb401b05d20833804,
    ),
    (
        "meridian_shipyard_scaffolding.glb",
        258712,
        0x0fd181c02c94bab8,
    ),
    ("meridian_vehicle_factory.glb", 410696, 0xb0c77f7186429420),
    (
        "meridian_vehicle_factory_damaged.glb",
        267328,
        0xda69dc9abb18cad2,
    ),
    (
        "meridian_vehicle_factory_foundation.glb",
        121352,
        0xc51b820487e65211,
    ),
    (
        "meridian_vehicle_factory_heavily_damaged.glb",
        231640,
        0x827e39528049cc14,
    ),
    (
        "meridian_vehicle_factory_scaffolding.glb",
        235028,
        0xb7d12f5d358ea560,
    ),
    ("network_automated_gun.glb", 181108, 0x2b2c22d4cc261db2),
    (
        "network_automated_gun_disabled.glb",
        181136,
        0x8150ec2caf3697fa,
    ),
    (
        "network_automated_gun_rubble.glb",
        83620,
        0xedf08d222818e092,
    ),
    ("network_unmanned_artillery.glb", 315844, 0xbbea3d9e1a6f30d4),
    (
        "network_unmanned_artillery_no_uplink.glb",
        204992,
        0xff5aa0384e17d6bd,
    ),
    (
        "network_unmanned_artillery_rubble.glb",
        182224,
        0x89140b88f98a739f,
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
        0x7521ae1e0d2174e5,
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
        245672,
        0x905271194d186ef5,
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
        120432,
        0xc6268f556caa5886,
    ),
    (
        "veyran_advanced_defences_heavily_damaged.glb",
        257996,
        0x081bb6f2e74e9a32,
    ),
    (
        "veyran_advanced_defences_scaffolding.glb",
        165972,
        0x2b6e5ed89c734fca,
    ),
    ("veyran_barracks.glb", 399536, 0xc27b3381af6f0336),
    ("veyran_barracks_damaged.glb", 167684, 0x6a24a77e34823636),
    ("veyran_barracks_foundation.glb", 120928, 0x6c8e5a9e9595f674),
    (
        "veyran_barracks_heavily_damaged.glb",
        187020,
        0x24de2ab7fa89c771,
    ),
    (
        "veyran_barracks_scaffolding.glb",
        179852,
        0x5bb480b6a9c6cd57,
    ),
    ("veyran_construction_center.glb", 473936, 0xbf8b5f7880446186),
    (
        "veyran_construction_center_damaged.glb",
        340364,
        0x7a8403f244813d0f,
    ),
    (
        "veyran_construction_center_foundation.glb",
        123424,
        0xde869a6d92aa2d8c,
    ),
    (
        "veyran_construction_center_heavily_damaged.glb",
        313268,
        0xef74c9910e46fb5f,
    ),
    (
        "veyran_construction_center_scaffolding.glb",
        215240,
        0x1f2c823ecd3d33b1,
    ),
    ("veyran_defence_tower.glb", 343516, 0xe2b3fb24edd0e4d5),
    (
        "veyran_defence_tower_damaged.glb",
        204144,
        0x20a942114819c9ab,
    ),
    (
        "veyran_defence_tower_foundation.glb",
        120016,
        0xde1d64f00eba5ea6,
    ),
    (
        "veyran_defence_tower_heavily_damaged.glb",
        193544,
        0x759d96da791a0333,
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
        120432,
        0x0483ecc6eb6b814e,
    ),
    (
        "veyran_fortified_bunker_heavily_damaged.glb",
        177084,
        0x46c9205c1e983388,
    ),
    (
        "veyran_fortified_bunker_scaffolding.glb",
        192596,
        0x3fe115d2e09a8406,
    ),
    ("veyran_light_tank.glb", 223532, 0x520461c2447f5b49),
    ("veyran_ore_silo.glb", 376304, 0x92d839a170709e73),
    ("veyran_ore_silo_damaged.glb", 194508, 0x7be1214d50099bfc),
    ("veyran_ore_silo_foundation.glb", 120424, 0xc08b3ae56b55770b),
    (
        "veyran_ore_silo_heavily_damaged.glb",
        200152,
        0xf1a5414e2c5ab0e1,
    ),
    (
        "veyran_ore_silo_scaffolding.glb",
        133792,
        0x728e6ef88c4b0e4a,
    ),
    ("veyran_power_plant.glb", 473580, 0xd0deb10bf3a80a22),
    ("veyran_power_plant_damaged.glb", 374676, 0x66382552af7fb3d3),
    (
        "veyran_power_plant_foundation.glb",
        120880,
        0x84af8195996edbec,
    ),
    (
        "veyran_power_plant_heavily_damaged.glb",
        240048,
        0x9a6c9e78c5de7f43,
    ),
    (
        "veyran_power_plant_scaffolding.glb",
        190532,
        0x595a9d60bfa70695,
    ),
    ("veyran_radar.glb", 388296, 0xeaa285854203adbf),
    ("veyran_radar_damaged.glb", 233232, 0x480896d6fcab5f56),
    ("veyran_radar_foundation.glb", 120412, 0x544eb62aa83010ca),
    (
        "veyran_radar_heavily_damaged.glb",
        152020,
        0x9085c715cb78ec5e,
    ),
    ("veyran_radar_scaffolding.glb", 211316, 0xb95419eeeb16999c),
    ("veyran_refinery.glb", 459380, 0xcf55f48b59516ff3),
    ("veyran_refinery_damaged.glb", 329220, 0xc800861d804be608),
    ("veyran_refinery_foundation.glb", 121332, 0x298dcfabde25c3fb),
    (
        "veyran_refinery_heavily_damaged.glb",
        289184,
        0x93aef1f0060c8ced,
    ),
    (
        "veyran_refinery_scaffolding.glb",
        222160,
        0x9924800eff8fb117,
    ),
    ("veyran_shipyard.glb", 451476, 0x68cf9c9c079381df),
    ("veyran_shipyard_damaged.glb", 298044, 0xd08844195ae86239),
    ("veyran_shipyard_foundation.glb", 123836, 0x25db31387c7b71e2),
    (
        "veyran_shipyard_heavily_damaged.glb",
        282144,
        0xbffef0b74692aefb,
    ),
    (
        "veyran_shipyard_scaffolding.glb",
        281104,
        0xc23cfb086ddcacba,
    ),
    ("veyran_vehicle_factory.glb", 416244, 0x4b25a30c31da70da),
    (
        "veyran_vehicle_factory_damaged.glb",
        249648,
        0xd99f6223107e6545,
    ),
    (
        "veyran_vehicle_factory_foundation.glb",
        121352,
        0x33ec115dd9dc4c86,
    ),
    (
        "veyran_vehicle_factory_heavily_damaged.glb",
        242272,
        0xdd98fed0ea219bcd,
    ),
    (
        "veyran_vehicle_factory_scaffolding.glb",
        238480,
        0xa9fd6959200c926f,
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
