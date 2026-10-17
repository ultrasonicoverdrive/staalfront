//! Finds the line of a value in a RON text from its path.
//!
//! `ron` reports a line for syntax errors but not for values that parsed fine
//! and are wrong afterwards (an unknown character name, an overlapping line).
//! To give those the line number the writer needs, this module scans the text
//! once with a small RON-shaped scanner and records where every struct field,
//! list element and tuple element starts.
//!
//! A path looks like `timeline.tracks[1].events[3].kind.speaker`: fields joined
//! with dots, list and tuple positions in brackets. `Some(x)` is transparent
//! (it has the path of `x`). The scanner only needs the *shape* of RON, not its
//! meaning, so it also copes with `#![enable(...)]`, comments, raw strings and
//! `Name(...)` wrappers. If it ever gets lost, lookups simply return `None` and
//! the message goes without a line number.

use std::collections::BTreeMap;

/// Line numbers (1-based) of the values in one RON text.
#[derive(Clone, Debug, Default)]
pub struct SpanIndex {
    lines: BTreeMap<String, usize>,
}

impl SpanIndex {
    pub fn new(text: &str) -> SpanIndex {
        let mut s = Scanner {
            b: text.as_bytes(),
            i: 0,
            line: 1,
            out: BTreeMap::new(),
        };
        s.skip_attributes();
        s.value("");
        SpanIndex { lines: s.out }
    }

    /// Line of the value at `path`. If the exact value is not written in the
    /// text (a field left at its default), the nearest enclosing value's line is
    /// returned.
    pub fn line_of(&self, path: &str) -> Option<usize> {
        let mut p = path;
        loop {
            if let Some(l) = self.lines.get(p) {
                return Some(*l);
            }
            if p.is_empty() {
                return None;
            }
            // Drop the last segment: `.name` or `[n]`.
            let cut = p.rfind(['.', '[']).unwrap_or(0);
            p = &p[..cut];
        }
    }
}

/// Joins a parent path and a field name.
pub fn join(path: &str, field: &str) -> String {
    if path.is_empty() {
        field.to_string()
    } else {
        format!("{path}.{field}")
    }
}

struct Scanner<'a> {
    b: &'a [u8],
    i: usize,
    line: usize,
    out: BTreeMap<String, usize>,
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn ends_token(c: u8) -> bool {
    matches!(
        c,
        b' ' | b'\t'
            | b'\r'
            | b'\n'
            | b','
            | b':'
            | b'('
            | b')'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b'"'
            | b'\''
            | b'/'
    )
}

impl Scanner<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn peek_at(&self, off: usize) -> Option<u8> {
        self.b.get(self.i + off).copied()
    }

    fn bump(&mut self) {
        if self.peek() == Some(b'\n') {
            self.line += 1;
        }
        self.i += 1;
    }

    fn record(&mut self, path: &str) {
        self.out.entry(path.to_string()).or_insert(self.line);
    }

    /// Skip whitespace, `// ...` and (nested) `/* ... */` comments.
    fn skip_ws(&mut self) {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\r' | b'\n') => self.bump(),
                Some(b'/') if self.peek_at(1) == Some(b'/') => {
                    while !matches!(self.peek(), None | Some(b'\n')) {
                        self.bump();
                    }
                }
                Some(b'/') if self.peek_at(1) == Some(b'*') => {
                    self.bump();
                    self.bump();
                    let mut depth = 1;
                    while depth > 0 {
                        match (self.peek(), self.peek_at(1)) {
                            (None, _) => return,
                            (Some(b'/'), Some(b'*')) => {
                                self.bump();
                                self.bump();
                                depth += 1;
                            }
                            (Some(b'*'), Some(b'/')) => {
                                self.bump();
                                self.bump();
                                depth -= 1;
                            }
                            _ => self.bump(),
                        }
                    }
                }
                _ => return,
            }
        }
    }

    /// Skip leading `#![enable(...)]` extension attributes.
    fn skip_attributes(&mut self) {
        loop {
            self.skip_ws();
            if self.peek() == Some(b'#') {
                while !matches!(self.peek(), None | Some(b']')) {
                    self.bump();
                }
                self.bump();
            } else {
                return;
            }
        }
    }

    fn string(&mut self) {
        self.bump(); // opening quote
        while let Some(c) = self.peek() {
            match c {
                b'\\' => {
                    self.bump();
                    self.bump();
                }
                b'"' => {
                    self.bump();
                    return;
                }
                _ => self.bump(),
            }
        }
    }

    fn raw_string(&mut self) {
        self.bump(); // 'r'
        let mut hashes = 0;
        while self.peek() == Some(b'#') {
            hashes += 1;
            self.bump();
        }
        if self.peek() != Some(b'"') {
            return;
        }
        self.bump();
        while let Some(c) = self.peek() {
            self.bump();
            if c == b'"' {
                let mut n = 0;
                while n < hashes && self.peek() == Some(b'#') {
                    n += 1;
                    self.bump();
                }
                if n == hashes {
                    return;
                }
            }
        }
    }

    fn char_literal(&mut self) {
        self.bump(); // opening quote
        while let Some(c) = self.peek() {
            match c {
                b'\\' => {
                    self.bump();
                    self.bump();
                }
                b'\'' => {
                    self.bump();
                    return;
                }
                _ => self.bump(),
            }
        }
    }

    fn token(&mut self) -> &str {
        let start = self.i;
        while let Some(c) = self.peek() {
            if ends_token(c) {
                break;
            }
            self.bump();
        }
        std::str::from_utf8(&self.b[start..self.i]).unwrap_or("")
    }

    /// The text of a plain `"quoted"` key at the current position (no escapes),
    /// without moving.
    fn quoted_key(&self) -> Option<String> {
        if self.peek() != Some(b'"') {
            return None;
        }
        let rest = &self.b[self.i + 1..];
        let end = rest
            .iter()
            .position(|&c| matches!(c, b'"' | b'\\' | b'\n'))?;
        (rest[end] == b'"').then(|| String::from_utf8_lossy(&rest[..end]).into_owned())
    }

    /// One value at `path`.
    fn value(&mut self, path: &str) {
        self.skip_ws();
        let Some(c) = self.peek() else { return };
        match c {
            b'"' => {
                self.record(path);
                self.string();
            }
            b'\'' => {
                self.record(path);
                self.char_literal();
            }
            b'(' => {
                self.record(path);
                self.group(b')', path);
            }
            b'[' => {
                self.record(path);
                self.group(b']', path);
            }
            b'{' => {
                self.record(path);
                self.group(b'}', path);
            }
            b'r' if matches!(self.peek_at(1), Some(b'"' | b'#')) => {
                self.record(path);
                self.raw_string();
            }
            b')' | b']' | b'}' | b',' | b':' => {
                // Malformed; consume so the caller cannot loop forever.
                self.bump();
            }
            _ => {
                self.record(path);
                let name = self.token().to_string();
                self.skip_ws();
                if self.peek() == Some(b'(') {
                    if name == "Some" {
                        self.bump();
                        self.value(path);
                        self.skip_ws();
                        if self.peek() == Some(b')') {
                            self.bump();
                        }
                    } else {
                        self.group(b')', path);
                    }
                }
            }
        }
    }

    /// The inside of `( ... )`, `[ ... ]` or `{ ... }`; the opening bracket is
    /// the current byte.
    fn group(&mut self, close: u8, path: &str) {
        self.bump(); // opening bracket
        let mut index = 0usize;
        loop {
            self.skip_ws();
            match self.peek() {
                None => return,
                Some(c) if c == close => {
                    self.bump();
                    return;
                }
                Some(b',') => {
                    self.bump();
                    continue;
                }
                Some(b')' | b']' | b'}') => {
                    // Mismatched bracket: stop this group.
                    return;
                }
                _ => {}
            }
            // `name: value` (a field)?
            if self.peek().is_some_and(is_ident_start) {
                let (si, sl) = (self.i, self.line);
                let name = self.token().to_string();
                self.skip_ws();
                if self.peek() == Some(b':') {
                    self.bump();
                    let p = join(path, &name);
                    self.value(&p);
                    continue;
                }
                self.i = si;
                self.line = sl;
            }
            // Otherwise a list element, tuple element or map entry.
            let p = format!("{path}[{index}]");
            index += 1;
            let before = self.i;
            let key = self.quoted_key();
            let key_line = self.line;
            self.value(&p);
            self.skip_ws();
            if self.peek() == Some(b':') {
                // Map entry `key: value`. A map with text keys (a part's
                // `mouth_shapes: { "A": ... }`) is also found by key.
                self.bump();
                if let Some(k) = key {
                    self.out.entry(join(path, &k)).or_insert(key_line);
                }
                self.value(&format!("{p}.value"));
            }
            if self.i == before {
                // No progress (cannot happen with the arms above, but a stuck
                // scanner would hang the loader).
                self.bump();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = r##"// header comment
#![enable(implicit_some)]
Scene(
    name: "a (tricky) \" string, with: stuff",
    layers: [
        (name: "bg", art: Rect(w: 1.0, h: 2.0, color: "#000000")),
        (
            name: "fg",
            art: Image("x.png"),
        ),
    ],
    /* block /* nested */ comment */
    timeline: (tracks: [
        (name: "t", events: [
            (at: 0, kind: Wait(duration_ms: 5)),
            (at: 5,
             kind: Dialogue(speaker: "x", text: "hi", duration_ms: 3, voice: Some("v.wav")),
            ),
        ]),
    ]),
    raw: r#"a "quoted" )] thing"#,
    last: 1,
)
"##;

    #[test]
    fn finds_fields_and_elements() {
        let ix = SpanIndex::new(TEXT);
        assert_eq!(ix.line_of("name"), Some(4));
        assert_eq!(ix.line_of("layers"), Some(5));
        assert_eq!(ix.line_of("layers[0]"), Some(6));
        assert_eq!(ix.line_of("layers[0].art.color"), Some(6));
        assert_eq!(ix.line_of("layers[1]"), Some(7));
        assert_eq!(ix.line_of("layers[1].art"), Some(9));
        assert_eq!(ix.line_of("timeline.tracks[0].events[0]"), Some(15));
        assert_eq!(ix.line_of("timeline.tracks[0].events[1]"), Some(16));
        assert_eq!(ix.line_of("timeline.tracks[0].events[1].kind"), Some(17));
        assert_eq!(
            ix.line_of("timeline.tracks[0].events[1].kind.voice"),
            Some(17)
        );
        assert_eq!(ix.line_of("raw"), Some(21));
        assert_eq!(ix.line_of("last"), Some(22));
    }

    #[test]
    fn missing_paths_fall_back_to_the_enclosing_value() {
        let ix = SpanIndex::new(TEXT);
        assert_eq!(ix.line_of("layers[0].pos"), Some(6));
        assert_eq!(
            ix.line_of("timeline.tracks[0].events[0].kind.nothing"),
            Some(15)
        );
        assert_eq!(ix.line_of("nonexistent"), Some(3));
    }

    #[test]
    fn text_keys_of_a_map_are_found_by_name() {
        let ix = SpanIndex::new(
            "(parts: [(\n  mouth_shapes: {\n    \"A\": Rect(w: 1.0),\n    \"D\": Circle(),\n  },\n)])",
        );
        assert_eq!(ix.line_of("parts[0].mouth_shapes.A"), Some(3));
        assert_eq!(ix.line_of("parts[0].mouth_shapes.D"), Some(4));
        assert_eq!(ix.line_of("parts[0].mouth_shapes"), Some(2));
    }

    #[test]
    fn survives_garbage() {
        for text in [
            "",
            "(",
            ")))",
            "[[[",
            "\"unterminated",
            "a: b: c",
            "/* open",
        ] {
            let ix = SpanIndex::new(text);
            let _ = ix.line_of("x.y[3]");
        }
    }
}
