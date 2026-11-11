//! The sound of the fight (WP11.3): the events the picture shows ([`FxPlayer::shown`]) become positional
//! sounds.
//!
//! Each frame the runner calls [`FxSound::play_frame`] after it has set the listener from the camera. Per shown
//! event [`FxPlayer::sound_name`] gives the sound event name (`weapon.fire.cannon`, `explosion.large`, ...; an
//! event with no name is silent), the event's ground position and tick make a [`Request`], and the
//! [`Planner`] of `sf_audio` picks what plays: which sounds exist (a name with nothing in the bank and no Red
//! Alert mapping is skipped without a message), what is too quiet for the camera's zoom and distance
//! (`spatial::mix`, the one model), what merges into one playback, and what is held back by the voice limit,
//! the spacing and the priority. The numbers are in `assets/config/audio.ron`; `docs/rules/audio.md` has the
//! rules. [`FxSound::plan_events`] is the device-free half; the tests call it and `play_frame` themselves.
//!
//! Nothing here reaches the simulation: it only reads the events the sim thread queued for the picture.

use std::path::Path;

use sf_audio::music::EventWeights;
use sf_audio::{
    AudioConfig, AudioSystem, Category, Cue, Listener, Mood, MoodTracker, PlanStats, Planner,
    Request, SpatialParams,
};

use crate::fx::FxPlayer;
use crate::fx_events::{FxEvent, FxKind};
use crate::settings::{Mix, SoundBase, SoundSettings, Volume};

/// What the sound of the fight did, summed over the run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SoundTotals {
    pub requested: u64,
    pub silent: u64,
    pub inaudible: u64,
    pub merged: u64,
    pub limited: u64,
    pub spaced: u64,
    pub played: u64,
}

impl SoundTotals {
    fn add(&mut self, s: &PlanStats) {
        self.requested += u64::from(s.requested);
        self.silent += u64::from(s.silent);
        self.inaudible += u64::from(s.inaudible);
        self.merged += u64::from(s.merged);
        self.limited += u64::from(s.limited);
        self.spaced += u64::from(s.spaced);
        self.played += u64::from(s.planned);
    }
}

/// The audio start-up the runner does once: read `<asset_root>/config/audio.ron`, let `ra_override`
/// (`--ra-audio`) win over its `ra_audio`, switch the original audio on or off, give `audio` the file's
/// spatial numbers and sfx volume, and build the fight-sound planner from the file's rules. Returns the
/// planner and the config warning, if any (the caller prints it).
pub fn start_audio(
    asset_root: &Path,
    ra_override: Option<String>,
    audio: &mut AudioSystem,
) -> (FxSound, Option<String>) {
    let (sound, warning, _) =
        start_audio_with(asset_root, ra_override, &SoundSettings::default(), audio);
    (sound, warning)
}

/// [`start_audio`] with the player's sound settings (WP11.5b): a volume the player chose wins over the
/// one of `audio.ron`, the saved Red Alert switch and folder over the file's, and `ra_override`
/// (`--ra-audio`) over both. Also returns what the sound is with nothing chosen ([`SoundBase`], the
/// numbers of the file) for the settings page.
pub fn start_audio_with(
    asset_root: &Path,
    ra_override: Option<String>,
    saved: &SoundSettings,
    audio: &mut AudioSystem,
) -> (FxSound, Option<String>, SoundBase) {
    let (config, warning) = AudioConfig::load(asset_root);
    let base = SoundBase::from_config(&config);
    let now = saved.resolve(&base, ra_override.as_deref());
    audio.enable_original_audio(&now.ra);
    audio.set_spatial_params(config.spatial.params());
    // Dynamic music (WP11.4): crossfade and default tracks.
    audio.set_music_config(&config.music);
    apply_mix(audio, &now.mix);
    (FxSound::new(&config), warning, base)
}

/// Gives the audio the volumes of the settings: the master volume, and one for each category the
/// sliders have (the effects are the `Sfx` category, the unit voices the `Voice` category).
pub fn apply_mix(audio: &mut AudioSystem, mix: &Mix) {
    audio.set_master_volume(mix.master);
    audio.set_category_volume(Category::Sfx, mix.effects);
    audio.set_category_volume(Category::Music, mix.music);
    audio.set_category_volume(Category::Voice, mix.voices);
}

/// The sound of a slider's test button: a CC0 sound of the bank (never a Red Alert one), in the volume
/// group of that slider, so what is heard is what the slider sets: the effects' test in the effects,
/// the voices' in the voices, the master's in the interface group, which only the master volume
/// governs, so it is heard with the effects at 0 %. The music has none.
pub const TEST_SOUND: &str = "test_click";

pub fn play_test_sound(audio: &mut AudioSystem, which: Volume) {
    match which {
        Volume::Master => audio.play_as(TEST_SOUND, Category::Ui),
        Volume::Effects => audio.play_as(TEST_SOUND, Category::Sfx),
        Volume::Voices => audio.play_as(TEST_SOUND, Category::Voice),
        Volume::Music => {}
    }
}

/// Turns the shown events into sounds. See the module documentation.
pub struct FxSound {
    voices: sf_audio::VoiceConfig,
    planner: Planner,
    totals: SoundTotals,
    mood: MoodTracker,
}

/// What one shown event counts for towards combat (`music.mood.weights` of `audio.ron`).
pub fn mood_weight(kind: &FxKind, w: &EventWeights) -> u32 {
    match kind {
        FxKind::Fire { .. } => w.fire,
        FxKind::Impact { .. } => w.impact,
        FxKind::Death { .. } => w.death,
    }
}

impl FxSound {
    /// The rules from `config`. The spatial numbers are not kept here: each frame they come from the
    /// [`AudioSystem`] (`spatial_params`), so the plan and the output use one copy.
    pub fn new(config: &AudioConfig) -> FxSound {
        FxSound {
            voices: config.voices,
            planner: Planner::new(config.positional.clone()),
            totals: SoundTotals::default(),
            mood: MoodTracker::new(config.music.mood),
        }
    }

    /// The numbers of the unit voices' brake (`voices` of `audio.ron`, WP11.2).
    pub fn voice_config(&self) -> sf_audio::VoiceConfig {
        self.voices
    }

    pub fn totals(&self) -> SoundTotals {
        self.totals
    }

    /// Forget when things last played (a new game).
    pub fn reset(&mut self) {
        self.planner.reset();
        self.mood.reset();
    }

    /// The mood of the music after the shown `events`, at sim tick `now`, with the camera looking at `focus`
    /// (metres, `[x, z]`). Only events within `music.mood.radius_m` of the focus count. Device-free; the tests
    /// call it.
    pub fn mood_after(&mut self, events: &[FxEvent], focus: [f32; 2], now: u64) -> Mood {
        let cfg = *self.mood.config();
        let r2 = cfg.radius_m * cfg.radius_m;
        for ev in events {
            let (dx, dz) = (ev.pos[0] - focus[0], ev.pos[1] - focus[1]);
            if dx * dx + dz * dz <= r2 {
                self.mood.add(ev.tick, mood_weight(&ev.kind, &cfg.weights));
            }
        }
        self.mood.update(now)
    }

    /// What the runner calls once a frame, **before** [`play_frame`](Self::play_frame) (which forgets the shown
    /// events): the mood from the shown events, handed to the music. Returns the mood.
    pub fn music_frame(
        &mut self,
        fx: &FxPlayer,
        focus: [f32; 2],
        now: u64,
        audio: &mut AudioSystem,
    ) -> Mood {
        let mood = self.mood_after(fx.shown(), focus, now);
        audio.set_music_mood(mood);
        mood
    }

    /// The sounds the frame's shown events make, device-free. `has_sound` says which names have a sound.
    pub fn plan_events<'a>(
        &mut self,
        fx: &'a FxPlayer,
        events: &[FxEvent],
        listener: &Listener,
        params: &SpatialParams,
        has_sound: impl Fn(&str) -> bool,
        out: &mut Vec<Cue<'a>>,
    ) -> PlanStats {
        out.clear();
        if events.is_empty() {
            return PlanStats::default();
        }
        // The height is not heard (spatial.rs), so the ground is not asked for it.
        let requests: Vec<Request<'a>> = events
            .iter()
            .filter_map(|ev| {
                fx.sound_name(ev).map(|sound| Request {
                    sound,
                    pos: [ev.pos[0], 0.0, ev.pos[1]],
                    tick: ev.tick,
                })
            })
            .collect();
        let stats = self
            .planner
            .plan(listener, params, &requests, has_sound, out);
        self.totals.add(&stats);
        stats
    }

    /// What the runner calls once a frame, after `fx.frame(..)`: plan the sounds of the events just shown and
    /// start them on `audio`, whose listener the caller has set from the camera. Without an output device the
    /// plan is made all the same and nothing is heard.
    pub fn play_frame(&mut self, fx: &mut FxPlayer, audio: &mut AudioSystem) -> PlanStats {
        let listener = audio.listener();
        let params = audio.spatial_params();
        let mut cues = Vec::new();
        let stats = self.plan_events(
            fx,
            fx.shown(),
            &listener,
            &params,
            |name| audio.has_sound(name),
            &mut cues,
        );
        for cue in &cues {
            audio.play_at(cue.sound, cue.pos);
        }
        drop(cues);
        fx.clear_shown();
        stats
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::fx::FxFrame;
    use crate::fx_events::FxKind;
    use crate::sim_snapshot::SimSnapshot;
    use sf_audio::PlayTrace;
    use sf_data::units::UnitDefs;
    use sf_render::FlatGround;
    use sf_render::glam::Mat4;
    use sf_sim::{ImpactCause, World, WorldConfig};

    /// Every sound event name effects.ron gives (pinned by `the_event_names_follow_the_scheme` in fx.rs).
    const NAMES: [&str; 17] = [
        "weapon.fire.bullet",
        "weapon.fire.precision",
        "weapon.fire.cannon",
        "weapon.fire.rocket",
        "weapon.fire.shell",
        "weapon.fire.howitzer",
        "weapon.fire.demolition",
        "impact.bullet",
        "impact.precision",
        "impact.cannon",
        "impact.rocket",
        "impact.shell",
        "impact.demolition",
        "impact.miss",
        "unit.destroyed.infantry",
        "explosion.small",
        "explosion.medium",
    ];

    fn assets() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
    }

    /// A scratch asset root under the temp dir, removed when dropped.
    struct TempRoot(PathBuf);

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// An asset root whose bank has a sound for each of `names` (the CC0 click, copied; nothing else).
    fn root_with(tag: &str, names: &[&str]) -> TempRoot {
        let d = std::env::temp_dir().join(format!("sf_game_fxsound_{}_{tag}", std::process::id()));
        std::fs::create_dir_all(d.join("audio/sfx")).unwrap();
        std::fs::copy(
            assets().join("audio/sfx/test_click.wav"),
            d.join("audio/sfx/test_click.wav"),
        )
        .unwrap();
        let mut bank = String::from("(sounds: {\n");
        for n in names {
            bank += &format!("  \"{n}\": (file: \"audio/sfx/test_click.wav\"),\n");
        }
        bank += "})\n";
        std::fs::write(d.join("audio/bank.ron"), bank).unwrap();
        TempRoot(d)
    }

    /// A system without output that records what it would play.
    fn tracing_audio(root: &Path) -> AudioSystem {
        let mut a = AudioSystem::silent(root);
        a.set_trace(true);
        a
    }

    /// A tracing system together with the scratch root it reads, which goes when this does.
    struct Traced {
        audio: AudioSystem,
        _root: TempRoot,
    }

    impl std::ops::Deref for Traced {
        type Target = AudioSystem;
        fn deref(&self) -> &AudioSystem {
            &self.audio
        }
    }

    impl std::ops::DerefMut for Traced {
        fn deref_mut(&mut self) -> &mut AudioSystem {
            &mut self.audio
        }
    }

    fn all_names_root(tag: &str) -> TempRoot {
        let mut names = NAMES.to_vec();
        names.push("explosion.large");
        root_with(tag, &names)
    }

    fn all_names_audio(tag: &str) -> Traced {
        let root = all_names_root(tag);
        Traced {
            audio: tracing_audio(&root.0),
            _root: root,
        }
    }

    fn weapon(id: &str) -> u16 {
        UnitDefs::embedded().weapon(id).unwrap().kind
    }

    fn fire_at(weapon: u16, pos: [f32; 2], tick: u64) -> FxEvent {
        FxEvent {
            tick,
            weapon,
            pos,
            size: 6.0,
            ..FxEvent::default()
        }
    }

    fn hit_at(weapon: u16, hit: bool, splash: bool, pos: [f32; 2], tick: u64) -> FxEvent {
        FxEvent {
            kind: FxKind::Impact {
                cause: ImpactCause::Proximity,
                hit,
                splash,
            },
            size: 0.0,
            ..fire_at(weapon, pos, tick)
        }
    }

    fn death_at(size_m: f32, structure: bool, pos: [f32; 2], tick: u64) -> FxEvent {
        FxEvent {
            kind: FxKind::Death {
                wreck: true,
                structure,
            },
            weapon: 0,
            size: size_m,
            ..fire_at(0, pos, tick)
        }
    }

    fn snapshot_at(ticks: u32) -> SimSnapshot {
        let mut world = World::new(WorldConfig::new(1).with_map_size(8, 8)).unwrap();
        for _ in 0..ticks {
            world.step();
        }
        SimSnapshot::capture(&world)
    }

    fn fx_frame() -> FxFrame {
        FxFrame {
            view_proj: Mat4::IDENTITY,
            focal_px: 800.0,
            viewport: [960.0, 540.0],
            model_weight: 1.0,
            reference_px: 100.0,
            icon_px: 18.0,
            dt: 0.05,
            alpha: 1.0,
        }
    }

    /// What the runner does in a frame: the picture takes the events (`FxPlayer::frame`), the listener
    /// follows the camera, `play_frame` plays. Returns what the audio got.
    fn game_frame(
        fx: &mut FxPlayer,
        sound: &mut FxSound,
        audio: &mut AudioSystem,
        snap_tick: u32,
        listener: Listener,
    ) -> (Vec<PlayTrace>, PlanStats) {
        fx.frame(&snapshot_at(snap_tick), &FlatGround::default(), &fx_frame());
        audio.set_listener(listener);
        let stats = sound.play_frame(fx, audio);
        (audio.take_trace(), stats)
    }

    fn listener(distance: f32) -> Listener {
        Listener::new([12.0, 0.0, 12.0], [1.0, 0.0, 0.0], distance)
    }

    fn setup() -> (FxPlayer, FxSound) {
        (FxPlayer::embedded(), FxSound::new(&AudioConfig::embedded()))
    }

    /// The names this file tests with are the ones effects.ron gives, and each has a rule of its own in
    /// audio.ron (not the fallback): a new effect sound without a rule is noticed here.
    #[test]
    fn every_sound_name_of_the_effects_has_a_rule() {
        let effects = sf_data::effects::Effects::embedded();
        let mut names: Vec<&str> = effects
            .effects
            .values()
            .map(|e| e.sound.as_str())
            .filter(|s| !s.is_empty())
            .collect();
        names.sort_unstable();
        let mut mine: Vec<&str> = NAMES.to_vec();
        mine.push("explosion.large");
        mine.sort_unstable();
        assert_eq!(names, mine);
        let cfg = AudioConfig::embedded().positional;
        for n in names {
            assert_ne!(
                cfg.rule(n),
                &cfg.fallback,
                "{n} falls through to the fallback"
            );
        }
    }

    #[test]
    fn each_event_plays_its_sound_at_full_volume_in_the_centre() {
        // One event per frame so nothing is thinned: ticks 0, 2, 4, ... shown by snapshots 1, 3, 5, ...
        let (mut fx, mut sound) = setup();
        let mut audio = all_names_audio("each");
        let p = [12.0, 12.0];
        let w = weapon;
        let events = [
            (fire_at(w("rifle"), p, 0), "weapon.fire.bullet"),
            (fire_at(w("long_rifle"), p, 0), "weapon.fire.precision"),
            (fire_at(w("cannon"), p, 0), "weapon.fire.cannon"),
            (fire_at(w("rocket"), p, 0), "weapon.fire.rocket"),
            (fire_at(w("battery_shell"), p, 0), "weapon.fire.shell"),
            (fire_at(w("howitzer"), p, 0), "weapon.fire.howitzer"),
            (fire_at(w("satchel_charge"), p, 0), "weapon.fire.demolition"),
            (hit_at(w("rifle"), true, false, p, 0), "impact.bullet"),
            (hit_at(w("cannon"), true, true, p, 0), "impact.cannon"),
            (hit_at(w("rifle"), false, false, p, 0), "impact.miss"),
            (death_at(1.6, false, p, 0), "unit.destroyed.infantry"),
            (death_at(4.8, false, p, 0), "explosion.small"),
            (death_at(6.0, false, p, 0), "explosion.medium"),
            (death_at(8.0, true, p, 0), "explosion.large"),
        ];
        for (i, (ev, want)) in events.iter().enumerate() {
            let tick = 1 + 2 * i as u32;
            let ev = FxEvent {
                tick: u64::from(tick) - 1,
                ..*ev
            };
            fx.push_events(&[ev]);
            let (trace, stats) = game_frame(&mut fx, &mut sound, &mut audio, tick, listener(30.0));
            assert_eq!(trace.len(), 1, "{want}: {stats:?}");
            assert_eq!(trace[0].name, *want);
            assert_eq!((trace[0].gain, trace[0].pan), (1.0, 0.0), "{want}");
        }
    }

    #[test]
    fn a_name_with_no_sound_is_silent_and_gives_no_error() {
        // A bank without the fight sounds (the shipped one has them since WP11.3b): nothing plays, nothing is
        // counted as unknown.
        let (mut fx, mut sound) = setup();
        let root = root_with("silent", &[]);
        let mut audio = tracing_audio(&root.0);
        fx.push_events(&[
            fire_at(weapon("rifle"), [12.0, 12.0], 0),
            death_at(8.0, true, [12.0, 12.0], 0),
        ]);
        let (trace, stats) = game_frame(&mut fx, &mut sound, &mut audio, 1, listener(30.0));
        assert!(trace.is_empty());
        assert_eq!((stats.requested, stats.silent, stats.planned), (2, 2, 0));
        assert_eq!(audio.stats().unknown, 0, "no 'unknown sound' for an event");
        // And an event the effects make no sound for at all is not even requested.
        fx.push_events(&[FxEvent {
            kind: FxKind::Impact {
                cause: ImpactCause::Proximity,
                hit: false,
                splash: false,
            },
            ..fire_at(weapon("rifle"), [12.0, 12.0], 1)
        }]);
        game_frame(&mut fx, &mut sound, &mut audio, 2, listener(30.0));
        assert_eq!(audio.stats().unknown, 0);
    }

    #[test]
    fn pan_and_gain_follow_where_the_event_is() {
        let (mut fx, mut sound) = setup();
        let mut audio = all_names_audio("pan");
        // 20 m right of the focus and 60 m behind it, camera at 30 m.
        fx.push_events(&[
            fire_at(weapon("cannon"), [32.0, 12.0], 0),
            fire_at(weapon("cannon"), [-10.0, 12.0], 0),
        ]);
        let (trace, _) = game_frame(&mut fx, &mut sound, &mut audio, 1, listener(30.0));
        // 20 m is inside the full-volume radius, 22 m left too; they are 42 m apart: two cues.
        assert_eq!(trace.len(), 2);
        let right = trace.iter().find(|t| t.pan > 0.0).expect("a right cue");
        let left = trace.iter().find(|t| t.pan < 0.0).expect("a left cue");
        assert!(right.pan > 0.3 && left.pan < -0.3, "{trace:?}");
        // The model's own numbers: pan = across / (pan_width + pan_growth * distance) * max_pan.
        let want = 20.0 / (10.0 + 30.0) * 0.9;
        assert!((right.pan - want).abs() < 1e-5, "{} vs {want}", right.pan);
    }

    #[test]
    fn zooming_out_is_never_louder_through_the_game_path() {
        // Several positions, each as one event in a fresh game, heard at growing camera distances. Every
        // frame goes through FxPlayer::frame -> FxSound::play_frame -> AudioSystem.
        let cams = [10.0, 30.0, 40.0, 60.0, 100.0, 250.0, 1000.0];
        let offsets = [0.0_f32, 15.0, 40.0, 60.0, 90.0, 110.0];
        let mut checked = 0;
        for off in offsets {
            let mut last = f32::INFINITY;
            for (i, cam) in cams.iter().enumerate() {
                let (mut fx, mut sound) = setup();
                let mut audio = all_names_audio("zoom");
                fx.push_events(&[fire_at(weapon("cannon"), [12.0 + off, 12.0], 0)]);
                let (trace, _) = game_frame(&mut fx, &mut sound, &mut audio, 1, listener(*cam));
                let g = trace.first().map_or(0.0, |t| t.gain);
                assert!(
                    g <= last + 1e-6,
                    "offset {off}: camera {cam} is louder ({g}) than the one before ({last})"
                );
                last = g;
                checked += usize::from(i > 0 && g > 0.0);
            }
        }
        assert!(checked > 10, "the grid must hear something: {checked}");
        // And the far end really is quieter than the near end for a source on the focus.
        let (mut fx, mut sound) = setup();
        let mut audio = all_names_audio("zoom2");
        fx.push_events(&[fire_at(weapon("cannon"), [12.0, 12.0], 0)]);
        let (near, _) = game_frame(&mut fx, &mut sound, &mut audio, 1, listener(30.0));
        let (mut fx, mut sound) = setup();
        fx.push_events(&[fire_at(weapon("cannon"), [12.0, 12.0], 0)]);
        let (far, _) = game_frame(&mut fx, &mut sound, &mut audio, 1, listener(400.0));
        assert!(far[0].gain < near[0].gain * 0.5, "{far:?} {near:?}");
    }

    #[test]
    fn a_sound_far_away_is_quieter_and_beyond_the_radius_gone() {
        let (mut fx, mut sound) = setup();
        let mut audio = all_names_audio("far");
        fx.push_events(&[
            fire_at(weapon("cannon"), [12.0, 12.0], 0),
            fire_at(weapon("cannon"), [12.0, 72.0], 0),
            fire_at(weapon("cannon"), [12.0, 300.0], 0),
        ]);
        let (trace, stats) = game_frame(&mut fx, &mut sound, &mut audio, 1, listener(30.0));
        assert_eq!(trace.len(), 2);
        assert!(trace.iter().any(|t| t.gain == 1.0));
        assert!(trace.iter().any(|t| t.gain > 0.0 && t.gain < 0.5));
        assert_eq!(stats.inaudible, 1);
    }

    #[test]
    fn a_volley_in_one_tick_merges_into_one_playback() {
        let (mut fx, mut sound) = setup();
        let mut audio = all_names_audio("volley");
        let shots: Vec<FxEvent> = (0..40)
            .map(|i| {
                fire_at(
                    weapon("rifle"),
                    [12.0 + (i % 8) as f32 * 0.5, 12.0 + (i / 8) as f32 * 0.5],
                    0,
                )
            })
            .collect();
        fx.push_events(&shots);
        let (trace, stats) = game_frame(&mut fx, &mut sound, &mut audio, 1, listener(30.0));
        assert_eq!(trace.len(), 1);
        assert_eq!((stats.requested, stats.merged, stats.planned), (40, 39, 1));
    }

    #[test]
    fn the_same_fight_sounds_the_same_twice() {
        let fight = |tag: &str| {
            let (mut fx, mut sound) = setup();
            let mut audio = all_names_audio(tag);
            let events = heavy_fight(1);
            fx.push_events(&events);
            game_frame(&mut fx, &mut sound, &mut audio, 2, listener(60.0)).0
        };
        let (a, b) = (fight("det1"), fight("det2"));
        assert!(!a.is_empty());
        assert_eq!(a, b);
    }

    /// A battle at the focus: `ticks` ticks of rifles, cannons, rockets, hits and deaths in a 60 m square.
    fn heavy_fight(ticks: u64) -> Vec<FxEvent> {
        let mut v = Vec::new();
        let mut n = 0u32;
        let mut spot = || {
            n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let x = (n >> 8) % 600;
            n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let z = (n >> 8) % 600;
            [x as f32 / 10.0 - 18.0, z as f32 / 10.0 - 18.0]
        };
        let kinds: Vec<u16> = ["rifle", "cannon", "rocket", "long_rifle", "battery_shell"]
            .iter()
            .map(|id| weapon(id))
            .collect();
        for t in 0..ticks {
            for i in 0..300 {
                let k = kinds[i % kinds.len()];
                v.push(fire_at(k, spot(), t));
                v.push(hit_at(k, true, i % 3 == 0, spot(), t));
            }
            for i in 0..20 {
                v.push(death_at([1.6, 4.8, 6.0, 8.0][i % 4], i % 4 == 3, spot(), t));
            }
        }
        v
    }

    #[test]
    fn a_heavy_fight_stays_inside_the_budgets() {
        let cfg = AudioConfig::embedded();
        let (mut fx, mut sound) = setup();
        let mut audio = all_names_audio("heavy");
        let events = heavy_fight(1);
        assert_eq!(events.len(), 620);
        let named = events.iter().filter(|e| fx.sound_name(e).is_some()).count();
        fx.push_events(&events);
        let (trace, stats) = game_frame(&mut fx, &mut sound, &mut audio, 1, listener(60.0));
        assert_eq!(stats.requested as usize, named);
        assert!(named > 600, "{named}");
        assert!(trace.len() <= cfg.positional.max_cues_per_frame as usize);
        assert!(!trace.is_empty());
        // Per event at most its voice limit.
        for name in NAMES {
            let n = trace.iter().filter(|t| t.name == name).count();
            let max = cfg.positional.rule(name).max_per_batch as usize;
            assert!(n <= max, "{name}: {n} > {max}");
        }
        // The loud kinds are not crowded out by the many rifles: a death or a shell is among the cues.
        assert!(
            trace
                .iter()
                .any(|t| t.name.starts_with("explosion") || t.name == "unit.destroyed.infantry")
        );
        assert_eq!(
            stats.planned + stats.merged + stats.limited + stats.spaced + stats.inaudible,
            stats.requested - stats.silent
        );
    }

    #[test]
    fn an_old_event_and_a_replayed_frame_are_silent() {
        let (mut fx, mut sound) = setup();
        let mut audio = all_names_audio("old");
        let limit = u64::from(fx.limits().max_event_age_ticks);
        fx.push_events(&[fire_at(weapon("cannon"), [12.0, 12.0], 0)]);
        // The picture takes it 41 ticks late (a minimised window): expired, so no sound either.
        let (trace, _) = game_frame(
            &mut fx,
            &mut sound,
            &mut audio,
            (limit + 1) as u32,
            listener(30.0),
        );
        assert!(trace.is_empty());
        // A fresh event plays once; a second frame that shows nothing new does not play it again.
        fx.push_events(&[fire_at(weapon("cannon"), [12.0, 12.0], 50)]);
        let (trace, _) = game_frame(&mut fx, &mut sound, &mut audio, 51, listener(30.0));
        assert_eq!(trace.len(), 1);
        // The shown events are taken by the audio: a frame that does not call `FxPlayer::frame` has none, and
        // a planner with no memory of the spacing (a new one) hears nothing either.
        assert!(fx.shown().is_empty());
        FxSound::new(&AudioConfig::embedded()).play_frame(&mut fx, &mut audio);
        assert!(audio.take_trace().is_empty());
        let (trace, _) = game_frame(&mut fx, &mut sound, &mut audio, 52, listener(30.0));
        assert!(trace.is_empty());
    }

    #[test]
    fn spacing_thins_a_steady_stream_and_a_far_one_more() {
        // One cannon firing every tick, for 40 ticks, near and far.
        let heard = |x: f32| {
            let (mut fx, mut sound) = setup();
            let mut audio = all_names_audio("stream");
            let mut n = 0;
            for t in 0..40u32 {
                fx.push_events(&[fire_at(weapon("cannon"), [x, 12.0], u64::from(t))]);
                n += game_frame(&mut fx, &mut sound, &mut audio, t + 1, listener(30.0))
                    .0
                    .len();
            }
            n
        };
        let (near, far) = (heard(12.0), heard(100.0));
        assert_eq!(
            near, 40,
            "min_spacing_ticks is 1 for the cannon: every tick"
        );
        assert!(far > 0 && far < near / 2, "{far} vs {near}");
    }

    #[test]
    fn zooming_out_makes_a_steady_stream_less_frequent_through_the_game_path() {
        let heard = |cam: f32| {
            let (mut fx, mut sound) = setup();
            let mut audio = all_names_audio("zoomstream");
            let mut n = 0;
            for t in 0..60u32 {
                fx.push_events(&[fire_at(weapon("cannon"), [42.0, 12.0], u64::from(t))]);
                n += game_frame(&mut fx, &mut sound, &mut audio, t + 1, listener(cam))
                    .0
                    .len();
            }
            n
        };
        let (close, middle, far) = (heard(30.0), heard(100.0), heard(400.0));
        assert!(
            close > middle && middle > far && far > 0,
            "{close} {middle} {far}"
        );
    }

    /// Gain, pan and number of cues of a 30-frame stream of cannon shots 20 m right of the focus.
    fn stream_through(sound: &mut FxSound, audio: &mut AudioSystem) -> (f32, f32, usize) {
        let mut fx = FxPlayer::embedded();
        let mut first = None;
        let mut n = 0;
        for t in 0..30u32 {
            fx.push_events(&[fire_at(weapon("cannon"), [32.0, 12.0], u64::from(t))]);
            let (trace, _) = game_frame(&mut fx, sound, audio, t + 1, listener(30.0));
            if first.is_none() {
                first = trace.first().map(|c| (c.gain, c.pan));
            }
            n += trace.len();
        }
        let (g, p) = first.expect("the first shot is heard");
        (g, p, n)
    }

    #[test]
    fn start_audio_follows_the_file_on_disk() {
        // The runner's start-up (`start_audio`) on a scratch root whose audio.ron differs from the shipped one:
        // half the pan width, half the sfx volume, and every event spaced 1000 ticks apart.
        let scratch = all_names_root("startup");
        let spacing = |s: &str, key: &str| {
            let mut out = String::new();
            for line in s.lines() {
                match line.find(key) {
                    Some(i) => {
                        let rest = &line[i + key.len()..];
                        let end = rest.find(',').unwrap();
                        out += &format!("{}{key}1000{}\n", &line[..i], &rest[end..]);
                    }
                    None => out += &format!("{line}\n"),
                }
            }
            out
        };
        let text = sf_audio::config::EMBEDDED_CONFIG
            .replace("max_pan: 0.9", "max_pan: 0.45")
            .replace("sfx_volume: 1.0", "sfx_volume: 0.5");
        let text = spacing(
            &spacing(&text, "min_spacing_ticks: "),
            "far_spacing_ticks: ",
        );
        assert_ne!(text, sf_audio::config::EMBEDDED_CONFIG);
        std::fs::create_dir_all(scratch.0.join("config")).unwrap();
        std::fs::write(scratch.0.join("config/audio.ron"), &text).unwrap();
        let mut audio = tracing_audio(&scratch.0);
        let (mut sound, warning) = start_audio(&scratch.0, None, &mut audio);
        assert_eq!(warning, None, "the scratch file is valid");
        let (gain, pan, cues) = stream_through(&mut sound, &mut audio);

        // The same root without the file: the shipped numbers.
        std::fs::remove_file(scratch.0.join("config/audio.ron")).unwrap();
        let mut shipped_audio = tracing_audio(&scratch.0);
        let (mut shipped, w) = start_audio(&scratch.0, None, &mut shipped_audio);
        assert_eq!(w, None);
        let (g0, p0, n0) = stream_through(&mut shipped, &mut shipped_audio);

        assert!((pan - p0 * 0.5).abs() < 1e-5, "pan {pan} vs shipped {p0}");
        assert!(
            (gain - g0 * 0.5).abs() < 1e-5,
            "gain {gain} vs shipped {g0}"
        );
        assert_eq!(cues, 1, "1000 ticks of spacing: one shot in 30 frames");
        assert!(n0 > 10, "shipped spacing lets the stream through: {n0}");
    }

    #[test]
    fn the_planner_follows_spatial_params_set_after_start() {
        // A later `set_spatial_params` (the settings screen) is what the planner uses too: here every sound is
        // made inaudible to the planner by raising `audible_gain`, so nothing is planned.
        let mut audio = all_names_audio("later");
        let (mut fx, mut sound) = setup();
        fx.push_events(&[fire_at(weapon("cannon"), [32.0, 12.0], 0)]);
        let (trace, _) = game_frame(&mut fx, &mut sound, &mut audio, 1, listener(30.0));
        assert_eq!(trace.len(), 1);
        let mut p = audio.spatial_params();
        p.audible_gain = 2.0;
        audio.set_spatial_params(p);
        fx.push_events(&[fire_at(weapon("cannon"), [32.0, 12.0], 40)]);
        let (trace, stats) = game_frame(&mut fx, &mut sound, &mut audio, 41, listener(30.0));
        assert!(trace.is_empty(), "{trace:?}");
        assert_eq!((stats.inaudible, stats.planned), (1, 0), "{stats:?}");
    }

    /// The cost of the sound selection in a heavy fight:`FxSound::plan_events` (what `play_frame` runs on the
    /// shown events) plus the `play_at` calls, on 620 events a frame for 40 frames. Run with `--release
    /// --nocapture` to see the numbers. The particle pool is left out (`FxPlayer::frame` is slow in a debug
    /// build and is not what is measured). No assertion about speed beyond a very loose one.
    #[test]
    fn the_cost_of_a_heavy_fight_frame() {
        let (fx, mut sound) = setup();
        let mut audio = all_names_audio("cost");
        let l = listener(60.0);
        audio.set_listener(l);
        let frames = 40u32;
        let fight = heavy_fight(1);
        let mut worst = std::time::Duration::ZERO;
        let mut total = std::time::Duration::ZERO;
        let mut cues = Vec::new();
        for f in 0..frames {
            let events: Vec<FxEvent> = fight
                .iter()
                .map(|e| FxEvent {
                    tick: u64::from(f),
                    ..*e
                })
                .collect();
            let started = std::time::Instant::now();
            sound.plan_events(
                &fx,
                &events,
                &l,
                &audio.spatial_params(),
                |n| audio.has_sound(n),
                &mut cues,
            );
            for c in &cues {
                audio.play_at(c.sound, c.pos);
            }
            let took = started.elapsed();
            let _ = audio.take_trace();
            total += took;
            worst = worst.max(took);
        }
        let t = sound.totals();
        let n = u64::from(frames);
        println!(
            "WP11.3 sound selection, {} events per frame: mean {:?}, worst {:?} over {frames} frames; \
             per frame {} requests, {} played, {} merged, {} limited, {} spaced, {} inaudible, {} silent",
            fight.len(),
            total / frames,
            worst,
            t.requested / n,
            t.played / n,
            t.merged / n,
            t.limited / n,
            t.spaced / n,
            t.inaudible / n,
            t.silent / n,
        );
        assert!(total / frames < std::time::Duration::from_millis(20));
    }

    // ---- the CC0 fight sounds (WP11.3b) --------------------------------------------------------------------

    /// Every fight event and the file of its CC0 sound in the shipped bank, as literals (swap-proof: a bank
    /// entry that points at another event's file, or a file that is not there, fails).
    const FIGHT_FILES: [(&str, &str); 18] = [
        ("weapon.fire.bullet", "audio/sfx/weapon_fire_bullet.wav"),
        (
            "weapon.fire.precision",
            "audio/sfx/weapon_fire_precision.wav",
        ),
        ("weapon.fire.cannon", "audio/sfx/weapon_fire_cannon.wav"),
        ("weapon.fire.rocket", "audio/sfx/weapon_fire_rocket.wav"),
        ("weapon.fire.shell", "audio/sfx/weapon_fire_shell.wav"),
        ("weapon.fire.howitzer", "audio/sfx/weapon_fire_howitzer.wav"),
        (
            "weapon.fire.demolition",
            "audio/sfx/weapon_fire_demolition.wav",
        ),
        ("impact.bullet", "audio/sfx/impact_bullet.wav"),
        ("impact.precision", "audio/sfx/impact_precision.wav"),
        ("impact.cannon", "audio/sfx/impact_cannon.wav"),
        ("impact.rocket", "audio/sfx/impact_rocket.wav"),
        ("impact.shell", "audio/sfx/impact_shell.wav"),
        ("impact.demolition", "audio/sfx/impact_demolition.wav"),
        ("impact.miss", "audio/sfx/impact_miss.wav"),
        (
            "unit.destroyed.infantry",
            "audio/sfx/unit_destroyed_infantry.wav",
        ),
        ("explosion.small", "audio/sfx/explosion_small.wav"),
        ("explosion.medium", "audio/sfx/explosion_medium.wav"),
        ("explosion.large", "audio/sfx/explosion_large.wav"),
    ];

    /// (event, volume, pitch_variation, max_instances) of the shipped bank, as literals: a volume set to 1.0, all
    /// of them halved, or a changed cap or pitch spread fails here.
    const FIGHT_TUNING: [(&str, f32, f32, u32); 18] = [
        ("weapon.fire.bullet", 0.225, 0.06, 6),
        ("weapon.fire.precision", 0.3, 0.04, 4),
        ("weapon.fire.cannon", 0.3, 0.05, 6),
        ("weapon.fire.rocket", 0.275, 0.04, 4),
        ("weapon.fire.shell", 0.275, 0.05, 4),
        ("weapon.fire.howitzer", 0.325, 0.03, 3),
        ("weapon.fire.demolition", 0.25, 0.0, 2),
        ("impact.bullet", 0.15, 0.08, 6),
        ("impact.precision", 0.175, 0.05, 4),
        ("impact.cannon", 0.25, 0.05, 6),
        ("impact.rocket", 0.275, 0.04, 4),
        ("impact.shell", 0.25, 0.05, 4),
        ("impact.demolition", 0.3, 0.03, 3),
        ("impact.miss", 0.125, 0.08, 4),
        ("unit.destroyed.infantry", 0.2, 0.06, 4),
        ("explosion.small", 0.25, 0.05, 4),
        ("explosion.medium", 0.3, 0.04, 4),
        ("explosion.large", 0.35, 0.03, 3),
    ];

    #[test]
    fn the_tuning_of_every_fight_sound_is_pinned() {
        let bank = shipped_bank();
        for (event, volume, pitch, cap) in FIGHT_TUNING {
            let d = bank.get(event).unwrap();
            assert_eq!(
                (d.volume, d.pitch_variation, d.max_instances),
                (volume, pitch, cap),
                "{event}"
            );
            assert!(
                d.original_as_is,
                "{event}: a mapped original must play as-is"
            );
        }
    }

    fn shipped_bank() -> sf_audio::Bank {
        sf_audio::Bank::load(&assets().join("audio/bank.ron")).unwrap()
    }

    /// The 16-bit samples of a generated file (a 44-byte header, mono).
    fn wav_samples(file: &str) -> Vec<i16> {
        let b = std::fs::read(assets().join(file)).unwrap();
        assert_eq!(&b[36..40], b"data", "{file}");
        b[44..]
            .chunks(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect()
    }

    #[test]
    fn every_fight_event_has_a_non_silent_cc0_sound_in_the_shipped_bank() {
        let bank = shipped_bank();
        // The literal list is the event list of effects.ron (plus the large explosion).
        let mut listed: Vec<&str> = FIGHT_FILES.iter().map(|(e, _)| *e).collect();
        let mut named: Vec<&str> = NAMES.to_vec();
        named.push("explosion.large");
        listed.sort_unstable();
        named.sort_unstable();
        assert_eq!(listed, named);
        for (event, file) in FIGHT_FILES {
            let def = bank
                .get(event)
                .unwrap_or_else(|| panic!("{event} has no bank entry"));
            assert_eq!(def.file, file, "{event}");
            assert_eq!(def.category, sf_audio::Category::Sfx, "{event}");
            assert!(
                def.volume > 0.0 && def.volume <= 1.0,
                "{event} volume {}",
                def.volume
            );
            let samples = wav_samples(file);
            assert!(samples.iter().any(|s| s.abs() > 10_000), "{file} is silent");
        }
        // And the game's own path: each event is a sound for the audio system and plays at full gain x volume.
        let mut audio = tracing_audio(&assets());
        for (event, _) in FIGHT_FILES {
            assert!(audio.has_sound(event), "{event}");
            audio.set_listener(listener(30.0));
            audio.play_at(event, [12.0, 0.0, 12.0]);
        }
        let trace = audio.take_trace();
        assert_eq!(trace.len(), 18);
        for (t, (event, _)) in trace.iter().zip(FIGHT_FILES) {
            assert_eq!(t.name, event);
            assert_eq!(t.gain, bank.get(event).unwrap().volume, "{event}");
        }
        assert_eq!(audio.stats().unknown, 0);
    }

    /// Plays `frames` (events per tick, 20 ticks a second) through the planner and the audio system and mixes the
    /// traced playbacks, each file times its traced gain, at sample accuracy. Returns the peak of the sum as a
    /// share of full scale. Pan is ignored: with the player's pan law the louder channel is never above the
    /// centre gain, so this is an upper bound; pitch variation and the voice limits are ignored too.
    fn mixed_peak(frames: &[Vec<FxEvent>]) -> f32 {
        let bank = shipped_bank();
        let (fx, mut sound) = setup();
        let mut audio = tracing_audio(&assets());
        let l = listener(30.0);
        audio.set_listener(l);
        let per_tick = 22_050 / 20;
        let mut mix = vec![0.0f32; frames.len() * per_tick + 3 * 22_050];
        let mut cues = Vec::new();
        let mut cache: std::collections::BTreeMap<String, Vec<i16>> = Default::default();
        for (f, events) in frames.iter().enumerate() {
            sound.plan_events(
                &fx,
                events,
                &l,
                &audio.spatial_params(),
                |n| audio.has_sound(n),
                &mut cues,
            );
            for c in &cues {
                audio.play_at(c.sound, c.pos);
            }
            for t in audio.take_trace() {
                let samples = cache
                    .entry(t.name.clone())
                    .or_insert_with(|| wav_samples(&bank.get(&t.name).unwrap().file));
                for (i, s) in samples.iter().enumerate() {
                    mix[f * per_tick + i] += f32::from(*s) / 32_768.0 * t.gain;
                }
            }
        }
        mix.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    /// 20 tanks in a cluster at the focus fire a volley every 30 ticks, staggered, each shell hits 4 ticks later;
    /// two medium and one large explosion on top and a few infantry deaths: ten seconds of the loudest ordinary fight.
    fn tank_fight() -> Vec<Vec<FxEvent>> {
        let cannon = weapon("cannon");
        (0..200u64)
            .map(|t| {
                let mut v = Vec::new();
                for i in 0..20u64 {
                    let p = [12.0 + (i % 5) as f32, 12.0 + (i / 5) as f32];
                    if t % 30 == i * 3 % 30 {
                        v.push(fire_at(cannon, p, t));
                    }
                    if t >= 4 && (t - 4) % 30 == i * 3 % 30 {
                        v.push(hit_at(cannon, true, false, [p[0] + 8.0, p[1]], t));
                    }
                }
                if t == 60 || t == 62 {
                    v.push(death_at(6.0, false, [20.0, 14.0], t));
                }
                if t == 100 {
                    v.push(death_at(8.0, true, [14.0, 14.0], t));
                }
                if t % 40 == 0 {
                    v.push(death_at(1.6, false, [18.0, 16.0], t));
                }
                v
            })
            .collect()
    }

    /// Decision 3 of WP11.3b: a 20-tank fight at the default settings is not louder than the music. The music
    /// at `music_volume` 0.6 (audio.ron) peaks at 0.6 of full scale at most (a normalised track), so the fight's
    /// mixed peak must stay at or below 0.5; a muffled bank (all volumes 0) fails the other way, the fight must be heard.
    #[test]
    fn a_20_tank_fight_is_not_louder_than_the_music() {
        let cfg = AudioConfig::embedded();
        assert_eq!(cfg.mix.music_volume, 0.6);
        assert_eq!(cfg.mix.sfx_volume, 1.0);
        let peak = mixed_peak(&tank_fight());
        println!("WP11.3b: mixed peak of the 20-tank fight {peak:.3} of full scale (music 0.6)");
        assert!(peak <= 0.5, "peak {peak}");
        assert!(peak >= 0.15, "the fight is too quiet to hear: {peak}");
    }

    /// One event of every fight kind at `p`, with the event name the game gives it (checked against
    /// `FxPlayer::sound_name`, the function the game uses).
    fn every_fight_event(p: [f32; 2], tick: u64) -> Vec<(&'static str, FxEvent)> {
        let w = weapon;
        let at = |e: FxEvent| FxEvent { tick, ..e };
        let all = vec![
            ("weapon.fire.bullet", fire_at(w("rifle"), p, 0)),
            ("weapon.fire.precision", fire_at(w("long_rifle"), p, 0)),
            ("weapon.fire.cannon", fire_at(w("cannon"), p, 0)),
            ("weapon.fire.rocket", fire_at(w("rocket"), p, 0)),
            ("weapon.fire.shell", fire_at(w("battery_shell"), p, 0)),
            ("weapon.fire.howitzer", fire_at(w("howitzer"), p, 0)),
            ("weapon.fire.demolition", fire_at(w("satchel_charge"), p, 0)),
            ("impact.bullet", hit_at(w("rifle"), true, false, p, 0)),
            (
                "impact.precision",
                hit_at(w("long_rifle"), true, false, p, 0),
            ),
            ("impact.cannon", hit_at(w("cannon"), true, true, p, 0)),
            ("impact.rocket", hit_at(w("rocket"), true, true, p, 0)),
            ("impact.shell", hit_at(w("battery_shell"), true, true, p, 0)),
            (
                "impact.demolition",
                hit_at(w("satchel_charge"), true, true, p, 0),
            ),
            ("impact.miss", hit_at(w("rifle"), false, false, p, 0)),
            ("unit.destroyed.infantry", death_at(1.6, false, p, 0)),
            ("explosion.small", death_at(4.8, false, p, 0)),
            ("explosion.medium", death_at(6.0, false, p, 0)),
            ("explosion.large", death_at(8.0, true, p, 0)),
        ];
        let fx = FxPlayer::embedded();
        all.into_iter()
            .map(|(name, e)| {
                let e = at(e);
                assert_eq!(fx.sound_name(&e).as_deref(), Some(name), "{name}");
                (name, e)
            })
            .collect()
    }

    /// Each of the 18 events alone at the focus: audible, and none close to the 0.5 ceiling.
    #[test]
    fn every_fight_event_alone_is_audible_and_modest() {
        for (name, ev) in every_fight_event([12.0, 12.0], 0) {
            let peak = mixed_peak(&[vec![ev]]);
            println!("WP11.3b: {name} alone peaks at {peak:.3}");
            assert!((0.05..=0.35).contains(&peak), "{name}: {peak}");
        }
    }

    /// 20 tanks fire on the same tick and 20 shells land four ticks later, repeated every 30 ticks for 10 s, through
    /// the real planner (merge, `max_per_batch`, spacing).
    #[test]
    fn a_simultaneous_volley_of_20_tanks_is_not_louder_than_the_music() {
        let cannon = weapon("cannon");
        let frames: Vec<Vec<FxEvent>> = (0..200u64)
            .map(|t| {
                let mut v = Vec::new();
                for i in 0..20u64 {
                    let p = [12.0 + (i % 5) as f32, 12.0 + (i / 5) as f32];
                    if t % 30 == 0 {
                        v.push(fire_at(cannon, p, t));
                    }
                    if t % 30 == 4 {
                        v.push(hit_at(cannon, true, false, [p[0] + 8.0, p[1]], t));
                    }
                }
                v
            })
            .collect();
        let peak = mixed_peak(&frames);
        println!("WP11.3b: simultaneous 20-tank volley peaks at {peak:.3}");
        assert!(peak <= 0.5, "peak {peak}");
        assert!(peak >= 0.15, "{peak}");
    }

    /// Peak of `names` fired together on the same tick at the focus, repeated four times.
    fn together_peak(names: &[&str]) -> f32 {
        let frames: Vec<Vec<FxEvent>> = (0..40u64)
            .map(|t| {
                if t % 10 == 0 {
                    every_fight_event([12.0, 12.0], t)
                        .into_iter()
                        .filter(|(n, _)| names.contains(n))
                        .map(|(_, e)| e)
                        .collect()
                } else {
                    Vec::new()
                }
            })
            .collect();
        mixed_peak(&frames)
    }

    /// A heavy moment: four kinds on one tick at one spot (a cannon shot and its hit, a rocket, a medium explosion)
    /// must not clip. Measured and written down in `audio.md` (gaps): eight kinds on one tick at one spot sum to
    /// about 1.13 and all 18 to about 1.37, which would clip; the game does not produce that.
    #[test]
    fn a_heavy_moment_of_four_kinds_does_not_clip() {
        let four = [
            "weapon.fire.cannon",
            "impact.cannon",
            "weapon.fire.rocket",
            "explosion.medium",
        ];
        let peak = together_peak(&four);
        println!("WP11.3b: four kinds at once peak at {peak:.3}");
        assert!(peak <= 0.95, "peak {peak}");
        let eight = [
            "weapon.fire.cannon",
            "impact.cannon",
            "weapon.fire.rocket",
            "impact.rocket",
            "weapon.fire.howitzer",
            "impact.shell",
            "explosion.medium",
            "explosion.large",
        ];
        println!(
            "WP11.3b: eight kinds at once peak at {:.3}",
            together_peak(&eight)
        );
        let all: Vec<&str> = FIGHT_FILES.iter().map(|(e, _)| *e).collect();
        println!(
            "WP11.3b: all 18 kinds at once peak at {:.3}",
            together_peak(&all)
        );
    }

    // ---- the music mood (WP11.4) -------------------------------------------------------------------------

    #[test]
    fn the_weights_of_the_shipped_file_per_kind() {
        let w = AudioConfig::embedded().music.mood.weights;
        assert_eq!(mood_weight(&fire_at(1, [0.0; 2], 0).kind, &w), 1);
        assert_eq!(
            mood_weight(&hit_at(1, true, false, [0.0; 2], 0).kind, &w),
            1
        );
        assert_eq!(mood_weight(&death_at(4.0, false, [0.0; 2], 0).kind, &w), 3);
    }

    #[test]
    fn only_events_near_the_camera_focus_count() {
        let (_, mut sound) = setup();
        let cannon = weapon("cannon");
        // 80 m is the shipped radius: ten shots just inside count, ten just outside do not.
        let far: Vec<FxEvent> = (0..10)
            .map(|t| fire_at(cannon, [12.0 + 80.5, 12.0], t))
            .collect();
        assert_eq!(sound.mood_after(&far, [12.0, 12.0], 10), Mood::Calm);
        let near: Vec<FxEvent> = (0..10)
            .map(|t| fire_at(cannon, [12.0 + 79.5, 12.0], t))
            .collect();
        assert_eq!(sound.mood_after(&near, [12.0, 12.0], 10), Mood::Combat);
    }

    /// A scripted fight through the game's own path (events -> `FxPlayer::frame` -> `music_frame` ->
    /// `AudioSystem`): calm, a skirmish at ticks 100 to 160, calm again. The log lines are the evidence in the PR.
    #[test]
    fn a_scripted_fight_switches_the_music_with_hysteresis() {
        let root = root_with("mood_fight", &["calm_theme", "combat_theme"]);
        let mut cfg = AudioConfig::embedded();
        cfg.music.fallback.calm = Some("calm_theme".into());
        cfg.music.fallback.combat = Some("combat_theme".into());
        let mut audio = AudioSystem::silent(&root.0);
        audio.set_music_config(&cfg.music);
        audio.set_music_trace(true);
        let mut sound = FxSound::new(&cfg);
        let mut fx = FxPlayer::embedded();
        let cannon = weapon("cannon");
        let mut world = World::new(WorldConfig::new(1).with_map_size(8, 8)).unwrap();
        let mut log = Vec::new();
        for t in 0..800u64 {
            world.step();
            let mut ev = Vec::new();
            if (100..160).contains(&t) && t % 4 == 0 {
                ev.push(fire_at(cannon, [32.0, 12.0], t));
                ev.push(hit_at(cannon, true, false, [32.0, 12.0], t));
            }
            if t == 140 {
                ev.push(death_at(4.0, false, [30.0, 14.0], t));
            }
            if t == 120 {
                ev.push(fire_at(cannon, [2000.0, 2000.0], t)); // out of sight: not counted
            }
            fx.push_events(&ev);
            fx.frame(
                &SimSnapshot::capture(&world),
                &FlatGround::default(),
                &fx_frame(),
            );
            let now = world.tick().0;
            sound.music_frame(&fx, [12.0, 12.0], now, &mut audio);
            for tr in audio.take_music_trace() {
                log.push(format!(
                    "tick {now}: mood {} -> track {}",
                    tr.mood.name(),
                    tr.track
                ));
            }
            fx.clear_shown();
        }
        for l in &log {
            println!("{l}");
        }
        // Pinned: calm from the start; combat when the skirmish adds up to 6 (three volleys of shot plus hit,
        // ticks 100 to 108, shown at snapshot tick 109); calm 300 ticks after the last counted event (the
        // volley of tick 156; the far shot of tick 120 never counted). The skirmish itself, with 15 volleys
        // and a death, does not flip it back and forth.
        assert_eq!(
            log,
            [
                "tick 1: mood calm -> track calm_theme",
                "tick 109: mood combat -> track combat_theme",
                "tick 456: mood calm -> track calm_theme",
            ]
        );
    }

    // ---- the sound settings (WP11.5b) ----------------------------------------------------------

    use crate::settings::SoundSettings;
    use crate::unit_voice::{UnitVoice, UnitVoices};
    use sf_audio::{Category, RaStatus};

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6
    }

    /// Four different volumes: a slider wired to the wrong category gives another gain here.
    fn four_volumes() -> SoundSettings {
        SoundSettings {
            master: Some(90),
            music: Some(70),
            effects: Some(50),
            voices: Some(30),
            ..SoundSettings::default()
        }
    }

    #[test]
    fn each_slider_reaches_its_own_category_and_the_voices_are_not_the_effects() {
        let root = root_with("sliders", &["weapon.fire.cannon", "unit.move"]);
        let mut audio = tracing_audio(&root.0);
        let (_, warning, _) = start_audio_with(&root.0, None, &four_volumes(), &mut audio);
        assert_eq!(warning, None);
        assert!(near(audio.master_volume(), 0.9));
        assert!(near(audio.category_volume(Category::Music), 0.7));
        assert!(near(audio.category_volume(Category::Sfx), 0.5));
        assert!(near(audio.category_volume(Category::Voice), 0.3));
        // What the output would get: a shot at the effects' volume, a voice at the voices', both under
        // the master volume (the bank entries' own volume is 1.0).
        audio.play("weapon.fire.cannon");
        let mut voices = UnitVoices::new(AudioConfig::embedded().voices);
        assert!(voices.speak(UnitVoice::Move, 10_000, &mut audio));
        let gains: Vec<(String, f32)> = audio
            .take_trace()
            .into_iter()
            .map(|t| (t.name, t.gain))
            .collect();
        assert_eq!(gains.len(), 2, "{gains:?}");
        assert_eq!(gains[0].0, "weapon.fire.cannon");
        assert!(near(gains[0].1, 0.5 * 0.9), "effects {gains:?}");
        assert_eq!(gains[1].0, "unit.move");
        assert!(near(gains[1].1, 0.3 * 0.9), "voices {gains:?}");
    }

    #[test]
    fn a_player_who_never_opened_the_page_hears_what_audio_ron_says() {
        // The shipped numbers: master 1, effects 1, music 0.6, voices 1 - what the game did before.
        let root = root_with("defaults", &["weapon.fire.cannon"]);
        let mut audio = tracing_audio(&root.0);
        let (_, _, base) = start_audio_with(&root.0, None, &SoundSettings::default(), &mut audio);
        assert_eq!(audio.master_volume(), 1.0);
        assert_eq!(audio.category_volume(Category::Sfx), 1.0);
        assert_eq!(audio.category_volume(Category::Music), 0.6);
        assert_eq!(audio.category_volume(Category::Voice), 1.0);
        assert_eq!(base, crate::settings::SoundBase::default());
        // An audio.ron on the disk with other numbers, and no voice volume (a file from before 11.5b):
        // those numbers, and the voices follow the effects.
        std::fs::create_dir_all(root.0.join("config")).unwrap();
        let text = sf_audio::config::EMBEDDED_CONFIG
            .replace("sfx_volume: 1.0", "sfx_volume: 0.5")
            .replace("music_volume: 0.6", "music_volume: 0.35");
        // Drop the two lines whole, whatever the line endings of the checkout are (CRLF on Windows).
        let text: String = text
            .lines()
            .filter(|l| !l.contains("voice_volume: Some") && !l.contains("master_volume: 1.0"))
            .flat_map(|l| [l, "\n"])
            .collect();
        assert!(!text.contains("voice_volume: Some") && !text.contains("master_volume: 1.0"));
        std::fs::write(root.0.join("config/audio.ron"), text).unwrap();
        let mut audio = tracing_audio(&root.0);
        let (_, warning, base) =
            start_audio_with(&root.0, None, &SoundSettings::default(), &mut audio);
        assert_eq!(warning, None, "an older audio.ron still reads");
        assert_eq!(audio.master_volume(), 1.0);
        assert_eq!(audio.category_volume(Category::Sfx), 0.5);
        assert_eq!(audio.category_volume(Category::Music), 0.35);
        assert_eq!(audio.category_volume(Category::Voice), 0.5);
        assert_eq!((base.effects, base.music, base.voices), (0.5, 0.35, 0.5));
        // The player's choice still wins over that file.
        let mut audio = tracing_audio(&root.0);
        start_audio_with(&root.0, None, &four_volumes(), &mut audio);
        assert!(near(audio.category_volume(Category::Sfx), 0.5));
        assert!(near(audio.category_volume(Category::Music), 0.7));
        assert!(near(audio.category_volume(Category::Voice), 0.3));
    }

    /// Lets the loader thread finish: the status after the Red Alert files were read (or not).
    fn settled_ra_status(audio: &mut AudioSystem) -> RaStatus {
        for _ in 0..500 {
            audio.update();
            if audio.original_audio_status() != RaStatus::Loading {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        audio.original_audio_status()
    }

    #[test]
    fn the_command_line_folder_wins_over_the_saved_switch_at_the_start() {
        let root = root_with("ra_cli", &["unit.move"]);
        let nothing = root.0.join("no_such_folder");
        let nothing = nothing.display().to_string();
        let saved_off = SoundSettings {
            ra_enabled: Some(false),
            ra_path: Some(nothing.clone()),
            ..SoundSettings::default()
        };
        // Saved off, no flag: off.
        let mut audio = tracing_audio(&root.0);
        start_audio_with(&root.0, None, &saved_off, &mut audio);
        assert_eq!(audio.original_audio_status(), RaStatus::Off);
        // Saved off, with the flag: reading that folder.
        let mut audio = tracing_audio(&root.0);
        start_audio_with(&root.0, Some(nothing.clone()), &saved_off, &mut audio);
        assert_ne!(audio.original_audio_status(), RaStatus::Off);
        // Saved on with a folder, no flag: reading it too (the saved path is used).
        let saved_on = SoundSettings {
            ra_enabled: Some(true),
            ra_path: Some(nothing),
            ..SoundSettings::default()
        };
        let mut audio = tracing_audio(&root.0);
        start_audio_with(&root.0, None, &saved_on, &mut audio);
        assert_ne!(audio.original_audio_status(), RaStatus::Off);
        // Saved on with no folder at all: nothing to read, no error, the game's own sounds.
        let no_folder = SoundSettings {
            ra_enabled: Some(true),
            ..SoundSettings::default()
        };
        let mut audio = tracing_audio(&root.0);
        start_audio_with(&root.0, None, &no_folder, &mut audio);
        assert_eq!(audio.original_audio_status(), RaStatus::Off);
    }

    #[test]
    fn the_switch_on_without_files_reads_nothing_and_the_game_keeps_its_own_sounds() {
        let root = root_with("ra_missing", &["unit.move"]);
        let folder = root.0.join("no_such_folder").display().to_string();
        let mut audio = tracing_audio(&root.0);
        let saved = SoundSettings {
            ra_enabled: Some(true),
            ra_path: Some(folder),
            ..SoundSettings::default()
        };
        start_audio_with(&root.0, None, &saved, &mut audio);
        // Read, found nothing: no sound of the Red Alert files replaces any event.
        assert_eq!(settled_ra_status(&mut audio), RaStatus::Ready { mapped: 0 });
        assert_eq!(audio.original_audio_catalog().unwrap().sound_count(), 0);
        // The event still has the bank's sound, and plays it: no panic, no error.
        let mut voices = UnitVoices::new(AudioConfig::embedded().voices);
        assert!(voices.speak(UnitVoice::Move, 10_000, &mut audio));
        assert_eq!(audio.take_trace().len(), 1);
    }

    #[test]
    fn the_master_test_is_heard_with_the_effects_and_the_voices_at_zero() {
        let root = root_with("master_test", &[TEST_SOUND]);
        let mut audio = tracing_audio(&root.0);
        let saved = SoundSettings {
            master: Some(80),
            effects: Some(0),
            voices: Some(0),
            ..SoundSettings::default()
        };
        start_audio_with(&root.0, None, &saved, &mut audio);
        play_test_sound(&mut audio, Volume::Master);
        play_test_sound(&mut audio, Volume::Effects);
        let gains: Vec<f32> = audio.take_trace().iter().map(|t| t.gain).collect();
        assert_eq!(gains.len(), 2, "{gains:?}");
        assert!(near(gains[0], 0.8), "master test: {gains:?}");
        assert!(
            near(gains[1], 0.0),
            "the effects test follows its slider: {gains:?}"
        );
    }

    #[test]
    fn the_test_sounds_are_in_the_group_of_their_slider_and_the_music_has_none() {
        let root = root_with("test_sound", &[TEST_SOUND]);
        let mut audio = tracing_audio(&root.0);
        start_audio_with(&root.0, None, &four_volumes(), &mut audio);
        // The click's own volume in the real bank is 0.8; here (a scratch bank) it is 1.0.
        play_test_sound(&mut audio, Volume::Master);
        play_test_sound(&mut audio, Volume::Effects);
        play_test_sound(&mut audio, Volume::Voices);
        play_test_sound(&mut audio, Volume::Music);
        let gains: Vec<f32> = audio.take_trace().iter().map(|t| t.gain).collect();
        assert_eq!(gains.len(), 3, "the music has no test sound: {gains:?}");
        assert!(
            near(gains[0], 0.9),
            "master test plays in the interface group, under the master volume only: {gains:?}"
        );
        assert!(near(gains[1], 0.5 * 0.9), "{gains:?}");
        assert!(near(gains[2], 0.3 * 0.9), "{gains:?}");
        // The test sound is one of the shipped bank (CC0), never a mapped Red Alert one.
        assert!(
            sf_audio::Bank::load(&assets().join("audio/bank.ron"))
                .unwrap()
                .get(TEST_SOUND)
                .is_some()
        );
    }
}
