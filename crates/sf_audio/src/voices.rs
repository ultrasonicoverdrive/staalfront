//! Voice limiting: no more than N live instances per sound and M in total,
//! so 100 identical shots in one frame do not stack into noise and a
//! flood cannot exhaust the mixer. Pure bookkeeping, generic over the
//! backend's handle type so it is testable without a device.

use std::collections::BTreeMap;

/// Default cap over all sounds together.
pub const DEFAULT_MAX_TOTAL: usize = 64;

/// Why a play request was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// `max_instances` of this sound reached.
    SoundFull,
    /// Global cap reached.
    TotalFull,
}

pub struct VoiceLimiter<H> {
    voices: BTreeMap<String, Vec<H>>,
    max_total: usize,
}

impl<H> VoiceLimiter<H> {
    pub fn new(max_total: usize) -> Self {
        Self {
            voices: BTreeMap::new(),
            max_total,
        }
    }

    /// Forget finished voices (`is_alive` false).
    pub fn prune(&mut self, is_alive: impl Fn(&H) -> bool) {
        for list in self.voices.values_mut() {
            list.retain(&is_alive);
        }
        self.voices.retain(|_, list| !list.is_empty());
    }

    /// Live voices of one sound (as of the last [`prune`](Self::prune)).
    pub fn count(&self, name: &str) -> usize {
        self.voices.get(name).map_or(0, Vec::len)
    }

    pub fn total(&self) -> usize {
        self.voices.values().map(Vec::len).sum()
    }

    /// Whether one more instance of `name` may start now.
    pub fn check(&self, name: &str, max_instances: u32) -> Result<(), Refused> {
        if self.count(name) >= max_instances as usize {
            Err(Refused::SoundFull)
        } else if self.total() >= self.max_total {
            Err(Refused::TotalFull)
        } else {
            Ok(())
        }
    }

    /// Record a started voice.
    pub fn add(&mut self, name: &str, handle: H) {
        self.voices
            .entry(name.to_string())
            .or_default()
            .push(handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_sound_limit_drops_the_flood() {
        let mut v = VoiceLimiter::<u32>::new(64);
        let mut played = 0;
        for i in 0..100 {
            if v.check("shot", 4).is_ok() {
                v.add("shot", i);
                played += 1;
            }
        }
        assert_eq!(played, 4);
        assert_eq!(v.count("shot"), 4);
        assert_eq!(v.check("shot", 4), Err(Refused::SoundFull));
    }

    #[test]
    fn other_sounds_are_unaffected_by_a_full_one() {
        let mut v = VoiceLimiter::<u32>::new(64);
        v.add("a", 1);
        assert_eq!(v.check("a", 1), Err(Refused::SoundFull));
        assert_eq!(v.check("b", 1), Ok(()));
    }

    #[test]
    fn total_cap_applies_across_sounds() {
        let mut v = VoiceLimiter::<u32>::new(3);
        v.add("a", 1);
        v.add("b", 2);
        v.add("c", 3);
        assert_eq!(v.total(), 3);
        assert_eq!(v.check("d", 8), Err(Refused::TotalFull));
    }

    #[test]
    fn finished_voices_free_their_slot() {
        let mut v = VoiceLimiter::<u32>::new(64);
        v.add("a", 1);
        v.add("a", 2);
        assert_eq!(v.check("a", 2), Err(Refused::SoundFull));
        v.prune(|&h| h != 1);
        assert_eq!(v.count("a"), 1);
        assert_eq!(v.check("a", 2), Ok(()));
        v.prune(|_| false);
        assert_eq!(v.total(), 0);
        assert_eq!(v.count("a"), 0);
    }
}
