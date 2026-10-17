//! `fountain2scene <in.fountain> <out.ron> [--name NAME] [--assets DIR]`
//!
//! Converts a Fountain briefing into a skeleton scene RON (see
//! `sf_cutscene::fountain` and `docs/cutscene-format.md`), then reads the
//! result back through the scene loader. Exit status 0 = converted and valid
//! (warnings and missing assets are information), 1 = the result is invalid,
//! 2 = bad usage or a file could not be read or written.

use std::path::PathBuf;
use std::process::ExitCode;

use sf_cutscene::fountain::convert_file;

const USAGE: &str = "usage: fountain2scene <in.fountain> <out.ron> [--name NAME] [--assets DIR]\n\
  --name NAME   scene name (default: the output file's name, lower-cased)\n\
  --assets DIR  asset root for the missing-asset list (default: ./assets if it exists)";

fn main() -> ExitCode {
    let mut positional: Vec<String> = Vec::new();
    let mut name: Option<String> = None;
    let mut assets: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--name" => name = args.next(),
            "--assets" => assets = args.next().map(PathBuf::from),
            _ if a.starts_with("--") => {
                eprintln!("fountain2scene: unknown option {a}\n{USAGE}");
                return ExitCode::from(2);
            }
            _ => positional.push(a),
        }
    }
    let [input, output] = positional.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let assets = assets.or_else(|| {
        let default = PathBuf::from("assets");
        default.is_dir().then_some(default)
    });

    let report = match convert_file(
        input.as_ref(),
        output.as_ref(),
        name.as_deref(),
        assets.as_deref(),
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("fountain2scene: {e}");
            return ExitCode::from(2);
        }
    };
    for w in &report.warnings {
        eprintln!("{input}:{}: warning: {}", w.line, w.message);
    }
    for m in &report.missing_assets {
        eprintln!("{output}: note: {m}");
    }
    for e in &report.errors {
        eprintln!("{e}");
    }
    println!(
        "{output}: {} events in {} tracks, {:.1} s; {} warnings, {} missing assets (placeholders are fine){}",
        report.events,
        report.tracks,
        f64::from(report.duration_ms) / 1000.0,
        report.warnings.len(),
        report.missing_assets.len(),
        if report.errors.is_empty() {
            String::new()
        } else {
            format!("; INVALID ({} errors)", report.errors.len())
        }
    );
    if report.errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
