//! Tests of the cue-file readers, with hand-written fixtures (no recordings).

use super::parse::{Parsed, parse_json, parse_tsv};
use super::{MouthCue, MouthCues, MouthShape, parse_cues};
use crate::math::Ms;
use MouthShape::{A, B, C, D, X};

/// Rhubarb's JSON output, as its documentation shows it (hand-written).
const JSON: &str = r#"{
  "metadata": {
    "soundFile": "l001.wav",
    "duration": 1.50
  },
  "mouthCues": [
    { "start": 0.00, "end": 0.19, "value": "X" },
    { "start": 0.19, "end": 0.37, "value": "B" },
    { "start": 0.37, "end": 0.65, "value": "D" },
    { "start": 0.65, "end": 0.80, "value": "A" },
    { "start": 0.80, "end": 1.20, "value": "C" },
    { "start": 1.20, "end": 1.50, "value": "X" }
  ]
}
"#;

/// The same speech as TSV.
const TSV: &str = "0.00\tX\n0.19\tB\n0.37\tD\n0.65\tA\n0.80\tC\n1.20\tX\n";

fn series(p: &Parsed) -> Vec<(Ms, MouthShape)> {
    p.cues
        .cues()
        .iter()
        .map(|c| (c.start_ms, c.shape))
        .collect()
}

const EXPECTED: [(Ms, MouthShape); 6] = [(0, X), (190, B), (370, D), (650, A), (800, C), (1200, X)];

#[test]
fn json_becomes_integer_milliseconds() {
    let p = parse_json(JSON, 1500).unwrap();
    assert_eq!(series(&p), EXPECTED);
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
}

#[test]
fn tsv_becomes_the_same_series() {
    let p = parse_tsv(TSV, 1500).unwrap();
    assert_eq!(series(&p), EXPECTED);
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    assert_eq!(
        series(&parse_cues(TSV, 1500).unwrap()),
        series(&parse_cues(JSON, 1500).unwrap())
    );
}

#[test]
fn the_series_is_looked_up_at_cue_boundaries() {
    let c = parse_json(JSON, 1500).unwrap().cues;
    // A cue is in force from its own start time, not one millisecond later.
    assert_eq!(c.shape_at(-1), X);
    assert_eq!(c.shape_at(0), X);
    assert_eq!(c.shape_at(189), X);
    assert_eq!(c.shape_at(190), B);
    assert_eq!(c.shape_at(369), B);
    assert_eq!(c.shape_at(370), D);
    assert_eq!(c.shape_at(649), D);
    assert_eq!(c.shape_at(650), A);
    assert_eq!(c.shape_at(1199), C);
    assert_eq!(c.shape_at(1200), X);
    assert_eq!(c.shape_at(1_000_000), X);
}

#[test]
fn seconds_are_rounded_to_the_nearest_millisecond() {
    let p = parse_tsv("0.0004\tX\n0.0006\tA\n0.1236\tB\n2\tC\n", 5000).unwrap();
    assert_eq!(series(&p), [(0, X), (1, A), (124, B), (2000, C)]);
}

#[test]
fn crlf_a_byte_order_mark_blank_lines_and_spaces_are_fine() {
    let p = parse_tsv("\u{feff}0.00\tX\r\n\r\n0.50  A\r\n\r\n", 1000).unwrap();
    assert_eq!(series(&p), [(0, X), (500, A)]);
    let p = parse_json(&format!("\u{feff}{}", JSON.replace('\n', "\r\n")), 1500).unwrap();
    assert_eq!(series(&p), EXPECTED);
}

#[test]
fn the_format_is_found_from_the_first_character() {
    assert_eq!(series(&parse_cues(JSON, 1500).unwrap()), EXPECTED);
    assert_eq!(
        series(&parse_cues(&format!("\n\n  {JSON}"), 1500).unwrap()),
        EXPECTED
    );
    assert_eq!(series(&parse_cues(TSV, 1500).unwrap()), EXPECTED);
}

// -- errors -------------------------------------------------------------

#[test]
fn an_unknown_shape_is_an_error_with_its_line_in_tsv() {
    let e = parse_tsv("0.00\tX\n0.19\tB\n0.37\tZ\n0.65\tA\n", 1000).unwrap_err();
    assert_eq!(e.line, 3);
    assert!(e.message.contains("unknown mouth shape \"Z\""), "{e}");
    assert!(e.message.contains("A B C D E F G H X"), "{e}");
    assert!(e.to_string().starts_with("line 3: "), "{e}");
}

#[test]
fn an_unknown_shape_is_an_error_with_its_line_in_json() {
    let bad = JSON.replace("\"value\": \"A\"", "\"value\": \"Q\"");
    let e = parse_json(&bad, 1500).unwrap_err();
    assert_eq!(e.line, 10, "{e}");
    assert!(e.message.contains("unknown mouth shape \"Q\""), "{e}");
}

#[test]
fn shapes_are_exact_capital_letters() {
    for bad in ["x", "AB", "I", "0", "rest", "ß"] {
        let e = parse_tsv(&format!("0.0\tX\n0.1\t{bad}\n"), 1000).unwrap_err();
        assert_eq!(e.line, 2, "{bad}");
    }
    let e = parse_json(&JSON.replace("\"B\"", "\"b\""), 1500).unwrap_err();
    assert_eq!(e.line, 8, "{e}");
    let e = parse_json(&JSON.replace("\"B\"", "7"), 1500).unwrap_err();
    assert!(e.message.contains("shape letter"), "{e}");
}

#[test]
fn malformed_tsv_lines_are_errors_with_their_line() {
    for (text, line) in [
        ("0.00\tX\n0.5\n", 2),
        ("0.00\tX\nabc\tA\n", 2),
        ("0.00\tX\n0.5\tA\tB\n", 2),
        ("-0.1\tX\n", 1),
        ("0.00\tX\n\n\n99999999\tA\n", 4),
        ("NaN\tX\n", 1),
    ] {
        let e = parse_tsv(text, 1000).unwrap_err();
        assert_eq!(e.line, line, "{text:?}: {e}");
    }
}

#[test]
fn malformed_json_is_an_error_with_a_line() {
    // A missing comma after the second cue.
    let bad = JSON.replacen("\"value\": \"B\" },", "\"value\": \"B\" }", 1);
    let e = parse_json(&bad, 1500).unwrap_err();
    assert_eq!(e.line, 9, "{e}");
    assert!(e.message.contains("expected `,` or `]`"), "{e}");

    let e = parse_json("{ \"mouthCues\": [", 1500).unwrap_err();
    assert!(e.message.contains("the file ends"), "{e}");
    let e = parse_json(&JSON.replace("0.19, \"end\"", "x, \"end\""), 1500).unwrap_err();
    assert_eq!(e.line, 8, "{e}");
}

#[test]
fn json_that_is_not_a_cue_list_is_explained() {
    let e = parse_json("[]", 1000).unwrap_err();
    assert!(e.message.contains("mouthCues"), "{e}");
    let e = parse_json("{ \"metadata\": {} }", 1000).unwrap_err();
    assert!(e.message.contains("no \"mouthCues\""), "{e}");
    let e = parse_json("{ \"mouthCues\": 3 }", 1000).unwrap_err();
    assert!(e.message.contains("must be a list"), "{e}");
    let e = parse_json("{ \"mouthCues\": [] }", 1000).unwrap_err();
    assert!(e.message.contains("no mouth cues"), "{e}");
    let e = parse_json("{ \"mouthCues\": [ 3 ] }", 1000).unwrap_err();
    assert!(e.message.contains("must be an object"), "{e}");
    let e = parse_json("{ \"mouthCues\": [ { \"start\": 0 } ] }", 1000).unwrap_err();
    assert!(e.message.contains("without \"value\""), "{e}");
    let e = parse_json("{ \"mouthCues\": [ { \"value\": \"A\" } ] }", 1000).unwrap_err();
    assert!(e.message.contains("without \"start\""), "{e}");
    let e = parse_json(
        "{ \"mouthCues\": [ { \"start\": \"0\", \"value\": \"A\" } ] }",
        1000,
    )
    .unwrap_err();
    assert!(e.message.contains("number of seconds"), "{e}");
    let e = parse_json(
        "{ \"mouthCues\": [ { \"start\": 1, \"end\": 0.5, \"value\": \"A\" } ] }",
        5000,
    )
    .unwrap_err();
    assert!(e.message.contains("ends before it starts"), "{e}");
    assert!(parse_json(&format!("{JSON} trailing"), 1500).is_err());
    assert!(parse_tsv("", 1000).is_err());
    assert!(parse_tsv("\n\n", 1000).is_err());
}

#[test]
fn deeply_nested_json_is_refused_not_overflowed() {
    let text = format!("{}{}", "[".repeat(5000), "]".repeat(5000));
    let e = parse_json(&text, 1000).unwrap_err();
    assert!(e.message.contains("deeply"), "{e}");
}

// -- warnings and clipping -------------------------------------------------

#[test]
fn cues_out_of_order_are_sorted_with_a_warning() {
    let p = parse_tsv("0.00\tX\n0.50\tA\n0.20\tB\n0.80\tC\n", 2000).unwrap();
    assert_eq!(series(&p), [(0, X), (200, B), (500, A), (800, C)]);
    assert_eq!(p.warnings.len(), 1);
    assert_eq!(p.warnings[0].line, 3);
    assert!(p.warnings[0].message.contains("sorted"), "{:?}", p.warnings);
}

#[test]
fn two_cues_at_one_time_keep_the_later() {
    let p = parse_tsv("0.00\tX\n0.50\tA\n0.50\tD\n1.00\tX\n", 2000).unwrap();
    assert_eq!(series(&p), [(0, X), (500, D), (1000, X)]);
    assert_eq!(p.warnings.len(), 1);
    assert_eq!(p.warnings[0].line, 3);
}

#[test]
fn cues_after_the_end_of_the_line_are_dropped_with_one_warning() {
    let p = parse_tsv("0.00\tX\n0.50\tA\n1.00\tB\n1.50\tC\n2.00\tD\n", 1200).unwrap();
    assert_eq!(series(&p), [(0, X), (500, A), (1000, B)]);
    assert_eq!(p.warnings.len(), 1, "{:?}", p.warnings);
    assert_eq!(p.warnings[0].line, 4);
    assert!(p.warnings[0].message.contains("2 cues"), "{:?}", p.warnings);
    assert!(
        p.warnings[0].message.contains("1200 ms"),
        "{:?}",
        p.warnings
    );
    // A cue exactly at the end is outside the line (the line is [0, duration)).
    let p = parse_tsv("0.00\tX\n1.20\tA\n", 1200).unwrap();
    assert_eq!(series(&p), [(0, X)]);
    assert_eq!(p.warnings.len(), 1);
}

#[test]
fn a_last_cue_that_runs_past_the_end_is_clipped_with_a_warning() {
    let p = parse_json(JSON, 1300).unwrap();
    assert_eq!(series(&p), EXPECTED, "the series itself is unchanged");
    // The last cue (1.20-1.50) is the one that runs past 1300 ms.
    assert_eq!(p.warnings.len(), 1, "{:?}", p.warnings);
    assert_eq!(p.warnings[0].line, 12);
    assert!(
        p.warnings[0].message.contains("clipped"),
        "{:?}",
        p.warnings
    );
    // In range: nothing to say.
    assert!(parse_json(JSON, 1500).unwrap().warnings.is_empty());
    assert!(parse_json(JSON, 5000).unwrap().warnings.is_empty());
}

#[test]
fn a_shorter_line_cuts_the_series_where_the_line_ends() {
    let p = parse_json(JSON, 700).unwrap();
    assert_eq!(series(&p), [(0, X), (190, B), (370, D), (650, A)]);
    assert_eq!(p.warnings.len(), 1);
    assert!(p.warnings[0].message.contains("2 cues"), "{:?}", p.warnings);
}

#[test]
fn gaps_in_json_are_rest() {
    let text = r#"{ "mouthCues": [
        { "start": 0.0, "end": 0.2, "value": "A" },
        { "start": 0.5, "end": 0.7, "value": "D" }
    ] }"#;
    let p = parse_json(text, 1000).unwrap();
    assert_eq!(series(&p), [(0, A), (200, X), (500, D), (700, X)]);
    assert!(p.warnings.is_empty());
    // No `end` at all is read like TSV.
    let text =
        r#"{ "mouthCues": [ { "start": 0.1, "value": "A" }, { "start": 0.5, "value": "X" } ] }"#;
    assert_eq!(
        series(&parse_json(text, 1000).unwrap()),
        [(100, A), (500, X)]
    );
}

#[test]
fn overlapping_json_cues_warn_and_the_later_start_wins() {
    let text = r#"{ "mouthCues": [
        { "start": 0.0, "end": 0.6, "value": "A" },
        { "start": 0.4, "end": 0.7, "value": "D" }
    ] }"#;
    let p = parse_json(text, 1000).unwrap();
    assert_eq!(series(&p), [(0, A), (400, D), (700, X)]);
    assert_eq!(p.warnings.len(), 1);
    assert_eq!(p.warnings[0].line, 3);
}

#[test]
fn a_series_before_its_first_cue_is_rest() {
    let p = parse_tsv("0.30\tA\n", 1000).unwrap();
    assert_eq!(p.cues.shape_at(0), X);
    assert_eq!(p.cues.shape_at(299), X);
    assert_eq!(p.cues.shape_at(300), A);
}

#[test]
fn cue_lists_built_by_hand_are_sorted_and_deduplicated() {
    let c = MouthCues::new(vec![
        MouthCue {
            start_ms: 200,
            shape: B,
        },
        MouthCue {
            start_ms: 0,
            shape: X,
        },
        MouthCue {
            start_ms: 200,
            shape: D,
        },
    ]);
    assert_eq!(c.len(), 2);
    assert_eq!(c.shape_at(250), D);
}
