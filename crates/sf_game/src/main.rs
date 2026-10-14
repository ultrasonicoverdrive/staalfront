//! The game binary: glues all crates together.
//!
//! WP1.1: a bare runner. Opens the window, counts sim ticks and frames and
//! prints the rates once per second. No rendering yet.

use std::time::{Duration, Instant};

use sf_engine::{Action, App, Context, EngineConfig, Resized};

struct Runner {
    started: Option<Instant>,
    report_at: Instant,
    ticks: u32,
    frames: u32,
    total_ticks: u64,
    exit_after: Option<Duration>,
}

impl Runner {
    fn new(exit_after: Option<Duration>) -> Self {
        let now = Instant::now();
        Self {
            started: None,
            report_at: now + Duration::from_secs(1),
            ticks: 0,
            frames: 0,
            total_ticks: 0,
            exit_after,
        }
    }
}

impl App for Runner {
    fn on_start(&mut self, ctx: &mut Context) {
        let now = Instant::now();
        self.started = Some(now);
        self.report_at = now + Duration::from_secs(1);
        let s = ctx.size();
        println!(
            "window {}x{} px, scale factor {}, assets {}",
            s.width,
            s.height,
            ctx.scale_factor(),
            ctx.assets().root().display()
        );
    }

    fn fixed_update(&mut self, _ctx: &mut Context, _tick: u64) {
        self.ticks += 1;
        self.total_ticks += 1;
    }

    fn render(&mut self, ctx: &mut Context, _alpha: f32) {
        self.frames += 1;
        let actions = ctx.actions();
        if actions.pressed(Action::ToggleFullscreen) {
            ctx.toggle_fullscreen();
            println!("fullscreen: {}", ctx.is_fullscreen());
        }
        if actions.pressed(Action::Quit) {
            println!("quit requested");
            ctx.request_exit();
        }

        let now = Instant::now();
        if let Some(limit) = self.exit_after
            && self.started.is_some_and(|s| now.duration_since(s) >= limit)
        {
            println!("--exit-after reached");
            ctx.request_exit();
        }
        if now >= self.report_at {
            let ft = ctx.frame_stats().summary();
            match ft {
                Some(f) => println!(
                    "{} fps, {} ticks/s (total {}), frame ms avg {:.2} p99 {:.2} max {:.2}",
                    self.frames,
                    self.ticks,
                    self.total_ticks,
                    f.avg.as_secs_f64() * 1e3,
                    f.p99.as_secs_f64() * 1e3,
                    f.max.as_secs_f64() * 1e3,
                ),
                None => println!("{} fps, {} ticks/s", self.frames, self.ticks),
            }
            self.frames = 0;
            self.ticks = 0;
            self.report_at += Duration::from_secs(1);
        }
    }

    fn on_resize(&mut self, _ctx: &mut Context, size: Resized) {
        println!(
            "resized {}x{} @ {}",
            size.width, size.height, size.scale_factor
        );
    }

    fn on_asset_changed(&mut self, _ctx: &mut Context, path: &std::path::Path) {
        println!("asset changed: {}", path.display());
    }

    fn on_exit(&mut self, _ctx: &mut Context) {
        println!("exit after {} ticks", self.total_ticks);
    }
}

/// `--exit-after <seconds>`: quit by itself (smoke tests).
fn exit_after_arg() -> Option<Duration> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--exit-after" {
            return args
                .next()?
                .parse::<f64>()
                .ok()
                .map(Duration::from_secs_f64);
        }
    }
    None
}

fn main() {
    if let Err(e) = sf_engine::run(EngineConfig::default(), Runner::new(exit_after_arg())) {
        eprintln!("sf_game: {e}");
        std::process::exit(1);
    }
}
