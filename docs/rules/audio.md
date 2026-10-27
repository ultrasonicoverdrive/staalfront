# Positional sound (WP11.3)

Presentation only: nothing here reaches the simulation, so no checksum depends on it. The numbers are in
`assets/config/audio.ron` (read by `sf_audio::config::AudioConfig`, compiled in as the fallback, range-checked when it
is parsed; a broken file gives a warning on stderr and the shipped numbers).

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
  broken file.
* `sf_audio` `positional::tests`: merge, voice limit, priority, spacing (near vs far, zoom), order independence,
  a tick counter that starts over.
* `sf_game` `fx_sound::tests` run events through `FxPlayer::frame` and `FxSound::play_frame` into a silent
  `AudioSystem` that records what it would play (`set_trace`): each of the 14 kinds of event gets its sound at the
  model's gain and pan, a name with no sound is silent without an `unknown` count, zooming out is never louder for a
  grid of positions, a volley merges, a heavy fight stays inside its budgets and gives the same sounds twice, an
  expired event and a frame shown twice are silent.

## 4. Gaps

* The bank has no weapon, impact or explosion sounds yet; only `explosion.small` has a Red Alert mapping.
* The sim emits no event for a unit being selected or ordered, a structure placed or finished, or a harvester
  unloading: those are WP11.2 (voices) and later; nothing was added to `sf_sim`.
* Music (WP11.4) and the settings screen (WP11.5) are not part of this; `mix.sfx_volume` is the only volume setting.
* `audio.ron` is read once at start; no hot reload.
* A moving unit's sound stops at the place it had when the event happened (no Doppler, no following).
