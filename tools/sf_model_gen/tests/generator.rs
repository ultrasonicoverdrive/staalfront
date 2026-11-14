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
        498696,
        0xaf5e52b011b4dcf6,
    ),
    (
        "meridian_advanced_defences_foundation.glb",
        180904,
        0xc2e46121bc981209,
    ),
    (
        "meridian_advanced_defences_heavily_damaged.glb",
        544468,
        0x6f553aeced530915,
    ),
    (
        "meridian_advanced_defences_scaffolding.glb",
        470108,
        0x56d2bd33d53cd0a2,
    ),
    ("meridian_barracks.glb", 400680, 0x5a709d612a35a29b),
    ("meridian_barracks_damaged.glb", 437188, 0x3594f97e1c238afd),
    (
        "meridian_barracks_foundation.glb",
        201436,
        0x08910d5d665df059,
    ),
    (
        "meridian_barracks_heavily_damaged.glb",
        453140,
        0x28d8d4d8bb92bb39,
    ),
    (
        "meridian_barracks_scaffolding.glb",
        480784,
        0xcd4022be79e9a4fd,
    ),
    (
        "meridian_construction_center.glb",
        519268,
        0x703bf9b24e8e6b0f,
    ),
    (
        "meridian_construction_center_damaged.glb",
        575748,
        0xea969d0ed37202de,
    ),
    (
        "meridian_construction_center_foundation.glb",
        243632,
        0x9f72468286bd8820,
    ),
    (
        "meridian_construction_center_heavily_damaged.glb",
        482356,
        0xac69609b7d5a1696,
    ),
    (
        "meridian_construction_center_scaffolding.glb",
        594356,
        0xe6e69adbb52176b3,
    ),
    ("meridian_decoy_emplacement.glb", 435896, 0x8f0d07d415a043ee),
    (
        "meridian_decoy_emplacement_damaged.glb",
        435352,
        0x97af77fbdb976b72,
    ),
    (
        "meridian_decoy_emplacement_foundation.glb",
        148968,
        0x1e4f682b655533d9,
    ),
    (
        "meridian_decoy_emplacement_heavily_damaged.glb",
        529172,
        0xbb095e7cb5885d9c,
    ),
    (
        "meridian_decoy_emplacement_scaffolding.glb",
        424732,
        0x3d823b08dd11679f,
    ),
    ("meridian_defence_tower.glb", 369488, 0xb6c78b3844246548),
    (
        "meridian_defence_tower_damaged.glb",
        367572,
        0x445183d4a94249dc,
    ),
    (
        "meridian_defence_tower_foundation.glb",
        149000,
        0xa94cc08810244cf4,
    ),
    (
        "meridian_defence_tower_heavily_damaged.glb",
        452804,
        0x354ed9b7ef26aea1,
    ),
    (
        "meridian_defence_tower_scaffolding.glb",
        337380,
        0xd8af108097f6546e,
    ),
    ("meridian_harvester.glb", 251960, 0xfdc6d76c5edada0d),
    ("meridian_light_tank.glb", 242188, 0xa78c91f749af33da),
    ("meridian_ore_silo.glb", 380744, 0xe5b00b983cb5c144),
    ("meridian_ore_silo_damaged.glb", 494852, 0x42843618e4f13c21),
    (
        "meridian_ore_silo_foundation.glb",
        180896,
        0x74219bd8fda5b1de,
    ),
    (
        "meridian_ore_silo_heavily_damaged.glb",
        531944,
        0x3e367cf87a4b7b14,
    ),
    (
        "meridian_ore_silo_scaffolding.glb",
        400768,
        0x16b09ab7ef671676,
    ),
    ("meridian_power_plant.glb", 400008, 0x4446ef96fb135c58),
    (
        "meridian_power_plant_damaged.glb",
        438236,
        0x76bd970effdd197f,
    ),
    (
        "meridian_power_plant_foundation.glb",
        191156,
        0xcddb6a49ff50f4fe,
    ),
    (
        "meridian_power_plant_heavily_damaged.glb",
        391108,
        0x638d9d2aa3615f18,
    ),
    (
        "meridian_power_plant_scaffolding.glb",
        403300,
        0xa871a356d7799d8b,
    ),
    ("meridian_radar.glb", 376052, 0xbd6463999840423e),
    ("meridian_radar_damaged.glb", 412452, 0x066c77b2c9b3edde),
    ("meridian_radar_foundation.glb", 180884, 0xd0a412bf74560b93),
    (
        "meridian_radar_heavily_damaged.glb",
        432696,
        0x80c721c09eb6ac04,
    ),
    ("meridian_radar_scaffolding.glb", 439480, 0x278a9c38e2ef60e8),
    ("meridian_refinery.glb", 522540, 0x77a1dc82be4b49ec),
    ("meridian_refinery_damaged.glb", 567260, 0x3e17cfe627ac5c1a),
    (
        "meridian_refinery_foundation.glb",
        221928,
        0xee6dcf3b951d7d96,
    ),
    (
        "meridian_refinery_heavily_damaged.glb",
        508176,
        0x932f96a7dfc21f0f,
    ),
    (
        "meridian_refinery_scaffolding.glb",
        592364,
        0x55702227e9bda1f6,
    ),
    ("meridian_shipyard.glb", 478568, 0xd66cbabc1a0347db),
    ("meridian_shipyard_damaged.glb", 517584, 0xfea34273f732ffbd),
    (
        "meridian_shipyard_foundation.glb",
        253828,
        0x47c414a68d95bba9,
    ),
    (
        "meridian_shipyard_heavily_damaged.glb",
        505764,
        0x82e103c49c4657fb,
    ),
    (
        "meridian_shipyard_scaffolding.glb",
        669460,
        0x91e0a3b78885b336,
    ),
    ("meridian_vehicle_factory.glb", 410696, 0xb0c77f7186429420),
    (
        "meridian_vehicle_factory_damaged.glb",
        470376,
        0x8aa911debd9f7950,
    ),
    (
        "meridian_vehicle_factory_foundation.glb",
        221944,
        0x5b737353d6224b42,
    ),
    (
        "meridian_vehicle_factory_heavily_damaged.glb",
        486452,
        0x85f1defe506c1e9f,
    ),
    (
        "meridian_vehicle_factory_scaffolding.glb",
        569984,
        0x9c5090def54c70ea,
    ),
    ("network_automated_gun.glb", 175400, 0xf04984acd441ad3d),
    (
        "network_automated_gun_disabled.glb",
        175428,
        0x844f1a402e2b169a,
    ),
    (
        "network_automated_gun_rubble.glb",
        235236,
        0x9870ed19101cc26f,
    ),
    ("network_unmanned_artillery.glb", 315844, 0xbbea3d9e1a6f30d4),
    (
        "network_unmanned_artillery_no_uplink.glb",
        204992,
        0xff5aa0384e17d6bd,
    ),
    (
        "network_unmanned_artillery_rubble.glb",
        382168,
        0xb0059d2abf3e5a00,
    ),
    ("neutral_archive_building.glb", 275476, 0xb8be02c0f8602a58),
    (
        "neutral_archive_building_damaged.glb",
        376772,
        0x548279be9de10176,
    ),
    (
        "neutral_archive_building_heavily_damaged.glb",
        292492,
        0xd7964b7763e11819,
    ),
    ("neutral_bridge_segment.glb", 148280, 0x54ddf03d83f7b952),
    (
        "neutral_bridge_segment_damaged.glb",
        148304,
        0x7d98294829bdeae6,
    ),
    (
        "neutral_bridge_segment_destroyed.glb",
        125180,
        0x05ae3c5888140114,
    ),
    ("neutral_evidence_crate.glb", 32092, 0x6018e9ef3f3683d6),
    ("neutral_fuel_depot.glb", 248452, 0x21e728be44db6684),
    (
        "neutral_fuel_depot_burnt_out.glb",
        231508,
        0x84d36548014d8b37,
    ),
    ("neutral_fuel_depot_damaged.glb", 291232, 0xf8f0692e2d02bc23),
    ("neutral_generator.glb", 154904, 0xe29ff62acdcaaf95),
    ("neutral_generator_off.glb", 153640, 0x261ecfdcf4f01c6a),
    ("neutral_generator_rubble.glb", 140936, 0xa58813b637876bac),
    ("neutral_rail_spur.glb", 145356, 0x732a769f53d7bd1c),
    (
        "neutral_rail_spur_destroyed.glb",
        116596,
        0x05e61107d8ca5b33,
    ),
    ("neutral_relay_transmitter.glb", 286012, 0x6b0be1bdab50e282),
    (
        "neutral_relay_transmitter_damaged.glb",
        284668,
        0x152499e24041c660,
    ),
    (
        "neutral_relay_transmitter_rubble.glb",
        186812,
        0x19faed29e47e7eff,
    ),
    ("neutral_rubble_1x1.glb", 41312, 0x624ed4667383eedb),
    ("neutral_rubble_2x2.glb", 145484, 0x1bd88e4099f91e57),
    ("neutral_rubble_2x3.glb", 189992, 0x648b0a57f2b1dbe4),
    ("neutral_rubble_3x4.glb", 205924, 0x584d874d5cfa8d35),
    ("neutral_rubble_4x4.glb", 205920, 0x18d317dad48c6fd4),
    ("neutral_rubble_4x5.glb", 205916, 0x56def30f56b43b6b),
    ("veyran_advanced_defences.glb", 411700, 0xe38c652a55a83052),
    (
        "veyran_advanced_defences_damaged.glb",
        451740,
        0xf7a9d3bf968baa66,
    ),
    (
        "veyran_advanced_defences_foundation.glb",
        180900,
        0x939e6d80f62350f8,
    ),
    (
        "veyran_advanced_defences_heavily_damaged.glb",
        521356,
        0x8a7f2cde49e19d8c,
    ),
    (
        "veyran_advanced_defences_scaffolding.glb",
        417672,
        0x63ff7bb477082d7f,
    ),
    ("veyran_barracks.glb", 389272, 0xe2850a7c497a0d7d),
    ("veyran_barracks_damaged.glb", 429776, 0x2f9c4433b36c6712),
    ("veyran_barracks_foundation.glb", 191180, 0x01763e272f217526),
    (
        "veyran_barracks_heavily_damaged.glb",
        484176,
        0x6a296de025210709,
    ),
    (
        "veyran_barracks_scaffolding.glb",
        486928,
        0xaa2c1678d73be77c,
    ),
    ("veyran_construction_center.glb", 473936, 0xbf8b5f7880446186),
    (
        "veyran_construction_center_damaged.glb",
        534176,
        0x440155c5a41cd74d,
    ),
    (
        "veyran_construction_center_foundation.glb",
        243620,
        0xf3febb2f758eb7f2,
    ),
    (
        "veyran_construction_center_heavily_damaged.glb",
        544396,
        0x31f99130d947a05d,
    ),
    (
        "veyran_construction_center_scaffolding.glb",
        562216,
        0x4d980b07d655abeb,
    ),
    ("veyran_defence_tower.glb", 343516, 0xe2b3fb24edd0e4d5),
    (
        "veyran_defence_tower_damaged.glb",
        343428,
        0xf990209b76a9e5bd,
    ),
    (
        "veyran_defence_tower_foundation.glb",
        149008,
        0x87f2fcba2af32efd,
    ),
    (
        "veyran_defence_tower_heavily_damaged.glb",
        306144,
        0x574434f5077fa2ce,
    ),
    (
        "veyran_defence_tower_scaffolding.glb",
        249764,
        0xe8f47fc76ef21af2,
    ),
    ("veyran_fortified_bunker.glb", 361584, 0x12c917903a222eb2),
    (
        "veyran_fortified_bunker_damaged.glb",
        400372,
        0x35e9327ed5b3d4e9,
    ),
    (
        "veyran_fortified_bunker_foundation.glb",
        180904,
        0x9424e7d0e0afb5d2,
    ),
    (
        "veyran_fortified_bunker_heavily_damaged.glb",
        481460,
        0x843369ed01f2cd85,
    ),
    (
        "veyran_fortified_bunker_scaffolding.glb",
        442636,
        0x4e2e2eb4585a5322,
    ),
    ("veyran_light_tank.glb", 223532, 0x520461c2447f5b49),
    ("veyran_ore_silo.glb", 376304, 0x92d839a170709e73),
    ("veyran_ore_silo_damaged.glb", 503600, 0xb5a00de394b5453e),
    ("veyran_ore_silo_foundation.glb", 180892, 0xcd5655a6b37cb62d),
    (
        "veyran_ore_silo_heavily_damaged.glb",
        544260,
        0x0f9298430a464070,
    ),
    (
        "veyran_ore_silo_scaffolding.glb",
        386860,
        0x0158241ee8b46f21,
    ),
    ("veyran_power_plant.glb", 473580, 0xd0deb10bf3a80a22),
    ("veyran_power_plant_damaged.glb", 511236, 0x47bf141642bfd6da),
    (
        "veyran_power_plant_foundation.glb",
        191152,
        0x939f230ac1ef3863,
    ),
    (
        "veyran_power_plant_heavily_damaged.glb",
        413664,
        0x01426406d5ace9c3,
    ),
    (
        "veyran_power_plant_scaffolding.glb",
        452948,
        0x1b946a5d1ccb8424,
    ),
    ("veyran_radar.glb", 388296, 0xeaa285854203adbf),
    ("veyran_radar_damaged.glb", 430852, 0xe2f9e1a8dae92b63),
    ("veyran_radar_foundation.glb", 180880, 0xdb4bf7e4c18c1450),
    (
        "veyran_radar_heavily_damaged.glb",
        467276,
        0xfd58f3683337b49c,
    ),
    ("veyran_radar_scaffolding.glb", 472100, 0xeb9d5ef8164adfad),
    ("veyran_refinery.glb", 459380, 0xcf55f48b59516ff3),
    ("veyran_refinery_damaged.glb", 492132, 0xae22efe89a4574fe),
    ("veyran_refinery_foundation.glb", 221924, 0xede27d5f8835065a),
    (
        "veyran_refinery_heavily_damaged.glb",
        511520,
        0xfaa011a7cf7496b1,
    ),
    (
        "veyran_refinery_scaffolding.glb",
        600856,
        0xdf20ab62d7e5e6b5,
    ),
    ("veyran_shipyard.glb", 451476, 0x68cf9c9c079381df),
    ("veyran_shipyard_damaged.glb", 487992, 0xd83888e57c84bd73),
    ("veyran_shipyard_foundation.glb", 253824, 0x2753b548f295ee3e),
    (
        "veyran_shipyard_heavily_damaged.glb",
        526884,
        0xf199d4d18c7dfb85,
    ),
    (
        "veyran_shipyard_scaffolding.glb",
        678400,
        0x7f4ebed9978855ca,
    ),
    ("veyran_vehicle_factory.glb", 416244, 0x4b25a30c31da70da),
    (
        "veyran_vehicle_factory_damaged.glb",
        520380,
        0x299a24142fce0950,
    ),
    (
        "veyran_vehicle_factory_foundation.glb",
        221944,
        0x5fd3e79f8b145ca6,
    ),
    (
        "veyran_vehicle_factory_heavily_damaged.glb",
        509164,
        0xdcf81fc20dff1781,
    ),
    (
        "veyran_vehicle_factory_scaffolding.glb",
        549008,
        0x0976c6d954e4f207,
    ),
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

/// One run of the generator, shared by the tests of this file (a run takes a while: 143 files).
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
    assert_eq!(first.len(), PINNED.len());
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
    for (name, bytes) in first() {
        let on_disk = std::fs::read(set_dir().join(name))
            .unwrap_or_else(|e| panic!("{name} is not committed: {e}"));
        assert!(
            on_disk == *bytes,
            "{name} is out of date: run `cargo run -p sf_model_gen`"
        );
    }
}
