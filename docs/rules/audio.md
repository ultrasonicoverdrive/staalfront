# Positional sound (WP11.3)

Presentation only: nothing here reaches the simulation, so no checksum depends on it. The numbers are in
`assets/config/audio.ron` (read by `sf_audio::config::AudioConfig`, compiled in as the fallback, range-checked when it
is parsed). A file that does not parse or fails a range check gives a warning on stderr and the shipped `spatial`,
`mix` and `positional` (those three fall back together), but keeps the file's `ra_audio` when that section reads on
its own, so a bad tuning number or an older file without the newer sections does not switch the original audio off
(`a_bad_tuning_value_or_a_missing_section_keeps_ra_audio`). The runner's start-up is `fx_sound::start_audio`.

The spatial numbers exist once, in the `AudioSystem`: `FxSound::play_frame` reads `AudioSystem::spatial_params()`
every frame and hands them to `Planner::plan`, so a later `set_spatial_params` (the settings screen, WP11.5) changes
the plan and the output together (`the_planner_follows_spatial_params_set_after_start`).

## 1. One model for volume and pan

`sf_audio::spatial::mix(listener, position, params)` is the only place that works out how loud and where a sound is.
Everything is metres on the ground plane; the height of a source is not heard.

* **Distance**: full volume within `ref_radius_m` of the point the camera looks at, silent beyond `max_radius_m`, a
  squared fade in between.
* **Zoom**: no attenuation while the camera is nearer than `near_distance_m`; farther out the gain is
  `near_distance_m / camera distance`, but never below `min_zoom_gain`.
* **Total gain = distance gain x zoom gain.** The distance gain does not depend on the camera distance and the zoom
  gain never rises with it, so for a fixed position zooming out is never louder (and the audible radius never widens).
* **Pan**: the offset along the camera's right vector over `pan_width_m + pan_growth x camera distance`, times
  `max_pan`. Pan does not change the gain.
* A sound below `audible_gain` is not worth a voice and is dropped.

The listener is rebuilt from the camera every frame in `Runner::frame` (focus, right vector, camera distance).

## 2. From event to sound

Each frame the runner calls `FxSound::play_frame` after `FxPlayer::frame` and after the listener is set:

1. `FxPlayer::shown()` is the events the picture showed in this frame (events not yet due and expired events are not
   in it, so a sound is in step with its flash and a minimised window does not set off a backlog).
2. `FxPlayer::sound_name(event)` is the sound event name of the effect (`weapon.fire.cannon`, `impact.miss`,
   `explosion.large`, ... see `effects.md` section 3); an event without one is not requested at all.
3. `AudioSystem::has_sound(name)`: a name with nothing in the bank and no Red Alert mapping (WP11.7) is **silent**:
   skipped and counted (`silent`), no message, no `unknown` count. Only the CC0 bank and the Red Alert mapping are used;
   since WP11.3b every fight event has a CC0 sound in the bank ("Fight sounds" below), so none of them is silent in the shipped game.
4. `Planner::plan` (`sf_audio::positional`) chooses the playbacks; the runner then calls `AudioSystem::play_at` for
   each, which applies the bank's volume, the category volume (`mix.sfx_volume`, or the player's Effects volume), the master
   volume (WP11.5b) and the same `mix`.

### The planner, per frame

| Step | Rule |
|---|---|
| quiet | gain below `audible_gain` at the listener: dropped (`inaudible`) |
| order | higher `priority` first, then louder (nearer, more zoomed in), then the earlier tick, then the order received; a total order, so the same events always give the same sounds |
| merge | an event with the same sound **and the same tick** within `merge_radius_m` of a playback already chosen is folded into it (`merged`); the playback stays where the louder one is, `Cue::merged` counts them. A volley of 40 rifles in one tick is one sound |
| voice limit | at most `max_per_batch` playbacks of one event per frame, and `max_cues_per_frame` over all (`limited`) |
| spacing | not within `spacing` ticks of the tick the event last played in an earlier frame (`spaced`) |

`spacing = min_spacing_ticks + (far_spacing_ticks - min_spacing_ticks) x (1 - gain)`, rounded: at full volume the
minimum, at the edge of hearing the far value. The gain includes the zoom, so **a farther sound is quieter and also
less frequent, and zooming out makes every sound less frequent**. 20 ticks are a second. The bank's `max_instances`
still caps the voices alive at once (`voices.rs`).

Rules are looked up by event name, then by dotted prefix (`weapon.fire` covers `weapon.fire.bullet`), then the
`fallback`. The shipped values are in `audio.ron`; `the_shipped_audio_ron_is_these_numbers` pins them all.

## Volumes (WP11.5b)

`sf_audio::Category` has four groups: `Sfx`, `Ui`, `Music` and `Voice`. A sound's gain is **its own bank volume x the volume of
its group x the master volume** (`AudioSystem::effective_volume`; `set_master_volume`, `set_category_volume`,
`the_master_volume_multiplies_every_category_and_a_voice_has_its_own`). A group of the bank entry is only the default: the unit
voices are played as `Voice` whatever their entry says (`AudioSystem::play_as`, used by `unit_voice` and the settings screen's test
sounds). The master volume reaches a running Red Alert track like the music volume does
(`the_master_volume_reaches_a_running_red_alert_track`). The interface sounds (`Ui`, also the sound browser) have no slider and
follow the master volume only.

`audio.ron` `mix` gained `master_volume` (default 1.0 when a file leaves it out) and `voice_volume` (written `Some(1.0)`; left
out, the voices follow `sfx_volume`, which is what they did before). Both are range-checked 0 to 2 like the others. These are the
values the game uses until the player moves a slider; the player's choice is in the settings file (`menus.md` "Sound"), resolved
by `settings::SoundSettings::resolve`, and `fx_sound::start_audio_with` applies it at the start (`start_audio` is the same
without settings).

## 3. Tests

* `sf_audio` `config::tests`: the shipped file against literals, the prefix lookup, every refusal of `validate`, a
  broken file, a bad tuning value or missing section that keeps `ra_audio`.
* `sf_audio` `positional::tests`: merge, voice limit, priority, spacing (near vs far, zoom), order independence,
  a tick counter that starts over.
* `sf_game` `fx_sound::tests` run events through `FxPlayer::frame` and `FxSound::play_frame` into a silent
  `AudioSystem` that records what it would play (`set_trace`): each of the 14 kinds of event gets its sound at the
  model's gain and pan, a name with no sound is silent without an `unknown` count, zooming out is never louder for a
  grid of positions, a volley merges, a heavy fight stays inside its budgets and gives the same sounds twice, an
  expired event and a frame shown twice are silent. `start_audio_follows_the_file_on_disk` runs the runner's
  start-up on a scratch `audio.ron` (other pan, volume and spacing) and checks the traced gain and pan and the number
  of cues follow it. The single `play_frame` call in `Runner::frame` is covered by the live run only.

## 3b. Music (WP11.4)

* Mood: shots, hits and deaths within `music.mood.radius_m` (80 m) of the camera focus count 1, 1 and 3. Calm turns to
  combat at a score of 6 within 100 ticks (5 s). Combat turns back to calm after at least 200 ticks and 300 ticks
  (15 s) without a counted event. Ticks are the sim's, 20 per second; the unit tests `sf_audio` `music::tests` and
  `sf_game` `fx_sound::tests` (`a_scripted_fight_switches_the_music_with_hysteresis`, with tick labels) pin it.
* Tracks: `ra_mapping.ron` `music` (empty = every track of `SCORES.MIX`), shuffled per mood, never the same twice in a
  row; the order is seeded per launch (`AudioSystem::new`), fixed in tests. Without Red Alert files or with the switch off: `music.fallback` of `audio.ron` (a bank sound; shipped
  `None` = silence; the running music fades out), no error, no extra warning.
* Loading: a track is decoded on a background thread when it is chosen. Measured in a release build on the dev
  laptop (3-minute stereo 22.05 kHz IMA fixture, 3.9 MiB compressed): the archive scan 11 ms for three tracks (none
  decoded), decode 37 to 38 ms per track on the thread, 30.3 MiB in memory per track, and the worst main-thread cost of a
  `set_music_mood` + `update` call 0.12 ms over 80 calls (`music_switch_cost`, an `--ignored` test that prints the numbers).

## 3c. Fight sounds (WP11.3b)

Every one of the 18 fight sound events (`effects.ron`: the 7 `weapon.fire.*`, 7 `impact.*`, `unit.destroyed.infantry` and
`explosion.small/medium/large`) has an entry in `assets/audio/bank.ron` with a CC0 file in `assets/audio/sfx/`, so the shipped game
is not silent in a fight. The files are **generated by this project** (`tools/sf_sound_gen`, own work, CC0, licence line in
`assets/LICENSES.md`): seeded noise, one-pole filters, exponential envelopes and falling or rising sines, **integer maths only** so the
bytes are the same on every platform. Nothing is sampled from or made to imitate any game (B11).

* **Regenerate**: `cargo run -p sf_sound_gen` writes `assets/audio/sfx/*.wav`; `-- --check` compares the files with a fresh run
  (exit code 1 when one is out of date); `-- --table` prints the table below. A new sound is a recipe and a `SOUNDS` entry in
  `tools/sf_sound_gen/src/lib.rs`, its bank entry, and its pinned hash in `tools/sf_sound_gen/tests/generator.rs`.
* **Format**: mono, 16-bit, 22.05 kHz WAV, 18 files, 571,926 bytes together (budget 2 MB). Every file is normalised to the same
  peak (26000 of 32767), so **the loudness of an event is its bank `volume`** (0.125 to 0.35), not the file (a frame of many is capped by the limiter, section 3c).
* **A mapped original plays as it did before this WP** (sweep of PR #122): the 18 bank entries carry `original_as_is: true`, so a Red Alert sound that replaces the event plays with volume 1.0, no pitch spread and the default voice cap, **not** with the bank's volume, pitch spread and `max_instances`, which are tuned for the CC0 file. Without that flag `resolve` would give a mapped original the bank entry's 0.125 to 0.35 and its cap. Other bank entries (`ui.click`) keep handing their volume and category to a mapped original as before. Test `the_original_wins_where_mapped_and_the_cc0_fight_sound_plays_otherwise` checks the resolved definition and the traced gain of all 18 (mapped: volume 1.0, pitch spread 0, cap 8; unmapped: the bank's).
* **Precedence**: unchanged, `AudioSystem::resolve`: with the original audio on and the event mapped in `ra_mapping.ron` the Red Alert
  sound plays (with the bank's volume and category, unless the entry is `original_as_is`, as all 18 fight entries are), otherwise the CC0 sound. The four events Red Alert has no sound for
  (`weapon.fire.demolition`, `impact.bullet`, `impact.precision`, `impact.miss`) play the CC0 sound also with `--ra-audio`.
* **Volume**: the mixed peak of a 20-tank fight at the default settings is 0.466 of full scale, below the music (`music_volume`
  0.6, peak at most 0.6); 0.466 is asserted by `the_limiter_leaves_the_20_tank_fights_as_they_were`, and `a_20_tank_fight_is_not_louder_than_the_music` asserts 0.15 to 0.5. The positional and zoom rules (section 1 and 2) and the
  Effects and master sliders apply to these sounds like to any other.
* **The limiter** (WP11.3b follow-up, `FxSound::start_cues`): the planned playbacks of one frame are summed (bank volume x category x master x spatial gain, `AudioSystem::gain_at`); above `positional.max_summed_gain` (`audio.ron`, 1.2) all of them are turned down by the same factor (`max_summed_gain / sum`, `play_at_scaled`), a quieter frame plays unchanged, and a mapped original sound is neither counted nor turned down (`a_mapped_original_is_not_counted_and_not_turned_down`, with a fixture install made from our own encoder). Measured peaks (`a_heavy_moment_does_not_clip`, which also switches the limiter off to show the old numbers): eight kinds on one tick at one spot 1.13 before and 0.582 now, all 18 kinds 1.37 before and 0.384 now (the test asserts only that both stay at or below 1.0 and the before numbers; 0.582 and 0.384 are printed, measured 14-11-2026); four kinds 0.485 (asserted), the staggered 20-tank fight 0.466 (`the_limiter_leaves_the_20_tank_fights_as_they_were`) and the simultaneous volley 0.238 (asserted, `a_simultaneous_volley_of_20_tanks_is_not_louder_than_the_music`) are below the limit and unchanged. The limits of the rule are in section 4.
* **Hidden events are audible** (current rule, `fog_look.md`): the audio plays `FxPlayer::shown()`, so a shot or an explosion in the shroud is heard at its distance volume, and with these sounds that is now true in the shipped game. Tijmen may change this ("no sound from what I cannot see"); it is a design choice, not a defect.

| Event | File | Length | Character |
|---|---|---|---|
| `weapon.fire.bullet` | `weapon_fire_bullet.wav` | 0.25 s | dry, sharp crack with a short low thump |
| `weapon.fire.precision` | `weapon_fire_precision.wav` | 0.5 s | harder crack, a body and two soft echoes |
| `weapon.fire.cannon` | `weapon_fire_cannon.wav` | 0.6 s | deep boom (150 to 40 Hz) with a rumble and a small crack |
| `weapon.fire.rocket` | `weapon_fire_rocket.wav` | 0.9 s | pop, then a swelling whoosh that dies away |
| `weapon.fire.shell` | `weapon_fire_shell.wav` | 0.5 s | mid-pitched thump (220 to 75 Hz), shorter than the cannon |
| `weapon.fire.howitzer` | `weapon_fire_howitzer.wav` | 1 s | very deep, long boom (100 to 28 Hz) and a low echo |
| `weapon.fire.demolition` | `weapon_fire_demolition.wav` | 0.45 s | two ticks and a rising beep: a charge being set |
| `impact.bullet` | `impact_bullet.wav` | 0.15 s | very short tick with a high ping |
| `impact.precision` | `impact_precision.wav` | 0.25 s | ping (1.1 kHz falling) over a dull thud |
| `impact.cannon` | `impact_cannon.wav` | 0.7 s | small blast: low body, noise and a crack |
| `impact.rocket` | `impact_rocket.wav` | 0.9 s | bigger blast with a rumbling tail |
| `impact.shell` | `impact_shell.wav` | 0.6 s | medium blast, shorter and higher than the rocket's |
| `impact.demolition` | `impact_demolition.wav` | 1.2 s | heavy blast (80 to 25 Hz) with a long rumble |
| `impact.miss` | `impact_miss.wav` | 0.3 s | thin falling zing (2.6 kHz to 900 Hz): a near miss |
| `unit.destroyed.infantry` | `unit_destroyed_infantry.wav` | 0.5 s | dull thud and a rattle with a faint falling tone |
| `explosion.small` | `explosion_small.wav` | 0.8 s | small explosion, no tail |
| `explosion.medium` | `explosion_medium.wav` | 1.2 s | medium explosion with a rumbling tail |
| `explosion.large` | `explosion_large.wav` | 2 s | large explosion (80 to 26 Hz), long rumble |

Tests: `sf_sound_gen` `tests/generator.rs` (determinism and a pinned length and hash per file, the committed files equal a fresh run,
each file loud, unclipped, the right length, under budget, each event's bank entry points at its file); `sf_game` `fx_sound::tests`
`every_fight_event_has_a_non_silent_cc0_sound_in_the_shipped_bank` (literal event to file list, volume, the game's own `has_sound`
and trace), `a_20_tank_fight_is_not_louder_than_the_music`; `sf_audio` `the_original_wins_where_mapped_and_the_cc0_fight_sound_plays_otherwise`.

## 4. Gaps

* Clipping (limiter, rule in section 3c): it works per frame, so frames that overlap in time are not limited together (the test frames are 10 ticks apart; the sim runs at 20 ticks a second and a file lasts up to 2 s, 40 ticks, so up to four frames overlap; the measured peaks do include that overlap because the mix is summed over time, but nothing proves a real fight stays below 1.0); the summed gain counts a cue that the voice cap (`max_instances`) later drops, so a frame can be turned down more than needed; `play`/`play_as` (UI, voices, announcements, the settings test sounds) and the music are not limited; the whole frame is turned down, not only the excess, so all 18 kinds come out quieter (0.384) than eight (0.582); a mapped original sound is not counted, so it can still add to a loud frame; nobody listened to it.
* Fight sounds (WP11.3b): the bank has one file per event, no variants (the bank format has one file per name), so the only
  variety is `pitch_variation`. Nobody could listen in the build container: the volumes come from a measured mix, the character
  of each sound from the table in 3c above; Tijmen to listen, `weapon.fire.demolition` (two ticks and a rising beep) first, for any likeness to a Red Alert sound (B11). The sim has no event for a structure being hit, so there is no such sound.
* The sim emits no event for a structure placed or finished, or a harvester unloading: those are later WPs; nothing
  was added to `sf_sim`. The replies to selecting and ordering a unit are WP11.2 (`voices.md`), raised on the UI side.
* The volumes are set in the file (`mix`) and, since WP11.5b, on the settings screen's Sound page (see "Volumes").
* `audio.ron` is read once at start; no hot reload.
* A moving unit's sound stops at the place it had when the event happened (no Doppler, no following).
* Music is decoded whole on a background thread, not streamed from disk: 30 MiB per running track (normally two
  in memory, running + fading; briefly three plus the 16-bit copy when the mood flips back during a fade), and the track starts a
  few tens of milliseconds after the mood changes (the old one plays on until then). A streaming decoder would cut the memory.
* With the shipped empty `music` section calm and combat draw from the same pool, so a mood change plays another track of
  it. Tune by ear in the F1 sound browser and fill the lists in `ra_mapping.ron`.
* The wiring in `Runner::frame` (one `music_frame` call) is covered by the live run only; the pieces it calls are unit-tested.
* The end of a fight and a track change share one fade time (`music.crossfade_ms`).
