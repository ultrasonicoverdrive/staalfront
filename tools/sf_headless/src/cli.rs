//! The command line: `sf_headless <scenario.ron> [--ticks N] [--every K]
//! [--out log.json] [--expect HEX]`.
//!
//! Arguments are parsed by hand ([`parse_args`]); [`run_cli`] does the work
//! and returns the exit code, writing to the streams it is given, so it can be
//! tested without a process.
//!
//! # Exit codes
//!
//! | Code | Meaning |
//! |---|---|
//! | 0 | the run finished (and matched `--expect` if given) |
//! | 1 | `--expect` was given and the final checksum differs |
//! | 2 | bad usage, an unreadable or invalid scenario, or a log that could not be written |
//!
//! The log is written before `--expect` is checked, so a mismatch leaves the
//! evidence behind.

use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use crate::log::{self, Host};
use crate::run::{self, DEFAULT_EVERY, Outcome, RunOptions};
use crate::scenario::Scenario;

/// Exit code: success.
pub const EXIT_OK: u8 = 0;
/// Exit code: the final checksum differs from `--expect`.
pub const EXIT_MISMATCH: u8 = 1;
/// Exit code: usage error, invalid scenario or I/O failure.
pub const EXIT_INVALID: u8 = 2;

/// The help text.
pub const USAGE: &str = "\
Usage: sf_headless <scenario.ron> [options]

Runs a scenario on the deterministic simulation, without a window or a GPU,
and prints the checksum of the final state.

Options:
  --ticks N      run N ticks instead of the scenario's own length
  --every K      record a checksum after every K-th tick (default 100); the
                 last tick is always recorded
  --out FILE     write the JSON log to FILE
  --expect HEX   exit with status 1 unless the final checksum equals HEX
                 (with or without 0x; underscores are ignored)
  -h, --help     show this text

Exit status: 0 success, 1 --expect mismatch, 2 usage error, invalid scenario
or unwritable log.
";

/// What the command line asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    /// The scenario file.
    pub scenario: PathBuf,
    /// `--ticks`: overrides the scenario's length.
    pub ticks: Option<u64>,
    /// `--every`: checksum interval.
    pub every: u64,
    /// `--out`: where to write the JSON log.
    pub out: Option<PathBuf>,
    /// `--expect`: the final checksum the run must end with.
    pub expect: Option<u64>,
}

/// The result of parsing the arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Parsed {
    /// Run a scenario.
    Run(Args),
    /// Show the help text.
    Help,
}

/// Parses a checksum as given to `--expect`: optional `0x`, 1 to 16 hex digits,
/// case-insensitive, `_` and surrounding whitespace ignored. The whitespace
/// rule is for values read from a file with `$(cat file)`, which on Windows
/// may end in a carriage return.
pub fn parse_checksum(text: &str) -> Result<u64, String> {
    let t = text.trim();
    let digits = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
        .unwrap_or(t);
    let digits: String = digits.chars().filter(|&c| c != '_').collect();
    if digits.is_empty() || digits.len() > 16 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "`{t}` is not a checksum (expected up to 16 hex digits, like 0x52a4964f24ae7c56)"
        ));
    }
    u64::from_str_radix(&digits, 16).map_err(|e| format!("`{t}` is not a checksum: {e}"))
}

fn number(flag: &str, value: &str) -> Result<u64, String> {
    match value.parse::<u64>() {
        Ok(n) if n >= 1 => Ok(n),
        Ok(_) => Err(format!("{flag} must be at least 1")),
        Err(_) => Err(format!("{flag} needs a whole number, got `{value}`")),
    }
}

/// Parses the arguments after the program name.
///
/// Flags take their value as the next argument or after `=`
/// (`--ticks 500`, `--ticks=500`). Giving a flag twice, an unknown flag, a
/// missing scenario or a second scenario is an error.
pub fn parse_args<I>(args: I) -> Result<Parsed, String>
where
    I: IntoIterator<Item = String>,
{
    let mut iter = args.into_iter();
    let mut scenario: Option<PathBuf> = None;
    let mut ticks = None;
    let mut every = None;
    let mut out = None;
    let mut expect = None;

    while let Some(arg) = iter.next() {
        if arg == "-h" || arg == "--help" {
            return Ok(Parsed::Help);
        }
        if let Some(flag_text) = arg.strip_prefix("--") {
            let (flag, inline) = match flag_text.split_once('=') {
                Some((f, v)) => (f, Some(v.to_string())),
                None => (flag_text, None),
            };
            if !matches!(flag, "ticks" | "every" | "out" | "expect") {
                return Err(format!("unknown option `--{flag}`"));
            }
            let value = match inline {
                Some(v) => v,
                None => iter
                    .next()
                    .ok_or_else(|| format!("--{flag} needs a value"))?,
            };
            let name = format!("--{flag}");
            let twice = || format!("{name} given twice");
            match flag {
                "ticks" => {
                    if ticks.replace(number(&name, &value)?).is_some() {
                        return Err(twice());
                    }
                }
                "every" => {
                    if every.replace(number(&name, &value)?).is_some() {
                        return Err(twice());
                    }
                }
                "out" => {
                    if out.replace(PathBuf::from(value)).is_some() {
                        return Err(twice());
                    }
                }
                _ => {
                    if expect.replace(parse_checksum(&value)?).is_some() {
                        return Err(twice());
                    }
                }
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            return Err(format!("unknown option `{arg}`"));
        } else if scenario.replace(PathBuf::from(arg)).is_some() {
            return Err("more than one scenario file given".to_string());
        }
    }

    let scenario = scenario.ok_or_else(|| "no scenario file given".to_string())?;
    Ok(Parsed::Run(Args {
        scenario,
        ticks,
        every: every.unwrap_or(DEFAULT_EVERY),
        out,
        expect,
    }))
}

fn summary(outcome: &Outcome, scenario_path: &str, host: &Host) -> String {
    format!(
        "scenario:   {} ({scenario_path})\n\
         seed:       {}\n\
         ticks:      {} (checksum every {})\n\
         entities:   {} alive at the end\n\
         commands:   {} scheduled, {} executed, {} ignored\n\
         initial:    {}\n\
         final:      {}\n\
         speed:      {} ticks/s ({} ms)\n",
        outcome.scenario,
        log::hex(outcome.seed),
        outcome.ticks,
        outcome.every,
        outcome.entities,
        outcome.commands_scheduled,
        outcome.commands_executed,
        outcome.commands_ignored,
        log::hex(outcome.initial_checksum),
        log::hex(outcome.final_checksum),
        host.ticks_per_second(outcome.ticks),
        host.elapsed_micros / 1_000,
    )
}

/// Runs the tool with the given arguments (after the program name) and
/// returns the exit code. Results go to `out`, problems to `err`.
pub fn run_cli(args: Vec<String>, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let args = match parse_args(args) {
        Ok(Parsed::Help) => {
            let _ = out.write_all(USAGE.as_bytes());
            return EXIT_OK;
        }
        Ok(Parsed::Run(args)) => args,
        Err(message) => {
            let _ = writeln!(err, "sf_headless: {message}\n\n{USAGE}");
            return EXIT_INVALID;
        }
    };

    let scenario = match Scenario::load(&args.scenario) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(err, "sf_headless: {e}");
            return EXIT_INVALID;
        }
    };

    let options = RunOptions {
        ticks: args.ticks.unwrap_or(scenario.ticks),
        every: args.every,
    };
    // The only clock in the tool, and it only brackets the run: the elapsed
    // time goes into the log's `host` block after the fact.
    let started = Instant::now();
    let outcome = match run::run(&scenario, &options) {
        Ok(o) => o,
        Err(e) => {
            let _ = writeln!(err, "sf_headless: {e}");
            return EXIT_INVALID;
        }
    };
    let host = Host::current(started.elapsed());

    if let Some(path) = &args.out {
        let text = log::render(&outcome, &host);
        let written = match path.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => std::fs::create_dir_all(dir),
            _ => Ok(()),
        }
        .and_then(|()| std::fs::write(path, text));
        if let Err(e) = written {
            let _ = writeln!(err, "sf_headless: cannot write {}: {e}", path.display());
            return EXIT_INVALID;
        }
    }

    let _ = out.write_all(summary(&outcome, &args.scenario.to_string_lossy(), &host).as_bytes());

    if let Some(expected) = args.expect {
        if expected == outcome.final_checksum {
            let _ = writeln!(out, "expect:     ok");
        } else {
            let _ = writeln!(
                err,
                "sf_headless: checksum mismatch: expected {}, got {}",
                log::hex(expected),
                log::hex(outcome.final_checksum)
            );
            return EXIT_MISMATCH;
        }
    }
    EXIT_OK
}

#[cfg(test)]
mod tests;
