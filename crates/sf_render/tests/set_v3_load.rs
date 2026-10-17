//! The third art delivery (`assets/set_v3`, 30 models) through the engine's
//! glb loader: every file loads with the engine axes (front `+X`, up `+Y`,
//! 1 m), merges its mesh nodes into parts, strips its `_LOD<n>` suffixes and
//! turns into a model set; and the `set_v3` pack of `assets/config/models.ron`
//! lists exactly the files of the folder. No GPU is needed.
//!
//! What the delivery does or does not meet of the art brief is the business of
//! `tools/sf_asset_check`, not of this test.

use std::collections::BTreeSet;
use std::path::PathBuf;

use sf_render::glb::{self, AxisConvention, LoadedModel, PackFile};
use sf_render::model::{Joint, MAX_LODS, MAX_PARTS};

fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

/// The `.glb` file names of `assets/set_v3`, sorted.
fn files() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(assets().join("set_v3"))
        .expect("assets/set_v3 exists")
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".glb"))
        .collect();
    names.sort();
    names
}

fn load(file: &str) -> LoadedModel {
    glb::load_glb_file(&assets().join("set_v3").join(file), &AxisConvention::ENGINE)
        .unwrap_or_else(|e| panic!("{file}: {e}"))
}

#[test]
fn the_delivery_has_thirty_models() {
    assert_eq!(files().len(), 30);
}

#[test]
fn every_model_loads_merges_its_parts_and_strips_the_lod_suffixes() {
    for file in files() {
        let model = load(&file);
        let stem = file.strip_suffix(".glb").unwrap();
        assert_eq!(model.name, stem, "{file}: the root name without _LOD<n>");
        assert_eq!(model.levels.len(), 3, "{file}: LOD0 to LOD2");
        assert!(model.levels.len() <= MAX_LODS);
        for (n, level) in model.levels.iter().enumerate() {
            let at = format!("{file} LOD{n}");
            assert!(
                (1..=MAX_PARTS).contains(&level.parts.len()),
                "{at}: {} parts",
                level.parts.len()
            );
            for (i, part) in level.parts.iter().enumerate() {
                assert!(!part.name.contains("_LOD"), "{at}: part {:?}", part.name);
                assert!(
                    part.parent.is_none_or(|p| p < i),
                    "{at}: parents come first"
                );
                let mesh = &part.mesh;
                assert!(
                    !mesh.indices.is_empty(),
                    "{at}: part {} is empty",
                    part.name
                );
                assert_eq!(mesh.indices.len() % 3, 0, "{at}");
                assert!(
                    mesh.indices
                        .iter()
                        .all(|&i| (i as usize) < mesh.vertices.len()),
                    "{at}: index out of range"
                );
                for v in &mesh.vertices {
                    assert!(v.position.iter().all(|c| c.is_finite()), "{at}");
                    // A unit normal, or zero on a zero-area triangle (the
                    // generator writes those; `sf_asset_check` reports them).
                    let len = v.normal.iter().map(|c| c * c).sum::<f32>().sqrt();
                    assert!(
                        len < 1e-3 || (len - 1.0).abs() < 1e-3,
                        "{at}: normal length {len}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_mesh_nodes_of_a_figure_merge_into_one_part() {
    // Torso, head, arms and legs are mesh nodes without children: the loader
    // folds them into one `body` part per level. (Separate limbs would need
    // them to be parts of their own, which the engine does not rig yet.)
    for file in files().iter().filter(|f| {
        [
            "rifleman",
            "rocket_trooper",
            "engineer",
            "scout",
            "sniper",
            "medic",
            "demolition_team",
            "civilian",
            "technician",
        ]
        .iter()
        .any(|r| f.contains(r))
    }) {
        let model = load(file);
        for level in &model.levels {
            let names: Vec<&str> = level.parts.iter().map(|p| p.name.as_str()).collect();
            assert_eq!(names, ["body"], "{file}");
        }
    }
}

#[test]
fn a_wreck_keeps_its_turret_as_a_part_that_turns() {
    // `hull` has a mesh and a child, `turret` too: both become rigged parts.
    // The `barrel` of these wrecks is a mesh node without children, which the
    // loader folds into the turret (no separate part to tilt); the checker
    // reports that as `engine-parts`.
    for file in [
        "neutral_wreck_medium_vehicle.glb",
        "neutral_wreck_vessel.glb",
    ] {
        let model = load(file);
        for level in &model.levels {
            let by_name = |name: &str| {
                level
                    .parts
                    .iter()
                    .position(|p| p.name == name)
                    .unwrap_or_else(|| panic!("{file}: no part {name}"))
            };
            let (hull, turret) = (by_name("hull"), by_name("turret"));
            assert_eq!(level.parts[turret].parent, Some(hull), "{file}");
            assert_eq!(level.parts[turret].joint, Joint::Yaw, "{file}");
            assert!(
                level.parts.iter().all(|p| p.name != "barrel"),
                "{file}: the barrel was folded into the turret"
            );
        }
    }
}

#[test]
fn faction_models_carry_the_team_colour_and_neutral_ones_do_not() {
    for file in files() {
        let model = load(&file);
        let team: usize = model.levels[0]
            .parts
            .iter()
            .flat_map(|p| &p.mesh.vertices)
            .filter(|v| v.color[3] >= 0.5)
            .count();
        let faction = !file.starts_with("neutral_");
        assert_eq!(team > 0, faction, "{file}: {team} team vertices in LOD0");
    }
}

#[test]
fn the_set_v3_pack_lists_the_files_of_the_folder_once_each_and_loads() {
    let config = std::fs::read_to_string(assets().join(glb::MODELS_CONFIG)).unwrap();
    let file: PackFile = ron::from_str(&config).unwrap();
    let spec = file
        .packs
        .get("set_v3")
        .expect("models.ron has the pack set_v3");
    assert_eq!(spec.dir, "set_v3");
    let listed: Vec<&str> = spec.models.iter().map(|m| m.file.as_str()).collect();
    let unique: BTreeSet<&str> = listed.iter().copied().collect();
    assert_eq!(unique.len(), listed.len(), "a file is listed twice");
    let on_disk = files();
    assert_eq!(
        unique,
        on_disk.iter().map(String::as_str).collect::<BTreeSet<_>>(),
        "the pack and the folder differ"
    );
    // The pack loads and becomes a model set (what `sf_game --models set_v3` does).
    let pack = glb::load_pack(&config, "set_v3", &assets()).expect("pack loads");
    assert_eq!(pack.models.len(), 30);
    assert_eq!(pack.icons.len(), 30);
    assert_eq!(pack.set.models().len(), 30);
}
