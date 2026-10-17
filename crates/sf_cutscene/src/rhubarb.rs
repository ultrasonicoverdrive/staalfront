//! Running Rhubarb Lip Sync from the `lipsync` tool (WP12.3).
//!
//! Rhubarb (https://github.com/DanielSWolf/rhubarb-lip-sync, MIT licence) turns
//! a recording into mouth shapes. It is **not** part of this repository and is
//! not downloaded by it: the artist installs the release for their system and
//! the tool finds the `rhubarb` executable on `PATH` or by `--rhubarb <path>`.
//! This module builds the command line, finds the executable, plans a batch
//! over a folder, and runs the process. Reading what Rhubarb wrote is
//! [`crate::lipsync`].

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where the releases of Rhubarb Lip Sync are published.
pub const RELEASES_URL: &str = "https://github.com/DanielSWolf/rhubarb-lip-sync/releases";

/// The names the executable can have on this system.
pub fn executable_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["rhubarb.exe", "rhubarb.cmd", "rhubarb.bat"]
    } else {
        &["rhubarb"]
    }
}

/// What to tell someone who has no Rhubarb yet.
pub fn install_instructions() -> String {
    format!(
        "Rhubarb Lip Sync was not found.\n\
\n\
The lipsync tool needs the `rhubarb` program to turn a recording into mouth shapes.\n\
Rhubarb Lip Sync is free software under the MIT licence; Staalfront does not include it\n\
and does not download it for you. To install it:\n\
\n\
  1. Open {RELEASES_URL}\n\
  2. Download the zip for your system (Windows, macOS or Linux) from the latest release\n\
  3. Unzip it anywhere. Keep the whole folder together: the `rhubarb` program needs the\n\
     `res` folder next to it\n\
  4. Either add that folder to your PATH, or tell the tool where it is:\n\
       lipsync --rhubarb C:\\Tools\\Rhubarb\\rhubarb.exe voice.wav -o voice.json\n\
\n\
Check that it works with:  rhubarb --version"
    )
}

/// Look for an executable called one of `names` in each of `dirs`, in order.
pub fn find_in_dirs<I>(dirs: I, names: &[&str]) -> Option<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
{
    dirs.into_iter().find_map(|dir| {
        names
            .iter()
            .map(|n| dir.join(n))
            .find(|candidate| candidate.is_file())
    })
}

/// Why no usable `rhubarb` was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotFound {
    /// `--rhubarb <path>` names something that is not a file.
    GivenPathMissing(PathBuf),
    /// Not on `PATH`.
    NotOnPath,
}

impl fmt::Display for NotFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotFound::GivenPathMissing(p) => {
                writeln!(f, "--rhubarb {}: no such file.\n", p.display())?;
                f.write_str(&install_instructions())
            }
            NotFound::NotOnPath => f.write_str(&install_instructions()),
        }
    }
}

/// Find Rhubarb: the path the user gave, else the first `rhubarb` on `path_var`
/// (the value of `PATH`).
pub fn find(explicit: Option<&Path>, path_var: Option<&OsStr>) -> Result<PathBuf, NotFound> {
    if let Some(p) = explicit {
        return if p.is_file() {
            Ok(p.to_path_buf())
        } else {
            Err(NotFound::GivenPathMissing(p.to_path_buf()))
        };
    }
    let dirs = path_var
        .map(|v| std::env::split_paths(v).collect::<Vec<_>>())
        .unwrap_or_default();
    find_in_dirs(dirs, executable_names()).ok_or(NotFound::NotOnPath)
}

/// Which file format Rhubarb should write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Json,
    Tsv,
}

impl Format {
    /// `tsv` for a `.tsv` file name, JSON for anything else.
    pub fn of_path(path: &Path) -> Format {
        match path.extension().and_then(OsStr::to_str) {
            Some(e) if e.eq_ignore_ascii_case("tsv") => Format::Tsv,
            _ => Format::Json,
        }
    }

    pub fn parse(name: &str) -> Option<Format> {
        match name.to_ascii_lowercase().as_str() {
            "json" => Some(Format::Json),
            "tsv" => Some(Format::Tsv),
            _ => None,
        }
    }

    /// The argument of Rhubarb's `-f`, and the file name extension.
    pub fn name(self) -> &'static str {
        match self {
            Format::Json => "json",
            Format::Tsv => "tsv",
        }
    }
}

/// The text a recording says, which makes Rhubarb's result better.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dialog {
    /// The words themselves; written to a temporary file for Rhubarb.
    Text(String),
    /// A text file that already holds them.
    File(PathBuf),
}

/// One recording to analyse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub wav: PathBuf,
    pub out: PathBuf,
    pub format: Format,
    pub dialog: Option<Dialog>,
    /// Rhubarb's `--recognizer`: `pocketSphinx` (English, the default) or
    /// `phonetic` (any language, less precise).
    pub recognizer: Option<String>,
}

/// The arguments for `rhubarb`. `dialog_file` is the text file to pass to
/// `-d` (for a [`Dialog::Text`] this is the temporary file).
pub fn arguments(job: &Job, dialog_file: Option<&Path>) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec![
        "-f".into(),
        job.format.name().into(),
        "-o".into(),
        job.out.as_os_str().into(),
    ];
    if let Some(d) = dialog_file {
        a.push("-d".into());
        a.push(d.as_os_str().into());
    }
    if let Some(r) = &job.recognizer {
        a.push("-r".into());
        a.push(r.into());
    }
    a.push("-q".into());
    a.push(job.wav.as_os_str().into());
    a
}

/// What went wrong running Rhubarb.
#[derive(Debug)]
pub enum RunError {
    /// The process could not be started.
    Start(io::Error),
    /// Rhubarb ran and failed; its own message.
    Failed { status: String, message: String },
    /// A temporary or output file could not be handled.
    Io(String),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunError::Start(e) => write!(f, "cannot start rhubarb: {e}"),
            RunError::Failed { status, message } if message.is_empty() => {
                write!(f, "rhubarb failed ({status})")
            }
            RunError::Failed { status, message } => {
                write!(f, "rhubarb failed ({status}): {message}")
            }
            RunError::Io(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for RunError {}

/// A temporary dialog file that is removed when it goes out of scope.
struct TempDialog(PathBuf);

impl Drop for TempDialog {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Run `exe` for `job`. Returns when Rhubarb has written `job.out`.
pub fn run(exe: &Path, job: &Job) -> Result<(), RunError> {
    let (dialog_path, _temp) = match &job.dialog {
        None => (None, None),
        Some(Dialog::File(p)) => (Some(p.clone()), None),
        Some(Dialog::Text(text)) => {
            let path = std::env::temp_dir().join(format!(
                "staalfront-lipsync-{}-{}.txt",
                std::process::id(),
                job.wav
                    .file_stem()
                    .and_then(OsStr::to_str)
                    .unwrap_or("line")
            ));
            fs::write(&path, text)
                .map_err(|e| RunError::Io(format!("cannot write {}: {e}", path.display())))?;
            (Some(path.clone()), Some(TempDialog(path)))
        }
    };
    let output = Command::new(exe)
        .args(arguments(job, dialog_path.as_deref()))
        .output()
        .map_err(RunError::Start)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let text = if stderr.trim().is_empty() {
            stdout
        } else {
            stderr
        };
        let tail: Vec<&str> = text.lines().rev().take(12).collect();
        let message = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
        return Err(RunError::Failed {
            status: output.status.to_string(),
            message: message.trim().to_string(),
        });
    }
    if !job.out.is_file() {
        return Err(RunError::Io(format!(
            "rhubarb finished but did not write {}",
            job.out.display()
        )));
    }
    Ok(())
}

/// One recording of a batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchItem {
    /// The line id: the WAV's file name without its extension.
    pub id: String,
    pub job: Job,
}

/// The recordings of a folder: every `*.wav` (any case), by file name, each
/// producing `<id>.json` (or `.tsv`) in `out_dir` (default: next to the WAV).
/// With `dialog_dir`, `<id>.txt` in it is the text of the line when it exists.
pub fn plan_batch(
    wav_dir: &Path,
    out_dir: Option<&Path>,
    dialog_dir: Option<&Path>,
    format: Format,
    recognizer: Option<&str>,
) -> io::Result<Vec<BatchItem>> {
    let mut wavs: Vec<PathBuf> = fs::read_dir(wav_dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(OsStr::to_str)
                    .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
        })
        .collect();
    wavs.sort();
    Ok(wavs
        .into_iter()
        .filter_map(|wav| {
            let id = wav.file_stem()?.to_str()?.to_string();
            let out = out_dir
                .unwrap_or(wav_dir)
                .join(format!("{id}.{}", format.name()));
            let dialog = dialog_dir
                .map(|d| d.join(format!("{id}.txt")))
                .filter(|p| p.is_file())
                .map(Dialog::File);
            Some(BatchItem {
                id,
                job: Job {
                    wav,
                    out,
                    format,
                    dialog,
                    recognizer: recognizer.map(str::to_string),
                },
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sf_rhubarb_{}_{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn instructions_name_the_licence_and_the_download_page() {
        let t = install_instructions();
        assert!(t.contains("MIT"));
        assert!(t.contains("https://github.com/DanielSWolf/rhubarb-lip-sync/releases"));
        assert!(t.contains("--rhubarb"));
        assert!(t.contains("PATH"));
    }

    #[test]
    fn the_first_directory_with_the_program_wins() {
        let a = temp_dir("a");
        let b = temp_dir("b");
        let name = executable_names()[0];
        fs::write(b.join(name), "x").unwrap();
        assert_eq!(
            find_in_dirs(vec![a.clone(), b.clone()], executable_names()),
            Some(b.join(name))
        );
        fs::write(a.join(name), "x").unwrap();
        assert_eq!(
            find_in_dirs(vec![a.clone(), b.clone()], executable_names()),
            Some(a.join(name))
        );
        assert_eq!(
            find_in_dirs(Vec::<PathBuf>::new(), executable_names()),
            None
        );
        // A folder with that name is not a program.
        let c = temp_dir("c");
        fs::create_dir(c.join(name)).unwrap();
        assert_eq!(find_in_dirs(vec![c], executable_names()), None);
    }

    #[test]
    fn find_uses_the_given_path_or_the_path_variable() {
        let d = temp_dir("find");
        let exe = d.join(executable_names()[0]);
        fs::write(&exe, "x").unwrap();
        let var = std::env::join_paths([d.clone()]).unwrap();
        assert_eq!(find(None, Some(&var)), Ok(exe.clone()));
        assert_eq!(find(Some(&exe), None), Ok(exe.clone()));
        let missing = d.join("nope");
        assert_eq!(
            find(Some(&missing), Some(&var)),
            Err(NotFound::GivenPathMissing(missing))
        );
        assert_eq!(find(None, None), Err(NotFound::NotOnPath));
        assert_eq!(find(None, Some(OsStr::new(""))), Err(NotFound::NotOnPath));
    }

    #[test]
    fn arguments_follow_rhubarbs_documented_options() {
        let job = Job {
            wav: "in/voice.wav".into(),
            out: "out/voice.json".into(),
            format: Format::Json,
            dialog: None,
            recognizer: Some("phonetic".into()),
        };
        let a: Vec<String> = arguments(&job, Some(Path::new("t.txt")))
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            a,
            [
                "-f",
                "json",
                "-o",
                "out/voice.json",
                "-d",
                "t.txt",
                "-r",
                "phonetic",
                "-q",
                "in/voice.wav"
            ]
        );
        let a: Vec<String> = arguments(
            &Job {
                format: Format::Tsv,
                recognizer: None,
                ..job
            },
            None,
        )
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
        assert_eq!(
            a,
            ["-f", "tsv", "-o", "out/voice.json", "-q", "in/voice.wav"]
        );
    }

    #[test]
    fn format_follows_the_file_name() {
        assert_eq!(Format::of_path(Path::new("a.tsv")), Format::Tsv);
        assert_eq!(Format::of_path(Path::new("a.TSV")), Format::Tsv);
        assert_eq!(Format::of_path(Path::new("a.json")), Format::Json);
        assert_eq!(Format::of_path(Path::new("a")), Format::Json);
        assert_eq!(Format::parse("JSON"), Some(Format::Json));
        assert_eq!(Format::parse("xml"), None);
    }

    #[test]
    fn a_batch_is_the_wavs_by_name_with_their_texts() {
        let wavs = temp_dir("batch_wav");
        let texts = temp_dir("batch_txt");
        let out = temp_dir("batch_out");
        for n in ["l020.wav", "l003.WAV", "l011.wav", "notes.txt", "l004.ogg"] {
            fs::write(wavs.join(n), "x").unwrap();
        }
        fs::create_dir(wavs.join("sub.wav")).unwrap();
        fs::write(texts.join("l011.txt"), "Hello.").unwrap();
        let plan = plan_batch(&wavs, Some(&out), Some(&texts), Format::Json, None).unwrap();
        let ids: Vec<&str> = plan.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, ["l003", "l011", "l020"]);
        assert_eq!(plan[1].job.out, out.join("l011.json"));
        assert_eq!(
            plan[1].job.dialog,
            Some(Dialog::File(texts.join("l011.txt")))
        );
        assert_eq!(plan[0].job.dialog, None);
        let beside = plan_batch(&wavs, None, None, Format::Tsv, Some("phonetic")).unwrap();
        assert_eq!(beside[0].job.out, wavs.join("l003.tsv"));
        assert_eq!(beside[0].job.recognizer.as_deref(), Some("phonetic"));
        assert!(plan_batch(&wavs.join("missing"), None, None, Format::Json, None).is_err());
    }
}
