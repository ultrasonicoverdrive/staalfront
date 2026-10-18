//! `cargo run -p sf_icon_gen [-- <dir>] [--check]`: write the placeholder icons
//! (and `icons.ron`) to `assets/icons`, or with `--check` only compare them
//! with what is there. Deterministic: running it twice changes nothing.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut check = false;
    let mut dir = PathBuf::from("assets/icons");
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--check" => check = true,
            "-h" | "--help" => {
                println!("usage: sf_icon_gen [<output dir, default assets/icons>] [--check]");
                return ExitCode::SUCCESS;
            }
            other => dir = PathBuf::from(other),
        }
    }
    let mut stale = 0;
    for (name, bytes) in sf_icon_gen::files() {
        let path = dir.join(&name);
        if check {
            let same = std::fs::read(&path).is_ok_and(|on_disk| {
                if name.ends_with(".png") {
                    match (
                        sf_icon_gen::decode_png(&on_disk),
                        sf_icon_gen::decode_png(&bytes),
                    ) {
                        (Ok(a), Ok(b)) => a == b,
                        _ => false,
                    }
                } else {
                    on_disk == bytes
                }
            });
            if !same {
                println!("out of date: {}", path.display());
                stale += 1;
            }
        } else {
            if let Err(e) =
                std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, &bytes))
            {
                eprintln!("{}: {e}", path.display());
                return ExitCode::FAILURE;
            }
            println!("wrote {}", path.display());
        }
    }
    if stale > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
