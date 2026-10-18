//! `cargo run -p sf_icon_gen [-- [--cursors] [<dir>] [--check]]`: write the
//! placeholder strategic icons (and `icons.ron`) to `assets/icons`, or with
//! `--cursors` the placeholder mouse cursors (and `cursors.ron`) to
//! `assets/ui/cursors`; with `--check` only compare them with what is there.
//! Deterministic: running it twice changes nothing.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut check = false;
    let mut cursors = false;
    let mut dir = None;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--check" => check = true,
            "--cursors" => cursors = true,
            "-h" | "--help" => {
                println!(
                    "usage: sf_icon_gen [--cursors] [<output dir, default assets/icons, \
                     or assets/ui/cursors with --cursors>] [--check]"
                );
                return ExitCode::SUCCESS;
            }
            other => dir = Some(PathBuf::from(other)),
        }
    }
    let (files, side) = if cursors {
        (
            sf_icon_gen::cursors::files(),
            sf_icon_gen::cursors::SIDE as u32,
        )
    } else {
        (sf_icon_gen::files(), sf_icon_gen::SIDE as u32)
    };
    let dir = dir.unwrap_or_else(|| {
        PathBuf::from(if cursors {
            "assets/ui/cursors"
        } else {
            "assets/icons"
        })
    });
    let mut stale = 0;
    for (name, bytes) in files {
        let path = dir.join(&name);
        if check {
            let same = std::fs::read(&path).is_ok_and(|on_disk| {
                if name.ends_with(".png") {
                    match (
                        sf_icon_gen::decode_png_sized(&on_disk, side),
                        sf_icon_gen::decode_png_sized(&bytes, side),
                    ) {
                        (Ok(a), Ok(b)) => a == b,
                        _ => false,
                    }
                } else {
                    sf_icon_gen::same_text(&on_disk, &bytes)
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
