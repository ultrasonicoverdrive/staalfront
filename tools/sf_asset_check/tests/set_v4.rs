//! The delivered set `assets/set_v4`, judged by the checker.
//!
//! Unlike set v3, this set is stored only if it passes: the six files of the
//! delivery that failed a rule of the art brief are left out (see
//! `docs/progress.md`), so no file in the folder may fail. Warnings are
//! findings for the producer (an emissive lamp material, vessels above the
//! waterline, ...) and do not keep a file out. The files are stored byte for
//! byte as delivered, so they keep the delivery's layout: one glTF scene per
//! level of detail, which the checker and the engine loader read as the three
//! roots of one scene.

use std::path::PathBuf;

use sf_asset_check::doc::inspect;
use sf_asset_check::{Checker, Status};
use sf_render::glb::LodLayout;

fn set_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/set_v4")
}

fn files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(set_dir())
        .expect("assets/set_v4 exists")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "glb"))
        .collect();
    files.sort();
    files
}

#[test]
fn no_file_of_the_set_fails_the_checker() {
    let files = files();
    assert_eq!(files.len(), 194, "194 of the 200 delivered models");
    let checker = Checker::embedded();
    let mut passing = 0;
    for file in &files {
        let report = checker.check_path(file);
        assert_ne!(
            report.status(),
            Status::Fail,
            "{}: {:#?}",
            report.file,
            report.findings
        );
        assert_eq!(report.facts.triangles.len(), 3, "{}", report.file);
        assert!(report.facts.class.is_some(), "{}", report.file);
        passing += usize::from(report.status() == Status::Pass);
    }
    // The delivery's own claim ("200 of 200 pass") does not hold for the
    // checker: most files carry a warning. Three are clean; if more become
    // clean this number goes up, and a drop means a rule got stricter.
    assert_eq!(passing, 3);
}

#[test]
fn the_files_keep_the_delivery_layout_of_one_scene_per_lod() {
    for file in files() {
        let doc = inspect(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(doc.layout, LodLayout::ScenePerLod, "{}", file.display());
        assert_eq!(doc.roots.len(), 3, "{}", file.display());
    }
}
