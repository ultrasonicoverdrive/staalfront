//! The fence of `docs/rules/ai.md`: `observe.rs` is the only file of this crate that names the simulation's
//! world. A source test, like the other source lints of the repository: it reads the files of `src/` and fails
//! when the type's name (or a way to reach it without naming it) shows up anywhere else, comments included.

use std::path::{Path, PathBuf};

/// The one file allowed to name the world.
const FENCE: &str = "observe.rs";

/// Why `text` reaches the simulation's world, one entry per kind of hit; empty when it does not.
fn reaches_world(text: &str) -> Vec<&'static str> {
    let mut hits = Vec::new();
    let bytes = text.as_bytes();
    let word = "World";
    let mut from = 0;
    while let Some(at) = text[from..].find(word) {
        let start = from + at;
        let end = start + word.len();
        let before =
            start == 0 || !(bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
        let after =
            end >= bytes.len() || !(bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_');
        if before && after {
            hits.push("the name World");
            break;
        }
        from = end;
    }
    if text.contains("sf_sim::world") {
        hits.push("the module sf_sim::world");
    }
    if text.contains("sf_sim::*") {
        hits.push("a glob import of sf_sim, which brings the world into scope unnamed");
    }
    hits
}

fn sources() -> Vec<(PathBuf, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).unwrap();
            out.push((path, text));
        }
    }
    out.sort();
    out
}

#[test]
fn the_checker_finds_what_it_is_meant_to_find() {
    assert!(!reaches_world("fn f(w: &World) {}").is_empty());
    assert!(!reaches_world("use sf_sim::{Cell, World};").is_empty());
    assert!(!reaches_world("World").is_empty());
    assert!(!reaches_world("use sf_sim::world::StepReport;").is_empty());
    assert!(!reaches_world("use sf_sim::*;").is_empty());
    // A word that only contains the name does not count.
    assert!(reaches_world("let hello_World_x = 1; let Worlds = 2; let SubWorld = 3;").is_empty());
    assert!(reaches_world("let world = 1; // the world").is_empty());
}

#[test]
fn only_the_fence_file_names_the_world() {
    let files = sources();
    assert!(
        files.iter().any(|(p, _)| p.ends_with(FENCE)),
        "{FENCE} is gone: the fence moved, update this test and docs/rules/ai.md"
    );
    let mut leaks = Vec::new();
    for (path, text) in &files {
        if path.ends_with(FENCE) {
            continue;
        }
        for hit in reaches_world(text) {
            leaks.push(format!("{}: {hit}", path.display()));
        }
    }
    assert!(
        leaks.is_empty(),
        "only observe.rs may name the simulation's world; the rest of sf_ai takes an Observation:\n{}",
        leaks.join("\n")
    );
}

#[test]
fn the_fence_file_does_name_the_world() {
    // Guards the test above against passing because nothing is read.
    let files = sources();
    let (_, text) = files.iter().find(|(p, _)| p.ends_with(FENCE)).unwrap();
    assert!(!reaches_world(text).is_empty());
}
