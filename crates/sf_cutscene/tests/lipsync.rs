//! Lip-sync (WP12.3) at the level of a scene: cue files loaded by the loader,
//! the mouth shape per character in the sampled state, rigs with mouth
//! variants, the validator, the placeholder figures of the Fountain
//! converter, and the `lipsync` tool.
//!
//! All fixtures are hand-written text made in the test; there are no
//! recordings.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use MouthShape::{A, B, C, D, E, F, G, H, X};
use sf_cutscene::fountain::{ImportOptions, convert};
use sf_cutscene::lipsync::flap::flap;
use sf_cutscene::{Art, MouthShape, NullSink, Player, Scene, SceneError, load_file, load_str};

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sf_lipsync_test_{}_{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

/// Ada's line (cue file `l1.json`).
const ADA_JSON: &str = r#"{
  "metadata": { "soundFile": "l1.wav", "duration": 2.00 },
  "mouthCues": [
    { "start": 0.00, "end": 0.20, "value": "X" },
    { "start": 0.20, "end": 0.50, "value": "B" },
    { "start": 0.50, "end": 0.90, "value": "D" },
    { "start": 0.90, "end": 1.20, "value": "A" },
    { "start": 1.20, "end": 2.00, "value": "X" }
  ]
}
"#;

/// Bo's line (cue file `l2.tsv`).
const BO_TSV: &str = "0.00\tX\n0.40\tC\n0.90\tE\n1.50\tF\n2.00\tX\n";

const SCENE: &str = r##"#![enable(implicit_some)]
Scene(
    name: "lip",
    characters: [
        (id: "ada", parts: [
            (name: "head", art: Circle(radius: 50.0, color: "#e8c39e")),
            (name: "mouth", parent: "head", art: Rect(w: 20.0, h: 6.0, color: "#7a2e2e"), mouth_shapes: {
                "A": Rect(w: 20.0, h: 3.0, color: "#111111"),
                "D": Rect(w: 20.0, h: 20.0, color: "#222222"),
                "X": Rect(w: 20.0, h: 2.0, color: "#333333"),
            }),
        ]),
        (id: "bo", parts: [
            (name: "head", art: Circle(radius: 50.0, color: "#c8946c")),
            (name: "mouth", parent: "head", art: Rect(w: 20.0, h: 6.0, color: "#7a2e2e")),
        ]),
    ],
    timeline: (tracks: [
        (name: "stage", events: [
            (at: 0, kind: Enter(character: "ada", pos: (-300.0, 0.0))),
            (at: 0, kind: Enter(character: "bo", pos: (300.0, 0.0))),
        ]),
        (name: "dialogue", events: [
            (at: 1000, kind: Dialogue(speaker: "ada", text: "Hello there.", duration_ms: 2000, voice: "voice/l1.wav", lipsync: "voice/l1.json")),
            (at: 4000, kind: Dialogue(speaker: "bo", text: "Good morning to you.", duration_ms: 2400, lipsync: "voice/l2.tsv")),
            (at: 7000, kind: Dialogue(speaker: "ada", text: "Hold the line until dawn.", duration_ms: 1800)),
            (at: 9000, kind: Dialogue(speaker: "bo", text: "Radio check.", duration_ms: 1000, delivery: VoiceOver, lipsync: "voice/l2.tsv")),
        ]),
    ]),
)
"##;

/// A scene with its cue files in a fresh asset root, loaded the way the game
/// loads one.
fn demo_scene(name: &str) -> (Scene, PathBuf) {
    let root = temp_dir(name);
    write(&root, "voice/l1.json", ADA_JSON);
    write(&root, "voice/l2.tsv", BO_TSV);
    let text = SCENE;
    let loaded = sf_cutscene::load_str(text, Some(&root)).unwrap_or_else(|e| panic!("{e}"));
    assert!(loaded.lipsync.is_empty(), "{:?}", loaded.lipsync);
    (loaded.scene, root)
}

fn mouth_of(s: &Scene, who: &str, t: i32) -> MouthShape {
    let state = s.sample(t);
    state
        .characters
        .iter()
        .find(|c| c.character.id == who)
        .unwrap_or_else(|| panic!("{who} is not on stage at {t}"))
        .mouth
}

#[test]
fn the_loader_reads_the_cue_files_the_scene_names() {
    let (scene, _) = demo_scene("loads");
    assert_eq!(scene.mouth_cues.len(), 2);
    assert_eq!(
        scene.mouth_cues.paths().collect::<Vec<_>>(),
        ["voice/l1.json", "voice/l2.tsv"]
    );
    let ada = scene.mouth_cues.get("voice/l1.json").unwrap();
    assert_eq!(ada.len(), 5);
    assert_eq!(ada.shape_at(500), D);
    // The cue sets are not part of the scene text: they do not come back out.
    let text = ron::ser::to_string(&scene).unwrap();
    assert!(!text.contains("mouth_cues"), "{text}");
}

#[test]
fn the_sampled_mouth_follows_the_cue_file_of_the_speaking_character() {
    let (scene, _) = demo_scene("sampled");
    // Ada's line starts at 1000 ms: elapsed = t - 1000. Cue starts: 0 X, 200 B,
    // 500 D, 900 A, 1200 X. Each cue is in force from its own start.
    let ada = |t| mouth_of(&scene, "ada", t);
    assert_eq!(ada(0), X, "before her line");
    assert_eq!(ada(999), X);
    assert_eq!(ada(1000), X, "first cue");
    assert_eq!(ada(1199), X);
    assert_eq!(ada(1200), B, "exactly at a cue start");
    assert_eq!(ada(1499), B);
    assert_eq!(ada(1500), D);
    assert_eq!(ada(1899), D);
    assert_eq!(ada(1900), A);
    assert_eq!(ada(2199), A);
    assert_eq!(ada(2200), X);
    assert_eq!(ada(2999), X, "last millisecond of the line");
    assert_eq!(ada(3000), X, "line over");
    // Bo does not move his mouth while Ada talks.
    for t in (1000..3000).step_by(50) {
        assert_eq!(mouth_of(&scene, "bo", t), X, "{t}");
    }
    // Bo's line from the TSV: 4000 + 400 C, + 900 E, + 1500 F, + 2000 X.
    let bo = |t| mouth_of(&scene, "bo", t);
    assert_eq!(bo(4000), X);
    assert_eq!(bo(4399), X);
    assert_eq!(bo(4400), C);
    assert_eq!(bo(4899), C);
    assert_eq!(bo(4900), E);
    assert_eq!(bo(5499), E);
    assert_eq!(bo(5500), F);
    assert_eq!(bo(5999), F);
    assert_eq!(bo(6000), X);
    assert_eq!(bo(6400), X);
    for t in (4000..6400).step_by(50) {
        assert_eq!(mouth_of(&scene, "ada", t), X, "{t}");
    }
}

#[test]
fn a_voice_over_is_not_drawn_talking() {
    let (scene, _) = demo_scene("voiceover");
    // The cue file of Bo's radio line says C from 400 ms on, but it is V.O.
    for t in (9000..10_000).step_by(25) {
        assert_eq!(mouth_of(&scene, "bo", t), X, "{t}");
    }
}

#[test]
fn a_line_without_a_cue_file_flaps_from_its_text() {
    let (scene, _) = demo_scene("flap");
    let reference = flap("Hold the line until dawn.", 1800);
    let mut seen = std::collections::BTreeSet::new();
    for t in 7000..8800 {
        let m = mouth_of(&scene, "ada", t);
        assert_eq!(m, reference.shape_at(t - 7000), "{t}");
        seen.insert(m);
    }
    assert!(seen.len() >= 4, "the mouth really moves: {seen:?}");
    assert!(seen.contains(&X), "and rests between the words");
    assert_eq!(mouth_of(&scene, "ada", 8800), X, "line over");

    // Without an asset root nothing is loaded and every line flaps, also the
    // ones that name a cue file.
    let bare = load_str(SCENE, None).unwrap();
    assert!(bare.scene.mouth_cues.is_empty());
    assert!(bare.lipsync.is_empty());
    let reference = flap("Hello there.", 2000);
    for t in 1000..3000 {
        assert_eq!(
            mouth_of(&bare.scene, "ada", t),
            reference.shape_at(t - 1000)
        );
    }
}

#[test]
fn sampling_in_any_order_and_across_seeks_gives_the_same_mouths() {
    use std::collections::BTreeMap;
    let (scene, _) = demo_scene("seek");
    let times: Vec<i32> = (0..10_500).step_by(13).collect();
    let n = times.len();
    let eval = |t: i32| {
        (
            mouth_of_either(&scene, "ada", t),
            mouth_of_either(&scene, "bo", t),
        )
    };
    let forward: BTreeMap<i32, _> = times.iter().map(|&t| (t, eval(t))).collect();
    let backward: BTreeMap<i32, _> = times.iter().rev().map(|&t| (t, eval(t))).collect();
    // A stride that visits every time once, in a jumbled order (37 and n share
    // no factor).
    let jumped: BTreeMap<i32, _> = (0..n)
        .map(|i| times[(i * 37) % n])
        .map(|t| (t, eval(t)))
        .collect();
    assert_eq!(forward.len(), n);
    assert_eq!(forward, backward);
    assert_eq!(forward, jumped);
    // The mouths are not all rest: the test would pass trivially otherwise.
    assert!(forward.values().any(|(a, _)| *a == D));
    assert!(forward.values().any(|(_, b)| *b == F));
    // sample_step (what a player calls each frame) agrees with sample.
    let mut prev = None;
    for &t in &times {
        let stepped = scene.sample_step(prev, t);
        assert_eq!(stepped.characters, scene.sample(t).characters, "{t}");
        prev = Some(t);
    }
}

fn mouth_of_either(s: &Scene, who: &str, t: i32) -> MouthShape {
    let state = s.sample(t);
    state
        .characters
        .iter()
        .find(|c| c.character.id == who)
        .map_or(X, |c| c.mouth)
}

#[test]
fn the_player_keeps_the_mouth_right_through_seeks_and_pauses() {
    let (scene, _) = demo_scene("player");
    let mut p = Player::new(scene);
    let mut sink = NullSink;
    let mouth = |p: &Player, who: &str| {
        p.sample()
            .characters
            .iter()
            .find(|c| c.character.id == who)
            .unwrap()
            .mouth
    };
    p.update(0, &mut sink);
    p.pause();
    // Seek into the middle of Ada's line while paused.
    p.seek(1650);
    p.update(100, &mut sink);
    assert_eq!(p.time_ms(), 1650);
    assert_eq!(mouth(&p, "ada"), D);
    // Time passes on the wall clock; the paused mouth does not change.
    for wall in [500, 900, 5000] {
        p.update(wall, &mut sink);
        assert_eq!(p.time_ms(), 1650);
        assert_eq!(mouth(&p, "ada"), D);
    }
    // Step back and forward by hand.
    p.seek(1250);
    assert_eq!(mouth(&p, "ada"), B);
    p.seek(1100);
    assert_eq!(mouth(&p, "ada"), X);
    p.seek(5600);
    assert_eq!(mouth(&p, "bo"), F);
    assert_eq!(mouth(&p, "ada"), X);
    // Playing on from a seek: the mouth follows the clock.
    p.seek(1450);
    p.play();
    p.update(5000, &mut sink); // first frame after a seek: restores the sound, no time passes
    assert_eq!(p.time_ms(), 1450);
    p.update(5050, &mut sink);
    assert_eq!(p.time_ms(), 1500);
    assert_eq!(mouth(&p, "ada"), D);
    p.update(5450, &mut sink);
    assert_eq!(p.time_ms(), 1900);
    assert_eq!(mouth(&p, "ada"), A);
}

#[test]
fn broken_or_missing_cue_files_never_stop_a_scene() {
    let root = temp_dir("broken");
    write(&root, "voice/bad.tsv", "0.00\tX\n0.30\tZ\n0.60\tD\n");
    // A file longer than its line, with an extra cue at the same time.
    write(
        &root,
        "voice/long.tsv",
        "0.00\tX\n0.30\tD\n0.30\tA\n1.00\tB\n3.00\tC\n",
    );
    let text = r##"#![enable(implicit_some)]
Scene(
    name: "b",
    characters: [(id: "ada", parts: [(name: "mouth", art: Rect(w: 2.0, h: 2.0, color: "#000000"))])],
    timeline: (tracks: [
        (name: "stage", events: [(at: 0, kind: Enter(character: "ada", pos: (0.0, 0.0)))]),
        (name: "dialogue", events: [
            (at: 0, kind: Dialogue(speaker: "ada", text: "One two three.", duration_ms: 1500, lipsync: "voice/bad.tsv")),
            (at: 2000, kind: Dialogue(speaker: "ada", text: "Four five six.", duration_ms: 1500, lipsync: "voice/long.tsv")),
            (at: 4000, kind: Dialogue(speaker: "ada", text: "Seven eight nine.", duration_ms: 1500, lipsync: "voice/missing.tsv")),
        ]),
    ]),
)
"##;
    let l = load_str(text, Some(&root)).unwrap_or_else(|e| panic!("{e}"));
    // bad.tsv: unusable, with the line of the unknown shape.
    let bad: Vec<_> = l
        .lipsync
        .iter()
        .filter(|i| i.file == "voice/bad.tsv")
        .collect();
    assert_eq!(bad.len(), 1, "{:?}", l.lipsync);
    assert!(bad[0].unusable);
    assert_eq!(bad[0].line, Some(2));
    assert!(bad[0].message.contains("unknown mouth shape \"Z\""));
    // long.tsv: used, with its corrections reported.
    let long: Vec<_> = l
        .lipsync
        .iter()
        .filter(|i| i.file == "voice/long.tsv")
        .collect();
    assert!(long.len() >= 2, "{long:?}");
    assert!(long.iter().all(|i| !i.unusable));
    assert!(
        long.iter()
            .any(|i| i.line == Some(3) && i.message.contains("two cues"))
    );
    assert!(
        long.iter()
            .any(|i| i.line == Some(5) && i.message.contains("ignored"))
    );
    // missing.tsv: not an issue of the cue reader; it is a missing asset.
    assert!(l.lipsync.iter().all(|i| i.file != "voice/missing.tsv"));
    assert!(
        l.missing_assets
            .iter()
            .any(|m| m.file == "voice/missing.tsv" && m.used_by.contains("lip-sync"))
    );
    // Which cue sets made it into the scene.
    assert_eq!(
        l.scene.mouth_cues.paths().collect::<Vec<_>>(),
        ["voice/long.tsv"]
    );
    // The unusable and the missing file flap; the long one is followed, with the
    // later of its two cues at 300 ms.
    let s = &l.scene;
    for (start, text, dur) in [
        (0, "One two three.", 1500),
        (4000, "Seven eight nine.", 1500),
    ] {
        let reference = flap(text, dur);
        for t in start..start + dur {
            assert_eq!(mouth_of(s, "ada", t), reference.shape_at(t - start), "{t}");
        }
    }
    assert_eq!(mouth_of(s, "ada", 2299), X);
    assert_eq!(mouth_of(s, "ada", 2300), A);
    assert_eq!(mouth_of(s, "ada", 2999), A);
    assert_eq!(mouth_of(s, "ada", 3000), B);
    assert_eq!(mouth_of(s, "ada", 3499), B);
    assert_eq!(mouth_of(s, "ada", 3500), X, "line over");
}

#[test]
fn an_unreadable_cue_file_is_reported_not_fatal() {
    let root = temp_dir("unreadable");
    // A folder where the file should be.
    fs::create_dir_all(root.join("voice/l1.json")).unwrap();
    let text = SCENE.replace("voice/l2.tsv", "voice/none.tsv");
    let l = load_str(&text, Some(&root)).unwrap();
    // A folder is not a file: treated like a file that is not there.
    assert!(l.lipsync.is_empty(), "{:?}", l.lipsync);
    assert!(l.scene.mouth_cues.is_empty());
}

#[test]
fn a_huge_cue_file_is_refused() {
    let root = temp_dir("huge");
    let mut big = String::from("0.00\tX\n");
    big.reserve(5 * 1024 * 1024);
    let mut t = 1;
    while big.len() < 4 * 1024 * 1024 + 100 {
        big.push_str(&format!("{}.{:03}\tA\n", t / 1000, t % 1000));
        t += 1;
    }
    write(&root, "voice/l1.json", &big);
    let l = load_str(SCENE, Some(&root)).unwrap();
    let issue = l
        .lipsync
        .iter()
        .find(|i| i.file == "voice/l1.json")
        .unwrap();
    assert!(issue.unusable);
    assert!(issue.message.contains("larger than"), "{issue:?}");
}

// -- rigs --------------------------------------------------------------------

fn part_with(shapes: &[MouthShape]) -> sf_cutscene::Part {
    let text = format!(
        r##"(name: "mouth", art: Rect(w: 1.0, h: 1.0, color: "#000000"), mouth_shapes: {{ {} }})"##,
        shapes
            .iter()
            .map(|s| format!(
                "\"{s}\": Rect(w: 1.0, h: {}.0, color: \"#ffffff\")",
                2 + *s as i32
            ))
            .collect::<Vec<_>>()
            .join(", ")
    );
    ron::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

fn height_of(art: &Art) -> f32 {
    match art {
        Art::Rect { h, .. } => *h,
        other => panic!("{other:?}"),
    }
}

/// Which variant a shape draws: `None` for the part's own art (height 1).
fn drawn(part: &sf_cutscene::Part, shape: MouthShape) -> Option<MouthShape> {
    let h = height_of(part.mouth_art(shape));
    MouthShape::ALL
        .into_iter()
        .find(|s| (2 + *s as i32) as f32 == h)
}

#[test]
fn a_missing_shape_falls_back_to_the_nearest_then_x_then_the_default_art() {
    // Every shape present: each draws itself.
    let full = part_with(&MouthShape::ALL);
    for s in MouthShape::ALL {
        assert_eq!(drawn(&full, s), Some(s));
    }
    // The fixed table, nearest first.
    let table: [(MouthShape, &[MouthShape]); 9] = [
        (A, &[X, B]),
        (B, &[C, A]),
        (C, &[B, D]),
        (D, &[C, E]),
        (E, &[F, C]),
        (F, &[E, C]),
        (G, &[B, C]),
        (H, &[C, D]),
        (X, &[A]),
    ];
    for (s, similar) in table {
        assert_eq!(s.similar(), similar, "{s}");
    }
    // D is missing: C is the nearest. Take C away too: E. Then X.
    assert_eq!(drawn(&part_with(&[A, B, C, E, F, G, H, X]), D), Some(C));
    assert_eq!(drawn(&part_with(&[A, B, E, F, G, H, X]), D), Some(E));
    assert_eq!(drawn(&part_with(&[A, B, F, G, H, X]), D), Some(X));
    // No similar one and no X: the part's own art.
    assert_eq!(drawn(&part_with(&[A, B, F, G, H]), D), None);
    assert_eq!(drawn(&part_with(&[A]), C), None, "A is not near C");
    // X missing: A is the nearest.
    assert_eq!(drawn(&part_with(&[A, D]), X), Some(A));
    // A part with no variants always draws its art.
    let plain: sf_cutscene::Part =
        ron::from_str(r##"(name: "mouth", art: Rect(w: 1.0, h: 1.0, color: "#000000"))"##).unwrap();
    for s in MouthShape::ALL {
        assert_eq!(drawn(&plain, s), None);
    }
    assert!(plain.mouth_shapes.is_empty());
}

#[test]
fn scenes_without_mouth_variants_still_load_and_write_the_same() {
    let old = r##"#![enable(implicit_some)]
Scene(name: "old", characters: [(id: "a", parts: [
    (name: "mouth", art: Rect(w: 1.0, h: 1.0, color: "#000000")),
])])"##;
    let scene = load_str(old, None).unwrap().scene;
    let part = &scene.characters[0].parts[0];
    assert!(part.mouth_shapes.is_empty());
    let written = ron::ser::to_string(&scene).unwrap();
    assert!(!written.contains("mouth_shapes"), "{written}");
    // With variants it round-trips.
    let (scene, _) = demo_scene("roundtrip");
    let written = ron::ser::to_string(&scene).unwrap();
    assert!(written.contains("mouth_shapes"));
    let again = sf_cutscene::parse_str(&written).unwrap();
    assert_eq!(again.characters, scene.characters);
}

#[test]
fn an_unknown_shape_key_is_a_scene_error_with_its_line() {
    for key in ["Q", "a", "AB", ""] {
        let text = SCENE.replace("\"D\": Rect", &format!("\"{key}\": Rect"));
        let err = load_str(&text, None).unwrap_err();
        let SceneError::Parse { line, message, .. } = &err else {
            panic!("{key}: {err:?}");
        };
        assert_eq!(*line, 9, "{key}: {err}");
        assert!(message.contains("mouth shape"), "{key}: {message}");
    }
}

#[test]
fn mouth_variants_are_validated_and_their_pictures_listed() {
    // A bad size is reported on the line of that shape.
    let text = SCENE.replace(
        "\"D\": Rect(w: 20.0, h: 20.0",
        "\"D\": Rect(w: 0.0, h: 20.0",
    );
    let err = load_str(&text, None).unwrap_err();
    let SceneError::Invalid { issues, .. } = &err else {
        panic!("{err:?}");
    };
    assert_eq!(issues.len(), 1, "{err}");
    assert_eq!(issues[0].line, Some(9), "{err}");
    assert!(issues[0].message.contains("mouth shape D"), "{err}");

    // A picture that is not below the asset root is an error; one that does
    // not exist yet is a missing asset, with the line and what it is for.
    let bad = SCENE.replace(
        "\"A\": Rect(w: 20.0, h: 3.0, color: \"#111111\")",
        "\"A\": Image(\"../x.png\")",
    );
    assert!(load_str(&bad, None).is_err());
    let root = temp_dir("pictures");
    let pics = SCENE.replace(
        "\"A\": Rect(w: 20.0, h: 3.0, color: \"#111111\")",
        "\"A\": Image(\"art/ada/mouth_a.png\")",
    );
    let l = load_str(&pics, Some(&root)).unwrap();
    let m = l
        .missing_assets
        .iter()
        .find(|m| m.file == "art/ada/mouth_a.png")
        .expect("listed");
    assert_eq!(m.line, Some(8));
    assert!(m.used_by.contains("mouth shape A"), "{}", m.used_by);
    assert_eq!(
        sf_cutscene::preview::picture_paths(&l.scene),
        ["art/ada/mouth_a.png"]
    );
}

#[test]
fn the_preview_watches_the_cue_files() {
    let (scene, _) = demo_scene("watch");
    assert_eq!(
        sf_cutscene::preview::lipsync_paths(&scene),
        ["voice/l1.json", "voice/l2.tsv"]
    );
}

// -- the demo and the converter ----------------------------------------------

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

#[test]
fn the_demo_scene_rigs_have_a_variant_for_every_shape_and_talk_without_voices() {
    let loaded = load_file(
        &assets().join("scenes/f1_m01_briefing.ron"),
        Some(&assets()),
    )
    .unwrap();
    assert!(loaded.lipsync.is_empty());
    let scene = loaded.scene;
    for id in ["captain_elian", "mira"] {
        let c = scene.character(id).unwrap();
        let mouth = &c.parts[c.part_index("mouth").unwrap()];
        for s in MouthShape::ALL {
            assert!(mouth.mouth_shapes.contains_key(&s), "{id} has no {s}");
        }
        // They really differ from one another.
        for a in MouthShape::ALL {
            for b in MouthShape::ALL {
                if a != b {
                    assert_ne!(
                        mouth.mouth_shapes[&a], mouth.mouth_shapes[&b],
                        "{id}: {a} {b}"
                    );
                }
            }
        }
    }
    // Mid-line the mouths move (flap), and only the speaker's.
    let elian: std::collections::BTreeSet<MouthShape> = (2200..9800)
        .step_by(20)
        .map(|t| mouth_of(&scene, "captain_elian", t))
        .collect();
    assert!(elian.len() >= 4, "{elian:?}");
    for t in (2200..9800).step_by(100) {
        assert_eq!(mouth_of(&scene, "mira", t), X, "{t}");
    }
}

#[test]
fn the_converters_placeholder_figures_talk() {
    let fountain = "Title: T\n\nINT. ROOM - DAY\n\nMAJOR\nHold the line until dawn.\n";
    let import = convert(
        fountain,
        &ImportOptions {
            name: "t".into(),
            source: None,
        },
    );
    let scene = import.scene;
    let major = &scene.characters[0];
    let mouth = &major.parts[major.part_index("mouth").unwrap()];
    assert_eq!(mouth.mouth_shapes.len(), 9);
    // The text written out and read back keeps the variants.
    let back = load_str(&import.ron, None).unwrap().scene;
    assert_eq!(back.characters[0], *major);
    // While he speaks the mouth shape changes.
    let line = scene
        .events_in_order()
        .into_iter()
        .find(|e| matches!(e.kind, sf_cutscene::EventKind::Dialogue { .. }))
        .unwrap();
    let start = line.at;
    let seen: std::collections::BTreeSet<MouthShape> = (start..start + 2000)
        .step_by(20)
        .map(|t| mouth_of(&scene, &major.id, t))
        .collect();
    assert!(seen.len() >= 3, "{seen:?}");
}

// -- the lipsync tool ----------------------------------------------------------

fn run_tool(args: &[&str], path: &Path) -> (Option<i32>, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_lipsync"))
        .args(args)
        .env("PATH", path)
        .output()
        .expect("run lipsync");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn without_rhubarb_the_tool_prints_how_to_install_it_and_fails() {
    let empty = temp_dir("no_rhubarb_path");
    let wav = empty.join("voice.wav");
    fs::write(&wav, "RIFF").unwrap();
    let out = empty.join("voice.json");
    let args = [wav.to_str().unwrap(), "-o", out.to_str().unwrap()];
    let (code, stdout, stderr) = run_tool(&args, &empty);
    assert_eq!(code, Some(3), "{stdout}{stderr}");
    assert!(
        stderr.contains("Rhubarb Lip Sync was not found"),
        "{stderr}"
    );
    assert!(stderr.contains("MIT"), "{stderr}");
    assert!(
        stderr.contains("https://github.com/DanielSWolf/rhubarb-lip-sync/releases"),
        "{stderr}"
    );
    assert!(stderr.contains("--rhubarb"), "{stderr}");
    assert!(!out.exists(), "nothing is written");
    // The batch form says the same.
    let (code, _, stderr) = run_tool(&["--batch", empty.to_str().unwrap()], &empty);
    assert_eq!(code, Some(3), "{stderr}");
    assert!(stderr.contains("releases"), "{stderr}");
    // A --rhubarb path that is not there is named.
    let ghost = empty.join("ghost");
    let (code, _, stderr) = run_tool(
        &["--rhubarb", ghost.to_str().unwrap(), wav.to_str().unwrap()],
        &empty,
    );
    assert_eq!(code, Some(3));
    assert!(stderr.contains("ghost: no such file"), "{stderr}");
}

#[test]
fn the_tool_reports_bad_usage_before_anything_else() {
    let empty = temp_dir("usage_path");
    for args in [
        vec![],
        vec!["--bogus"],
        vec!["a.wav", "b.wav"],
        vec!["a.wav", "--batch", "dir"],
        vec!["a.wav", "--dialog"],
        vec!["a.wav", "--dialog", "x", "--dialog-file", "y"],
        vec!["a.wav", "--format", "json"],
        vec!["--batch", "dir", "--format", "xml"],
    ] {
        let (code, _, stderr) = run_tool(&args, &empty);
        assert_eq!(code, Some(2), "{args:?}: {stderr}");
        assert!(stderr.contains("usage:"), "{args:?}: {stderr}");
    }
    let (code, stdout, _) = run_tool(&["--help"], &empty);
    assert_eq!(code, Some(0));
    assert!(stdout.contains("--batch"));
}

/// A stand-in for `rhubarb` (a shell script), to test how the tool calls it:
/// it logs its arguments and writes a fixed cue file where `-o` says.
#[cfg(unix)]
#[test]
fn the_tool_runs_rhubarb_for_one_file_and_for_a_folder() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir("fake_rhubarb");
    let exe = dir.join("rhubarb");
    let log = dir.join("calls.log");
    fs::write(
        &exe,
        format!(
            "#!/bin/sh\n\
PATH=/usr/bin:/bin\n\
echo \"$@\" >> '{log}'\n\
d=''\n\
while [ $# -gt 0 ]; do\n\
  case \"$1\" in -o) out=\"$2\"; shift;; -d) d=\"$2\"; shift;; esac\n\
  shift\n\
done\n\
if [ -n \"$d\" ]; then cat \"$d\" >> '{log}'; echo >> '{log}'; fi\n\
case \"$out\" in *.tsv) printf '0.00\\tX\\n0.50\\tD\\n1.00\\tX\\n' > \"$out\";;\n\
*) printf '{{ \"mouthCues\": [ {{ \"start\": 0.0, \"end\": 0.5, \"value\": \"X\" }}, {{ \"start\": 0.5, \"end\": 1.0, \"value\": \"D\" }} ] }}' > \"$out\";; esac\n",
            log = log.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
    let empty = temp_dir("fake_rhubarb_path");

    // One file, text on the command line, found by --rhubarb.
    let wav = dir.join("l017.wav");
    fs::write(&wav, "RIFF").unwrap();
    let out = dir.join("out").join("l017.json");
    let (code, stdout, stderr) = run_tool(
        &[
            wav.to_str().unwrap(),
            "--dialog",
            "Hold the line.",
            "-o",
            out.to_str().unwrap(),
            "--rhubarb",
            exe.to_str().unwrap(),
        ],
        &empty,
    );
    assert_eq!(code, Some(0), "{stdout}{stderr}");
    assert!(stdout.contains("3 cues"), "{stdout}"); // X, D and the closing rest
    let cues = sf_cutscene::lipsync::parse_cues(&fs::read_to_string(&out).unwrap(), 5000).unwrap();
    assert_eq!(cues.cues.shape_at(600), D);
    let calls = fs::read_to_string(&log).unwrap();
    assert!(calls.contains("-f json"), "{calls}");
    assert!(calls.contains("-d "), "{calls}");
    assert!(
        calls.contains("Hold the line."),
        "the text reached rhubarb:\n{calls}"
    );
    assert!(calls.contains("l017.wav"), "{calls}");
    // The temporary text file is gone.
    let leftovers = fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.starts_with("staalfront-lipsync-") && name.ends_with("-l017.txt")
        })
        .count();
    assert_eq!(leftovers, 0);

    // Found on PATH, TSV by the file name, with a recogniser.
    let tsv = dir.join("one.tsv");
    let path_with_rhubarb = std::env::join_paths([dir.clone()]).unwrap();
    let (code, stdout, stderr) = run_tool(
        &[
            wav.to_str().unwrap(),
            "-o",
            tsv.to_str().unwrap(),
            "--recognizer",
            "phonetic",
        ],
        Path::new(&path_with_rhubarb),
    );
    assert_eq!(code, Some(0), "{stdout}{stderr}");
    assert!(fs::read_to_string(&tsv).unwrap().starts_with("0.00\tX"));
    let calls = fs::read_to_string(&log).unwrap();
    assert!(
        calls.contains("-f tsv") && calls.contains("-r phonetic"),
        "{calls}"
    );

    // A folder: l003 and l011 (with a text), the others are ignored.
    let voices = dir.join("voices");
    let texts = dir.join("texts");
    let cues_out = dir.join("cues");
    fs::create_dir_all(&voices).unwrap();
    fs::create_dir_all(&texts).unwrap();
    for n in ["l011.wav", "l003.wav", "readme.txt"] {
        fs::write(voices.join(n), "RIFF").unwrap();
    }
    fs::write(texts.join("l011.txt"), "Eleven.").unwrap();
    fs::remove_file(&log).unwrap();
    let (code, stdout, stderr) = run_tool(
        &[
            "--batch",
            voices.to_str().unwrap(),
            "-o",
            cues_out.to_str().unwrap(),
            "--dialog-dir",
            texts.to_str().unwrap(),
            "--rhubarb",
            exe.to_str().unwrap(),
        ],
        &empty,
    );
    assert_eq!(code, Some(0), "{stdout}{stderr}");
    assert!(stdout.contains("2 of 2 lines done"), "{stdout}");
    assert!(cues_out.join("l003.json").is_file() && cues_out.join("l011.json").is_file());
    let calls = fs::read_to_string(&log).unwrap();
    assert_eq!(
        calls.matches("-d ").count(),
        1,
        "only l011 has a text:\n{calls}"
    );
    assert!(calls.contains("Eleven."), "{calls}");
    let first = calls.find("l003.wav").unwrap();
    assert!(
        first < calls.find("l011.wav").unwrap(),
        "in name order:\n{calls}"
    );

    // A failing rhubarb is reported per line and fails the batch.
    fs::write(
        &exe,
        "#!/bin/sh\necho 'cannot read the sound file' >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
    let (code, _, stderr) = run_tool(
        &[
            "--batch",
            voices.to_str().unwrap(),
            "--rhubarb",
            exe.to_str().unwrap(),
        ],
        &empty,
    );
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.contains("cannot read the sound file"), "{stderr}");
    assert!(stderr.contains("l003"), "{stderr}");

    // A rhubarb that writes rubbish is caught, with the line.
    fs::write(
        &exe,
        "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in -o) out=\"$2\"; shift;; esac; shift; done\nprintf '0.00\\tX\\n0.20\\tQ\\n' > \"$out\"\n",
    )
    .unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
    let (code, _, stderr) = run_tool(
        &[
            wav.to_str().unwrap(),
            "-o",
            tsv.to_str().unwrap(),
            "--rhubarb",
            exe.to_str().unwrap(),
        ],
        &empty,
    );
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.contains(":2: unknown mouth shape"), "{stderr}");
}

#[test]
fn scene_check_names_a_cue_file_it_cannot_use() {
    let root = temp_dir("scene_check");
    write(&root, "voice/l1.json", ADA_JSON);
    write(&root, "voice/l2.tsv", "0.00\tX\n0.40\tZ\n");
    write(&root, "scene.ron", SCENE);
    let run = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_scene_check"))
            .arg(root.join("scene.ron"))
            .arg("--assets")
            .arg(&root)
            .args(extra)
            .output()
            .expect("run scene_check")
    };
    let out = run(&[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("l2.tsv:2: error: unknown mouth shape"),
        "{stderr}"
    );
    // With the file put right the scene checks out and says it used both.
    write(&root, "voice/l2.tsv", BO_TSV);
    let out = run(&[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(stdout.contains("2 lip-sync files used"), "{stdout}");
}
