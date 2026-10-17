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

/// Every spoken line of a Fountain file in document order, read without the
/// importer: a cue is an upper-case line after a blank line; its dialogue is
/// the following non-parenthetical lines.
fn spoken_lines(text: &str) -> Vec<String> {
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
    want
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
        // Independent reading of the source, not through the importer.
        let mut want = spoken_lines(&text);
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

// ---------------------------------------------------------------------------
// WP12.5: the demo scene, the mission 1 briefing, directed by hand.
//
// The text is Tijmen's. The scene may add art, movement, camera and pauses; it
// may not change, add or drop a spoken line, a speaker or a sound cue name.

const DEMO_SCENE: &str = "assets/scenes/f1_m01_briefing.ron";
const DEMO_BRIEFING: &str = "content/briefings/F1-M01.fountain";
/// The shot of the command post; the generator names it after the scene heading.
const DEMO_SHOT: &str = "forward_command_post_dawn";

fn demo() -> sf_cutscene::LoadedScene {
    load_file(&repo().join(DEMO_SCENE), Some(&repo().join("assets")))
        .unwrap_or_else(|e| panic!("{e}"))
}

fn demo_briefing_text() -> String {
    fs::read_to_string(repo().join(DEMO_BRIEFING)).unwrap()
}

/// What the generator makes of the briefing today.
fn demo_fresh() -> sf_cutscene::fountain::Import {
    convert(
        &demo_briefing_text(),
        &ImportOptions {
            name: "f1_m01_briefing".into(),
            source: None,
        },
    )
}

/// `(speaker, text, delivery)` of every line, in the order it is spoken.
fn spoken(s: &Scene) -> Vec<(String, String, String)> {
    s.events_in_order()
        .into_iter()
        .filter_map(|e| match e.kind {
            EventKind::Dialogue {
                speaker,
                text,
                delivery,
                ..
            } => Some((speaker.clone(), text.clone(), format!("{delivery:?}"))),
            _ => None,
        })
        .collect()
}

/// The sound cues by name, sorted (`Sfx radio click`, `Music Stop `, ...).
fn cue_names(s: &Scene) -> Vec<String> {
    let mut v: Vec<String> = s
        .events_in_order()
        .into_iter()
        .filter_map(|e| match e.kind {
            EventKind::Sfx { note, .. } => Some(format!("Sfx {note}")),
            EventKind::Music { action, note, .. } => Some(format!("Music {action:?} {note}")),
            EventKind::Ambience { action, note, .. } => Some(format!("Ambience {action:?} {note}")),
            _ => None,
        })
        .collect();
    v.sort();
    v
}

#[test]
fn demo_scene_tells_the_briefing_word_for_word() {
    let loaded = demo();
    let scene = &loaded.scene;
    let fresh = demo_fresh().scene;
    assert_eq!(scene.source.as_deref(), Some(DEMO_BRIEFING));
    assert_eq!(scene.title, fresh.title);

    // Every line, its speaker and the order: the same as the importer makes
    // of the Fountain file ...
    let ours = spoken(scene);
    assert_eq!(ours, spoken(&fresh), "dialogue differs from the importer's");
    assert_eq!(ours.len(), 8);
    // ... and the same as an independent reading of that file.
    let texts: Vec<String> = ours.iter().map(|(_, t, _)| t.clone()).collect();
    assert_eq!(texts, spoken_lines(&demo_briefing_text()));

    // The cues keep their names (SFX, music, ambience, the map note).
    assert_eq!(cue_names(scene), cue_names(&fresh));
    let map_notes = |s: &Scene| -> Vec<String> {
        s.events_in_order()
            .into_iter()
            .filter_map(|e| match e.kind {
                EventKind::MapOverlay { note, .. } => Some(note.clone()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(map_notes(scene), map_notes(&fresh));

    // Every parenthetical is still a pose hint for its speaker.
    let hints = |s: &Scene| -> Vec<(String, String)> {
        s.events_in_order()
            .into_iter()
            .filter_map(|e| match e.kind {
                EventKind::SetPose {
                    character,
                    hint: Some(h),
                    ..
                } => Some((character.clone(), h.clone())),
                _ => None,
            })
            .collect()
    };
    let ours = hints(scene);
    for h in hints(&fresh) {
        assert!(ours.contains(&h), "pose hint {h:?} is gone");
    }
    // The cast is the importer's: the same ids and names.
    let cast = |s: &Scene| -> Vec<(String, String)> {
        let mut v: Vec<_> = s
            .characters
            .iter()
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect();
        v.sort();
        v
    };
    assert_eq!(cast(scene), cast(&fresh));
}

/// The importer's durations stand, give or take 30 %.
#[test]
fn demo_scene_timing_stays_within_30_percent_of_the_importer() {
    let durations = |s: &Scene| -> Vec<i32> {
        s.events_in_order()
            .into_iter()
            .filter_map(|e| match e.kind {
                EventKind::Dialogue { duration_ms, .. } => Some(*duration_ms),
                _ => None,
            })
            .collect()
    };
    let ours = durations(&demo().scene);
    let fresh = durations(&demo_fresh().scene);
    assert_eq!(ours.len(), fresh.len());
    for (i, (a, b)) in ours.iter().zip(&fresh).enumerate() {
        assert!(
            (b * 70 / 100..=b * 130 / 100).contains(a),
            "line {}: {a} ms against the importer's {b} ms",
            i + 1
        );
    }
}

/// Every edit of the generated skeleton says `hand edit`: an event line of the
/// scene file is either one the generator writes today (comment or not) or
/// carries that mark.
#[test]
fn demo_scene_marks_every_hand_edit() {
    fn code(line: &str) -> &str {
        line.split("  //").next().unwrap().trim()
    }
    let generated = demo_fresh().ron;
    let generated: Vec<&str> = generated
        .lines()
        .filter(|l| l.trim_start().starts_with("(at:"))
        .map(code)
        .collect();
    assert!(generated.len() > 20, "the generator's events were found");

    let text = fs::read_to_string(repo().join(DEMO_SCENE)).unwrap();
    let events: Vec<&str> = text
        .lines()
        .filter(|l| l.trim_start().starts_with("(at:"))
        .collect();
    assert!(events.len() > 40, "{} event lines", events.len());
    let mut kept = 0;
    for line in &events {
        if line.contains("hand edit") {
            continue;
        }
        kept += 1;
        assert!(
            generated.contains(&code(line)),
            "event neither as generated nor marked `hand edit`:\n{line}"
        );
    }
    assert!(kept >= 5, "some events are exactly the generator's: {kept}");
    // The hand-made blocks say so too.
    for needle in [
        "// hand edit: the cast",
        "-- the command post",
        "-- the map insert",
    ] {
        assert!(text.contains(needle), "missing the comment `{needle}`");
    }
}

#[test]
fn demo_scene_is_all_geometry_and_silent() {
    let loaded = demo();
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    // Nothing to be missing: no picture, no sound, no voice.
    assert!(
        loaded.missing_assets.is_empty(),
        "{:?}",
        loaded.missing_assets
    );
    let scene = &loaded.scene;
    let is_image = |a: &sf_cutscene::Art| matches!(a, sf_cutscene::Art::Image(_));
    assert!(!scene.layers.iter().any(|l| is_image(&l.art)));
    assert!(
        !scene
            .characters
            .iter()
            .any(|c| c.parts.iter().any(|p| is_image(&p.art)))
    );
    for e in scene.events_in_order() {
        match e.kind {
            EventKind::Sfx { sound, .. } => assert!(sound.is_none()),
            EventKind::Music { file, .. } | EventKind::Ambience { file, .. } => {
                assert!(file.is_none());
            }
            EventKind::Dialogue { voice, lipsync, .. } => {
                assert!(
                    voice.is_none() && lipsync.is_none(),
                    "voice slots stay empty"
                );
            }
            _ => {}
        }
    }
}

/// The part and pose names an artist has to keep when replacing the
/// placeholders (docs/cutscene-format.md, section 10).
const KEPT_PARTS: [&str; 7] = [
    "torso",
    "head",
    "mouth",
    "upper_arm_l",
    "upper_arm_r",
    "forearm_l",
    "forearm_r",
];
const KEPT_POSES: [&str; 4] = ["neutral", "point", "lean", "arms_crossed"];

#[test]
fn every_speaker_has_a_rig_with_the_kept_parts_and_poses() {
    let scene = demo().scene;
    let speakers: std::collections::BTreeSet<String> =
        spoken(&scene).into_iter().map(|(s, _, _)| s).collect();
    assert_eq!(speakers.len(), 2);
    for id in &speakers {
        let c = scene
            .characters
            .iter()
            .find(|c| &c.id == id)
            .unwrap_or_else(|| panic!("{id} has no character"));
        for part in KEPT_PARTS {
            assert!(
                c.parts.iter().any(|p| p.name == part),
                "{id}: no part {part}"
            );
        }
        assert!(c.parts.len() >= 10, "{id}: a rig, not a stick");
        assert!(
            (3..=5).contains(&c.poses.len()),
            "{id}: {} poses",
            c.poses.len()
        );
        for pose in KEPT_POSES {
            assert!(
                c.poses.iter().any(|p| p.name == pose),
                "{id}: no pose {pose}"
            );
        }
        for pose in &c.poses {
            for pp in &pose.parts {
                assert!(
                    c.parts.iter().any(|p| p.name == pp.part),
                    "{id}/{}: part {}",
                    pose.name,
                    pp.part
                );
            }
        }
        // Distinct poses look distinct.
        for (i, a) in c.poses.iter().enumerate() {
            for b in &c.poses[i + 1..] {
                assert_ne!(a.parts, b.parts, "{id}: {} equals {}", a.name, b.name);
            }
        }
    }
    // Mira wears a headset: hand to ear for the radio.
    let mira = scene.characters.iter().find(|c| c.id == "mira").unwrap();
    assert!(mira.poses.iter().any(|p| p.name == "radio"));
}

#[test]
fn the_command_post_has_two_or_three_parallax_depths() {
    let scene = demo().scene;
    let post: Vec<_> = scene
        .layers
        .iter()
        .filter(|l| l.shots.iter().any(|s| s == DEMO_SHOT))
        .collect();
    let mut depths: Vec<f32> = post.iter().map(|l| l.parallax).collect();
    depths.sort_by(f32::total_cmp);
    depths.dedup();
    assert!((2..=3).contains(&depths.len()), "depths {depths:?}");
    assert!(depths.contains(&1.0), "the stage plane is one of them");
    assert!(depths.iter().any(|d| *d < 1.0), "something far away");
    // In front of and behind the figures.
    assert!(post.iter().any(|l| l.z < 0) && post.iter().any(|l| l.z >= 0));
    // The map shot has its own picture, drawn over the room.
    let map: Vec<_> = scene
        .layers
        .iter()
        .filter(|l| l.shots.iter().any(|s| s == "map"))
        .collect();
    assert!(map.len() >= 8, "{} map layers", map.len());
    assert!(map.iter().all(|l| l.z > 0 && l.parallax == 0.0));
}

#[test]
fn demo_scene_is_directed_walk_through() {
    let scene = demo().scene;
    let events = scene.events_in_order();
    let at = |pred: &dyn Fn(&EventKind) -> bool| -> Vec<i32> {
        events
            .iter()
            .filter(|e| pred(e.kind))
            .map(|e| e.at)
            .collect()
    };
    let line = |needle: &str| -> (i32, i32) {
        events
            .iter()
            .find_map(|e| match e.kind {
                EventKind::Dialogue {
                    text, duration_ms, ..
                } if text.starts_with(needle) => Some((e.at, e.at + duration_ms)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no line starting {needle:?}"))
    };

    // Both characters walk in before the first line and leave at the end.
    let first_line = line("The relay went dark").0;
    for who in ["captain_elian", "mira"] {
        let enter = at(&|k| matches!(k, EventKind::Enter { character, .. } if character == who));
        let exit = at(&|k| matches!(k, EventKind::Exit { character } if character == who));
        let moves = at(&|k| matches!(k, EventKind::Move { character, .. } if character == who));
        assert_eq!((enter.len(), exit.len()), (1, 1), "{who}");
        assert!(enter[0] < first_line && moves.len() >= 2, "{who}");
        assert!(
            exit[0] > line("Note it").1,
            "{who} leaves after the last line"
        );
    }
    assert!(scene.sample(scene.duration_ms()).characters.is_empty());

    // Camera: moves, a push-in on the twist, the cut to the map and back.
    let pans = at(&|k| matches!(k, EventKind::CameraPan { .. })).len();
    let zooms = at(&|k| matches!(k, EventKind::CameraZoom { .. })).len();
    assert!(pans >= 4 && zooms >= 4, "{pans} pans, {zooms} zooms");
    let twist = line("One thing I cannot explain");
    let push = events.iter().any(|e| {
        matches!(e.kind, EventKind::CameraZoom { to, .. } if *to >= 1.3)
            && (twist.0 - 100..twist.1).contains(&e.at)
    });
    assert!(push, "a close push-in on Mira's doubt");

    let cuts: Vec<(i32, Option<String>)> = events
        .iter()
        .filter_map(|e| match e.kind {
            EventKind::CameraCut { shot, .. } => Some((e.at, shot.clone())),
            _ => None,
        })
        .collect();
    let shots: Vec<Option<&str>> = cuts.iter().map(|(_, s)| s.as_deref()).collect();
    assert_eq!(shots, [Some(DEMO_SHOT), Some("map"), Some(DEMO_SHOT)]);
    let (map_in, map_out) = (cuts[1].0, cuts[2].0);
    let overlay = events
        .iter()
        .find_map(|e| match e.kind {
            EventKind::MapOverlay {
                markers,
                duration_ms,
                ..
            } => Some((e.at, *duration_ms, markers.clone())),
            _ => None,
        })
        .expect("a map overlay");
    assert_eq!(overlay.0, map_in, "the overlay starts with the cut");
    assert!(overlay.0 + overlay.1 <= map_out);
    assert!(overlay.2.len() >= 3 && overlay.2.iter().any(|m| m.to.is_some()));
    assert!(overlay.2.iter().any(|m| m.to.is_none()));
    let scouts = line("Scouts report");
    assert!(
        map_in < scouts.0 && scouts.1 <= map_out,
        "the map line is spoken over the map"
    );
    // The briefing has no explosion, so nothing shakes.
    assert!(
        events
            .iter()
            .all(|e| !matches!(e.kind, EventKind::CameraShake { .. }))
    );

    // Walk it at 25 fps like a player: finite everywhere, sounds exactly once.
    let end = scene.duration_ms();
    assert!((55_000..=70_000).contains(&end), "{end}");
    let (mut prev, mut t, mut fired) = (None, 0, 0);
    let (mut saw_map, mut max_chars) = (false, 0);
    while t <= end {
        let s = scene.sample_step(prev, t);
        fired += s.sounds.len();
        saw_map |= s.map_overlay.is_some();
        max_chars = max_chars.max(s.characters.len());
        for c in &s.characters {
            for p in &c.parts {
                assert!(p.world.tx.is_finite() && p.world.ty.is_finite(), "t={t}");
            }
        }
        prev = Some(t);
        t += 40;
    }
    assert!(saw_map);
    assert_eq!(max_chars, 2);
    let want = events.iter().filter(|e| e.kind.is_sound()).count();
    assert_eq!(fired, want);
    // At the middle of every line its subtitle is up, with the right text and
    // with both characters on stage; the film ends on black.
    for e in &events {
        if let EventKind::Dialogue {
            text, duration_ms, ..
        } = e.kind
        {
            let s = scene.sample(e.at + duration_ms / 2);
            assert_eq!(s.subtitle.expect("subtitle").text, *text);
            assert_eq!(s.characters.len(), 2);
        }
    }
    let last = scene.sample(end);
    assert!(last.finished);
    assert_eq!(last.fade.alpha, 1.0, "ends on black");
}

#[test]
fn scene_check_passes_the_demo_scene_with_no_warnings() {
    let scene = repo().join(DEMO_SCENE);
    let assets = repo().join("assets");
    let out = Command::new(env!("CARGO_BIN_EXE_scene_check"))
        .arg(&scene)
        .arg("--assets")
        .arg(&assets)
        .output()
        .expect("run scene_check");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    assert!(
        stdout.contains(", 0 warnings, 0 missing assets"),
        "{stdout}"
    );
    assert!(stderr.is_empty(), "{stderr}");
}
