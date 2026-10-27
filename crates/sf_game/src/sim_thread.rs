//! The simulation on its own thread (WP1.15).
//!
//! `World::step` costs 13 ms on average and 22 ms at worst with 2,000 units
//! that collide (the review of WP4.4). Stepped inside the frame, that is a
//! hitch in every third frame at 60 Hz. So the world lives on a thread of its
//! own, and the main thread (window, input, camera, rendering, UI) never
//! touches it: it sends commands in and reads snapshots out.
//!
//! ```text
//!  main thread                                   sim thread ("sf-sim")
//!  -----------                                   ---------------------
//!  input -> SimHandle::send --- mpsc channel --> World::schedule(tick, ..)
//!                              Command{tick,..}  World::step()   20 Hz, own clock
//!  render <- SimHandle::latest <-- one slot ---- SimSnapshot (publish after each pass)
//!            (Arc clone, nanoseconds)
//! ```
//!
//! # Commands
//!
//! A command is stamped with the tick it runs at when it is sent, as the
//! command queue always wanted: [`SimHandle::send`] takes the tick of the
//! newest published snapshot plus [`INPUT_DELAY_TICKS`]. The snapshot's tick
//! is the next one to run, and that one may be running right now, so the
//! delay is one tick: the command runs in the tick after it, at most 50 ms
//! later than the single-threaded game, which stamped the tick about to run.
//! (An input delay of a tick or two is what lockstep wants anyway.) The sim
//! thread hands the command to `World::schedule` with that tick, so the
//! lockstep semantics are the world's, unchanged: the commands of a tick run
//! in `(player, sequence)` order whatever the threads did. One exception, and
//! it is not silent: a command that arrives for a tick the sim has already run
//! (the main thread read an old snapshot while the sim caught up) is
//! scheduled for the next tick instead, counted
//! ([`SimCounters::late_commands`], shown in the F1 panel), printed to stderr
//! and **always logged** with the tick that was asked for and the tick it ran
//! at ([`SimHandle::moved_commands`], the last [`MOVED_LOG_CAP`] of them; the
//! game keeps this log, one entry only for a command that was moved, so it is
//! cheap and bounded). The full command log ([`SimConfig::record_commands`],
//! every command with the tick it ran at, so that a run can be replayed) is
//! off in the game, where it would grow with the length of the session, and on
//! in the tests.
//!
//! # Time, and what happens when the sim is slow
//!
//! The sim thread keeps its own clock: tick `n` is *due* at `start + n *
//! period` (50 ms), and the thread sleeps on the command channel until then,
//! so a command wakes it but never makes it step early. It publishes a
//! snapshot after each pass.
//!
//! If a tick overruns (the sim is behind its clock), the thread catches up,
//! but never more than [`MAX_CATCH_UP_TICKS`] extra ticks in one pass, and
//! after such a burst it waits out the rest of the 50 ms interval before the
//! next one: the sim never takes more than `1 + MAX_CATCH_UP_TICKS` ticks per
//! wall interval, so a long stall (a laptop that slept, a breakpoint) cannot
//! become a second of back-to-back ticks. If it is still more than
//! [`MAX_LAG_TICKS`] behind, it gives up catching up and restarts its clock
//! from now: game time runs slower than the wall clock, no tick is ever
//! skipped (that would break lockstep), and [`SimCounters::clock_resets`]
//! counts it. Meanwhile rendering is untouched: it draws the newest snapshot
//! every frame, and the F1 panel says "sim behind".
//!
//! # Snapshots and drawing
//!
//! The newest [`SimSnapshot`] sits in one slot behind a mutex that is held
//! for the time of a pointer swap, by either side, and never while a snapshot
//! is built or read. The sim thread builds the next one in a buffer it takes
//! back from the ones the main thread has let go of (`Arc` strong count 1),
//! so in steady state nothing is allocated. Each snapshot carries both the
//! previous and the current state of every entity, so the renderer
//! interpolates inside the newest one; a frame that comes between two
//! publishes draws the same snapshot with a larger alpha ([`interpolation_alpha`]).
//!
//! # Determinism
//!
//! Nothing here touches the rules: the thread only calls `World::schedule`
//! and `World::step`, with commands in the order they were sent. The same
//! commands for the same ticks give the same checksums as a single-threaded
//! run, which `tests/sim_thread.rs` checks tick by tick against
//! `sf_headless`. [`Pacing::Manual`] is how: the world steps only when told
//! to, so a test controls exactly which commands are in before which tick.
//!
//! # Shutdown
//!
//! Dropping the handle (or [`SimHandle::stop`]) tells the thread to stop and
//! joins it; the longest wait is the tick that is running. A thread that
//! panicked is joined too, reported by [`SimExit::panicked`], and the main
//! loop notices it through [`SimHandle::is_running`].

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use sf_sim::{Command, PlacementRejection, PlayerId, TICK_MILLIS, Tick, World};

use crate::sim_snapshot::{GhostQuery, Pace, SimSnapshot, Statics};

/// One sim tick of wall time (20 Hz).
pub const TICK_PERIOD: Duration = Duration::from_millis(TICK_MILLIS as u64);

/// A command is stamped for the newest published snapshot's tick plus this:
/// the snapshot's tick may be running already, so the command goes one
/// later. See the module documentation.
pub const INPUT_DELAY_TICKS: u64 = 1;

/// The most ticks beyond the due one that one pass takes when the sim is
/// behind; also the most per wall interval.
pub const MAX_CATCH_UP_TICKS: u32 = 3;

/// Behind its clock by more than this many ticks, the sim restarts its clock
/// (the game slows down) instead of trying to catch up.
pub const MAX_LAG_TICKS: u32 = 20;

/// How long after a tick was due its result starts to be shown. A tick that
/// is published within this time of its due time is drawn without a stutter:
/// the previous snapshot is still interpolating towards the state the new one
/// starts from. It is the budget of a tick on the sim thread (the sleep
/// jitter of the operating system included); the price is that the picture is
/// this much behind the sim. A tick that is published later stalls the
/// picture for the difference (counted in [`SimCounters::late_publishes`]).
pub const INTERP_DELAY: Duration = Duration::from_millis(30);

/// Every this many ticks the checksum is kept for the benchmark's list.
const CHECKPOINT_EVERY: u64 = 100;

/// The most per-tick times kept (about 2.7 hours of game time).
const TICK_TIMES_CAP: usize = 200_000;

/// The most moved commands kept ([`SimHandle::moved_commands`]).
pub const MOVED_LOG_CAP: usize = 1000;

/// Old snapshots the sim thread keeps for reuse.
const RETIRED_KEPT: usize = 3;

/// What drives the sim thread's ticks.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Pacing {
    /// Its own 20 Hz clock: the game.
    Realtime,
    /// Only [`SimHandle::advance`] steps the world, one snapshot per tick:
    /// the tests, which decide which commands are in before which tick.
    Manual,
}

/// How the sim thread runs.
#[derive(Clone, Debug)]
pub struct SimConfig {
    /// The length of a tick on the clock (50 ms in the game; tests shorten it).
    pub period: Duration,
    /// Own clock or stepped by hand.
    pub pacing: Pacing,
    /// Realtime only: hold the clock until [`SimHandle::start`], so the sim
    /// does not run while the window and the renderer are being made.
    pub start_paused: bool,
    /// See [`MAX_CATCH_UP_TICKS`].
    pub max_catch_up: u32,
    /// See [`MAX_LAG_TICKS`].
    pub max_lag_ticks: u32,
    /// Sleep this long inside every tick, after the step: a stand-in for a
    /// heavy tick (`--sim-delay-ms`, and the tests of a slow sim).
    pub extra_tick_time: Duration,
    /// Sleep this long once, in the step of this tick number: one hitch, for
    /// the tests of catching up.
    pub hitch: Option<(u64, Duration)>,
    /// Panic in the step of this tick number: the test hook for a sim thread
    /// that dies.
    pub panic_at: Option<u64>,
    /// Keep every command with the tick it really ran at ([`SimExit::commands`]).
    pub record_commands: bool,
}

impl SimConfig {
    /// The game: own clock, 50 ms ticks.
    pub fn realtime() -> SimConfig {
        SimConfig {
            period: TICK_PERIOD,
            pacing: Pacing::Realtime,
            start_paused: false,
            max_catch_up: MAX_CATCH_UP_TICKS,
            max_lag_ticks: MAX_LAG_TICKS,
            extra_tick_time: Duration::ZERO,
            hitch: None,
            panic_at: None,
            record_commands: false,
        }
    }

    /// Stepped by hand with [`SimHandle::advance`].
    pub fn manual() -> SimConfig {
        SimConfig {
            pacing: Pacing::Manual,
            ..SimConfig::realtime()
        }
    }
}

impl Default for SimConfig {
    fn default() -> SimConfig {
        SimConfig::realtime()
    }
}

/// A command as the sim thread ran it, for replaying a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoggedCommand {
    /// The tick the sender stamped.
    pub requested: Tick,
    /// The tick it was scheduled for: `requested`, or the next tick when that
    /// had already run.
    pub effective: Tick,
    /// Ticks the world had run when the command was handed to it. The world's
    /// checksum includes the commands that are waiting for their tick, so a
    /// replay that matches checksum for checksum puts each command in at this
    /// moment, not before.
    pub arrived: Tick,
    /// Who sent it.
    pub player: PlayerId,
    /// What it asks.
    pub command: Command,
}

/// What the sim thread leaves behind when it ends.
#[derive(Clone, Debug, Default)]
pub struct SimExit {
    /// Ticks the world had run.
    pub ticks: u64,
    /// The checksum after every tick, from the world's history.
    pub checksums: Vec<u64>,
    /// The commands in the order they ran, when
    /// [`SimConfig::record_commands`] asked for them.
    pub commands: Vec<LoggedCommand>,
    /// The thread ended by panicking; the rest is empty.
    pub panicked: bool,
    /// What the thread panicked with (the panic's message), when it did.
    pub panic_message: Option<String>,
}

/// Counters of the sim thread, readable at any time.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct SimCounters {
    /// Ticks the thread has stepped.
    pub ticks: u64,
    /// Passes that ended behind the clock.
    pub behind_passes: u64,
    /// Times the clock was restarted because the lag passed
    /// [`MAX_LAG_TICKS`].
    pub clock_resets: u64,
    /// Commands that arrived for a tick that had already run.
    pub late_commands: u64,
    /// Snapshots published later than [`INTERP_DELAY`] after their tick was
    /// due: the picture stalled for them.
    pub late_publishes: u64,
    /// Total time spent in ticks, in microseconds.
    pub step_micros: u64,
    /// The most ticks one pass has stepped: never more than
    /// `1 + max_catch_up`.
    pub max_pass_ticks: u64,
}

enum Msg {
    Command {
        tick: Tick,
        player: PlayerId,
        command: Command,
    },
    /// Manual pacing: step this many ticks, in order with the commands.
    Advance(u32),
    /// Realtime with `start_paused`: begin the clock.
    Start,
    /// The placement question to answer in every snapshot from now on (WP6.2), or none.
    Ghost(Option<GhostQuery>),
    Shutdown,
}

/// The builds the simulation refused that the handle keeps (the last ones).
pub const REJECTED_LOG_CAP: usize = 64;

/// What the two threads share.
struct Shared {
    latest: Mutex<Arc<SimSnapshot>>,
    published: Condvar,
    /// How long every tick took, in microseconds, in order.
    tick_micros: Mutex<Vec<u32>>,
    /// `(ticks run, checksum)` every [`CHECKPOINT_EVERY`] ticks.
    checkpoints: Mutex<Vec<(u64, u64)>>,
    ticks: AtomicU64,
    step_micros: AtomicU64,
    behind_passes: AtomicU64,
    clock_resets: AtomicU64,
    late_commands: AtomicU64,
    late_publishes: AtomicU64,
    max_pass_ticks: AtomicU64,
    /// Commands that were moved to a later tick, the last [`MOVED_LOG_CAP`].
    moved: Mutex<Vec<LoggedCommand>>,
    /// Every `BuildStructure` the world refused (WP6.2): how many in all, and the last
    /// [`REJECTED_LOG_CAP`] of them.
    rejected: Mutex<(u64, Vec<PlacementRejection>)>,
}

/// A lock that a panic elsewhere does not poison for good: the data behind
/// every lock here is valid at all times.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The main thread's end: send commands, read snapshots, stop.
pub struct SimHandle {
    tx: Sender<Msg>,
    shared: Arc<Shared>,
    join: Option<JoinHandle<SimExit>>,
    exit: Option<SimExit>,
    period: Duration,
}

impl SimHandle {
    /// Moves `world` onto a new thread. A first snapshot is taken here, so
    /// [`SimHandle::latest`] always has something to return.
    pub fn spawn(world: World, config: SimConfig) -> std::io::Result<SimHandle> {
        let statics = Statics::new(&world);
        let mut first = SimSnapshot::blank();
        first.fill(&world, &statics, Pace::now());
        let shared = Arc::new(Shared {
            latest: Mutex::new(Arc::new(first)),
            published: Condvar::new(),
            tick_micros: Mutex::new(Vec::new()),
            checkpoints: Mutex::new(Vec::new()),
            ticks: AtomicU64::new(0),
            step_micros: AtomicU64::new(0),
            behind_passes: AtomicU64::new(0),
            clock_resets: AtomicU64::new(0),
            late_commands: AtomicU64::new(0),
            late_publishes: AtomicU64::new(0),
            max_pass_ticks: AtomicU64::new(0),
            moved: Mutex::new(Vec::new()),
            rejected: Mutex::new((0, Vec::new())),
        });
        let (tx, rx) = mpsc::channel();
        let period = config.period;
        let sim = Sim {
            world,
            started: !config.start_paused,
            config,
            rx,
            shared: Arc::clone(&shared),
            statics,
            retired: Vec::new(),
            ghost: None,
            log: Vec::new(),
            last_step: Duration::ZERO,
        };
        let join = thread::Builder::new()
            .name("sf-sim".into())
            .spawn(move || sim.run())?;
        Ok(SimHandle {
            tx,
            shared,
            join: Some(join),
            exit: None,
            period,
        })
    }

    /// The newest snapshot. Cheap (a lock for a pointer copy) and never
    /// waits for the sim: safe to call every frame, and as often as needed.
    pub fn latest(&self) -> Arc<SimSnapshot> {
        Arc::clone(&lock(&self.shared.latest))
    }

    /// Sends `command` from `player` for the tick after the newest snapshot's
    /// ([`INPUT_DELAY_TICKS`]) and returns that tick.
    pub fn send(&self, player: PlayerId, command: Command) -> Tick {
        let tick = self.latest().tick().saturating_add(INPUT_DELAY_TICKS);
        self.send_at(tick, player, command);
        tick
    }

    /// Sends `command` from `player` for `tick`. Commands reach the world in
    /// the order they were sent.
    pub fn send_at(&self, tick: Tick, player: PlayerId, command: Command) {
        // A dead sim thread has nobody to tell: the main loop notices it with
        // `is_running`.
        let _ = self.tx.send(Msg::Command {
            tick,
            player,
            command,
        });
    }

    /// Asks the simulation, in every snapshot from the next one on, whether `query.player` may
    /// build `query.kind` on `query.cell` (WP6.2); `None` stops asking. The answer is
    /// `World::preview_placement`, read with [`SimSnapshot::ghost`]. Send it when the question
    /// changes, not every frame.
    pub fn set_ghost(&self, query: Option<GhostQuery>) {
        let _ = self.tx.send(Msg::Ghost(query));
    }

    /// The builds the world refused (WP6.2), as `(how many in all, the last ones)`: each with
    /// the tick, the player, the kind, the cell and the reason. A `BuildStructure` that is
    /// refused is reported here and nowhere else; the game says it on the console.
    pub fn rejected_builds(&self) -> (u64, Vec<PlacementRejection>) {
        lock(&self.shared.rejected).clone()
    }

    /// Starts the clock of a thread made with [`SimConfig::start_paused`].
    pub fn start(&self) {
        let _ = self.tx.send(Msg::Start);
    }

    /// [`Pacing::Manual`]: runs `ticks` ticks, after the commands sent so
    /// far, and publishes a snapshot after each. Ignored by a realtime sim.
    pub fn advance(&self, ticks: u32) {
        let _ = self.tx.send(Msg::Advance(ticks));
    }

    /// Waits until a snapshot of at least `tick` ticks run is published and
    /// returns the newest one; `None` after `timeout`.
    pub fn wait_for_tick(&self, tick: Tick, timeout: Duration) -> Option<Arc<SimSnapshot>> {
        let guard = lock(&self.shared.latest);
        let (guard, _) = self
            .shared
            .published
            .wait_timeout_while(guard, timeout, |snap| snap.tick() < tick)
            .unwrap_or_else(PoisonError::into_inner);
        (guard.tick() >= tick).then(|| Arc::clone(&guard))
    }

    /// The length of a tick on this sim's clock.
    pub fn period(&self) -> Duration {
        self.period
    }

    /// The thread's counters now.
    pub fn counters(&self) -> SimCounters {
        let s = &self.shared;
        SimCounters {
            ticks: s.ticks.load(Ordering::Relaxed),
            behind_passes: s.behind_passes.load(Ordering::Relaxed),
            clock_resets: s.clock_resets.load(Ordering::Relaxed),
            late_commands: s.late_commands.load(Ordering::Relaxed),
            late_publishes: s.late_publishes.load(Ordering::Relaxed),
            step_micros: s.step_micros.load(Ordering::Relaxed),
            max_pass_ticks: s.max_pass_ticks.load(Ordering::Relaxed),
        }
    }

    /// How long each tick took since the thread began, in milliseconds, from
    /// the `from`-th tick on.
    pub fn tick_ms_since(&self, from: usize) -> Vec<f64> {
        lock(&self.shared.tick_micros)
            .iter()
            .skip(from)
            .map(|&us| f64::from(us) / 1e3)
            .collect()
    }

    /// The commands that were moved to a later tick because theirs had run
    /// (the last [`MOVED_LOG_CAP`]), each with the tick that was asked for
    /// and the tick it ran at. Always kept, whatever
    /// [`SimConfig::record_commands`] says.
    pub fn moved_commands(&self) -> Vec<LoggedCommand> {
        lock(&self.shared.moved).clone()
    }

    /// `(ticks run, checksum)` after every 100th tick.
    pub fn checkpoints(&self) -> Vec<(u64, u64)> {
        lock(&self.shared.checkpoints).clone()
    }

    /// False once the thread has ended: stopped, or panicked.
    pub fn is_running(&self) -> bool {
        self.join.as_ref().is_some_and(|j| !j.is_finished())
    }

    /// Tells the thread to stop, waits for it (at most the tick that is
    /// running) and returns what it left. Does nothing the second time.
    pub fn stop(&mut self) -> &SimExit {
        if let Some(join) = self.join.take() {
            // Ignored when the thread is already gone.
            let _ = self.tx.send(Msg::Shutdown);
            self.exit = Some(join.join().unwrap_or_else(|payload| {
                // Keep what it panicked with and say so: the default panic
                // hook printed it on the sim thread, this puts it where the
                // game's own messages are.
                let message = panic_text(payload.as_ref());
                eprintln!("sf_game: the simulation thread panicked: {message}");
                SimExit {
                    panicked: true,
                    panic_message: Some(message),
                    ..SimExit::default()
                }
            }));
        }
        self.exit.get_or_insert_with(SimExit::default)
    }
}

/// The text of a panic payload (`panic!("..")` gives a `&str` or a `String`).
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(not a text)".to_owned())
}

impl Drop for SimHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// How far between the snapshot's last two states the picture is at `now`,
/// `0.0` to `1.0`.
///
/// The snapshot's last tick was due at `pace.due`; its result is shown from
/// [`INTERP_DELAY`] later for one `period`. Before that the picture is still
/// finishing the tick before (the previous snapshot at alpha 1), so a new
/// snapshot that was published in time starts at 0 exactly where the old one
/// ended. Past the end it holds at 1 (the sim is behind: no extrapolation).
pub fn interpolation_alpha(now: Instant, pace: &Pace, period: Duration) -> f32 {
    let shown = now
        .saturating_duration_since(pace.due)
        .saturating_sub(INTERP_DELAY);
    (shown.as_secs_f64() / period.as_secs_f64()).min(1.0) as f32
}

enum Flow {
    Go,
    Stop,
}

/// The sim thread's state.
struct Sim {
    world: World,
    config: SimConfig,
    /// The clock runs (false only while held by `start_paused`).
    started: bool,
    rx: Receiver<Msg>,
    shared: Arc<Shared>,
    statics: Statics,
    /// Snapshots that were published and replaced, waiting for the main
    /// thread to let go so that their buffers can be used again.
    retired: Vec<Arc<SimSnapshot>>,
    /// The placement question to answer in every snapshot (WP6.2).
    ghost: Option<GhostQuery>,
    log: Vec<LoggedCommand>,
    last_step: Duration,
}

impl Sim {
    fn run(mut self) -> SimExit {
        match self.config.pacing {
            Pacing::Realtime => self.run_realtime(),
            Pacing::Manual => self.run_manual(),
        }
        SimExit {
            ticks: self.world.tick().0,
            checksums: self.world.checksum_history().to_vec(),
            commands: std::mem::take(&mut self.log),
            panicked: false,
            panic_message: None,
        }
    }

    fn run_manual(&mut self) {
        while let Ok(msg) = self.rx.recv() {
            if matches!(self.handle(msg), Flow::Stop) {
                return;
            }
        }
    }

    fn run_realtime(&mut self) {
        while !self.started {
            match self.rx.recv() {
                Ok(msg) => {
                    if matches!(self.handle(msg), Flow::Stop) {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
        let period = self.config.period;
        let mut due = Instant::now();
        loop {
            if !self.wait_until(due) {
                return;
            }
            let pass_start = Instant::now();
            let behind = pass_start.saturating_duration_since(due);
            let owed = 1 + (behind.as_nanos() / period.as_nanos().max(1)) as u64;
            let steps = owed.min(1 + u64::from(self.config.max_catch_up));
            let mut last_due = due;
            for k in 0..steps {
                // Commands that came while the last tick ran go in before
                // this one.
                while let Ok(msg) = self.rx.try_recv() {
                    if matches!(self.handle(msg), Flow::Stop) {
                        return;
                    }
                }
                last_due = due + period * k as u32;
                self.step_once();
            }
            due += period * steps as u32;
            self.shared
                .max_pass_ticks
                .fetch_max(steps, Ordering::Relaxed);
            let now = Instant::now();
            let lag = now.saturating_duration_since(due);
            if !lag.is_zero() {
                self.shared.behind_passes.fetch_add(1, Ordering::Relaxed);
            }
            if lag > period * self.config.max_lag_ticks {
                // Too far behind to catch up: the game runs slower than the
                // clock from here. No tick is skipped.
                due = now;
                self.shared.clock_resets.fetch_add(1, Ordering::Relaxed);
            }
            self.publish(last_due, lag);
            // A burst of catch-up ticks is followed by the rest of its
            // interval: never more than `1 + max_catch_up` ticks per
            // `period` of wall time.
            if steps > 1 && !self.wait_until(pass_start + period) {
                return;
            }
        }
    }

    /// Handles messages until `deadline`; false when the thread should end.
    fn wait_until(&mut self, deadline: Instant) -> bool {
        loop {
            let now = Instant::now();
            if now >= deadline {
                return true;
            }
            match self.rx.recv_timeout(deadline - now) {
                Ok(msg) => {
                    if matches!(self.handle(msg), Flow::Stop) {
                        return false;
                    }
                }
                Err(RecvTimeoutError::Timeout) => return true,
                Err(RecvTimeoutError::Disconnected) => return false,
            }
        }
    }

    fn handle(&mut self, msg: Msg) -> Flow {
        match msg {
            Msg::Command {
                tick,
                player,
                command,
            } => self.schedule(tick, player, command),
            Msg::Advance(n) => {
                if self.config.pacing == Pacing::Manual {
                    for _ in 0..n {
                        self.step_once();
                        self.publish(Instant::now(), Duration::ZERO);
                    }
                }
            }
            Msg::Start => self.started = true,
            Msg::Ghost(query) => {
                self.ghost = query;
                // Stepped by hand there is no clock to carry the answer to the next snapshot.
                if self.config.pacing == Pacing::Manual {
                    self.publish(Instant::now(), Duration::ZERO);
                }
            }
            Msg::Shutdown => return Flow::Stop,
        }
        Flow::Go
    }

    fn schedule(&mut self, tick: Tick, player: PlayerId, command: Command) {
        let now = self.world.tick();
        let effective = tick.max(now);
        let moved = effective != tick;
        if moved {
            self.shared.late_commands.fetch_add(1, Ordering::Relaxed);
        }
        let logged = (self.config.record_commands || moved).then(|| command.clone());
        match self.world.schedule(effective, player, command) {
            Ok(()) => {
                if let Some(command) = logged {
                    let entry = LoggedCommand {
                        requested: tick,
                        effective,
                        arrived: now,
                        player,
                        command,
                    };
                    if moved {
                        eprintln!(
                            "sf_game: sim: a command for tick {tick} came after it had run; \
                             it runs in tick {effective}"
                        );
                        let mut kept = lock(&self.shared.moved);
                        if kept.len() >= MOVED_LOG_CAP {
                            kept.remove(0);
                        }
                        kept.push(entry.clone());
                    }
                    if self.config.record_commands {
                        self.log.push(entry);
                    }
                }
            }
            Err(e) => eprintln!("sf_game: sim: command refused: {e}"),
        }
    }

    /// One `World::step`, timed.
    fn step_once(&mut self) {
        let began = Instant::now();
        let number = self.world.tick().0;
        let report = self.world.step();
        if report.placements_rejected > 0 {
            let mut kept = lock(&self.shared.rejected);
            kept.0 += u64::from(report.placements_rejected);
            kept.1.extend_from_slice(self.world.placement_rejections());
            let excess = kept.1.len().saturating_sub(REJECTED_LOG_CAP);
            kept.1.drain(..excess);
        }
        if self.config.panic_at == Some(number) {
            panic!("sim thread test hook: panic in tick {number}");
        }
        if !self.config.extra_tick_time.is_zero() {
            thread::sleep(self.config.extra_tick_time);
        }
        if let Some((at, extra)) = self.config.hitch
            && at == number
        {
            thread::sleep(extra);
        }
        let spent = began.elapsed();
        self.last_step = spent;
        let micros = u32::try_from(spent.as_micros()).unwrap_or(u32::MAX);
        let shared = &self.shared;
        shared.ticks.fetch_add(1, Ordering::Relaxed);
        shared
            .step_micros
            .fetch_add(u64::from(micros), Ordering::Relaxed);
        {
            let mut times = lock(&shared.tick_micros);
            if times.len() < TICK_TIMES_CAP {
                times.push(micros);
            }
        }
        let ran = self.world.tick().0;
        if ran.is_multiple_of(CHECKPOINT_EVERY) {
            lock(&shared.checkpoints).push((ran, report.checksum));
        }
    }

    /// Builds the snapshot of the world as it is and makes it the newest.
    fn publish(&mut self, due: Instant, lag: Duration) {
        let mut next = self.take_buffer();
        let pace = Pace {
            due,
            step: self.last_step,
            lag,
        };
        Arc::get_mut(&mut next)
            .expect("a buffer taken from the pool is held by nobody else")
            .fill(&self.world, &self.statics, pace);
        if let Some(query) = self.ghost {
            Arc::get_mut(&mut next)
                .expect("a buffer taken from the pool is held by nobody else")
                .answer(&self.world, query);
        }
        // Counted when the snapshot is ready, which is when the main thread
        // can first see it.
        if self.config.pacing == Pacing::Realtime && due.elapsed() > INTERP_DELAY {
            self.shared.late_publishes.fetch_add(1, Ordering::Relaxed);
        }
        let old = std::mem::replace(&mut *lock(&self.shared.latest), next);
        self.shared.published.notify_all();
        self.retired.push(old);
        while self.retired.len() > RETIRED_KEPT {
            self.retired.remove(0);
        }
    }

    /// A buffer for the next snapshot: a retired one the main thread has let
    /// go of, or a new one. A retired snapshot can no longer be reached from
    /// the slot, so its count can only fall.
    fn take_buffer(&mut self) -> Arc<SimSnapshot> {
        match self.retired.iter().position(|a| Arc::strong_count(a) == 1) {
            Some(i) => self.retired.remove(i),
            None => Arc::new(SimSnapshot::blank()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pace(due: Instant) -> Pace {
        Pace {
            due,
            step: Duration::ZERO,
            lag: Duration::ZERO,
        }
    }

    #[test]
    fn the_picture_waits_out_the_delay_then_runs_a_tick_and_holds() {
        let due = Instant::now();
        let at = |after: Duration| interpolation_alpha(due + after, &pace(due), TICK_PERIOD);
        assert_eq!(at(Duration::ZERO), 0.0);
        assert_eq!(at(INTERP_DELAY), 0.0);
        assert!((at(INTERP_DELAY + TICK_PERIOD / 2) - 0.5).abs() < 1e-6);
        assert_eq!(at(INTERP_DELAY + TICK_PERIOD), 1.0);
        // The sim is behind: the picture stays on the last state, it does not
        // run on past it.
        assert_eq!(at(Duration::from_secs(5)), 1.0);
        // A due time in the future (clock skew in a test) is the start.
        assert_eq!(
            interpolation_alpha(due, &pace(due + TICK_PERIOD), TICK_PERIOD),
            0.0
        );
    }

    /// The old snapshot ends where the new one starts, so a tick that is
    /// published in time cannot be seen as a jump.
    #[test]
    fn consecutive_snapshots_join_up() {
        let due = Instant::now();
        let next_due = due + TICK_PERIOD;
        let when = due + INTERP_DELAY + TICK_PERIOD;
        assert_eq!(interpolation_alpha(when, &pace(due), TICK_PERIOD), 1.0);
        assert_eq!(interpolation_alpha(when, &pace(next_due), TICK_PERIOD), 0.0);
    }

    #[test]
    fn a_command_is_stamped_one_tick_after_the_newest_snapshot() {
        let world = World::new(sf_sim::WorldConfig::new(1).with_map_size(8, 8)).unwrap();
        let mut sim = SimHandle::spawn(world, SimConfig::manual()).unwrap();
        assert_eq!(
            sim.send(PlayerId(0), Command::Stop { ids: Vec::new() }),
            Tick(1)
        );
        sim.advance(3);
        sim.wait_for_tick(Tick(3), Duration::from_secs(10)).unwrap();
        assert_eq!(
            sim.send(PlayerId(0), Command::Stop { ids: Vec::new() }),
            Tick(3 + INPUT_DELAY_TICKS)
        );
        sim.stop();
    }
}
