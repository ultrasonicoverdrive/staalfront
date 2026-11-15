//! `cargo run -p sf_music_gen [-- [<dir>] [--check] [--table]]`: write the CC0 music to
//! `assets/audio/music`, or with `--check` only compare the files with what is there (exit code 1 when a file
//! is missing or out of date), or with `--table` print the track table (name, file, BPM, key, length, size,
//! character). Deterministic: running it twice changes nothing.

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
                    "usage: sf_music_gen [<output dir, default assets/audio/music>] [--check] [--table]"
                );
                return ExitCode::SUCCESS;
            }
            other => dir = Some(PathBuf::from(other)),
        }
    }
    if table {
        let files = sf_music_gen::files();
        let mut total = 0;
        for (t, (_, bytes)) in sf_music_gen::TRACKS.iter().zip(&files) {
            let secs = sf_music_gen::samples_of(t) as f64 / f64::from(sf_music_gen::SAMPLE_RATE);
            total += bytes.len();
            println!(
                "| `{}` | `{}` | {} | {} | {:.1} s | {} bytes | {} |",
                t.bank_name,
                t.file,
                t.bpm,
                t.key,
                secs,
                bytes.len(),
                t.character
            );
        }
        println!(
            "total {total} bytes of a budget of {}",
            sf_music_gen::BUDGET_BYTES
        );
        return ExitCode::SUCCESS;
    }
    let dir = dir.unwrap_or_else(|| PathBuf::from("assets/audio/music"));
    let mut stale = 0;
    for (name, bytes) in sf_music_gen::files() {
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
            println!("wrote {} ({} bytes)", path.display(), bytes.len());
        }
    }
    if stale > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
