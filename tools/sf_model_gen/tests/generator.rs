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
// Re-pinned 15-11-2026 (WP11.6p, realistic palette; again after its review on 17-11-2026): the
// albedo palette of model_palette.ron, grime 0.40 -> 0.20 plus dust towards (118, 104, 82),
// highlight 0.16 -> 0.05, wear 0.35 -> 0.20 with the paint's tint, panel lines 560 -> 500, team
// vertices without grime, and (review) a material factor of at least 0.15 so the dust cannot clamp
// a dark slot's vertex colour. Every file moved; no recipe or kit geometry changed, so only COLOR_0
// and the material factors differ (every file is 60-80 bytes shorter than on main: the factor of
// each dark slot now prints as 0.15).
const PINNED: [(&str, usize, u64); 143] = [
    ("meridian_advanced_defences.glb", 456748, 0x4ce0f07825f2bb8e),
    (
        "meridian_advanced_defences_damaged.glb",
        256960,
        0xa11183f24364e190,
    ),
    (
        "meridian_advanced_defences_foundation.glb",
        88680,
        0x4bc3ac971e61b359,
    ),
    (
        "meridian_advanced_defences_heavily_damaged.glb",
        262228,
        0x0dccef5b448fd785,
    ),
    (
        "meridian_advanced_defences_scaffolding.glb",
        205288,
        0xa5ce97190b41cadb,
    ),
    ("meridian_barracks.glb", 400832, 0x802236b97daa8bf0),
    ("meridian_barracks_damaged.glb", 203384, 0x7794bf278f230f74),
    (
        "meridian_barracks_foundation.glb",
        93508,
        0xcae6c32511a6e75e,
    ),
    (
        "meridian_barracks_heavily_damaged.glb",
        189660,
        0x4d781d5031e8166d,
    ),
    (
        "meridian_barracks_scaffolding.glb",
        194572,
        0x257144d4c1431041,
    ),
    (
        "meridian_construction_center.glb",
        541076,
        0x509f5f72fd660ef9,
    ),
    (
        "meridian_construction_center_damaged.glb",
        428812,
        0xbd79560b69c155f0,
    ),
    (
        "meridian_construction_center_foundation.glb",
        114052,
        0xd71ffb8e1dbb911c,
    ),
    (
        "meridian_construction_center_heavily_damaged.glb",
        272024,
        0xa222e25bfd84f30e,
    ),
    (
        "meridian_construction_center_scaffolding.glb",
        266456,
        0x6b084690ac0b8948,
    ),
    ("meridian_decoy_emplacement.glb", 436276, 0x1df1f4ba87a4fc8d),
    (
        "meridian_decoy_emplacement_damaged.glb",
        209904,
        0x0d3a7c8f7052b9c8,
    ),
    (
        "meridian_decoy_emplacement_foundation.glb",
        82060,
        0x9901c511fee875ec,
    ),
    (
        "meridian_decoy_emplacement_heavily_damaged.glb",
        240080,
        0xa4d0e0cf4b091377,
    ),
    (
        "meridian_decoy_emplacement_scaffolding.glb",
        175692,
        0x571f45601d35f3aa,
    ),
    ("meridian_defence_tower.glb", 368724, 0xc841e733ff613b50),
    (
        "meridian_defence_tower_damaged.glb",
        155672,
        0x5cf6f3da475a7df8,
    ),
    (
        "meridian_defence_tower_foundation.glb",
        82088,
        0x09f0632b0b405201,
    ),
    (
        "meridian_defence_tower_heavily_damaged.glb",
        182864,
        0x08ff5f748730edd6,
    ),
    (
        "meridian_defence_tower_scaffolding.glb",
        114284,
        0x4272f1989112a6ab,
    ),
    ("meridian_harvester.glb", 290188, 0x7a8e76b3536f7c0e),
    ("meridian_light_tank.glb", 281332, 0xfe258b1ea5341b37),
    ("meridian_ore_silo.glb", 380436, 0xe5763e8d83d7b6bd),
    ("meridian_ore_silo_damaged.glb", 210868, 0xbb81e79a10f0991f),
    (
        "meridian_ore_silo_foundation.glb",
        88672,
        0x7397d681d7b590ea,
    ),
    (
        "meridian_ore_silo_heavily_damaged.glb",
        212372,
        0xd357af425cf17bcb,
    ),
    (
        "meridian_ore_silo_scaffolding.glb",
        142608,
        0x66c8511cb76abe46,
    ),
    ("meridian_power_plant.glb", 399928, 0x9930cbe5a8d13efb),
    (
        "meridian_power_plant_damaged.glb",
        309420,
        0xbb61ab52297482f6,
    ),
    (
        "meridian_power_plant_foundation.glb",
        93460,
        0xc2b4ac2e970bcdfa,
    ),
    (
        "meridian_power_plant_heavily_damaged.glb",
        212496,
        0x0513c66fedea564a,
    ),
    (
        "meridian_power_plant_scaffolding.glb",
        194116,
        0x5a113ddc9d781235,
    ),
    ("meridian_radar.glb", 376432, 0x2f9acfe811763f1e),
    ("meridian_radar_damaged.glb", 184016, 0xfa3caae033fbfae8),
    ("meridian_radar_foundation.glb", 88660, 0xc68413729753ae27),
    (
        "meridian_radar_heavily_damaged.glb",
        144720,
        0x8313cb14554d98c1,
    ),
    ("meridian_radar_scaffolding.glb", 192948, 0xaa096ed9a9c9a24e),
    ("meridian_refinery.glb", 528844, 0x5fc3bb83611bcadb),
    ("meridian_refinery_damaged.glb", 402408, 0x0ed323f8f012b883),
    (
        "meridian_refinery_foundation.glb",
        107388,
        0x7c3e129d28fbd9ac,
    ),
    (
        "meridian_refinery_heavily_damaged.glb",
        300360,
        0x5fd679f755c3f794,
    ),
    (
        "meridian_refinery_scaffolding.glb",
        237764,
        0x2b15b13659f3370c,
    ),
    ("meridian_shipyard.glb", 551004, 0x18f9009f1d42fab3),
    ("meridian_shipyard_damaged.glb", 406196, 0xd224d55384e6d168),
    (
        "meridian_shipyard_foundation.glb",
        119468,
        0xa30bb9095f283ff1,
    ),
    (
        "meridian_shipyard_heavily_damaged.glb",
        301876,
        0x6c8dc6be8bc304d3,
    ),
    (
        "meridian_shipyard_scaffolding.glb",
        280752,
        0xe7fdb3fcf86b4528,
    ),
    ("meridian_vehicle_factory.glb", 409704, 0x085be9685cf6acc1),
    (
        "meridian_vehicle_factory_damaged.glb",
        296892,
        0xc38c80ed9b438787,
    ),
    (
        "meridian_vehicle_factory_foundation.glb",
        107404,
        0x5d12d6624d8dcb60,
    ),
    (
        "meridian_vehicle_factory_heavily_damaged.glb",
        239316,
        0xb75de0fb1e44b12b,
    ),
    (
        "meridian_vehicle_factory_scaffolding.glb",
        253880,
        0x1ff5a18df110b00e,
    ),
    ("network_automated_gun.glb", 181480, 0x2102f07621f56e3e),
    (
        "network_automated_gun_disabled.glb",
        181508,
        0x122f14b26666ba23,
    ),
    (
        "network_automated_gun_rubble.glb",
        84908,
        0xcbc6871c54d4b706,
    ),
    ("network_unmanned_artillery.glb", 316460, 0x55cd7e0d7192e8fe),
    (
        "network_unmanned_artillery_no_uplink.glb",
        205604,
        0x4ba20b4f15a8ab4a,
    ),
    (
        "network_unmanned_artillery_rubble.glb",
        182252,
        0xbb0f86503f8c3b82,
    ),
    ("neutral_archive_building.glb", 275416, 0xc0e4d4bdfac6cf03),
    (
        "neutral_archive_building_damaged.glb",
        183692,
        0xacdc973cc53b036f,
    ),
    (
        "neutral_archive_building_heavily_damaged.glb",
        120052,
        0x80ce7892964b497f,
    ),
    ("neutral_bridge_segment.glb", 148676, 0x6b6ff8dbe17cbf7c),
    (
        "neutral_bridge_segment_damaged.glb",
        93468,
        0xf1424b59ea2c8acb,
    ),
    (
        "neutral_bridge_segment_destroyed.glb",
        87216,
        0x14a833803414f970,
    ),
    ("neutral_evidence_crate.glb", 32028, 0x781dd4a0ae7fdd5a),
    ("neutral_fuel_depot.glb", 262076, 0xb20ffab2325f306e),
    (
        "neutral_fuel_depot_burnt_out.glb",
        144688,
        0x125242dedf414ea2,
    ),
    ("neutral_fuel_depot_damaged.glb", 209780, 0x39e49088c27bdf4d),
    ("neutral_generator.glb", 154844, 0x0c571d18affad55a),
    ("neutral_generator_off.glb", 153576, 0x8b7098b24715b09c),
    ("neutral_generator_rubble.glb", 69364, 0xb6aff55f930f2ed9),
    ("neutral_rail_spur.glb", 146216, 0x535cb7c3fe17448c),
    ("neutral_rail_spur_destroyed.glb", 57592, 0x3a7a5c0c666af979),
    ("neutral_relay_transmitter.glb", 285952, 0xea78c89e310fa0e6),
    (
        "neutral_relay_transmitter_damaged.glb",
        230112,
        0xf019a3ec75ec6850,
    ),
    (
        "neutral_relay_transmitter_rubble.glb",
        112312,
        0x0b1eb6c7726bf6f5,
    ),
    ("neutral_rubble_1x1.glb", 32124, 0xfee9f04e3a6eb272),
    ("neutral_rubble_2x2.glb", 101620, 0x230603f423b6c4a3),
    ("neutral_rubble_2x3.glb", 133376, 0x37174f079d1127e3),
    ("neutral_rubble_3x4.glb", 143832, 0xb72c93a370204a58),
    ("neutral_rubble_4x4.glb", 143836, 0xcf702228ed25bc8b),
    ("neutral_rubble_4x5.glb", 143828, 0xa43e9191adf3f64b),
    ("veyran_advanced_defences.glb", 411408, 0x42d6cc227c6348fe),
    (
        "veyran_advanced_defences_damaged.glb",
        242308,
        0x95be67e42497d54c,
    ),
    (
        "veyran_advanced_defences_foundation.glb",
        88692,
        0x610a1b494e770d44,
    ),
    (
        "veyran_advanced_defences_heavily_damaged.glb",
        259068,
        0x6bbc9ec4e9d09ddf,
    ),
    (
        "veyran_advanced_defences_scaffolding.glb",
        169320,
        0xa7b7b1c60d8e8a90,
    ),
    ("veyran_barracks.glb", 398792, 0x3b49ab597174a5d0),
    ("veyran_barracks_damaged.glb", 174460, 0x1e6e4091dec635f8),
    ("veyran_barracks_foundation.glb", 93520, 0x81c940ed72e029d4),
    (
        "veyran_barracks_heavily_damaged.glb",
        187412,
        0x278c34847f1e0258,
    ),
    (
        "veyran_barracks_scaffolding.glb",
        191192,
        0x3b18bfc0dba1d519,
    ),
    ("veyran_construction_center.glb", 526320, 0xee4725b6bf00bff7),
    (
        "veyran_construction_center_damaged.glb",
        392064,
        0x28059ffa9f70e9e3,
    ),
    (
        "veyran_construction_center_foundation.glb",
        114060,
        0xd08a965d26256669,
    ),
    (
        "veyran_construction_center_heavily_damaged.glb",
        327460,
        0x52f61f226fd77574,
    ),
    (
        "veyran_construction_center_scaffolding.glb",
        236164,
        0x722947c1d986320f,
    ),
    ("veyran_defence_tower.glb", 371276, 0xd43b817170f7cc76),
    (
        "veyran_defence_tower_damaged.glb",
        235100,
        0x7a65b4a9bef3ba24,
    ),
    (
        "veyran_defence_tower_foundation.glb",
        82116,
        0x34fe36ba79071408,
    ),
    (
        "veyran_defence_tower_heavily_damaged.glb",
        214924,
        0x1843330be929ee96,
    ),
    (
        "veyran_defence_tower_scaffolding.glb",
        148500,
        0xed8093f62c25ed5b,
    ),
    ("veyran_fortified_bunker.glb", 361292, 0x8849f1524c17b409),
    (
        "veyran_fortified_bunker_damaged.glb",
        158332,
        0x54034016e5e5cb84,
    ),
    (
        "veyran_fortified_bunker_foundation.glb",
        88692,
        0xb9d1e3e35957cb4b,
    ),
    (
        "veyran_fortified_bunker_heavily_damaged.glb",
        177468,
        0x44cf735f5ac570cc,
    ),
    (
        "veyran_fortified_bunker_scaffolding.glb",
        203700,
        0x8e105618cc250691,
    ),
    ("veyran_light_tank.glb", 269992, 0x23e9588bd5e1cff7),
    ("veyran_ore_silo.glb", 375332, 0x65898a80a5c867bb),
    ("veyran_ore_silo_damaged.glb", 212464, 0x69072f01d9e55909),
    ("veyran_ore_silo_foundation.glb", 88684, 0xc83f5130d386c65b),
    (
        "veyran_ore_silo_heavily_damaged.glb",
        211140,
        0x68f903e8455276af,
    ),
    (
        "veyran_ore_silo_scaffolding.glb",
        138968,
        0xfc24ad969c5af331,
    ),
    ("veyran_power_plant.glb", 496316, 0xf14bae30b7d8a400),
    ("veyran_power_plant_damaged.glb", 395932, 0xa923424d78073d14),
    (
        "veyran_power_plant_foundation.glb",
        93472,
        0x729e973bd1f83232,
    ),
    (
        "veyran_power_plant_heavily_damaged.glb",
        256516,
        0x7040292466046c1f,
    ),
    (
        "veyran_power_plant_scaffolding.glb",
        201876,
        0xbe343148b3233034,
    ),
    ("veyran_radar.glb", 388460, 0x03cf34933b6065d5),
    ("veyran_radar_damaged.glb", 233396, 0x9308bfa9f1d1309c),
    ("veyran_radar_foundation.glb", 88672, 0x4ecf625629c6c37e),
    (
        "veyran_radar_heavily_damaged.glb",
        157648,
        0xa58a93b43269853d,
    ),
    ("veyran_radar_scaffolding.glb", 210788, 0xe160a8efe9cc7d97),
    ("veyran_refinery.glb", 495804, 0x97bf3144dfc7d136),
    ("veyran_refinery_damaged.glb", 363820, 0xbd3e194cf39db9d5),
    ("veyran_refinery_foundation.glb", 107400, 0x76db85a90c99b5f7),
    (
        "veyran_refinery_heavily_damaged.glb",
        299724,
        0xf15e2bc927829ede,
    ),
    (
        "veyran_refinery_scaffolding.glb",
        243992,
        0xb392e38fcd4a0f18,
    ),
    ("veyran_shipyard.glb", 505684, 0x2cf9399d9bdfcd0b),
    ("veyran_shipyard_damaged.glb", 363428, 0xf18b85b1c965c18a),
    ("veyran_shipyard_foundation.glb", 119480, 0x881350c4773bd041),
    (
        "veyran_shipyard_heavily_damaged.glb",
        303972,
        0xcac9e441726c0d12,
    ),
    (
        "veyran_shipyard_scaffolding.glb",
        283324,
        0x9546b0090498fc50,
    ),
    ("veyran_vehicle_factory.glb", 415276, 0x0f64cbb059cbc082),
    (
        "veyran_vehicle_factory_damaged.glb",
        269196,
        0x4e0cdad0d42a109b,
    ),
    (
        "veyran_vehicle_factory_foundation.glb",
        107416,
        0xbda83b1ea7a8bc53,
    ),
    (
        "veyran_vehicle_factory_heavily_damaged.glb",
        249736,
        0xd2db425923be17b4,
    ),
    (
        "veyran_vehicle_factory_scaffolding.glb",
        258716,
        0xfd05be6ac75ba385,
    ),
];

/// The units of WP11.6b (vehicles, vessels, the heavy gun team), pinned the same way; see
// the first table for how to re-pin. Sorted by file name.
// WP11.6i: the two trial riflemen are in this table too.
const PINNED_UNITS: [(&str, usize, u64); 16] = [
    (
        "meridian_amphibious_transport.glb",
        237668,
        0x42a1d7791d9e5628,
    ),
    ("meridian_heavy_tank.glb", 312276, 0x6571e3f651b02903),
    ("meridian_light_transport.glb", 217992, 0xe9090bdeca18f309),
    ("meridian_repair_truck.glb", 242876, 0x323b82afb27c6e92),
    ("meridian_rifleman.glb", 481380, 0xa32633be9333cbe3),
    ("meridian_siege_artillery.glb", 308352, 0x456dd572a60a7d98),
    ("neutral_ferry.glb", 208584, 0xae63c9581a0b4ec6),
    ("neutral_supply_truck.glb", 287828, 0x9b6b7bb2c6c0de8e),
    ("veyran_gunboat.glb", 287372, 0x872372efed0ffc38),
    ("veyran_harvester.glb", 267852, 0x6fb9b5ebeee77ba1),
    ("veyran_heavy_gun_team.glb", 141028, 0xc2ab15873ad394ac),
    ("veyran_heavy_tank.glb", 325028, 0x14b055063773f3ce),
    ("veyran_repair_truck.glb", 240152, 0x1d7f23322dc1e450),
    ("veyran_rifleman.glb", 315192, 0xa392d905e41ed0a0),
    ("veyran_siege_artillery.glb", 307420, 0x5c33861f4163d63d),
    ("veyran_transport_truck.glb", 224872, 0xb84a74fd741234e9),
];

/// WP11.6d: the 14 decor and wreck files (4 wrecks, 2 rocks, a fence, a cliff edge, 4 ore field
/// stages, 2 broadleaf trees), first pinned on this branch. The files of the other tables kept
/// their bytes: the new `flora` palette entry and the `faction == flora` branch of
/// `Palette::materials` change no existing model (checked: `--check` against the earlier set).
const PINNED_DECOR: [(&str, usize, u64); 14] = [
    ("neutral_cliff_edge.glb", 169688, 0x108efa20ebdf6087),
    ("neutral_fence.glb", 78184, 0x995919e27c48e56f),
    ("neutral_ore_field_stage_01.glb", 64204, 0x41e683a6b3105b4b),
    ("neutral_ore_field_stage_02.glb", 89288, 0x59413127b3685582),
    ("neutral_ore_field_stage_03.glb", 126924, 0xfbe0a511e75a2e86),
    ("neutral_ore_field_stage_04.glb", 177088, 0xe0c50157f919eacc),
    ("neutral_rock_large.glb", 189340, 0x40f97822c06afd32),
    ("neutral_rock_small.glb", 118188, 0x80732670133e829c),
    ("neutral_tree_broadleaf.glb", 82252, 0xcac08d62e8a80cc7),
    (
        "neutral_tree_broadleaf_fallen.glb",
        85312,
        0xe9d55b946c4d58f9,
    ),
    (
        "neutral_wreck_heavy_vehicle.glb",
        166376,
        0xd405650e02d6a7de,
    ),
    (
        "neutral_wreck_light_vehicle.glb",
        142936,
        0xcc4dee4f6149cdd9,
    ),
    (
        "neutral_wreck_medium_vehicle.glb",
        219544,
        0x4bbb5dcfa38f1d06,
    ),
    ("neutral_wreck_vessel.glb", 146372, 0x0cf13dd80a6c0b04),
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
    // The decor and the wrecks of WP11.6d are pinned in their own table.
    assert_eq!(
        first.len(),
        PINNED.len() + PINNED_UNITS.len() + PINNED_DECOR.len()
    );
    for (name, bytes) in first {
        let (_, len, hash) = PINNED
            .iter()
            .chain(PINNED_UNITS.iter())
            .chain(PINNED_DECOR.iter())
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
