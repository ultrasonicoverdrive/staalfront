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
const PINNED: [(&str, usize, u64); 143] = [
    ("meridian_advanced_defences.glb", 457052, 0x0c0dc29c9e944e34),
    (
        "meridian_advanced_defences_damaged.glb",
        497208,
        0x5a766127a9defa9b,
    ),
    (
        "meridian_advanced_defences_foundation.glb",
        180904,
        0xc2e46121bc981209,
    ),
    (
        "meridian_advanced_defences_heavily_damaged.glb",
        544576,
        0x3d2ef08476593194,
    ),
    (
        "meridian_advanced_defences_scaffolding.glb",
        470108,
        0x56d2bd33d53cd0a2,
    ),
    ("meridian_barracks.glb", 400680, 0x40158c4754768874),
    ("meridian_barracks_damaged.glb", 414164, 0x991f3a90554e21cb),
    (
        "meridian_barracks_foundation.glb",
        191208,
        0xcd6f20ea843624d4,
    ),
    (
        "meridian_barracks_heavily_damaged.glb",
        453696,
        0xa32d41fde6fc5b35,
    ),
    (
        "meridian_barracks_scaffolding.glb",
        483288,
        0x1fe2f44ca2ac4744,
    ),
    (
        "meridian_construction_center.glb",
        519268,
        0x703bf9b24e8e6b0f,
    ),
    (
        "meridian_construction_center_damaged.glb",
        580648,
        0x001fc2246e24580c,
    ),
    (
        "meridian_construction_center_foundation.glb",
        243632,
        0x9f72468286bd8820,
    ),
    (
        "meridian_construction_center_heavily_damaged.glb",
        483376,
        0x014afd928d1d20c7,
    ),
    (
        "meridian_construction_center_scaffolding.glb",
        594356,
        0xe6e69adbb52176b3,
    ),
    ("meridian_decoy_emplacement.glb", 435896, 0x8f0d07d415a043ee),
    (
        "meridian_decoy_emplacement_damaged.glb",
        431704,
        0x52ef01a33804e99c,
    ),
    (
        "meridian_decoy_emplacement_foundation.glb",
        148968,
        0x1e4f682b655533d9,
    ),
    (
        "meridian_decoy_emplacement_heavily_damaged.glb",
        518572,
        0x7d33536089a61ea9,
    ),
    (
        "meridian_decoy_emplacement_scaffolding.glb",
        424732,
        0x3d823b08dd11679f,
    ),
    ("meridian_defence_tower.glb", 369488, 0xb6c78b3844246548),
    (
        "meridian_defence_tower_damaged.glb",
        368376,
        0x3826e1088c286a45,
    ),
    (
        "meridian_defence_tower_foundation.glb",
        149000,
        0xa94cc08810244cf4,
    ),
    (
        "meridian_defence_tower_heavily_damaged.glb",
        453032,
        0xc63e3ac6d779ec81,
    ),
    (
        "meridian_defence_tower_scaffolding.glb",
        337380,
        0xd8af108097f6546e,
    ),
    ("meridian_harvester.glb", 251960, 0xfdc6d76c5edada0d),
    ("meridian_light_tank.glb", 242188, 0xa78c91f749af33da),
    ("meridian_ore_silo.glb", 380744, 0xe5b00b983cb5c144),
    ("meridian_ore_silo_damaged.glb", 490976, 0x7a0cbe8eecabcc7b),
    (
        "meridian_ore_silo_foundation.glb",
        180896,
        0x74219bd8fda5b1de,
    ),
    (
        "meridian_ore_silo_heavily_damaged.glb",
        533640,
        0xd845e90cc31b2dd6,
    ),
    (
        "meridian_ore_silo_scaffolding.glb",
        400768,
        0x16b09ab7ef671676,
    ),
    ("meridian_power_plant.glb", 400008, 0x4446ef96fb135c58),
    (
        "meridian_power_plant_damaged.glb",
        439372,
        0x7200ed290a8fa9a5,
    ),
    (
        "meridian_power_plant_foundation.glb",
        191156,
        0xcddb6a49ff50f4fe,
    ),
    (
        "meridian_power_plant_heavily_damaged.glb",
        391108,
        0x814f2bb700d5282f,
    ),
    (
        "meridian_power_plant_scaffolding.glb",
        403300,
        0xa871a356d7799d8b,
    ),
    ("meridian_radar.glb", 376052, 0xbd6463999840423e),
    ("meridian_radar_damaged.glb", 415300, 0x2074a34f49bf8e49),
    ("meridian_radar_foundation.glb", 180884, 0xd0a412bf74560b93),
    (
        "meridian_radar_heavily_damaged.glb",
        431444,
        0xcc4e9a983c9e1a0a,
    ),
    ("meridian_radar_scaffolding.glb", 439480, 0x278a9c38e2ef60e8),
    ("meridian_refinery.glb", 522540, 0x77a1dc82be4b49ec),
    ("meridian_refinery_damaged.glb", 561216, 0x18fd8796741bb444),
    (
        "meridian_refinery_foundation.glb",
        221928,
        0xee6dcf3b951d7d96,
    ),
    (
        "meridian_refinery_heavily_damaged.glb",
        509656,
        0x781a921216031832,
    ),
    (
        "meridian_refinery_scaffolding.glb",
        592364,
        0x55702227e9bda1f6,
    ),
    ("meridian_shipyard.glb", 478568, 0xd66cbabc1a0347db),
    ("meridian_shipyard_damaged.glb", 516568, 0x91102bc2201a3880),
    (
        "meridian_shipyard_foundation.glb",
        253828,
        0x47c414a68d95bba9,
    ),
    (
        "meridian_shipyard_heavily_damaged.glb",
        505764,
        0x0b8cbcb314cc71a3,
    ),
    (
        "meridian_shipyard_scaffolding.glb",
        669460,
        0x91e0a3b78885b336,
    ),
    ("meridian_vehicle_factory.glb", 410696, 0xb0c77f7186429420),
    (
        "meridian_vehicle_factory_damaged.glb",
        470256,
        0xc6d3774c73e368f1,
    ),
    (
        "meridian_vehicle_factory_foundation.glb",
        221944,
        0x5b737353d6224b42,
    ),
    (
        "meridian_vehicle_factory_heavily_damaged.glb",
        486444,
        0x7dc52e50ddca8e83,
    ),
    (
        "meridian_vehicle_factory_scaffolding.glb",
        569984,
        0x9c5090def54c70ea,
    ),
    ("network_automated_gun.glb", 181108, 0x2b2c22d4cc261db2),
    (
        "network_automated_gun_disabled.glb",
        181136,
        0x8150ec2caf3697fa,
    ),
    (
        "network_automated_gun_rubble.glb",
        235248,
        0x424fc12db5ef0aeb,
    ),
    ("network_unmanned_artillery.glb", 315844, 0xbbea3d9e1a6f30d4),
    (
        "network_unmanned_artillery_no_uplink.glb",
        204992,
        0xff5aa0384e17d6bd,
    ),
    (
        "network_unmanned_artillery_rubble.glb",
        382280,
        0x9d4577a6ce7cd918,
    ),
    ("neutral_archive_building.glb", 275476, 0xb8be02c0f8602a58),
    (
        "neutral_archive_building_damaged.glb",
        363548,
        0x7edb4181edd9ec88,
    ),
    (
        "neutral_archive_building_heavily_damaged.glb",
        292484,
        0x016072710dc1ce49,
    ),
    ("neutral_bridge_segment.glb", 148280, 0x54ddf03d83f7b952),
    (
        "neutral_bridge_segment_damaged.glb",
        146368,
        0x7e7ad4bfb6a6846f,
    ),
    (
        "neutral_bridge_segment_destroyed.glb",
        126316,
        0xef7ae96197e1ea09,
    ),
    ("neutral_evidence_crate.glb", 32088, 0x2b4502f209c5868b),
    ("neutral_fuel_depot.glb", 248452, 0x21e728be44db6684),
    (
        "neutral_fuel_depot_burnt_out.glb",
        228772,
        0x34bee2849c8c0d91,
    ),
    ("neutral_fuel_depot_damaged.glb", 292256, 0x2a542904344f2781),
    ("neutral_generator.glb", 154904, 0xe29ff62acdcaaf95),
    ("neutral_generator_off.glb", 153640, 0x261ecfdcf4f01c6a),
    ("neutral_generator_rubble.glb", 140820, 0x2acaa6c0d7b139ed),
    ("neutral_rail_spur.glb", 145356, 0x732a769f53d7bd1c),
    (
        "neutral_rail_spur_destroyed.glb",
        116260,
        0x3463a753a27a61f9,
    ),
    ("neutral_relay_transmitter.glb", 286012, 0x6b0be1bdab50e282),
    (
        "neutral_relay_transmitter_damaged.glb",
        284896,
        0xbe8290de04fe64ec,
    ),
    (
        "neutral_relay_transmitter_rubble.glb",
        187844,
        0xf37761488ff967ee,
    ),
    ("neutral_rubble_1x1.glb", 41312, 0xccec2f6aec4a955c),
    ("neutral_rubble_2x2.glb", 145484, 0x8dc71454f45772e3),
    ("neutral_rubble_2x3.glb", 189992, 0xa9b049f04fb23c44),
    ("neutral_rubble_3x4.glb", 205920, 0xc0532453471f1dfc),
    ("neutral_rubble_4x4.glb", 205920, 0xd62683357957c4eb),
    ("neutral_rubble_4x5.glb", 205916, 0xdd7db6c6c093d9b0),
    ("veyran_advanced_defences.glb", 411700, 0xe38c652a55a83052),
    (
        "veyran_advanced_defences_damaged.glb",
        449344,
        0xd438c52a3ef3dec6,
    ),
    (
        "veyran_advanced_defences_foundation.glb",
        180900,
        0x939e6d80f62350f8,
    ),
    (
        "veyran_advanced_defences_heavily_damaged.glb",
        521472,
        0xf21ba327ee538b6f,
    ),
    (
        "veyran_advanced_defences_scaffolding.glb",
        417672,
        0x63ff7bb477082d7f,
    ),
    ("veyran_barracks.glb", 399536, 0xc27b3381af6f0336),
    ("veyran_barracks_damaged.glb", 430352, 0x06499dfe54597d57),
    ("veyran_barracks_foundation.glb", 191204, 0xbd301143e19eb6f5),
    (
        "veyran_barracks_heavily_damaged.glb",
        484056,
        0xdd7e24da58fd3fed,
    ),
    (
        "veyran_barracks_scaffolding.glb",
        493772,
        0xb3005a36bbfc2870,
    ),
    ("veyran_construction_center.glb", 473936, 0xbf8b5f7880446186),
    (
        "veyran_construction_center_damaged.glb",
        540552,
        0xeef8bf79bdb87aa0,
    ),
    (
        "veyran_construction_center_foundation.glb",
        243620,
        0xf3febb2f758eb7f2,
    ),
    (
        "veyran_construction_center_heavily_damaged.glb",
        544396,
        0xa465143d82d953c2,
    ),
    (
        "veyran_construction_center_scaffolding.glb",
        562216,
        0x4d980b07d655abeb,
    ),
    ("veyran_defence_tower.glb", 343516, 0xe2b3fb24edd0e4d5),
    (
        "veyran_defence_tower_damaged.glb",
        340352,
        0x2d2c7ae7489f3803,
    ),
    (
        "veyran_defence_tower_foundation.glb",
        149008,
        0x87f2fcba2af32efd,
    ),
    (
        "veyran_defence_tower_heavily_damaged.glb",
        305688,
        0xc19f0fa1544933db,
    ),
    (
        "veyran_defence_tower_scaffolding.glb",
        249764,
        0xe8f47fc76ef21af2,
    ),
    ("veyran_fortified_bunker.glb", 361584, 0x12c917903a222eb2),
    (
        "veyran_fortified_bunker_damaged.glb",
        399692,
        0x6766b1f58a6381de,
    ),
    (
        "veyran_fortified_bunker_foundation.glb",
        180904,
        0x9424e7d0e0afb5d2,
    ),
    (
        "veyran_fortified_bunker_heavily_damaged.glb",
        481460,
        0xebe083668e2f0866,
    ),
    (
        "veyran_fortified_bunker_scaffolding.glb",
        442636,
        0x4e2e2eb4585a5322,
    ),
    ("veyran_light_tank.glb", 223532, 0x520461c2447f5b49),
    ("veyran_ore_silo.glb", 376304, 0x92d839a170709e73),
    ("veyran_ore_silo_damaged.glb", 508272, 0xa0d4bd76d4e03b46),
    ("veyran_ore_silo_foundation.glb", 180892, 0xcd5655a6b37cb62d),
    (
        "veyran_ore_silo_heavily_damaged.glb",
        550404,
        0xd3ee6c18d5b64159,
    ),
    (
        "veyran_ore_silo_scaffolding.glb",
        386860,
        0x0158241ee8b46f21,
    ),
    ("veyran_power_plant.glb", 473580, 0xd0deb10bf3a80a22),
    ("veyran_power_plant_damaged.glb", 532788, 0xbc15086af08cc7fa),
    (
        "veyran_power_plant_foundation.glb",
        191152,
        0x939f230ac1ef3863,
    ),
    (
        "veyran_power_plant_heavily_damaged.glb",
        413664,
        0x1d298169b82da834,
    ),
    (
        "veyran_power_plant_scaffolding.glb",
        452948,
        0x1b946a5d1ccb8424,
    ),
    ("veyran_radar.glb", 388296, 0xeaa285854203adbf),
    ("veyran_radar_damaged.glb", 430848, 0x43779586b2d755c9),
    ("veyran_radar_foundation.glb", 180880, 0xdb4bf7e4c18c1450),
    (
        "veyran_radar_heavily_damaged.glb",
        467848,
        0x2251943c8b969192,
    ),
    ("veyran_radar_scaffolding.glb", 472100, 0xeb9d5ef8164adfad),
    ("veyran_refinery.glb", 459380, 0xcf55f48b59516ff3),
    ("veyran_refinery_damaged.glb", 492368, 0xfe0774e4f1b37f2e),
    ("veyran_refinery_foundation.glb", 221924, 0xede27d5f8835065a),
    (
        "veyran_refinery_heavily_damaged.glb",
        510380,
        0x34e705baa7a8c24e,
    ),
    (
        "veyran_refinery_scaffolding.glb",
        600856,
        0xdf20ab62d7e5e6b5,
    ),
    ("veyran_shipyard.glb", 451476, 0x68cf9c9c079381df),
    ("veyran_shipyard_damaged.glb", 487300, 0xd87fc8aa44280643),
    ("veyran_shipyard_foundation.glb", 253824, 0x2753b548f295ee3e),
    (
        "veyran_shipyard_heavily_damaged.glb",
        526536,
        0x397d69daf539f02f,
    ),
    (
        "veyran_shipyard_scaffolding.glb",
        678400,
        0x7f4ebed9978855ca,
    ),
    ("veyran_vehicle_factory.glb", 416244, 0x4b25a30c31da70da),
    (
        "veyran_vehicle_factory_damaged.glb",
        483556,
        0x146a043a477a893d,
    ),
    (
        "veyran_vehicle_factory_foundation.glb",
        221944,
        0x5fd3e79f8b145ca6,
    ),
    (
        "veyran_vehicle_factory_heavily_damaged.glb",
        511440,
        0x4402f7b613059dce,
    ),
    (
        "veyran_vehicle_factory_scaffolding.glb",
        549008,
        0x0976c6d954e4f207,
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
