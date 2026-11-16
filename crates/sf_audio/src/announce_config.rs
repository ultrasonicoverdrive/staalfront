//! The numbers of the announcements (WP11.2c): `announce` in `assets/config/audio.ron`. The decision
//! "play or wait" is made by `sf_game::announce::Announcer`; this is only the data and its checks.
//! Rules in `docs/rules/announcements.md`.

use std::collections::BTreeMap;

use serde::Deserialize;

/// How one announcement is treated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnounceRule {
    /// Higher goes first when several wait.
    pub priority: u8,
    /// After this announcement has started, the same one is not raised again within this many milliseconds
    /// (at least `min_gap_ms`): a base that is hit for a minute says so once in a while, not every second.
    pub repeat_gap_ms: u32,
}

/// `announce` of `assets/config/audio.ron`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnounceConfig {
    /// After an announcement has started, no other one starts within this many milliseconds.
    pub min_gap_ms: u32,
    /// An announcement that has waited this long (for a unit voice or another announcement to end) is dropped:
    /// news that is old is not news.
    pub max_wait_ms: u32,
    /// At most this many announcements wait; when it is full the lowest priority (then the oldest) goes.
    pub queue_len: u32,
    /// Ticks after a game starts in which nothing is raised: the starting objects come into the world and must
    /// not be announced as news. The state of the game at the end of it is the baseline.
    pub settle_ticks: u32,
    /// The rule of an event that has none of its own.
    pub fallback: AnnounceRule,
    /// Rules per event name (`announce.base_under_attack`, ...).
    pub events: BTreeMap<String, AnnounceRule>,
}

impl AnnounceConfig {
    /// The rule of `event`.
    pub fn rule(&self, event: &str) -> AnnounceRule {
        self.events.get(event).copied().unwrap_or(self.fallback)
    }

    /// Range checks.
    pub fn validate(&self) -> Result<(), String> {
        const MAX_MS: u32 = 600_000;
        if self.min_gap_ms > 60_000 {
            return Err("announce.min_gap_ms must be at most 60000".into());
        }
        if self.max_wait_ms == 0 || self.max_wait_ms > 60_000 {
            return Err("announce.max_wait_ms must be in 1..=60000".into());
        }
        if !(1..=16).contains(&self.queue_len) {
            return Err("announce.queue_len must be in 1..=16".into());
        }
        if self.settle_ticks > 6000 {
            return Err("announce.settle_ticks must be at most 6000".into());
        }
        let check = |what: &str, r: &AnnounceRule| {
            if r.repeat_gap_ms < self.min_gap_ms || r.repeat_gap_ms > MAX_MS {
                Err(format!(
                    "{what}: repeat_gap_ms must be in min_gap_ms..={MAX_MS}"
                ))
            } else {
                Ok(())
            }
        };
        check("announce.fallback", &self.fallback)?;
        for (name, rule) in &self.events {
            if !name.starts_with("announce.") {
                return Err(format!(
                    "announce.events: \"{name}\" is not an announce.* event"
                ));
            }
            check(&format!("announce.events.\"{name}\""), rule)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> AnnounceConfig {
        AnnounceConfig {
            min_gap_ms: 2000,
            max_wait_ms: 4000,
            queue_len: 4,
            settle_ticks: 100,
            fallback: AnnounceRule {
                priority: 1,
                repeat_gap_ms: 5000,
            },
            events: BTreeMap::new(),
        }
    }

    #[test]
    fn an_event_without_a_rule_takes_the_fallback() {
        let mut c = cfg();
        c.events.insert(
            "announce.victory".into(),
            AnnounceRule {
                priority: 9,
                repeat_gap_ms: 3000,
            },
        );
        assert_eq!(c.rule("announce.victory").priority, 9);
        assert_eq!(c.rule("announce.nothing"), c.fallback);
    }

    #[test]
    fn nonsense_is_refused() {
        assert!(cfg().validate().is_ok());
        let mut c = cfg();
        c.fallback.repeat_gap_ms = 1999;
        assert!(c.validate().is_err(), "repeat gap under the min gap");
        let mut c = cfg();
        c.queue_len = 0;
        assert!(c.validate().is_err());
        let mut c = cfg();
        c.max_wait_ms = 0;
        assert!(c.validate().is_err());
        let mut c = cfg();
        c.events.insert("unit.select".into(), c.fallback);
        assert!(c.validate().is_err(), "not an announce.* name");
    }
}
