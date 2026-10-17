//! The live preview on real files in a temp directory, with a fake clock:
//! edit in place, atomic-rename saves, a delete-then-create save, a broken
//! save that is fixed, a picture that appears, a Fountain file, and a save
//! that leaves size and time alone. No window, no GPU.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use sf_cutscene::preview::{Applied, LivePreview, Preview, Source};
use sf_cutscene::watch::SceneWatcher;

/// A fresh directory in the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "sf_cutscene-live-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(dir.join("assets")).unwrap();
        TempDir(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn assets(&self) -> PathBuf {
        self.0.join("assets")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn scene(name: &str, length_ms: i32, picture: Option<&str>) -> String {
    let layers = picture.map_or(String::new(), |p| {
        format!(r#"layers: [(name: "bg", art: Image("{p}"), z: -5)],"#)
    });
    format!(
        r##"#![enable(implicit_some)]
Scene(
    name: "{name}",
    {layers}
    timeline: (tracks: [
        (name: "t", events: [(at: 0, kind: Wait(duration_ms: {length_ms}))]),
    ]),
)"##
    )
}

/// The standard rhythm (look every 250 ms, settle for 150 ms).
fn open(source: Source, assets: &Path) -> LivePreview {
    let first = source.load(assets);
    let preview = Preview::new(source, assets.to_path_buf(), first, false);
    LivePreview::with_watcher(preview, Some(SceneWatcher::new()))
}

/// Tick the fake clock from `from` to `to` in 10 ms steps and collect what
/// reloaded.
fn run(live: &mut LivePreview, from: u64, to: u64) -> Vec<Applied> {
    (from..=to)
        .step_by(10)
        .filter_map(|now| live.tick(now).map(|r| r.applied))
        .collect()
}

#[test]
fn saving_in_place_reloads_once_and_keeps_the_playhead() {
    let dir = TempDir::new("inplace");
    let file = dir.path("scene.ron");
    fs::write(&file, scene("one", 6000, None)).unwrap();
    let mut live = open(Source::Scene(file.clone()), &dir.assets());
    assert!(run(&mut live, 0, 1000).is_empty(), "nothing changed yet");
    live.preview_mut().player_mut().seek(2500);

    fs::write(&file, scene("two", 9000, None)).unwrap();
    let reloads = run(&mut live, 1010, 3000);
    assert_eq!(reloads, [Applied::Reloaded { recovered: false }]);
    let p = live.preview();
    assert_eq!(p.player().scene().name, "two");
    assert_eq!(p.player().duration_ms(), 9000);
    assert_eq!(p.player().time_ms(), 2500, "same playhead");
    assert!(
        run(&mut live, 3010, 6000).is_empty(),
        "and no further reload"
    );
}

#[test]
fn the_reload_waits_for_the_burst_to_settle() {
    let dir = TempDir::new("settle");
    let file = dir.path("scene.ron");
    fs::write(&file, scene("one", 6000, None)).unwrap();
    let mut live = open(Source::Scene(file.clone()), &dir.assets());
    assert!(live.tick(0).is_none());
    fs::write(&file, scene("two", 6000, None)).unwrap();
    assert!(live.tick(250).is_none(), "seen at 250, waits 150 ms");
    assert!(live.tick(399).is_none());
    let r = live.tick(400).expect("settled");
    assert_eq!(r.applied, Applied::Reloaded { recovered: false });
}

#[test]
fn a_rename_style_save_reloads() {
    let dir = TempDir::new("rename");
    let file = dir.path("scene.ron");
    fs::write(&file, scene("one", 6000, None)).unwrap();
    let mut live = open(Source::Scene(file.clone()), &dir.assets());
    run(&mut live, 0, 500);

    // Write a temp file next to it and rename it over the scene, as editors do.
    let tmp = dir.path(".scene.ron.swp-4711");
    fs::write(&tmp, scene("two", 7000, None)).unwrap();
    assert!(
        run(&mut live, 510, 800).is_empty(),
        "the temp file is not watched"
    );
    fs::rename(&tmp, &file).unwrap();
    let reloads = run(&mut live, 810, 1500);
    assert_eq!(reloads, [Applied::Reloaded { recovered: false }]);
    assert_eq!(live.preview().player().scene().name, "two");
}

#[test]
fn delete_then_create_is_one_reload_without_an_error() {
    let dir = TempDir::new("delcreate");
    let file = dir.path("scene.ron");
    fs::write(&file, scene("one", 6000, None)).unwrap();
    let mut live = open(Source::Scene(file.clone()), &dir.assets());
    run(&mut live, 0, 500);

    // The editor deletes the file; the next look (at 750) sees it gone ...
    fs::remove_file(&file).unwrap();
    let mut reloads = run(&mut live, 510, 750);
    // ... and the new one is in place before the 150 ms of quiet are over.
    fs::write(&file, scene("two", 6000, None)).unwrap();
    reloads.extend(run(&mut live, 760, 1500));
    assert!(
        !live.preview().has_errors(),
        "{:?}",
        live.preview().errors()
    );
    assert_eq!(live.preview().player().scene().name, "two");
    assert_eq!(reloads, [Applied::Reloaded { recovered: false }]);
}

#[test]
fn a_broken_save_keeps_the_scene_and_the_fix_clears_the_error() {
    let dir = TempDir::new("broken");
    let file = dir.path("scene.ron");
    fs::write(&file, scene("one", 6000, None)).unwrap();
    let mut live = open(Source::Scene(file.clone()), &dir.assets());
    run(&mut live, 0, 500);
    live.preview_mut().player_mut().seek(1000);

    fs::write(&file, scene("two", 6000, None).replace("Wait(", "Wait")).unwrap();
    let reloads = run(&mut live, 510, 1200);
    assert_eq!(reloads, [Applied::Rejected]);
    let p = live.preview();
    assert_eq!(p.player().scene().name, "one", "the last good scene stays");
    let e = &p.errors()[0];
    assert!(e.line.is_some(), "{e}");
    assert!(e.to_string().contains("scene.ron"), "{e}");
    assert!(!p.panels(&[]).is_empty(), "the overlay shows it");

    fs::write(&file, scene("three", 8000, None)).unwrap();
    let reloads = run(&mut live, 1210, 2000);
    assert_eq!(reloads, [Applied::Reloaded { recovered: true }]);
    let p = live.preview();
    assert!(!p.has_errors());
    assert!(p.panels(&[]).is_empty(), "the overlay is gone");
    assert_eq!(p.player().scene().name, "three");
    assert_eq!(p.player().time_ms(), 1000);
}

#[test]
fn a_picture_that_appears_or_changes_reloads() {
    let dir = TempDir::new("picture");
    let file = dir.path("scene.ron");
    fs::write(&file, scene("one", 6000, Some("art/bg.png"))).unwrap();
    let mut live = open(Source::Scene(file.clone()), &dir.assets());
    let watched: Vec<PathBuf> = live.preview().watched_files();
    assert_eq!(watched, [file.clone(), dir.assets().join("art/bg.png")]);
    assert!(run(&mut live, 0, 500).is_empty());

    // The artist saves the picture for the first time.
    fs::create_dir_all(dir.assets().join("art")).unwrap();
    fs::write(dir.assets().join("art/bg.png"), b"first").unwrap();
    assert_eq!(
        run(&mut live, 510, 1200),
        [Applied::Reloaded { recovered: false }],
        "a missing picture appearing is a change"
    );
    // And saves it again with a picture of the same size.
    fs::write(dir.assets().join("art/bg.png"), b"other").unwrap();
    assert_eq!(
        run(&mut live, 1210, 2000),
        [Applied::Reloaded { recovered: false }]
    );
    // The scene stops using it: it is no longer watched.
    fs::write(&file, scene("two", 6000, None)).unwrap();
    assert_eq!(run(&mut live, 2010, 2800).len(), 1);
    assert_eq!(live.preview().watched_files(), [file]);
    fs::write(dir.assets().join("art/bg.png"), b"third").unwrap();
    assert!(run(&mut live, 2810, 4000).is_empty());
}

#[test]
fn a_save_that_leaves_size_and_time_alone_is_still_seen() {
    let dir = TempDir::new("coarse");
    let file = dir.path("scene.ron");
    let a = scene("one", 1500, None);
    let b = scene("one", 1600, None);
    assert_eq!(a.len(), b.len());
    fs::write(&file, &a).unwrap();
    let mut live = open(Source::Scene(file.clone()), &dir.assets());
    run(&mut live, 0, 500);

    // A file system with coarse time stamps: the new content, the old time.
    let mtime = fs::metadata(&file).unwrap().modified().unwrap();
    fs::write(&file, &b).unwrap();
    let f = fs::File::options().write(true).open(&file).unwrap();
    f.set_modified(mtime).unwrap();
    drop(f);
    assert_eq!(run(&mut live, 510, 1200).len(), 1);
    assert_eq!(live.preview().player().duration_ms(), 1600);
}

#[test]
fn a_fountain_file_is_converted_again_on_every_save() {
    let dir = TempDir::new("fountain");
    let file = dir.path("brief.fountain");
    let one = "Title: T\n\nINT. TENT - DAWN\n\nMAJOR HALVERSEN\nCommander.\n";
    fs::write(&file, one).unwrap();
    let mut live = open(Source::Fountain(file.clone()), &dir.assets());
    assert!(live.preview().is_loaded());
    let before = live.preview().player().duration_ms();
    run(&mut live, 0, 500);

    let two =
        format!("{one}\nMAJOR HALVERSEN\nWe hold the crossing until the engineers are across.\n");
    fs::write(&file, two).unwrap();
    assert_eq!(
        run(&mut live, 510, 1200),
        [Applied::Reloaded { recovered: false }]
    );
    assert!(live.preview().player().duration_ms() > before);
}

#[test]
fn without_watching_nothing_ever_reloads() {
    let dir = TempDir::new("nowatch");
    let file = dir.path("scene.ron");
    fs::write(&file, scene("one", 6000, None)).unwrap();
    let source = Source::Scene(file.clone());
    let first = source.load(&dir.assets());
    let mut live = LivePreview::new(Preview::new(source, dir.assets(), first, false), false);
    assert!(!live.is_watching());
    fs::write(&file, scene("two", 6000, None)).unwrap();
    assert!(run(&mut live, 0, 2000).is_empty());
    assert_eq!(live.preview().player().scene().name, "one");
}

#[test]
fn a_window_that_opened_on_a_broken_file_loads_it_once_it_is_fixed() {
    let dir = TempDir::new("openbroken");
    let file = dir.path("scene.ron");
    fs::write(&file, "Scene(").unwrap();
    let mut live = open(Source::Scene(file.clone()), &dir.assets());
    assert!(!live.preview().is_loaded());
    assert!(live.preview().has_errors());
    run(&mut live, 0, 500);
    fs::write(&file, scene("fixed", 6000, None)).unwrap();
    assert_eq!(
        run(&mut live, 510, 1200),
        [Applied::Reloaded { recovered: true }]
    );
    assert!(live.preview().is_loaded());
    assert_eq!(live.preview().player().scene().name, "fixed");
}
