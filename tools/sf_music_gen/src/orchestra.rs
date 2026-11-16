//! The orchestral layer of the combat track (WP11.4c): timpani, low brass, cello and the big accents, played
//! from real CC0 recordings (VSCO 2 Community Edition) by the integer player of [`crate::sampler`]. The layer
//! follows the bars and roots of the combat track exactly (128 BPM, 36 bars, nine sections of four bars, the
//! phrase A D D Bb C and the phrase B D G Bb A) and is our own arrangement; nothing is quoted from any piece.
//!
//! Sections of the combat track: 0 intro, 1 snare and pad, 2 stabs (B), 3 full (A), 4 breakdown (B), 5 build
//! (A), 6 full with lead (A), 7 full with lead (B), 8 outro (A). The layer, per section:
//!
//! * **Timpani** on the root, on beat one of every bar of sections 1, 2, 3, 5, 6, 7 and of the outro up to bar
//!   33; on beats one and three in the full sections 3, 6 and 7 (harder hits there); soft hits on the even bars
//!   of the breakdown (section 4).
//! * **Low brass** in the full sections and the first two bars of the outro: a held trombone on root + 12, a
//!   horn on root + 19 and a short tuba on the root.
//! * **Stabs**: trombone staccato on the 16ths 0, 3, 6 and 10 of every bar of sections 2 and 7, under the synth
//!   stabs.
//! * **Cello** spiccato eighths on root + 12 in sections 5, 6 and 7.
//! * **Accents**: a crash and the orchestral bass drum at bars 12 and 24, a gong at bar 16 (into the
//!   breakdown), a timpani roll over bar 23 (the last bar of the build) up to the D of bar 24.
//!
//! Every pitched sample is stored with its measured pitch in cents of a MIDI note (`pitch_cents`, 6000 =
//! middle C); a note is played from the sample nearest to it, shifted by the difference. The pitches are
//! pinned by tests that measure them again from the committed files.

use crate::sampler::{parse_wav, play};
use sf_sound_gen::{SAMPLE_RATE, ms};

/// One committed sample: where it came from and how it was cut.
pub struct SampleSpec {
    /// File stem in `tools/sf_music_gen/samples/`.
    pub id: &'static str,
    /// Path of the recording inside the VSCO 2 CE repository.
    pub src: &'static str,
    /// Pitch in cents of a MIDI note (3460 = 34.6, between A#1 and B1), 0 for a sound without a pitch.
    pub pitch_cents: i32,
    /// Milliseconds kept from the start of the recording.
    pub keep_ms: u32,
    /// Milliseconds of fade at the cut.
    pub fade_ms: u32,
    /// The committed WAV file.
    pub bytes: &'static [u8],
}

macro_rules! sample {
    ($id:literal, $src:literal, $pitch:expr, $keep:expr, $fade:expr) => {
        SampleSpec {
            id: $id,
            src: $src,
            pitch_cents: $pitch,
            keep_ms: $keep,
            fade_ms: $fade,
            bytes: include_bytes!(concat!("../samples/", $id, ".wav")),
        }
    };
}

/// The tuning of the VSCO file names: a note name with octave `o` is MIDI `12 * (o + 2) + pitch class`, one
/// octave below the usual C4 = 60 (checked by measurement, see the test `the_committed_samples_have_their_pitch`).
/// The timpani files carry no note; their pitch (and the roll's) is what the same test measures.
pub const SAMPLES: [SampleSpec; 32] = [
    sample!(
        "tim2_v4_rr1",
        "Percussion/Timpani/Timpani2_Hit_v4_rr1_Sum.wav",
        3440,
        2500,
        500
    ),
    sample!(
        "tim2_v4_rr2",
        "Percussion/Timpani/Timpani2_Hit_v4_rr2_Sum.wav",
        3440,
        2500,
        500
    ),
    sample!(
        "tim2_v3_rr1",
        "Percussion/Timpani/Timpani2_Hit_v3_rr1_Sum.wav",
        3440,
        2500,
        500
    ),
    sample!(
        "tim2_v3_rr2",
        "Percussion/Timpani/Timpani2_Hit_v3_rr2_Sum.wav",
        3440,
        2500,
        500
    ),
    sample!(
        "tim3_v4_rr1",
        "Percussion/Timpani/Timpani3_Hit_v4_rr1_Sum.wav",
        3750,
        2500,
        500
    ),
    sample!(
        "tim3_v4_rr2",
        "Percussion/Timpani/Timpani3_Hit_v4_rr2_Sum.wav",
        3750,
        2500,
        500
    ),
    sample!(
        "tim3_v3_rr1",
        "Percussion/Timpani/Timpani3_Hit_v3_rr1_Sum.wav",
        3750,
        2500,
        500
    ),
    sample!(
        "tim3_v3_rr2",
        "Percussion/Timpani/Timpani3_Hit_v3_rr2_Sum.wav",
        3750,
        2500,
        500
    ),
    sample!(
        "tim5_v4_rr1",
        "Percussion/Timpani/Timpani5_Hit_v4_rr1_Sum.wav",
        4270,
        2500,
        500
    ),
    sample!(
        "tim5_v4_rr2",
        "Percussion/Timpani/Timpani5_Hit_v4_rr2_Sum.wav",
        4270,
        2500,
        500
    ),
    sample!(
        "tim5_v3_rr1",
        "Percussion/Timpani/Timpani5_Hit_v3_rr1_Sum.wav",
        4270,
        2500,
        500
    ),
    sample!(
        "tim5_v3_rr2",
        "Percussion/Timpani/Timpani5_Hit_v3_rr2_Sum.wav",
        4270,
        2500,
        500
    ),
    sample!(
        "tim2_v1_rr1",
        "Percussion/Timpani/Timpani2_Hit_v1_rr1_Sum.wav",
        3440,
        2500,
        500
    ),
    sample!(
        "tim3_v1_rr1",
        "Percussion/Timpani/Timpani3_Hit_v1_rr1_Sum.wav",
        3750,
        2500,
        500
    ),
    sample!(
        "tim3_roll",
        "Percussion/Timpani/Rolls/Timpani3_Roll_v5_rr1_Sum.wav",
        3750,
        2000,
        100
    ),
    sample!(
        "bdrum",
        "Percussion/BDrumNewhit_v6_rr1_Sum.wav",
        0,
        2000,
        500
    ),
    sample!(
        "crash",
        "Percussion/cymbal-crash1_ff_rr1.wav",
        0,
        4000,
        1500
    ),
    sample!("gong", "Percussion/gongHit_f.wav", 0, 8000, 3000),
    sample!(
        "tbn_sus_as1",
        "Brass/Tenor Trombone/sus/tenortbn_sus_A#1_v2_1.wav",
        4600,
        2100,
        100
    ),
    sample!(
        "tbn_sus_d2",
        "Brass/Tenor Trombone/sus/tenortbn_sus_D2_v2_1.wav",
        5000,
        2100,
        100
    ),
    sample!(
        "tbn_sus_f2",
        "Brass/Tenor Trombone/sus/tenortbn_sus_F2_v2_1.wav",
        5300,
        2100,
        100
    ),
    sample!(
        "tbn_stac_as1",
        "Brass/Tenor Trombone/stac/tenortbn_stac_A#1_v3_rr1.wav",
        4600,
        500,
        40
    ),
    sample!(
        "tbn_stac_f2",
        "Brass/Tenor Trombone/stac/tenortbn_stac_F2_v3_rr1.wav",
        5300,
        500,
        40
    ),
    sample!(
        "horn_a2",
        "Brass/F Horn/sus/MOHorn_sus_A2_v2_1.wav",
        5700,
        2100,
        100
    ),
    sample!(
        "horn_c3",
        "Brass/F Horn/sus/MOHorn_sus_C3_v2_1.wav",
        6000,
        2100,
        100
    ),
    sample!(
        "horn_f2",
        "Brass/F Horn/sus/MOHorn_sus_F2_v2_1.wav",
        5300,
        2100,
        100
    ),
    sample!(
        "tuba_as0",
        "Brass/Tuba/stac/Tuba3_stac_A#0_v2_rr1_Sum.wav",
        3400,
        600,
        60
    ),
    sample!(
        "tuba_ds1",
        "Brass/Tuba/stac/Tuba3_stac_D#1_v2_rr1_Sum.wav",
        3900,
        600,
        60
    ),
    sample!(
        "tuba_f1",
        "Brass/Tuba/stac/Tuba3_stac_F1_v2_rr1_Sum.wav",
        4100,
        600,
        60
    ),
    sample!(
        "cello_b1",
        "Strings/Cello Section/spic/spic_B1_v2_RR1.wav",
        4700,
        400,
        40
    ),
    sample!(
        "cello_d2",
        "Strings/Cello Section/spic/spic_D2_v2_RR1.wav",
        5000,
        400,
        40
    ),
    sample!(
        "cello_f2",
        "Strings/Cello Section/spic/spic_F2_v2_RR1.wav",
        5300,
        400,
        40
    ),
];

/// The samples, parsed once.
pub struct Bank {
    data: Vec<Vec<i16>>,
}

impl Bank {
    pub fn load() -> Bank {
        Bank {
            data: SAMPLES.iter().map(|s| parse_wav(s.bytes)).collect(),
        }
    }

    fn index(id: &str) -> usize {
        SAMPLES
            .iter()
            .position(|s| s.id == id)
            .unwrap_or_else(|| panic!("no sample {id}"))
    }

    /// The samples of sample `id`.
    pub fn data(&self, id: &str) -> &[i16] {
        &self.data[Self::index(id)]
    }

    /// Play `id` unshifted (for sounds without a pitch).
    fn plain(&self, id: &str, max_len: usize) -> Vec<i32> {
        play(self.data(id), 0, max_len, ms(FADE_MS))
    }

    /// Play the one of `ids` whose pitch is nearest to MIDI note `target`, shifted onto it (the first of two
    /// equally near wins).
    fn note(&self, ids: &[&str], target: u32, max_len: usize) -> Vec<i32> {
        let want = target as i32 * 100;
        let id = ids
            .iter()
            .min_by_key(|id| (want - SAMPLES[Self::index(id)].pitch_cents).abs())
            .expect("a sample");
        let shift = want - SAMPLES[Self::index(id)].pitch_cents;
        play(self.data(id), shift, max_len, ms(FADE_MS))
    }
}

/// Fade at the end of every note that is cut short, in milliseconds.
const FADE_MS: u32 = 80;

const TIMPANI: [&str; 3] = ["tim2", "tim3", "tim5"];
const TROMBONE_SUS: [&str; 3] = ["tbn_sus_as1", "tbn_sus_d2", "tbn_sus_f2"];
const TROMBONE_STAC: [&str; 2] = ["tbn_stac_as1", "tbn_stac_f2"];
const HORN: [&str; 3] = ["horn_a2", "horn_c3", "horn_f2"];
const TUBA: [&str; 3] = ["tuba_as0", "tuba_ds1", "tuba_f1"];
const CELLO: [&str; 3] = ["cello_b1", "cello_d2", "cello_f2"];

/// The parts of the layer, kept apart so a test can look at each.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    Timpani,
    LowBrass,
    Stabs,
    Cello,
    Accents,
}

pub const PARTS: [Part; 5] = [
    Part::Timpani,
    Part::LowBrass,
    Part::Stabs,
    Part::Cello,
    Part::Accents,
];

/// Add `part` times `gain`/256 into the ring `buf` from sample `start`, wrapping round the end.
fn put_ring(buf: &mut [i32], start: usize, part: &[i32], gain: i32) {
    let n = buf.len();
    for (i, s) in part.iter().enumerate() {
        buf[(start + i) % n] += (i64::from(*s) * i64::from(gain) / 256) as i32;
    }
}

/// Sample at which 16th number `step` starts.
fn at(step: usize, bpm: u32) -> usize {
    (step as u64 * u64::from(SAMPLE_RATE) * 15 / u64::from(bpm)) as usize
}

/// A timpani hit on MIDI note `root` (an octave up when the root is below 33, which is lower than the drums
/// go), of velocity layer `vel` (3 or 4) and round robin `rr` (1 or 2).
fn timpani_hit(bank: &Bank, root: u32, vel: u32, rr: u32, max_len: usize) -> Vec<i32> {
    let target = if root >= 33 { root } else { root + 12 };
    // Velocity 1 exists for drums 2 and 3 only (the breakdown never needs drum 5).
    let drums = if vel == 1 {
        &TIMPANI[..2]
    } else {
        &TIMPANI[..]
    };
    let ids: Vec<String> = drums.iter().map(|t| format!("{t}_v{vel}_rr{rr}")).collect();
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    bank.note(&ids, target, max_len)
}

/// The layer, part by part, as rings of `bars * 16` sixteenths at `bpm`. `root_of(bar)` is the root (MIDI) of
/// the bar. 32767 is full scale, the same unit as the track; the gains are in 1/256 of that.
pub fn combat_layers(
    bpm: u32,
    bars: usize,
    root_of: impl Fn(usize) -> u32,
) -> Vec<(Part, Vec<i32>)> {
    let bank = Bank::load();
    let n = at(bars * 16, bpm);
    let bar_len = at(16, bpm);
    let mut timp = vec![0i32; n];
    let mut brass = vec![0i32; n];
    let mut stabs = vec![0i32; n];
    let mut cello = vec![0i32; n];
    let mut acc = vec![0i32; n];
    for bar in 0..bars {
        let sec = bar / 4;
        let root = root_of(bar);
        let t0 = at(bar * 16, bpm);
        let full = matches!(sec, 3 | 6 | 7);
        let outro_start = sec == 8 && bar < 34;
        // Timpani: beat one in most sections, beats one and three in the full ones, soft in the breakdown.
        if matches!(sec, 1 | 2 | 3 | 5 | 6 | 7) || outro_start {
            let (vel, g) = if full { (4, 141) } else { (3, 102) };
            let hit = timpani_hit(&bank, root, vel, 1 + bar as u32 % 2, usize::MAX);
            put_ring(&mut timp, t0, &hit, g);
            if full {
                let hit = timpani_hit(&bank, root, 3, 2 - bar as u32 % 2, usize::MAX);
                put_ring(&mut timp, at(bar * 16 + 8, bpm), &hit, 102);
            }
        }
        if sec == 4 && bar % 2 == 0 {
            let hit = timpani_hit(&bank, root, 1, 1, usize::MAX);
            put_ring(&mut timp, t0, &hit, 90);
        }
        // Low brass: held trombone and horn, a short tuba.
        if full || outro_start {
            let held = bar_len * 95 / 100;
            put_ring(
                &mut brass,
                t0,
                &bank.note(&TROMBONE_SUS, root + 12, held),
                72,
            );
            put_ring(&mut brass, t0, &bank.note(&HORN, root + 19, held), 56);
            put_ring(&mut brass, t0, &bank.note(&TUBA, root, ms(400)), 77);
        }
        // Trombone staccato stabs under the synth stabs.
        if matches!(sec, 2 | 7) {
            for s in [0usize, 3, 6, 10] {
                let v = bank.note(&TROMBONE_STAC, root + 12, ms(250));
                put_ring(&mut stabs, at(bar * 16 + s, bpm), &v, 64);
            }
        }
        // Cello spiccato eighths.
        if (5..=7).contains(&sec) {
            for s in (0..16usize).step_by(2) {
                let v = bank.note(&CELLO, root + 12, ms(200));
                let g = if s % 4 == 0 { 56 } else { 41 };
                put_ring(&mut cello, at(bar * 16 + s, bpm), &v, g);
            }
        }
        // Accents.
        if bar == 12 || bar == 24 {
            put_ring(&mut acc, t0, &bank.plain("crash", usize::MAX), 77);
            put_ring(&mut acc, t0, &bank.plain("bdrum", usize::MAX), 128);
        }
        if bar == 16 {
            put_ring(&mut acc, t0, &bank.plain("gong", usize::MAX), 90);
        }
        if bar == 23 {
            // The roll runs into the D of bar 24, half a semitone up from where it was recorded.
            let want = 38 * 100 - SAMPLES[Bank::index("tim3_roll")].pitch_cents;
            let roll = play(bank.data("tim3_roll"), want, bar_len, ms(FADE_MS));
            put_ring(&mut acc, t0, &roll, 115);
        }
    }
    vec![
        (Part::Timpani, timp),
        (Part::LowBrass, brass),
        (Part::Stabs, stabs),
        (Part::Cello, cello),
        (Part::Accents, acc),
    ]
}

#[cfg(test)]
mod tests {
    use super::put_ring;

    #[test]
    fn put_ring_wraps_round_the_end_and_scales_by_the_gain() {
        let mut ring = vec![0i32; 8];
        put_ring(&mut ring, 6, &[256, 512, 768, 1024], 128);
        // 6 and 7 at the end, then 0 and 1 at the start; gain 128/256 halves.
        assert_eq!(ring, vec![384, 512, 0, 0, 0, 0, 128, 256]);
        // A second put adds.
        put_ring(&mut ring, 7, &[256], 256);
        assert_eq!(ring[7], 512);
    }
}
