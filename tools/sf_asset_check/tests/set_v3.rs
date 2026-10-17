//! The delivered set `assets/set_v3`, judged by the checker.
//!
//! This does not demand that the set passes: the set is Tijmen's delivery and
//! its rule findings are reported to him (see the PR that added it). It pins
//! what must hold for the engine and the checker to work with the files at
//! all: every file is a valid glb the engine loader accepts, has a name and a
//! budget class, and stays within the triangle budget (raised to 1500/250/60
//! for infantry).

use std::path::PathBuf;

use sf_asset_check::Checker;

fn set_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/set_v3")
}

#[test]
fn every_file_of_the_set_is_loadable_classified_and_within_budget() {
    let mut files: Vec<PathBuf> = std::fs::read_dir(set_dir())
        .expect("assets/set_v3 exists")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "glb"))
        .collect();
    files.sort();
    assert_eq!(files.len(), 30, "the delivery has 30 models");
    let checker = Checker::embedded();
    for file in &files {
        let report = checker.check_path(file);
        for rule in [
            "io",
            "glb",
            "engine-load",
            "file-name",
            "class",
            "lod-roots",
            "budget",
            "materials",
            "textures",
        ] {
            assert!(
                !report.failed(rule),
                "{}: {rule}: {:#?}",
                report.file,
                report.findings
            );
        }
        assert_eq!(report.facts.triangles.len(), 3, "{}", report.file);
        assert!(report.facts.class.is_some(), "{}", report.file);
    }
}
