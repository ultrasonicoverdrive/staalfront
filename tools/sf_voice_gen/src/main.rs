//! `cargo run -p sf_voice_gen [-- [<dir>] [--check] [--table]]`: filter the Piper sources into the CC0 walkie-talkie unit voices in
//! `assets/audio/voice`, or with `--check` only compare them with what is there (exit code 1 when a file is
//! missing or out of date), or with `--table` print the line table (event, file, length, words).
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
                    "usage: sf_voice_gen [<output dir, default assets/audio/voice>] [--check] [--table]"
                );
                return ExitCode::SUCCESS;
            }
            other => dir = Some(PathBuf::from(other)),
        }
    }
    if table {
        for l in &sf_voice_gen::LINES {
            println!(
                "| `{}` | `{}` | {} s | \"{}\" |",
                l.event,
                l.file,
                f64::from(l.ms()) / 1000.0,
                l.text
            );
        }
        return ExitCode::SUCCESS;
    }
    let dir = dir.unwrap_or_else(|| PathBuf::from("assets/audio/voice"));
    let mut stale = 0;
    for (name, bytes) in sf_voice_gen::files() {
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
    if stale > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
