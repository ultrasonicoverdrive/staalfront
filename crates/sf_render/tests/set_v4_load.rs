//! Art set v4 (`assets/set_v4`, 194 models) through the engine's glb loader:
//! every file loads with the engine axes (front `+X`, up `+Y`, 1 m), reads
//! its levels of detail from one glTF scene each (the delivery's layout, see
//! `docs/conventions.md` section 12), merges its mesh nodes into parts, strips
//! its `_LOD<n>` suffixes and turns into a model set; and the `set_v4` pack of
//! `assets/config/models.ron` lists exactly the files of the folder. No GPU is
//! needed.
//!
//! What the delivery does or does not meet of the art brief is the business of
//! `tools/sf_asset_check`, not of this test.

use std::collections::BTreeSet;
use std::path::PathBuf;

use sf_render::glb::{self, AxisConvention, LoadedModel, LodLayout, PackFile};
use sf_render::model::{Joint, MAX_LODS, MAX_PARTS};

fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

/// The `.glb` file names of `assets/set_v4`, sorted.
fn files() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(assets().join("set_v4"))
        .expect("assets/set_v4 exists")
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".glb"))
        .collect();
    names.sort();
    names
}

fn load(file: &str) -> LoadedModel {
    glb::load_glb_file(&assets().join("set_v4").join(file), &AxisConvention::ENGINE)
        .unwrap_or_else(|e| panic!("{file}: {e}"))
}

#[test]
fn the_set_has_194_models_and_not_the_six_that_failed_the_checker() {
    let files = files();
    assert_eq!(files.len(), 194);
    // The delivery had 200; these six break rules of the art brief that the
    // checker measures (wrecks without fx nodes, two infantry teams whose
    // parts the engine cannot rig) and are not in the repo. docs/progress.md
    // lists them for the producer.
    for left_out in [
        "neutral_wreck_boat.glb",
        "neutral_wreck_heavy.glb",
        "neutral_wreck_light.glb",
        "neutral_wreck_medium.glb",
        "veyran_demolition_team.glb",
        "veyran_heavy_gun_team.glb",
    ] {
        assert!(!files.iter().any(|f| f == left_out), "{left_out}");
    }
}

#[test]
fn every_file_keeps_its_levels_in_one_scene_each() {
    // The delivery's layout, stored unchanged: `scenes` LOD0, LOD1 and LOD2,
    // one root each. If a later delivery changes it, the loader reads that too
    // (see the unit tests of `glb`); this pins what is in the repo.
    for file in files() {
        let bytes = std::fs::read(assets().join("set_v4").join(&file)).unwrap();
        let gltf = gltf::Gltf::from_slice(&bytes).unwrap();
        let (layout, roots) =
            glb::lod_roots(&gltf.document).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(layout, LodLayout::ScenePerLod, "{file}");
        let stem = file.strip_suffix(".glb").unwrap();
        let names: Vec<String> = roots
            .iter()
            .map(|r| r.name().unwrap_or("").to_owned())
            .collect();
        assert_eq!(
            names,
            (0..3).map(|n| format!("{stem}_LOD{n}")).collect::<Vec<_>>(),
            "{file}"
        );
    }
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
                // A part without geometry is a pivot (the `hull` of a figure,
                // whose torso hangs on it): it must carry another part.
                assert!(
                    !mesh.indices.is_empty() || level.parts.iter().any(|p| p.parent == Some(i)),
                    "{at}: part {} is empty and carries nothing",
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
fn tanks_artillery_and_gunboats_keep_their_turret_and_barrel_as_parts_that_move() {
    for file in files().iter().filter(|f| {
        ["_light_tank.glb", "_heavy_tank.glb", "_siege_artillery.glb"]
            .iter()
            .any(|s| f.ends_with(s))
            || f.as_str() == "veyran_gunboat.glb"
            || f.starts_with("network_unmanned_artillery")
    }) {
        let model = load(file);
        for level in &model.levels {
            let by_name = |name: &str| {
                level
                    .parts
                    .iter()
                    .position(|p| p.name == name)
                    .unwrap_or_else(|| panic!("{file}: no part {name}"))
            };
            let (hull, turret, barrel) = (by_name("hull"), by_name("turret"), by_name("barrel"));
            assert_eq!(level.parts[hull].parent, None, "{file}");
            assert_eq!(level.parts[turret].parent, Some(hull), "{file}");
            assert_eq!(level.parts[turret].joint, Joint::Yaw, "{file}");
            assert_eq!(level.parts[barrel].parent, Some(turret), "{file}");
            assert_eq!(level.parts[barrel].joint, Joint::Pitch, "{file}");
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
fn the_set_v4_pack_lists_the_files_of_the_folder_once_each_and_loads() {
    let config = std::fs::read_to_string(assets().join(glb::MODELS_CONFIG)).unwrap();
    let file: PackFile = ron::from_str(&config).unwrap();
    let spec = file
        .packs
        .get("set_v4")
        .expect("models.ron has the pack set_v4");
    assert_eq!(spec.dir, "set_v4");
    let listed: Vec<&str> = spec.models.iter().map(|m| m.file.as_str()).collect();
    let unique: BTreeSet<&str> = listed.iter().copied().collect();
    assert_eq!(unique.len(), listed.len(), "a file is listed twice");
    let on_disk = files();
    assert_eq!(
        unique,
        on_disk.iter().map(String::as_str).collect::<BTreeSet<_>>(),
        "the pack and the folder differ"
    );
    // The pack loads and becomes a model set (what `sf_game --models set_v4` does).
    let pack = glb::load_pack(&config, "set_v4", &assets()).expect("pack loads");
    assert_eq!(pack.models.len(), 194);
    assert_eq!(pack.icons.len(), 194);
    assert_eq!(pack.set.models().len(), 194);
}
