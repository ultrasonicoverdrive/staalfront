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

use sf_audio::ra::RaAudioConfig;
use sf_audio::{
    AudioConfig, AudioSystem, Category, Cue, Listener, PlanStats, Planner, Request, SpatialParams,
};

use crate::fx::FxPlayer;
use crate::fx_events::FxEvent;

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
    let (mut config, warning) = AudioConfig::load(asset_root);
    if let Some(path) = ra_override {
        config.ra_audio = RaAudioConfig {
            enabled: true,
            path,
        };
    }
    audio.enable_original_audio(&config.ra_audio);
    audio.set_spatial_params(config.spatial.params());
    audio.set_category_volume(Category::Sfx, config.mix.sfx_volume);
    (FxSound::new(&config), warning)
}

/// Turns the shown events into sounds. See the module documentation.
pub struct FxSound {
    planner: Planner,
    totals: SoundTotals,
}

impl FxSound {
    /// The rules from `config`. The spatial numbers are not kept here: each frame they come from the
    /// [`AudioSystem`] (`spatial_params`), so the plan and the output use one copy.
    pub fn new(config: &AudioConfig) -> FxSound {
        FxSound {
            planner: Planner::new(config.positional.clone()),
            totals: SoundTotals::default(),
        }
    }

    pub fn totals(&self) -> SoundTotals {
        self.totals
    }

    /// Forget when things last played (a new game).
    pub fn reset(&mut self) {
        self.planner.reset();
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
        // The shipped bank has no weapon, impact or explosion sounds: nothing plays, nothing is counted as unknown.
        let (mut fx, mut sound) = setup();
        let mut audio = tracing_audio(&assets());
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
}
