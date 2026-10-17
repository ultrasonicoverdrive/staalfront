//! Tests of the flap fallback and of the per-line lookup.

use proptest::prelude::*;

use super::flap::{MIN_SLOT_MS, flap};
use super::{MouthCueSet, MouthCues, MouthShape, line_shape, parse_cues};
use crate::math::Ms;
use MouthShape::{A, B, C, D, E, G, X};

fn shapes(c: &MouthCues) -> Vec<(Ms, MouthShape)> {
    c.cues().iter().map(|c| (c.start_ms, c.shape)).collect()
}

const LINE: &str = "The relay went dark at 04:10. By dawn, every unit north of the ridge was operating on stale orders.";

#[test]
fn the_flap_is_the_same_every_time_and_depends_on_the_text() {
    let a = flap(LINE, 7600);
    assert_eq!(a, flap(LINE, 7600));
    assert_eq!(a, MouthCues::flap(LINE, 7600));
    assert_ne!(a, flap("Operating on nothing, Captain.", 7600));
}

#[test]
fn a_known_text_gives_the_expected_slots() {
    // go: E; on: E; a rest between the words; 125 ms per slot in a long line.
    assert_eq!(
        shapes(&flap("Go on", 3000)),
        [(0, E), (125, X), (250, E), (375, X)]
    );
    // map: closed lips for the M, then the A. make: the final e is silent.
    assert_eq!(shapes(&flap("map", 3000)), [(0, A), (125, D), (250, X)]);
    assert_eq!(shapes(&flap("make", 3000)), [(0, A), (125, D), (250, X)]);
    // table keeps both syllables ("le" is not silent).
    assert_eq!(shapes(&flap("table", 3000)), [(0, D), (125, C), (250, X)]);
    // very: the V puts the teeth on the lip; two syllables, the second a Y.
    assert_eq!(
        shapes(&flap("very", 3000)),
        [(0, G), (125, C), (250, B), (375, X)]
    );
    // A word without a vowel still moves the mouth.
    assert_eq!(shapes(&flap("hmm", 3000)), [(0, C), (125, X)]);
    // A sentence end rests for two slots.
    assert_eq!(
        shapes(&flap("Go. Go", 3000)),
        [(0, E), (125, X), (375, E), (500, X)]
    );
    // Accents and capitals are vowels like any other.
    assert_eq!(shapes(&flap("ÉLAN", 3000)), shapes(&flap("elan", 3000)));
}

#[test]
fn text_without_letters_or_a_line_without_time_is_rest() {
    for (text, d) in [
        ("", 2000),
        ("... -- 04:10 ...", 2000),
        ("hello", 0),
        ("hello", -5),
    ] {
        assert_eq!(shapes(&flap(text, d)), [(0, X)], "{text:?} {d}");
    }
}

#[test]
fn words_are_separated_by_rest() {
    let s = shapes(&flap("Hold the line until dawn", 4000));
    // Five words: at least four rests between them.
    assert!(s.iter().filter(|(_, sh)| *sh == X).count() >= 4, "{s:?}");
    // The same shape never follows itself (it would be one cue).
    assert!(s.windows(2).all(|w| w[0].1 != w[1].1), "{s:?}");
}

#[test]
fn the_flap_moves_at_most_10_times_a_second_and_around_8_when_the_text_fills_the_line() {
    let s = shapes(&flap(LINE, 7600));
    // Never faster than one change per 100 ms.
    assert!(
        s.windows(2).all(|w| w[1].0 - w[0].0 >= MIN_SLOT_MS),
        "{s:?}"
    );
    // Changes per second over the part of the line that moves.
    let moving = s.last().unwrap().0;
    let per_second = (s.len() - 1) as f64 / (f64::from(moving) / 1000.0);
    assert!((5.0..=10.0).contains(&per_second), "{per_second} {s:?}");
}

#[test]
fn a_short_text_stops_and_a_long_text_is_cut_at_the_end_of_the_line() {
    // One word in five seconds: it moves for a moment and then rests.
    let f = flap("Go", 5000);
    assert_eq!(shapes(&f), [(0, E), (125, X)]);
    assert_eq!(f.shape_at(4999), X);
    // A long text in a short line: slots are 100 ms and the rest is cut off.
    let s = shapes(&flap(LINE, 1000));
    assert!(s.iter().all(|(t, _)| *t < 1000), "{s:?}");
    assert!(
        s.windows(2).all(|w| w[1].0 - w[0].0 >= MIN_SLOT_MS),
        "{s:?}"
    );
    assert!(s.len() >= 6, "{s:?}");
}

#[test]
fn letters_of_every_script_never_panic() {
    for text in [
        "Привет, мир",
        "日本語のテスト",
        "ǅ ŉ ß ǰ İstanbul",
        "a\u{0300}\u{0301} \u{200b} \t\n x",
        "😀 :) <3",
    ] {
        let f = flap(text, 3000);
        assert_eq!(f.shape_at(-1), X);
        assert!(!f.is_empty());
    }
}

proptest! {
    #[test]
    fn the_flap_is_well_formed_for_any_text_and_length(text in ".{0,200}", d in -10i32..20_000) {
        let f = flap(&text, d);
        let s = shapes(&f);
        prop_assert!(!s.is_empty());
        prop_assert_eq!(s[0].0, 0);
        prop_assert!(s.windows(2).all(|w| w[0].0 < w[1].0), "increasing");
        prop_assert!(s.windows(2).all(|w| w[0].1 != w[1].1), "no repeated shape");
        if d > 0 {
            prop_assert!(s.iter().all(|(t, _)| *t < d), "inside the line");
        }
        prop_assert_eq!(&f, &flap(&text, d));
    }
}

// -- the lookup for a line --------------------------------------------------

fn set_with(path: &str, text: &str) -> MouthCueSet {
    let mut set = MouthCueSet::default();
    set.insert(path, parse_cues(text, 2000).unwrap().cues);
    set
}

#[test]
fn a_line_with_a_loaded_cue_file_uses_it() {
    let set = set_with("v/l1.tsv", "0.00\tX\n0.50\tD\n1.00\tA\n");
    let at = |e| line_shape(&set, Some("v/l1.tsv"), "Hello there.", e, 2000);
    assert_eq!(at(0), X);
    assert_eq!(at(499), X);
    assert_eq!(at(500), D);
    assert_eq!(at(999), D);
    assert_eq!(at(1000), A);
    assert_eq!(at(1999), A);
}

#[test]
fn outside_the_line_the_mouth_rests() {
    let set = set_with("v/l1.tsv", "0.00\tD\n");
    assert_eq!(line_shape(&set, Some("v/l1.tsv"), "Hi.", -1, 2000), X);
    assert_eq!(line_shape(&set, Some("v/l1.tsv"), "Hi.", 0, 2000), D);
    assert_eq!(line_shape(&set, Some("v/l1.tsv"), "Hi.", 1999, 2000), D);
    assert_eq!(line_shape(&set, Some("v/l1.tsv"), "Hi.", 2000, 2000), X);
    assert_eq!(line_shape(&set, None, "Hi.", 2000, 2000), X);
}

#[test]
fn a_line_without_a_cue_file_or_with_one_that_is_not_loaded_flaps() {
    let set = set_with("v/l1.tsv", "0.00\tD\n");
    let text = "Hold the line until dawn";
    let flapped = flap(text, 4000);
    let want: Vec<MouthShape> = (0..4000).step_by(37).map(|e| flapped.shape_at(e)).collect();
    for lipsync in [None, Some("v/other.tsv")] {
        let got: Vec<MouthShape> = (0..4000)
            .step_by(37)
            .map(|e| line_shape(&set, lipsync, text, e, 4000))
            .collect();
        assert_eq!(got, want, "{lipsync:?}");
    }
    assert!(want.iter().any(|s| *s != X), "the flap moves the mouth");
}

// -- random cue files never panic ------------------------------------------------

use super::parse::{parse_json, parse_tsv};

fn seconds(ms: u32) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

fn assert_well_formed(c: &MouthCues, duration: Ms) {
    // (A file whose cues all come after the line ends gives an empty series: rest.)
    let s = shapes(c);
    assert!(
        s.windows(2).all(|w| w[0].0 < w[1].0),
        "sorted, no two at one time: {s:?}"
    );
    assert!(
        s.iter().all(|(t, _)| *t >= 0 && *t < duration.max(1)),
        "inside the line: {s:?}"
    );
    // Lookups work everywhere, and the series never overlaps itself: the shape in
    // force at a cue's start is that cue's.
    for (t, shape) in &s {
        assert_eq!(c.shape_at(*t), *shape);
    }
    for t in [-1, 0, duration - 1, duration, duration + 1] {
        let _ = c.shape_at(t);
    }
}

proptest! {
    /// Any list of cues, in any order, with any lengths (including none) and
    /// times that collide: the reader either refuses it or gives a sorted series.
    #[test]
    fn random_json_cue_lists_never_panic(
        cues in proptest::collection::vec((0u32..4000, 0u32..4000, 0usize..9, any::<bool>()), 0..40),
        duration in 1i32..5000,
    ) {
        let body: Vec<String> = cues
            .iter()
            .map(|&(start, end, shape, with_end)| {
                let end = if with_end { format!(", \"end\": {}", seconds(end)) } else { String::new() };
                format!(
                    "{{ \"start\": {}{end}, \"value\": \"{}\" }}",
                    seconds(start),
                    MouthShape::ALL[shape]
                )
            })
            .collect();
        let text = format!("{{ \"mouthCues\": [ {} ] }}", body.join(", "));
        if let Ok(p) = parse_json(&text, duration) {
            assert_well_formed(&p.cues, duration);
        }
    }

    #[test]
    fn random_tsv_cue_lists_never_panic(
        cues in proptest::collection::vec((0u32..4000, 0usize..9), 0..40),
        duration in 1i32..5000,
    ) {
        let text: String = cues
            .iter()
            .map(|&(t, shape)| format!("{}\t{}\n", seconds(t), MouthShape::ALL[shape]))
            .collect();
        if let Ok(p) = parse_tsv(&text, duration) {
            assert_well_formed(&p.cues, duration);
        }
    }

    /// Whatever text it is given, in either format.
    #[test]
    fn random_text_never_panics(text in ".{0,300}", duration in -5i32..5000) {
        if let Ok(p) = parse_cues(&text, duration) {
            assert_well_formed(&p.cues, duration);
        }
    }

    /// Cue lists built by hand, in any order and with repeats.
    #[test]
    fn hand_built_series_are_sorted_and_unique(
        cues in proptest::collection::vec((-100i32..500, 0usize..9), 0..30),
    ) {
        let c = MouthCues::new(
            cues.iter()
                .map(|&(t, s)| super::MouthCue { start_ms: t, shape: MouthShape::ALL[s] })
                .collect(),
        );
        let s = shapes(&c);
        prop_assert!(s.windows(2).all(|w| w[0].0 < w[1].0));
    }
}
