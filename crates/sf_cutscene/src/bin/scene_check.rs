//! `scene_check <scene.ron> [--assets DIR]`
//!
//! Loads a scene and prints what is wrong with it, with line numbers. Exit
//! status 0 = valid (warnings and missing assets are listed but not fatal),
//! 1 = invalid or unreadable, 2 = bad usage.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: scene_check <scene.ron> [--assets DIR]\n\
  --assets DIR  asset root for the missing-asset list (default: ./assets if it exists)";

fn main() -> ExitCode {
    let mut file: Option<String> = None;
    let mut assets: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--assets" => assets = args.next().map(PathBuf::from),
            _ if a.starts_with("--") || file.is_some() => {
                eprintln!("scene_check: unexpected argument {a}\n{USAGE}");
                return ExitCode::from(2);
            }
            _ => file = Some(a),
        }
    }
    let Some(file) = file else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let assets = assets.or_else(|| {
        let default = PathBuf::from("assets");
        default.is_dir().then_some(default)
    });

    match sf_cutscene::load_file(Path::new(&file), assets.as_deref()) {
        Ok(l) => {
            for w in &l.warnings {
                match w.line {
                    Some(n) => eprintln!("{file}:{n}: warning: {}", w.message),
                    None => eprintln!("{file}: warning: {}", w.message),
                }
            }
            for m in &l.missing_assets {
                match m.line {
                    Some(n) => eprintln!("{file}:{n}: note: missing {} ({})", m.file, m.used_by),
                    None => eprintln!("{file}: note: missing {} ({})", m.file, m.used_by),
                }
            }
            println!(
                "{file}: ok, {:.1} s, {} warnings, {} missing assets",
                f64::from(l.scene.duration_ms()) / 1000.0,
                l.warnings.len(),
                l.missing_assets.len()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
