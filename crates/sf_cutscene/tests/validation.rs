//! What the loader rejects, and with which line number.

use std::fs;
use std::path::PathBuf;

use sf_cutscene::{SceneError, load_str};

const BASE: &str = r##"#![enable(implicit_some)]
Scene(
    name: "v",
    layers: [
        (name: "sky", art: Rect(w: 10, h: 10, color: "#000000"), z: -1, shots: ["main"]),
    ],
    characters: [
        (id: "a", parts: [
            (name: "body", art: Rect(w: 10, h: 20, color: "#ff0000")),
            (name: "arm", parent: "body", art: Rect(w: 4, h: 10, color: "#00ff00")),
        ], poses: [
            (name: "up", parts: [(part: "arm", angle: 45)]),
        ]),
        (id: "b"),
    ],
    timeline: (tracks: [
        (name: "stage", events: [
            (at: 0, kind: Enter(character: "a", pos: (0, 0))),
            (at: 100, kind: SetPose(character: "a", pose: "up", duration_ms: 50)),
        ]),
        (name: "camera", events: [
            (at: 0, kind: CameraCut(shot: "main")),
        ]),
        (name: "dialogue", events: [
            (at: 200, kind: Dialogue(speaker: "a", text: "Hi", duration_ms: 500)),
            (at: 700, kind: Dialogue(speaker: "b", text: "Yo", duration_ms: 500, delivery: VoiceOver)),
        ]),
    ]),
)
"##;

/// 1-based line of the first line containing `needle`.
fn line_of(text: &str, needle: &str) -> usize {
    text.lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} not in text"))
        + 1
}

/// Load `text` expecting validation errors; returns (line, message) pairs.
fn errors(text: &str) -> Vec<(Option<usize>, String)> {
    match load_str(text, None) {
        Err(SceneError::Invalid { issues, .. }) => {
            issues.into_iter().map(|i| (i.line, i.message)).collect()
        }
        Err(other) => panic!("expected validation errors, got: {other}"),
        Ok(_) => panic!("expected validation errors, but the scene loaded"),
    }
}

/// Assert there is an error on `line` whose message contains `frag`.
fn assert_error(text: &str, line: usize, frag: &str) {
    let errs = errors(text);
    assert!(
        errs.iter()
            .any(|(l, m)| *l == Some(line) && m.contains(frag)),
        "wanted an error containing {frag:?} on line {line}, got: {errs:#?}"
    );
}

fn with(from: &str, to: &str) -> String {
    assert!(BASE.contains(from), "{from:?} not in BASE");
    BASE.replacen(from, to, 1)
}

#[test]
fn the_base_scene_is_valid() {
    let l = load_str(BASE, None).unwrap_or_else(|e| panic!("{e}"));
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    assert_eq!(l.scene.duration_ms(), 1200);
}

#[test]
fn unknown_character_reference() {
    let t = with("Enter(character: \"a\"", "Enter(character: \"zed\"");
    assert_error(&t, line_of(&t, "Enter("), "unknown character \"zed\"");
    let t = with("speaker: \"b\"", "speaker: \"nobody\"");
    assert_error(&t, line_of(&t, "nobody"), "unknown character \"nobody\"");
}

#[test]
fn unknown_part_in_a_pose() {
    let t = with("(part: \"arm\", angle: 45)", "(part: \"tail\", angle: 45)");
    assert_error(&t, line_of(&t, "tail"), "unknown part \"tail\"");
}

#[test]
fn unknown_pose_in_an_event() {
    let t = with("pose: \"up\"", "pose: \"down\"");
    assert_error(&t, line_of(&t, "down"), "has no pose \"down\"");
    let t = with("pos: (0, 0))", "pos: (0, 0), pose: \"down\")");
    assert_error(&t, line_of(&t, "down"), "has no pose \"down\"");
}

#[test]
fn unknown_parent_and_parent_loops() {
    let t = with("parent: \"body\"", "parent: \"skull\"");
    assert_error(&t, line_of(&t, "skull"), "unknown parent part \"skull\"");
    let t = with(
        "(name: \"body\", art",
        "(name: \"body\", parent: \"arm\", art",
    );
    let errs = errors(&t);
    assert!(
        errs.iter().any(|(_, m)| m.contains("form a loop")),
        "{errs:#?}"
    );
    let t = with(
        "(name: \"body\", art",
        "(name: \"body\", parent: \"body\", art",
    );
    assert_error(&t, line_of(&t, "parent: \"body\", art"), "its own parent");
}

#[test]
fn one_speaker_cannot_overlap_themselves() {
    let t = with(
        "            (at: 700, kind: Dialogue(speaker: \"b\"",
        "            (at: 400, kind: Dialogue(speaker: \"a\", text: \"Again\", duration_ms: 100)),\n            (at: 700, kind: Dialogue(speaker: \"b\"",
    );
    assert_error(&t, line_of(&t, "Again"), "\"a\" is still talking");
    // Back to back is fine, and different speakers may talk over each other.
    let t = with(
        "            (at: 700, kind: Dialogue(speaker: \"b\"",
        "            (at: 700, kind: Dialogue(speaker: \"a\", text: \"Next\", duration_ms: 100)),\n            (at: 700, kind: Dialogue(speaker: \"b\"",
    );
    load_str(&t, None).unwrap_or_else(|e| panic!("{e}"));
    let t = with(
        "(at: 700, kind: Dialogue(speaker: \"b\"",
        "(at: 300, kind: Dialogue(speaker: \"b\"",
    );
    load_str(&t, None).unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn negative_times_and_durations() {
    let t = with("(at: 0, kind: Enter(", "(at: -5, kind: Enter(");
    assert_error(&t, line_of(&t, "at: -5"), "time is negative");
    let t = with("duration_ms: 50", "duration_ms: -50");
    assert_error(
        &t,
        line_of(&t, "duration_ms: -50"),
        "duration -50 ms is negative",
    );
}

#[test]
fn zero_length_dialogue_and_empty_text() {
    let t = with(
        "text: \"Hi\", duration_ms: 500",
        "text: \"Hi\", duration_ms: 0",
    );
    assert_error(&t, line_of(&t, "\"Hi\""), "more than 0 ms");
    let t = with("text: \"Hi\"", "text: \"  \"");
    assert_error(&t, line_of(&t, "text: \"  \""), "the line is empty");
}

#[test]
fn unsorted_tracks_are_rejected() {
    let t = with(
        "            (at: 0, kind: Enter(character: \"a\", pos: (0, 0))),\n            (at: 100, kind: SetPose(character: \"a\", pose: \"up\", duration_ms: 50)),",
        "            (at: 100, kind: SetPose(character: \"a\", pose: \"up\", duration_ms: 50)),\n            (at: 0, kind: Enter(character: \"a\", pos: (0, 0))),",
    );
    assert_error(&t, line_of(&t, "Enter("), "must be sorted by time");
}

#[test]
fn stage_state_errors() {
    // Move before Enter.
    let t = with(
        "            (at: 100, kind: SetPose",
        "            (at: 50, kind: Move(character: \"b\", to: (1, 1), duration_ms: 10)),\n            (at: 100, kind: SetPose",
    );
    assert_error(
        &t,
        line_of(&t, "Move("),
        "\"b\" is not on stage at that time",
    );
    // Enter twice.
    let t = with(
        "            (at: 100, kind: SetPose",
        "            (at: 50, kind: Enter(character: \"a\", pos: (1, 1))),\n            (at: 100, kind: SetPose",
    );
    assert_error(
        &t,
        line_of(&t, "at: 50"),
        "already on stage (entered at 0 ms)",
    );
    // Exit, then act.
    let t = with(
        "            (at: 100, kind: SetPose",
        "            (at: 50, kind: Exit(character: \"a\")),\n            (at: 100, kind: SetPose",
    );
    assert_error(
        &t,
        line_of(&t, "SetPose"),
        "\"a\" is not on stage at that time",
    );
    // Exit without Enter.
    let t = with(
        "(at: 0, kind: Enter(character: \"a\", pos: (0, 0))),",
        "(at: 0, kind: Exit(character: \"a\")),",
    );
    assert_error(&t, line_of(&t, "Exit("), "\"a\" is not on stage");
}

#[test]
fn on_screen_speaker_off_stage_is_only_a_warning() {
    let t = with(
        "speaker: \"b\", text: \"Yo\", duration_ms: 500, delivery: VoiceOver",
        "speaker: \"b\", text: \"Yo\", duration_ms: 500",
    );
    let l = load_str(&t, None).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(l.warnings.len(), 1);
    assert_eq!(l.warnings[0].line, Some(line_of(&t, "\"Yo\"")));
    assert!(l.warnings[0].message.contains("not on stage"));
}

#[test]
fn duplicate_names() {
    let t = with("(id: \"b\")", "(id: \"a\")");
    assert_error(
        &t,
        line_of(&t, "(id: \"a\")"),
        "character id \"a\" is used twice",
    );
    let t = with("(name: \"arm\", parent", "(name: \"body\", parent");
    assert_error(
        &t,
        line_of(&t, "(name: \"body\", parent"),
        "part name \"body\" is used twice",
    );
    // An unknown field is a parse error, not a validation error.
    let t = with("(name: \"up\", parts", "(name: \"up\", colour: 1, parts");
    assert!(matches!(load_str(&t, None), Err(SceneError::Parse { .. })));
    let t = with("(name: \"camera\"", "(name: \"stage\"");
    let second = t
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains("(name: \"stage\", events"))
        .map(|(i, _)| i + 1)
        .last()
        .unwrap();
    assert_error(&t, second, "track name \"stage\" is used twice");
}

#[test]
fn layer_shot_must_exist() {
    let t = with("shots: [\"main\"]", "shots: [\"mian\"]");
    assert_error(&t, line_of(&t, "mian"), "no CameraCut creates a shot");
}

#[test]
fn format_version_is_checked() {
    let t = with("name: \"v\",", "format: 2,\n    name: \"v\",");
    assert_error(&t, line_of(&t, "format: 2"), "unsupported format version 2");
}

#[test]
fn bad_numbers() {
    let t = with(
        "CameraCut(shot: \"main\")",
        "CameraCut(zoom: 0, shot: \"main\")",
    );
    assert_error(&t, line_of(&t, "zoom: 0"), "zoom 0 must be more than 0");
    let t = with("Rect(w: 10, h: 10,", "Rect(w: 0, h: 10,");
    assert_error(&t, line_of(&t, "w: 0"), "rectangle size");
}

#[test]
fn huge_angles_offsets_and_zooms_are_out_of_range() {
    // Review follow-up of WP12.1: a typo like 4500000 must not pass as a number.
    let t = with(
        "(part: \"arm\", angle: 45)",
        "(part: \"arm\", angle: 4500000)",
    );
    assert_error(
        &t,
        line_of(&t, "angle: 4500000"),
        "angle 4500000 is out of range",
    );
    let t = with(
        "(part: \"arm\", angle: 45)",
        "(part: \"arm\", angle: 45, offset: (1e30, 0))",
    );
    assert_error(&t, line_of(&t, "offset: (1e30"), "offset");
    let t = with(
        "(part: \"arm\", angle: 45)",
        "(part: \"arm\", angle: 45, scale: (5000, 1))",
    );
    assert_error(&t, line_of(&t, "scale: (5000"), "scale");
    let t = with("pos: (0, 0)", "pos: (250000, 0)");
    assert_error(
        &t,
        line_of(&t, "pos: (250000"),
        "position (250000, 0) is out of range",
    );
    let t = with(
        "CameraCut(shot: \"main\")",
        "CameraCut(zoom: 100000, shot: \"main\")",
    );
    assert_error(
        &t,
        line_of(&t, "zoom: 100000"),
        "zoom 100000 is out of range",
    );
    let t = with(
        "CameraCut(shot: \"main\")",
        "CameraCut(zoom: 0.00001, shot: \"main\")",
    );
    assert_error(&t, line_of(&t, "zoom: 0.00001"), "is out of range");
    let t = with("Rect(w: 10, h: 10,", "Rect(w: 10, h: 9999999,");
    assert_error(&t, line_of(&t, "h: 9999999"), "rectangle size");
    // Large but sane numbers are fine.
    let ok = with(
        "(part: \"arm\", angle: 45)",
        "(part: \"arm\", angle: -720, offset: (500, -500))",
    );
    assert!(load_str(&ok, None).is_ok());
}

#[test]
fn rounded_rectangles_are_checked_like_rectangles() {
    let t = with(
        "Rect(w: 10, h: 10, color: \"#000000\")",
        "RoundRect(w: 10, h: 10, radius: -2, color: \"#000000\")",
    );
    assert_error(
        &t,
        line_of(&t, "radius: -2"),
        "corner radius -2 must be 0 or more",
    );
    let ok = with(
        "Rect(w: 10, h: 10, color: \"#000000\")",
        "RoundRect(w: 10, h: 10, radius: 4, color: \"#000000\")",
    );
    assert!(load_str(&ok, None).is_ok());
}

#[test]
fn all_problems_are_reported_in_one_go() {
    let t = with("Enter(character: \"a\"", "Enter(character: \"zed\"")
        .replace("pose: \"up\"", "pose: \"down\"");
    let errs = errors(&t);
    assert!(errs.len() >= 2, "{errs:#?}");
}

#[test]
fn asset_paths_must_stay_inside_the_asset_root() {
    for bad in [
        "../secret.png",
        "/etc/passwd",
        "C:/x.png",
        "art\\\\x.png",
        "a/../../b.png",
        "",
    ] {
        let t = with(
            "(name: \"sky\", art: Rect(w: 10, h: 10, color: \"#000000\")",
            &format!("(name: \"sky\", art: Image(\"{bad}\")"),
        );
        let errs = errors(&t);
        assert!(
            errs.iter()
                .any(|(l, m)| *l == Some(line_of(&t, "Image(")) && m.contains("asset path")),
            "{bad:?}: {errs:#?}"
        );
    }
    let t = with(
        "text: \"Hi\", duration_ms: 500",
        "text: \"Hi\", duration_ms: 500, voice: \"../v.wav\"",
    );
    assert_error(&t, line_of(&t, "../v.wav"), "voice");
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sf_cutscene_{}_{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn missing_assets_are_listed_not_fatal() {
    let t = with(
        "(name: \"sky\", art: Rect(w: 10, h: 10, color: \"#000000\")",
        "(name: \"sky\", art: Image(\"art/sky.png\")",
    )
    .replace(
        "text: \"Hi\", duration_ms: 500",
        "text: \"Hi\", duration_ms: 500, voice: \"voice/hi.ogg\", lipsync: \"voice/hi.json\"",
    );
    // No root given: nothing checked, nothing listed.
    let l = load_str(&t, None).unwrap_or_else(|e| panic!("{e}"));
    assert!(l.missing_assets.is_empty());

    let root = temp_dir("assets");
    fs::create_dir_all(root.join("art")).unwrap();
    fs::create_dir_all(root.join("voice")).unwrap();
    fs::write(root.join("art/sky.png"), b"x").unwrap();
    let l = load_str(&t, Some(&root)).unwrap_or_else(|e| panic!("{e}"));
    let missing: Vec<_> = l
        .missing_assets
        .iter()
        .map(|m| (m.file.as_str(), m.line))
        .collect();
    let voice_line = line_of(&t, "voice/hi.ogg");
    assert_eq!(
        missing,
        [
            ("voice/hi.ogg", Some(voice_line)),
            ("voice/hi.json", Some(voice_line)),
        ]
    );
    assert!(
        l.missing_assets[0]
            .used_by
            .contains("voice of the Dialogue at 200 ms")
    );

    // Once the files exist the list is empty.
    fs::write(root.join("voice/hi.ogg"), b"x").unwrap();
    fs::write(root.join("voice/hi.json"), b"x").unwrap();
    let l = load_str(&t, Some(&root)).unwrap_or_else(|e| panic!("{e}"));
    assert!(l.missing_assets.is_empty());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn syntax_errors_carry_line_and_column() {
    let t = with("pos: (0, 0)", "pos: (0, 0");
    match load_str(&t, None) {
        Err(SceneError::Parse { line, column, .. }) => {
            assert!(line >= line_of(&t, "Enter("), "line {line}");
            assert!(column >= 1);
        }
        other => panic!("{other:?}"),
    }
    // A wrong variant name.
    let t = with("Enter(", "Entre(");
    match load_str(&t, None) {
        Err(SceneError::Parse { line, message, .. }) => {
            assert_eq!(line, line_of(&t, "Entre("));
            assert!(message.contains("Entre"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    // A missing required field.
    let t = with("(id: \"b\")", "(name: \"b\")");
    match load_str(&t, None) {
        Err(SceneError::Parse { message, .. }) => assert!(message.contains("id"), "{message}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn errors_print_like_compiler_messages() {
    let t = with("Enter(character: \"a\"", "Enter(character: \"zed\"");
    let e = load_str(&t, None).unwrap_err().in_file("scenes/x.ron");
    let want = format!("scenes/x.ron:{}: ", line_of(&t, "Enter("));
    assert!(e.to_string().starts_with(&want), "{e}");
}
