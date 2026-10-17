//! Reading mouth-cue files.
//!
//! Written from the public description of the output of Rhubarb Lip Sync
//! (https://github.com/DanielSWolf/rhubarb-lip-sync, MIT), not from its source:
//!
//! * **JSON**: an object with a `mouthCues` list; every cue is an object with
//!   `start` and `end` in seconds and `value`, the shape letter. A `metadata`
//!   object (sound file, duration) may come first and is ignored.
//! * **TSV**: one cue per line, `time<TAB>shape`; the time is in seconds and is
//!   when the shape *starts*. Rhubarb ends a file with an `X` at the end of the
//!   sound.
//!
//! The result is an integer-millisecond time series ([`MouthCues`]). The
//! reader checks what a hand-edited or truncated file can get wrong: an
//! unknown shape or a malformed line is an **error** with its line number;
//! cues out of order, twice at one time, overlapping, or running past the end
//! of the line they belong to are **warnings** and are put right (sorted,
//! merged, clipped). Only the reader knows the text of the file, so it is the
//! one that can name lines; the sampler just looks values up.

use std::fmt;

use super::{MouthCue, MouthCues, MouthShape};
use crate::math::Ms;

/// Nothing in a cue file is later than a day; anything beyond is a typo.
const MAX_TIME_MS: f64 = 86_400_000.0;
/// How deeply JSON may nest before the reader gives up (cue files are flat).
const MAX_DEPTH: usize = 32;

/// A cue file that cannot be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LipsyncError {
    /// Line in the cue file, 1-based.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for LipsyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for LipsyncError {}

/// Something that was put right while reading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LipsyncWarning {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for LipsyncWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// A cue file that was read: the time series and what had to be corrected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub cues: MouthCues,
    pub warnings: Vec<LipsyncWarning>,
}

fn err(line: usize, message: impl Into<String>) -> LipsyncError {
    LipsyncError {
        line,
        message: message.into(),
    }
}

/// One cue as written, before the checks.
#[derive(Clone, Copy, Debug)]
struct Raw {
    line: usize,
    start_ms: Ms,
    /// JSON only.
    end_ms: Option<Ms>,
    shape: MouthShape,
}

/// Seconds to whole milliseconds (rounded), or why not.
fn seconds_to_ms(secs: f64, line: usize, what: &str) -> Result<Ms, LipsyncError> {
    if !secs.is_finite() {
        return Err(err(line, format!("{what} is not a number")));
    }
    if secs < 0.0 {
        return Err(err(
            line,
            format!("{what} {secs} s is negative; times count from the start of the sound"),
        ));
    }
    let ms = (secs * 1000.0).round();
    if ms > MAX_TIME_MS {
        return Err(err(
            line,
            format!("{what} {secs} s is more than a day; a typo?"),
        ));
    }
    Ok(ms as Ms)
}

fn shape_of(text: &str, line: usize) -> Result<MouthShape, LipsyncError> {
    MouthShape::parse(text).ok_or_else(|| {
        err(
            line,
            format!("unknown mouth shape {text:?} (the shapes are A B C D E F G H X)"),
        )
    })
}

// ---------------------------------------------------------------------------
// TSV
// ---------------------------------------------------------------------------

/// Read `time<TAB>shape` lines. `duration_ms` is the length of the line of
/// dialogue the cues belong to; cues after it are dropped with a warning.
pub fn parse_tsv(text: &str, duration_ms: Ms) -> Result<Parsed, LipsyncError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut raw = Vec::new();
    for (i, row) in text.lines().enumerate() {
        let line = i + 1;
        if row.trim().is_empty() {
            continue;
        }
        let mut fields = row.split_whitespace();
        let (Some(time), Some(shape), None) = (fields.next(), fields.next(), fields.next()) else {
            return Err(err(
                line,
                format!("expected `time<TAB>shape` (seconds, then a shape letter), found {row:?}"),
            ));
        };
        let secs: f64 = time
            .parse()
            .map_err(|_| err(line, format!("{time:?} is not a time in seconds")))?;
        raw.push(Raw {
            line,
            start_ms: seconds_to_ms(secs, line, "time")?,
            end_ms: None,
            shape: shape_of(shape, line)?,
        });
    }
    finish(raw, duration_ms)
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

/// A JSON value with the line it starts on.
#[derive(Debug)]
struct Node {
    line: usize,
    value: Value,
}

#[derive(Debug)]
enum Value {
    Null,
    Bool,
    Number(f64),
    Text(String),
    List(Vec<Node>),
    Object(Vec<(String, Node)>),
}

impl Value {
    fn kind(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool => "a boolean",
            Value::Number(_) => "a number",
            Value::Text(_) => "a string",
            Value::List(_) => "a list",
            Value::Object(_) => "an object",
        }
    }
}

/// A small JSON reader that remembers where every value is. Cue files are a
/// few kilobytes of flat objects; this keeps the line numbers a general
/// parser would throw away.
struct Reader<'a> {
    b: &'a [u8],
    i: usize,
    line: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn bump(&mut self) {
        if self.peek() == Some(b'\n') {
            self.line += 1;
        }
        self.i += 1;
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.bump();
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), LipsyncError> {
        self.skip_ws();
        if self.peek() == Some(c) {
            self.bump();
            Ok(())
        } else {
            Err(err(
                self.line,
                format!("expected `{}`{}", c as char, self.found()),
            ))
        }
    }

    fn found(&self) -> String {
        match self.peek() {
            None => ", found the end of the file".into(),
            Some(c) if c.is_ascii_graphic() => format!(", found `{}`", c as char),
            Some(_) => String::new(),
        }
    }

    fn value(&mut self, depth: usize) -> Result<Node, LipsyncError> {
        if depth > MAX_DEPTH {
            return Err(err(self.line, "nested too deeply"));
        }
        self.skip_ws();
        let line = self.line;
        let value = match self.peek() {
            None => return Err(err(line, "the file ends where a value was expected")),
            Some(b'{') => self.object(depth)?,
            Some(b'[') => self.list(depth)?,
            Some(b'"') => Value::Text(self.string()?),
            Some(b't') => self.word("true", Value::Bool)?,
            Some(b'f') => self.word("false", Value::Bool)?,
            Some(b'n') => self.word("null", Value::Null)?,
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number()?,
            Some(_) => {
                return Err(err(
                    line,
                    format!(
                        "expected a value{}",
                        self.found().replace(", found", "; found")
                    ),
                ));
            }
        };
        Ok(Node { line, value })
    }

    fn word(&mut self, word: &str, v: Value) -> Result<Value, LipsyncError> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            for _ in 0..word.len() {
                self.bump();
            }
            Ok(v)
        } else {
            Err(err(self.line, format!("expected `{word}`")))
        }
    }

    fn number(&mut self) -> Result<Value, LipsyncError> {
        let start = self.i;
        while matches!(
            self.peek(),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.bump();
        }
        let text = std::str::from_utf8(&self.b[start..self.i]).unwrap_or("");
        text.parse::<f64>()
            .map(Value::Number)
            .map_err(|_| err(self.line, format!("{text:?} is not a number")))
    }

    fn string(&mut self) -> Result<String, LipsyncError> {
        let start_line = self.line;
        self.bump(); // opening quote
        let mut out = Vec::new();
        loop {
            match self.peek() {
                None => return Err(err(start_line, "a string is never closed")),
                Some(b'"') => {
                    self.bump();
                    break;
                }
                Some(b'\\') => {
                    self.bump();
                    let esc = self.peek();
                    self.bump();
                    match esc {
                        Some(b'"') => out.push(b'"'),
                        Some(b'\\') => out.push(b'\\'),
                        Some(b'/') => out.push(b'/'),
                        Some(b'b') => out.push(8),
                        Some(b'f') => out.push(12),
                        Some(b'n') => out.push(b'\n'),
                        Some(b'r') => out.push(b'\r'),
                        Some(b't') => out.push(b'\t'),
                        Some(b'u') => {
                            let mut code = 0u32;
                            for _ in 0..4 {
                                let d = self.peek().and_then(|c| (c as char).to_digit(16));
                                let Some(d) = d else {
                                    return Err(err(self.line, "bad \\u escape in a string"));
                                };
                                code = code * 16 + d;
                                self.bump();
                            }
                            let ch = char::from_u32(code).unwrap_or('\u{fffd}');
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return Err(err(self.line, "bad escape in a string")),
                    }
                }
                Some(c) => {
                    out.push(c);
                    self.bump();
                }
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    fn list(&mut self, depth: usize) -> Result<Value, LipsyncError> {
        self.bump(); // [
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.bump();
            return Ok(Value::List(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.bump(),
                Some(b']') => {
                    self.bump();
                    return Ok(Value::List(items));
                }
                _ => {
                    return Err(err(
                        self.line,
                        format!("expected `,` or `]`{}", self.found()),
                    ));
                }
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, LipsyncError> {
        self.bump(); // {
        let mut members = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.bump();
            return Ok(Value::Object(members));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(err(
                    self.line,
                    format!("expected a quoted name{}", self.found()),
                ));
            }
            let key = self.string()?;
            self.expect(b':')?;
            members.push((key, self.value(depth + 1)?));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.bump(),
                Some(b'}') => {
                    self.bump();
                    return Ok(Value::Object(members));
                }
                _ => {
                    return Err(err(
                        self.line,
                        format!("expected `,` or `}}`{}", self.found()),
                    ));
                }
            }
        }
    }
}

fn member<'a>(members: &'a [(String, Node)], name: &str) -> Option<&'a Node> {
    members.iter().find(|(k, _)| k == name).map(|(_, n)| n)
}

fn number_of(
    members: &[(String, Node)],
    name: &str,
    cue_line: usize,
) -> Result<Option<(f64, usize)>, LipsyncError> {
    match member(members, name) {
        None => Ok(None),
        Some(Node {
            value: Value::Number(n),
            line,
        }) => Ok(Some((*n, *line))),
        Some(n) => Err(err(
            n.line.max(cue_line),
            format!(
                "\"{name}\" must be a number of seconds, not {}",
                n.value.kind()
            ),
        )),
    }
}

/// Read the `mouthCues` of a Rhubarb JSON file. `duration_ms` as in
/// [`parse_tsv`].
pub fn parse_json(text: &str, duration_ms: Ms) -> Result<Parsed, LipsyncError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut r = Reader {
        b: text.as_bytes(),
        i: 0,
        line: 1,
    };
    let root = r.value(0)?;
    r.skip_ws();
    if r.peek().is_some() {
        return Err(err(
            r.line,
            "unexpected text after the end of the JSON value",
        ));
    }
    let Value::Object(top) = &root.value else {
        return Err(err(
            root.line,
            "expected an object with a \"mouthCues\" list (Rhubarb's JSON output)",
        ));
    };
    let Some(cues) = member(top, "mouthCues") else {
        return Err(err(
            root.line,
            "no \"mouthCues\" list; is this Rhubarb's JSON output (`-f json`)?",
        ));
    };
    let Value::List(items) = &cues.value else {
        return Err(err(
            cues.line,
            format!("\"mouthCues\" must be a list, not {}", cues.value.kind()),
        ));
    };
    let mut raw = Vec::new();
    for item in items {
        let Value::Object(members) = &item.value else {
            return Err(err(
                item.line,
                format!(
                    "a cue must be an object with start, end and value, not {}",
                    item.value.kind()
                ),
            ));
        };
        let Some((start, start_line)) = number_of(members, "start", item.line)? else {
            return Err(err(item.line, "cue without \"start\""));
        };
        let end = number_of(members, "end", item.line)?;
        let Some(value) = member(members, "value") else {
            return Err(err(item.line, "cue without \"value\" (the shape letter)"));
        };
        let Value::Text(shape) = &value.value else {
            return Err(err(
                value.line,
                format!(
                    "\"value\" must be a shape letter, not {}",
                    value.value.kind()
                ),
            ));
        };
        raw.push(Raw {
            line: item.line,
            start_ms: seconds_to_ms(start, start_line, "start")?,
            end_ms: match end {
                Some((e, l)) => Some(seconds_to_ms(e, l, "end")?),
                None => None,
            },
            shape: shape_of(shape, value.line)?,
        });
    }
    for c in &raw {
        if c.end_ms.is_some_and(|e| e < c.start_ms) {
            return Err(err(c.line, "the cue ends before it starts"));
        }
    }
    finish(raw, duration_ms)
}

/// Read a cue file of either kind: JSON when the first character is `{`
/// or `[`, otherwise TSV.
pub fn parse(text: &str, duration_ms: Ms) -> Result<Parsed, LipsyncError> {
    let first = text
        .trim_start_matches('\u{feff}')
        .trim_start()
        .chars()
        .next();
    if matches!(first, Some('{' | '[')) {
        parse_json(text, duration_ms)
    } else {
        parse_tsv(text, duration_ms)
    }
}

// ---------------------------------------------------------------------------
// The checks both formats share
// ---------------------------------------------------------------------------

fn finish(mut raw: Vec<Raw>, duration_ms: Ms) -> Result<Parsed, LipsyncError> {
    let mut warnings: Vec<LipsyncWarning> = Vec::new();
    let warn = |warnings: &mut Vec<LipsyncWarning>, line: usize, message: String| {
        warnings.push(LipsyncWarning { line, message });
    };
    if raw.is_empty() {
        return Err(err(1, "the file has no mouth cues"));
    }

    // Order.
    if let Some(w) = raw.windows(2).find(|w| w[1].start_ms < w[0].start_ms) {
        warn(
            &mut warnings,
            w[1].line,
            format!(
                "cue at {} ms comes after a cue at {} ms; cues are sorted by time",
                w[1].start_ms, w[0].start_ms
            ),
        );
        raw.sort_by_key(|c| c.start_ms);
    }

    // The same moment twice: the later one in the file wins.
    let mut kept: Vec<Raw> = Vec::with_capacity(raw.len());
    for c in raw {
        match kept.last_mut() {
            Some(last) if last.start_ms == c.start_ms => {
                warn(
                    &mut warnings,
                    c.line,
                    format!(
                        "two cues at {} ms; the later one ({}) is used",
                        c.start_ms, c.shape
                    ),
                );
                *last = c;
            }
            _ => kept.push(c),
        }
    }

    // Overlap: the next cue starts before this one ends.
    for w in kept.windows(2) {
        if w[0].end_ms.is_some_and(|e| e > w[1].start_ms) {
            warn(
                &mut warnings,
                w[1].line,
                format!(
                    "the cue before ends at {} ms, after this one starts at {} ms; this one wins",
                    w[0].end_ms.unwrap_or(0),
                    w[1].start_ms
                ),
            );
        }
    }

    // Inside the line: drop what starts after it, clip what runs past it.
    let first_late = kept.iter().position(|c| c.start_ms >= duration_ms);
    if let Some(i) = first_late {
        let n = kept.len() - i;
        warn(
            &mut warnings,
            kept[i].line,
            format!(
                "{n} cue{} start at or after the end of the line ({duration_ms} ms) and {} ignored",
                if n == 1 { "" } else { "s" },
                if n == 1 { "is" } else { "are" },
            ),
        );
        kept.truncate(i);
    } else if let Some(last) = kept
        .last()
        .filter(|c| c.end_ms.is_some_and(|e| e > duration_ms))
    {
        warn(
            &mut warnings,
            last.line,
            format!(
                "the last cue ends at {} ms, after the end of the line ({duration_ms} ms); clipped",
                last.end_ms.unwrap_or(0)
            ),
        );
    }

    // To a series of starts. A gap between one cue's end and the next start is
    // rest; so is what follows the last cue's end.
    let mut cues: Vec<MouthCue> = Vec::with_capacity(kept.len());
    for (i, c) in kept.iter().enumerate() {
        cues.push(MouthCue {
            start_ms: c.start_ms,
            shape: c.shape,
        });
        let next_start = kept.get(i + 1).map_or(duration_ms, |n| n.start_ms);
        if let Some(end) = c
            .end_ms
            .filter(|&e| e < next_start && c.shape != MouthShape::X)
        {
            cues.push(MouthCue {
                start_ms: end,
                shape: MouthShape::X,
            });
        }
    }
    Ok(Parsed {
        cues: MouthCues::from_sorted(cues),
        warnings,
    })
}
