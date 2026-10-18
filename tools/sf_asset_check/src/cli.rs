//! The command line: arguments in, report text out, exit code back.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::check::Checker;
use crate::report::{self, Totals};

/// Exit code: everything passed (warnings allowed unless `--deny-warnings`).
pub const EXIT_OK: u8 = 0;
/// Exit code: at least one file failed a rule.
pub const EXIT_FAILED: u8 = 1;
/// Exit code: the arguments or the paths are unusable.
pub const EXIT_USAGE: u8 = 2;

const HELP: &str = "\
sf_asset_check: checks .glb models against the art brief.

usage: sf_asset_check [options] <path>...

  <path>            a .glb file, or a folder (its .glb files, not recursive)
  -q, --quiet       only print files that have findings
  --format <f>      text (default) or markdown (a table plus the findings)
  --deny-warnings   warnings also make the exit code 1
  -h, --help        this text

exit code: 0 = no failures, 1 = a file broke a rule, 2 = bad arguments

Per file: the LOD roots <id>_LOD0..2 and their identical node structure and
origin; triangles per LOD against the budget of the file's class (infantry
1500/250/60, two-figure 2x that, vehicle, decor and wreck 1500/500/150,
building 5000/1500/300; the class comes from the role in unit_defs.ron);
at most 6 materials, base colour only; `team` present for factions and
absent for neutral objects, 20-40 % of the top-down view of LOD0; the lowest
point at Y = 0 (a vessel: the waterline); the node names of the class; the
orientation (vehicles longer along X than Z, infantry upright).
";

/// Run with `args` (without the program name). Reports go to `out`, problems
/// with the arguments to `err`.
pub fn run_cli(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let mut quiet = false;
    let mut markdown = false;
    let mut deny_warnings = false;
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                let _ = write!(out, "{HELP}");
                return EXIT_OK;
            }
            "-q" | "--quiet" => quiet = true,
            "--deny-warnings" => deny_warnings = true,
            "--format" => match it.next().map(String::as_str) {
                Some("text") => markdown = false,
                Some("markdown") => markdown = true,
                other => {
                    let _ = writeln!(
                        err,
                        "sf_asset_check: --format needs text or markdown, got {other:?}"
                    );
                    return EXIT_USAGE;
                }
            },
            flag if flag.starts_with('-') => {
                let _ = writeln!(err, "sf_asset_check: unknown option {flag} (try --help)");
                return EXIT_USAGE;
            }
            path => paths.push(PathBuf::from(path)),
        }
    }
    if paths.is_empty() {
        let _ = writeln!(
            err,
            "sf_asset_check: give at least one file or folder (try --help)"
        );
        return EXIT_USAGE;
    }
    let mut files = Vec::new();
    for path in &paths {
        match expand(path) {
            Ok(found) => files.extend(found),
            Err(e) => {
                let _ = writeln!(err, "sf_asset_check: {e}");
                return EXIT_USAGE;
            }
        }
    }
    let checker = Checker::embedded();
    let reports: Vec<_> = files.iter().map(|f| checker.check_path(f)).collect();
    let text = if markdown {
        report::markdown(&reports)
    } else {
        report::text(&reports, quiet)
    };
    let _ = write!(out, "{text}");
    let totals = Totals::of(&reports);
    if totals.fail > 0 || (deny_warnings && totals.warn > 0) {
        EXIT_FAILED
    } else {
        EXIT_OK
    }
}

/// A file as itself, a folder as its `.glb` files by name.
fn expand(path: &Path) -> Result<Vec<PathBuf>, String> {
    if path.is_dir() {
        let mut files: Vec<PathBuf> = std::fs::read_dir(path)
            .map_err(|e| format!("cannot read folder {}: {e}", path.display()))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("glb")))
            .collect();
        files.sort();
        if files.is_empty() {
            return Err(format!("no .glb files in {}", path.display()));
        }
        Ok(files)
    } else if path.is_file() {
        Ok(vec![path.to_path_buf()])
    } else {
        Err(format!("{} is neither a file nor a folder", path.display()))
    }
}
