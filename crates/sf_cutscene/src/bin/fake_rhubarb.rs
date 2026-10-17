//! A stand-in for the `rhubarb` program, for the tests of the `lipsync` tool.
//! **Not a tool**: it only exists so that the tests can run a real program on
//! every operating system (a shell script would not run on Windows).
//!
//! It understands the options `lipsync` passes (`-f json|tsv`, `-o FILE`,
//! `-d FILE`, `-r NAME`, `-q`, then the sound file) and behaves as the
//! environment says:
//!
//! * `FAKE_RHUBARB_LOG=FILE`: append the arguments (and the text of `-d`) to FILE.
//! * `FAKE_RHUBARB_MODE`: unset or `ok` writes a short, valid cue file in the
//!   requested format; `fail` writes half a file and exits 1 with a message;
//!   `silent` writes nothing and exits 0; `garbage` writes a cue file with an
//!   unknown shape and exits 0.

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut out, mut format, mut dialog) = (None, String::from("tsv"), None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" => out = it.next().cloned(),
            "-f" => format = it.next().cloned().unwrap_or_default(),
            "-d" => dialog = it.next().cloned(),
            "-r" => {
                it.next();
            }
            _ => {}
        }
    }
    if let Ok(log) = std::env::var("FAKE_RHUBARB_LOG")
        && let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
    {
        let _ = writeln!(f, "{}", args.join(" "));
        if let Some(d) = &dialog {
            let text = std::fs::read_to_string(d).unwrap_or_default();
            let _ = writeln!(f, "dialog: {text}");
        }
    }
    let Some(out) = out else {
        eprintln!("fake rhubarb: no -o");
        return ExitCode::from(2);
    };
    let write = |text: &str| std::fs::write(&out, text).is_ok();
    match std::env::var("FAKE_RHUBARB_MODE").as_deref() {
        Ok("fail") => {
            write("{ \"mouthCues\": [ { \"start\": 0.0, ");
            eprintln!("cannot read the sound file");
            ExitCode::from(1)
        }
        Ok("silent") => ExitCode::SUCCESS,
        Ok("garbage") => {
            write("0.00\tX\n0.20\tQ\n");
            ExitCode::SUCCESS
        }
        _ => {
            let text = if format == "json" {
                "{ \"mouthCues\": [ { \"start\": 0.0, \"end\": 0.5, \"value\": \"X\" }, { \"start\": 0.5, \"end\": 1.0, \"value\": \"D\" } ] }"
            } else {
                "0.00\tX\n0.50\tD\n1.00\tX\n"
            };
            if write(text) {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
    }
}
