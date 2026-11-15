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
// 16 segments on round parts in LOD0. Every file moved; with those four kit rules switched back
// the generator writes the old bytes of all 157 files again (checked with `--check` against the
// previous set).
const PINNED: [(&str, usize, u64); 143] = [
    ("meridian_advanced_defences.glb", 456824, 0xb0e314c2c3b77ccc),
    (
        "meridian_advanced_defences_damaged.glb",
        257040,
        0xbf8a4d3e2c99d0c1,
    ),
    (
        "meridian_advanced_defences_foundation.glb",
        114768,
        0xbc49f1a08483f0fc,
    ),
    (
        "meridian_advanced_defences_heavily_damaged.glb",
        265496,
        0xb91f1ee1403fe4ae,
    ),
    (
        "meridian_advanced_defences_scaffolding.glb",
        219516,
        0x95c7d63223debf27,
    ),
    ("meridian_barracks.glb", 400908, 0x20b428c0933c8670),
    ("meridian_barracks_damaged.glb", 203460, 0x33826857d688a147),
    (
        "meridian_barracks_foundation.glb",
        119600,
        0xac420b4245b66ece,
    ),
    (
        "meridian_barracks_heavily_damaged.glb",
        192936,
        0x7509318e635d2c3d,
    ),
    (
        "meridian_barracks_scaffolding.glb",
        208800,
        0x884017daad6b38fb,
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
        140152,
        0xf037da225d08f8f8,
    ),
    (
        "meridian_construction_center_heavily_damaged.glb",
        275296,
        0x883be75a793faf50,
    ),
    (
        "meridian_construction_center_scaffolding.glb",
        280668,
        0x16145f2dcefe6dc5,
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
        266160,
        0xf40ddbe15af3db5b,
    ),
    (
        "meridian_decoy_emplacement_scaffolding.glb",
        189912,
        0x6a70cf3ce7f6ac68,
    ),
    ("meridian_defence_tower.glb", 368804, 0x9150cd20bfc0f17f),
    (
        "meridian_defence_tower_damaged.glb",
        155752,
        0x543b54ff3f6601c3,
    ),
    (
        "meridian_defence_tower_foundation.glb",
        108168,
        0xcbd8686245e9e101,
    ),
    (
        "meridian_defence_tower_heavily_damaged.glb",
        207816,
        0x71f35ff7c2775a11,
    ),
    (
        "meridian_defence_tower_scaffolding.glb",
        151332,
        0xa7c59cfb2c9ee045,
    ),
    ("meridian_harvester.glb", 290264, 0x32a6a2485ae46b55),
    ("meridian_light_tank.glb", 281412, 0xf15f5bea148c1775),
    ("meridian_ore_silo.glb", 380516, 0x3b4c137c8ec45183),
    ("meridian_ore_silo_damaged.glb", 210944, 0xea97e950a1d323e5),
    (
        "meridian_ore_silo_foundation.glb",
        114760,
        0x080c0c94cee2d01d,
    ),
    (
        "meridian_ore_silo_heavily_damaged.glb",
        238452,
        0x7e491f36af4d7b3a,
    ),
    (
        "meridian_ore_silo_scaffolding.glb",
        179636,
        0x8dc46d884c41f925,
    ),
    ("meridian_power_plant.glb", 400008, 0xe98d486d15a01951),
    (
        "meridian_power_plant_damaged.glb",
        309500,
        0x85034bf3c1f96d98,
    ),
    (
        "meridian_power_plant_foundation.glb",
        119552,
        0x2c37ba1dfc9945fd,
    ),
    (
        "meridian_power_plant_heavily_damaged.glb",
        215764,
        0xa4431839c65d2950,
    ),
    (
        "meridian_power_plant_scaffolding.glb",
        208344,
        0xb1dcd4f260b9b14f,
    ),
    ("meridian_radar.glb", 376508, 0x042b4a4b4d701186),
    ("meridian_radar_damaged.glb", 184096, 0x2cdf9b28315afa9e),
    ("meridian_radar_foundation.glb", 114748, 0x499a280e72f39e9c),
    (
        "meridian_radar_heavily_damaged.glb",
        166256,
        0xf83458515bd999b7,
    ),
    ("meridian_radar_scaffolding.glb", 229980, 0xb8d170ec562259c9),
    ("meridian_refinery.glb", 528924, 0x2646912c3b362c51),
    ("meridian_refinery_damaged.glb", 402484, 0x701ccd7fb0936153),
    (
        "meridian_refinery_foundation.glb",
        133488,
        0x6a73fa81984d708b,
    ),
    (
        "meridian_refinery_heavily_damaged.glb",
        303632,
        0xb0c1b0b158ea14a6,
    ),
    (
        "meridian_refinery_scaffolding.glb",
        251976,
        0xc3bddb2779b8bd7c,
    ),
    ("meridian_shipyard.glb", 551080, 0x0076b5041ab0563a),
    ("meridian_shipyard_damaged.glb", 406272, 0xa680336609c3db01),
    (
        "meridian_shipyard_foundation.glb",
        145568,
        0x674255b54f804ff5,
    ),
    (
        "meridian_shipyard_heavily_damaged.glb",
        305156,
        0xb1fb24beca98479c,
    ),
    (
        "meridian_shipyard_scaffolding.glb",
        298388,
        0xbb2eb3191e366c67,
    ),
    ("meridian_vehicle_factory.glb", 409784, 0x7298f0c9e987e175),
    (
        "meridian_vehicle_factory_damaged.glb",
        296972,
        0xcc0b15d52e189e91,
    ),
    (
        "meridian_vehicle_factory_foundation.glb",
        130084,
        0x130ba539670cf07a,
    ),
    (
        "meridian_vehicle_factory_heavily_damaged.glb",
        242588,
        0x60b5df17679fffa4,
    ),
    (
        "meridian_vehicle_factory_scaffolding.glb",
        268096,
        0x9cb6f0f8657b2ebc,
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
        0x7f4b348c889b1cae,
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
        123308,
        0x469349485d5cf92d,
    ),
    ("neutral_bridge_segment.glb", 148736, 0xdcabb485a48318ac),
    (
        "neutral_bridge_segment_damaged.glb",
        93528,
        0x90c5a0864193a4ac,
    ),
    (
        "neutral_bridge_segment_destroyed.glb",
        113288,
        0x906c0ac7b4415f41,
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
    ("neutral_rail_spur_destroyed.glb", 76816, 0xd10b9bfea467e5c1),
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
        114764,
        0x1113cda79ef3b282,
    ),
    (
        "veyran_advanced_defences_heavily_damaged.glb",
        285124,
        0xfaddaf4b08e7a44f,
    ),
    (
        "veyran_advanced_defences_scaffolding.glb",
        206328,
        0xb52035ced358e93d,
    ),
    ("veyran_barracks.glb", 398852, 0x98e73b5876532728),
    ("veyran_barracks_damaged.glb", 174524, 0xb13c5e99f88d235e),
    ("veyran_barracks_foundation.glb", 119596, 0x0338b08ce2abed68),
    (
        "veyran_barracks_heavily_damaged.glb",
        190672,
        0xea689f084bfcd4a4,
    ),
    (
        "veyran_barracks_scaffolding.glb",
        205400,
        0x0864c6c86514d385,
    ),
    ("veyran_construction_center.glb", 526384, 0x2bd97f5e21d90a92),
    (
        "veyran_construction_center_damaged.glb",
        392128,
        0x03fd4dc79fc9180d,
    ),
    (
        "veyran_construction_center_foundation.glb",
        140144,
        0xad2ac2e4e2f82b52,
    ),
    (
        "veyran_construction_center_heavily_damaged.glb",
        330720,
        0xea632c3640053b59,
    ),
    (
        "veyran_construction_center_scaffolding.glb",
        257204,
        0xaf22ffbf4aec7f3f,
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
        218188,
        0x7c7caea46936cfa0,
    ),
    (
        "veyran_defence_tower_scaffolding.glb",
        162716,
        0x10a13c569c685130,
    ),
    ("veyran_fortified_bunker.glb", 361356, 0x051f4c911b1a745f),
    (
        "veyran_fortified_bunker_damaged.glb",
        158396,
        0x6c10547f098b6e27,
    ),
    (
        "veyran_fortified_bunker_foundation.glb",
        114764,
        0x0edae01bb9481f5d,
    ),
    (
        "veyran_fortified_bunker_heavily_damaged.glb",
        180728,
        0xd78ac913134fb828,
    ),
    (
        "veyran_fortified_bunker_scaffolding.glb",
        217904,
        0x0c72745a40583c7b,
    ),
    ("veyran_light_tank.glb", 270056, 0x82087dce651c6037),
    ("veyran_ore_silo.glb", 375396, 0xd0765d6b23f84a89),
    ("veyran_ore_silo_damaged.glb", 212528, 0xd2587e9901e11667),
    ("veyran_ore_silo_foundation.glb", 114756, 0xa83c2a7547d5f689),
    (
        "veyran_ore_silo_heavily_damaged.glb",
        237204,
        0x628f39db8cff9f09,
    ),
    (
        "veyran_ore_silo_scaffolding.glb",
        172564,
        0x0a9df77ab04b4dd0,
    ),
    ("veyran_power_plant.glb", 496380, 0x61abee99b9f63680),
    ("veyran_power_plant_damaged.glb", 395996, 0x53c1cdc36cbc6e86),
    (
        "veyran_power_plant_foundation.glb",
        119548,
        0xe94ea92391186cd2,
    ),
    (
        "veyran_power_plant_heavily_damaged.glb",
        259780,
        0x69743a77a4394b5d,
    ),
    (
        "veyran_power_plant_scaffolding.glb",
        216076,
        0x9e4e6a590300b679,
    ),
    ("veyran_radar.glb", 388524, 0xbe1362130cdb780b),
    ("veyran_radar_damaged.glb", 233460, 0x4d5d3a252804c62d),
    ("veyran_radar_foundation.glb", 114744, 0xf9938001d0ee5684),
    (
        "veyran_radar_heavily_damaged.glb",
        183716,
        0x08ea10612209c9d4,
    ),
    ("veyran_radar_scaffolding.glb", 247800, 0x7521be5be876c09f),
    ("veyran_refinery.glb", 495868, 0xb37ef58ecfa50f9c),
    ("veyran_refinery_damaged.glb", 363884, 0xed0adc5681ad8e9b),
    ("veyran_refinery_foundation.glb", 133488, 0x2f6316069976347a),
    (
        "veyran_refinery_heavily_damaged.glb",
        302980,
        0xfc3150f1321dd7ae,
    ),
    (
        "veyran_refinery_scaffolding.glb",
        263892,
        0xf1b52e9779e0868b,
    ),
    ("veyran_shipyard.glb", 505748, 0x2885f91af578eb4a),
    ("veyran_shipyard_damaged.glb", 363492, 0x67adcde365fee13f),
    ("veyran_shipyard_foundation.glb", 145564, 0xde8cc9f5e640f616),
    (
        "veyran_shipyard_heavily_damaged.glb",
        307232,
        0xc8acfb0a4171150a,
    ),
    (
        "veyran_shipyard_scaffolding.glb",
        320324,
        0xb68dbf23576e1f26,
    ),
    ("veyran_vehicle_factory.glb", 415336, 0xbc4266f57880827c),
    (
        "veyran_vehicle_factory_damaged.glb",
        269260,
        0x19c2f19243dd0ab4,
    ),
    (
        "veyran_vehicle_factory_foundation.glb",
        130080,
        0x3d8690b652d2d772,
    ),
    (
        "veyran_vehicle_factory_heavily_damaged.glb",
        252992,
        0x81cf83866e301a62,
    ),
    (
        "veyran_vehicle_factory_scaffolding.glb",
        278616,
        0x3d628c9cce52570d,
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
    ("neutral_ferry.glb", 206228, 0x32d645bbcbb1fc9b),
    ("neutral_supply_truck.glb", 287888, 0xf75227bb06af2425),
    ("veyran_gunboat.glb", 286288, 0xe8a1be41d0ef51ee),
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
