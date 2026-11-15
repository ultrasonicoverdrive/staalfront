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
* **Not positional.** A unit voice is the player's own interface: `AudioSystem::play_as(.., Category::Voice)`, full
  volume, so the **voice volume** (`mix.voice_volume`, the Unit voices row of the sound settings) and the master volume
  apply; the effects volume no longer does (WP11.5b; `each_slider_reaches_its_own_category_and_the_voices_are_not_the_effects`).
  Without a `voice_volume` in an older `audio.ron` the voices follow `sfx_volume`, as before.

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

* **Without the player's Red Alert files the walkie-talkie lines play** (WP11.2b). `bank.ron` has an entry for each of the
  three events with three lines each (section 3a), category `Voice`, `max_instances: 1`, `original_as_is: true`.
* With `--ra-audio` (or `ra_audio` in `audio.ron`) the mapped Red Alert voice plays, exactly as before WP11.2b: the
  event is in the loaded mapping, so `resolve` gives the Red Alert sound with volume 1, no pitch spread and the
  default cap (`original_as_is`), and no variant is picked. A mapped name whose file is not found is not in the
  loaded mapping, so that event plays its own lines. `ra_mapping.ron` has had defaults for the three events since
  WP11.7 (voice lines carry a variant as extension, `V01` etc.); they were not changed. Which variant sounds right is
  chosen by ear in the F1 sound browser. No Red Alert name was added. Test:
  `the_original_voice_wins_where_found_and_the_cc0_lines_play_otherwise`.

### 3a. The walkie-talkie lines (WP11.2b)

Our own English lines (written for this project, not taken from any game), spoken by the offline speech
synthesizer **Piper** with the voice **`en_US-joe-medium`** (Tijmen chose joe over norman by ear, 15-11-2026), then
passed through our own walkie-talkie filter (`tools/sf_voice_gen`), so the units sound as if they speak over a
field radio (Tijmen, 15-11-2026: it must sound like a walkie-talkie; this replaced a first robot colour).

* **Sources.** The raw Piper renders are committed in `tools/sf_voice_gen/voice_src/` (with the voice's
  `MODEL_CARD.md`) and are the pinned input (length and FNV-1a hash per file). They were made once, offline, by
  `tools/sf_voice_gen/piper_lines.py` (piper-tts 1.8.0, default settings, the text with a full stop). CI never runs
  Piper; the game and the repository do not contain it. Re-render only to change a line or the voice.
* **Filter.** `cargo run -p sf_voice_gen` writes `assets/audio/voice/`, `-- --check` compares, `-- --table` prints
  this table:

| Event | File | Length | Words |
|---|---|---|---|
| `unit.select` | `unit_select_ready.wav` | 0.513 s | "Ready" |
| `unit.select` | `unit_select_online.wav` | 0.67 s | "Online" |
| `unit.select` | `unit_select_standing_by.wav` | 1.018 s | "Standing by" |
| `unit.move` | `unit_move_moving.wav` | 0.514 s | "Moving" |
| `unit.move` | `unit_move_route_set.wav` | 0.678 s | "Route set" |
| `unit.move` | `unit_move_on_it.wav` | 0.515 s | "On it" |
| `unit.attack` | `unit_attack_engaging.wav` | 0.688 s | "Engaging" |
| `unit.attack` | `unit_attack_target_locked.wav` | 0.853 s | "Target locked" |
| `unit.attack` | `unit_attack_firing.wav` | 0.555 s | "Firing" |

The chain, per line (`render` in `tools/sf_voice_gen/src/lib.rs`; every number is a named constant there):

1. **Trim** Piper's silence: from 15 ms before the first sample above 2 % of the peak to 40 ms after the last.
2. **Band**: two high passes at 300 Hz and two low passes at 3000 Hz (Butterworth biquads, fourth order on each
   side), and a presence bell of +6 dB at 1800 Hz. Measured with tones (`the_walkie_talkie_band_passes_300_to_3000_hz_with_a_presence_bump`):
   gain 1.09 at 700 Hz, 1.80 at 1800 Hz, 0.059 at 150 Hz, 0.012 at 100 Hz, 0.025 at 6 kHz, 0.002 at 8 kHz.
3. **Radio compressor** (gain `knee / (envelope + knee)`, knee 20 % of the peak, release 40 ms) and **soft
   saturation** (driven to 140 %, bent by `1.5u - 0.5u^3`), then the two low passes once more so the grit stays in
   the band.
4. **Hiss** in the same band at 2 % of the peak, a 45 ms **squelch** when the talk key opens (noise above 900 Hz,
   a click and a fast fall, 45 % of the peak) and a 70 ms squelch **tail** at the release (30 %).
5. Normalised to 26000 of 32767 (the fight sounds' `FILE_PEAK`), faded out over the last 5 ms.

Integer maths only: the filter coefficients come from cosine and exponential series summed in 128-bit fixed point
(no platform `sin` or `exp`), so the outputs are byte-equal on Linux and Windows. Each file is the trimmed source
plus 115 ms of squelch, at most 1.5 s (orchestrator, 15-11-2026); the longest is 1.018 s.

**Volume.** 0.35 for all three, the volume of the loudest fight entry (`explosion.large`). Measured by
`the_unit_voices_are_heard_over_a_fight_and_not_louder_than_its_loudest_sound` (sf_game `fx_sound.rs`, default
settings, each line through `play_as(.., Voice)`): every line's peak is at most the peak of the loudest fight sound alone
at the focus (both 0.278 of full scale), its loudness (RMS while it sounds, 0.055 to 0.090) is above that of the
loudest fight sound (0.042), and at least half (-6 dB) the RMS of the whole 20-tank fight mix (0.105). A line is not
louder than that whole fight on average: with the same peak as one explosion it cannot be. Whether it is clearly heard
in a real fight is for the ear.

**Heard.** The tests show the band, the levels and the pins, not that a person understands the words. Tijmen listened
on 15-11-2026 to the nine filtered lines as committed here (the files written at 19:52 that day, unchanged since) and
judged them good ("joe klinkt goed"). Not yet heard in a running game over a fight.

## 4. Where it is wired

* `crates/sf_audio/src/voice_brake.rs`: `VoiceConfig`, `VoiceBrake`.
* `crates/sf_game/src/unit_voice.rs`: `UnitVoice` (the three events), `selection_frame` (which selection step
  speaks), `order_voice` (which order speaks), `UnitVoices::speak` (the sound check, the brake, the call).
* `runner.rs`: `handle_selection` takes the orderable units before its step and calls `selection_frame` with
  whether a gesture or a group recall happened; `handle_orders` calls `order_voice` on the command a right-click
  produced; both go through `Runner::say`.

## 5. Tests

`sf_audio` `voice_brake::tests` (gaps, repeat gap, busy, a refused request does not move the gap, reset, config
checks), `config::tests` (the shipped numbers), `live::tests::playing_follows_a_voice_from_start_to_end`; since WP11.2b
`bank::tests::variants_parse_and_a_bad_variant_is_removed_with_a_warning`, `system::tests::pick_variant_takes_any_but_the_last_one`,
`the_voice_events_pick_their_variants_and_never_repeat_one`, `the_original_voice_wins_where_found_and_the_cc0_lines_play_otherwise`;
`tools/sf_voice_gen/tests/generator.rs` (pinned sources and outputs, committed files equal a fresh run, a source for every
line, length the trimmed source plus the squelch and at most 1.5 s, same peak, no clipping, the band measured with tones,
the words equal to the render script's, the integer cosine and exponential).
`sf_game` `unit_voice::tests` with the production `Selection`, `Pointer`, `commands::right_click` and `AudioSystem`:
click, enemy and neutral click, an enemy in the fog, a structure alone, shift add and remove, the 30 frames of a
box drag and the frames after it, double click, group recall and an empty group, move, queued move, attack,
attack-move, stop/scatter/hold, the spam thinned by the gaps, a busy voice, a silent event using no gap, the
shipped bank speaking a line of each event (`the_shipped_bank_speaks_its_lines_without_red_alert_files`), a bank
sound played through a real `AudioSystem` (recorded by its trace).

**Checked by reading only:** the two call sites in `runner.rs` need a window (`Context`, the pointer, the
actions). They are four short additions (`before`/`acted`/`additive_step` in `handle_selection`, and
`order_voice` in `handle_orders`); the decisions they call are the tested functions above. A hearing test needs
the player's own Red Alert files and a sound device, neither of which CI has.

## 6. Gaps

* One voice per action, not per unit type or per unit; the bank has no per-type voices. All units speak with the same
  voice (joe); per-faction or per-unit voices are a later step.
* Heard by Tijmen as files (15-11-2026, judged good), not yet in the game over a fight.
* A selection whose first unit is a harvester or a transport speaks like any other: the select line is the same.
* The F1 sound browser plays any entry, but the three mapped names are defaults, not checked by ear in this repo.
* Orders given by the sidebar (rally points, repair, sell) and structure commands have no voice.
* A voice skipped by the brake is lost, not delayed.
