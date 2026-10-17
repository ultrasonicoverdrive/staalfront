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
//! Options: `--assets DIR` (default: found like the game does), `--quit-after
//! MS` (leave by itself, for smoke tests).
//! Exit status: 0 = played and quit, 1 = the scene is invalid or the window
//! could not be opened, 2 = bad usage or an unreadable file.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use sf_cutscene::fountain::{ImportOptions, convert, scene_name};
use sf_cutscene::render::{CutsceneRenderer, Hud};
use sf_cutscene::{
    Bindings, Key, LoopAction, Player, RecordingSink, SoundEvent, load_file, load_str,
};
use sf_engine::winit::event::MouseButton;
use sf_engine::winit::keyboard::KeyCode;
use sf_engine::{App, AssetRoot, Context, EngineConfig, Resized};
use sf_render::{GpuOptions, Renderer};

const USAGE: &str = "usage: play_scene <scene.ron> [--assets DIR] [--quit-after MS]\n\
       play_scene --fountain <file.fountain> [--assets DIR] [--quit-after MS]\n\
  --fountain FILE  convert a Fountain briefing on the fly and play it\n\
  --assets DIR     asset root (default: found like the game does)\n\
  --quit-after MS  quit by itself after MS milliseconds (smoke tests)\n\
keys: Space/click pause, Left/Right seek 2 s, N/Tab next shot, Home/R restart,\n\
      End skip to the end, Esc quit";

struct Args {
    scene: Option<PathBuf>,
    fountain: Option<PathBuf>,
    assets: Option<PathBuf>,
    quit_after: Option<u64>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        scene: None,
        fountain: None,
        assets: None,
        quit_after: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
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

/// Load the scene and print what `scene_check` would. `Err` carries the exit code.
fn load(args: &Args, assets: &Path) -> Result<sf_cutscene::LoadedScene, ExitCode> {
    let (name, loaded) = if let Some(path) = &args.fountain {
        let text = std::fs::read_to_string(path).map_err(|e| {
            eprintln!("play_scene: {}: cannot read: {e}", path.display());
            ExitCode::from(2)
        })?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("scene");
        let import = convert(
            &text,
            &ImportOptions {
                name: scene_name(stem),
                source: Some(path.display().to_string().replace('\\', "/")),
            },
        );
        for w in &import.warnings {
            eprintln!("{}:{}: warning: {}", path.display(), w.line, w.message);
        }
        (
            path.display().to_string(),
            load_str(&import.ron, Some(assets)),
        )
    } else if let Some(path) = &args.scene {
        (path.display().to_string(), load_file(path, Some(assets)))
    } else {
        unreachable!("parse_args needs one of them")
    };
    match loaded {
        Ok(l) => {
            for w in &l.warnings {
                match w.line {
                    Some(n) => eprintln!("{name}:{n}: warning: {}", w.message),
                    None => eprintln!("{name}: warning: {}", w.message),
                }
            }
            for m in &l.missing_assets {
                eprintln!(
                    "{name}: note: missing {} ({}) - drawn as a placeholder or silent",
                    m.file, m.used_by
                );
            }
            Ok(l)
        }
        Err(e) => {
            eprintln!("{}", e.clone().in_file(&name));
            // An unreadable file is a usage problem; an invalid scene is not.
            Err(if matches!(e, sf_cutscene::SceneError::Io { .. }) {
                ExitCode::from(2)
            } else {
                ExitCode::FAILURE
            })
        }
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

struct Viewer {
    player: Player,
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
                cutscenes.preload_pictures(self.player.scene());
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
        // Keys and clicks to the player.
        let mut presses: Vec<Key> = KEYS
            .iter()
            .filter(|(code, _)| ctx.input().key_pressed(*code))
            .map(|(_, key)| *key)
            .collect();
        if ctx.input().button_pressed(MouseButton::Left) {
            presses.push(Key::Click);
        }
        for key in presses {
            self.player.press(key, &self.bindings);
        }

        let mut heard = RecordingSink::default();
        self.player.update(now, &mut heard);
        for e in &heard.0 {
            eprintln!(
                "[{:6.1} s] {}",
                f64::from(self.player.time_ms()) / 1000.0,
                describe(e)
            );
        }

        let title = format!(
            "{} - {:.1} / {:.1} s{}",
            self.player
                .scene()
                .title
                .as_deref()
                .unwrap_or(&self.player.scene().name),
            f64::from(self.player.time_ms()) / 1000.0,
            f64::from(self.player.duration_ms()) / 1000.0,
            if self.player.is_paused() {
                " (paused)"
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
        let duration = f64::from(self.player.duration_ms().max(1));
        let hud = Hud {
            progress: (f64::from(self.player.time_ms()) / duration) as f32,
            shot_marks: self
                .player
                .shot_starts()
                .iter()
                .map(|s| (f64::from(*s) / duration) as f32)
                .collect(),
            paused: self.player.is_paused(),
        };
        let state = self.player.sample();
        let scene = self.player.scene();
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
    let loaded = match load(&args, &assets) {
        Ok(l) => l,
        Err(code) => return code,
    };
    let title = loaded
        .scene
        .title
        .clone()
        .unwrap_or_else(|| loaded.scene.name.clone());
    eprintln!(
        "play_scene: {title}, {:.1} s; Space pauses, Left/Right seek, N next shot, Esc quits",
        f64::from(loaded.scene.duration_ms()) / 1000.0
    );
    let viewer = Viewer {
        player: Player::new(loaded.scene),
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
