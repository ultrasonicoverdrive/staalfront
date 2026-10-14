//! [`AudioSystem`]: the public face of the crate.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kira::{AudioManagerSettings, DefaultBackend};

use crate::bank::{Bank, Category, SoundDef};
use crate::live::{Live, Outcome, Play, Player};
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
        sys
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

    pub fn set_spatial_params(&mut self, params: SpatialParams) {
        self.params = params;
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
        if let Some(p) = &mut self.player
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

    fn play_with(&mut self, name: &str, pos: Option<[f32; 3]>) {
        if self.player.is_none() {
            return;
        }
        let Some(def) = self.lookup(name) else { return };
        let (mut gain, mut pan) = (self.base_gain(&def), 0.0);
        if let Some(pos) = pos {
            let m = spatial::mix(&self.listener, pos, &self.params);
            if !m.is_audible() {
                self.stats.inaudible += 1;
                return;
            }
            gain *= m.gain;
            pan = m.pan;
        }
        let rate = self.rate_for(&def);
        let path = self.root.join(&def.file);
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
        if self.player.is_none() {
            return;
        }
        let Some(def) = self.lookup(name) else { return };
        let gain = self.base_gain(&def);
        let path = self.root.join(&def.file);
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

    /// Per-frame housekeeping (releases finished voices).
    pub fn update(&mut self) {
        if let Some(p) = &mut self.player {
            p.tick();
        }
    }

    // ---- helpers ----------------------------------------------------------

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
        s.stop_music();
        s.update();
        assert!(s.reload_bank().is_empty());
        assert_eq!(s.current_music(), None);
        assert_eq!(s.stats(), Stats::default());
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
}
