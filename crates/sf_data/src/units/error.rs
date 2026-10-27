//! [`DefError`]: a refused definition file, with the file and the line.
//!
//! `ron` reports the line and column of a syntax or type error itself. The
//! checks that come after parsing (an unknown weapon, a duplicate id) work on
//! plain values, which carry no position, so [`Source`] finds the line again
//! by searching the text for the quoted id of the record in question. The
//! search is a best effort for error messages only: if the file is formatted in
//! an unusual way the message just has no line number.

use std::fmt;

/// Why a definition file was refused: which file, where in it, and what is
/// wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefError {
    /// The file the error is in (as given to the loader, for example
    /// `assets/config/unit_defs.ron`).
    pub file: String,
    /// 1-based line, when known.
    pub line: Option<usize>,
    /// 1-based column, when known.
    pub column: Option<usize>,
    /// What is wrong.
    pub message: String,
}

impl DefError {
    /// An error about a whole file (it could not be read, a file is missing).
    pub fn file(file: impl Into<String>, message: impl Into<String>) -> Self {
        DefError {
            file: file.into(),
            line: None,
            column: None,
            message: message.into(),
        }
    }
}

impl fmt::Display for DefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (Some(l), Some(c)) => write!(f, "{}:{l}:{c}: {}", self.file, self.message),
            (Some(l), None) => write!(f, "{}:{l}: {}", self.file, self.message),
            _ => write!(f, "{}: {}", self.file, self.message),
        }
    }
}

impl std::error::Error for DefError {}

/// One definition file's text and name, able to turn a record into a line
/// number for error messages.
#[derive(Copy, Clone)]
pub(crate) struct Source<'a> {
    pub file: &'a str,
    pub text: &'a str,
}

impl<'a> Source<'a> {
    pub fn new(file: &'a str, text: &'a str) -> Self {
        Source { file, text }
    }

    /// An error without a position.
    pub fn error(&self, message: impl Into<String>) -> DefError {
        DefError::file(self.file, message)
    }

    /// An error from `ron` (it has a line and a column).
    pub fn ron_error(&self, e: &ron::error::SpannedError) -> DefError {
        DefError {
            file: self.file.to_string(),
            line: Some(e.span.start.line),
            column: Some(e.span.start.col),
            message: e.code.to_string(),
        }
    }

    /// Byte offsets of every `key: "value"` pair in the text.
    fn pairs(&self, key: &str, value: &str) -> Vec<usize> {
        let quoted = format!("\"{value}\"");
        let mut found = Vec::new();
        let mut from = 0;
        while let Some(rel) = self.text[from..].find(&quoted) {
            let at = from + rel;
            let before = self.text[..at].trim_end();
            if let Some(head) = before.strip_suffix(':')
                && head.trim_end().ends_with(key)
            {
                found.push(at);
            }
            from = at + quoted.len();
        }
        found
    }

    fn position(&self, offset: usize) -> (usize, usize) {
        let head = &self.text[..offset];
        let line = 1 + head.matches('\n').count();
        let column = 1 + head.rsplit('\n').next().map_or(0, |l| l.chars().count());
        (line, column)
    }

    /// An error at the `nth` (0-based) record whose `key` is `value`, for
    /// example the second `id: "rifleman"`.
    pub fn error_at_record(
        &self,
        key: &str,
        value: &str,
        nth: usize,
        message: impl Into<String>,
    ) -> DefError {
        let offset = self.pairs(key, value).get(nth).copied();
        self.error_at(offset, message)
    }

    /// An error at the first `needle` (a quoted value such as a weapon name)
    /// found inside the record `key: "value"`, that is before the next
    /// `id:`. Falls back to the record itself when the needle is not found
    /// there.
    pub fn error_in_record(
        &self,
        key: &str,
        value: &str,
        needle: &str,
        message: impl Into<String>,
    ) -> DefError {
        let offset = self.pairs(key, value).first().map(|&start| {
            let end = self.text[start + 1..]
                .find("id:")
                .map_or(self.text.len(), |rel| start + 1 + rel);
            let quoted = format!("\"{needle}\"");
            match self.text[start..end].find(&quoted) {
                Some(rel) => start + rel,
                None => start,
            }
        });
        self.error_at(offset, message)
    }

    /// An error at the first `needle` (quoted) after the first `after` inside
    /// the record `key: "value"`; for a needle that also occurs earlier in the
    /// record, such as a building that lists its own id in `requires:`. Falls
    /// back to [`error_in_record`](Self::error_in_record) when `after` is not there.
    pub fn error_in_record_after(
        &self,
        key: &str,
        value: &str,
        after: &str,
        needle: &str,
        message: impl Into<String>,
    ) -> DefError {
        let offset = self.pairs(key, value).first().and_then(|&start| {
            let end = self.text[start + 1..]
                .find("id:")
                .map_or(self.text.len(), |rel| start + 1 + rel);
            let from = start + self.text[start..end].find(after)?;
            let quoted = format!("\"{needle}\"");
            Some(
                self.text[from..end]
                    .find(&quoted)
                    .map_or(from, |rel| from + rel),
            )
        });
        match offset {
            Some(_) => self.error_at(offset, message),
            None => self.error_in_record(key, value, needle, message),
        }
    }

    /// Refuses a number literal with more than three decimals or with an
    /// exponent anywhere in the file (comments and strings are skipped).
    ///
    /// The RON reader turns `1.0000000001` into an `f64` before any field
    /// sees it, and a literal with a fourth decimal that is a zero (`1.0000`)
    /// arrives as a plain 1; looking at the text is what keeps "at most three
    /// decimals" exact for every number in the file, line numbers included.
    pub fn check_numbers(&self) -> Result<(), DefError> {
        let bytes = self.text.as_bytes();
        let mut i = 0;
        let mut depth = 0_usize; // nesting of block comments
        while i < bytes.len() {
            let c = bytes[i];
            let next = bytes.get(i + 1).copied();
            if depth > 0 {
                if c == b'*' && next == Some(b'/') {
                    depth -= 1;
                    i += 2;
                } else if c == b'/' && next == Some(b'*') {
                    depth += 1;
                    i += 2;
                } else {
                    i += 1;
                }
            } else if c == b'/' && next == Some(b'/') {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            } else if c == b'/' && next == Some(b'*') {
                depth = 1;
                i += 2;
            } else if c == b'"' {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            } else if c.is_ascii_alphabetic() || c == b'_' {
                // An identifier (`testpack_v1`): its digits are not numbers.
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
            } else if c.is_ascii_digit() {
                let start = i;
                if c == b'0' && matches!(next, Some(b'x' | b'b' | b'o')) {
                    while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
                        i += 1;
                    }
                    continue;
                }
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                    i += 1;
                }
                let mut decimals = 0;
                if bytes.get(i) == Some(&b'.') {
                    i += 1;
                    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                        decimals += usize::from(bytes[i] != b'_');
                        i += 1;
                    }
                }
                let exponent = matches!(bytes.get(i), Some(b'e' | b'E'));
                if exponent {
                    i += 1;
                    while i < bytes.len() && matches!(bytes[i], b'+' | b'-' | b'0'..=b'9') {
                        i += 1;
                    }
                }
                if decimals > 3 || exponent {
                    let literal = &self.text[start..i];
                    let why = if exponent {
                        "write it as a plain decimal, without an exponent"
                    } else {
                        "at most three decimals are allowed"
                    };
                    return Err(self.error_at(Some(start), format!("number {literal}: {why}")));
                }
            } else {
                i += 1;
            }
        }
        Ok(())
    }

    fn error_at(&self, offset: Option<usize>, message: impl Into<String>) -> DefError {
        match offset {
            Some(o) => {
                let (line, column) = self.position(o);
                DefError {
                    file: self.file.to_string(),
                    line: Some(line),
                    column: Some(column),
                    message: message.into(),
                }
            }
            None => self.error(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "(\n    units: [\n        (id: \"a\", weapons: [\"w\"]),\n        (\n            id: \"b\",\n            weapons: [\"zz\"],\n        ),\n    ],\n)\n";

    #[test]
    fn the_file_scan_counts_decimals_on_the_text() {
        let check = |text: &str| Source::new("f.ron", text).check_numbers();
        // Fine: three decimals, integers, digits in names, comments and strings.
        assert!(check("(speed: 1.125, hp: 50, id: \"a1.23456\", // 0.00001\n x: v1)").is_ok());
        assert!(check("/* 1.23456 /* nested 9.99999 */ 2.34567 */ (a: 0.5)").is_ok());
        assert!(check("(a: 1_000.250)").is_ok());
        let e = check("(a: 1,\n b: 1.0000000001)").unwrap_err();
        assert_eq!((e.line, e.column), (Some(2), Some(5)));
        assert!(e.message.contains("1.0000000001"), "{e}");
        assert!(check("(a: 1.0000)").is_err());
        assert!(check("(a: 2e3)").is_err());
        assert!(check("(a: 1.5E-2)").is_err());
    }

    #[test]
    fn finds_records_and_references() {
        let s = Source::new("f.ron", TEXT);
        let e = s.error_at_record("id", "b", 0, "boom");
        assert_eq!((e.line, e.column), (Some(5), Some(17)));
        assert_eq!(e.to_string(), "f.ron:5:17: boom");
        let e = s.error_in_record("id", "b", "zz", "unknown weapon");
        assert_eq!(e.line, Some(6));
        // A needle that is not inside the record falls back to the record.
        let e = s.error_in_record("id", "a", "zz", "x");
        assert_eq!(e.line, Some(3));
        // Not found at all: no line.
        let e = s.error_at_record("id", "nope", 0, "x");
        assert_eq!(e.line, None);
        assert_eq!(e.to_string(), "f.ron: x");
    }
}
