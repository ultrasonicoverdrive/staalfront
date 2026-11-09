//! The brake on unit voices (WP11.2): a pure decision, "play or skip", from the event, the time and
//! whether a voice is still sounding. No device, no clock of its own: the caller passes the time in
//! milliseconds. Rules in `docs/rules/voices.md`, numbers in `assets/config/audio.ron` (`voices`).

use serde::Deserialize;

/// `voices` of `assets/config/audio.ron`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceConfig {
    /// After a voice has started, no other voice starts within this many milliseconds.
    pub min_gap_ms: u32,
    /// After a voice has started, the same event does not start again within this many milliseconds
    /// (at least `min_gap_ms`): clicking one unit ten times is not ten replies.
    pub repeat_gap_ms: u32,
}

impl VoiceConfig {
    /// Range checks.
    pub fn validate(&self) -> Result<(), String> {
        const MAX: u32 = 60_000;
        if self.min_gap_ms > MAX || self.repeat_gap_ms > MAX {
            return Err(format!("voices: the gaps must be at most {MAX} ms"));
        }
        if self.repeat_gap_ms < self.min_gap_ms {
            return Err("voices.repeat_gap_ms must not be below min_gap_ms".into());
        }
        Ok(())
    }
}

/// Remembers when the last voice started and which event it was.
#[derive(Clone, Debug)]
pub struct VoiceBrake {
    config: VoiceConfig,
    last: Option<(u64, String)>,
}

impl VoiceBrake {
    pub fn new(config: VoiceConfig) -> Self {
        Self { config, last: None }
    }

    /// Forget the last voice (a new game).
    pub fn reset(&mut self) {
        self.last = None;
    }

    /// Whether a voice for `event` may start at `now_ms`. `busy` says a unit voice is still sounding:
    /// then nothing starts (one voice at a time; the one playing is not cut off). A `true` answer is
    /// taken as "it started": it opens the gap. Call it only when the voice will really be played, so a
    /// silent event (no sound mapped) never uses up the gap.
    pub fn allow(&mut self, event: &str, now_ms: u64, busy: bool) -> bool {
        if busy {
            return false;
        }
        if let Some((at, last_event)) = &self.last {
            let since = now_ms.saturating_sub(*at);
            let gap = if last_event == event {
                self.config.repeat_gap_ms
            } else {
                self.config.min_gap_ms
            };
            if since < u64::from(gap) {
                return false;
            }
        }
        self.last = Some((now_ms, event.to_string()));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: VoiceConfig = VoiceConfig {
        min_gap_ms: 400,
        repeat_gap_ms: 1200,
    };

    #[test]
    fn the_first_voice_plays() {
        let mut b = VoiceBrake::new(CFG);
        assert!(b.allow("unit.select", 0, false));
    }

    #[test]
    fn another_event_waits_for_the_min_gap() {
        let mut b = VoiceBrake::new(CFG);
        assert!(b.allow("unit.select", 1000, false));
        assert!(!b.allow("unit.move", 1399, false));
        assert!(b.allow("unit.move", 1400, false));
    }

    #[test]
    fn the_same_event_waits_for_the_longer_repeat_gap() {
        let mut b = VoiceBrake::new(CFG);
        assert!(b.allow("unit.move", 0, false));
        assert!(!b.allow("unit.move", 400, false));
        assert!(!b.allow("unit.move", 1199, false));
        assert!(b.allow("unit.move", 1200, false));
    }

    #[test]
    fn a_voice_that_is_still_sounding_is_not_cut_off() {
        let mut b = VoiceBrake::new(CFG);
        assert!(b.allow("unit.select", 0, false));
        // Long past both gaps, but the first voice is still going.
        assert!(!b.allow("unit.move", 5000, true));
        // A refused request does not open a gap: the next one is judged on the voice that did start.
        assert!(b.allow("unit.move", 5001, false));
    }

    #[test]
    fn a_refused_request_does_not_move_the_gap() {
        let mut b = VoiceBrake::new(CFG);
        assert!(b.allow("unit.move", 0, false));
        assert!(!b.allow("unit.move", 1000, false));
        // Judged from t=0 (the voice that played), not from the refused t=1000.
        assert!(b.allow("unit.move", 1200, false));
    }

    #[test]
    fn reset_forgets_the_last_voice() {
        let mut b = VoiceBrake::new(CFG);
        assert!(b.allow("unit.move", 0, false));
        b.reset();
        assert!(b.allow("unit.move", 10, false));
    }

    #[test]
    fn config_checks() {
        assert!(CFG.validate().is_ok());
        let bad = VoiceConfig {
            min_gap_ms: 500,
            repeat_gap_ms: 100,
        };
        assert!(bad.validate().is_err());
        let huge = VoiceConfig {
            min_gap_ms: 0,
            repeat_gap_ms: 1_000_000,
        };
        assert!(huge.validate().is_err());
    }
}
