//! The benchmark of milestone M0 (`sf_demo --bench`, WP1.14).
//!
//! A fixed camera flight from close-up to the whole map and back, the frame
//! times of every frame of it, and a JSON file with the result. Three parts:
//!
//! * [`Flight`] is the camera path: a table of segments, each with a start and
//!   end zoom and focus, eased with a smoothstep. The camera state is a pure
//!   function of the progress `0..=1`, so every run flies the same path
//!   whatever the frame rate. The progress follows the clock (about 75 s) or,
//!   for the CI smoke test, the frame count.
//! * [`Recorder`] collects one [`FrameSample`] per frame and the GPU times
//!   that arrive later.
//! * [`Report`] is `bench.json`: percentiles over the whole flight and per
//!   segment ("phase"), units in view, draw calls and triangles, the slowest
//!   frames with their breakdown, and the sim checksums.
//!
//! The percentile is the nearest rank, like `sf_engine::FrameStats`.

use serde::Serialize;

/// Frame time that the M0 criterion asks the 99th percentile to stay under
/// (60 fps).
pub const P99_TARGET_MS: f64 = 1000.0 / 60.0;

/// Frames longer than this count as a hitch in the report.
pub const HITCH_MS: f64 = 25.0;

/// Frames at the start pose that are flown but not measured (the first
/// frames are shader, driver and buffer warm-up, not rendering speed).
pub const WARMUP_FRAMES: u32 = 90;

/// How many of the slowest frames the report lists.
const WORST_FRAMES: usize = 12;

/// One stretch of the flight.
#[derive(Clone, Copy, Debug)]
pub struct Segment {
    pub name: &'static str,
    /// Share of the flight; the weights are normalised.
    pub weight: f64,
    /// Zoom `0` (closest) to `1` (whole map) at the start and the end.
    pub zoom: (f64, f64),
    /// Focus in metres at the start and the end.
    pub focus: ((f64, f64), (f64, f64)),
}

/// The flight: close-up over the base, a slow glide out to the whole map, a
/// hold, a glide back in, a quick flick out and back (a fast wheel burst,
/// where the hitches of follow-up 14 showed), and a low pass across the map.
/// Positions are for the 1,024 m map; the base is at its centre.
pub const SEGMENTS: [Segment; 9] = [
    Segment {
        name: "close_up",
        weight: 8.0,
        zoom: (0.0, 0.0),
        focus: ((500.0, 500.0), (530.0, 520.0)),
    },
    Segment {
        name: "glide_out",
        weight: 22.0,
        zoom: (0.0, 1.0),
        focus: ((530.0, 520.0), (512.0, 512.0)),
    },
    Segment {
        name: "overview",
        weight: 12.0,
        zoom: (1.0, 1.0),
        focus: ((512.0, 512.0), (512.0, 512.0)),
    },
    Segment {
        name: "glide_in",
        weight: 22.0,
        zoom: (1.0, 0.15),
        focus: ((512.0, 512.0), (700.0, 380.0)),
    },
    Segment {
        name: "mid_hold",
        weight: 6.0,
        zoom: (0.15, 0.15),
        focus: ((700.0, 380.0), (690.0, 400.0)),
    },
    Segment {
        name: "flick_out",
        weight: 4.0,
        zoom: (0.15, 1.0),
        focus: ((690.0, 400.0), (512.0, 512.0)),
    },
    Segment {
        name: "overview_again",
        weight: 3.0,
        zoom: (1.0, 1.0),
        focus: ((512.0, 512.0), (512.0, 512.0)),
    },
    Segment {
        name: "flick_in",
        weight: 4.0,
        zoom: (1.0, 0.0),
        focus: ((512.0, 512.0), (300.0, 650.0)),
    },
    Segment {
        name: "low_pass",
        weight: 19.0,
        zoom: (0.0, 0.02),
        focus: ((300.0, 650.0), (820.0, 720.0)),
    },
];

/// Where the camera is at one moment of the flight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub zoom: f64,
    pub focus: (f64, f64),
    /// Index into [`SEGMENTS`].
    pub segment: usize,
}

fn smoothstep(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// The camera at `progress` (`0..=1`, clamped) of the flight over a map of
/// `map_metres` (the table is written for 1,024 m and scaled to the map).
pub fn pose_at(progress: f64, map_metres: (f64, f64)) -> Pose {
    let total: f64 = SEGMENTS.iter().map(|s| s.weight).sum();
    let mut left = progress.clamp(0.0, 1.0) * total;
    for (i, s) in SEGMENTS.iter().enumerate() {
        if left <= s.weight || i == SEGMENTS.len() - 1 {
            let t = smoothstep((left / s.weight).clamp(0.0, 1.0));
            let scale = (map_metres.0 / 1024.0, map_metres.1 / 1024.0);
            return Pose {
                zoom: lerp(s.zoom.0, s.zoom.1, t),
                focus: (
                    lerp(s.focus.0.0, s.focus.1.0, t) * scale.0,
                    lerp(s.focus.0.1, s.focus.1.1, t) * scale.1,
                ),
                segment: i,
            };
        }
        left -= s.weight;
    }
    unreachable!("the last segment always returns")
}

/// What was measured about one frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameSample {
    /// Wall time since the previous frame started.
    pub frame_ms: f64,
    /// Time the main thread worked: the whole frame callback, without waiting
    /// for the display. Since WP1.15 the sim runs on its own thread and is not
    /// in it (before, the ticks run since the last frame were added).
    pub cpu_ms: f64,
    /// Building the instances (culling, levels of detail, icons, picking).
    pub build_ms: f64,
    /// Uploading the instance buffers.
    pub upload_ms: f64,
    /// Recording and submitting the frame and presenting it.
    pub render_ms: f64,
    /// Time the sim thread spent in the ticks that finished since the last
    /// frame. It runs in parallel with the frame; not part of `cpu_ms`.
    pub sim_ms: f64,
    pub units_in_view: u32,
    pub draw_calls: u32,
    pub triangles: u32,
    /// Instance buffers created or grown during this frame.
    pub buffer_allocs: u32,
    pub segment: u8,
}

/// Collects the samples of one run.
#[derive(Default)]
pub struct Recorder {
    pub frames: Vec<FrameSample>,
    /// GPU frame times in the order they finished, each with the index (in
    /// `frames`) of the frame it measured. The result arrives a few frames
    /// after the frame was submitted, so this is not the frame being
    /// recorded when it arrived.
    pub gpu: Vec<(usize, f64)>,
    /// Checksums of the sim: `(ticks run, checksum)` every 100 ticks.
    pub checksums: Vec<(u64, u64)>,
}

impl Recorder {
    pub fn push(&mut self, sample: FrameSample) {
        self.frames.push(sample);
    }

    /// Add GPU times as `(frame number, ms)`. The frame number counts every
    /// frame of the run from 0, warm-up included; the first `warmup` frames
    /// are not measured, so their times are dropped and the others are
    /// credited to `frames[number - warmup]`, the frame they were submitted
    /// in, however late they arrive.
    pub fn push_gpu(&mut self, times: impl IntoIterator<Item = (u64, f64)>, warmup: u32) {
        for (number, ms) in times {
            if let Some(at) = number.checked_sub(u64::from(warmup)) {
                self.gpu.push((at as usize, ms));
            }
        }
    }
}

/// The statistics of a list of milliseconds.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Stats {
    pub samples: usize,
    pub avg: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

/// Nearest-rank percentile of a sorted list (`p` in `1..=100`).
fn rank(sorted: &[f64], p: usize) -> f64 {
    sorted[(sorted.len() * p).div_ceil(100).max(1) - 1]
}

impl Stats {
    /// `None` for an empty list.
    pub fn of(values: &[f64]) -> Option<Stats> {
        if values.is_empty() {
            return None;
        }
        let mut v = values.to_vec();
        v.sort_by(f64::total_cmp);
        Some(Stats {
            samples: v.len(),
            avg: round3(v.iter().sum::<f64>() / v.len() as f64),
            p50: round3(rank(&v, 50)),
            p95: round3(rank(&v, 95)),
            p99: round3(rank(&v, 99)),
            max: round3(v[v.len() - 1]),
        })
    }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Average and largest value of a count per frame.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Counts {
    pub avg: f64,
    pub max: u32,
}

impl Counts {
    fn of(values: impl Iterator<Item = u32> + Clone) -> Counts {
        let n = values.clone().count().max(1);
        Counts {
            avg: round3(values.clone().map(f64::from).sum::<f64>() / n as f64),
            max: values.max().unwrap_or(0),
        }
    }
}

/// `bench.json`.
#[derive(Serialize)]
pub struct Report {
    pub format: u32,
    pub tool: &'static str,
    pub version: &'static str,
    pub build: &'static str,
    pub host: Host,
    pub display: Display,
    pub present: Present,
    pub scene: Scene,
    pub flight: FlightInfo,
    pub frames: FramesInfo,
    pub frame_ms: Option<Stats>,
    pub cpu_ms: Option<Stats>,
    pub gpu_ms: GpuInfo,
    pub criterion: Criterion,
    pub phases: Vec<Phase>,
    pub hitches: Hitches,
    pub sim: SimInfo,
}

#[derive(Serialize)]
pub struct Host {
    pub os: &'static str,
    pub arch: &'static str,
    pub adapter: String,
    pub backend_env: Option<String>,
}

#[derive(Serialize)]
pub struct Display {
    /// What was asked for, e.g. `1920x1080 fullscreen`.
    pub requested: String,
    /// The size the frames were really rendered at.
    pub actual: (u32, u32),
    /// `windowed`, `borderless` or `exclusive`.
    pub screen_mode: String,
    pub monitor: Option<String>,
    pub monitor_size: Option<(u32, u32)>,
    pub refresh_hz: Option<f64>,
    pub note: Option<String>,
}

#[derive(Serialize)]
pub struct Present {
    pub mode: String,
    pub vsync: bool,
    pub requested: &'static str,
    pub note: Option<String>,
}

#[derive(Serialize)]
pub struct Scene {
    pub units: u32,
    pub buildings: u32,
    pub models: Option<String>,
    pub map_cells: (u32, u32),
    pub map_metres: (u32, u32),
    pub metres_per_cell: u32,
    pub instance_buffers_reserved: bool,
}

#[derive(Serialize)]
pub struct FlightInfo {
    /// `time` (the flight follows the clock) or `frames` (CI smoke test).
    pub progress_by: &'static str,
    pub seconds: f64,
    pub frames_limit: Option<u32>,
    pub warmup_frames: u32,
    pub segments: Vec<String>,
}

#[derive(Serialize)]
pub struct FramesInfo {
    pub count: usize,
    pub seconds: f64,
    pub avg_fps: f64,
}

#[derive(Serialize)]
pub struct GpuInfo {
    pub available: bool,
    /// `n/a` reason when it is not.
    pub note: Option<String>,
    pub stats: Option<Stats>,
}

#[derive(Serialize)]
pub struct Criterion {
    pub name: &'static str,
    pub p99_target_ms: f64,
    pub p99_ms: Option<f64>,
    pub met: Option<bool>,
    /// Why `met` should not be read as an answer for the reference hardware.
    pub caveats: Vec<String>,
}

#[derive(Serialize)]
pub struct Phase {
    pub name: &'static str,
    pub frames: usize,
    pub frame_ms: Option<Stats>,
    pub cpu_ms: Option<Stats>,
    pub gpu_ms: Option<Stats>,
    pub units_in_view: Counts,
    pub draw_calls: Counts,
    pub triangles: Counts,
    pub buffer_allocs: u32,
}

#[derive(Serialize)]
pub struct Hitches {
    pub threshold_ms: f64,
    pub count: usize,
    pub worst: Vec<Hitch>,
}

#[derive(Serialize)]
pub struct Hitch {
    pub frame: usize,
    pub phase: &'static str,
    pub frame_ms: f64,
    pub cpu_ms: f64,
    pub sim_ms: f64,
    pub build_ms: f64,
    pub upload_ms: f64,
    pub render_ms: f64,
    pub buffer_allocs: u32,
}

#[derive(Serialize)]
pub struct SimInfo {
    /// The sim runs on its own thread (WP1.15): `frame_ms` and `cpu_ms` do not
    /// contain it, and numbers from before that change are not comparable.
    pub thread: bool,
    pub note: &'static str,
    /// How long each sim tick took on the sim thread during the measured
    /// flight (the budget is 50 ms), including an artificial `--sim-delay-ms`.
    pub tick_ms: Option<Stats>,
    /// Passes of the sim thread that ended behind its 20 Hz clock.
    pub behind_passes: u64,
    /// Times the sim gave up catching up and restarted its clock.
    pub clock_resets: u64,
    /// Commands that arrived for a tick that had already run.
    pub late_commands: u64,
    /// Snapshots published after the picture needed them (a stall).
    pub late_publishes: u64,
    /// `--sim-delay-ms`: the artificial time added to every tick.
    pub extra_tick_ms: f64,
    pub ticks: u64,
    pub final_checksum: Option<String>,
    /// After every 100th tick (`tick` = ticks run). Compare two machines at
    /// equal ticks: how many ticks a flight takes depends on the clock, the
    /// checksum after N ticks does not.
    pub checksums: Vec<TickChecksum>,
}

#[derive(Serialize)]
pub struct TickChecksum {
    pub tick: u64,
    pub checksum: String,
}

/// Format a checksum the way the headless runner does.
pub fn hex(v: u64) -> String {
    format!("0x{v:016x}")
}

/// What the sim thread did during the measured flight.
#[derive(Clone, Debug, Default)]
pub struct SimRun {
    /// The time of each tick, milliseconds.
    pub tick_ms: Vec<f64>,
    pub behind_passes: u64,
    pub clock_resets: u64,
    pub late_commands: u64,
    pub late_publishes: u64,
    pub extra_tick_ms: f64,
}

/// What the report needs from the run besides the samples.
pub struct RunFacts {
    pub host: Host,
    pub display: Display,
    pub present: Present,
    pub scene: Scene,
    pub flight: FlightInfo,
    pub gpu_note: Option<String>,
    pub gpu_enabled: bool,
    pub software_adapter: bool,
    pub ticks: u64,
    pub sim: SimRun,
}

impl Report {
    pub fn build(rec: &Recorder, facts: RunFacts) -> Report {
        let col = |f: fn(&FrameSample) -> f64| -> Vec<f64> { rec.frames.iter().map(f).collect() };
        let frame_ms = Stats::of(&col(|s| s.frame_ms));
        let cpu_ms = Stats::of(&col(|s| s.cpu_ms));
        let gpu_all: Vec<f64> = rec.gpu.iter().map(|g| g.1).collect();
        let seconds = rec.frames.iter().map(|s| s.frame_ms).sum::<f64>() / 1000.0;

        let phases = SEGMENTS
            .iter()
            .enumerate()
            .map(|(i, seg)| {
                let idx: Vec<usize> = (0..rec.frames.len())
                    .filter(|&f| usize::from(rec.frames[f].segment) == i)
                    .collect();
                let of = |f: fn(&FrameSample) -> f64| -> Vec<f64> {
                    idx.iter().map(|&k| f(&rec.frames[k])).collect()
                };
                let gpu: Vec<f64> = rec
                    .gpu
                    .iter()
                    .filter(|(at, _)| idx.binary_search(at).is_ok())
                    .map(|g| g.1)
                    .collect();
                let frames = idx.iter().map(|&k| &rec.frames[k]);
                Phase {
                    name: seg.name,
                    frames: idx.len(),
                    frame_ms: Stats::of(&of(|s| s.frame_ms)),
                    cpu_ms: Stats::of(&of(|s| s.cpu_ms)),
                    gpu_ms: Stats::of(&gpu),
                    units_in_view: Counts::of(frames.clone().map(|s| s.units_in_view)),
                    draw_calls: Counts::of(frames.clone().map(|s| s.draw_calls)),
                    triangles: Counts::of(frames.clone().map(|s| s.triangles)),
                    buffer_allocs: frames.map(|s| s.buffer_allocs).sum(),
                }
            })
            .collect();

        let mut order: Vec<usize> = (0..rec.frames.len()).collect();
        order.sort_by(|&a, &b| rec.frames[b].frame_ms.total_cmp(&rec.frames[a].frame_ms));
        let hitch_count = rec.frames.iter().filter(|s| s.frame_ms > HITCH_MS).count();
        let worst = order
            .iter()
            .take(WORST_FRAMES)
            .map(|&k| {
                let s = &rec.frames[k];
                Hitch {
                    frame: k,
                    phase: SEGMENTS[usize::from(s.segment)].name,
                    frame_ms: round3(s.frame_ms),
                    cpu_ms: round3(s.cpu_ms),
                    sim_ms: round3(s.sim_ms),
                    build_ms: round3(s.build_ms),
                    upload_ms: round3(s.upload_ms),
                    render_ms: round3(s.render_ms),
                    buffer_allocs: s.buffer_allocs,
                }
            })
            .collect();

        let mut caveats = Vec::new();
        if facts.software_adapter {
            caveats.push(
                "software adapter (lavapipe / WARP): frame times say nothing about a real GPU"
                    .to_owned(),
            );
        }
        if facts.present.vsync {
            caveats.push(
                "vsync is on: frame times are paced by the display, p99 is not meaningful"
                    .to_owned(),
            );
        }
        if let Some(note) = &facts.present.note {
            caveats.push(format!("present mode: {note}"));
        }
        if facts.flight.progress_by == "frames" {
            caveats.push("short smoke-test flight (progress follows the frame count)".to_owned());
        }
        if facts.display.actual != (1920, 1080) {
            caveats.push(format!(
                "resolution {}x{}, not the reference 1920x1080",
                facts.display.actual.0, facts.display.actual.1
            ));
        }
        let p99 = frame_ms.as_ref().map(|s| s.p99);
        let sim = SimInfo {
            thread: true,
            note: "the simulation runs on its own thread since WP1.15: frame_ms and cpu_ms \
                   exclude it; runs from before that change are not comparable",
            tick_ms: Stats::of(&facts.sim.tick_ms),
            behind_passes: facts.sim.behind_passes,
            clock_resets: facts.sim.clock_resets,
            late_commands: facts.sim.late_commands,
            late_publishes: facts.sim.late_publishes,
            extra_tick_ms: round3(facts.sim.extra_tick_ms),
            ticks: facts.ticks,
            final_checksum: rec.checksums.last().map(|c| hex(c.1)),
            checksums: rec
                .checksums
                .iter()
                .map(|&(tick, c)| TickChecksum {
                    tick,
                    checksum: hex(c),
                })
                .collect(),
        };
        Report {
            format: 1,
            tool: "sf_demo --bench",
            version: env!("CARGO_PKG_VERSION"),
            build: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            host: facts.host,
            display: facts.display,
            present: facts.present,
            scene: facts.scene,
            flight: facts.flight,
            frames: FramesInfo {
                count: rec.frames.len(),
                seconds: round3(seconds),
                avg_fps: round3(rec.frames.len() as f64 / seconds.max(1e-9)),
            },
            frame_ms,
            cpu_ms,
            gpu_ms: GpuInfo {
                available: facts.gpu_enabled && !gpu_all.is_empty(),
                note: if facts.gpu_enabled && !gpu_all.is_empty() {
                    None
                } else {
                    Some(format!(
                        "n/a: {}",
                        facts
                            .gpu_note
                            .unwrap_or_else(|| "no timestamp results".into())
                    ))
                },
                stats: Stats::of(&gpu_all),
            },
            criterion: Criterion {
                name: "M0: p99 frame time over the whole flight with 5,000 units",
                p99_target_ms: round3(P99_TARGET_MS),
                p99_ms: p99,
                met: p99.map(|p| p <= P99_TARGET_MS),
                caveats,
            },
            phases,
            hitches: Hitches {
                threshold_ms: HITCH_MS,
                count: hitch_count,
                worst,
            },
            sim,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("the report serialises") + "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: (f64, f64) = (1024.0, 1024.0);

    #[test]
    fn the_flight_starts_close_and_ends_close_and_visits_the_whole_map() {
        let start = pose_at(0.0, MAP);
        assert_eq!((start.zoom, start.segment), (0.0, 0));
        let end = pose_at(1.0, MAP);
        assert!(end.zoom < 0.05, "ends close: {}", end.zoom);
        assert_eq!(end.segment, SEGMENTS.len() - 1);
        let max = (0..=1000)
            .map(|i| pose_at(f64::from(i) / 1000.0, MAP).zoom)
            .fold(0.0, f64::max);
        assert_eq!(max, 1.0);
        // Out of range progress is clamped, not extrapolated.
        assert_eq!(pose_at(-3.0, MAP), start);
        assert_eq!(pose_at(7.0, MAP), end);
    }

    #[test]
    fn the_path_is_continuous_and_visits_every_segment_in_order() {
        let mut last = pose_at(0.0, MAP);
        let mut seen = vec![last.segment];
        for i in 1..=20_000 {
            let p = pose_at(f64::from(i) / 20_000.0, MAP);
            // A step of 1/20000 of the flight never jumps.
            assert!((p.zoom - last.zoom).abs() < 0.01, "zoom jump at {i}");
            assert!((p.focus.0 - last.focus.0).abs() < 5.0, "focus jump at {i}");
            assert!((p.focus.1 - last.focus.1).abs() < 5.0, "focus jump at {i}");
            if p.segment != last.segment {
                seen.push(p.segment);
            }
            last = p;
        }
        assert_eq!(seen, (0..SEGMENTS.len()).collect::<Vec<_>>());
    }

    #[test]
    fn the_flight_scales_with_the_map() {
        let half = pose_at(0.5, (512.0, 512.0));
        let full = pose_at(0.5, MAP);
        assert_eq!(half.zoom, full.zoom);
        assert!((half.focus.0 * 2.0 - full.focus.0).abs() < 1e-9);
    }

    #[test]
    fn percentiles_are_nearest_rank() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        let s = Stats::of(&v).unwrap();
        assert_eq!(
            (s.samples, s.p50, s.p95, s.p99, s.max),
            (100, 50.0, 95.0, 99.0, 100.0)
        );
        assert_eq!(s.avg, 50.5);
        assert!(Stats::of(&[]).is_none());
        let one = Stats::of(&[4.0]).unwrap();
        assert_eq!((one.p50, one.p99, one.max), (4.0, 4.0, 4.0));
    }

    fn facts(software: bool, vsync: bool) -> RunFacts {
        RunFacts {
            host: Host {
                os: "linux",
                arch: "x86_64",
                adapter: "test".into(),
                backend_env: None,
            },
            display: Display {
                requested: "1920x1080 fullscreen".into(),
                actual: (1920, 1080),
                screen_mode: "borderless".into(),
                monitor: None,
                monitor_size: Some((1920, 1080)),
                refresh_hz: Some(60.0),
                note: None,
            },
            present: Present {
                mode: "Immediate".into(),
                vsync,
                requested: "uncapped",
                note: None,
            },
            scene: Scene {
                units: 5000,
                buildings: 24,
                models: Some("testpack".into()),
                map_cells: (256, 256),
                map_metres: (1024, 1024),
                metres_per_cell: 4,
                instance_buffers_reserved: true,
            },
            flight: FlightInfo {
                progress_by: "time",
                seconds: 75.0,
                frames_limit: None,
                warmup_frames: WARMUP_FRAMES,
                segments: SEGMENTS.iter().map(|s| s.name.to_owned()).collect(),
            },
            gpu_note: Some("test".into()),
            gpu_enabled: false,
            software_adapter: software,
            ticks: 1500,
            sim: SimRun {
                tick_ms: vec![4.0, 5.0, 6.0, 20.0],
                late_publishes: 1,
                ..SimRun::default()
            },
        }
    }

    fn recorder() -> Recorder {
        let mut r = Recorder::default();
        for i in 0..200_u32 {
            r.push(FrameSample {
                frame_ms: if i == 150 { 40.0 } else { 8.0 },
                cpu_ms: 5.0,
                units_in_view: 100 + i,
                draw_calls: 30,
                triangles: 1000,
                buffer_allocs: u32::from(i == 150),
                segment: (i / 25) as u8,
                ..FrameSample::default()
            });
        }
        r.checksums = vec![(100, 1), (200, 2)];
        r
    }

    #[test]
    fn the_report_is_valid_json_with_the_documented_keys() {
        let json = Report::build(&recorder(), facts(false, false)).to_json();
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        for key in [
            "format",
            "host",
            "display",
            "present",
            "scene",
            "flight",
            "frames",
            "frame_ms",
            "cpu_ms",
            "gpu_ms",
            "criterion",
            "phases",
            "hitches",
            "sim",
        ] {
            assert!(v.get(key).is_some(), "missing {key}");
        }
        assert_eq!(v["frame_ms"]["samples"], 200);
        assert_eq!(v["frame_ms"]["max"], 40.0);
        assert_eq!(v["frame_ms"]["p50"], 8.0);
        assert_eq!(v["display"]["actual"][0], 1920);
        assert_eq!(v["sim"]["final_checksum"], "0x0000000000000002");
        // The sim thread's own per-tick time, next to the frames' (WP1.15).
        assert_eq!(v["sim"]["thread"], true);
        assert_eq!(v["sim"]["tick_ms"]["samples"], 4);
        assert_eq!(v["sim"]["tick_ms"]["max"], 20.0);
        assert_eq!(v["sim"]["late_publishes"], 1);
        assert_eq!(v["sim"]["clock_resets"], 0);
        assert_eq!(v["hitches"]["count"], 1);
        assert_eq!(v["hitches"]["worst"][0]["frame"], 150);
        assert_eq!(v["hitches"]["worst"][0]["buffer_allocs"], 1);
        assert_eq!(v["phases"].as_array().unwrap().len(), SEGMENTS.len());
        // GPU time was not available: stated as n/a, not as zero.
        assert_eq!(v["gpu_ms"]["available"], false);
        assert!(v["gpu_ms"]["note"].as_str().unwrap().starts_with("n/a"));
        assert!(v["gpu_ms"]["stats"].is_null());
    }

    #[test]
    fn the_criterion_follows_p99_and_names_its_caveats() {
        let ok = Report::build(&recorder(), facts(false, false));
        assert_eq!(ok.criterion.met, Some(true), "p99 of the sample is 8 ms");
        assert!(
            ok.criterion.caveats.is_empty(),
            "{:?}",
            ok.criterion.caveats
        );
        let soft = Report::build(&recorder(), facts(true, true));
        assert_eq!(soft.criterion.caveats.len(), 2);
        let mut slow = Recorder::default();
        for _ in 0..100 {
            slow.push(FrameSample {
                frame_ms: 20.0,
                ..FrameSample::default()
            });
        }
        assert_eq!(
            Report::build(&slow, facts(false, false)).criterion.met,
            Some(false)
        );
    }

    #[test]
    fn phases_split_the_frames_by_segment() {
        let r = Report::build(&recorder(), facts(false, false));
        assert_eq!(r.phases[0].frames, 25);
        assert_eq!(r.phases[0].units_in_view.max, 124);
        assert_eq!(r.phases[6].buffer_allocs, 1);
        assert_eq!(r.phases.iter().map(|p| p.frames).sum::<usize>(), 200);
    }

    #[test]
    fn gpu_times_are_attributed_to_the_frame_they_measured() {
        let mut rec = recorder();
        rec.gpu = vec![(10, 2.0), (30, 4.0), (31, 6.0)];
        let mut f = facts(false, false);
        f.gpu_enabled = true;
        let r = Report::build(&rec, f);
        assert!(r.gpu_ms.available);
        assert_eq!(r.gpu_ms.stats.as_ref().unwrap().samples, 3);
        assert_eq!(r.phases[0].gpu_ms.as_ref().unwrap().samples, 1);
        assert_eq!(r.phases[1].gpu_ms.as_ref().unwrap().samples, 2);
    }

    #[test]
    fn a_late_gpu_time_goes_to_the_frame_it_was_submitted_in() {
        let warmup = 5;
        let mut rec = Recorder::default();
        // Two phases of three measured frames; the frames are recorded one
        // at a time and each result arrives two frames after its frame.
        for i in 0..6 {
            rec.push(FrameSample {
                segment: u8::from(i >= 3),
                ..FrameSample::default()
            });
        }
        // Frame numbers count the warm-up: measured frame k is number k + 5.
        // Frame 2 (last of phase 0) is measured at 7.0 ms, frame 3 (first of
        // phase 1) at 1.0 ms; both arrive while frames 4 and 5 are recorded.
        rec.push_gpu([(7, 7.0)], warmup);
        rec.push_gpu([(8, 1.0)], warmup);
        // A warm-up frame's time (number 3) is dropped, number 5 is frame 0.
        rec.push_gpu([(3, 99.0), (5, 3.0)], warmup);
        assert_eq!(rec.gpu, vec![(2, 7.0), (3, 1.0), (0, 3.0)]);
        let mut f = facts(false, false);
        f.gpu_enabled = true;
        let r = Report::build(&rec, f);
        let (p0, p1) = (&r.phases[0], &r.phases[1]);
        assert_eq!(p0.gpu_ms.as_ref().unwrap().samples, 2);
        assert_eq!(p0.gpu_ms.as_ref().unwrap().max, 7.0);
        assert_eq!(p1.gpu_ms.as_ref().unwrap().samples, 1);
        assert_eq!(p1.gpu_ms.as_ref().unwrap().max, 1.0);
    }
}
