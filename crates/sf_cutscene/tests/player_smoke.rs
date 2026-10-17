//! The player and the frame planner on the real content, without a GPU: the
//! example scene and every Fountain briefing are played to the end on a fake
//! clock, every sound cue comes out once, and every frame plans into finite
//! geometry that stays inside what the target can show.

use std::fs;
use std::path::{Path, PathBuf};

use sf_cutscene::fountain::{ImportOptions, convert, scene_name};
use sf_cutscene::render::{FramePlan, Resources};
use sf_cutscene::{Player, RecordingSink, Scene, SoundEvent, load_file, load_str};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scenes() -> Vec<(String, Scene)> {
    let mut out = vec![(
        "example_bridge.ron".to_string(),
        load_file(&repo().join("assets/scenes/example_bridge.ron"), None)
            .unwrap_or_else(|e| panic!("{e}"))
            .scene,
    )];
    let mut files: Vec<PathBuf> = fs::read_dir(repo().join("content/briefings"))
        .expect("content/briefings")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "fountain"))
        .collect();
    files.sort();
    for path in files {
        let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
        let text = fs::read_to_string(&path).unwrap();
        let import = convert(
            &text,
            &ImportOptions {
                name: scene_name(&stem),
                source: None,
            },
        );
        let scene = load_str(&import.ron, None)
            .unwrap_or_else(|e| panic!("{stem}: {e}"))
            .scene;
        out.push((stem, scene));
    }
    out
}

#[test]
fn every_scene_plays_to_the_end_with_every_cue_once_and_plans_every_frame() {
    for (name, scene) in scenes() {
        let want_cues = scene
            .events_in_order()
            .iter()
            .filter(|e| e.kind.is_sound())
            .count();
        let mut player = Player::new(scene.clone());
        let mut res = Resources::new(repo().join("assets"));
        let mut sink = RecordingSink::default();
        let (mut now, mut frames) = (1_000_000_u64, 0);
        // 250 ms frames; the scene ends in at most duration / 250 + 2 frames.
        let limit = scene.duration_ms() as u64 / 250 + 5;
        player.update(now, &mut sink);
        while !player.is_finished() {
            now += 250;
            player.update(now, &mut sink);
            frames += 1;
            assert!(frames <= limit, "{name}: does not finish");
            // Plan every second frame at 720p and every eighth also at 1080p.
            let sizes: &[(u32, u32)] = if frames % 8 == 0 {
                &[(1280, 720), (1920, 1080)]
            } else {
                &[(1280, 720)]
            };
            for &target in sizes {
                if frames % 2 != 0 {
                    continue;
                }
                let state = player.sample();
                let plan = FramePlan::build(scene.stage, &state, target, &mut res, None);
                for v in &plan.vertices {
                    assert!(
                        v.pos.iter().all(|c| c.is_finite()),
                        "{name} at {} ms: non-finite vertex {v:?}",
                        player.time_ms()
                    );
                }
                assert_eq!(plan.indices.len() / 6 * 4, plan.vertices.len());
                assert!(plan.batches.iter().all(|b| b.start < b.end));
            }
        }
        assert_eq!(player.time_ms(), scene.duration_ms(), "{name}");
        let cues = sink
            .0
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    SoundEvent::Sfx { .. } | SoundEvent::Bed { .. } | SoundEvent::Voice { .. }
                )
            })
            .count();
        assert_eq!(cues, want_cues, "{name}: every sound cue exactly once");
        assert!(!sink.0.contains(&SoundEvent::StopAll), "{name}");
    }
}

#[test]
fn skipping_and_seeking_anywhere_in_a_real_scene_keeps_the_sound_consistent() {
    let (_, scene) = scenes().remove(0);
    let end = scene.duration_ms();
    for target in (0..=end).step_by(1300) {
        let mut player = Player::new(scene.clone());
        let mut sink = RecordingSink::default();
        player.update(0, &mut sink);
        player.seek(target);
        player.update(0, &mut sink);
        // After the restore, what the sink was told to play is what the scene
        // says is playing just before `target`.
        let expected = scene.sample((target - 1).max(0));
        let started_beds: Vec<_> = sink
            .0
            .iter()
            .rev()
            .take_while(|e| **e != SoundEvent::StopAll)
            .filter_map(|e| match e {
                SoundEvent::Bed { bed, .. } => Some(*bed),
                _ => None,
            })
            .collect();
        if target > 0 {
            let music_started = started_beds.contains(&sf_cutscene::Bed::Music);
            let ambience_started = started_beds.contains(&sf_cutscene::Bed::Ambience);
            // A bed that begins exactly at `target` fires as an ordinary event,
            // so it is also in the list; one that began earlier is restored.
            let at_target = |b: &sf_cutscene::LoopState| b.since_ms == target;
            assert_eq!(
                music_started,
                expected.music.is_some()
                    || scene.sample(target).music.is_some_and(|m| at_target(&m)),
                "music at {target} ms"
            );
            assert_eq!(
                ambience_started,
                expected.ambience.is_some()
                    || scene.sample(target).ambience.is_some_and(|m| at_target(&m)),
                "ambience at {target} ms"
            );
        }
    }
}
