//! Dynamic music (WP11.4): the calm/combat mood and the shuffled track order.
//!
//! Presentation side only: nothing here reaches `sf_sim`. The game feeds the
//! tracker the shots, hits and deaths it shows (with the sim tick they
//! happened in); the tracker says whether the music should be calm or combat.
//! All numbers are in `assets/config/audio.ron` (`music`), in ticks (20 per
//! second) and metres.

use std::collections::VecDeque;

use serde::Deserialize;

/// What the music should sound like.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mood {
    Calm,
    Combat,
}

impl Mood {
    pub const ALL: [Mood; 2] = [Mood::Calm, Mood::Combat];

    pub fn name(self) -> &'static str {
        match self {
            Mood::Calm => "calm",
            Mood::Combat => "combat",
        }
    }

    pub(crate) fn index(self) -> usize {
        match self {
            Mood::Calm => 0,
            Mood::Combat => 1,
        }
    }
}

/// How much one kind of event counts towards combat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventWeights {
    pub fire: u32,
    pub impact: u32,
    pub death: u32,
}

/// When calm turns to combat and back.
///
/// Calm -> combat: the weights of the counted events of the last
/// `window_ticks` add up to `enter_score` or more. Combat -> calm (hysteresis,
/// so it does not flip back and forth): at least `min_hold_ticks` since the
/// switch **and** no counted event for `calm_after_ticks`.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoodConfig {
    /// Only events within this many metres of the camera focus count.
    pub radius_m: f32,
    pub window_ticks: u64,
    pub enter_score: u32,
    pub min_hold_ticks: u64,
    pub calm_after_ticks: u64,
    pub weights: EventWeights,
}

/// Default music when no original Red Alert track is available for a mood:
/// the name of a sound of the bank (a CC0 track), or `None` for silence.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackTracks {
    pub calm: Option<String>,
    pub combat: Option<String>,
}

impl FallbackTracks {
    pub fn get(&self, mood: Mood) -> Option<&str> {
        match mood {
            Mood::Calm => self.calm.as_deref(),
            Mood::Combat => self.combat.as_deref(),
        }
    }
}

/// `music` of `audio.ron`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MusicConfig {
    /// Crossfade between two tracks.
    pub crossfade_ms: u32,
    pub mood: MoodConfig,
    pub fallback: FallbackTracks,
}

impl MusicConfig {
    pub fn validate(&self) -> Result<(), String> {
        let m = &self.mood;
        if !(m.radius_m.is_finite() && m.radius_m > 0.0) {
            return Err("music.mood.radius_m must be positive".into());
        }
        if m.window_ticks == 0 || m.calm_after_ticks == 0 {
            return Err("music.mood.window_ticks and calm_after_ticks must be at least 1".into());
        }
        if m.enter_score == 0 {
            return Err("music.mood.enter_score must be at least 1".into());
        }
        if self.crossfade_ms > 30_000 {
            return Err("music.crossfade_ms must be at most 30000".into());
        }
        Ok(())
    }
}

/// Decides calm or combat from the events it is given.
pub struct MoodTracker {
    cfg: MoodConfig,
    /// (tick, weight) of the counted events still inside the window.
    events: VecDeque<(u64, u32)>,
    mood: Mood,
    since: u64,
    last_event: u64,
}

impl MoodTracker {
    pub fn new(cfg: MoodConfig) -> Self {
        Self {
            cfg,
            events: VecDeque::new(),
            mood: Mood::Calm,
            since: 0,
            last_event: 0,
        }
    }

    pub fn config(&self) -> &MoodConfig {
        &self.cfg
    }

    pub fn mood(&self) -> Mood {
        self.mood
    }

    /// A new game: calm, nothing remembered.
    pub fn reset(&mut self) {
        *self = Self::new(self.cfg);
    }

    /// One counted event of `weight` (0 is ignored) that happened in `tick`.
    pub fn add(&mut self, tick: u64, weight: u32) {
        if weight == 0 {
            return;
        }
        self.events.push_back((tick, weight));
        self.last_event = self.last_event.max(tick);
    }

    /// The mood at tick `now`.
    pub fn update(&mut self, now: u64) -> Mood {
        let from = now.saturating_sub(self.cfg.window_ticks);
        self.events.retain(|&(t, _)| t > from);
        match self.mood {
            Mood::Calm => {
                let score: u64 = self.events.iter().map(|&(_, w)| u64::from(w)).sum();
                if score >= u64::from(self.cfg.enter_score) {
                    self.mood = Mood::Combat;
                    self.since = now;
                }
            }
            Mood::Combat => {
                let held = now.saturating_sub(self.since) >= self.cfg.min_hold_ticks;
                let quiet = now.saturating_sub(self.last_event) >= self.cfg.calm_after_ticks;
                if held && quiet {
                    self.mood = Mood::Calm;
                    self.since = now;
                }
            }
        }
        self.mood
    }
}

/// Plays every track of a pool once in random order before any repeats (a
/// shuffled bag), and never the same track twice in a row. Presentation-side
/// random numbers: a small xorshift, the seed comes from the caller (per launch in `AudioSystem::new`).
#[derive(Clone)]
pub struct Shuffle {
    rng: u32,
    bag: Vec<usize>,
    len: usize,
    last: Option<usize>,
}

impl Shuffle {
    pub fn new(seed: u32) -> Self {
        Self {
            rng: seed.max(1),
            bag: Vec::new(),
            len: 0,
            last: None,
        }
    }

    fn roll(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    /// The index (of `len` tracks) to play next; `None` for an empty pool.
    pub fn next(&mut self, len: usize) -> Option<usize> {
        if len == 0 {
            self.bag.clear();
            self.len = 0;
            return None;
        }
        if len != self.len {
            self.bag.clear();
            self.len = len;
            self.last = self.last.filter(|&l| l < len);
        }
        if self.bag.is_empty() {
            self.bag.extend(0..len);
            for i in (1..len).rev() {
                let j = self.roll() as usize % (i + 1);
                self.bag.swap(i, j);
            }
            // The bag is popped from the end: keep that from being the track
            // that just played.
            if len > 1 && self.bag.last().copied() == self.last {
                self.bag.swap(0, len - 1);
            }
        }
        let i = self.bag.pop();
        self.last = i;
        i
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> MoodConfig {
        MoodConfig {
            radius_m: 60.0,
            window_ticks: 100,
            enter_score: 6,
            min_hold_ticks: 200,
            calm_after_ticks: 300,
            weights: EventWeights {
                fire: 1,
                impact: 1,
                death: 3,
            },
        }
    }

    #[test]
    fn a_few_shots_stay_calm_a_fight_turns_to_combat() {
        let mut t = MoodTracker::new(cfg());
        t.add(10, 1);
        t.add(12, 1);
        assert_eq!(t.update(20), Mood::Calm);
        t.add(21, 1);
        t.add(22, 3);
        assert_eq!(t.update(22), Mood::Combat, "score 6 inside the window");
    }

    #[test]
    fn events_older_than_the_window_do_not_add_up() {
        let mut t = MoodTracker::new(cfg());
        for tick in [0, 40, 80, 120, 160, 200] {
            t.add(tick, 1);
            assert_eq!(t.update(tick), Mood::Calm, "one shot per 40 ticks is calm");
        }
    }

    #[test]
    fn combat_holds_then_returns_to_calm_only_after_a_quiet_spell() {
        let mut t = MoodTracker::new(cfg());
        t.add(100, 6);
        assert_eq!(t.update(100), Mood::Combat);
        // quiet is not enough before min_hold_ticks ... and the last event keeps it going
        assert_eq!(t.update(299), Mood::Combat);
        t.add(350, 1);
        assert_eq!(
            t.update(649),
            Mood::Combat,
            "299 ticks after the last event"
        );
        assert_eq!(t.update(650), Mood::Calm, "300 ticks after the last event");
    }

    #[test]
    fn it_does_not_flip_back_and_forth() {
        let mut t = MoodTracker::new(cfg());
        let mut switches = 0;
        let mut last = Mood::Calm;
        // a skirmish with a shot every 10 ticks for 1000 ticks
        for tick in 0..1000u64 {
            if tick % 10 == 0 {
                t.add(tick, 1);
            }
            let m = t.update(tick);
            if m != last {
                switches += 1;
                last = m;
            }
        }
        assert_eq!(switches, 1, "calm -> combat once, and it stays");
        assert_eq!(t.mood(), Mood::Combat);
    }

    #[test]
    fn zero_weight_events_and_reset() {
        let mut t = MoodTracker::new(cfg());
        t.add(5, 0);
        assert_eq!(t.update(5), Mood::Calm);
        t.add(6, 99);
        assert_eq!(t.update(6), Mood::Combat);
        t.reset();
        assert_eq!(t.mood(), Mood::Calm);
        assert_eq!(t.update(7), Mood::Calm);
    }

    #[test]
    fn shuffle_plays_every_track_once_per_round_and_never_repeats_in_a_row() {
        let mut s = Shuffle::new(7);
        let mut prev = None;
        for _round in 0..20 {
            let mut seen = [false; 5];
            for _ in 0..5 {
                let i = s.next(5).unwrap();
                assert!(!seen[i], "each track once per round");
                seen[i] = true;
                assert_ne!(Some(i), prev, "no immediate repeat");
                prev = Some(i);
            }
            assert!(seen.iter().all(|&b| b));
        }
    }

    #[test]
    fn shuffle_edge_cases() {
        let mut s = Shuffle::new(1);
        assert_eq!(s.next(0), None);
        assert_eq!(s.next(1), Some(0));
        assert_eq!(
            s.next(1),
            Some(0),
            "one track repeats, nothing else to play"
        );
        // the pool shrinks: the index stays in range
        for _ in 0..50 {
            assert!(s.next(3).unwrap() < 3);
        }
        // same seed, same order (no clock)
        let a: Vec<_> = {
            let mut s = Shuffle::new(99);
            (0..12).map(|_| s.next(6).unwrap()).collect()
        };
        let b: Vec<_> = {
            let mut s = Shuffle::new(99);
            (0..12).map(|_| s.next(6).unwrap()).collect()
        };
        assert_eq!(a, b);
    }
}
