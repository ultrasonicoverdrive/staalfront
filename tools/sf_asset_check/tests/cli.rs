//! The command line: folders, exit codes, output formats.

mod common;

use std::path::PathBuf;

use common::{decor, infantry, wreck};
use sf_asset_check::cli::{EXIT_FAILED, EXIT_OK, EXIT_USAGE, run_cli};

/// A scratch folder that removes itself.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("sf_asset_check_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn put(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path.display().to_string()
    }

    fn dir(&self) -> String {
        self.0.display().to_string()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> (u8, String, String) {
    let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_cli(&args, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// Two good files and one over the budget, plus a file that is not a model.
fn mixed_folder(tag: &str) -> Scratch {
    let s = Scratch::new(tag);
    s.put(
        "meridian_rifleman.glb",
        &infantry("meridian_rifleman", 0.30).bytes(),
    );
    s.put("neutral_tree_pine.glb", &decor("neutral_tree_pine").bytes());
    let mut over = infantry("meridian_scout", 0.30);
    over.part_mut(0, "torso").reps = 130;
    s.put("meridian_scout.glb", &over.bytes());
    s.put("notes.txt", b"not a model, ignored");
    s
}

#[test]
fn a_folder_is_checked_in_name_order_and_a_failure_sets_the_exit_code() {
    let s = mixed_folder("folder");
    let (code, out, err) = run(&[&s.dir()]);
    assert_eq!(code, EXIT_FAILED, "{err}");
    let order: Vec<usize> = ["meridian_rifleman", "meridian_scout", "neutral_tree_pine"]
        .iter()
        .map(|n| {
            out.find(&format!("  {n}.glb"))
                .unwrap_or_else(|| panic!("{n} in {out}"))
        })
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{out}");
    assert!(out.contains("FAIL  meridian_scout.glb"), "{out}");
    assert!(out.contains("budget: LOD0 has 1620 triangles"), "{out}");
    assert!(!out.contains("notes.txt"));
    assert!(
        out.contains("3 files: 2 pass, 0 with warnings only, 1 fail"),
        "{out}"
    );
}

#[test]
fn files_can_be_named_one_by_one() {
    let s = mixed_folder("files");
    let good = format!("{}/meridian_rifleman.glb", s.dir());
    let (code, out, _) = run(&[&good]);
    assert_eq!(code, EXIT_OK);
    assert!(out.contains("PASS  meridian_rifleman.glb"), "{out}");
    assert!(out.contains("1 files: 1 pass"), "{out}");
}

#[test]
fn quiet_leaves_out_the_files_without_findings() {
    let s = mixed_folder("quiet");
    let (_, out, _) = run(&["-q", &s.dir()]);
    assert!(out.contains("meridian_scout.glb"));
    assert!(!out.contains("meridian_rifleman.glb"), "{out}");
    assert!(
        out.contains("3 files"),
        "the totals still count all of them"
    );
}

#[test]
fn markdown_is_a_table_and_a_list_of_findings() {
    let s = mixed_folder("markdown");
    let (_, out, _) = run(&["--format", "markdown", &s.dir()]);
    assert!(out.starts_with("| file | class |"), "{out}");
    assert!(
        out.contains("| `meridian_rifleman.glb` | infantry | 72/72/24 | 1500/250/60 |"),
        "{out}"
    );
    assert!(
        out.contains("- `meridian_scout.glb`\n  - **FAIL** `budget`:"),
        "{out}"
    );
}

#[test]
fn warnings_fail_the_run_only_with_deny_warnings() {
    let s = Scratch::new("warn");
    // A figure sunk 10 cm into the ground: a warning, no failure.
    let mut m = infantry("meridian_rifleman", 0.30);
    for lod in &mut m.lods {
        lod.origin = [0.0, -0.1, 0.0];
    }
    s.put("meridian_rifleman.glb", &m.bytes());
    s.put(
        "neutral_wreck_vessel.glb",
        &wreck("neutral_wreck_vessel").bytes(),
    );
    let (code, out, _) = run(&[&s.dir()]);
    assert_eq!(code, EXIT_OK, "{out}");
    assert!(out.contains("WARN  meridian_rifleman.glb"), "{out}");
    assert_eq!(run(&["--deny-warnings", &s.dir()]).0, EXIT_FAILED);
}

#[test]
fn bad_arguments_are_exit_code_2_with_a_message() {
    let s = Scratch::new("usage");
    for args in [
        vec![],
        vec!["--nonsense"],
        vec!["--format"],
        vec!["--format", "xml", "x"],
        vec!["/nonexistent/path.glb"],
    ] {
        let (code, out, err) = run(&args);
        assert_eq!(code, EXIT_USAGE, "{args:?}");
        assert!(out.is_empty(), "{args:?}: {out}");
        assert!(err.starts_with("sf_asset_check:"), "{args:?}: {err}");
    }
    // A folder without .glb files.
    s.put("readme.txt", b"x");
    let (code, _, err) = run(&[&s.dir()]);
    assert_eq!(code, EXIT_USAGE);
    assert!(err.contains("no .glb files"), "{err}");
}

#[test]
fn help_says_how_to_use_it() {
    let (code, out, _) = run(&["--help"]);
    assert_eq!(code, EXIT_OK);
    assert!(out.contains("usage: sf_asset_check") && out.contains("--deny-warnings"));
}
