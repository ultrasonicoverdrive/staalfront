//! `cargo run -p sf_music_gen [-- [<dir>] [--check] [--table]]`: write the CC0 music to
//! `assets/audio/music`, or with `--check` only compare the files with what is there (exit code 1 when a file
//! is missing or out of date), or with `--table` print the track table (name, file, BPM, key, length, size,
//! character). Deterministic: running it twice changes nothing.
//!
//! `--import <VSCO 2 CE directory>` is the one-off step that makes the committed samples in
//! `tools/sf_music_gen/samples/` from the recordings (WP11.4c); it is not part of CI and the render never needs
//! the recordings. The directory is only read.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut check = false;
    let mut table = false;
    let mut dir = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--import" => {
                let Some(from) = args.next() else {
                    eprintln!("--import needs the VSCO 2 CE directory");
                    return ExitCode::FAILURE;
                };
                return import(&PathBuf::from(from));
            }
            "--check" => check = true,
            "--table" => table = true,
            "-h" | "--help" => {
                println!(
                    "usage: sf_music_gen [<output dir, default assets/audio/music>] [--check] [--table] | --import <VSCO 2 CE dir>"
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

/// Make `tools/sf_music_gen/samples/*.wav` from the VSCO recordings (read only).
fn import(from: &std::path::Path) -> ExitCode {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("samples");
    if let Err(e) = std::fs::create_dir_all(&out) {
        eprintln!("{}: {e}", out.display());
        return ExitCode::FAILURE;
    }
    let mut total = 0;
    for s in &sf_music_gen::orchestra::SAMPLES {
        let src = from.join(s.src);
        let bytes = match std::fs::read(&src) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{}: {e}", src.display());
                return ExitCode::FAILURE;
            }
        };
        let wav = match sf_music_gen::sampler::import(&bytes, s.keep_ms, s.fade_ms) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("{}: {}", src.display(), e.0);
                return ExitCode::FAILURE;
            }
        };
        let dst = out.join(format!("{}.wav", s.id));
        if let Err(e) = std::fs::write(&dst, &wav) {
            eprintln!("{}: {e}", dst.display());
            return ExitCode::FAILURE;
        }
        total += wav.len();
        println!("{} <- {} ({} bytes)", dst.display(), s.src, wav.len());
    }
    println!("total {total} bytes");
    ExitCode::SUCCESS
}
