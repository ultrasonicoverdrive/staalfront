//! `cargo run -p sf_model_gen [-- [<dir>] [--check] [--table]]`: write the models of set v5 to
//! `assets/set_v5`, or with `--check` only compare them with what is there (exit code 1 when a
//! file is missing or out of date), or with `--table` print the triangles per level.
//! Deterministic: running it twice changes nothing.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut check = false;
    let mut table = false;
    let mut dir = None;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--check" => check = true,
            "--table" => table = true,
            "-h" | "--help" => {
                println!(
                    "usage: sf_model_gen [<output dir, default assets/set_v5>] [--check] [--table]"
                );
                return ExitCode::SUCCESS;
            }
            other => dir = Some(PathBuf::from(other)),
        }
    }
    if table {
        for build in sf_model_gen::models::MODELS {
            let m = build();
            let t = m.triangles();
            println!("| `{}.glb` | {} | {} | {} |", m.stem, t[0], t[1], t[2]);
        }
        return ExitCode::SUCCESS;
    }
    let dir = dir.unwrap_or_else(|| PathBuf::from("assets/set_v5"));
    let mut stale = 0;
    for (name, bytes) in sf_model_gen::files() {
        let path = dir.join(&name);
        if check {
            if std::fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
                println!("out of date: {}", path.display());
                stale += 1;
            }
        } else if let Err(e) =
            std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, &bytes))
        {
            eprintln!("{}: {e}", path.display());
            return ExitCode::FAILURE;
        } else {
            println!("wrote {}", path.display());
        }
    }
    // The restyled cutscene parts go under assets/ (only with the default output folder).
    if dir.as_path() == std::path::Path::new("assets/set_v5") {
        for (rel, bytes) in sf_model_gen::cutout_files() {
            let path = PathBuf::from("assets").join(&rel);
            if check {
                if std::fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
                    println!("out of date: {}", path.display());
                    stale += 1;
                }
            } else if let Err(e) = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&path, &bytes))
            {
                eprintln!("{}: {e}", path.display());
                return ExitCode::FAILURE;
            }
        }
    }
    if stale > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
