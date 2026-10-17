//! `play_scene <scene.ron> [options]` / `play_scene --fountain <file> [options]`
//!
//! Plays a cartoon scene in a window, for the writer and the artist to
//! preview it. With `--fountain` the briefing is converted on the fly (the
//! same conversion as `fountain2scene`, nothing is written) and played.
//!
//! Keys (the `Bindings::viewer()` preset): Space or a click pauses and
//! continues, Left and Right seek 2 s, N or Tab jump to the next shot, Home
//! or R restart, End skips to the end, Esc quits. A thin bar along the bottom
//! shows how far the scene is, with a tick at every shot; two bars in the top
//! right corner mean "paused". Sound cues are printed to the terminal as they
//! come up (there is no sound yet).
//!
//! `--watch` (WP12.6) reloads the scene when the file or one of its pictures is
//! saved: the playhead stays where it is, a broken save keeps the last good
//! scene on screen with the error (`file:line:col: message`) in an overlay and on
//! stderr, and the overlay goes when the file is fixed. H toggles a small HUD
//! (time, shot, speaker, file), J and L step -/+ 1 s, `,` and `.` step -/+ 0.1 s.
//! It is a flag of this viewer, not a second binary: the window, the keys, the
//! renderer and the way a scene is found are the same, and a writer has one
//! command to remember.
//!
//! Options: `--watch`, `--assets DIR` (default: found like the game does),
//! `--quit-after MS` (leave by itself, for smoke tests).
//! Exit status: 0 = played and quit, 1 = the scene is invalid or the window
//! could not be opened, 2 = bad usage or an unreadable file.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use sf_cutscene::preview::{Applied, LivePreview, Preview, Source, seconds};
use sf_cutscene::render::{CutsceneRenderer, Hud};
use sf_cutscene::{Bindings, Key, LoopAction, RecordingSink, SoundEvent};
use sf_engine::winit::event::MouseButton;
use sf_engine::winit::keyboard::KeyCode;
use sf_engine::{App, AssetRoot, Context, EngineConfig, Resized};
use sf_render::{GpuOptions, Renderer};

const USAGE: &str = "usage: play_scene <scene.ron> [--watch] [--assets DIR] [--quit-after MS]\n\
       play_scene --fountain <file.fountain> [--watch] [--assets DIR] [--quit-after MS]\n\
  --fountain FILE  convert a Fountain briefing on the fly and play it\n\
  --watch          reload when the file or one of its pictures is saved (keeps the\n\
                   playhead; errors show in an overlay, the last good scene stays)\n\
  --assets DIR     asset root (default: found like the game does)\n\
  --quit-after MS  quit by itself after MS milliseconds (smoke tests)\n\
keys: Space/click pause, Left/Right seek 2 s, J/L seek 1 s, , and . seek 0.1 s,\n\
      N/Tab next shot, Home/R restart, End skip to the end, H writer HUD, Esc quit";

struct Args {
    scene: Option<PathBuf>,
    fountain: Option<PathBuf>,
    assets: Option<PathBuf>,
    quit_after: Option<u64>,
    watch: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        scene: None,
        fountain: None,
        assets: None,
        quit_after: None,
        watch: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "--watch" => a.watch = true,
            "--fountain" => a.fountain = Some(args.next().ok_or("--fountain needs a file")?.into()),
            "--assets" => a.assets = Some(args.next().ok_or("--assets needs a directory")?.into()),
            "--quit-after" => {
                let v = args
                    .next()
                    .ok_or("--quit-after needs a number of milliseconds")?;
                a.quit_after = Some(
                    v.parse()
                        .map_err(|_| format!("--quit-after {v:?} is not a number"))?,
                );
            }
            _ if arg.starts_with("--") => return Err(format!("unknown option {arg}")),
            _ if a.scene.is_none() => a.scene = Some(arg.into()),
            _ => return Err(format!("unexpected argument {arg}")),
        }
    }
    match (&a.scene, &a.fountain) {
        (None, None) => Err("give a scene file or --fountain FILE".into()),
        (Some(_), Some(_)) => Err("give either a scene file or --fountain, not both".into()),
        _ => Ok(a),
    }
}

/// winit's keys the viewer listens to, with the player's name for each.
const KEYS: [(KeyCode, Key); 38] = [
    (KeyCode::Space, Key::Space),
    (KeyCode::Enter, Key::Enter),
    (KeyCode::NumpadEnter, Key::Enter),
    (KeyCode::Tab, Key::Tab),
    (KeyCode::ArrowLeft, Key::Left),
    (KeyCode::ArrowRight, Key::Right),
    (KeyCode::Home, Key::Home),
    (KeyCode::End, Key::End),
    (KeyCode::KeyA, Key::Char('a')),
    (KeyCode::KeyB, Key::Char('b')),
    (KeyCode::KeyC, Key::Char('c')),
    (KeyCode::KeyD, Key::Char('d')),
    (KeyCode::KeyE, Key::Char('e')),
    (KeyCode::KeyF, Key::Char('f')),
    (KeyCode::KeyG, Key::Char('g')),
    (KeyCode::KeyH, Key::Char('h')),
    (KeyCode::KeyI, Key::Char('i')),
    (KeyCode::KeyJ, Key::Char('j')),
    (KeyCode::KeyK, Key::Char('k')),
    (KeyCode::KeyL, Key::Char('l')),
    (KeyCode::KeyM, Key::Char('m')),
    (KeyCode::KeyN, Key::Char('n')),
    (KeyCode::KeyO, Key::Char('o')),
    (KeyCode::KeyP, Key::Char('p')),
    (KeyCode::KeyQ, Key::Char('q')),
    (KeyCode::KeyR, Key::Char('r')),
    (KeyCode::KeyS, Key::Char('s')),
    (KeyCode::KeyT, Key::Char('t')),
    (KeyCode::KeyU, Key::Char('u')),
    (KeyCode::KeyV, Key::Char('v')),
    (KeyCode::KeyW, Key::Char('w')),
    (KeyCode::KeyX, Key::Char('x')),
    (KeyCode::KeyY, Key::Char('y')),
    (KeyCode::KeyZ, Key::Char('z')),
    (KeyCode::Digit1, Key::Char('1')),
    (KeyCode::Digit2, Key::Char('2')),
    (KeyCode::Digit3, Key::Char('3')),
    (KeyCode::Digit4, Key::Char('4')),
];

/// The writer's fine-step keys: J and L one second back and forward, `,` and
/// `.` a tenth of a second.
const FINE_STEPS: [(KeyCode, i32); 4] = [
    (KeyCode::KeyJ, -1000),
    (KeyCode::KeyL, 1000),
    (KeyCode::Comma, -100),
    (KeyCode::Period, 100),
];

struct Viewer {
    live: LivePreview,
    bindings: Bindings,
    assets: PathBuf,
    quit_after: Option<u64>,
    started: Instant,
    renderer: Option<Renderer>,
    cutscenes: Option<CutsceneRenderer>,
    shown_title: String,
    /// Pictures whose failure was already printed.
    reported: std::collections::BTreeSet<String>,
}

impl Viewer {
    /// Print each picture that could not be loaded, once, with the reason, so
    /// a writer knows why a part shows as a magenta box.
    fn report_failed_pictures(&mut self) {
        let Some(cutscenes) = &self.cutscenes else {
            return;
        };
        for (path, why) in cutscenes.failed_pictures() {
            if self.reported.insert(path.to_string()) {
                eprintln!("play_scene: picture {path}: {why} - drawn as a magenta box");
            }
        }
    }

    /// Look for a saved file; on a reload read the pictures again and say on
    /// stderr what happened.
    fn check_for_reload(&mut self, now: u64) {
        let Some(reload) = self.live.tick(now) else {
            return;
        };
        let preview = self.live.preview();
        match reload.applied {
            Applied::Reloaded { recovered } => {
                for line in &reload.notes {
                    eprintln!("{line}");
                }
                let p = preview.player();
                eprintln!(
                    "play_scene: {}reloaded; {} s long, staying at {} s",
                    if recovered { "error fixed, " } else { "" },
                    seconds(p.duration_ms()),
                    seconds(p.time_ms()),
                );
            }
            Applied::Rejected => {
                eprintln!("play_scene: reload failed, still showing the last good version:");
                for d in preview.errors() {
                    eprintln!("{d}");
                }
            }
        }
        // After either outcome the pictures are read again: the last good
        // scene's pictures stay watched while the file is broken, so a picture
        // saved in that time shows up too.
        if let Some(cutscenes) = &mut self.cutscenes {
            cutscenes.forget_pictures();
            cutscenes.preload_pictures(self.live.preview().player().scene());
        }
        self.reported.clear();
    }
}

fn describe(e: &SoundEvent) -> String {
    let said = |note: &str, file: &Option<String>| match (note.is_empty(), file) {
        (false, Some(f)) => format!("{note} ({f})"),
        (false, None) => note.to_string(),
        (true, Some(f)) => f.clone(),
        (true, None) => "(nothing named)".to_string(),
    };
    match e {
        SoundEvent::Sfx { sound, note, .. } => format!("sfx: {}", said(note, sound)),
        SoundEvent::Voice {
            speaker,
            file,
            offset_ms,
        } => {
            format!("voice of {speaker}: {file} from {offset_ms} ms")
        }
        SoundEvent::Bed {
            bed,
            action,
            file,
            note,
            ..
        } => {
            let verb = match action {
                LoopAction::Play => "start",
                LoopAction::Stop => "stop",
            };
            format!("{bed:?}: {verb} {}", said(note, file))
        }
        SoundEvent::StopAll => "all sound stops".to_string(),
        SoundEvent::Pause => "sound paused".to_string(),
        SoundEvent::Resume => "sound continues".to_string(),
    }
}

impl App for Viewer {
    fn on_start(&mut self, ctx: &mut Context) {
        let size = ctx.size();
        let options = GpuOptions::window();
        match Renderer::for_window(ctx.window().clone(), size.width, size.height, &options) {
            Ok(renderer) => {
                eprintln!("play_scene: {}", renderer.adapter_summary());
                let gpu = renderer.gpu();
                let mut cutscenes = CutsceneRenderer::new(
                    &gpu.device,
                    &gpu.queue,
                    renderer.color_format(),
                    &self.assets,
                );
                cutscenes.preload_pictures(self.live.preview().player().scene());
                self.cutscenes = Some(cutscenes);
                self.renderer = Some(renderer);
                self.report_failed_pictures();
            }
            Err(e) => {
                eprintln!("play_scene: cannot open the renderer: {e}");
                ctx.request_exit();
            }
        }
    }

    fn fixed_update(&mut self, _ctx: &mut Context, _tick: u64) {}

    fn on_resize(&mut self, _ctx: &mut Context, size: Resized) {
        if let Some(r) = &mut self.renderer {
            r.resize(size.width, size.height);
        }
    }

    fn render(&mut self, ctx: &mut Context, _alpha: f32) {
        let now = self.started.elapsed().as_millis() as u64;
        if self.quit_after.is_some_and(|limit| now >= limit)
            || ctx.input().key_pressed(KeyCode::Escape)
        {
            ctx.request_exit();
            return;
        }
        self.check_for_reload(now);

        // Keys and clicks to the player.
        let mut presses: Vec<Key> = KEYS
            .iter()
            .filter(|(code, _)| ctx.input().key_pressed(*code))
            .map(|(_, key)| *key)
            .collect();
        if ctx.input().button_pressed(MouseButton::Left) {
            presses.push(Key::Click);
        }
        let preview = self.live.preview_mut();
        for key in presses {
            preview.player_mut().press(key, &self.bindings);
        }
        // The writer's keys: fine steps and the HUD.
        for (code, delta) in FINE_STEPS {
            if ctx.input().key_pressed(code) {
                preview.step(delta);
            }
        }
        if ctx.input().key_pressed(KeyCode::KeyH) {
            preview.toggle_hud();
        }

        let mut heard = RecordingSink::default();
        preview.player_mut().update(now, &mut heard);
        let player = preview.player();
        for e in &heard.0 {
            eprintln!(
                "[{:6.1} s] {}",
                f64::from(player.time_ms()) / 1000.0,
                describe(e)
            );
        }

        let title = format!(
            "{} - {:.1} / {:.1} s{}{}",
            player
                .scene()
                .title
                .as_deref()
                .unwrap_or(&player.scene().name),
            f64::from(player.time_ms()) / 1000.0,
            f64::from(player.duration_ms()) / 1000.0,
            if player.is_paused() { " (paused)" } else { "" },
            if preview.has_errors() {
                " - ERROR, see the overlay"
            } else {
                ""
            },
        );
        if title != self.shown_title {
            ctx.window().set_title(&title);
            self.shown_title = title;
        }

        let (Some(renderer), Some(cutscenes)) = (&mut self.renderer, &mut self.cutscenes) else {
            return;
        };
        let preview = self.live.preview();
        let player = preview.player();
        let duration = f64::from(player.duration_ms().max(1));
        let failed: Vec<(String, String)> = cutscenes
            .failed_pictures()
            .into_iter()
            .map(|(p, why)| (p.to_string(), why.to_string()))
            .collect();
        let hud = Hud {
            progress: (f64::from(player.time_ms()) / duration) as f32,
            shot_marks: player
                .shot_starts()
                .iter()
                .map(|s| (f64::from(*s) / duration) as f32)
                .collect(),
            paused: player.is_paused(),
            panels: preview.panels(&failed),
        };
        let state = player.sample();
        let scene = player.scene();
        if let Err(e) = renderer.render_with(|t| cutscenes.render(t, scene, &state, Some(&hud))) {
            eprintln!("play_scene: render failed: {e}");
            ctx.request_exit();
        }

        self.report_failed_pictures();
    }
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(m) => {
            eprintln!("play_scene: {m}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let assets = args
        .assets
        .clone()
        .unwrap_or_else(|| AssetRoot::resolve().root().to_path_buf());
    let source = match (&args.fountain, &args.scene) {
        (Some(f), _) => Source::Fountain(f.clone()),
        (None, Some(s)) => Source::Scene(s.clone()),
        (None, None) => unreachable!("parse_args needs one of them"),
    };
    let first = source.load(&assets);
    let file = source.path().display().to_string();
    match &first {
        Ok(l) => {
            for line in l.notes(&file) {
                eprintln!("{line}");
            }
        }
        Err(f) => {
            for d in &f.diagnostics {
                eprintln!("{d}");
            }
            // A file that is not there is a usage problem; a bad file is not.
            // Watching, a bad file is no reason to leave: the window opens
            // with the errors on it and loads the file when it is fixed.
            if f.unreadable {
                return ExitCode::from(2);
            }
            if !args.watch {
                return ExitCode::FAILURE;
            }
            eprintln!("play_scene: waiting for a valid {file} (the window shows the errors)");
        }
    }
    let preview = Preview::new(source, assets.clone(), first, args.watch);
    let player = preview.player();
    let title = player
        .scene()
        .title
        .clone()
        .unwrap_or_else(|| player.scene().name.clone());
    eprintln!(
        "play_scene: {title}, {:.1} s; Space pauses, Left/Right seek, N next shot, H HUD, Esc quits{}",
        f64::from(player.duration_ms()) / 1000.0,
        if args.watch {
            "; watching for saves"
        } else {
            ""
        },
    );
    let viewer = Viewer {
        live: LivePreview::new(preview, args.watch),
        bindings: Bindings::viewer(),
        assets,
        quit_after: args.quit_after,
        started: Instant::now(),
        renderer: None,
        cutscenes: None,
        shown_title: String::new(),
        reported: std::collections::BTreeSet::new(),
    };
    let config = EngineConfig {
        title: format!("Staalfront cutscene - {title}"),
        ..EngineConfig::default()
    };
    match sf_engine::run(config, viewer) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("play_scene: {e}");
            ExitCode::FAILURE
        }
    }
}
