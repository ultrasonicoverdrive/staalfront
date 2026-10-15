//! The JSON log of a run.
//!
//! Written by hand: the log is flat, holds numbers, hex strings and one name,
//! and its layout is part of the contract (CI compares two logs with `diff`),
//! which a pretty-printing library would not promise. The layout is fixed:
//!
//! ```text
//! {
//!   "format": 1,
//!   "scenario": "wander_5000",
//!   "seed": "0x0000000057a1f407",
//!   "map": {"width": 256, "height": 256},
//!   "ticks": 10000,
//!   "every": 100,
//!   "commands_scheduled": 5000,
//!   "commands_executed": 5000,
//!   "commands_ignored": 0,
//!   "entities": 5000,
//!   "initial_checksum": "0x...",
//!   "final_checksum": "0x...",
//!   "checksums": [
//!     {"ticks": 100, "checksum": "0x..."},
//!     ...
//!   ],
//!   "host": {
//!     "os": "linux",
//!     "arch": "x86_64",
//!     "debug_assertions": false,
//!     "elapsed_ms": 11000,
//!     "ticks_per_second": 909
//!   }
//! }
//! ```
//!
//! Everything above `host` is deterministic and must be equal on every
//! machine; `host` describes the machine and the wall time and is the last
//! key, so removing it (CI does, with `jq 'del(.host)'`) leaves exactly the
//! part that is compared. Checksums and the seed are 16-digit hex strings
//! because a JSON number would lose bits in most readers.

use std::fmt::Write as _;
use std::time::Duration;

use crate::run::Outcome;

/// The `format` field. Bump it when the layout changes.
pub const FORMAT: u32 = 1;

/// Facts about the machine and the wall time of one run. Nothing in here may
/// ever reach the simulation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host {
    /// `std::env::consts::OS`.
    pub os: &'static str,
    /// `std::env::consts::ARCH`.
    pub arch: &'static str,
    /// True in a build with debug assertions (a debug build).
    pub debug_assertions: bool,
    /// Wall time of the run, in microseconds.
    pub elapsed_micros: u128,
}

impl Host {
    /// This machine and this build, with the measured wall time.
    pub fn current(elapsed: Duration) -> Host {
        Host {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            debug_assertions: cfg!(debug_assertions),
            elapsed_micros: elapsed.as_micros(),
        }
    }

    /// Ticks per second of wall time for a run of `ticks` ticks, rounded
    /// down; a run shorter than a microsecond counts as one microsecond.
    pub fn ticks_per_second(&self, ticks: u64) -> u128 {
        u128::from(ticks) * 1_000_000 / self.elapsed_micros.max(1)
    }
}

/// A checksum as written everywhere: `0x` and 16 lowercase hex digits.
pub fn hex(value: u64) -> String {
    format!("{value:#018x}")
}

/// A JSON string literal for `text`: quotes, backslashes and control
/// characters escaped, everything else (including non-ASCII) as is.
pub fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The complete log text, ending in a newline, lines separated by `\n` on
/// every platform.
pub fn render(outcome: &Outcome, host: &Host) -> String {
    let mut s = String::new();
    // Writing to a String cannot fail.
    let _ = write_log(&mut s, outcome, host);
    s
}

fn write_log(s: &mut String, o: &Outcome, host: &Host) -> std::fmt::Result {
    writeln!(s, "{{")?;
    writeln!(s, "  \"format\": {FORMAT},")?;
    writeln!(s, "  \"scenario\": {},", json_string(&o.scenario))?;
    writeln!(s, "  \"seed\": \"{}\",", hex(o.seed))?;
    writeln!(
        s,
        "  \"map\": {{\"width\": {}, \"height\": {}}},",
        o.map_width, o.map_height
    )?;
    writeln!(s, "  \"ticks\": {},", o.ticks)?;
    writeln!(s, "  \"every\": {},", o.every)?;
    writeln!(s, "  \"commands_scheduled\": {},", o.commands_scheduled)?;
    writeln!(s, "  \"commands_executed\": {},", o.commands_executed)?;
    writeln!(s, "  \"commands_ignored\": {},", o.commands_ignored)?;
    writeln!(s, "  \"entities\": {},", o.entities)?;
    writeln!(
        s,
        "  \"initial_checksum\": \"{}\",",
        hex(o.initial_checksum)
    )?;
    writeln!(s, "  \"final_checksum\": \"{}\",", hex(o.final_checksum))?;
    writeln!(s, "  \"checksums\": [")?;
    for (i, sample) in o.samples.iter().enumerate() {
        let comma = if i + 1 < o.samples.len() { "," } else { "" };
        writeln!(
            s,
            "    {{\"ticks\": {}, \"checksum\": \"{}\"}}{comma}",
            sample.ticks,
            hex(sample.checksum)
        )?;
    }
    writeln!(s, "  ],")?;
    writeln!(s, "  \"host\": {{")?;
    writeln!(s, "    \"os\": {},", json_string(host.os))?;
    writeln!(s, "    \"arch\": {},", json_string(host.arch))?;
    writeln!(s, "    \"debug_assertions\": {},", host.debug_assertions)?;
    writeln!(s, "    \"elapsed_ms\": {},", host.elapsed_micros / 1_000)?;
    writeln!(
        s,
        "    \"ticks_per_second\": {}",
        host.ticks_per_second(o.ticks)
    )?;
    writeln!(s, "  }}")?;
    writeln!(s, "}}")
}

#[cfg(test)]
mod tests;
