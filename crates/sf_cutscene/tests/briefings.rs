//! The real content: every Fountain briefing in `content/briefings/` converts
//! to a scene that validates, and the committed example scene is that
//! conversion, hand-checked.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sf_cutscene::fountain::{ImportOptions, convert, scene_name};
use sf_cutscene::{EventKind, Scene, load_file, load_str};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn briefings() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(repo().join("content/briefings"))
        .expect("content/briefings exists")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "fountain"))
        .collect();
    v.sort();
    v
}

fn dialogue_texts(s: &Scene) -> Vec<(String, String)> {
    let mut v: Vec<_> = s
        .events_in_order()
        .into_iter()
        .filter_map(|e| match e.kind {
            EventKind::Dialogue { speaker, text, .. } => Some((speaker.clone(), text.clone())),
            _ => None,
        })
        .collect();
    v.sort();
    v
}

#[test]
fn every_briefing_converts_and_validates() {
    let files = briefings();
    assert!(
        files.iter().any(|p| p.ends_with("EXAMPLE-bridge.fountain")),
        "the example briefing is missing"
    );
    let assets = repo().join("assets");
    for path in &files {
        let stem = path.file_stem().unwrap().to_str().unwrap();
        let text = fs::read_to_string(path).unwrap();
        let import = convert(
            &text,
            &ImportOptions {
                name: scene_name(stem),
                source: None,
            },
        );
        let loaded = load_str(&import.ron, Some(&assets))
            .unwrap_or_else(|e| panic!("{stem}: {e}\n--- generated RON ---\n{}", import.ron));
        // Placeholders are allowed: missing assets are information only. The
        // importer assigns no files, so none can be missing.
        assert!(loaded.missing_assets.is_empty(), "{stem}");
        assert!(
            loaded.warnings.is_empty(),
            "{stem}: validation warnings {:?}",
            loaded.warnings
        );
        // The generated text is exactly the scene that was built.
        assert_eq!(
            sf_cutscene::parse_str(&import.ron).unwrap(),
            import.scene,
            "{stem}: RON does not round-trip"
        );

        // Real briefings follow the conventions, so the importer has nothing to
        // complain about. The template has deliberately empty slots.
        if !stem.starts_with('_') {
            assert!(
                import.warnings.is_empty(),
                "{stem}: importer warnings: {:#?}",
                import.warnings
            );
            // Every briefing speaks, is 20 s to 3 minutes long, and has at
            // least one voice we can name.
            let d = dialogue_texts(&import.scene);
            assert!(d.len() >= 3, "{stem}: only {} lines", d.len());
            let secs = import.scene.duration_ms() / 1000;
            assert!((20..=180).contains(&secs), "{stem}: {secs} s");
            println!(
                "{stem}: {} lines, {} characters, {} s",
                d.len(),
                import.scene.characters.len(),
                secs
            );
        }
    }
}

/// Every spoken line of the briefing ends up in the scene, word for word.
#[test]
fn no_dialogue_is_lost_or_invented() {
    for path in briefings() {
        let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
        if stem.starts_with('_') {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        let import = convert(
            &text,
            &ImportOptions {
                name: scene_name(&stem),
                source: None,
            },
        );
        // Independent reading of the source: a cue is an upper-case line after
        // a blank line; its dialogue is the following non-parenthetical lines.
        let lines: Vec<&str> = text.lines().collect();
        let mut want: Vec<String> = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            let l = lines[i].trim();
            let prev_blank = i == 0 || lines[i - 1].trim().is_empty();
            let next_text = lines.get(i + 1).is_some_and(|n| !n.trim().is_empty());
            let name = l.split('(').next().unwrap().trim();
            let is_cue = prev_blank
                && next_text
                && !l.is_empty()
                && !l.starts_with(['[', '>', '('])
                && name == name.to_uppercase()
                && name.chars().any(char::is_alphabetic)
                && !l.starts_with("INT.")
                && !l.starts_with("EXT.");
            if is_cue {
                let mut j = i + 1;
                let mut words: Vec<&str> = Vec::new();
                while j < lines.len() && !lines[j].trim().is_empty() {
                    let t = lines[j].trim();
                    if !t.starts_with('(') && !t.starts_with("[[") {
                        words.extend(t.split_whitespace());
                    }
                    j += 1;
                }
                want.push(words.join(" "));
                i = j;
            } else {
                i += 1;
            }
        }
        let mut got: Vec<String> = dialogue_texts(&import.scene)
            .into_iter()
            .map(|(_, t)| t)
            .collect();
        want.sort();
        got.sort();
        assert_eq!(got, want, "{stem}");
    }
}

#[test]
fn the_example_scene_is_the_converted_example_briefing() {
    let scene_path = repo().join("assets/scenes/example_bridge.ron");
    let loaded =
        load_file(&scene_path, Some(&repo().join("assets"))).unwrap_or_else(|e| panic!("{e}"));
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    assert!(
        loaded.missing_assets.is_empty(),
        "{:?}",
        loaded.missing_assets
    );
    let scene = loaded.scene;
    assert_eq!(scene.name, "example_bridge");
    assert_eq!(
        scene.source.as_deref(),
        Some("content/briefings/EXAMPLE-bridge.fountain")
    );

    // Drift guard: the dialogue is exactly what the briefing says today.
    let text =
        fs::read_to_string(repo().join("content/briefings/EXAMPLE-bridge.fountain")).unwrap();
    let fresh = convert(
        &text,
        &ImportOptions {
            name: "example_bridge".into(),
            source: None,
        },
    );
    assert_eq!(
        dialogue_texts(&scene),
        dialogue_texts(&fresh.scene),
        "assets/scenes/example_bridge.ron no longer matches EXAMPLE-bridge.fountain; \
         regenerate with fountain2scene and redo the hand edits"
    );
    assert_eq!(scene.title, fresh.scene.title);
}

#[test]
fn the_example_scene_plays_through() {
    let scene = load_file(&repo().join("assets/scenes/example_bridge.ron"), None)
        .unwrap()
        .scene;
    let end = scene.duration_ms();
    assert!(end > 20_000, "{end}");

    // Walk the whole scene at 25 fps, as a player would.
    let mut prev = None;
    let mut fired = 0;
    let mut t = 0;
    let mut saw_map = false;
    let mut saw_shake = false;
    let mut max_chars = 0;
    while t <= end {
        let s = scene.sample_step(prev, t);
        fired += s.sounds.len();
        saw_map |= s.map_overlay.is_some();
        saw_shake |= s.camera.shake != sf_cutscene::Vec2::ZERO;
        max_chars = max_chars.max(s.characters.len());
        for c in &s.characters {
            for p in &c.parts {
                assert!(p.world.tx.is_finite() && p.world.ty.is_finite(), "t={t}");
            }
        }
        prev = Some(t);
        t += 40;
    }
    assert!(saw_map, "the map overlay never showed");
    assert!(saw_shake, "the explosion never shook the camera");
    assert_eq!(max_chars, 2);
    // Every sound cue of the scene fired exactly once during the walk.
    let want = scene
        .events_in_order()
        .iter()
        .filter(|e| e.kind.is_sound())
        .count();
    assert_eq!(fired, want);

    // At the middle of every line its subtitle is up with the right text.
    for e in scene.events_in_order() {
        if let EventKind::Dialogue {
            text, duration_ms, ..
        } = e.kind
        {
            let s = scene.sample(e.at + duration_ms / 2);
            assert_eq!(s.subtitle.expect("subtitle").text, text);
        }
    }
    assert!(scene.sample(end).finished);
    assert_eq!(scene.sample(end).fade.alpha, 1.0, "ends on black");
}

fn run(bin: &str, args: &[&Path]) -> (Option<i32>, String, String) {
    let out = Command::new(bin).args(args).output().expect("run binary");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn temp_file(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("sf_cutscene_{}_{name}", std::process::id()))
}

#[test]
fn fountain2scene_cli_writes_a_valid_scene() {
    let out = temp_file("cli_example_bridge.ron");
    let input = repo().join("content/briefings/EXAMPLE-bridge.fountain");
    let (code, stdout, stderr) = run(env!("CARGO_BIN_EXE_fountain2scene"), &[&input, &out]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.contains("0 warnings"), "{stdout}");
    let written = fs::read_to_string(&out).unwrap();
    let loaded = load_str(&written, None).unwrap();
    // Named after the output file.
    assert!(
        loaded.scene.name.starts_with("sf_cutscene_"),
        "{}",
        loaded.scene.name
    );

    // The scene checker agrees.
    let (code, stdout, _) = run(env!("CARGO_BIN_EXE_scene_check"), &[&out]);
    assert_eq!(code, Some(0));
    assert!(stdout.contains("ok"), "{stdout}");
    let _ = fs::remove_file(&out);
}

#[test]
fn cli_reports_usage_and_unreadable_input() {
    let (code, _, stderr) = run(env!("CARGO_BIN_EXE_fountain2scene"), &[]);
    assert_eq!(code, Some(2));
    assert!(stderr.contains("usage:"), "{stderr}");
    let out = temp_file("never_written.ron");
    let (code, _, stderr) = run(
        env!("CARGO_BIN_EXE_fountain2scene"),
        &[Path::new("/no/such/file.fountain"), &out],
    );
    assert_eq!(code, Some(2));
    assert!(stderr.contains("cannot read"), "{stderr}");
    assert!(!out.exists());
}

#[test]
fn scene_check_prints_line_numbered_errors() {
    let bad = temp_file("bad_scene.ron");
    fs::write(
        &bad,
        "Scene(\n    name: \"bad\",\n    timeline: (tracks: [\n        (name: \"t\", events: [\n            (at: 0, kind: Exit(character: \"ghost\")),\n        ]),\n    ]),\n)\n",
    )
    .unwrap();
    let (code, _, stderr) = run(env!("CARGO_BIN_EXE_scene_check"), &[&bad]);
    assert_eq!(code, Some(1));
    let want = format!("{}:5: ", bad.display());
    assert!(stderr.contains(&want), "{stderr}");
    assert!(stderr.contains("unknown character \"ghost\""), "{stderr}");
    let _ = fs::remove_file(&bad);
}
