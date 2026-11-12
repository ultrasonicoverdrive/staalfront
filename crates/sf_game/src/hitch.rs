//! The hitch log (WP1.16a, `docs/rules/hitch_log.md`): what a long frame in ordinary play was made of.
//!
//! The benchmark measures with vsync off and a scripted camera. A play with vsync on that stutters
//! (follow-up 35: frames of 0.6 to 1.6 s, 125 late snapshots) needs the same kind of numbers in the
//! real game, written down for every frame that was long, so that the next play hands back a file.
//!
//! # What is measured
//!
//! A frame is the time from the start of one `render` callback to the start of the next, the
//! number `FrameStats` and the benchmark call the frame time. It is made of the callback itself and the
//! **gap** after it, in which the event loop was not in our code (the window system, the driver, the
//! operating system). The callback is split into laps, in the order it runs: the snapshot hand-over
//! (convert, fog, ore), the UI and camera, the scene build, the instance upload, the audio, the draw
//! (split into acquiring the swap-chain image, recording, submitting, presenting) and the input
//! handlers. On Windows the CPU time of the main thread over the whole frame is read with
//! `GetThreadTimes`; a long frame with little CPU time means the thread was not running (another
//! program, the antivirus), a long frame with all of it means our code was busy. Elsewhere that field
//! is "not measured" (`null` in the file).
//!
//! Window events (resize, move, focus, occlusion, scale factor), the music tracks that started and the
//! time they took to decode, the number of sounds started, and the sim ticks over 20 ms that ran
//! while the frame was long are attached to the line.
//!
//! # What it costs
//!
//! When nothing is slow, a frame costs one comparison against [`crate::bench::HITCH_MS`] plus the
//! laps (one clock read each) and, on Windows, one `GetThreadTimes` call; a frame over the threshold
//! also counts. A line is built only for a frame over [`LOG_MS`]. The test
//! `the_bookkeeping_of_a_quiet_frame_is_small` measures the per-frame cost on the machine it runs on
//! (the best of five batches against a generous bound). That a quiet frame does no slow-path work is counted, not
//! timed, on synthetic frame times (the test `a_quiet_frame_does_no_slow_path_work`): no hitch counted, no
//! line, `slow_ticks` not asked, one thread-clock read per frame where it is measured.

use std::path::Path;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::bench::{HITCH_MS, WARMUP_FRAMES};

/// A frame over this many milliseconds writes a line to the log (the plan's number).
pub const LOG_MS: f64 = 50.0;

/// The most lines kept; frames over [`LOG_MS`] beyond it are counted but not described.
pub const MAX_LINES: usize = 2_000;

/// The most events remembered between two frames (a window drag sends one per pixel).
const MAX_EVENTS: usize = 24;

/// The parts of one frame's callback, in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct FrameParts {
    /// Taking the newest snapshot: converting it for the instancer, the fog upload, the ore.
    pub snapshot_ms: f64,
    /// The HUD, the menus and the camera.
    pub ui_ms: f64,
    /// Building the scene: instances, effects, overlays, bars, the cursor.
    pub scene_ms: f64,
    /// Uploading the instance buffers (a buffer that has to grow is allocated here).
    pub upload_ms: f64,
    /// `AudioSystem::update` (the music hand-over), the mood and starting the sounds.
    pub audio_ms: f64,
    /// Waiting for the swap chain to give an image (with vsync: the wait for the display).
    pub acquire_ms: f64,
    /// Recording the passes and the UI, until the submit.
    pub record_ms: f64,
    /// `queue.submit`.
    pub submit_ms: f64,
    /// `queue.present`.
    pub present_ms: f64,
    /// The input handlers after the draw: clicks, orders, the once-a-second line.
    pub input_ms: f64,
}

/// The lap a [`HitchLog::lap`] call closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lap {
    Snapshot,
    Ui,
    Scene,
    Upload,
    Audio,
    Input,
}

/// One frame over [`LOG_MS`], as it is written to the file.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HitchLine {
    /// The number of the frame that ran long (the first is 1).
    pub frame: u64,
    /// Seconds since the log began, at the start of the frame that followed.
    pub at_s: f64,
    /// The sim tick of the snapshot the long frame drew.
    pub tick: u64,
    /// From the start of the frame to the start of the next: callback and gap.
    pub frame_ms: f64,
    /// The time after the callback in which the event loop was elsewhere.
    pub gap_ms: f64,
    /// The callback itself.
    pub callback_ms: f64,
    /// The callback's laps.
    pub parts: FrameParts,
    /// CPU time of the main thread over `frame_ms`; `null` where it is not measured.
    pub cpu_ms: Option<f64>,
    /// Sounds started in the callback.
    pub sounds_started: u32,
    /// Window events and music events of the frame, in words.
    pub events: Vec<String>,
    /// Sim ticks over 20 ms that ran between this frame's snapshot and the next: `(tick, ms, think_ms)`;
    /// the third number is the time the computer opponents thought in that tick (WP9.1d), part of the
    /// second, 0 without an AI.
    pub sim_slow_ticks: Vec<(u64, f64, f64)>,
}

/// What the sim thread says about the whole play, handed in when the log is written.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SimTotals {
    pub ticks: u64,
    pub late_snapshots: u64,
    pub behind_passes: u64,
    pub clock_resets: u64,
    pub late_commands: u64,
    pub tick_ms_max: f64,
    pub slow_ticks: Vec<(u64, f64, f64)>,
    /// Thinks of computer opponents run in the play (WP9.1d), their mean and their longest, in ms; all 0
    /// in a match without one.
    pub thinks: u64,
    pub think_ms_mean: f64,
    pub think_ms_max: f64,
}

/// The counts of a play, for the summary line and the file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Totals {
    /// Frames measured (the first frame has nothing before it).
    pub frames: u64,
    /// Frames over [`HITCH_MS`].
    pub over_hitch: u64,
    /// Frames over [`LOG_MS`].
    pub over_log: u64,
    /// The same two, not counting the first [`WARMUP_FRAMES`] frames (start-up compiles and loads).
    pub frames_after_warmup: u64,
    pub over_hitch_after_warmup: u64,
    pub over_log_after_warmup: u64,
    pub longest_ms: f64,
    /// Frames over [`LOG_MS`] that were not described because the log was full.
    pub lines_dropped: u64,
    /// Frames whose swap-chain acquire gave something else than a normal image (WP1.16a): the frame was
    /// skipped or the surface wanted configuring again.
    pub acquire_suboptimal: u64,
    pub acquire_timeout: u64,
    pub acquire_occluded: u64,
    pub acquire_outdated: u64,
    pub acquire_lost: u64,
    pub acquire_other: u64,
}

/// A frame in which the renderer created or grew instance buffers (suspect 1 of WP1.16).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct BufferGrowth {
    pub frame: u64,
    pub tick: u64,
    /// Buffers created or grown in this frame's upload.
    pub added: u64,
    /// All buffers created or grown since the renderer began counting (the reservation at start included).
    pub total: u64,
    /// Bytes of all of them.
    pub bytes: u64,
    /// The whole upload lap of the frame.
    pub upload_ms: f64,
}

/// The most buffer growths kept.
const MAX_GROWTHS: usize = 500;

/// The file `--hitch-log` writes.
#[derive(Serialize)]
struct HitchFile<'a> {
    version: u32,
    hitch_threshold_ms: f64,
    log_threshold_ms: f64,
    warmup_frames: u32,
    /// `true` where the CPU time of the main thread is measured.
    cpu_measured: bool,
    /// Windows only: how many `GetThreadTimes` calls were made and what they cost in all, in ms.
    thread_times_calls: u64,
    thread_times_total_ms: f64,
    totals: Totals,
    sim: &'a SimTotals,
    buffer_growth: &'a [BufferGrowth],
    hitches: &'a [HitchLine],
}

/// The hitch log of a play.
#[derive(Debug)]
pub struct HitchLog {
    begun: Instant,
    last_start: Option<Instant>,
    callback_end: Instant,
    lap_at: Instant,
    parts: FrameParts,
    tick: u64,
    sounds: u32,
    callback_ms: f64,
    cpu: ThreadClock,
    cpu_at_start: Option<Duration>,
    events: Vec<String>,
    events_dropped: u32,
    totals: Totals,
    lines: Vec<HitchLine>,
    /// Frame intervals completed so far.
    frame_no: u64,
    /// The renderer's buffer count at the last frame; `None` before the first.
    buffers_seen: Option<u64>,
    growth: Vec<BufferGrowth>,
}

impl HitchLog {
    pub fn new(now: Instant) -> HitchLog {
        HitchLog {
            begun: now,
            last_start: None,
            callback_end: now,
            lap_at: now,
            parts: FrameParts::default(),
            tick: 0,
            sounds: 0,
            callback_ms: 0.0,
            cpu: ThreadClock::default(),
            cpu_at_start: None,
            events: Vec::new(),
            events_dropped: 0,
            totals: Totals::default(),
            lines: Vec::new(),
            frame_no: 0,
            buffers_seen: None,
            growth: Vec::new(),
        }
    }

    /// The top of a frame, at `now`. If the frame before this one (start to start) was over
    /// [`LOG_MS`] a line is built for it; `slow_ticks` is asked for the sim ticks over 20 ms that ran
    /// since the tick the long frame drew (given), up to the newest one, and is called only then.
    /// When nothing is slow this is one comparison, and on Windows one `GetThreadTimes` call.
    pub fn begin_frame(
        &mut self,
        now: Instant,
        slow_ticks: impl FnOnce(u64) -> Vec<(u64, f64, f64)>,
    ) {
        let cpu_now = self.cpu.now();
        if let Some(previous) = self.last_start.replace(now) {
            let ms = now.duration_since(previous).as_secs_f64() * 1e3;
            if ms > HITCH_MS {
                self.slow_frame(now, ms, cpu_now, slow_ticks);
            }
            self.totals.frames += 1;
            self.frame_no += 1;
        }
        // Events belong to the frame they fell in; a quiet frame forgets them.
        if !self.events.is_empty() {
            self.events.clear();
            self.events_dropped = 0;
        }
        self.cpu_at_start = cpu_now;
        self.lap_at = now;
        self.sounds = 0;
    }

    /// A frame over [`HITCH_MS`]: counted, and described if it is over [`LOG_MS`].
    #[cold]
    fn slow_frame(
        &mut self,
        now: Instant,
        ms: f64,
        cpu_now: Option<Duration>,
        slow_ticks: impl FnOnce(u64) -> Vec<(u64, f64, f64)>,
    ) {
        let warm = self.frame_no >= u64::from(WARMUP_FRAMES);
        self.totals.over_hitch += 1;
        self.totals.over_hitch_after_warmup += u64::from(warm);
        self.totals.longest_ms = self.totals.longest_ms.max(ms);
        if ms <= LOG_MS {
            self.events.clear();
            self.events_dropped = 0;
            return;
        }
        self.totals.over_log += 1;
        self.totals.over_log_after_warmup += u64::from(warm);
        if self.lines.len() >= MAX_LINES {
            self.totals.lines_dropped += 1;
            self.events.clear();
            self.events_dropped = 0;
            return;
        }
        let gap_ms = now.duration_since(self.callback_end).as_secs_f64() * 1e3;
        let mut events = std::mem::take(&mut self.events);
        if self.events_dropped > 0 {
            events.push(format!("... and {} more events", self.events_dropped));
            self.events_dropped = 0;
        }
        let cpu_ms = match (cpu_now, self.cpu_at_start) {
            (Some(now), Some(then)) => Some(now.saturating_sub(then).as_secs_f64() * 1e3),
            _ => None,
        };
        self.lines.push(HitchLine {
            frame: self.frame_no + 1,
            at_s: now.duration_since(self.begun).as_secs_f64(),
            tick: self.tick,
            frame_ms: ms,
            gap_ms,
            callback_ms: self.callback_ms,
            parts: self.parts,
            cpu_ms,
            sounds_started: self.sounds,
            events,
            sim_slow_ticks: slow_ticks(self.tick),
        });
    }

    /// The snapshot this frame draws is the one of `tick` (set once it has been taken).
    pub fn set_tick(&mut self, tick: u64) {
        self.tick = tick;
    }

    /// Closes a lap of the callback at `now`: the time since the last lap goes to its part.
    pub fn lap(&mut self, lap: Lap, now: Instant) {
        let ms = now.duration_since(self.lap_at).as_secs_f64() * 1e3;
        self.lap_at = now;
        let p = &mut self.parts;
        match lap {
            Lap::Snapshot => p.snapshot_ms = ms,
            Lap::Ui => p.ui_ms = ms,
            Lap::Scene => p.scene_ms = ms,
            Lap::Upload => p.upload_ms = ms,
            Lap::Audio => p.audio_ms = ms,
            Lap::Input => p.input_ms = ms,
        }
    }

    /// Closes the draw lap: the whole draw call took the time since the last lap, of which the swap
    /// chain wait, the submit and the present are given (the renderer measures them); the rest was
    /// recording the passes.
    pub fn lap_draw(
        &mut self,
        now: Instant,
        acquire: Duration,
        submit: Duration,
        present: Duration,
    ) {
        let total = now.duration_since(self.lap_at).as_secs_f64() * 1e3;
        self.lap_at = now;
        let ms = |d: Duration| d.as_secs_f64() * 1e3;
        let p = &mut self.parts;
        p.acquire_ms = ms(acquire);
        p.submit_ms = ms(submit);
        p.present_ms = ms(present);
        p.record_ms = (total - p.acquire_ms - p.submit_ms - p.present_ms).max(0.0);
    }

    /// The callback is over at `now`.
    pub fn end_frame(&mut self, now: Instant, sounds_started: u32) {
        self.callback_end = now;
        self.sounds = sounds_started;
        if let Some(start) = self.last_start {
            self.callback_ms = now.duration_since(start).as_secs_f64() * 1e3;
        }
    }

    /// Something happened that belongs on the line of the frame it falls in: a window event, a music
    /// track. At most a few dozen are kept between two frames.
    pub fn note(&mut self, text: impl FnOnce() -> String) {
        if self.events.len() < MAX_EVENTS {
            self.events.push(text());
        } else {
            self.events_dropped += 1;
        }
    }

    /// The renderer's instance buffer count after this frame's upload (`total` buffers of `bytes`
    /// bytes in all, created or grown since it began counting). One comparison per frame; when the count
    /// went up, the frame is remembered with the time its upload took.
    pub fn observe_buffers(&mut self, total: u64, bytes: u64, upload_ms: f64) {
        let before = self.buffers_seen.replace(total);
        if let Some(before) = before
            && total != before
        {
            self.buffers_grew(total.saturating_sub(before), total, bytes, upload_ms);
        }
    }

    #[cold]
    fn buffers_grew(&mut self, added: u64, total: u64, bytes: u64, upload_ms: f64) {
        self.note(|| {
            format!("instance buffers: {added} created or grown (now {total}, {bytes} bytes)")
        });
        if self.growth.len() < MAX_GROWTHS {
            self.growth.push(BufferGrowth {
                frame: self.frame_no + 1,
                tick: self.tick,
                added,
                total,
                bytes,
                upload_ms,
            });
        }
    }

    /// What the swap chain gave this frame, and how long the acquire waited. An ordinary image costs a
    /// comparison; anything else is counted and written on the frame's line (`acquire timeout after
    /// 1000 ms` is the best-fit explanation of a frame of a second on DX12).
    pub fn observe_acquire(&mut self, outcome: sf_render::AcquireOutcome, waited: Duration) {
        if outcome.is_trouble() {
            self.acquire_trouble(outcome, waited);
        }
    }

    #[cold]
    fn acquire_trouble(&mut self, outcome: sf_render::AcquireOutcome, waited: Duration) {
        use sf_render::AcquireOutcome as A;
        let t = &mut self.totals;
        match outcome {
            A::Suboptimal => t.acquire_suboptimal += 1,
            A::Timeout => t.acquire_timeout += 1,
            A::Occluded => t.acquire_occluded += 1,
            A::Outdated => t.acquire_outdated += 1,
            A::Lost => t.acquire_lost += 1,
            A::Other | A::Success => t.acquire_other += 1,
        }
        self.note(|| {
            format!(
                "swap chain acquire: {} after {:.0} ms",
                outcome.name(),
                waited.as_secs_f64() * 1e3
            )
        });
    }

    pub fn growth(&self) -> &[BufferGrowth] {
        &self.growth
    }

    pub fn totals(&self) -> Totals {
        Totals {
            frames_after_warmup: self.totals.frames.saturating_sub(u64::from(WARMUP_FRAMES)),
            ..self.totals
        }
    }

    pub fn lines(&self) -> &[HitchLine] {
        &self.lines
    }

    pub fn cpu_measured(&self) -> bool {
        self.cpu.measured()
    }

    /// The text of the exit summary: total frames, how many were over the benchmark's threshold and over
    /// the log's, with and without the start-up frames, and what the sim says.
    pub fn summary(&self, sim: &SimTotals) -> String {
        let t = &self.totals();
        let pct = |n: u64, of: u64| {
            if of == 0 {
                0.0
            } else {
                100.0 * n as f64 / of as f64
            }
        };
        format!(
            "hitches: {} frames, {} over {HITCH_MS} ms ({:.3} %), {} over {LOG_MS} ms; \
             without the first {WARMUP_FRAMES} frames: {} frames, {} over {HITCH_MS} ms ({:.3} %), {} over {LOG_MS} ms; \
             longest {:.1} ms; late snapshots {}, sim ticks over 20 ms {}; \
             swap chain acquires that gave no normal image: timeout {}, occluded {}, outdated {}, lost {}, suboptimal {}, other {}",
            t.frames,
            t.over_hitch,
            pct(t.over_hitch, t.frames),
            t.over_log,
            t.frames_after_warmup,
            t.over_hitch_after_warmup,
            pct(t.over_hitch_after_warmup, t.frames_after_warmup),
            t.over_log_after_warmup,
            t.longest_ms,
            sim.late_snapshots,
            sim.slow_ticks.len(),
            t.acquire_timeout,
            t.acquire_occluded,
            t.acquire_outdated,
            t.acquire_lost,
            t.acquire_suboptimal,
            t.acquire_other,
        )
    }

    /// The file `--hitch-log` asks for.
    pub fn to_json(&self, sim: &SimTotals) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&HitchFile {
            version: 1,
            hitch_threshold_ms: HITCH_MS,
            log_threshold_ms: LOG_MS,
            warmup_frames: WARMUP_FRAMES,
            cpu_measured: self.cpu.measured(),
            thread_times_calls: self.cpu.calls(),
            thread_times_total_ms: self.cpu.total_ms(),
            totals: self.totals(),
            sim,
            buffer_growth: &self.growth,
            hitches: &self.lines,
        })
    }

    pub fn write(&self, path: &Path, sim: &SimTotals) -> Result<(), String> {
        let json = self.to_json(sim).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }
}

/// The CPU time of the main thread (`sf_thread_cpu`), with the number of calls and what they cost.
#[derive(Debug, Default)]
pub struct ThreadClock {
    calls: u64,
    spent: Duration,
}

impl ThreadClock {
    /// The thread's CPU time (kernel and user) since it began, on Windows; `None` elsewhere.
    pub fn now(&mut self) -> Option<Duration> {
        if !sf_thread_cpu::MEASURED {
            return None;
        }
        let began = Instant::now();
        let cpu = sf_thread_cpu::thread_cpu_time();
        self.calls += 1;
        self.spent += began.elapsed();
        cpu
    }

    pub fn measured(&self) -> bool {
        sf_thread_cpu::MEASURED
    }

    pub fn calls(&self) -> u64 {
        self.calls
    }

    pub fn total_ms(&self) -> f64 {
        self.spent.as_secs_f64() * 1e3
    }
}

/// The window events that go on the line of the frame they fall in: the ones that move, resize, hide
/// or take the focus from the window, which on Windows can block the event loop. Cursor moves, keys
/// and the redraw request are not recorded.
pub fn window_note(event: &sf_engine::winit::event::WindowEvent) -> Option<String> {
    use sf_engine::winit::event::WindowEvent as W;
    match event {
        W::Resized(size) => Some(format!("resized {}x{}", size.width, size.height)),
        W::Moved(at) => Some(format!("moved to {},{}", at.x, at.y)),
        W::Focused(on) => Some(format!("focus {}", if *on { "gained" } else { "lost" })),
        W::Occluded(on) => Some(format!(
            "window {}",
            if *on {
                "occluded (hidden)"
            } else {
                "visible again"
            }
        )),
        W::ScaleFactorChanged { scale_factor, .. } => {
            Some(format!("scale factor now {scale_factor}"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(begun: Instant, ms: u64) -> Instant {
        begun + Duration::from_millis(ms)
    }

    fn no_ticks(_: u64) -> Vec<(u64, f64, f64)> {
        Vec::new()
    }

    /// Runs frames that start at the given millisecond marks; each callback takes 2 ms.
    fn run(marks: &[u64]) -> HitchLog {
        let begun = Instant::now();
        let mut log = HitchLog::new(begun);
        for (i, &m) in marks.iter().enumerate() {
            log.begin_frame(at(begun, m), no_ticks);
            log.set_tick(i as u64);
            log.lap(Lap::Scene, at(begun, m + 1));
            log.end_frame(at(begun, m + 2), 0);
        }
        log
    }

    #[test]
    fn a_quiet_run_counts_frames_and_writes_no_line() {
        let marks: Vec<u64> = (0..200).map(|i| i * 17).collect();
        let log = run(&marks);
        let t = log.totals();
        assert_eq!((t.frames, t.over_hitch, t.over_log), (199, 0, 0));
        assert!(log.lines().is_empty());
    }

    #[test]
    fn the_thresholds_are_the_benchmarks_and_the_plans() {
        assert_eq!(HITCH_MS, 25.0);
        assert_eq!(LOG_MS, 50.0);
        // Frames of 20, 26, 50, 51 and 200 ms: two are hitches without a line, then 51 and 200 ms.
        let log = run(&[0, 20, 46, 96, 147, 347, 364]);
        let t = log.totals();
        assert_eq!(t.frames, 6);
        assert_eq!((t.over_hitch, t.over_log), (4, 2), "{t:?}");
        let ms: Vec<u64> = log
            .lines()
            .iter()
            .map(|l| l.frame_ms.round() as u64)
            .collect();
        assert_eq!(ms, vec![51, 200]);
        assert!((t.longest_ms - 200.0).abs() < 1e-6);
    }

    /// The frame that ran long is the one before the frame that notices it: its tick, its laps and
    /// the gap after its callback.
    #[test]
    fn a_line_describes_the_frame_that_ran_long() {
        let begun = Instant::now();
        let mut log = HitchLog::new(begun);
        log.begin_frame(at(begun, 0), no_ticks);
        log.set_tick(10);
        log.lap(Lap::Snapshot, at(begun, 3));
        log.lap(Lap::Scene, at(begun, 8));
        log.lap_draw(
            at(begun, 28),
            Duration::from_millis(12),
            Duration::from_millis(1),
            Duration::from_millis(2),
        );
        log.end_frame(at(begun, 30), 4);
        log.note(|| "resized 1920x991".to_owned());
        log.begin_frame(at(begun, 130), |from| {
            assert_eq!(from, 10, "from the tick the long frame drew");
            vec![(10, 62.5, 3.0)]
        });
        let line = &log.lines()[0];
        assert_eq!((line.frame, line.tick, line.sounds_started), (1, 10, 4));
        assert!((line.frame_ms - 130.0).abs() < 1e-6);
        assert!((line.callback_ms - 30.0).abs() < 1e-6);
        assert!((line.gap_ms - 100.0).abs() < 1e-6);
        let p = line.parts;
        assert!((p.snapshot_ms - 3.0).abs() < 1e-6 && (p.scene_ms - 5.0).abs() < 1e-6);
        assert!((p.acquire_ms - 12.0).abs() < 1e-6 && (p.present_ms - 2.0).abs() < 1e-6);
        // 20 ms of drawing, of which 15 were waits: 5 ms of recording.
        assert!((p.record_ms - 5.0).abs() < 1e-6, "{p:?}");
        assert_eq!(line.events, vec!["resized 1920x991".to_owned()]);
        assert_eq!(line.sim_slow_ticks, vec![(10, 62.5, 3.0)]);
        // Off Windows the CPU time is not measured; the field says so.
        assert_eq!(line.cpu_ms.is_some(), cfg!(windows));
    }

    #[test]
    fn events_belong_to_one_line_and_are_bounded() {
        let begun = Instant::now();
        let mut log = HitchLog::new(begun);
        log.begin_frame(at(begun, 0), no_ticks);
        log.end_frame(at(begun, 1), 0);
        for i in 0..100 {
            log.note(|| format!("moved {i}"));
        }
        log.begin_frame(at(begun, 100), no_ticks);
        let first = &log.lines()[0].events;
        assert_eq!(first.len(), MAX_EVENTS + 1);
        assert_eq!(first.last().unwrap(), "... and 76 more events");
        log.end_frame(at(begun, 101), 0);
        log.begin_frame(at(begun, 200), no_ticks);
        assert!(
            log.lines()[1].events.is_empty(),
            "the events were handed over"
        );
    }

    #[test]
    fn start_up_frames_are_counted_apart() {
        let begun = Instant::now();
        let mut log = HitchLog::new(begun);
        let mut t = 0;
        // One hitch among the first frames, one after the warm-up.
        for i in 0..(WARMUP_FRAMES as u64 + 40) {
            log.begin_frame(at(begun, t), no_ticks);
            log.end_frame(at(begun, t + 1), 0);
            t += if i == 3 || i == WARMUP_FRAMES as u64 + 10 {
                90
            } else {
                17
            };
        }
        let c = log.totals();
        assert_eq!((c.over_hitch, c.over_hitch_after_warmup), (2, 1), "{c:?}");
        assert_eq!((c.over_log, c.over_log_after_warmup), (2, 1));
        assert!(c.frames_after_warmup < c.frames);
    }

    #[test]
    fn the_file_is_json_with_the_counts_and_the_lines() {
        let log = run(&[0, 17, 200, 217]);
        let sim = SimTotals {
            late_snapshots: 3,
            slow_ticks: vec![(7, 31.0, 4.5)],
            thinks: 40,
            think_ms_mean: 0.25,
            think_ms_max: 4.5,
            ..SimTotals::default()
        };
        let value: serde_json::Value = serde_json::from_str(&log.to_json(&sim).unwrap()).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["totals"]["frames"], 3);
        assert_eq!(value["totals"]["over_hitch"], 1);
        assert_eq!(value["sim"]["late_snapshots"], 3);
        // WP9.1d: the think time is in the file, per slow tick and as totals.
        assert_eq!(
            value["sim"]["slow_ticks"][0],
            serde_json::json!([7, 31.0, 4.5])
        );
        assert_eq!(value["sim"]["thinks"], 40);
        assert_eq!(value["sim"]["think_ms_mean"], 0.25);
        assert_eq!(value["sim"]["think_ms_max"], 4.5);
        assert_eq!(value["hitches"][0]["frame"], 2);
        assert!(value["hitches"][0]["cpu_ms"].is_null() || cfg!(windows));
        assert_eq!(value["cpu_measured"], cfg!(windows));
        let text = log.summary(&sim);
        assert!(
            text.contains("4 frames") || text.contains("3 frames"),
            "{text}"
        );
        assert!(text.contains("late snapshots 3"), "{text}");
    }

    #[test]
    fn only_the_events_that_move_or_hide_the_window_are_noted() {
        use sf_engine::winit::dpi::{PhysicalPosition, PhysicalSize};
        use sf_engine::winit::event::WindowEvent as W;
        let note = |e: W| window_note(&e);
        assert_eq!(
            note(W::Resized(PhysicalSize::new(1920, 991))).unwrap(),
            "resized 1920x991"
        );
        assert_eq!(
            note(W::Moved(PhysicalPosition::new(-8, 12))).unwrap(),
            "moved to -8,12"
        );
        assert_eq!(note(W::Focused(false)).unwrap(), "focus lost");
        assert_eq!(note(W::Occluded(true)).unwrap(), "window occluded (hidden)");
        assert_eq!(note(W::Destroyed), None);
        assert_eq!(note(W::RedrawRequested), None);
    }

    #[test]
    fn a_buffer_that_grew_is_remembered_with_its_upload_time() {
        let begun = Instant::now();
        let mut log = HitchLog::new(begun);
        log.begin_frame(at(begun, 0), no_ticks);
        log.set_tick(40);
        // The first observation is the reservation: not a growth.
        log.observe_buffers(4, 16_384, 0.2);
        log.begin_frame(at(begun, 17), no_ticks);
        log.observe_buffers(4, 16_384, 0.1);
        log.begin_frame(at(begun, 34), no_ticks);
        log.set_tick(41);
        log.observe_buffers(6, 24_576, 0.9);
        assert_eq!(
            log.growth(),
            &[BufferGrowth {
                frame: 3,
                tick: 41,
                added: 2,
                total: 6,
                bytes: 24_576,
                upload_ms: 0.9
            }]
        );
        log.begin_frame(at(begun, 51), no_ticks);
        log.observe_buffers(6, 24_576, 0.1);
        assert_eq!(log.growth().len(), 1);
    }

    /// A swap chain that gives no image goes on the line of its frame and into the totals; a normal
    /// image leaves no trace.
    #[test]
    fn an_acquire_that_timed_out_is_counted_and_written_on_the_line() {
        use sf_render::AcquireOutcome as A;
        let begun = Instant::now();
        let mut log = HitchLog::new(begun);
        log.begin_frame(at(begun, 0), no_ticks);
        log.observe_acquire(A::Success, Duration::from_millis(3));
        log.end_frame(at(begun, 1), 0);
        log.begin_frame(at(begun, 17), no_ticks);
        log.observe_acquire(A::Timeout, Duration::from_millis(1000));
        log.end_frame(at(begun, 18), 0);
        log.begin_frame(at(begun, 1100), no_ticks);
        let t = log.totals();
        assert_eq!(
            (t.acquire_timeout, t.acquire_occluded, t.acquire_other),
            (1, 0, 0)
        );
        let line = &log.lines()[0];
        assert_eq!(
            line.events,
            vec!["swap chain acquire: timeout after 1000 ms".to_owned()]
        );
        assert!(log.summary(&SimTotals::default()).contains("timeout 1,"));
    }

    /// What a quiet frame does, counted instead of timed: after 1,000 frames that all stay at or under
    /// [`HITCH_MS`] (synthetic times, so no pause of the machine can change the result) nothing was
    /// counted as a hitch, no line or event was built, `slow_ticks` was never asked, and the thread
    /// clock was read once per frame where it is measured and never elsewhere (so a second clock read
    /// is caught only on Windows, where `sf_thread_cpu::MEASURED` is true; on Linux the count is 0 by
    /// construction). The cost in time is
    /// `the_bookkeeping_of_a_quiet_frame_is_small`.
    #[test]
    fn a_quiet_frame_does_no_slow_path_work() {
        let begun = Instant::now();
        let mut log = HitchLog::new(begun);
        let n = 1_000u64;
        for i in 0..n {
            // Frames of 17 ms, every tenth of exactly HITCH_MS: not over it, so still quiet.
            let start = i * 17 + (i / 10) * 8;
            let ask =
                |_: u64| -> Vec<(u64, f64, f64)> { panic!("slow_ticks asked in a quiet frame") };
            log.begin_frame(at(begun, start), ask);
            log.lap(Lap::Scene, at(begun, start + 1));
            log.end_frame(at(begun, start + 2), 0);
        }
        let t = log.totals();
        assert_eq!(t.frames, n - 1);
        assert_eq!((t.over_hitch, t.over_log), (0, 0), "{t:?}");
        assert_eq!(t.longest_ms, 0.0, "the slow path was entered: {t:?}");
        assert!(log.lines().is_empty());
        assert!(log.events.is_empty());
        let reads = if sf_thread_cpu::MEASURED { n } else { 0 };
        assert_eq!(log.cpu.calls(), reads, "thread clock reads, one per frame");
        assert_eq!(log.cpu_measured(), sf_thread_cpu::MEASURED);
    }

    /// What a quiet frame costs in time: the bookkeeping of `begin_frame`, five laps and `end_frame`,
    /// with the clock read each of them takes in the runner, per frame, measured here (printed with
    /// `--nocapture`). The frame times are the real clock, so a pause of the machine can make one frame
    /// a hitch; whether a quiet frame stays out of the slow path is therefore not asserted here but in
    /// `a_quiet_frame_does_no_slow_path_work`. The number is the best of five batches (one pause spoils
    /// one batch, not all), and the bound is generous so that a busy machine does not fail it: it has
    /// to be small against a 17 ms frame, the number is what the PR quotes.
    #[test]
    fn the_bookkeeping_of_a_quiet_frame_is_small() {
        let mut log = HitchLog::new(Instant::now());
        let n = 40_000u32;
        let mut best = f64::MAX;
        for _ in 0..5 {
            let started = Instant::now();
            for _ in 0..n {
                log.begin_frame(Instant::now(), no_ticks);
                log.lap(Lap::Snapshot, Instant::now());
                log.lap(Lap::Ui, Instant::now());
                log.lap(Lap::Scene, Instant::now());
                log.lap(Lap::Upload, Instant::now());
                log.lap(Lap::Audio, Instant::now());
                log.end_frame(Instant::now(), 0);
            }
            best = best.min(started.elapsed().as_nanos() as f64 / f64::from(n));
        }
        println!(
            "hitch bookkeeping, quiet frame, 7 clock reads: {best:.0} ns (a frame is 16,700,000 ns)"
        );
        assert!(best < 20_000.0, "{best} ns per frame");
    }
}
