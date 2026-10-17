//! Fixtures are built here (own ISO writer, sf_audio's MIX writer and AUD
//! encoders); never real Red Alert files (B11).

use super::*;
use sf_audio::ra::aud::{Compression, Pcm, encode};
use sf_audio::ra::crypto::test_keypair;
use sf_audio::ra::mix::{MixFormat, write_mix};

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sf_ra_extract_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn pcm(rate: u32, channels: u16, n: i32) -> Pcm {
    Pcm {
        sample_rate: rate,
        channels,
        samples: (0..n).map(|i| ((i * 97) % 4000 - 2000) as i16).collect(),
    }
}

struct Fixture {
    iso: Vec<u8>,
    click: Vec<u8>,
    music: Vec<u8>,
    inner: Vec<u8>,
}

/// An ISO with INSTALL/MAIN.MIX (encrypted header) holding CLICK.AUD (IMA),
/// an unnamed AUD (Westwood ADPCM, stereo IMA would also do), a broken AUD,
/// a fake VQA, junk and a nested SOUNDS.MIX with one more AUD; plus a
/// loose MOVIE.VQA.
fn fixture() -> (Fixture, RsaKey) {
    let (public, private) = test_keypair();
    let click = encode(&pcm(22050, 1, 3000), Compression::Ima);
    let music = encode(&pcm(22050, 1, 5000), Compression::Westwood);
    let inner = encode(&pcm(11025, 2, 2000), Compression::Ima);
    let sounds = write_mix(&[(mix_id("INNER.AUD"), &inner)], MixFormat::Plain);
    let mut vqa = b"FORM\0\0\x10\0WVQAVQHD".to_vec();
    vqa.resize(4096, 0x55);
    let main = write_mix(
        &[
            (mix_id("CLICK.AUD"), &click),
            (0x1234_5678, &music),
            (mix_id("BROKEN.AUD"), &click[..40]),
            (mix_id("INTRO.VQA"), &vqa),
            (mix_id("JUNK.SHP"), b"not audio at all, just bytes"),
            (mix_id("SOUNDS.MIX"), &sounds),
        ],
        MixFormat::Encrypted {
            private: &private,
            blowfish: [9u8; 56],
        },
    );
    let iso = iso::write_iso(&[
        ("INSTALL/MAIN.MIX", &main),
        ("MOVIES/MOVIE.VQA", &vqa),
        ("README.TXT", b"hi"),
    ]);
    (
        Fixture {
            iso,
            click,
            music,
            inner,
        },
        public,
    )
}

fn args(input: &Path, out: &Path) -> Args {
    Args {
        input: input.to_path_buf(),
        out: out.to_path_buf(),
        names: None,
        dry_run: false,
    }
}

fn wav_pcm(path: &Path) -> Pcm {
    wav::read(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn iso_to_wav_round_trip_with_names() {
    let d = scratch("round_trip");
    let (fx, key) = fixture();
    let iso_path = d.join("redalert.iso");
    std::fs::write(&iso_path, &fx.iso).unwrap();
    let names = d.join("names.txt");
    std::fs::write(&names, "click.aud\n\nSOUNDS.MIX\r\nINNER.AUD\n").unwrap();
    let out = d.join("RedAlertSounds");
    let mut a = args(&iso_path, &out);
    a.names = Some(names);
    let mut log = Vec::new();
    let s = run(&a, &key, &mut log).unwrap();

    assert_eq!(s.archives, 2, "{s:?}");
    let mut sorted = s.sounds.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        [
            "MAIN.MIX/#12345678.wav",
            "MAIN.MIX/CLICK.wav",
            "MAIN.MIX/SOUNDS.MIX/INNER.wav"
        ]
    );
    assert_eq!(s.videos, 2, "one in the MIX, one loose on the disc");
    assert_eq!(s.other, 1);
    assert_eq!(s.failed.len(), 1);
    let broken = format!(
        "MAIN.MIX > #{:08X}: AUD data is truncated",
        mix_id("BROKEN.AUD")
    );
    assert_eq!(s.failed, [broken]);
    assert!(s.problems.is_empty(), "{:?}", s.problems);

    // The PCM in each WAV is exactly what the AUD decodes to.
    for (file, aud_bytes) in [
        ("MAIN.MIX/CLICK.wav", &fx.click),
        ("MAIN.MIX/#12345678.wav", &fx.music),
        ("MAIN.MIX/SOUNDS.MIX/INNER.wav", &fx.inner),
    ] {
        let got = wav_pcm(&out.join(file));
        assert_eq!(got, aud::decode(aud_bytes).unwrap(), "{file}");
    }
    let inner = wav_pcm(&out.join("MAIN.MIX/SOUNDS.MIX/INNER.wav"));
    assert_eq!((inner.sample_rate, inner.channels), (11025, 2));
    let total: u64 = s
        .sounds
        .iter()
        .map(|f| std::fs::metadata(out.join(f)).unwrap().len())
        .sum();
    assert_eq!(s.bytes, total);
    // Nothing else was written (no video, no junk).
    let mut all = Vec::new();
    walk(&out, 0, &mut all);
    assert_eq!(all.len(), 3);

    // The game reads the folder back: names and ids resolve.
    let cat = sf_audio::ra::scan(&out, &key, &[]);
    for n in ["CLICK.AUD", "#12345678", "INNER.AUD"] {
        let e = cat.find(n).unwrap_or_else(|| panic!("{n} in {cat:?}"));
        assert!(sf_audio::ra::decode_entry(e).is_ok());
    }

    let mut text = Vec::new();
    s.print(&mut text, false).unwrap();
    let text = String::from_utf8(text).unwrap();
    assert!(text.contains("sounds written: 3"), "{text}");
    assert!(text.contains("videos skipped:  2"), "{text}");
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn without_names_files_are_ids_and_dry_run_writes_nothing() {
    let d = scratch("ids");
    let (fx, key) = fixture();
    // A directory input with the MIX extracted from the image.
    let listing = iso::list_files(&mut std::io::Cursor::new(&fx.iso)).unwrap();
    let main = listing
        .files
        .iter()
        .find(|f| f.path.ends_with("MAIN.MIX"))
        .unwrap();
    let disc = d.join("disc");
    std::fs::create_dir_all(disc.join("INSTALL")).unwrap();
    std::fs::write(
        disc.join("INSTALL/main.mix"),
        &fx.iso[main.offset as usize..][..main.size as usize],
    )
    .unwrap();
    let out = d.join("out");
    let mut a = args(&disc, &out);
    a.dry_run = true;
    let mut log = Vec::new();
    let s = run(&a, &key, &mut log).unwrap();
    let click = format!("main.mix/#{:08X}.wav", mix_id("CLICK.AUD"));
    assert!(s.sounds.contains(&click), "{:?}", s.sounds);
    assert_eq!(s.sounds.len(), 3);
    assert!(s.bytes > 0);
    assert!(String::from_utf8(log).unwrap().contains(&click));
    assert!(!out.exists(), "dry run wrote nothing");

    // For real, from the single .mix file.
    a.input = disc.join("INSTALL/main.mix");
    a.dry_run = false;
    let s = run(&a, &key, &mut Vec::new()).unwrap();
    assert_eq!(s.sounds.len(), 3);
    assert!(out.join(&click).is_file());
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn refuses_to_write_inside_a_git_repository() {
    let d = scratch("git");
    let (fx, key) = fixture();
    let iso_path = d.join("ra.iso");
    std::fs::write(&iso_path, &fx.iso).unwrap();
    let repo = d.join("checkout");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    for out in [
        repo.join("assets/sounds"),
        repo.clone(),
        repo.join("x/../y"),
    ] {
        let err = run(&args(&iso_path, &out), &key, &mut Vec::new()).unwrap_err();
        assert!(err.contains("git repository"), "{err}");
        assert!(!repo.join("assets").exists());
    }
    // A `.git` file (worktrees, submodules) counts too.
    let wt = d.join("worktree");
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(wt.join(".git"), "gitdir: elsewhere").unwrap();
    assert!(git_repo_above(&wt.join("out")).is_some());
    // Next to the repository is fine.
    assert!(git_repo_above(&d.join("sounds")).is_none());
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn corrupt_iso_and_mix_inputs_are_reported_not_fatal() {
    let d = scratch("corrupt");
    let (fx, key) = fixture();
    let out = d.join("out");
    let iso_path = d.join("bad.iso");
    // Garbage, truncated and bit-flipped images.
    let mut inputs: Vec<Vec<u8>> = vec![
        Vec::new(),
        vec![0xAB; 50_000],
        fx.iso[..fx.iso.len() / 2].to_vec(),
    ];
    for at in (0..fx.iso.len()).step_by(fx.iso.len() / 40) {
        let mut m = fx.iso.clone();
        m[at] ^= 0xFF;
        inputs.push(m);
    }
    for (i, img) in inputs.iter().enumerate() {
        std::fs::write(&iso_path, img).unwrap();
        let s = run(&args(&iso_path, &out), &key, &mut Vec::new()).unwrap();
        if i < 2 {
            assert!(s.sounds.is_empty() && !s.problems.is_empty(), "{s:?}");
        }
    }
    // A broken .mix: one problem line, nothing written.
    let mix_path = d.join("BAD.MIX");
    std::fs::write(&mix_path, b"\0\0\x02\0garbage").unwrap();
    let out2 = d.join("out2");
    let s = run(&args(&mix_path, &out2), &key, &mut Vec::new()).unwrap();
    assert_eq!(
        (s.archives, s.sounds.len(), s.problems.len()),
        (0, 0, 1),
        "{s:?}"
    );
    assert!(!out2.exists());
    // Missing input, unknown file type, bad arguments.
    let s = run(&args(&d.join("nope.iso"), &out2), &key, &mut Vec::new()).unwrap();
    assert!(!s.problems.is_empty());
    let zip = d.join("ra.zip");
    std::fs::write(&zip, b"PK").unwrap();
    let s = run(&args(&zip, &out2), &key, &mut Vec::new()).unwrap();
    assert!(s.problems[0].contains("unzip first"), "{s:?}");
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn argument_parsing() {
    let p = |v: &[&str]| parse_args(v.iter().map(|s| s.to_string()));
    let a = p(&["ra.iso", "--out", "o", "--names", "n.txt", "--dry-run"]).unwrap();
    assert_eq!(a.input, PathBuf::from("ra.iso"));
    assert_eq!(a.names, Some(PathBuf::from("n.txt")));
    assert!(a.dry_run);
    assert!(p(&["ra.iso"]).is_err());
    assert!(p(&["--out", "o"]).is_err());
    assert!(p(&["a", "b", "--out", "o"]).is_err());
    assert!(p(&["a", "--out"]).is_err());
    assert!(p(&["a", "--out", "o", "--zip"]).is_err());
    assert_eq!(p(&["--help"]), Err(String::new()));
    assert_eq!(safe("../x/y"), ".._x_y");
    assert_eq!(safe(".."), "_");
    assert_eq!(wav_file_name(1, Some("foo.aud")), "FOO.wav");
    assert_eq!(wav_file_name(1, Some("A.JUV")), "A.JUV.wav");
    assert_eq!(wav_file_name(0xAB, None), "#000000AB.wav");
}
