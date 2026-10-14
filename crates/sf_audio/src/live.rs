//! The kira-backed player. Generic over the kira backend so tests can run
//! the whole path against kira's `MockBackend` without a sound device.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use kira::backend::Backend;
use kira::sound::PlaybackState;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::{AudioManager, AudioManagerSettings, Decibels, Panning, Tween};

use crate::voices::{DEFAULT_MAX_TOTAL, Refused, VoiceLimiter};

/// A fully resolved play request (bank, category and spatial mix applied).
pub(crate) struct Play<'a> {
    pub name: &'a str,
    pub path: &'a Path,
    /// Final linear amplitude.
    pub gain: f32,
    pub pan: f32,
    /// Playback rate, `1.0` = original pitch.
    pub rate: f64,
    pub max_instances: u32,
    pub looped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Played,
    /// Dropped by voice limiting.
    Limited,
    /// File could not be loaded or kira refused the sound.
    Failed,
    /// Music: this track is already playing.
    AlreadyPlaying,
}

/// What [`crate::AudioSystem`] needs from an output.
pub(crate) trait Player {
    fn play(&mut self, play: &Play) -> Outcome;
    fn play_music(&mut self, play: &Play, crossfade: Duration) -> Outcome;
    fn stop_music(&mut self, fade_out: Duration);
    /// New final amplitude for the running music track (category volume change).
    fn set_music_gain(&mut self, gain: f32);
    /// Name of the running music track, if any.
    fn music_name(&self) -> Option<&str>;
    /// Housekeeping once per frame: forget finished voices.
    fn tick(&mut self);
    /// Drop decoded files (after a bank reload) so edits are picked up.
    fn clear_cache(&mut self);
}

pub(crate) fn amplitude_to_db(a: f32) -> Decibels {
    if a.is_finite() && a > 0.001 {
        Decibels(20.0 * a.log10())
    } else {
        Decibels::SILENCE
    }
}

fn fade(d: Duration) -> Tween {
    Tween {
        duration: d,
        ..Tween::default()
    }
}

struct Music {
    name: String,
    handle: StaticSoundHandle,
}

pub(crate) struct Live<B: Backend> {
    manager: AudioManager<B>,
    /// Decoded files by path; `None` = failed once (not retried until the cache is cleared).
    cache: BTreeMap<PathBuf, Option<StaticSoundData>>,
    warned: BTreeSet<PathBuf>,
    voices: VoiceLimiter<StaticSoundHandle>,
    music: Option<Music>,
}

impl<B: Backend> Live<B> {
    pub fn new(settings: AudioManagerSettings<B>) -> Result<Self, B::Error> {
        Ok(Self {
            manager: AudioManager::new(settings)?,
            cache: BTreeMap::new(),
            warned: BTreeSet::new(),
            voices: VoiceLimiter::new(DEFAULT_MAX_TOTAL),
            music: None,
        })
    }

    #[cfg(test)]
    pub fn manager_mut(&mut self) -> &mut AudioManager<B> {
        &mut self.manager
    }

    #[cfg(test)]
    pub fn live_voices(&self, name: &str) -> usize {
        self.voices.count(name)
    }

    fn data(&mut self, path: &Path) -> Option<StaticSoundData> {
        if !self.cache.contains_key(path) {
            let loaded = match StaticSoundData::from_file(path) {
                Ok(d) => Some(d),
                Err(e) => {
                    if self.warned.insert(path.to_path_buf()) {
                        eprintln!("sf_audio: cannot load '{}': {e}; skipping", path.display());
                    }
                    None
                }
            };
            self.cache.insert(path.to_path_buf(), loaded);
        }
        self.cache.get(path).cloned().flatten()
    }

    fn configured(data: StaticSoundData, p: &Play, fade_in: Option<Tween>) -> StaticSoundData {
        let data = data
            .volume(amplitude_to_db(p.gain))
            .panning(Panning(p.pan.clamp(-1.0, 1.0)))
            .playback_rate(p.rate)
            .fade_in_tween(fade_in);
        if p.looped { data.loop_region(..) } else { data }
    }
}

impl<B: Backend> Player for Live<B> {
    fn play(&mut self, p: &Play) -> Outcome {
        self.voices.prune(|h| h.state() != PlaybackState::Stopped);
        match self.voices.check(p.name, p.max_instances) {
            Ok(()) => {}
            Err(Refused::SoundFull | Refused::TotalFull) => return Outcome::Limited,
        }
        let Some(data) = self.data(p.path) else {
            return Outcome::Failed;
        };
        match self.manager.play(Self::configured(data, p, None)) {
            Ok(handle) => {
                self.voices.add(p.name, handle);
                Outcome::Played
            }
            Err(e) => {
                eprintln!("sf_audio: cannot play '{}': {e}", p.name);
                Outcome::Failed
            }
        }
    }

    fn play_music(&mut self, p: &Play, crossfade: Duration) -> Outcome {
        if let Some(m) = &self.music
            && m.name == p.name
            && m.handle.state() != PlaybackState::Stopped
            && m.handle.state() != PlaybackState::Stopping
        {
            return Outcome::AlreadyPlaying;
        }
        let Some(data) = self.data(p.path) else {
            return Outcome::Failed; // keep the current track running
        };
        let tween = fade(crossfade);
        let fade_in = (!crossfade.is_zero() && self.music.is_some()).then_some(tween);
        match self.manager.play(Self::configured(data, p, fade_in)) {
            Ok(handle) => {
                if let Some(mut old) = self.music.take() {
                    old.handle.stop(tween);
                }
                self.music = Some(Music {
                    name: p.name.to_string(),
                    handle,
                });
                Outcome::Played
            }
            Err(e) => {
                eprintln!("sf_audio: cannot play music '{}': {e}", p.name);
                Outcome::Failed
            }
        }
    }

    fn stop_music(&mut self, fade_out: Duration) {
        if let Some(mut m) = self.music.take() {
            m.handle.stop(fade(fade_out));
        }
    }

    fn set_music_gain(&mut self, gain: f32) {
        if let Some(m) = &mut self.music {
            m.handle
                .set_volume(amplitude_to_db(gain), fade(Duration::from_millis(50)));
        }
    }

    fn music_name(&self) -> Option<&str> {
        self.music.as_ref().map(|m| m.name.as_str())
    }

    fn tick(&mut self) {
        self.voices.prune(|h| h.state() != PlaybackState::Stopped);
        if self
            .music
            .as_ref()
            .is_some_and(|m| m.handle.state() == PlaybackState::Stopped)
        {
            self.music = None;
        }
    }

    fn clear_cache(&mut self) {
        self.cache.clear();
        self.warned.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kira::backend::mock::{MockBackend, MockBackendSettings};

    fn click() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/audio/sfx/test_click.wav")
    }

    fn live() -> Live<MockBackend> {
        let settings = AudioManagerSettings::<MockBackend> {
            backend_settings: MockBackendSettings { sample_rate: 22050 },
            ..Default::default()
        };
        Live::new(settings).unwrap()
    }

    /// Run the mock mixer for `frames` internal buffers.
    fn run(l: &mut Live<MockBackend>, buffers: usize) {
        for _ in 0..buffers {
            l.manager_mut().backend_mut().on_start_processing();
            l.manager_mut().backend_mut().process();
        }
    }

    fn req<'a>(name: &'a str, path: &'a Path, max: u32) -> Play<'a> {
        Play {
            name,
            path,
            gain: 0.5,
            pan: 0.2,
            rate: 1.0,
            max_instances: max,
            looped: false,
        }
    }

    #[test]
    fn db_conversion() {
        assert_eq!(amplitude_to_db(1.0), Decibels::IDENTITY);
        assert!((amplitude_to_db(0.5).0 + 6.0206).abs() < 1e-3);
        assert_eq!(amplitude_to_db(0.0), Decibels::SILENCE);
        assert_eq!(amplitude_to_db(f32::NAN), Decibels::SILENCE);
    }

    #[test]
    fn plays_the_shipped_sound_and_limits_voices() {
        let path = click();
        let mut l = live();
        let outcomes: Vec<_> = (0..10).map(|_| l.play(&req("click", &path, 3))).collect();
        assert_eq!(
            outcomes.iter().filter(|o| **o == Outcome::Played).count(),
            3
        );
        assert_eq!(
            outcomes.iter().filter(|o| **o == Outcome::Limited).count(),
            7
        );
        assert_eq!(l.live_voices("click"), 3);

        // The click is 0.15 s = 3307 frames at 22.05 kHz; render it all.
        run(&mut l, 40);
        l.tick();
        assert_eq!(l.live_voices("click"), 0, "finished voices are released");
        assert_eq!(l.play(&req("click", &path, 3)), Outcome::Played);
    }

    #[test]
    fn missing_file_fails_softly_and_only_warns_once() {
        let mut l = live();
        let bad = Path::new("/no/such/file.wav");
        assert_eq!(l.play(&req("x", bad, 1)), Outcome::Failed);
        assert_eq!(l.play(&req("x", bad, 1)), Outcome::Failed);
        assert_eq!(l.warned.len(), 1);
        assert_eq!(
            l.play_music(&req("m", bad, 1), Duration::from_secs(1)),
            Outcome::Failed
        );
    }

    #[test]
    fn music_crossfades_and_stops() {
        let path = click();
        let mut l = live();
        let fade = Duration::from_millis(500);
        assert_eq!(l.play_music(&req("a", &path, 1), fade), Outcome::Played);
        assert_eq!(l.music_name(), Some("a"));
        assert_eq!(
            l.play_music(&req("a", &path, 1), fade),
            Outcome::AlreadyPlaying
        );

        assert_eq!(l.play_music(&req("b", &path, 1), fade), Outcome::Played);
        assert_eq!(l.music_name(), Some("b"));
        // The mixer keeps running with both tracks mid-fade.
        run(&mut l, 10);
        l.set_music_gain(0.3);
        run(&mut l, 10);
        l.stop_music(fade);
        assert_eq!(l.music_name(), None);
        run(&mut l, 10);
        l.stop_music(fade); // stopping nothing is fine
    }

    #[test]
    fn looped_music_keeps_playing_past_the_file_end() {
        let path = click();
        let mut l = live();
        let mut p = req("loop", &path, 1);
        p.looped = true;
        l.play_music(&p, Duration::ZERO);
        run(&mut l, 60); // far longer than the 0.15 s file
        l.tick();
        assert_eq!(l.music_name(), Some("loop"));
    }

    #[test]
    fn finished_music_is_forgotten() {
        let path = click();
        let mut l = live();
        l.play_music(&req("once", &path, 1), Duration::ZERO);
        run(&mut l, 60);
        l.tick();
        assert_eq!(l.music_name(), None);
    }
}
