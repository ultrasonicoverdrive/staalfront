# Unit voices (WP11.2)

The replies of the player's own units when they are selected, told to move, or told to attack. Presentation
only: nothing here reaches `sf_sim`, nothing is hashed, no pinned checksum can move. These are the player's own
actions, so there is no sim event; the runner tells `sf_game::unit_voice` what the player just did.

## 1. Three events

| Event | Raised by |
|---|---|
| `unit.select` | A selection step that leaves at least one own mobile unit selected: a click on an own unit, the release of a box drag, a double click (all units of the kind on screen), a control group recalled with a number key. |
| `unit.move` | The right-click order *Move* (also queued), and *Guard* (acknowledged with the move voice: no fourth event). |
| `unit.attack` | The right-click orders *Attack* (on a seen enemy) and *Attack-move* (`F` held). |

The names are the sound events of `assets/audio/bank.ron` and `assets/audio/ra_mapping.ron`. Nothing else
speaks: stop (`C`), scatter (`X`) and hold position (`H`) are silent, so is a selection that is only an
inspected enemy or neutral unit, a structure selected on its own, bare ground, and a shift-click that only
removes a unit from the selection. A unit that is not seen by the local player cannot be selected, inspected or
attacked at all (`selection::facts`, `fog_look.md`), so a hidden enemy is never voiced and never revealed by a
voice.

Decisions:

* **Which unit speaks.** None in particular: one event per action, whatever the number of units. The first
  selected unit (lowest id) is the natural speaker once the bank has a voice per unit type; that is a later
  step (a `unit.select.<role>` name that falls back to `unit.select`), not built.
* **A structure alone is silent.** Only mobile units speak (`Selection::orderable`); the three events
  are unit replies, and a structure's reply is not part of this WP.
* **One selection step is one voice.** The voice comes from the step that sets the selection, not from the state
  of the selection: a drag held for 30 frames says nothing until the button is released, and a selection that
  just stays says nothing on the frames after it. Clicking the same unit again is a new step, so it speaks again
  if the brake allows it.
* **Not positional.** A unit voice is the player's own interface: `AudioSystem::play`, full volume, category Sfx,
  so `mix.sfx_volume` of `audio.ron` applies. (WP11.5b, the sound settings, is not built; when it is, a voice
  volume belongs next to the sfx volume. No hook beyond the category exists yet.)

## 2. The brake

Numbers in `assets/config/audio.ron`, section `voices`, pinned by `config::tests`:

| Number | Shipped | Meaning |
|---|---|---|
| `min_gap_ms` | 400 | After a voice has started, no other voice starts within this many milliseconds. |
| `repeat_gap_ms` | 1200 | After a voice has started, the *same* event does not start again within this many milliseconds (at least `min_gap_ms`). |

And **one voice at a time**: while a voice of any of the three events is still sounding
(`AudioSystem::is_playing`, from the live voice handles), nothing new starts. The voice that is playing is not
cut off and a skipped request is not queued: it is simply dropped. A selection followed within 400 ms by a move
order therefore plays the selection voice only; the order itself is given either way.

`sf_audio::VoiceBrake::allow(event, now_ms, busy)` is the pure decision: it answers play or skip and, when it
says play, remembers the time and the event. A skipped request does not move the gaps. The caller
(`UnitVoices::speak`) asks it only for an event that has a sound, so **an event without a sound (no Red Alert
file mapped, nothing in `bank.ron`) is silent, prints nothing, counts no unknown sound and uses up no gap.**

## 3. Sound sources

* Without the player's Red Alert files the three events are silent: `bank.ron` has no CC0 sound for them yet. A
  CC0 sound added to the bank under these names plays at once (with its line in `assets/LICENSES.md`).
* With `--ra-audio` (or `ra_audio` in `audio.ron`) the mapped voice plays. `ra_mapping.ron` has had defaults for the
  three events since WP11.7 (voice lines carry a variant as extension, `V01` etc.); they were not changed here.
  Which variant sounds right is chosen by ear in the F1 sound browser, and an entry whose file is not found
  keeps the event silent with the loader's one warning. This WP adds no Red Alert name.

## 4. Where it is wired

* `crates/sf_audio/src/voice_brake.rs`: `VoiceConfig`, `VoiceBrake`.
* `crates/sf_game/src/unit_voice.rs`: `UnitVoice` (the three events), `selection_frame` (which selection step
  speaks), `order_voice` (which order speaks), `UnitVoices::speak` (the sound check, the brake, the call).
* `runner.rs`: `handle_selection` takes the orderable units before its step and calls `selection_frame` with
  whether a gesture or a group recall happened; `handle_orders` calls `order_voice` on the command a right-click
  produced; both go through `Runner::say`.

## 5. Tests

`sf_audio` `voice_brake::tests` (gaps, repeat gap, busy, a refused request does not move the gap, reset, config
checks), `config::tests` (the shipped numbers), `live::tests::playing_follows_a_voice_from_start_to_end`.
`sf_game` `unit_voice::tests` with the production `Selection`, `Pointer`, `commands::right_click` and `AudioSystem`:
click, enemy and neutral click, an enemy in the fog, a structure alone, shift add and remove, the 30 frames of a
box drag and the frames after it, double click, group recall and an empty group, move, queued move, attack,
attack-move, stop/scatter/hold, the spam thinned by the gaps, a busy voice, a silent event using no gap, the
shipped bank staying silent, a bank sound played through a real `AudioSystem` (recorded by its trace).

**Checked by reading only:** the two call sites in `runner.rs` need a window (`Context`, the pointer, the
actions). They are four short additions (`before`/`acted`/`additive_step` in `handle_selection`, and
`order_voice` in `handle_orders`); the decisions they call are the tested functions above. A hearing test needs
the player's own Red Alert files and a sound device, neither of which CI has.

## 6. Gaps

* One voice per action, not per unit type or per unit; the bank has no per-type voices.
* A selection whose first unit is a harvester or a transport speaks like any other: the select line is the same.
* The F1 sound browser plays any entry, but the three mapped names are defaults, not checked by ear in this repo.
* Orders given by the sidebar (rally points, repair, sell) and structure commands have no voice.
* A voice skipped by the brake is lost, not delayed.
