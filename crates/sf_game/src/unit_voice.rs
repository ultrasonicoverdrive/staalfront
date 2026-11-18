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
//! * [`speaker_faction`]: which side's voice speaks (WP11.2d): Meridian joe, Veyran cori, Neutral silent.
//!
//! Only the local player's own mobile units speak. A selection holds only own entities (an enemy is
//! inspected, never selected, and a hidden one is not found at all, see `selection::facts`), and
//! [`Selection::orderable`](crate::selection::Selection::orderable) leaves out structures, so the
//! callers hand in that list and nothing else can speak.

use sf_audio::{AudioSystem, VoiceBrake, VoiceConfig};
use sf_data::units::Faction;
use sf_sim::{Command, EntityId, Order};

use crate::sandbox::faction_of_owner;
use crate::sim_snapshot::SimSnapshot;

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

    /// The sound event of the bank and of `ra_mapping.ron`: the Meridian voice (joe), and the name the
    /// brake and the Red Alert mapping know the voice by, whichever side speaks.
    pub fn event(self) -> &'static str {
        match self {
            UnitVoice::Select => "unit.select",
            UnitVoice::Move => "unit.move",
            UnitVoice::Attack => "unit.attack",
        }
    }

    /// The bank event of the Veyran voice (cori, WP11.2d). Not in `ra_mapping.ron`: with Red Alert audio
    /// the original of [`UnitVoice::event`] speaks for both sides.
    pub fn veyran_event(self) -> &'static str {
        match self {
            UnitVoice::Select => "unit.select.veyran",
            UnitVoice::Move => "unit.move.veyran",
            UnitVoice::Attack => "unit.attack.veyran",
        }
    }

    /// The bank event of the voice of `faction`; `None` for Neutral, which never speaks.
    pub fn event_for(self, faction: Faction) -> Option<&'static str> {
        match faction {
            Faction::Meridian => Some(self.event()),
            Faction::Veyran => Some(self.veyran_event()),
            Faction::Neutral => None,
        }
    }
}

/// The side that speaks for these units: the faction of the owner of the unit with the lowest id that
/// belongs to a side (WP11.2d). A selection only holds the local player's own units, so it is one side; the
/// rule for a mixed list (units of both sides) is the lowest id, the leading unit, and a unit of nobody (a
/// neutral, an owner without a seat, a stale id) is skipped. `None`: nobody to speak.
pub fn speaker_faction(world: &SimSnapshot, ids: &[EntityId]) -> Option<Faction> {
    let mut sorted: Vec<EntityId> = ids.to_vec();
    sorted.sort_unstable();
    sorted.into_iter().find_map(|id| {
        let owner = world.get(id)?.owner;
        faction_of_owner(owner.0).filter(|f| *f != Faction::Neutral)
    })
}

/// The units an order is given to; empty for a command that is not an order.
pub fn order_units(command: &Command) -> &[EntityId] {
    match command {
        Command::Order { ids, .. } => ids,
        _ => &[],
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
    /// The event plays as a mapped original (a Red Alert voice that was found), not a bank file.
    fn original(&self, event: &str) -> bool;
    /// A unit voice is still sounding.
    fn busy(&self) -> bool;
    fn play(&mut self, event: &str);
}

impl VoiceOut for AudioSystem {
    fn has_sound(&self, event: &str) -> bool {
        AudioSystem::has_sound(self, event)
    }

    fn original(&self, event: &str) -> bool {
        self.is_original(event)
    }

    fn busy(&self) -> bool {
        // One voice at a time over the unit voices and the announcements (WP11.2c).
        self.is_playing(&crate::announce::voice_channel())
    }

    fn play(&mut self, event: &str) {
        // The voice volume of the settings, not the effects' (WP11.5b).
        AudioSystem::play_as(self, event, sf_audio::Category::Voice);
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

    /// The bank event that speaks `voice` for `faction` (WP11.2d), or `None` when nothing speaks: Neutral
    /// is silent. Meridian speaks the plain event (joe). Veyran speaks its own event (cori), unless
    /// the plain event plays a mapped Red Alert original (that one speaks for both sides, as before), or the
    /// bank has no Veyran event (a bank without per-side voices: the plain event, as before WP11.2d).
    pub fn event_to_play(
        voice: UnitVoice,
        faction: Faction,
        out: &dyn VoiceOut,
    ) -> Option<&'static str> {
        let own = voice.event_for(faction)?;
        if own != voice.event() && !out.original(voice.event()) && out.has_sound(own) {
            Some(own)
        } else {
            Some(voice.event())
        }
    }

    /// Say `voice` of a unit of `faction` at `now_ms` if it may be heard: the faction is not Neutral, the
    /// event has a sound (otherwise silent, no message, no gap used up), no unit voice is sounding, and the
    /// gaps of the brake have passed. The brake knows the plain event (`unit.select`) for both sides, so a
    /// Meridian and a Veyran voice share the gaps, as one voice channel should. Not positional: a unit voice
    /// is the player's own interface and always at full volume (category Voice, so the voice volume and the
    /// master volume apply). Returns whether it was played.
    pub fn speak(
        &mut self,
        voice: UnitVoice,
        faction: Faction,
        now_ms: u64,
        out: &mut dyn VoiceOut,
    ) -> bool {
        let Some(event) = Self::event_to_play(voice, faction, out) else {
            return false;
        };
        if !out.has_sound(event) {
            return false;
        }
        if !self.brake.allow(voice.event(), now_ms, out.busy()) {
            return false;
        }
        out.play(event);
        true
    }
}

#[cfg(test)]
mod tests;
