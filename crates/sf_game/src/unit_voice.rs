//! Unit voices (WP11.2): the replies of the player's own units to selecting, moving and attacking.
//! Rules in `docs/rules/voices.md`.
//!
//! These are the player's own actions, so they never come from the simulation: the runner tells this
//! module what the player just did and it decides whether a voice plays. Three events, the names of
//! `assets/audio/bank.ron` and `assets/audio/ra_mapping.ron`: `unit.select`, `unit.move`,
//! `unit.attack`.
//!
//! * [`selection_frame`]: which voice a selection step raises (pure).
//! * [`order_voice`]: which voice an order raises (pure).
//! * [`UnitVoices`]: the brake ([`sf_audio::VoiceBrake`]) and the call to the audio.
//!
//! Only the local player's own mobile units speak. A selection holds only own entities (an enemy is
//! inspected, never selected, and a hidden one is not found at all, see `selection::facts`), and
//! [`Selection::orderable`](crate::selection::Selection::orderable) leaves out structures, so the
//! callers hand in that list and nothing else can speak.

use sf_audio::{AudioSystem, VoiceBrake, VoiceConfig};
use sf_sim::{Command, EntityId, Order};

/// What a unit says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitVoice {
    /// The unit was picked.
    Select,
    /// It was told to go somewhere.
    Move,
    /// It was told to attack.
    Attack,
}

impl UnitVoice {
    pub const ALL: [UnitVoice; 3] = [UnitVoice::Select, UnitVoice::Move, UnitVoice::Attack];

    /// The sound event of the bank and of `ra_mapping.ron`.
    pub fn event(self) -> &'static str {
        match self {
            UnitVoice::Select => "unit.select",
            UnitVoice::Move => "unit.move",
            UnitVoice::Attack => "unit.attack",
        }
    }
}

/// The voice of a selection step, once per frame. `acted` says the player did something that sets
/// the selection this frame (a click, the release of a drag, a double-click, a group key): without it
/// nothing is said, however many frames a drag takes or however long the selection stays. `before` and
/// `after` are the orderable units (own mobile units) before and after the step. A step that leaves
/// no own unit selected says nothing, and a shift-click that only removes a unit says nothing: it
/// has to bring a unit that was not selected before.
pub fn selection_frame(
    acted: bool,
    additive: bool,
    before: &[EntityId],
    after: &[EntityId],
) -> Option<UnitVoice> {
    if !acted || after.is_empty() {
        return None;
    }
    if additive && after.iter().all(|id| before.contains(id)) {
        return None;
    }
    Some(UnitVoice::Select)
}

/// The voice of an order the player gave. Moving and guarding answer with the move voice, attack
/// and attack-move with the attack voice; stop, scatter and hold position say nothing.
pub fn order_voice(command: &Command) -> Option<UnitVoice> {
    let Command::Order { ids, order, .. } = command else {
        return None;
    };
    if ids.is_empty() {
        return None;
    }
    match order {
        Order::Move(_) | Order::Guard(_) => Some(UnitVoice::Move),
        Order::Attack(_) | Order::AttackMove(_) | Order::Pursue(_) => Some(UnitVoice::Attack),
        Order::Idle | Order::HoldPosition | Order::Scatter => None,
    }
}

/// What the voices need from the audio: [`AudioSystem`] in the game, a stand-in in tests.
pub trait VoiceOut {
    /// The event has a sound to play (the bank, or a mapped Red Alert sound). Never warns.
    fn has_sound(&self, event: &str) -> bool;
    /// A unit voice is still sounding.
    fn busy(&self) -> bool;
    fn play(&mut self, event: &str);
}

impl VoiceOut for AudioSystem {
    fn has_sound(&self, event: &str) -> bool {
        AudioSystem::has_sound(self, event)
    }

    fn busy(&self) -> bool {
        let names: Vec<&str> = UnitVoice::ALL.iter().map(|v| v.event()).collect();
        self.is_playing(&names)
    }

    fn play(&mut self, event: &str) {
        AudioSystem::play(self, event);
    }
}

/// The brake and the call to the audio.
pub struct UnitVoices {
    brake: VoiceBrake,
}

impl UnitVoices {
    /// The brake with the numbers of `voices` in `audio.ron`.
    pub fn new(config: VoiceConfig) -> Self {
        Self {
            brake: VoiceBrake::new(config),
        }
    }

    /// Forget the last voice (a new game).
    pub fn reset(&mut self) {
        self.brake.reset();
    }

    /// Say `voice` at `now_ms` if it may be heard: the event has a sound (otherwise silent, no
    /// message, no gap used up), no unit voice is sounding, and the gaps of the brake have passed.
    /// Not positional: a unit voice is the player's own interface and always at full volume
    /// (category Sfx, so `mix.sfx_volume` applies). Returns whether it was played.
    pub fn speak(&mut self, voice: UnitVoice, now_ms: u64, out: &mut dyn VoiceOut) -> bool {
        let event = voice.event();
        if !out.has_sound(event) {
            return false;
        }
        if !self.brake.allow(event, now_ms, out.busy()) {
            return false;
        }
        out.play(event);
        true
    }
}

#[cfg(test)]
mod tests;
