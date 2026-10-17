//! `lipsync <voice.wav> [--dialog TEXT] -o <out.json>` and
//! `lipsync --batch <folder> [-o <out-folder>]`
//!
//! Turns recordings into mouth-shape files by running Rhubarb Lip Sync
//! (an external program, MIT licence; not included and not downloaded), then
//! reads the result back to check it. See `docs/cutscene-format.md`, section 11.
//!
//! Exit status: 0 = done, 1 = Rhubarb failed or wrote something unusable,
//! 2 = bad usage or an input that does not exist, 3 = Rhubarb was not found
//! (the install instructions are printed).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sf_cutscene::lipsync::parse_cues;
use sf_cutscene::rhubarb::{self, Dialog, Format, Job};

const USAGE: &str = "usage: lipsync <voice.wav> [-o <out.json>] [--dialog TEXT | --dialog-file FILE]\n\
\x20      lipsync --batch <folder-of-wavs> [-o <out-folder>] [--dialog-dir <folder-of-txt>] [--format json|tsv]\n\
\n\
  -o PATH             where the mouth cues go (default: next to the WAV, .json);\n\
                      a .tsv name writes Rhubarb's TSV instead\n\
  --dialog TEXT       what is said; makes Rhubarb's result better\n\
  --dialog-file FILE  the same, in a text file\n\
  --batch FOLDER      every *.wav in FOLDER, each named by its line id (l017.wav -> l017.json)\n\
  --dialog-dir DIR    with --batch: DIR/<id>.txt is the text of that line, when it exists\n\
  --format json|tsv   with --batch: the file type (default json)\n\
  --recognizer NAME   pocketSphinx (English, default) or phonetic (any language)\n\
  --rhubarb PATH      the rhubarb program, when it is not on PATH\n\
\n\
Rhubarb Lip Sync must be installed separately; without it this prints how.";

#[derive(Default)]
struct Options {
    input: Option<PathBuf>,
    batch: Option<PathBuf>,
    out: Option<PathBuf>,
    dialog: Option<String>,
    dialog_file: Option<PathBuf>,
    dialog_dir: Option<PathBuf>,
    format: Option<Format>,
    recognizer: Option<String>,
    rhubarb: Option<PathBuf>,
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("lipsync: {message}\n{USAGE}");
    ExitCode::from(2)
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Option<Options>, String> {
    let mut o = Options::default();
    let mut args = args;
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "-h" | "--help" => return Ok(None),
            "-o" | "--out" => o.out = Some(value("-o")?.into()),
            "--dialog" => o.dialog = Some(value("--dialog")?),
            "--dialog-file" => o.dialog_file = Some(value("--dialog-file")?.into()),
            "--dialog-dir" => o.dialog_dir = Some(value("--dialog-dir")?.into()),
            "--batch" => o.batch = Some(value("--batch")?.into()),
            "--recognizer" => o.recognizer = Some(value("--recognizer")?),
            "--rhubarb" => o.rhubarb = Some(value("--rhubarb")?.into()),
            "--format" => {
                let v = value("--format")?;
                o.format = Some(
                    Format::parse(&v)
                        .ok_or_else(|| format!("--format {v}: the formats are json and tsv"))?,
                );
            }
            _ if a.starts_with('-') && a.len() > 1 => return Err(format!("unknown option {a}")),
            _ if o.input.is_none() => o.input = Some(a.into()),
            _ => return Err(format!("unexpected argument {a}")),
        }
    }
    Ok(Some(o))
}

/// Run one job and read back what Rhubarb wrote. Returns a summary line.
fn run_job(exe: &Path, job: &Job) -> Result<String, String> {
    if let Some(dir) = job.out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    rhubarb::run(exe, job).map_err(|e| e.to_string())?;
    let text =
        std::fs::read_to_string(&job.out).map_err(|e| format!("{}: {e}", job.out.display()))?;
    // No line to clip to yet: the cue file is checked on its own.
    let parsed = parse_cues(&text, i32::MAX)
        .map_err(|e| format!("{}:{}: {}", job.out.display(), e.line, e.message))?;
    for w in &parsed.warnings {
        eprintln!("{}:{}: warning: {}", job.out.display(), w.line, w.message);
    }
    let last = parsed.cues.cues().last().map_or(0, |c| c.start_ms);
    Ok(format!(
        "{}: {} cues, last change at {:.2} s",
        job.out.display(),
        parsed.cues.len(),
        f64::from(last) / 1000.0
    ))
}

fn main() -> ExitCode {
    let opts = match parse_args(std::env::args().skip(1)) {
        Ok(Some(o)) => o,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(m) => return usage_error(&m),
    };
    if opts.batch.is_some() == opts.input.is_some() {
        return usage_error("give one WAV file or --batch <folder>");
    }
    if opts.dialog.is_some() && opts.dialog_file.is_some() {
        return usage_error("use --dialog or --dialog-file, not both");
    }
    if opts.batch.is_none() && (opts.dialog_dir.is_some() || opts.format.is_some()) {
        return usage_error("--dialog-dir and --format are for --batch");
    }

    // Rhubarb first: without it nothing else matters, and the message is the
    // part a newcomer needs.
    let path_var = std::env::var_os("PATH");
    let exe = match rhubarb::find(opts.rhubarb.as_deref(), path_var.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(3);
        }
    };

    if let Some(folder) = &opts.batch {
        let plan = match rhubarb::plan_batch(
            folder,
            opts.out.as_deref(),
            opts.dialog_dir.as_deref(),
            opts.format.unwrap_or(Format::Json),
            opts.recognizer.as_deref(),
        ) {
            Ok(p) => p,
            Err(e) => return usage_error(&format!("{}: {e}", folder.display())),
        };
        if plan.is_empty() {
            return usage_error(&format!("{}: no .wav files", folder.display()));
        }
        let mut failed = 0;
        for item in &plan {
            match run_job(&exe, &item.job) {
                Ok(s) => println!("{}: {s}", item.id),
                Err(m) => {
                    eprintln!("{}: {m}", item.id);
                    failed += 1;
                }
            }
        }
        println!("{} of {} lines done", plan.len() - failed, plan.len());
        return if failed == 0 {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }

    let Some(wav) = opts.input else {
        return usage_error("give one WAV file or --batch <folder>");
    };
    if !wav.is_file() {
        return usage_error(&format!("{}: no such file", wav.display()));
    }
    let out = opts.out.unwrap_or_else(|| wav.with_extension("json"));
    let dialog = match (opts.dialog, opts.dialog_file) {
        (Some(t), _) => Some(Dialog::Text(t)),
        (None, Some(f)) => Some(Dialog::File(f)),
        (None, None) => None,
    };
    let job = Job {
        format: Format::of_path(&out),
        wav,
        out,
        dialog,
        recognizer: opts.recognizer,
    };
    match run_job(&exe, &job) {
        Ok(s) => {
            println!("{s}");
            ExitCode::SUCCESS
        }
        Err(m) => {
            eprintln!("lipsync: {m}");
            ExitCode::FAILURE
        }
    }
}
