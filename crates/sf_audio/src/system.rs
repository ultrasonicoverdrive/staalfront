//! [`AudioSystem`]: the public face of the crate.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use kira::sound::static_sound::StaticSoundData;

use kira::{AudioManagerSettings, DefaultBackend};

use crate::bank::{Bank, Category, SoundDef};
use crate::live::{Live, Outcome, Play, Player};
use crate::music::{FallbackTracks, Mood, MusicConfig, Shuffle};
use crate::ra::{self, Catalog, EntryInfo, Mapping, MusicTracks, RaAudioConfig};
use crate::spatial::{self, Listener, SpatialParams};

/// Bank file relative to the asset root.
pub const BANK_FILE: &str = "audio/bank.ron";
/// Default music crossfade.
pub const DEFAULT_CROSSFADE: Duration = Duration::from_millis(1500);

/// Counters of what happened to play requests (for tests and a debug overlay).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Handed to the output (always 0 on the silent backend).
    pub played: u64,
    /// Dropped by voice limiting.
    pub limited: u64,
    /// Dropped because too quiet to matter (far from the listener).
    pub inaudible: u64,
    /// Name not in the bank.
    pub unknown: u64,
    /// File missing/undecodable or the output refused it.
    pub failed: u64,
}

pub struct AudioSystem {
    root: PathBuf,
    bank: Bank,
    /// `None` = silent backend: every call is accepted and does nothing.
    player: Option<Box<dyn Player>>,
    listener: Listener,
    params: SpatialParams,
    crossfade: Duration,
    category_volume: [f32; 3],
    rng: u32,
    stats: Stats,
    warned: BTreeSet<String>,
    ra: Ra,
    music: MusicState,
    /// `Some` = recording what would be played (tests, debug); see [`AudioSystem::set_trace`].
    trace: Option<Vec<PlayTrace>>,
}

/// The mood-driven music (WP11.4).
struct MusicState {
    /// The mood the game asked for last.
    want: Option<Mood>,
    /// What was started for which mood (`None` track = silence / default).
    current: Option<(Mood, Option<String>)>,
    /// A Red Alert track plays (so a volume change reaches it).
    ra_track: bool,
    fallback: FallbackTracks,
    shuffle: [Shuffle; 2],
    /// A track being decoded on a background thread.
    pending: Option<Pending>,
    /// Consecutive tracks that failed to decode, per mood.
    failures: [usize; 2],
    /// Alternates, so only the running track and its fading predecessor are in memory.
    slot: u8,
    /// Warned about this track already.
    warned: BTreeSet<String>,
    /// Record the tracks started (see [`AudioSystem::take_music_trace`]).
    trace_on: bool,
    trace: Vec<MusicTrace>,
    last_load: Option<MusicLoad>,
}

struct Pending {
    mood: Mood,
    name: String,
    rx: Receiver<Result<(StaticSoundData, Duration), String>>,
}

/// A track the mood logic started (recorded when tracing).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MusicTrace {
    pub mood: Mood,
    /// Track label (entry name or `#id`), or the bank name of the default music.
    pub track: String,
}

/// What the last Red Alert track cost to load; decoding runs on a background thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MusicLoad {
    pub decode: Duration,
    /// Frames (stereo f32 in memory: 8 bytes each).
    pub frames: usize,
}

impl MusicState {
    fn new() -> Self {
        Self {
            want: None,
            current: None,
            ra_track: false,
            fallback: FallbackTracks {
                calm: None,
                combat: None,
            },
            shuffle: [Shuffle::new(0x2545_F491), Shuffle::new(0x9E37_79B1)],
            pending: None,
            failures: [0; 2],
            slot: 0,
            warned: BTreeSet::new(),
            trace_on: false,
            trace: Vec::new(),
            last_load: None,
        }
    }
}

/// One accepted play request as the output would get it.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayTrace {
    pub name: String,
    /// Base volume x category volume x spatial gain.
    pub gain: f32,
    pub pan: f32,
}

/// Original Red Alert audio (WP11.7): off, loading on a background thread,
/// or ready with the events it replaces.
enum Ra {
    Off,
    Loading(Receiver<ra::Loaded>),
    Ready {
        catalog: Arc<Catalog>,
        events: BTreeSet<String>,
        music: MusicTracks,
    },
}

/// State of the original Red Alert audio, for the sound browser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RaStatus {
    Off,
    Loading,
    /// Scanned: how many events now use a Red Alert sound.
    Ready {
        mapped: usize,
    },
}

/// Synthetic cache key of a decoded Red Alert sound.
fn ra_key(what: &str) -> PathBuf {
    PathBuf::from(format!("ra-audio:{what}"))
}

/// Bank entry used for a mapped event that has no CC0 sound yet.
fn default_def(key: &Path) -> SoundDef {
    SoundDef {
        file: key.display().to_string(),
        volume: 1.0,
        pitch_variation: 0.0,
        max_instances: crate::bank::DEFAULT_MAX_INSTANCES,
        category: Category::Sfx,
        looped: false,
    }
}

/// A seed that differs per launch: the clock mixed with the OS-seeded
/// `RandomState` hasher. Presentation only (music shuffle), never in sf_sim.
fn launch_seed() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    if let Ok(t) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        h.write_u128(t.as_nanos());
    }
    h.finish()
}

impl AudioSystem {
    /// Open the default output device and load `<asset_root>/audio/bank.ron`.
    /// Without a usable device (no sound card, CI, a headless container) this
    /// returns a silent system instead of failing. Never panics.
    pub fn new(asset_root: impl Into<PathBuf>) -> Self {
        let mut sys = Self::silent(asset_root);
        // cpal can panic on exotic hosts; a missing device must never take the game down.
        let opened = std::panic::catch_unwind(|| {
            Live::<DefaultBackend>::new(AudioManagerSettings::default()).map_err(|e| e.to_string())
        });
        match opened {
            Ok(Ok(live)) => sys.player = Some(Box::new(live)),
            Ok(Err(e)) => eprintln!("sf_audio: no audio output ({e}); running silently"),
            Err(_) => eprintln!("sf_audio: audio backend panicked; running silently"),
        }
        // A fresh shuffle order per launch (presentation only; sf_sim never sees it).
        sys.reseed_music(launch_seed());
        sys
    }

    /// Reseed the per-mood shuffles. [`silent`](Self::silent) and the test
    /// constructors keep fixed seeds, so their order is reproducible.
    pub fn reseed_music(&mut self, seed: u64) {
        let lo = seed as u32;
        let hi = (seed >> 32) as u32;
        self.music.shuffle = [
            Shuffle::new(lo ^ 0x2545_F491),
            Shuffle::new(hi ^ 0x9E37_79B1),
        ];
    }

    /// A system without output: accepts the whole API and does nothing
    /// (the bank is still loaded and validated).
    pub fn silent(asset_root: impl Into<PathBuf>) -> Self {
        let mut sys = Self {
            root: asset_root.into(),
            bank: Bank::default(),
            player: None,
            listener: Listener::default(),
            params: SpatialParams::default(),
            crossfade: DEFAULT_CROSSFADE,
            category_volume: [1.0; 3],
            rng: 0x9E37_79B9,
            stats: Stats::default(),
            warned: BTreeSet::new(),
            ra: Ra::Off,
            music: MusicState::new(),
            trace: None,
        };
        sys.load_bank();
        sys
    }

    #[cfg(test)]
    pub(crate) fn with_player(root: impl Into<PathBuf>, player: Box<dyn Player>) -> Self {
        let mut sys = Self::silent(root);
        sys.player = Some(player);
        sys
    }

    /// True when there is no output device (all calls are no-ops).
    pub fn is_silent(&self) -> bool {
        self.player.is_none()
    }

    pub fn bank(&self) -> &Bank {
        &self.bank
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    // ---- bank -------------------------------------------------------------

    /// Re-read the bank file. On a parse error the old bank stays. Returns
    /// the validation warnings (also logged).
    pub fn reload_bank(&mut self) -> Vec<String> {
        if let Some(p) = &mut self.player {
            p.clear_cache();
        }
        self.load_bank()
    }

    /// Hook for `sf_engine`'s `on_asset_changed`: reloads when the bank file
    /// itself changed. Returns whether it did.
    pub fn on_asset_changed(&mut self, path: &Path) -> bool {
        if path.ends_with(BANK_FILE) {
            self.reload_bank();
            true
        } else {
            false
        }
    }

    fn load_bank(&mut self) -> Vec<String> {
        let path = self.root.join(BANK_FILE);
        let mut bank = match Bank::load(&path) {
            Ok(b) => b,
            Err(e) => {
                let msg = format!("{} ({}); keeping the previous bank", e, path.display());
                eprintln!("sf_audio: {msg}");
                return vec![msg];
            }
        };
        let root = self.root.clone();
        let warnings = bank.validate(|f| root.join(f).is_file());
        for w in &warnings {
            eprintln!("sf_audio: {w}");
        }
        self.bank = bank;
        self.warned.clear();
        warnings
    }

    // ---- settings ---------------------------------------------------------

    /// Set the listener from the camera; call once per frame.
    pub fn set_listener(&mut self, listener: Listener) {
        self.listener = listener;
    }

    /// The listener as last set.
    pub fn listener(&self) -> Listener {
        self.listener
    }

    pub fn set_spatial_params(&mut self, params: SpatialParams) {
        self.params = params;
    }

    /// The spatial numbers as last set: the one copy the output and the
    /// fight-sound planner both use.
    pub fn spatial_params(&self) -> SpatialParams {
        self.params
    }

    /// Default crossfade/fade-out time for [`play_music`](Self::play_music)
    /// and [`stop_music`](Self::stop_music).
    pub fn set_crossfade(&mut self, d: Duration) {
        self.crossfade = d;
    }

    pub fn crossfade(&self) -> Duration {
        self.crossfade
    }

    /// Linear volume of a category (`1.0` default), applied to new sounds
    /// and, for music, to the running track.
    pub fn set_category_volume(&mut self, category: Category, volume: f32) {
        let v = if volume.is_finite() {
            volume.max(0.0)
        } else {
            1.0
        };
        self.category_volume[category.index()] = v;
        if category == Category::Music
            && self.music.ra_track
            && let Some(p) = &mut self.player
        {
            p.set_music_gain(v);
        } else if let Some(p) = &mut self.player
            && let Some(def) = p.music_name().and_then(|n| self.bank.get(n))
            && def.category == category
        {
            p.set_music_gain(def.volume * v);
        }
    }

    pub fn category_volume(&self, category: Category) -> f32 {
        self.category_volume[category.index()]
    }

    // ---- playing ----------------------------------------------------------

    /// Play a sound without position (UI, announcements).
    pub fn play(&mut self, name: &str) {
        self.play_with(name, None);
    }

    /// Play a sound at a world position (`+Y` up), relative to the listener.
    pub fn play_at(&mut self, name: &str, world_pos: [f32; 3]) {
        self.play_with(name, Some(world_pos));
    }

    /// Whether `name` has a sound to play: in the bank, or replaced by a
    /// mapped Red Alert sound. Unlike [`play`](Self::play) it never warns.
    pub fn has_sound(&self, name: &str) -> bool {
        self.bank.get(name).is_some()
            || matches!(&self.ra, Ra::Ready { events, .. } if events.contains(name))
    }

    /// Start (`true`) or stop (`false`) recording the play requests that get
    /// past the lookup and the audibility check, with the gain and pan the
    /// output would get. Works without a device; for tests and a debug view.
    pub fn set_trace(&mut self, on: bool) {
        self.trace = on.then(Vec::new);
    }

    /// The recorded requests since the last call (empty when not tracing).
    pub fn take_trace(&mut self) -> Vec<PlayTrace> {
        self.trace.as_mut().map(std::mem::take).unwrap_or_default()
    }

    fn play_with(&mut self, name: &str, pos: Option<[f32; 3]>) {
        // Look the name up first so unknown names warn even without a device.
        let Some((def, path)) = self.resolve(name) else {
            return;
        };
        if self.player.is_none() && self.trace.is_none() {
            return;
        }
        let (mut gain, mut pan) = (self.base_gain(&def), 0.0);
        if let Some(pos) = pos {
            let m = spatial::mix(&self.listener, pos, &self.params);
            if !m.is_audible(&self.params) {
                self.stats.inaudible += 1;
                return;
            }
            gain *= m.gain;
            pan = m.pan;
        }
        if let Some(t) = &mut self.trace {
            t.push(PlayTrace {
                name: name.to_string(),
                gain,
                pan,
            });
        }
        let rate = self.rate_for(&def);
        let outcome = self.player.as_mut().map(|p| {
            p.play(&Play {
                name,
                path: &path,
                gain,
                pan,
                rate,
                max_instances: def.max_instances,
                looped: def.looped,
            })
        });
        self.record(outcome);
    }

    /// Start a music track, crossfading from the current one over
    /// [`crossfade`](Self::crossfade). Playing the running track again does nothing.
    pub fn play_music(&mut self, name: &str) {
        let Some((def, path)) = self.resolve(name) else {
            return;
        };
        if self.player.is_none() {
            return;
        }
        let gain = self.base_gain(&def);
        let fade = self.crossfade;
        let outcome = self.player.as_mut().map(|p| {
            p.play_music(
                &Play {
                    name,
                    path: &path,
                    gain,
                    pan: 0.0,
                    rate: 1.0,
                    max_instances: 1,
                    looped: def.looped,
                },
                fade,
            )
        });
        self.record(outcome);
    }

    /// Fade the music out over [`crossfade`](Self::crossfade) and stop it.
    pub fn stop_music(&mut self) {
        let fade = self.crossfade;
        if let Some(p) = &mut self.player {
            p.stop_music(fade);
        }
    }

    /// Name of the running music track.
    pub fn current_music(&self) -> Option<&str> {
        self.player.as_ref().and_then(|p| p.music_name())
    }

    /// Per-frame housekeeping (releases finished voices, picks up the
    /// original Red Alert sounds once their background load is done).
    pub fn update(&mut self) {
        if let Some(p) = &mut self.player {
            p.tick();
        }
        self.poll_ra();
        self.drive_music();
    }

    // ---- dynamic music (WP11.4) -------------------------------------------

    /// The crossfade and the default tracks of `audio.ron` (`music`).
    pub fn set_music_config(&mut self, c: &MusicConfig) {
        self.crossfade = Duration::from_millis(u64::from(c.crossfade_ms));
        self.music.fallback = c.fallback.clone();
    }

    /// The music should sound like `mood`. Cheap to call every frame: it only
    /// does something when the mood changed or the running track ended. The
    /// track is one of the player's own Red Alert tracks of the mood (shuffled)
    /// when "Use original Red Alert audio" is on, else the default music of
    /// `audio.ron`, else there is silence.
    pub fn set_music_mood(&mut self, mood: Mood) {
        self.music.want = Some(mood);
        self.drive_music();
    }

    /// Record (or stop recording) the tracks the mood logic starts, also on
    /// a silent system (where nothing is decoded).
    pub fn set_music_trace(&mut self, on: bool) {
        self.music.trace_on = on;
        if !on {
            self.music.trace.clear();
        }
    }

    /// The tracks the mood logic started since the last call (only recorded
    /// while [`set_music_trace`](Self::set_music_trace) is on).
    pub fn take_music_trace(&mut self) -> Vec<MusicTrace> {
        std::mem::take(&mut self.music.trace)
    }

    /// What the last Red Alert track needed to load.
    pub fn last_music_load(&self) -> Option<MusicLoad> {
        self.music.last_load
    }

    fn note_music(&mut self, mood: Mood, track: &str) {
        if self.music.trace_on {
            self.music.trace.push(MusicTrace {
                mood,
                track: track.to_string(),
            });
        }
    }

    fn drive_music(&mut self) {
        let Some(mood) = self.music.want else { return };
        // A track being decoded: start it when ready (unless the mood moved on).
        if let Some(pend) = self.music.pending.take() {
            match pend.rx.try_recv() {
                Err(TryRecvError::Empty) if pend.mood == mood => {
                    self.music.pending = Some(pend);
                    return;
                }
                Err(TryRecvError::Empty) | Ok(Ok(_)) if pend.mood != mood => {}
                Ok(Ok((data, took))) => {
                    self.music.failures[mood.index()] = 0;
                    self.music.last_load = Some(MusicLoad {
                        decode: took,
                        frames: data.frames.len(),
                    });
                    self.start_ra_track(mood, &pend.name, data);
                    return;
                }
                Ok(Err(e)) => {
                    if self.music.warned.insert(pend.name.clone()) {
                        eprintln!(
                            "sf_audio: cannot play Red Alert track '{}': {e}; skipping",
                            pend.name
                        );
                    }
                    self.music.failures[pend.mood.index()] += 1;
                }
                Err(TryRecvError::Disconnected) => {
                    self.music.failures[pend.mood.index()] += 1;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if self.player.is_none() && !self.music.trace_on {
            return;
        }
        let ended = self.music.ra_track
            && self
                .player
                .as_ref()
                .is_some_and(|p| p.music_name().is_none());
        let up_to_date = matches!(&self.music.current, Some((m, _)) if *m == mood) && !ended;
        if up_to_date {
            return;
        }
        let tracks: Vec<EntryInfo> = match &self.ra {
            Ra::Ready { music, .. } => music.get(mood).to_vec(),
            _ => Vec::new(),
        };
        if !tracks.is_empty() && self.music.failures[mood.index()] < tracks.len() {
            let i = self.music.shuffle[mood.index()]
                .next(tracks.len())
                .unwrap_or(0);
            let entry = tracks[i].clone();
            let name = entry.label();
            if self.player.is_none() {
                // No output: record the choice, decode nothing.
                self.note_music(mood, &name);
                self.music.current = Some((mood, Some(name)));
                return;
            }
            let (tx, rx) = std::sync::mpsc::channel();
            let spawned = std::thread::Builder::new()
                .name("sf_audio music".into())
                .spawn(move || {
                    let t = Instant::now();
                    let r = ra::decode_entry(&entry).map(|p| (p.to_static_sound(), t.elapsed()));
                    let _ = tx.send(r);
                });
            match spawned {
                // The old track keeps playing until this one is decoded.
                Ok(_) => {
                    self.music.pending = Some(Pending { mood, name, rx });
                    self.music.current = None;
                }
                Err(e) => {
                    eprintln!("sf_audio: cannot start the music loader ({e})");
                    self.music.failures[mood.index()] = usize::MAX / 2;
                }
            }
            return;
        }
        // No Red Alert track for this mood: the default music, or silence.
        self.music.ra_track = false;
        let fallback = self.music.fallback.get(mood).map(str::to_string);
        match fallback.filter(|n| self.bank.get(n).is_some()) {
            Some(n) => {
                self.note_music(mood, &n);
                self.play_music(&n);
                self.music.current = Some((mood, Some(n)));
            }
            None => {
                // Silence: fade out whatever the previous mood left running.
                self.stop_music();
                self.music.current = Some((mood, None));
            }
        }
    }

    fn start_ra_track(&mut self, mood: Mood, name: &str, data: StaticSoundData) {
        let key = ra_key(&format!("music.{}", self.music.slot));
        self.music.slot ^= 1;
        let fade = self.crossfade;
        let gain = self.category_volume[Category::Music.index()];
        let Some(p) = &mut self.player else { return };
        p.inject(key.clone(), data);
        let outcome = p.play_music(
            &Play {
                name,
                path: &key,
                gain,
                pan: 0.0,
                rate: 1.0,
                max_instances: 1,
                looped: false,
            },
            fade,
        );
        self.record(Some(outcome));
        self.note_music(mood, name);
        self.music.ra_track = outcome != Outcome::Failed;
        self.music.current = Some((mood, Some(name.to_string())));
    }

    // ---- original Red Alert audio (WP11.7) ---------------------------------

    /// "Use original Red Alert audio": scan `config.path` and decode the
    /// sounds named in `audio/ra_mapping.ron` on a background thread; they
    /// replace their events once [`update`](Self::update) sees the result.
    /// Disabled, an empty path, or nothing found: the CC0 sounds stay, with
    /// one warning on stderr. Never blocks and never panics.
    pub fn enable_original_audio(&mut self, config: &RaAudioConfig) {
        self.ra = Ra::Off;
        self.music.pending = None;
        self.music.current = None;
        self.music.ra_track = false;
        if !config.enabled {
            return;
        }
        if config.path.trim().is_empty() {
            eprintln!(
                "sf_audio: original Red Alert audio is on but no path is set; using the CC0 sounds"
            );
            return;
        }
        let mapping = Mapping::load(&self.root).unwrap_or_else(|e| {
            eprintln!("sf_audio: no Red Alert sound mapping ({e}); only the sound browser works");
            Mapping::default()
        });
        let path = PathBuf::from(config.path.trim());
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("sf_audio ra loader".into())
            .spawn(move || {
                let loaded = ra::load(&path, &mapping, &ra::crypto::RsaKey::westwood());
                let _ = tx.send(loaded);
            });
        match spawned {
            Ok(_) => self.ra = Ra::Loading(rx),
            Err(e) => eprintln!(
                "sf_audio: cannot start the Red Alert audio loader ({e}); using the CC0 sounds"
            ),
        }
    }

    pub fn original_audio_status(&self) -> RaStatus {
        match &self.ra {
            Ra::Off => RaStatus::Off,
            Ra::Loading(_) => RaStatus::Loading,
            Ra::Ready { events, .. } => RaStatus::Ready {
                mapped: events.len(),
            },
        }
    }

    /// What was found under the configured path (the sound browser).
    pub fn original_audio_catalog(&self) -> Option<Arc<Catalog>> {
        match &self.ra {
            Ra::Ready { catalog, .. } => Some(Arc::clone(catalog)),
            _ => None,
        }
    }

    /// Sound browser: decode one entry and play it (UI category, no
    /// position). Returns the decode error to show next to the entry.
    pub fn play_original_entry(&mut self, entry: &EntryInfo) -> Result<(), String> {
        let pcm = ra::decode_entry(entry)?;
        let Some(p) = &mut self.player else {
            return Ok(());
        };
        // One key: only the last-played browser sound stays in memory.
        let key = ra_key("browser");
        p.inject(key.clone(), pcm.to_static_sound());
        let outcome = p.play(&Play {
            name: "ra.browser",
            path: &key,
            gain: self.category_volume[Category::Ui.index()],
            pan: 0.0,
            rate: 1.0,
            max_instances: 4,
            looped: false,
        });
        self.record(Some(outcome));
        match outcome {
            Outcome::Failed => Err("the output refused the sound".into()),
            _ => Ok(()),
        }
    }

    fn poll_ra(&mut self) {
        let Ra::Loading(rx) = &self.ra else { return };
        let loaded = match rx.try_recv() {
            Ok(l) => l,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                eprintln!("sf_audio: the Red Alert audio loader stopped; using the CC0 sounds");
                self.ra = Ra::Off;
                return;
            }
        };
        if let Some(w) = &loaded.warning {
            eprintln!("sf_audio: {w}");
        }
        let mut events = BTreeSet::new();
        for (event, data) in loaded.sounds {
            if let Some(p) = &mut self.player {
                p.inject(ra_key(&event), data);
            }
            events.insert(event);
        }
        self.ra = Ra::Ready {
            catalog: Arc::new(loaded.catalog),
            events,
            music: loaded.music,
        };
        // The Red Alert tracks are known now: pick again (also after a default was chosen).
        self.music.current = None;
        self.music.failures = [0; 2];
    }

    // ---- helpers ----------------------------------------------------------

    /// The definition and file for a name: a mapped Red Alert sound first
    /// (with the bank's volume and category when the event has one), else
    /// the bank.
    fn resolve(&mut self, name: &str) -> Option<(SoundDef, PathBuf)> {
        if let Ra::Ready { events, .. } = &self.ra
            && events.contains(name)
        {
            let key = ra_key(name);
            let def = self
                .bank
                .get(name)
                .cloned()
                .unwrap_or_else(|| default_def(&key));
            return Some((def, key));
        }
        let def = self.lookup(name)?;
        let path = self.root.join(&def.file);
        Some((def, path))
    }

    fn lookup(&mut self, name: &str) -> Option<SoundDef> {
        match self.bank.get(name) {
            Some(d) => Some(d.clone()),
            None => {
                self.stats.unknown += 1;
                if self.warned.insert(name.to_string()) {
                    eprintln!("sf_audio: unknown sound '{name}'; skipping");
                }
                None
            }
        }
    }

    fn base_gain(&self, def: &SoundDef) -> f32 {
        def.volume * self.category_volume[def.category.index()]
    }

    /// Playback rate with the sound's random pitch variation applied.
    fn rate_for(&mut self, def: &SoundDef) -> f64 {
        if def.pitch_variation <= 0.0 {
            return 1.0;
        }
        // xorshift32 -> uniform in [-1, 1]
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        let unit = f64::from(x) / f64::from(u32::MAX) * 2.0 - 1.0;
        1.0 + unit * f64::from(def.pitch_variation)
    }

    fn record(&mut self, outcome: Option<Outcome>) {
        match outcome {
            Some(Outcome::Played) => self.stats.played += 1,
            Some(Outcome::Limited) => self.stats.limited += 1,
            Some(Outcome::Failed) => self.stats.failed += 1,
            Some(Outcome::AlreadyPlaying) | None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AudioConfig;
    use crate::ra::RaAudioConfig;
    use kira::backend::mock::{MockBackend, MockBackendSettings};

    fn assets() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
    }

    fn mock_system() -> AudioSystem {
        let settings = AudioManagerSettings::<MockBackend> {
            backend_settings: MockBackendSettings { sample_rate: 22050 },
            ..Default::default()
        };
        AudioSystem::with_player(assets(), Box::new(Live::new(settings).unwrap()))
    }

    /// A scratch asset root with a custom bank (and the click wav next to it).
    fn scratch(name: &str, bank: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sf_audio_{}_{name}", std::process::id()));
        std::fs::create_dir_all(d.join("audio/sfx")).unwrap();
        std::fs::copy(
            assets().join("audio/sfx/test_click.wav"),
            d.join("audio/sfx/test_click.wav"),
        )
        .unwrap();
        std::fs::write(d.join("audio/bank.ron"), bank).unwrap();
        d
    }

    #[test]
    fn has_sound_never_warns_and_the_trace_records_gain_and_pan() {
        let mut s = AudioSystem::silent(assets());
        assert!(s.has_sound("test_click"));
        assert!(!s.has_sound("weapon.fire.cannon"));
        assert_eq!(s.stats().unknown, 0, "asking is not playing");
        // Not tracing: nothing recorded.
        s.play_at("test_click", [1.0, 0.0, 1.0]);
        assert!(s.take_trace().is_empty());
        // Tracing works without a device and records what the output would get: base volume (0.8)
        // x the model's gain, and the model's pan.
        s.set_trace(true);
        let l = Listener::new([0.0; 3], [1.0, 0.0, 0.0], 120.0);
        s.set_listener(l);
        s.play_at("test_click", [30.0, 0.0, 0.0]);
        s.play_at("test_click", [900.0, 0.0, 0.0]); // inaudible: not recorded
        s.play_at("no_such_sound", [0.0; 3]); // unknown: not recorded
        let m = spatial::mix(&l, [30.0, 0.0, 0.0], &SpatialParams::default());
        assert_eq!(
            s.take_trace(),
            vec![PlayTrace {
                name: "test_click".into(),
                gain: 0.8 * m.gain,
                pan: m.pan,
            }]
        );
        assert!(s.take_trace().is_empty());
        s.set_trace(false);
        s.play_at("test_click", [30.0, 0.0, 0.0]);
        assert!(s.take_trace().is_empty());
    }

    #[test]
    fn loads_the_shipped_bank() {
        let sys = AudioSystem::silent(assets());
        assert!(sys.bank().get("test_click").is_some());
    }

    #[test]
    fn silent_system_accepts_the_whole_api() {
        let mut s = AudioSystem::silent(assets());
        assert!(s.is_silent());
        s.set_listener(Listener::default());
        s.set_spatial_params(SpatialParams::default());
        s.set_crossfade(Duration::from_secs(2));
        s.set_category_volume(Category::Music, 0.5);
        s.play("test_click");
        s.play_at("test_click", [1.0, 0.0, 2.0]);
        s.play("does_not_exist");
        s.play_music("test_click");
        s.play_music("test_click");
        s.play_music("does_not_exist");
        s.stop_music();
        s.update();
        assert!(s.reload_bank().is_empty());
        assert_eq!(s.current_music(), None);
        // Only the two unknown names are noticed (and warned about) without a device.
        assert_eq!(
            s.stats(),
            Stats {
                unknown: 2,
                ..Stats::default()
            }
        );
    }

    #[test]
    fn missing_bank_file_is_survivable() {
        let mut s = AudioSystem::silent("/no/such/assets");
        assert!(s.bank().sounds.is_empty());
        s.play("anything");
    }

    #[test]
    fn plays_with_the_mock_output_and_counts() {
        let mut s = mock_system();
        assert!(!s.is_silent());
        s.play("test_click");
        s.play_at("test_click", [0.0, 0.0, 0.0]);
        s.play("nope");
        // far away: dropped before a voice is used
        s.play_at("test_click", [1.0e6, 0.0, 0.0]);
        let st = s.stats();
        assert_eq!(st.played, 2);
        assert_eq!(st.unknown, 1);
        assert_eq!(st.inaudible, 1);
    }

    #[test]
    fn voice_limit_from_the_bank_applies() {
        let d = scratch(
            "limit",
            r#"(sounds: {"shot": (file: "audio/sfx/test_click.wav", max_instances: 2)})"#,
        );
        let settings = AudioManagerSettings::<MockBackend> {
            backend_settings: MockBackendSettings { sample_rate: 22050 },
            ..Default::default()
        };
        let mut s = AudioSystem::with_player(&d, Box::new(Live::new(settings).unwrap()));
        for _ in 0..100 {
            s.play("shot");
        }
        assert_eq!(s.stats().played, 2);
        assert_eq!(s.stats().limited, 98);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn missing_sound_file_is_a_failed_play_not_a_panic() {
        let d = scratch(
            "missing",
            r#"(sounds: {"ghost": (file: "audio/sfx/ghost.wav")})"#,
        );
        let settings = AudioManagerSettings::<MockBackend> {
            backend_settings: MockBackendSettings { sample_rate: 22050 },
            ..Default::default()
        };
        let mut s = AudioSystem::with_player(&d, Box::new(Live::new(settings).unwrap()));
        s.play("ghost");
        s.play_music("ghost");
        assert_eq!(s.stats().failed, 2);
        assert_eq!(s.current_music(), None);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn music_crossfade_through_the_system() {
        let mut s = mock_system();
        s.set_crossfade(Duration::from_millis(250));
        assert_eq!(s.crossfade(), Duration::from_millis(250));
        s.play_music("test_click");
        assert_eq!(s.current_music(), Some("test_click"));
        s.play_music("test_loop");
        assert_eq!(s.current_music(), Some("test_loop"));
        s.set_category_volume(Category::Music, 0.25);
        assert_eq!(s.category_volume(Category::Music), 0.25);
        s.stop_music();
        assert_eq!(s.current_music(), None);
    }

    #[test]
    fn reload_keeps_the_old_bank_on_a_parse_error() {
        let d = scratch(
            "reload",
            r#"(sounds: {"a": (file: "audio/sfx/test_click.wav")})"#,
        );
        let mut s = AudioSystem::silent(&d);
        assert!(s.bank().get("a").is_some());

        std::fs::write(d.join("audio/bank.ron"), "(sounds: {").unwrap();
        let w = s.reload_bank();
        assert_eq!(w.len(), 1);
        assert!(s.bank().get("a").is_some(), "old bank kept");

        std::fs::write(
            d.join("audio/bank.ron"),
            r#"(sounds: {"b": (file: "audio/sfx/test_click.wav")})"#,
        )
        .unwrap();
        assert!(s.on_asset_changed(&d.join("audio/bank.ron")));
        assert!(s.bank().get("a").is_none() && s.bank().get("b").is_some());
        assert!(!s.on_asset_changed(&d.join("config/input.ron")));
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn pitch_variation_stays_in_range_and_varies() {
        let mut s = AudioSystem::silent(assets());
        let def = SoundDef {
            file: "x.wav".into(),
            volume: 1.0,
            pitch_variation: 0.1,
            max_instances: 1,
            category: Category::Sfx,
            looped: false,
        };
        let rates: Vec<f64> = (0..200).map(|_| s.rate_for(&def)).collect();
        assert!(rates.iter().all(|r| (0.9..=1.1).contains(r)));
        assert!(rates.iter().any(|r| *r > 1.02) && rates.iter().any(|r| *r < 0.98));
        let steady = SoundDef {
            pitch_variation: 0.0,
            ..def
        };
        assert_eq!(s.rate_for(&steady), 1.0);
    }

    #[test]
    fn device_probe_never_panics() {
        // Real cpal: no device in CI/containers -> silent; with one -> live.
        let s = AudioSystem::new(assets());
        let _ = s.is_silent();
    }

    /// Wait (in a test) until the background load is done.
    fn wait_for_ra(s: &mut AudioSystem) {
        for _ in 0..500 {
            s.update();
            if s.original_audio_status() != RaStatus::Loading {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("Red Alert audio loader did not finish");
    }

    /// A scratch asset root with a mapping and a fake Red Alert install
    /// built from our own encoder and MIX writer.
    fn ra_root(name: &str) -> (PathBuf, PathBuf) {
        use crate::ra::aud::{Compression, Pcm, encode};
        use crate::ra::mix::{MixFormat, mix_id, write_mix};
        let d = scratch(
            name,
            r#"(sounds: {"ui.click": (file: "audio/sfx/test_click.wav", category: Ui)})"#,
        );
        std::fs::write(
            d.join("audio/ra_mapping.ron"),
            r#"(sounds: {"ui.click": "CLICK.AUD", "unit.move": "MOVE.AUD", "unit.attack": "GONE.AUD"})"#,
        )
        .unwrap();
        let pcm = Pcm {
            sample_rate: 22050,
            channels: 1,
            samples: (0..2000).map(|i| ((i % 50) * 400 - 10000) as i16).collect(),
        };
        let aud = encode(&pcm, Compression::Ima);
        let mix = write_mix(
            &[(mix_id("CLICK.AUD"), &aud), (mix_id("MOVE.AUD"), &aud)],
            MixFormat::Plain,
        );
        let install = d.join("redalert");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("SOUNDS.MIX"), mix).unwrap();
        (d, install)
    }

    fn mock_at(root: &Path) -> AudioSystem {
        let settings = AudioManagerSettings::<MockBackend> {
            backend_settings: MockBackendSettings { sample_rate: 22050 },
            ..Default::default()
        };
        AudioSystem::with_player(root, Box::new(Live::new(settings).unwrap()))
    }

    #[test]
    fn original_audio_replaces_mapped_events_and_falls_back_for_the_rest() {
        let (d, install) = ra_root("ra_on");
        let mut s = mock_at(&d);
        s.enable_original_audio(&RaAudioConfig {
            enabled: true,
            path: install.display().to_string(),
        });
        wait_for_ra(&mut s);
        assert_eq!(s.original_audio_status(), RaStatus::Ready { mapped: 2 });
        // Mapped events have a sound for the positional planner (WP11.3), mapped-but-missing and unmapped do not.
        assert!(s.has_sound("unit.move") && s.has_sound("ui.click"));
        assert!(!s.has_sound("unit.attack") && !s.has_sound("weapon.fire.cannon"));
        // Mapped, with and without a CC0 entry: both play.
        s.play("ui.click");
        s.play("unit.move");
        assert_eq!(s.stats().played, 2);
        assert_eq!(s.stats().unknown, 0);
        // Mapped but missing in the install, no CC0 sound: silence.
        s.play("unit.attack");
        assert_eq!(s.stats().unknown, 1);
        // The browser plays any entry.
        let cat = s.original_audio_catalog().unwrap();
        let entry = cat.find("MOVE.AUD").unwrap().clone();
        assert!(s.play_original_entry(&entry).is_ok());
        assert_eq!(s.stats().played, 3);
        let mut broken = entry.clone();
        broken.size = 10;
        assert!(s.play_original_entry(&broken).is_err());
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn original_audio_with_a_missing_path_keeps_the_cc0_sounds() {
        let (d, _) = ra_root("ra_missing");
        let mut s = mock_at(&d);
        s.enable_original_audio(&RaAudioConfig {
            enabled: true,
            path: "/nonexistent".into(),
        });
        wait_for_ra(&mut s);
        assert_eq!(s.original_audio_status(), RaStatus::Ready { mapped: 0 });
        s.play("ui.click");
        assert_eq!(s.stats().played, 1, "CC0 click");
        // Disabled or no path: nothing starts.
        s.enable_original_audio(&RaAudioConfig::default());
        assert_eq!(s.original_audio_status(), RaStatus::Off);
        s.enable_original_audio(&RaAudioConfig {
            enabled: true,
            path: " ".into(),
        });
        assert_eq!(s.original_audio_status(), RaStatus::Off);
        // Silent backend: loads, and the API stays a no-op.
        let mut q = AudioSystem::silent(&d);
        q.enable_original_audio(&RaAudioConfig {
            enabled: true,
            path: d.join("redalert").display().to_string(),
        });
        wait_for_ra(&mut q);
        q.play("unit.move");
        assert_eq!(q.stats(), Stats::default());
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn original_audio_from_a_wav_folder_wins_over_the_mix() {
        use crate::ra::aud::Pcm;
        use crate::ra::{EntryKind, wav};
        let (d, install) = ra_root("ra_wav");
        // A WAV folder as sf_ra_extract writes it, next to the MIX: CLICK by
        // name, MOVE.AUD by its raw id.
        let folder = install.join("RedAlertSounds").join("SOUNDS.MIX");
        std::fs::create_dir_all(&folder).unwrap();
        let pcm = Pcm {
            sample_rate: 11025,
            channels: 2,
            samples: (0..400).map(|i| (i * 37 % 2000) as i16).collect(),
        };
        std::fs::write(folder.join("CLICK.wav"), wav::write(&pcm)).unwrap();
        let move_id = crate::ra::mix::mix_id("MOVE.AUD");
        std::fs::write(folder.join(format!("#{move_id:08X}.wav")), wav::write(&pcm)).unwrap();
        std::fs::write(folder.join("BAD.wav"), b"RIFF junk").unwrap();
        // Header says more samples than the file holds: listed as broken.
        std::fs::write(folder.join("CUT.wav"), &wav::write(&pcm)[..100]).unwrap();
        // Chunks before `data` that do not fit the 64 KiB scan window.
        let plain = wav::write(&pcm);
        let mut big = plain[..36].to_vec();
        big.extend_from_slice(b"LIST");
        big.extend_from_slice(&70_000u32.to_le_bytes());
        big.resize(big.len() + 70_000, 0);
        big.extend_from_slice(&plain[36..]);
        std::fs::write(folder.join("BIGHDR.wav"), &big).unwrap();
        let mut s = mock_at(&d);
        s.enable_original_audio(&RaAudioConfig {
            enabled: true,
            path: install.display().to_string(),
        });
        wait_for_ra(&mut s);
        assert_eq!(s.original_audio_status(), RaStatus::Ready { mapped: 2 });
        let cat = s.original_audio_catalog().unwrap();
        let wav_dir = cat
            .archives
            .iter()
            .find(|a| a.label == "RedAlertSounds/SOUNDS.MIX (WAV folder)")
            .expect("WAV folder listed in the browser");
        assert_eq!(wav_dir.entries.len(), 5);
        let bighdr = wav_dir
            .entries
            .iter()
            .find(|e| e.path.ends_with("BIGHDR.wav"))
            .unwrap();
        assert!(
            matches!(&bighdr.kind, EntryKind::Broken(m) if m == "WAV header larger than 64 KiB"),
            "{:?}",
            bighdr.kind
        );
        for name in ["CLICK.AUD", "MOVE.AUD"] {
            let e = cat.find(name).unwrap();
            assert!(
                matches!(
                    e.kind,
                    EntryKind::Wav {
                        sample_rate: 11025,
                        channels: 2,
                        ..
                    }
                ),
                "{name}"
            );
            assert_eq!(crate::ra::decode_entry(e).unwrap(), pcm);
        }
        assert!(
            wav_dir
                .entries
                .iter()
                .any(|e| matches!(e.kind, EntryKind::Broken(_)))
        );
        s.play("unit.move");
        assert_eq!(s.stats().played, 1);

        // Only the WAV folder (the MIX gone): still loads.
        std::fs::remove_file(install.join("SOUNDS.MIX")).ok();
        let mut q = mock_at(&d);
        q.enable_original_audio(&RaAudioConfig {
            enabled: true,
            path: install.join("RedAlertSounds").display().to_string(),
        });
        wait_for_ra(&mut q);
        assert_eq!(q.original_audio_status(), RaStatus::Ready { mapped: 2 });
        std::fs::remove_dir_all(d).ok();
    }

    // ---- dynamic music (WP11.4) ---------------------------------------------

    use crate::music::Mood;

    const THEME_BANK: &str = r#"(sounds: {
        "calm_theme": (file: "audio/sfx/test_click.wav", category: Music, looped: true),
        "combat_theme": (file: "audio/sfx/test_click.wav", category: Music, looped: true),
    })"#;

    /// A scratch root with a mapping and `MAIN.MIX` > `SCORES.MIX` holding the
    /// tracks `ZZTRACK1..n` (made up), `bad` of them damaged (long, cut short).
    fn music_root(name: &str, mapping: &str, tracks: usize, bad: bool) -> (PathBuf, PathBuf) {
        use crate::ra::aud::{Compression, Pcm, encode};
        use crate::ra::mix::{MixFormat, mix_id, write_mix};
        let d = scratch(name, THEME_BANK);
        std::fs::write(d.join("audio/ra_mapping.ron"), mapping).unwrap();
        let pcm = |frames: usize| Pcm {
            sample_rate: 22050,
            channels: 1,
            samples: (0..frames)
                .map(|i| ((i % 50) * 400) as i16 - 10000)
                .collect(),
        };
        let good = encode(&pcm(3000), Compression::Ima);
        let mut long = encode(&pcm(700_000), Compression::Ima);
        long.truncate(crate::ra::LARGE_AUD_BYTES as usize + 1000);
        let names: Vec<String> = (1..=tracks).map(|i| format!("ZZTRACK{i}.AUD")).collect();
        let files: Vec<(u32, &[u8])> = names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let data: &[u8] = if bad && i == 0 { &long } else { &good };
                (mix_id(n), data)
            })
            .collect();
        let scores = write_mix(&files, MixFormat::Plain);
        let install = d.join("redalert");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(
            install.join("MAIN.MIX"),
            write_mix(&[(mix_id("SCORES.MIX"), &scores)], MixFormat::Plain),
        )
        .unwrap();
        (d, install)
    }

    fn ra_on(s: &mut AudioSystem, install: &Path) {
        s.enable_original_audio(&RaAudioConfig {
            enabled: true,
            path: install.display().to_string(),
        });
        wait_for_ra(s);
    }

    /// Call the mood logic until it has started a track (the decode runs on a
    /// background thread).
    fn settle(s: &mut AudioSystem, mood: Mood) {
        for _ in 0..500 {
            s.set_music_mood(mood);
            s.update();
            if s.music.pending.is_none() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the music track did not load");
    }

    fn shipped_music() -> crate::music::MusicConfig {
        AudioConfig::embedded().music
    }

    #[test]
    fn without_red_alert_files_the_music_is_silent_without_a_warning_or_error() {
        let (d, _) = music_root("music_off", "(sounds: {})", 3, false);
        let mut s = mock_at(&d);
        s.set_music_config(&shipped_music());
        s.set_music_trace(true);
        for m in [Mood::Calm, Mood::Combat, Mood::Calm] {
            s.set_music_mood(m);
            s.update();
        }
        assert_eq!(s.current_music(), None);
        assert!(s.take_music_trace().is_empty());
        // The path is wrong: the loader finds nothing (its one warning), the music stays silent.
        s.enable_original_audio(&RaAudioConfig {
            enabled: true,
            path: d.join("nowhere").display().to_string(),
        });
        wait_for_ra(&mut s);
        s.set_music_mood(Mood::Combat);
        s.update();
        assert_eq!(s.current_music(), None);
        assert!(s.take_music_trace().is_empty());
        // Silent backend takes the same calls.
        let mut q = AudioSystem::silent(&d);
        q.set_music_mood(Mood::Combat);
        q.update();
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn the_default_music_of_the_config_plays_when_there_is_no_red_alert_track() {
        let (d, _) = music_root("music_default", "(sounds: {})", 1, false);
        let mut s = mock_at(&d);
        let mut c = shipped_music();
        c.fallback.calm = Some("calm_theme".into());
        c.fallback.combat = Some("combat_theme".into());
        s.set_music_config(&c);
        s.set_music_trace(true);
        s.set_music_mood(Mood::Calm);
        assert_eq!(s.current_music(), Some("calm_theme"));
        s.set_music_mood(Mood::Calm); // nothing new
        s.set_music_mood(Mood::Combat);
        assert_eq!(s.current_music(), Some("combat_theme"));
        assert_eq!(
            s.take_music_trace(),
            vec![
                MusicTrace {
                    mood: Mood::Calm,
                    track: "calm_theme".into()
                },
                MusicTrace {
                    mood: Mood::Combat,
                    track: "combat_theme".into()
                },
            ]
        );
        // A name the bank does not have is silence, not an error.
        c.fallback.calm = Some("not_in_the_bank".into());
        s.set_music_config(&c);
        s.set_music_mood(Mood::Calm);
        assert_eq!(s.current_music(), None, "the combat theme fades to silence");
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn red_alert_music_shuffles_the_archive_tracks_and_switches_on_the_mood() {
        let (d, install) = music_root("music_ra", "(sounds: {})", 3, false);
        let mut s = mock_at(&d);
        s.set_music_config(&shipped_music());
        s.set_music_trace(true);
        ra_on(&mut s, &install);
        settle(&mut s, Mood::Calm);
        let first = s.current_music().expect("a track plays").to_string();
        assert!(
            first.starts_with('#') || first.starts_with("ZZTRACK"),
            "{first}"
        );
        let load = s.last_music_load().expect("the load was measured");
        assert_eq!(load.frames, 3000);
        // Calling every frame changes nothing.
        for _ in 0..20 {
            s.set_music_mood(Mood::Calm);
            s.update();
        }
        assert_eq!(s.take_music_trace().len(), 1);
        // Combat: another track (the combat pool has its own shuffle) crossfades in.
        settle(&mut s, Mood::Combat);
        let second = s.current_music().unwrap().to_string();
        let t = s.take_music_trace();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].mood, Mood::Combat);
        assert_eq!(t[0].track, second);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn a_damaged_track_is_skipped_with_one_warning_and_the_mood_falls_back() {
        // One mapped track, damaged: it fails when decoded, the mood uses the default music.
        let (d, install) = music_root(
            "music_bad",
            r#"(sounds: {}, music: (calm: ["ZZTRACK1"]))"#,
            2,
            true,
        );
        let mut s = mock_at(&d);
        let mut c = shipped_music();
        c.fallback.calm = Some("calm_theme".into());
        s.set_music_config(&c);
        ra_on(&mut s, &install);
        for _ in 0..500 {
            s.set_music_mood(Mood::Calm);
            s.update();
            if s.current_music() == Some("calm_theme") {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(s.current_music(), Some("calm_theme"));
        assert_eq!(s.music.warned.len(), 1, "warned once");
        // The combat mood (empty list = both tracks of the archive) is not affected.
        settle(&mut s, Mood::Combat);
        assert!(
            s.current_music().unwrap().contains("ZZTRACK")
                || s.current_music().unwrap().starts_with('#')
        );
        std::fs::remove_dir_all(d).ok();
    }

    /// A player whose music "ends" when the test says so.
    struct Fake {
        name: Option<String>,
        ended: Arc<std::sync::atomic::AtomicBool>,
        started: Arc<std::sync::Mutex<Vec<String>>>,
        gains: Arc<std::sync::Mutex<Vec<f32>>>,
    }

    fn fake(
        ended: &Arc<std::sync::atomic::AtomicBool>,
        started: &Arc<std::sync::Mutex<Vec<String>>>,
        gains: &Arc<std::sync::Mutex<Vec<f32>>>,
    ) -> Box<Fake> {
        Box::new(Fake {
            name: None,
            ended: Arc::clone(ended),
            started: Arc::clone(started),
            gains: Arc::clone(gains),
        })
    }

    impl Player for Fake {
        fn play(&mut self, _: &Play) -> Outcome {
            Outcome::Played
        }
        fn play_music(&mut self, p: &Play, _: Duration) -> Outcome {
            self.name = Some(p.name.to_string());
            self.ended.store(false, std::sync::atomic::Ordering::SeqCst);
            self.started.lock().unwrap().push(p.name.to_string());
            Outcome::Played
        }
        fn stop_music(&mut self, _: Duration) {
            self.name = None;
        }
        fn set_music_gain(&mut self, g: f32) {
            self.gains.lock().unwrap().push(g);
        }
        fn music_name(&self) -> Option<&str> {
            if self.ended.load(std::sync::atomic::Ordering::SeqCst) {
                None
            } else {
                self.name.as_deref()
            }
        }
        fn tick(&mut self) {}
        fn clear_cache(&mut self) {}
        fn inject(&mut self, _: PathBuf, _: StaticSoundData) {}
    }

    #[test]
    fn when_a_track_ends_the_next_one_of_the_mood_starts() {
        let (d, install) = music_root("music_next", "(sounds: {})", 3, false);
        let ended = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let started = Arc::new(std::sync::Mutex::new(Vec::new()));
        let gains = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut s = AudioSystem::with_player(&d, fake(&ended, &started, &gains));
        ra_on(&mut s, &install);
        for _ in 0..6 {
            settle(&mut s, Mood::Calm);
            ended.store(true, std::sync::atomic::Ordering::SeqCst); // the track ends
        }
        let seen = started.lock().unwrap().clone();
        assert_eq!(seen.len(), 6, "{seen:?}");
        // No immediate repeat, and each round of three plays every track once.
        assert!(seen.windows(2).all(|w| w[0] != w[1]), "{seen:?}");
        for round in seen.chunks(3) {
            let mut r = round.to_vec();
            r.sort();
            r.dedup();
            assert_eq!(r.len(), 3, "{seen:?}");
        }
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn a_silent_system_records_the_choice_without_decoding() {
        let (d, install) = music_root("music_silent", "(sounds: {})", 3, false);
        let mut s = AudioSystem::silent(&d);
        s.set_music_trace(true);
        ra_on(&mut s, &install);
        s.set_music_mood(Mood::Calm);
        s.set_music_mood(Mood::Combat);
        let t = s.take_music_trace();
        assert_eq!(t.len(), 2);
        assert_eq!((t[0].mood, t[1].mood), (Mood::Calm, Mood::Combat));
        assert!(s.last_music_load().is_none(), "nothing was decoded");
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn music_volume_reaches_a_running_red_alert_track() {
        let (d, install) = music_root("music_vol", "(sounds: {})", 1, false);
        let ended = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let started = Arc::new(std::sync::Mutex::new(Vec::new()));
        let gains = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut s = AudioSystem::with_player(&d, fake(&ended, &started, &gains));
        ra_on(&mut s, &install);
        settle(&mut s, Mood::Calm);
        assert!(s.music.ra_track);
        assert_eq!(started.lock().unwrap().len(), 1);
        s.set_category_volume(Category::Music, 0.25);
        assert_eq!(
            *gains.lock().unwrap(),
            vec![0.25],
            "the running track got the gain"
        );
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn a_red_alert_track_fades_to_silence_when_the_new_mood_has_none() {
        // One track, mapped to calm only; combat names a missing track, no fallback.
        let mapping = "(sounds: {}, music: (calm: [\"ZZTRACK1\"], combat: [\"NOT_THERE\"]))";
        let (d, install) = music_root("music_to_silence", mapping, 1, false);
        let mut s = mock_at(&d);
        ra_on(&mut s, &install);
        settle(&mut s, Mood::Calm);
        assert!(s.music.ra_track);
        assert!(s.current_music().is_some());
        settle(&mut s, Mood::Combat);
        assert_eq!(
            s.current_music(),
            None,
            "the calm track must not play through combat"
        );
        assert!(!s.music.ra_track);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn production_reseeding_changes_the_order_tests_stay_fixed() {
        let a = AudioSystem::silent(assets());
        let b = AudioSystem::silent(assets());
        let mut x = a.music.shuffle[0].clone();
        let mut y = b.music.shuffle[0].clone();
        assert_eq!(x.next(8), y.next(8), "fixed seeds without reseeding");
        let mut c = AudioSystem::silent(assets());
        let order = |s: &AudioSystem| {
            let mut sh = s.music.shuffle[0].clone();
            (0..8).map(|_| sh.next(8)).collect::<Vec<_>>()
        };
        let base = order(&c);
        let differs = (1..20u64).any(|k| {
            c.reseed_music(k * 0x1234_5678_9ABC);
            order(&c) != base
        });
        assert!(differs);
    }

    /// Measurement for the PR (not run in CI): three 3-minute stereo 22.05 kHz tracks in a fake SCORES.MIX.
    /// `cargo test --release -p sf_audio music_switch_cost -- --ignored --nocapture`
    #[test]
    #[ignore = "measurement, prints numbers"]
    fn music_switch_cost() {
        use crate::ra::aud::{Compression, Pcm, encode};
        use crate::ra::mix::{MixFormat, mix_id, write_mix};
        let d = scratch("music_cost", THEME_BANK);
        std::fs::write(d.join("audio/ra_mapping.ron"), "(sounds: {})").unwrap();
        let frames = 22050 * 180;
        let pcm = Pcm {
            sample_rate: 22050,
            channels: 2,
            samples: (0..frames * 2)
                .map(|i| (((i / 2) % 97) * 300) as i16 - 14000)
                .collect(),
        };
        let aud = encode(&pcm, Compression::Ima);
        let scores = write_mix(
            &[
                (mix_id("ZZTRACK1.AUD"), &aud),
                (mix_id("ZZTRACK2.AUD"), &aud),
                (mix_id("ZZTRACK3.AUD"), &aud),
            ],
            MixFormat::Plain,
        );
        let install = d.join("redalert");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(
            install.join("MAIN.MIX"),
            write_mix(&[(mix_id("SCORES.MIX"), &scores)], MixFormat::Plain),
        )
        .unwrap();
        println!("track file: {} KiB compressed", aud.len() / 1024);
        let mut s = mock_at(&d);
        s.set_music_config(&shipped_music());
        let t = Instant::now();
        ra_on(&mut s, &install);
        println!(
            "scan + load of the archive (3 tracks, none decoded): {:?}",
            t.elapsed()
        );
        let mut worst = Duration::ZERO;
        let mut calls = 0;
        for mood in [Mood::Calm, Mood::Combat, Mood::Calm] {
            let started = Instant::now();
            let mut waited = 0;
            loop {
                let t = Instant::now();
                s.set_music_mood(mood);
                s.update();
                worst = worst.max(t.elapsed());
                calls += 1;
                if s.music.pending.is_none() {
                    break;
                }
                waited += 1;
                std::thread::sleep(Duration::from_millis(1));
            }
            let l = s.last_music_load().unwrap();
            println!(
                "switch to {}: track ready after {:?} ({} frames, decode thread {:?}, {:.1} MiB in memory, {} frames of waiting)",
                mood.name(),
                started.elapsed(),
                l.frames,
                l.decode,
                l.frames as f64 * 8.0 / 1048576.0,
                waited
            );
        }
        println!(
            "worst main-thread cost of one set_music_mood + update call over {calls} calls: {worst:?}"
        );
        std::fs::remove_dir_all(d).ok();
    }

    /// Writes a fake install (`MAIN.MIX` > `SCORES.MIX`, three 20-second tones, nothing of Red Alert) to the
    /// directory in `SF_MUSIC_FIXTURE`, to try `sf_game --ra-audio <dir>` by hand.
    #[test]
    #[ignore = "writes a fixture for a manual run"]
    fn write_music_fixture_for_a_manual_run() {
        use crate::ra::aud::{Compression, Pcm, encode};
        use crate::ra::mix::{MixFormat, mix_id, write_mix};
        let dir = PathBuf::from(std::env::var("SF_MUSIC_FIXTURE").expect("SF_MUSIC_FIXTURE"));
        std::fs::create_dir_all(&dir).unwrap();
        let tone = |period: usize| {
            let pcm = Pcm {
                sample_rate: 22050,
                channels: 2,
                samples: (0..22050 * 20 * 2)
                    .map(|i| {
                        if (i / 2 / period).is_multiple_of(2) {
                            6000
                        } else {
                            -6000
                        }
                    })
                    .collect(),
            };
            encode(&pcm, Compression::Ima)
        };
        let (a, b, c) = (tone(60), tone(40), tone(25));
        let scores = write_mix(
            &[
                (mix_id("ZZTRACK1.AUD"), &a),
                (mix_id("ZZTRACK2.AUD"), &b),
                (mix_id("ZZTRACK3.AUD"), &c),
            ],
            MixFormat::Plain,
        );
        std::fs::write(
            dir.join("MAIN.MIX"),
            write_mix(&[(mix_id("SCORES.MIX"), &scores)], MixFormat::Plain),
        )
        .unwrap();
    }
}
