//! The live preview (WP12.6): everything `play_scene --watch` does that is not
//! drawing, so that it can be tested without a window.
//!
//! * [`Source`] is the file being edited: a scene RON, or a Fountain briefing
//!   that is converted on the fly. [`Source::load`] gives a [`Loaded`] scene or
//!   a [`LoadFailure`] whose [`Diagnostic`]s read `file:line:col: message`
//!   (for a Fountain file the line is the line **in the Fountain file**).
//! * [`Preview`] is the reload state machine. It owns the [`Player`] and shows
//!   the last good scene; [`Preview::apply`] takes the result of a (re)load:
//!   a good scene replaces the old one **at the same time `t`** (clamped to the
//!   new length, sound restored as on a seek) and clears the error; a bad one
//!   changes nothing but the error list.
//! * [`LivePreview`] adds the [`SceneWatcher`]: every frame the window calls
//!   [`LivePreview::tick`] with its clock; when the scene file or one of its
//!   pictures changed and settled, the file is loaded again and applied.
//! * [`Preview::panels`] turns all of it into the text boxes the renderer draws
//!   (error list, missing pictures, the writer's HUD).

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::fountain::{ImportOptions, convert, scene_name};
use crate::load::{LoadedScene, SceneError, load_str, parse_str};
use crate::math::Ms;
use crate::model::{Art, Delivery, Scene};
use crate::player::{Mode, Player};
use crate::render::{HudLine, HudPanel, HudTone};
use crate::watch::SceneWatcher;

/// How many error lines the overlay lists before "... and N more".
pub const MAX_LISTED_ERRORS: usize = 12;

/// One problem, located like a compiler message: `file:line:col: message`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub file: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (Some(l), Some(c)) => write!(f, "{}:{l}:{c}: {}", self.file, self.message),
            (Some(l), None) => write!(f, "{}:{l}: {}", self.file, self.message),
            _ => write!(f, "{}: {}", self.file, self.message),
        }
    }
}

/// Why a file did not load: what to show, and whether it was a matter of
/// reading the file rather than of its content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadFailure {
    pub diagnostics: Vec<Diagnostic>,
    /// The file could not be read at all.
    pub unreadable: bool,
}

/// A scene that loaded, with what to tell the writer about it.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub scene: LoadedScene,
    /// Warnings of the Fountain converter and of the validator.
    pub warnings: Vec<Diagnostic>,
}

impl Loaded {
    /// The warnings and the missing files as lines for the terminal (the same
    /// the first load prints).
    pub fn notes(&self, file: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .warnings
            .iter()
            .map(|w| match (w.line, w.column) {
                (Some(l), _) => format!("{}:{l}: warning: {}", w.file, w.message),
                _ => format!("{}: warning: {}", w.file, w.message),
            })
            .collect();
        for m in &self.scene.missing_assets {
            out.push(format!(
                "{file}: note: missing {} ({}) - drawn as a placeholder or silent",
                m.file, m.used_by
            ));
        }
        out
    }
}

pub type LoadResult = Result<Loaded, LoadFailure>;

/// The file being edited.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A scene RON file.
    Scene(PathBuf),
    /// A Fountain briefing, converted in memory each time (nothing is written).
    Fountain(PathBuf),
}

impl Source {
    pub fn path(&self) -> &Path {
        match self {
            Source::Scene(p) | Source::Fountain(p) => p,
        }
    }

    /// Read the file and load it. `assets` is the asset root (to list missing
    /// pictures and sounds).
    pub fn load(&self, assets: &Path) -> LoadResult {
        let file = self.path().display().to_string();
        match std::fs::read_to_string(self.path()) {
            Ok(text) => self.load_text(&text, assets),
            Err(e) => Err(LoadFailure {
                diagnostics: vec![Diagnostic {
                    file,
                    line: None,
                    column: None,
                    message: format!("cannot read: {e}"),
                }],
                unreadable: true,
            }),
        }
    }

    /// Load from text already read (the file read is the only I/O of
    /// [`Source::load`]).
    pub fn load_text(&self, text: &str, assets: &Path) -> LoadResult {
        let file = self.path().display().to_string();
        match self {
            Source::Scene(_) => match load_str(text, Some(assets)) {
                Ok(l) => {
                    let warnings = l
                        .warnings
                        .iter()
                        .map(|w| Diagnostic {
                            file: file.clone(),
                            line: w.line,
                            column: None,
                            message: w.message.clone(),
                        })
                        .collect();
                    Ok(Loaded { scene: l, warnings })
                }
                Err(e) => Err(failure(e, &file, None)),
            },
            Source::Fountain(path) => {
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("scene");
                let import = convert(
                    text,
                    &ImportOptions {
                        name: scene_name(stem),
                        source: Some(file.replace('\\', "/")),
                    },
                );
                let mut warnings: Vec<Diagnostic> = import
                    .warnings
                    .iter()
                    .map(|w| Diagnostic {
                        file: file.clone(),
                        line: Some(w.line),
                        column: None,
                        message: w.message.clone(),
                    })
                    .collect();
                match load_str(&import.ron, Some(assets)) {
                    Ok(l) => {
                        for w in &l.warnings {
                            let (line, message) =
                                fountain_position(&import.ron, w.line, &w.message);
                            warnings.push(Diagnostic {
                                file: file.clone(),
                                line,
                                column: None,
                                message,
                            });
                        }
                        Ok(Loaded { scene: l, warnings })
                    }
                    Err(e) => Err(failure(e, &file, Some(&import.ron))),
                }
            }
        }
    }
}

/// `// line N` at the end of line `ron_line` (1-based) of a converted scene:
/// the line of the Fountain file the event came from.
fn fountain_line_of(ron: &str, ron_line: usize) -> Option<usize> {
    let text = ron.lines().nth(ron_line.checked_sub(1)?)?;
    let (_, tail) = text.rsplit_once("// line ")?;
    tail.trim().parse().ok()
}

/// Where a problem in a converted scene is, in terms of the Fountain file: the
/// line when the event carries one, otherwise the message says where in the
/// generated scene it is.
fn fountain_position(ron: &str, ron_line: Option<usize>, message: &str) -> (Option<usize>, String) {
    match ron_line {
        Some(l) => match fountain_line_of(ron, l) {
            Some(n) => (Some(n), message.to_string()),
            None => (None, format!("{message} (generated scene, line {l})")),
        },
        None => (None, message.to_string()),
    }
}

fn failure(e: SceneError, file: &str, converted: Option<&str>) -> LoadFailure {
    let position = |line: Option<usize>, message: &str| match converted {
        Some(ron) => fountain_position(ron, line, message),
        None => (line, message.to_string()),
    };
    let diagnostics = match e {
        SceneError::Io { message, .. } => vec![Diagnostic {
            file: file.to_string(),
            line: None,
            column: None,
            message,
        }],
        SceneError::Parse {
            line,
            column,
            message,
            ..
        } => {
            let (line, message) = position(Some(line), &message);
            vec![Diagnostic {
                file: file.to_string(),
                line,
                // The column of a generated scene means nothing in Fountain.
                column: converted.is_none().then_some(column),
                message,
            }]
        }
        SceneError::Invalid { issues, .. } => issues
            .iter()
            .map(|i| {
                let (line, message) = position(i.line, &i.message);
                Diagnostic {
                    file: file.to_string(),
                    line,
                    column: None,
                    message,
                }
            })
            .collect(),
    };
    LoadFailure {
        diagnostics,
        unreadable: false,
    }
}

/// Every picture a scene draws, once each, sorted: the files to watch (and the
/// ones the renderer loads).
pub fn picture_paths(scene: &Scene) -> Vec<String> {
    let layers = scene.layers.iter().map(|l| &l.art);
    let parts = scene
        .characters
        .iter()
        .flat_map(|c| c.parts.iter().map(|p| &p.art));
    let set: BTreeSet<&str> = layers
        .chain(parts)
        .filter_map(|a| match a {
            Art::Image(p) => Some(p.as_str()),
            _ => None,
        })
        .collect();
    set.into_iter().map(str::to_owned).collect()
}

/// What [`Preview::apply`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Applied {
    /// The new scene is showing. `recovered`: the load before this one had
    /// failed, so the error overlay is gone now.
    Reloaded { recovered: bool },
    /// The new file is not valid; the last good scene keeps playing and the
    /// errors are in [`Preview::errors`].
    Rejected,
}

/// The scene being shown, the error of the last failed reload, and the
/// writer's HUD settings.
#[derive(Clone, Debug)]
pub struct Preview {
    source: Source,
    assets: PathBuf,
    player: Player,
    errors: Vec<Diagnostic>,
    /// A real scene is showing (false: the placeholder before the first good
    /// load, so the window can open on a broken file).
    loaded: bool,
    reloads: u32,
    pictures: Vec<String>,
    hud_visible: bool,
}

/// An empty scene that stands in until the first good load.
fn placeholder() -> Scene {
    parse_str(r#"Scene(name: "waiting", title: Some("(waiting for a valid scene)"))"#)
        .expect("the placeholder scene is valid")
}

impl Preview {
    /// Start with the result of the first load. A failed first load gives a
    /// placeholder scene and the error list, so a watching window can open on a
    /// broken file and show the problem.
    pub fn new(source: Source, assets: PathBuf, first: LoadResult, hud_visible: bool) -> Preview {
        let mut p = Preview {
            source,
            assets,
            player: Player::new(placeholder()),
            errors: Vec::new(),
            loaded: false,
            reloads: 0,
            pictures: Vec::new(),
            hud_visible,
        };
        match first {
            Ok(l) => {
                p.pictures = picture_paths(&l.scene.scene);
                p.player = Player::new(l.scene.scene);
                p.loaded = true;
            }
            Err(f) => p.errors = f.diagnostics,
        }
        p
    }

    /// Take the result of a (re)load. See the module docs.
    pub fn apply(&mut self, result: LoadResult) -> Applied {
        match result {
            Ok(l) => {
                let recovered = !self.errors.is_empty();
                self.errors.clear();
                self.pictures = picture_paths(&l.scene.scene);
                if self.loaded {
                    self.player.replace_scene(l.scene.scene);
                } else {
                    self.player = Player::new(l.scene.scene);
                    self.loaded = true;
                }
                self.reloads += 1;
                Applied::Reloaded { recovered }
            }
            Err(f) => {
                self.errors = f.diagnostics;
                Applied::Rejected
            }
        }
    }

    pub fn source(&self) -> &Source {
        &self.source
    }

    pub fn assets(&self) -> &Path {
        &self.assets
    }

    pub fn player(&self) -> &Player {
        &self.player
    }

    pub fn player_mut(&mut self) -> &mut Player {
        &mut self.player
    }

    /// The errors of the last load; empty when the file is good.
    pub fn errors(&self) -> &[Diagnostic] {
        &self.errors
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Whether a real scene is showing (not the placeholder).
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// How many good reloads happened since the start.
    pub fn reloads(&self) -> u32 {
        self.reloads
    }

    /// The pictures the shown scene refers to.
    pub fn pictures(&self) -> &[String] {
        &self.pictures
    }

    pub fn hud_visible(&self) -> bool {
        self.hud_visible
    }

    pub fn toggle_hud(&mut self) {
        self.hud_visible = !self.hud_visible;
    }

    /// Step the playhead by `delta_ms` (J/L, `,` and `.`).
    pub fn step(&mut self, delta_ms: Ms) {
        self.player.seek_by(delta_ms);
    }

    /// The files whose change means a reload: the scene file, then the
    /// pictures of the scene on screen, as paths.
    pub fn watched_files(&self) -> Vec<PathBuf> {
        let mut v = vec![self.source.path().to_path_buf()];
        v.extend(self.pictures.iter().map(|p| self.assets.join(p)));
        v
    }

    /// The text boxes to draw: the error list (when the last load failed), the
    /// pictures that could not be loaded (`failed_pictures`: path, reason),
    /// and the writer's HUD (when switched on).
    pub fn panels(&self, failed_pictures: &[(String, String)]) -> Vec<HudPanel> {
        let mut out = Vec::new();
        if !self.errors.is_empty() {
            let head = if self.loaded {
                "Reload failed - still showing the last good version:"
            } else {
                "No valid scene yet - fix the file and save it:"
            };
            let mut lines = vec![HudLine::new(head, HudTone::Warning)];
            for d in self.errors.iter().take(MAX_LISTED_ERRORS) {
                lines.push(HudLine::new(d.to_string(), HudTone::Error));
            }
            if self.errors.len() > MAX_LISTED_ERRORS {
                lines.push(HudLine::new(
                    format!("... and {} more", self.errors.len() - MAX_LISTED_ERRORS),
                    HudTone::Dim,
                ));
            }
            out.push(HudPanel {
                tone: HudTone::Error,
                lines,
            });
        }
        if !failed_pictures.is_empty() {
            let mut lines = vec![HudLine::new(
                "Pictures not loaded (drawn magenta):",
                HudTone::Warning,
            )];
            for (path, why) in failed_pictures {
                lines.push(HudLine::new(format!("{path}: {why}"), HudTone::Warning));
            }
            out.push(HudPanel {
                tone: HudTone::Warning,
                lines,
            });
        }
        if self.hud_visible {
            out.push(HudPanel {
                tone: HudTone::Normal,
                lines: self.hud_lines(),
            });
        }
        out
    }

    /// The writer's HUD: time, shot, speaker, file.
    pub fn hud_lines(&self) -> Vec<HudLine> {
        let p = &self.player;
        let state = p.sample();
        let mode = match p.mode() {
            Mode::Playing => "playing",
            Mode::Paused => "paused",
            Mode::Finished => "finished",
        };
        let shots = p.shot_starts();
        let index = shots.iter().take_while(|&&s| s <= p.time_ms()).count();
        let shot = match (index, state.shot) {
            (0, _) if shots.is_empty() => "shot: none named".to_string(),
            (0, _) => format!("shot: - / {}", shots.len()),
            (i, Some(name)) => format!("shot: {i} / {}  {name}", shots.len()),
            (i, None) => format!("shot: {i} / {}", shots.len()),
        };
        let speaker = match &state.subtitle {
            Some(s) => {
                let tag = match s.delivery {
                    Delivery::OnScreen => "",
                    Delivery::VoiceOver => " (V.O.)",
                    Delivery::OffScreen => " (O.S.)",
                };
                format!("speaker: {}{tag}", s.speaker_name)
            }
            None => "speaker: -".to_string(),
        };
        vec![
            HudLine::new(
                format!(
                    "{} / {} s  {mode}",
                    seconds(p.time_ms()),
                    seconds(p.duration_ms())
                ),
                HudTone::Normal,
            ),
            HudLine::new(shot, HudTone::Normal),
            HudLine::new(speaker, HudTone::Normal),
            HudLine::new(self.source.path().display().to_string(), HudTone::Dim),
            HudLine::new("J/L -/+ 1 s   , . -/+ 0.1 s   H hides this", HudTone::Dim),
        ]
    }
}

/// Milliseconds as `12.340` (seconds, three decimals, no floats).
pub fn seconds(ms: Ms) -> String {
    let ms = i64::from(ms);
    let sign = if ms < 0 { "-" } else { "" };
    let ms = ms.abs();
    format!("{sign}{}.{:03}", ms / 1000, ms % 1000)
}

/// A [`Preview`] that also watches its files and reloads them.
#[derive(Debug)]
pub struct LivePreview {
    preview: Preview,
    watcher: Option<SceneWatcher>,
}

impl LivePreview {
    /// `watch = false` is a plain preview that never reloads.
    pub fn new(preview: Preview, watch: bool) -> LivePreview {
        LivePreview::with_watcher(preview, watch.then(SceneWatcher::new))
    }

    /// With a watcher of your own timing (tests).
    pub fn with_watcher(preview: Preview, watcher: Option<SceneWatcher>) -> LivePreview {
        let mut live = LivePreview { preview, watcher };
        live.rewatch();
        live
    }

    pub fn preview(&self) -> &Preview {
        &self.preview
    }

    pub fn preview_mut(&mut self) -> &mut Preview {
        &mut self.preview
    }

    pub fn is_watching(&self) -> bool {
        self.watcher.is_some()
    }

    fn rewatch(&mut self) {
        if let Some(w) = &mut self.watcher {
            w.set_paths(self.preview.watched_files());
        }
    }

    /// Call every frame with a monotonic millisecond clock. When the scene file
    /// or a picture changed and the changes settled, load the file again and
    /// apply it. `None`: nothing was due.
    pub fn tick(&mut self, now_ms: u64) -> Option<Reload> {
        if !self.watcher.as_mut()?.tick(now_ms) {
            return None;
        }
        let result = self.preview.source().load(self.preview.assets());
        let file = self.preview.source().path().display().to_string();
        let notes = result.as_ref().map(|l| l.notes(&file)).unwrap_or_default();
        let applied = self.preview.apply(result);
        self.rewatch();
        Some(Reload { applied, notes })
    }
}

/// What one reload did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reload {
    pub applied: Applied,
    /// Warnings and missing files of the new scene, as terminal lines (empty
    /// when it was rejected: the errors are in [`Preview::errors`]).
    pub notes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::{RecordingSink, SoundEvent};

    fn scene_ron(name: &str, wait_ms: i32) -> String {
        format!(
            r##"#![enable(implicit_some)]
Scene(
    name: "{name}",
    characters: [
        (id: "a", name: "Anna", parts: [(name: "body", art: Rect(w: 10, h: 20, color: "#ff0000"))]),
    ],
    timeline: (tracks: [
        (name: "stage", events: [
            (at: 0, kind: Enter(character: "a", pos: (0, 0))),
            (at: 0, kind: CameraCut(shot: "first")),
            (at: 0, kind: Wait(duration_ms: {wait_ms})),
            (at: 2000, kind: CameraCut(shot: "second")),
        ]),
        (name: "audio", events: [
            (at: 0, kind: Music(note: "drums")),
            (at: 1000, kind: Dialogue(speaker: "a", text: "Hello.", duration_ms: 1000, voice: "v/a.wav")),
        ]),
    ]),
)"##
        )
    }

    fn source() -> Source {
        Source::Scene(PathBuf::from("scenes/test.ron"))
    }

    fn load(text: &str) -> LoadResult {
        source().load_text(text, Path::new("assets"))
    }

    fn preview_of(text: &str) -> Preview {
        Preview::new(source(), PathBuf::from("assets"), load(text), false)
    }

    fn step(p: &mut Preview, now: u64) -> Vec<SoundEvent> {
        let mut sink = RecordingSink::default();
        p.player_mut().update(now, &mut sink);
        sink.0
    }

    #[test]
    fn a_good_scene_starts_clean() {
        let p = preview_of(&scene_ron("one", 6000));
        assert!(p.is_loaded());
        assert!(!p.has_errors());
        assert_eq!(p.player().scene().name, "one");
        assert_eq!(p.reloads(), 0);
    }

    #[test]
    fn good_then_bad_keeps_the_old_scene_and_sets_the_error() {
        let mut p = preview_of(&scene_ron("one", 6000));
        step(&mut p, 0);
        p.player_mut().seek(1500);
        step(&mut p, 10);
        let t = p.player().time_ms();
        assert_eq!(t, 1510);

        // A syntax error: an unclosed paren, so RON reports line and column.
        let bad = scene_ron("two", 6000).replace("pos: (0, 0)", "pos: (0, 0");
        let applied = p.apply(load(&bad));
        assert_eq!(applied, Applied::Rejected);
        assert!(p.has_errors());
        assert_eq!(p.player().scene().name, "one", "the last good scene stays");
        assert_eq!(p.player().time_ms(), t, "and keeps playing where it was");
        let d = &p.errors()[0];
        assert_eq!(d.file, "scenes/test.ron");
        assert!(d.line.is_some() && d.column.is_some(), "{d}");
        assert!(d.to_string().starts_with("scenes/test.ron:"), "{d}");
        assert_eq!(p.reloads(), 0);

        // A scene that parses but is invalid: line, no column.
        let invalid =
            scene_ron("two", 6000).replace("Enter(character: \"a\"", "Enter(character: \"zed\"");
        assert_eq!(p.apply(load(&invalid)), Applied::Rejected);
        let d = &p.errors()[0];
        assert!(d.line.is_some() && d.column.is_none(), "{d}");
        assert!(d.message.contains("zed"), "{d}");
        assert_eq!(p.player().scene().name, "one");
    }

    #[test]
    fn bad_then_good_clears_the_error() {
        let mut p = preview_of(&scene_ron("one", 6000));
        p.apply(load("Scene(nope"));
        assert!(p.has_errors());
        let applied = p.apply(load(&scene_ron("two", 6000)));
        assert_eq!(applied, Applied::Reloaded { recovered: true });
        assert!(!p.has_errors());
        assert!(p.panels(&[]).is_empty(), "the overlay is gone");
        assert_eq!(p.player().scene().name, "two");
        assert_eq!(p.reloads(), 1);
        // A further good one is a plain reload.
        assert_eq!(
            p.apply(load(&scene_ron("three", 6000))),
            Applied::Reloaded { recovered: false }
        );
    }

    #[test]
    fn two_bad_loads_in_a_row_replace_the_error_list() {
        let mut p = preview_of(&scene_ron("one", 6000));
        p.apply(load("Scene(nope"));
        let first = p.errors().to_vec();
        p.apply(load("Scene(name: 5)"));
        assert_ne!(p.errors(), first.as_slice());
        assert_eq!(p.errors().len(), 1);
    }

    #[test]
    fn the_playhead_is_kept_and_clamped() {
        let mut p = preview_of(&scene_ron("one", 6000));
        step(&mut p, 0);
        p.player_mut().seek(3500);
        step(&mut p, 10);
        assert_eq!(p.player().time_ms(), 3510);

        // Longer: same time, still playing.
        p.apply(load(&scene_ron("two", 20_000)));
        assert_eq!(p.player().time_ms(), 3510);
        assert_eq!(p.player().duration_ms(), 20_000);
        assert!(p.player().is_playing());

        // Shorter than the playhead: clamped to the new end.
        p.apply(load(&scene_ron("three", 3000)));
        assert_eq!(p.player().duration_ms(), 3000);
        assert_eq!(p.player().time_ms(), 3000);
        assert!(p.player().is_finished());

        // Paused stays paused at the same time.
        let mut q = preview_of(&scene_ron("one", 6000));
        step(&mut q, 0);
        q.player_mut().seek(2500);
        q.player_mut().pause();
        step(&mut q, 10);
        q.apply(load(&scene_ron("two", 9000)));
        assert!(q.player().is_paused());
        assert_eq!(q.player().time_ms(), 2500);
    }

    #[test]
    fn music_and_voice_are_restored_after_a_reload_as_on_a_seek() {
        let mut p = preview_of(&scene_ron("one", 6000));
        step(&mut p, 0);
        p.player_mut().seek(1400);
        step(&mut p, 10);
        p.apply(load(&scene_ron("two", 6000)));
        let e = step(&mut p, 20);
        assert_eq!(e[0], SoundEvent::StopAll, "{e:?}");
        assert!(
            e.iter().any(|e| matches!(e, SoundEvent::Bed { .. })),
            "the music carries on: {e:?}"
        );
        assert!(
            e.iter()
                .any(|e| matches!(e, SoundEvent::Voice { offset_ms: 410, .. })),
            "the voice line resumes 410 ms in: {e:?}"
        );
    }

    #[test]
    fn the_window_can_open_on_a_broken_file_and_recovers() {
        let failed = load("Scene(").unwrap_err();
        let mut p = Preview::new(source(), PathBuf::from("assets"), Err(failed), true);
        assert!(!p.is_loaded());
        assert!(p.has_errors());
        let panels = p.panels(&[]);
        assert!(panels[0].lines[0].text.contains("No valid scene yet"));
        assert_eq!(
            p.apply(load(&scene_ron("one", 6000))),
            Applied::Reloaded { recovered: true }
        );
        assert!(p.is_loaded());
        assert_eq!(p.player().time_ms(), 0);
        assert!(p.player().is_playing(), "starts from the beginning");
        assert_eq!(p.player().scene().name, "one");
    }

    #[test]
    fn the_overlay_lists_errors_and_missing_pictures() {
        let mut p = preview_of(&scene_ron("one", 6000));
        let bad = scene_ron("two", 6000).replace("pos: (0, 0)", "pos: (0, 0");
        p.apply(load(&bad));
        let pics = vec![("art/a/head.png".to_string(), "file not found".to_string())];
        let panels = p.panels(&pics);
        assert_eq!(panels.len(), 2);
        assert_eq!(panels[0].tone, HudTone::Error);
        assert!(panels[0].lines[0].text.contains("last good version"));
        assert!(panels[0].lines[1].text.starts_with("scenes/test.ron:"));
        assert_eq!(panels[1].tone, HudTone::Warning);
        assert_eq!(panels[1].lines[1].text, "art/a/head.png: file not found");
        // Fix the file: the error panel goes, the pictures list stays while the
        // pictures are missing.
        p.apply(load(&scene_ron("two", 6000)));
        let panels = p.panels(&pics);
        assert_eq!(panels.len(), 1);
        assert_eq!(panels[0].tone, HudTone::Warning);
    }

    #[test]
    fn a_long_error_list_is_capped() {
        let mut p = preview_of(&scene_ron("one", 6000));
        let failure = LoadFailure {
            diagnostics: (0..30)
                .map(|i| Diagnostic {
                    file: "f.ron".into(),
                    line: Some(i),
                    column: None,
                    message: "bad".into(),
                })
                .collect(),
            unreadable: false,
        };
        p.apply(Err(failure));
        let lines = &p.panels(&[])[0].lines;
        assert_eq!(lines.len(), 1 + MAX_LISTED_ERRORS + 1);
        assert_eq!(lines.last().unwrap().text, "... and 18 more");
    }

    #[test]
    fn the_hud_shows_time_shot_speaker_and_file() {
        let mut p = Preview::new(
            source(),
            PathBuf::from("assets"),
            load(&scene_ron("one", 6000)),
            true,
        );
        step(&mut p, 0);
        p.player_mut().seek(2500);
        step(&mut p, 10);
        let hud = p.hud_lines();
        let text: Vec<&str> = hud.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(text[0], "2.510 / 6.000 s  playing", "{text:?}");
        assert_eq!(text[1], "shot: 2 / 2  second");
        assert_eq!(text[2], "speaker: -");
        assert_eq!(text[3], "scenes/test.ron");
        p.player_mut().seek(1200);
        step(&mut p, 20);
        assert_eq!(p.hud_lines()[2].text, "speaker: Anna");
        assert_eq!(p.hud_lines()[1].text, "shot: 1 / 2  first");
        // Toggle.
        assert_eq!(p.panels(&[]).len(), 1);
        p.toggle_hud();
        assert!(p.panels(&[]).is_empty());
    }

    #[test]
    fn stepping_moves_the_playhead_in_small_amounts() {
        let mut p = preview_of(&scene_ron("one", 6000));
        step(&mut p, 0);
        p.player_mut().pause();
        p.player_mut().seek(2000);
        p.step(1000);
        assert_eq!(p.player().time_ms(), 3000);
        p.step(-100);
        assert_eq!(p.player().time_ms(), 2900);
        p.step(-10_000);
        assert_eq!(p.player().time_ms(), 0);
        assert!(p.player().is_paused());
    }

    #[test]
    fn seconds_are_written_without_floats() {
        assert_eq!(seconds(0), "0.000");
        assert_eq!(seconds(1234), "1.234");
        assert_eq!(seconds(60_005), "60.005");
        assert_eq!(seconds(-1500), "-1.500");
    }

    #[test]
    fn pictures_are_listed_once_each() {
        let text = scene_ron("one", 6000).replace(
            "Rect(w: 10, h: 20, color: \"#ff0000\")",
            "Image(\"art/a/body.png\")",
        );
        let text = text.replace(
            "characters: [",
            "layers: [(name: \"bg\", art: Image(\"art/bg.png\"), z: -5), (name: \"bg2\", art: Image(\"art/bg.png\"), z: -4)],\n    characters: [",
        );
        let p = preview_of(&text);
        assert_eq!(p.pictures(), ["art/a/body.png", "art/bg.png"]);
        let files = p.watched_files();
        assert_eq!(files[0], PathBuf::from("scenes/test.ron"));
        assert_eq!(files[1], Path::new("assets").join("art/a/body.png"));
    }

    const FOUNTAIN: &str = "Title: Test\n\nINT. TENT - DAWN\n\nMAJOR HALVERSEN stands.\n\nMAJOR HALVERSEN\nCommander. Good to see you.\n";

    #[test]
    fn a_fountain_file_converts_on_the_fly() {
        let src = Source::Fountain(PathBuf::from("content/briefings/T.fountain"));
        let l = src.load_text(FOUNTAIN, Path::new("assets")).expect("loads");
        assert!(l.scene.scene.duration_ms() > 0);
        assert!(
            l.scene
                .scene
                .source
                .as_deref()
                .is_some_and(|s| s.ends_with("T.fountain"))
        );
    }

    #[test]
    fn a_problem_in_a_converted_scene_points_at_the_fountain_line() {
        let ron = "a\nb  // line 7\nc\n";
        assert_eq!(fountain_line_of(ron, 2), Some(7));
        assert_eq!(fountain_line_of(ron, 1), None);
        assert_eq!(fountain_line_of(ron, 0), None);
        assert_eq!(fountain_line_of(ron, 99), None);
        let (line, msg) = fountain_position(ron, Some(2), "boom");
        assert_eq!((line, msg.as_str()), (Some(7), "boom"));
        let (line, msg) = fountain_position(ron, Some(3), "boom");
        assert_eq!(line, None);
        assert!(msg.contains("generated scene, line 3"), "{msg}");
    }

    #[test]
    fn an_unreadable_file_is_a_failure_that_says_so() {
        let src = Source::Scene(PathBuf::from("/definitely/not/here.ron"));
        let f = src.load(Path::new("assets")).unwrap_err();
        assert!(f.unreadable);
        assert!(f.diagnostics[0].message.contains("cannot read"));
        assert!(f.diagnostics[0].file.contains("here.ron"));
    }
}
