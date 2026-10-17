//! The "flap" fallback: mouth movement made from the text of a line, for the
//! time before a voice has been recorded and run through Rhubarb.
//!
//! It is a pure function of the text and the length of the line. Each word
//! becomes a few slots (one per syllable, found by counting groups of vowels,
//! with a closed-lips slot before a word that starts with B, M or P and a
//! teeth slot before F or V), words are separated by a rest slot, a sentence
//! end by two. The slots share the line's time, but a slot is never shorter
//! than 100 ms or longer than 125 ms, which is 8 to 10 changes per second: a
//! short text stops moving before the line ends, a long text is cut off at the
//! end of the line. The shape of a syllable comes from its first vowel.

use super::{MouthCue, MouthCues, MouthShape};
use crate::math::Ms;

/// The fastest the flap moves: one slot per this many milliseconds.
pub const MIN_SLOT_MS: Ms = 100;
/// The slowest the flap moves.
pub const MAX_SLOT_MS: Ms = 125;

/// The mouth for a vowel: open wide for A, open for E, narrow for I, round for
/// O, pucker for U.
fn vowel_shape(c: char) -> Option<MouthShape> {
    Some(match c {
        'a' | 'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => MouthShape::D,
        'e' | 'è' | 'é' | 'ê' | 'ë' => MouthShape::C,
        'i' | 'y' | 'ì' | 'í' | 'î' | 'ï' => MouthShape::B,
        'o' | 'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => MouthShape::E,
        'u' | 'ù' | 'ú' | 'û' | 'ü' => MouthShape::F,
        _ => return None,
    })
}

/// The slots of one word: onset, then one per syllable. Empty for a word with
/// no letters.
fn word_slots(word: &str) -> Vec<MouthShape> {
    let letters: Vec<char> = word
        .chars()
        .filter(|c| c.is_alphabetic())
        .flat_map(char::to_lowercase)
        .collect();
    let Some(&first) = letters.first() else {
        return Vec::new();
    };
    let mut slots = Vec::new();
    match first {
        'b' | 'm' | 'p' => slots.push(MouthShape::A),
        'f' | 'v' => slots.push(MouthShape::G),
        _ => {}
    }
    // One syllable per run of vowels, shaped by the run's first vowel.
    let mut nuclei: Vec<MouthShape> = Vec::new();
    let mut in_run = false;
    for &c in &letters {
        match vowel_shape(c) {
            Some(s) if !in_run => {
                nuclei.push(s);
                in_run = true;
            }
            Some(_) => {}
            None => in_run = false,
        }
    }
    // A final "e" after a consonant is usually silent ("make", "base"), unless
    // it is the only syllable or the word ends in "le".
    if nuclei.len() > 1
        && letters.last() == Some(&'e')
        && letters.len() >= 2
        && vowel_shape(letters[letters.len() - 2]).is_none()
        && letters[letters.len() - 2] != 'l'
    {
        nuclei.pop();
    }
    if nuclei.is_empty() {
        // No vowel at all ("hmm", "TNT"): a short open mouth.
        nuclei.push(if slots.first() == Some(&MouthShape::A) {
            MouthShape::B
        } else {
            MouthShape::C
        });
    }
    slots.extend(nuclei);
    slots
}

/// The flap for `text` spoken over `duration_ms`. Always starts with a cue at
/// 0 ms; cue times are strictly increasing and below `duration_ms`.
pub fn flap(text: &str, duration_ms: Ms) -> MouthCues {
    let mut slots: Vec<MouthShape> = Vec::new();
    for token in text.split_whitespace() {
        let word = word_slots(token);
        if word.is_empty() {
            continue;
        }
        if !slots.is_empty() {
            slots.push(MouthShape::X);
        }
        slots.extend(word);
        if token.ends_with(['.', '!', '?', '…']) {
            slots.push(MouthShape::X);
        }
    }
    if slots.is_empty() || duration_ms <= 0 {
        return MouthCues::from_sorted(vec![MouthCue {
            start_ms: 0,
            shape: MouthShape::X,
        }]);
    }
    let slot_ms = (duration_ms / slots.len() as Ms).clamp(MIN_SLOT_MS, MAX_SLOT_MS);
    let mut cues: Vec<MouthCue> = Vec::new();
    let mut push = |start_ms: Ms, shape: MouthShape| {
        if start_ms < duration_ms && cues.last().is_none_or(|c| c.shape != shape) {
            cues.push(MouthCue { start_ms, shape });
        }
    };
    for (i, &shape) in slots.iter().enumerate() {
        push(i as Ms * slot_ms, shape);
    }
    // After the last slot the mouth rests.
    push(slots.len() as Ms * slot_ms, MouthShape::X);
    MouthCues::from_sorted(cues)
}
