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
   skipped and counted (`silent`), no message, no `unknown` count. Only the CC0 bank and the existing mapping are used;
   no sound was added by WP11.3, so until the bank has the fight sounds only mapped events are heard.
4. `Planner::plan` (`sf_audio::positional`) chooses the playbacks; the runner then calls `AudioSystem::play_at` for
   each, which applies the bank's volume, the category volume (`mix.sfx_volume`) and the same `mix`.

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

## 4. Gaps

* The bank has no weapon, impact or explosion sounds yet, so without the player's Red Alert files the fight is silent
  (follow-up 31 in `docs/progress.md`: WP11.3b adds CC0 fight sounds). With them, `ra_mapping.ron` maps every fight
  event by role (PR #87) except `weapon.fire.demolition`, `impact.bullet`, `impact.precision` and `impact.miss`:
  Red Alert has no sound for those, so they stay silent until WP11.3b.
* The sim emits no event for a unit being selected or ordered, a structure placed or finished, or a harvester
  unloading: those are WP11.2 (voices) and later; nothing was added to `sf_sim`.
* The settings screen (WP11.5) is not part of this; `mix.sfx_volume` and `mix.music_volume` are set in the file.
* `audio.ron` is read once at start; no hot reload.
* A moving unit's sound stops at the place it had when the event happened (no Doppler, no following).
* Music is decoded whole on a background thread, not streamed from disk: 30 MiB per running track (normally two
  in memory, running + fading; briefly three plus the 16-bit copy when the mood flips back during a fade), and the track starts a
  few tens of milliseconds after the mood changes (the old one plays on until then). A streaming decoder would cut the memory.
* With the shipped empty `music` section calm and combat draw from the same pool, so a mood change plays another track of
  it. Tune by ear in the F1 sound browser and fill the lists in `ra_mapping.ron`.
* The wiring in `Runner::frame` (one `music_frame` call) is covered by the live run only; the pieces it calls are unit-tested.
* The end of a fight and a track change share one fade time (`music.crossfade_ms`).
