//! Scanning, mapping and fallback, on fixtures built here (own encoder, own
//! MIX writer, own RSA test key). No real Red Alert file is ever used.

use super::aud::{Compression, Pcm, encode};
use super::crypto::{BLOWFISH_KEY_LEN, test_keypair};
use super::mix::{MixFormat, mix_id, read_index, write_mix};
use super::*;
use std::io::Cursor;

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sf_audio_ra_{}_{name}", std::process::id()));
    std::fs::remove_dir_all(&d).ok();
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn beep(frames: usize) -> Pcm {
    Pcm {
        sample_rate: 22050,
        channels: 1,
        samples: (0..frames)
            .map(|i| if (i / 20) % 2 == 0 { 8000 } else { -8000 })
            .collect(),
    }
}

fn mapping(pairs: &[(&str, &str)]) -> Mapping {
    Mapping {
        sounds: pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect(),
        names: vec!["SOUNDS.MIX".into()],
    }
}

/// A fake install: `MAIN.MIX` (encrypted with the test key) holding
/// `SOUNDS.MIX` (plain) with two AUDs and a broken one, plus a loose AUD.
fn fake_install(dir: &Path) {
    let (_, private) = test_keypair();
    let ima = encode(&beep(2000), Compression::Ima);
    let ws = encode(&beep(1000), Compression::Westwood);
    let mut broken = ima.clone();
    broken.truncate(40);
    broken[2..6].copy_from_slice(&1000u32.to_le_bytes());
    let sounds = write_mix(
        &[
            (mix_id("CLICK.AUD"), &ima),
            (mix_id("BOOM.AUD"), &ws),
            (mix_id("BAD.AUD"), &broken),
            (mix_id("PALETTE.PAL"), &[1, 2, 3]),
        ],
        MixFormat::Plain,
    );
    let main = write_mix(
        &[(mix_id("SOUNDS.MIX"), &sounds), (mix_id("README"), b"hi")],
        MixFormat::Encrypted {
            private: &private,
            blowfish: [0x42; BLOWFISH_KEY_LEN],
        },
    );
    std::fs::create_dir_all(dir.join("cd1")).unwrap();
    std::fs::write(dir.join("cd1/MAIN.MIX"), main).unwrap();
    std::fs::write(dir.join("LOOSE.AUD"), &ima).unwrap();
    std::fs::write(dir.join("notes.txt"), b"not audio").unwrap();
}

#[test]
fn scans_nested_encrypted_archives_and_loose_files() {
    let (public, _) = test_keypair();
    let d = scratch("scan");
    fake_install(&d);
    let names = mapping(&[("ui.click", "CLICK.AUD")]).known_names();
    let c = scan(&d, &public, &names);
    assert!(c.problems.is_empty(), "{:?}", c.problems);
    let labels: Vec<&str> = c.archives.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "cd1/MAIN.MIX",
            "cd1/MAIN.MIX > SOUNDS.MIX",
            "(loose .aud files)"
        ]
    );
    assert!(c.archives[0].encrypted);
    let inner = &c.archives[1];
    assert_eq!(inner.entries.len(), 4);
    let click = inner
        .entries
        .iter()
        .find(|e| e.name.as_deref() == Some("CLICK.AUD"))
        .unwrap();
    assert!(matches!(
        click.kind,
        EntryKind::Aud {
            sample_rate: 22050,
            channels: 1,
            millis: 90,
            compression: Compression::Ima
        }
    ));
    let bad = inner
        .entries
        .iter()
        .find(|e| e.id == mix_id("BAD.AUD"))
        .unwrap();
    assert!(matches!(bad.kind, EntryKind::Broken(_)), "{:?}", bad.kind);
    assert_eq!(bad.label(), format!("#{:08X}", mix_id("BAD.AUD")));
    let pal = inner
        .entries
        .iter()
        .find(|e| e.id == mix_id("PALETTE.PAL"))
        .unwrap();
    assert_eq!(pal.kind, EntryKind::Other);
    assert_eq!(c.sound_count(), 3);
    // Playable from the browser.
    let pcm = decode_entry(click).unwrap();
    assert_eq!(pcm.frames(), 2000);
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn load_resolves_the_mapping_by_name_and_id() {
    let (public, _) = test_keypair();
    let d = scratch("load");
    fake_install(&d);
    let boom = format!("#{:08X}", mix_id("BOOM.AUD"));
    let m = mapping(&[
        ("ui.click", "click.aud"),
        ("explosion.small", &boom),
        ("unit.move", "LOOSE.AUD"),
        ("unit.attack", "MISSING.AUD"),
        ("unit.select", "BAD.AUD"),
    ]);
    let l = load(&d, &m, &public);
    let got: Vec<&str> = l.sounds.keys().map(String::as_str).collect();
    assert_eq!(got, ["explosion.small", "ui.click", "unit.move"]);
    let w = l.warning.unwrap();
    assert!(w.starts_with("2 of 5 mapped"), "{w}");
    assert!(w.contains("MISSING.AUD") && w.contains("BAD.AUD"), "{w}");
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn missing_path_and_empty_directory_fall_back_quietly() {
    let (public, _) = test_keypair();
    let m = mapping(&[("ui.click", "CLICK.AUD")]);
    let l = load(Path::new("/nonexistent/ra"), &m, &public);
    assert!(l.sounds.is_empty());
    assert!(l.warning.unwrap().contains("does not exist"));

    let d = scratch("empty");
    let l = load(&d, &m, &public);
    assert!(l.sounds.is_empty() && l.catalog.archives.is_empty());
    assert!(l.warning.unwrap().contains("no original Red Alert sounds"));
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn corrupt_files_are_reported_not_fatal() {
    let (public, _) = test_keypair();
    let d = scratch("corrupt");
    std::fs::write(d.join("EMPTY.MIX"), b"").unwrap();
    std::fs::write(d.join("JUNK.MIX"), [0xFFu8; 300]).unwrap();
    let real = write_mix(&[(1, b"abc")], MixFormat::Plain);
    std::fs::write(d.join("CUT.MIX"), &real[..real.len() - 2]).unwrap();
    std::fs::write(d.join("BAD.AUD"), b"nope").unwrap();
    // Encrypted with our test key but read with Westwood's: a clean error.
    let (_, private) = test_keypair();
    let enc = write_mix(
        &[(1, b"abc")],
        MixFormat::Encrypted {
            private: &private,
            blowfish: [1; BLOWFISH_KEY_LEN],
        },
    );
    std::fs::write(d.join("ENC.MIX"), enc).unwrap();
    let c = scan(&d, &RsaKey::westwood(), &[]);
    assert_eq!(c.problems.len(), 4, "{:?}", c.problems);
    assert_eq!(c.archives.len(), 1, "only the loose (bad) AUD is listed");
    assert_eq!(c.archives[0].entries[0].kind, EntryKind::Other);
    let l = load(&d, &mapping(&[("ui.click", "CLICK.AUD")]), &public);
    assert!(l.sounds.is_empty() && l.warning.is_some());
    std::fs::remove_dir_all(d).ok();
}

/// Fuzz-style: random bytes (a fixed xorshift seed, so reproducible), and
/// random corruptions of valid files, must never panic any reader.
#[test]
fn random_input_never_panics() {
    let (public, private) = test_keypair();
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let ima = encode(&beep(600), Compression::Ima);
    let ws = encode(&beep(600), Compression::Westwood);
    let mix_plain = write_mix(&[(1, &ima), (2, &ws)], MixFormat::Plain);
    let mix_enc = write_mix(
        &[(1, &ima)],
        MixFormat::Encrypted {
            private: &private,
            blowfish: [5; BLOWFISH_KEY_LEN],
        },
    );
    let seeds = [ima, ws, mix_plain, mix_enc];
    for round in 0..3000 {
        let mut data: Vec<u8> = if round % 2 == 0 {
            let len = (next() % 600) as usize;
            (0..len).map(|_| next() as u8).collect()
        } else {
            let mut d = seeds[round / 2 % seeds.len()].clone();
            for _ in 0..1 + next() % 8 {
                if d.is_empty() {
                    break;
                }
                let at = (next() as usize) % d.len();
                d[at] = next() as u8;
            }
            if next() % 3 == 0 {
                let keep = (next() as usize) % (d.len() + 1);
                d.truncate(keep);
            }
            d
        };
        // Some rounds look like a header to reach deeper code.
        if round % 7 == 0 && data.len() >= 12 {
            data[10] &= 3;
            data[11] = if round % 2 == 0 { 1 } else { 99 };
        }
        let _ = aud::decode(&data);
        let _ = aud::parse_header(&data);
        let mut out = Vec::new();
        let _ = aud::ws_decode_chunk(&data, (next() % 2000) as usize, &mut out);
        let len = data.len() as u64;
        let _ = read_index(&mut Cursor::new(&data), 0, len, &public);
        let _ = read_index(&mut Cursor::new(&data), 0, len, &RsaKey::westwood());
        let _ = public.blowfish_key(&data);
    }
}

#[test]
fn shipped_mapping_and_config_parse() {
    let m = Mapping::load(&assets()).unwrap();
    assert!(!m.sounds.is_empty());
    for event in m.sounds.keys() {
        assert!(event.contains('.'), "event names are dotted: {event}");
    }
    let (c, warning) = AudioConfig::load(&assets());
    assert!(warning.is_none());
    assert!(!c.ra_audio.enabled, "off by default");
    let (d, w) = AudioConfig::load(Path::new("/no/such"));
    assert_eq!(d, AudioConfig::default());
    assert!(w.is_none());
    assert!(Mapping::parse("(sounds: {").is_err());
}

/// The shipped names, as literals: these files exist on the discs (checked by
/// hand by the owner with `sf_ra_extract --dry-run`), a rename must be on
/// purpose.
#[test]
fn shipped_mapping_names_are_pinned() {
    let m = Mapping::load(&assets()).unwrap();
    let want = [
        ("ui.click", "RAMENU1.AUD"),
        ("unit.select", "AWAIT1.V01"),
        ("unit.move", "ACKNO.V01"),
        ("unit.attack", "AFFIRM1.V01"),
        ("building.placed", "PLACBLDG.AUD"),
        ("explosion.small", "KABOOM1.AUD"),
    ];
    assert_eq!(m.sounds.len(), want.len(), "{:?}", m.sounds);
    for (event, name) in want {
        assert_eq!(
            m.sounds.get(event).map(String::as_str),
            Some(name),
            "{event}"
        );
    }
}

/// A name with its own extension is looked up as written (`AWAIT1.V01`), a
/// bare WAV stem from `sf_ra_extract` still becomes `.AUD`.
#[test]
fn names_with_a_variant_extension_resolve_as_written() {
    assert_eq!(
        wav_name("AWAIT1.V01.wav"),
        (mix_id("AWAIT1.V01"), Some("AWAIT1.V01".into()))
    );
    assert_eq!(
        wav_name("ramenu1.wav"),
        (mix_id("RAMENU1.AUD"), Some("RAMENU1.AUD".into()))
    );
    assert_ne!(mix_id("AWAIT1.V01"), mix_id("AWAIT1.V01.AUD"));

    let (public, _) = test_keypair();
    let ima = encode(&beep(500), Compression::Ima);
    let d = scratch("variant");
    let mix = write_mix(
        &[
            (mix_id("AWAIT1.V01"), &ima),
            (mix_id("RAMENU1.AUD"), &ima),
            (mix_id("AWAIT1.V01.AUD"), &ima[..10]),
        ],
        MixFormat::Plain,
    );
    std::fs::write(d.join("SOUNDS.MIX"), mix).unwrap();
    let m = mapping(&[("unit.select", "AWAIT1.V01"), ("ui.click", "RAMENU1.AUD")]);
    let l = load(&d, &m, &public);
    assert_eq!(l.sounds.len(), 2, "{:?}", l.warning);
    assert!(l.warning.is_none(), "{:?}", l.warning);
    std::fs::remove_dir_all(d).ok();

    // The same through a WAV folder written by the extract tool.
    let d = scratch("variant_wav");
    std::fs::write(d.join("AWAIT1.V01.wav"), super::wav::write(&beep(500))).unwrap();
    let l = load(&d, &mapping(&[("unit.select", "AWAIT1.V01")]), &public);
    assert_eq!(l.sounds.len(), 1, "{:?}", l.warning);
    std::fs::remove_dir_all(d).ok();
}

/// An old-format archive whose `n` entries all cover the same bytes.
fn overlapping(n: u32, payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&(n as u16).to_le_bytes());
    v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    for id in 0..n {
        v.extend_from_slice(&(id + 1).to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    }
    v.extend_from_slice(payload);
    v
}

/// Archive bomb: four levels of 50 overlapping entries would be 50^4
/// visits; each region is classified and opened only once.
#[test]
fn overlapping_nested_entries_are_visited_once() {
    let (public, _) = test_keypair();
    let d = scratch("bomb");
    let mut data = encode(&beep(200), Compression::Ima);
    for _ in 0..4 {
        data = overlapping(50, &data);
    }
    std::fs::write(d.join("BOMB.MIX"), &data).unwrap();
    let c = scan(&d, &public, &[]);
    assert_eq!(c.archives.len(), 4, "{:?}", c.problems);
    let entries: usize = c.archives.iter().map(|a| a.entries.len()).sum();
    assert_eq!(entries, 200);
    assert!(entries <= MAX_ENTRIES && c.archives.len() <= MAX_ARCHIVES);
    assert_eq!(c.sound_count(), 50, "listed 50 times, decoded once");
    std::fs::remove_dir_all(d).ok();
}
