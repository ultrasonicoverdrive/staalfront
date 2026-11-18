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
  `music.calm` and `music.combat`, our own music, section 3d; `None` would be silence and the running music fades out), no error, no extra warning.
* Loading: a track is decoded on a background thread when it is chosen. Measured in a release build on the dev
  laptop (3-minute stereo 22.05 kHz IMA fixture, 3.9 MiB compressed): the archive scan 11 ms for three tracks (none
  decoded), decode 37 to 38 ms per track on the thread, 30.3 MiB in memory per track, and the worst main-thread cost of a
  `set_music_mood` + `update` call 0.12 ms over 80 calls (`music_switch_cost`, an `--ignored` test that prints the numbers).

## 3d. Own music (WP11.4b)

Without the player's Red Alert files the game plays two tracks of its own: `music.calm` and `music.combat` (`bank.ron`,
category `Music`, `looped: true`, one instance), named in `audio.ron` `music.fallback`. With `--ra-audio` and Red Alert tracks
found for the mood, the Red Alert track still wins (section 3b, unchanged: the fallback is only reached when the mood has no
playable Red Alert track). Our music is **generated by this project** (`tools/sf_music_gen`, own composition, CC0, licence line
in `assets/LICENSES.md`); nothing is quoted or made to sound like any game's music (B11). Since WP11.4c the combat track also plays real recordings (section 3d-2), which are CC0 too.

| Track | BPM | Key | Length | Size | Character |
|---|---|---|---|---|---|
| `music.calm` (`calm.flac`) | 84 | D minor | 68.6 s (24 bars) | 1,509,638 bytes | sparse and tense: slow detuned pads, a deep sub bass, a soft heartbeat pulse, a sparse plucked pattern in an echo, far metal hits and a slow lead near the end |
| `music.combat` (`combat.flac`) | 128 | D minor | 67.5 s (36 bars) | 2,052,329 bytes (2,021,170 before WP11.4c) | driving: four-on-the-floor kick, snare on two and four, 16th hats, a 16th bass ostinato, short chord stabs, a breakdown and a snare-roll build, a square lead in the last third, and since WP11.4c an orchestral layer of real CC0 samples (timpani, trombone, horn, tuba, cello, crash, bass drum, gong) |

(`cargo run -p sf_music_gen -- --table` prints the table from the code.) Both are in D minor so a crossfade between them is
not a clash. Together 3,561,967 bytes against a budget of 4,000,000 (`BUDGET_BYTES` is unchanged: the layer added 31,159 bytes).

* **Format**: mono, 16 bit, 22.05 kHz **FLAC**. A 44.1 kHz WAV would be 6 MB a track; mono 22.05 kHz WAV 3 MB a track. The FLAC encoder is
  ours (`tools/sf_music_gen/src/flac.rs`, fixed predictors and Rice codes, no dependency; the MD5 field is left zero, which the format allows)
  and only the decoder is new in the game: `sf_audio` turned on kira's `flac` feature (symphonia's pure-Rust FLAC decoder,
  `symphonia-bundle-flac`; no C code). The test `the_game_decoder_reads_the_fresh_bytes_and_the_committed_files_back_exactly` decodes the committed files
  with kira and compares every sample with a fresh render.
* **Generator**: integer maths only (a phase counter, a parabola for the sine, `sf_sound_gen`'s noise, one-pole filters and envelopes),
  so the bytes are the same on every platform; `cargo run -p sf_music_gen` writes `assets/audio/music/*.flac`, `-- --check` compares,
  `-- --table` lists. The sequencer is a few functions: `Song::put` puts a rendered note or drum hit at a 16th-note step,
  `calm()` and `combat()` hold the notes.
* **Seamless loop**: the track is mixed into a ring. A note, a pad release or an echo that runs past the end continues at the start
  (`Song::put` wraps, `comb` runs the echo and reverb round the ring), and every drum and note has a 1 to 2 ms attack, so the first
  sample does not jump. Test `the_loop_joins_without_a_jump`: the step from the last to the first sample is no bigger than the 90th
  percentile of the steps inside the track (measured: calm 372 against 1,451, combat 262 against 3,714); with the wrap removed from
  `Song::put` the calm join is 10,684 and the test fails (mutation, 15-11-2026). The ends of both tracks have music in them
  (`the_ends_have_music_in_them_...`), so the test does not pass on silence.
* **Loudness**: both files have the same peak (29,000 of 32,767, `FILE_PEAK`, no flat top: `nothing_clips_and_the_peak_is_the_file_peak`)
  and about the same RMS (calm 8,612, combat 9,120). The mood is set by the bank volume: combat 1.0, calm 0.7. At the default settings
  (`music_volume` 0.6) combat peaks at 0.53 of full scale, calm at 0.37; the 20-tank fight peaked at 0.466 until WP11.3c; since then the rule against the music is about RMS, section 3c "Volume", and the fight peaks at 0.950 after the master limiter)
* **Loading**: a bank sound is decoded when it is first needed and then kept (`Live::data`), never up front with the effects: the music
  is not in the list of sounds started at launch. This path decodes **on the calling thread** (unlike a Red Alert track, which decodes on a
  background thread): `the_shipped_fallback_music_plays_per_mood_without_red_alert_files` prints the first start, 298 ms for calm and 245 ms for combat in a
  debug build and 21 ms for calm and 19 ms for combat in a release build (15-11-2026, on a laptop with other builds running, one run each), about one frame at 60 fps, once per track per run. Decoded, a track is 2 x 4 bytes x 1.5 million frames = 12 MB; both stay cached.

### 3d-2. Orchestral layer of the combat track (WP11.4c)

Tijmen asked on 16-11-2026 for better music; he listened to a float preview of this arrangement and said "Ja, bouw in".
The combat track (not the calm one: `calm.flac` is byte-identical, pinned) gets a layer of real recordings from **VSCO 2:
Community Edition** (CC0 1.0, Versilian Studios / Sam Gossner; licence row in `assets/LICENSES.md`), played by our own integer
sequencer. Nothing is quoted from any piece; the arrangement is ours and follows the bars and roots of the track (128 BPM, 36 bars,
phrase A D D Bb C, phrase B D G Bb A, nine sections of four bars).

| Part | Where | What |
|---|---|---|
| Timpani | beat 1 of sections 1, 2, 3, 5, 6, 7 and bars 32 and 33 of the outro (velocity 3); beats 1 and 3 in the full sections 3, 6, 7 (velocity 4 on one); soft hits (velocity 3, lower gain) on the even bars of the breakdown | on the bar root, an octave up when the root is below MIDI 33 (round robin alternates per bar) |
| Low brass | sections 3, 6, 7 and bars 32 and 33 | held trombone on root + 12 (95% of the bar), horn on root + 19, tuba staccato on the root (400 ms) |
| Stabs | every bar of sections 2 and 7 | trombone staccato on the 16ths 0, 3, 6, 10 (250 ms) under the synth stabs |
| Cello | sections 5, 6, 7 | spiccato eighths on root + 12, accent on the quarters |
| Accents | bars 12 and 24 / 16 / 23 | crash and orchestral bass drum / gong into the breakdown / timpani roll over the last bar of the build, tuned up half a semitone to the D |

* **Samples**: 32 files, 2,982,568 bytes together, in `tools/sf_music_gen/samples/` (16-bit mono 22.05 kHz WAV). Each is pinned by length and
  FNV-1a (`every_committed_sample_is_pinned_by_length_and_hash`). Timpani: 3 drums x velocity 3 and 4 x 2 round robins (12) and velocity 1 of drums 2 and 3 (2), 110,294 bytes each,
  the D-roll (88,244), bass drum (88,244), crash (176,444), gong (352,844); trombone sustain 3 (92,654 each) and staccato 2 (22,094 each), horn 3 (92,654), tuba 3 (26,504), cello 3 (17,684).
  The velocity-1 hits are used for the soft hits of the breakdown, as in the preview, at gain 0.35 (90/256). (An earlier draft of this WP replaced them with velocity 3 because it
  believed they peak at 67 of 32768; that figure belongs to drum 5, which the breakdown never uses. Drums 2 and 3 peak at 459 to 801 per channel, comparable to the trombone source that is used.)
* **Import** (`cargo run -p sf_music_gen -- --import <VSCO 2 CE dir>`; not run in CI, the directory is only read): mono sum, 2:1 decimation
  through a 95-tap integer FIR (-6 dB at 10 kHz, -45 dB at 11.025 kHz, tested), cut to `keep_ms`, linear fade, peak-normalised to 32767, rounded once to 16 bit.
  `the_committed_samples_are_what_the_import_makes_of_the_recordings` redoes it when `SF_VSCO_DIR` points at a checkout (run once on 16-11-2026,
  commit 440300901dfe, all 32 equal); without the variable it only prints that it was skipped.
* **Player** (`sampler.rs`): integers only. The pitch ratio is `2^(cents/1200)` in Q16 from a Taylor series (pinned literals, within 1 of the float value for -2400..2400 cents);
  the value between two samples is a four-point cubic (Catmull-Rom) in integers; a note cut short ends in an 80 ms linear fade. At unison the output is the input.
  `the_render_path_has_no_float` searches the sources for float types and functions.
* **Pitches**: the VSCO file names count one octave low (C4 is not 60): MIDI = 12 x (octave + 2) + pitch class (measured on every used pitched sample,
  all within 25 cents of the name; `the_committed_samples_have_their_pitch`). The timpani files have no note; measured by autocorrelation over 12 hits:
  drum 2 = 34.1 to 34.5 (constant 34.4), drum 3 = 37.3 to 37.7 (37.5), drum 5 = 42.5 to 43.1 (42.7), the roll 37.5 (37.5); the constants are in `SAMPLES` as cents of a MIDI note
  and the test measures each committed file again and accepts +-50 cents, a strong period and a weaker one at half the period (no octave error).
  The brass, horn, tuba and cello constants are the note names. Tuning: nearest sample, shifted by the difference (largest shift 3 semitones).
* **Mix**: the layer is a ring of its own at the scale of the track, added to the finished track with the gains of the preview (`put_ring`, in 1/256), then an
  integer look-ahead peak limiter (5 ms blocks, falls at once, rises over about 150 ms, ring-safe) and the same peak normalisation as before. No pan (the track is mono).
* **Loudness** (`the_layer_does_not_make_the_combat_track_quieter_and_the_peak_stays_the_file_peak`): peak 29,000 before and after (so the 0.531 of
  `a_20_tank_fight_is_at_most_1_8_times_the_music` is unchanged); RMS 9,120 before, 9,158 after (calm 8,611, so the two tracks stay within 1.5). Per section against the old track
  the RMS is -0.1 to +0.3 dB (the float preview: -0.0 to +0.8 dB, the preview limited less). **The limiter does lower the track at the loud moments**
  (`the_limiter_numbers_quoted_in_the_documents_are_the_measured_ones`, gain per 5 ms block): in the full sections 3, 6 and 7 it is more than 1 dB down for 11.4%, 17.8% and 15.8% of the blocks
  (15.0% together) and goes down to -3.99 dB, at bar 24 (the crash, bass drum and timpani together), before the gain returns over about 150 ms. The limiter leaves the intro alone; the final normalisation lowers the whole track by about 0.07 dB (sections 0, 1 and 4 come out about -0.07 dB). Whether this pumps
  audibly is for the ear to judge; it is not claimed to be inaudible.
* **Differences from the float preview** (`mix.py`), all deliberate or forced: (1) the track is mono, so the preview's pan is dropped; (2) the preview resampled 44.1 kHz recordings with float
  interpolation; here they are decimated once at import and shifted by the integer player; (3) the samples are normalised to full scale after the cut (the preview normalised the whole file), so
  a sample whose peak lies outside the part kept is a little louder than there; (4) the nearest-sample choice is per the committed set, which has fewer notes than the preview's bank: the largest shift is 3 semitones
  (trombone staccato, 50 from F2 53); in bar 29 (root G, cello on 55) there is no A2 sample, so F2 shifted up 2 semitones plays; (5) the horn, trombone and tuba pick from 3, 3 and 3 samples, the cello from 3;
  (6) the limiter is the integer one described above (5 ms blocks, look-ahead of one block, gain interpolated per sample, ring-safe), the preview's was a float block limiter of the same shape with a ceiling of 0.89. (7) **the effective level is hotter than the preview's**: the import normalises the mono sum of the kept part to full scale
  (`sampler.rs`), while `mix.py` normalised each channel to its loudest channel over the whole file and then folded the stereo mix to (L+R)/2. Measured per sample on the 32 sources
  (review of PR #154): the layer is +0.3 to +3.4 dB louder per sample, about +3 dB for the held trombones, the horns, timpani 3 and 5 at v4, the roll and the cello, before the
  preview's pan losses (up to about another 1 dB for trombone, horn and cello). This is the main cause; (3) adds a little. Not compensated in the gains: the render is judged by ear as it is;
  (8) the timpani tuning constants differ from the preview's by up to about 30 cents; `tim5_v4_rr1` measures 43 cents from its constant (7 cents inside the test's 50-cent limit) and
  `tim2_v3_rr1` passes the double-period check by 0.01.
  The gain numbers are the preview's (the effective level is not, see (7)), rounded to 1/256 (timpani 0.55 and 0.4, breakdown 0.35, trombone 0.28, horn 0.22, tuba 0.3, stabs 0.25, cello 0.22 and 0.16, crash 0.3, bass drum 0.5, gong 0.35, roll 0.45).
  The trombone staccato uses the `v3` recordings, as the preview did.
* **Re-pin**: `combat.flac` moved from 2,021,170 bytes (FNV 0x95f629a23692dcd9) to 2,052,329 (0x19ee6457d321ba99); with the layer off the old bytes come back exactly
  (`without_the_layer_the_old_combat_track_comes_back_byte_for_byte`). `calm.flac` did not move.
* **Gaps**: not heard by us in the game (only the float preview was heard; this integer render has not been listened to yet); mono (the preview was panned);
  the loop repeats every 67.5 s as before. The octave guard of the pitch test checks both the half and the double period; a claim an octave too low is caught by the first (tested by mutation), a claim an octave
  too high was caught by the distance and strength checks (the search window is six semitones either way), not by the double-period check, which no mutation has exercised.

## 3c. Fight sounds (WP11.3b)

Every one of the 18 fight sound events (`effects.ron`: the 7 `weapon.fire.*`, 7 `impact.*`, `unit.destroyed.infantry` and
`explosion.small/medium/large`) has an entry in `assets/audio/bank.ron` with a CC0 file in `assets/audio/sfx/`, so the shipped game
is not silent in a fight. The files are **generated by this project** (`tools/sf_sound_gen`, own work, CC0, licence line in
`assets/LICENSES.md`): seeded noise, one-pole filters, exponential envelopes and falling or rising sines, **integer maths only** so the
bytes are the same on every platform. Nothing is sampled from or made to imitate any game (B11).

* **Regenerate**: `cargo run -p sf_sound_gen` writes `assets/audio/sfx/*.wav`; `-- --check` compares the files with a fresh run
  (exit code 1 when one is out of date); `-- --table` prints the table below. A new sound is a recipe and a `SOUNDS` entry in
  `tools/sf_sound_gen/src/lib.rs`, its bank entry, and its pinned hash in `tools/sf_sound_gen/tests/generator.rs`.
* **Format**: mono, 16-bit, 22.05 kHz WAV, 30 files (18 events; six of them have three files each, "Variants" below), 1,015,612 bytes
  together (571,926 before WP11.3c; budget 2 MB, `every_file_is_a_loud_unclipped_mono_wav_of_its_length_and_all_stay_in_budget`; the largest file is
  `explosion_large.wav`, 105,884 bytes, the smallest `impact_bullet*.wav`, 11,068). Every file is normalised to the same
  peak (26000 of 32767), so **the loudness of an event is its bank `volume`** (0.5 to 1.0 since WP11.3c, was 0.125 to 0.35), not the file (a frame of many is capped by the limiter, below).
* **A mapped original plays as it did before this WP** (sweep of PR #122): the 18 bank entries carry `original_as_is: true`, so a Red Alert sound that replaces the event plays as-is, apart from the master limiter: volume 1.0, no pitch spread and the default voice cap, **not** with the bank's volume, pitch spread and `max_instances`, which are tuned for the CC0 file. Without that flag `resolve` would give a mapped original the bank entry's 0.5 to 1.0 (0.125 to 0.35 before WP11.3c) and its cap, and its variants. Other bank entries (`ui.click`) keep handing their volume and category to a mapped original as before. Test `the_original_wins_where_mapped_and_the_cc0_fight_sound_plays_otherwise` checks the resolved definition and the traced gain of all 18 (mapped: volume 1.0, pitch spread 0, cap 8; unmapped: the bank's).
* **Precedence**: unchanged, `AudioSystem::resolve`: with the original audio on and the event mapped in `ra_mapping.ron` the Red Alert
  sound plays (with the bank's volume and category, unless the entry is `original_as_is`, as all 18 fight entries are), otherwise the CC0 sound. The four events Red Alert has no sound for
  (`weapon.fire.demolition`, `impact.bullet`, `impact.precision`, `impact.miss`) play the CC0 sound also with `--ra-audio`.
* **Power (WP11.3c).** Tijmen compared these sounds with the Red Alert originals (switch "Use original audio" in the sandbox fight) on
  15-11-2026: "geluid alles duidelijk zwakker. meer power er in". Two things made them weak: the bank volumes (0.125 to 0.35, where an original plays at 1.0)
  and thin files (a falling sine and filtered noise with no body). Now (a) every recipe has a **sub thump** under its body, a harder first 12 ms (a click
  of full-band noise with no attack), a **rumble** under every boom and, on the loud ones, one to three **slap echoes** (`Mix::slap`, a low-passed copy of the sound 75 to 520 ms later),
  longer files (cannon 0.6 -> 0.8 s, howitzer 1.0 -> 1.3 s, large explosion 2.0 -> 2.4 s); (b) **soft saturation** of the sum (`Mix::drive`: scale to 1.4 to 3.2 times full scale, then a
  cubic soft clip, then back to the file peak), which raises the RMS at the same peak: every file's RMS rose from 2,644-5,737 to 6,569-9,327
  (`every_file_has_power_rms_and_low_end`: RMS >= 6,000 and RMS below 250 Hz >= 4,000 for all 30); (c) the volumes went toward the original's 1.0. Still integer maths, the files deterministic and committed.
  The numbers, measured 18-11-2026 (the RMS is of the whole file, 16-bit units; the last column is the change of RMS x volume, the loudness of a single sound):

| Event | old volume | new volume | file RMS old | file RMS new | RMS x volume, change |
|---|---|---|---|---|---|
| `weapon.fire.bullet` | 0.225 | 0.7 | 2644 | 6838 | +18.1 dB |
| `weapon.fire.precision` | 0.3 | 0.85 | 2891 | 9326 | +19.2 dB |
| `weapon.fire.cannon` | 0.3 | 0.95 | 4394 | 8287 | +15.5 dB |
| `weapon.fire.rocket` | 0.275 | 0.85 | 4377 | 9082 | +16.1 dB |
| `weapon.fire.shell` | 0.275 | 0.85 | 3025 | 7173 | +17.3 dB |
| `weapon.fire.howitzer` | 0.325 | 1.0 | 4795 | 8755 | +15.0 dB |
| `weapon.fire.demolition` | 0.25 | 0.8 | 4244 | 6962 | +14.4 dB |
| `impact.bullet` | 0.15 | 0.55 | 2928 | 7412 | +19.4 dB |
| `impact.precision` | 0.175 | 0.7 | 3163 | 8024 | +20.1 dB |
| `impact.cannon` | 0.25 | 0.85 | 3916 | 7117 | +15.8 dB |
| `impact.rocket` | 0.275 | 0.9 | 3816 | 8247 | +17.0 dB |
| `impact.shell` | 0.25 | 0.85 | 3557 | 8069 | +17.7 dB |
| `impact.demolition` | 0.3 | 1.0 | 4704 | 8522 | +15.6 dB |
| `impact.miss` | 0.125 | 0.5 | 5737 | 7791 | +14.7 dB |
| `unit.destroyed.infantry` | 0.2 | 0.7 | 3433 | 8222 | +18.5 dB |
| `explosion.small` | 0.25 | 0.8 | 3861 | 6698 | +14.9 dB |
| `explosion.medium` | 0.3 | 0.95 | 4328 | 7246 | +14.5 dB |
| `explosion.large` | 0.35 | 1.0 | 3721 | 7288 | +15.0 dB |

  A single sound is now 15 to 20 dB louder in RMS and its peak at the focus is 0.40 to 0.79 of full scale (was 0.10 to 0.28), close to a full-scale original; the loudest
  single sound, `weapon.fire.howitzer`, peaks at 0.793. `every_fight_event_alone_is_audible_and_modest` asserts 0.35 to 0.80 for all 18.
* **Volume against the music (WP11.3c, restates the rule of WP11.3b/11.4b).** The old rule was fight peak <= 0.5 against a combat music peak of 0.531 (about 1 dB of headroom,
  no room to get louder). The music is a steady wall (RMS 0.168 at the default settings: the combat track's RMS 9,158 of 32,768, pinned in `sf_music_gen`
  `RMS_AFTER`, times bank volume 1.0 times `music_volume` 0.6), the fight is bursts, so peak against peak says little. The rule is now, measured on the **steady state**
  of the 20-tank fight (`tank_fight`, from 2 s on to 10 s; the onset, in which the first shots meet an empty load, is left out because it hid the quiet of the steady state in the first version of this WP),
  after the master limiter: **the steady RMS is at least twice what it was before WP11.3c (0.108, measured on commit 37002cf3 with this same harness) and at most 1.8 times the music's**,
  and nothing peaks above the master limiter's ceiling 0.95 (`a_20_tank_fight_is_at_most_1_8_times_the_music`). The name stayed; the fight may now be louder than the music, by 5 dB at most,
  which changes Tijmen's decision 3 of WP11.3b ("not louder than the music"). **Tijmen confirmed it on 18-11-2026** after listening to the release build of PR #157 ("ja en mag 5db hoger zijn").
  To move it later: the number is 1.8 in that test and `max_load_gain` in `audio.ron`.
  Measured 18-11-2026 with the shipped files:

  | 20-tank fight | peak | RMS, steady state (2 s to 10 s) | against 0.108 before | against the music's 0.168 |
  |---|---|---|---|---|
  | before WP11.3c (volumes 0.125-0.35, per-frame limiter 1.2 only; whole fight RMS 0.105) | 0.466 | 0.108 | 1.00 | 0.64 |
  | after, cue limiter only (before the master limiter) | 1.451 | 0.278 | 2.57 | 1.65 |
  | after, with the master limiter | 0.950 | 0.272 | 2.51 | 1.62 |
  | simultaneous 20-tank volley (artificial worst case, bound 2.2 x the music's), with the master limiter | 0.945 | 0.236 | | 1.40 |

  A first version of this WP (a load ceiling of 1.0 with a half-life of 6 ticks and no floor) made the dense fight **quieter** than before: a saturated load frees only 0.109 of gain a tick, so from tick 3 on
  cannon fire played at 0.10 of its volume (was 0.30), a cannon hit at 0.054 (was 0.25), the large explosion at 0.04 (was 0.35), a lone shot ducked its own hit 4 ticks later from 0.85 to about 0.40, and the
  10 s RMS of 0.130 hid all of it because it contained the loud first frame. The reworked limiter is below; `the_cue_limiter_keeps_every_cue_audible_in_a_dense_fight` pins the per-cue shares.
* **Clipping against the music (WP11.3c).** One howitzer peaks at 0.793, the combat music at 0.53 and a unit voice at 0.75; kira does not limit and the sound device cuts a sample above full scale flat. Measured
  (`fight_music_and_voice_together_clip_without_the_master_limiter_and_not_with_it`, at sample accuracy): the 20-tank fight, the loudest 13 s window of `combat.flac` at its bank volume times the music volume (0.6) and two voice
  lines at 5 s and 8 s sum to a peak of 1.680 and **1,560 of 286,550 samples above full scale**; through the master limiter the peak is 0.950 and no sample is above full scale. So the earlier text of this section, that a fight peak on a music peak was "a coincidence of two peaks, not a measured clip",
  was wrong: it clips with ordinary loud fights. The fix is a **master limiter on kira's main track** (`sf_audio::limiter`, a kira `Effect` of our own because kira's compressor has a smoothed envelope and lets the first
  milliseconds through): it follows the peak of each frame at once (no attack time) and lets go over `release_ms`, so no output sample is above the `ceiling`; numbers in `audio.ron` `limiter` (ceiling 0.95, release 150 ms; range-checked, pinned,
  `set` on the running effect by `fx_sound::start_audio_with`, test `start_audio_follows_the_file_on_disk`, a missing section gives the same numbers). It limits everything the output plays (fight, music, voices, interface sounds) and
  mapped Red Alert sounds too. The effect is tested on its own (`limiter::tests`: nothing above the ceiling, a quiet signal untouched, the release, the handle) and through `sf_audio::limit_samples`, which runs the same effect over the mixed test signal. Not heard: a limiter with
  no attack can add a little distortion on a sharp transient at the ceiling; the ceiling and release are the numbers to tune by ear.
* **Unit voices and announcements (WP11.3c, A3).** The bank volumes of the six unit-voice events (joe and cori) and the 19 announcements went from 0.35 to **0.95** (pinned: `the_unit_voices_are_heard_over_a_fight_...`, `the_announcements_are_heard_over_a_fight`,
  `unit_voice` tests, `sf_audio` bank tests). The rule is restored: **every voice line's RMS is at least 0.5 (-6 dB) of the steady-state RMS of the 20-tank fight (0.278 before the master limiter)**, and its peak is at most that of the loudest single fight sound (0.793; a voice peaks at 0.754).
  Measured: joe's unit lines 0.147 to 0.303 (0.53 to 1.09 of the fight's RMS; the quietest is `unit_attack_engaging`), cori's 0.216 to 0.366 (0.78 to 1.32), the 19 announcements from 0.173 (0.62). The old rule "above the loudest single sound's RMS" (0.268) is dropped: it held only while every fight sound was quieter than a voice.
* **The cue limiter** (`FxSound::start_cues`; numbers in `audio.ron` `positional`) has three parts, and what it leaves the master limiter catches. Choice made in WP11.3c, after the first version failed (above): a **higher ceiling with a floor and priorities** rather than a shorter half-life
  (a shorter half-life frees more gain per tick but ducks a lone shot's hit more) or a sample-peak model (the master limiter is that). (1) **Per frame**: the cues of one frame are summed (bank volume x category x master x spatial gain, `AudioSystem::gain_at`); above `max_summed_gain` (2.0) all are turned down by the same factor. (2) **Load over time**: the gains started so far fade with a half-life
  of `load_half_life_ticks` (4, 0.2 s) into a *load*, by the sim ticks of the cues; the frame's cues are turned down together so the load stays at or below `max_load_gain` (1.15). (3) **Priority and floor**: a cue of an event whose planner `priority` is at least `protected_priority` (4: deaths and explosions) is not turned down by the load and takes its room first;
  no other cue goes below `min_cue_scale` (0.12) of its own gain. The sum of gains is a pessimistic model (sounds do not peak together), so the ceiling is a loudness budget rather than a clip guard; the master limiter is the clip guard. A mapped original sound is neither counted nor turned down (`a_mapped_original_is_not_counted_and_not_turned_down`,
  `a_mapped_original_is_left_alone_when_the_load_is_full`: with the load full an original still plays at 1.0 and the load does not rise). Measured in the 20-tank fight from tick 40 (`the_cue_limiter_keeps_every_cue_audible_in_a_dense_fight`): cannon fire plays at 0.30 of its own gain (mean, minimum the floor 0.12; first version 0.10), a cannon hit at 0.20 (first version 0.054), the large explosion at tick 100
  at 1.0 (first version 0.04), the medium explosions and the infantry deaths at 1.0; a lone shot and its hit 4 ticks later at 1.0 and 0.79 (first version 0.47). `the_load_limiter_fades_with_its_half_life` pins the arithmetic (1.0, 0.65, the floor 0.12, a protected explosion 1.0, 1.0 after a pause, `reset`). `a_heavy_moment_does_not_clip` (four, eight and all 18 kinds at one moment, repeated four times): 0.719, 0.703 and 0.627
  against 2.505, 4.654 and 7.928 with no limiter (pinned with a tolerance of 0.01 and 0.02). A cue turned down to scale 0 would start a silent voice that takes a `max_instances` slot: `AudioSystem::play_at_scaled` now starts nothing at scale 0 (`a_cue_at_scale_zero_takes_no_voice_slot`); with the floor it cannot happen from the limiter. The new numbers are range-checked (`max_load_gain` in (0, 4], `load_half_life_ticks` 0.5 to 200, `min_cue_scale` in (0, 1]),
  a file from before them loads with the shipped values (`a_file_without_the_limiter_numbers_gets_the_shipped_ones`), and the edges are tested (`the_range_edges_of_the_limiter_numbers`). The limits of the rule are in section 4.
* **Hidden events are audible** (current rule, `fog_look.md`): the audio plays `FxPlayer::shown()`, so a shot or an explosion in the shroud is heard at its distance volume, and with these sounds that is now true in the shipped game. Tijmen may change this ("no sound from what I cannot see"); it is a design choice, not a defect.

### Variants and the six redesigned sounds (WP11.3c)

Tijmen on 15 and 16-11-2026: "de unit destroyed, weapon fire demolition, impact bullet en impact precision en impact miss moeten heel anders", "impact precision moet anders",
"unit destroyed ook", "fire bullet moet overtuigender". Six events got new recipes and **three files each**, played through the bank's `variants` (each play picks one of `file` and `variants`,
never the one before; `pick_variant`, tests `pick_variant_takes_any_but_the_last_one` and, for these six through the game's `play_at`, `the_six_redesigned_sounds_play_their_variants_and_never_repeat`):

| Event | Files (first is `file`) | What it is |
|---|---|---|
| `unit.destroyed.infantry` | `unit_destroyed_infantry.wav`, `_shout`, `_grunt` | a short, rough, **synthesised** pain cry (a glided sawtooth, two formants, slow-noise roughness, saturation; no Piper, no voice data, no speech) then the body falling (thud and sub) and gear (three metal ticks and a rattle): a falling cry, a rising shout cut short, a low grunt |
| `weapon.fire.demolition` | `weapon_fire_demolition.wav`, `_drop`, `_slow` | a heavy satchel set down (thud and canvas), a metal clunk (two tones that do not fit, a tick) and a second small clunk, then a fuse hiss with sparks; dropped from higher, or a lower thud and a longer fuse |
| `impact.bullet` | `impact_bullet.wav`, `_metal`, `_ricochet` | a dry thwack (noise, a thump, a sub) with falling debris grains: into dirt, into metal (a dull ring), and with a short rough ricochet whine |
| `impact.precision` | `impact_precision.wav`, `_deep`, `_slap` | a sharp crack and a heavy thud, nothing tonal above 300 Hz (**no ping**); a bigger thud with a low tail; a slap back |
| `impact.miss` | `impact_miss.wav`, `_far`, `_skip` | a whiz-by that brightens up to a crack as it passes and darkens after it, then a puff of dirt: close, farther (softer crack, longer whiz), close with a bigger puff |
| `weapon.fire.bullet` | `weapon_fire_bullet.wav`, `_mg`, `_heavy` | a sharp report with body, a muzzle blast with low end (a thump and a sub), a mechanical action click and a short outdoor slap: rifle, three quick machine-gun reports, a heavy rifle |

Nothing here is made to imitate a specific game's sound (B11) and nothing was listened to by the author: the table says what each should sound like, and the checks are measurements
(RMS, low end, a spectral peak test for the ping, the band energy of the cry). Measured: the three infantry files have 6,492 to 9,942 RMS between 300 Hz and 2 kHz in their first 150 ms, the old one 3,251;
the old `impact.precision` had a spectral peak-to-mean of 29.2 over 800 Hz to 4 kHz, the new three 11.5, 6.9 and 6.3 (the test's limit is 16). A variant file that is a copy of another fails
`the_six_redesigned_sounds_have_exactly_these_variants_...` (three different hashes). The pitch spread of the entry applies to every variant. **With `--ra-audio`** a mapped original plays as-is (volume 1.0, no pitch spread, the default cap, **and none of the variants**);
of the six, `weapon.fire.bullet` and `unit.destroyed.infantry` are mapped, the other four have no Red Alert sound and always play ours.

| Event | File | Length | Character |
|---|---|---|---|
| `weapon.fire.bullet` | `weapon_fire_bullet.wav` | 0.45 s | rifle: sharp report, muzzle blast with low end, action click, outdoor slap |
| `weapon.fire.bullet` | `weapon_fire_bullet_mg.wav` | 0.5 s | machine gun: three quick reports, each with body, click and slap |
| `weapon.fire.bullet` | `weapon_fire_bullet_heavy.wav` | 0.5 s | heavy rifle: a lower, harder report with more body |
| `weapon.fire.precision` | `weapon_fire_precision.wav` | 0.6 s | harder crack, a heavy body and two echoes |
| `weapon.fire.cannon` | `weapon_fire_cannon.wav` | 0.8 s | deep boom (150 to 40 Hz) with sub, rumble, crack and a slap echo |
| `weapon.fire.rocket` | `weapon_fire_rocket.wav` | 1 s | pop, then a swelling whoosh with a low body that dies away |
| `weapon.fire.shell` | `weapon_fire_shell.wav` | 0.65 s | mid-pitched thump (220 to 75 Hz) with sub and echo, shorter than the cannon |
| `weapon.fire.howitzer` | `weapon_fire_howitzer.wav` | 1.3 s | very deep, long boom (100 to 28 Hz), a sub and three low echoes |
| `weapon.fire.demolition` | `weapon_fire_demolition.wav` | 0.8 s | a heavy satchel set down, a metal clunk, then a fuse hiss with sparks |
| `weapon.fire.demolition` | `weapon_fire_demolition_drop.wav` | 0.8 s | the satchel dropped from higher: a harder thud, an early second clunk, a fuse hiss |
| `weapon.fire.demolition` | `weapon_fire_demolition_slow.wav` | 1 s | a lower thud, a clunk, a slower and longer fuse |
| `impact.bullet` | `impact_bullet.wav` | 0.25 s | dry thwack into dirt, with falling debris |
| `impact.bullet` | `impact_bullet_metal.wav` | 0.25 s | a sharper thwack into metal with a dull ring and a few bits |
| `impact.bullet` | `impact_bullet_ricochet.wav` | 0.25 s | thwack, debris and a short rough ricochet whine |
| `impact.precision` | `impact_precision.wav` | 0.4 s | a sharp crack and a heavy thud, nothing tonal above 300 Hz |
| `impact.precision` | `impact_precision_deep.wav` | 0.5 s | a crack and a bigger, deeper thud with a low tail |
| `impact.precision` | `impact_precision_slap.wav` | 0.5 s | a crack, a thud and a slap back |
| `impact.cannon` | `impact_cannon.wav` | 0.9 s | blast: low body, sub, noise, crack and a tail |
| `impact.rocket` | `impact_rocket.wav` | 1.1 s | bigger blast with a long rumbling tail |
| `impact.shell` | `impact_shell.wav` | 0.75 s | medium blast, shorter and higher than the rocket's |
| `impact.demolition` | `impact_demolition.wav` | 1.5 s | heavy blast (80 to 25 Hz) with a sub and a long rumble |
| `impact.miss` | `impact_miss.wav` | 0.4 s | whiz-by that cracks as it passes, then a puff of dirt |
| `impact.miss` | `impact_miss_far.wav` | 0.45 s | a farther pass: a longer whiz, a softer crack, a puff |
| `impact.miss` | `impact_miss_skip.wav` | 0.45 s | a close pass with a bigger puff of dirt |
| `unit.destroyed.infantry` | `unit_destroyed_infantry.wav` | 0.7 s | a rough falling pain cry, then the body falls and gear rattles |
| `unit.destroyed.infantry` | `unit_destroyed_infantry_shout.wav` | 0.6 s | a short rising shout cut off, a fall, gear |
| `unit.destroyed.infantry` | `unit_destroyed_infantry_grunt.wav` | 0.7 s | a low grunt, a heavy fall, gear |
| `explosion.small` | `explosion_small.wav` | 1 s | small explosion with sub and a short tail |
| `explosion.medium` | `explosion_medium.wav` | 1.5 s | medium explosion with a rumbling tail and echo |
| `explosion.large` | `explosion_large.wav` | 2.4 s | large explosion (80 to 26 Hz), sub, long rumble and echoes |

Tests: `sf_sound_gen` `tests/generator.rs` (determinism and a pinned length and hash per file, the committed files equal a fresh run,
each file loud, unclipped, the right length, under budget, each event's bank entry points at its first file and lists the rest as `variants`, `every_file_has_power_rms_and_low_end`, `impact_precision_has_no_ping_in_any_variant`, `the_infantry_death_starts_with_a_cry`, `the_six_redesigned_sounds_have_exactly_these_variants_and_nothing_else_has_any`); `sf_game` `fx_sound::tests`
`every_fight_event_has_a_non_silent_cc0_sound_in_the_shipped_bank` (literal event to file list, volume, the game's own `has_sound`
and trace), `a_20_tank_fight_is_at_most_1_8_times_the_music`; `sf_game` `the_six_redesigned_sounds_play_their_variants_and_never_repeat`; `sf_audio` `the_original_wins_where_mapped_and_the_cc0_fight_sound_plays_otherwise` (now also: a mapped original has no variants).

## 4. Gaps

* Clipping (limiters, rule in section 3c): the **master limiter** on the main track stops any sample above 0.95 (fight, music, voices, interface, mapped originals), tested on the mixed signal of the 20-tank fight, music and voices; the **cue limiter** is a gain model, not sample peak tracking (the load is the sum of gains of what started, faded with one half-life for every sound, a file's tail fades at its own speed), so it is a loudness budget; the numbers come from the 20-tank fight, the volley and the heavy moments against the real files, a real fight is not measured. The summed gain counts a cue that the voice cap (`max_instances`) later drops, so a frame can be turned down more than needed; a mapped original sound is not counted by the cue limiter (the master limiter still holds it); the master limiter has no attack time, so a sharp transient at the ceiling can distort a little, which is not judged by ear; it is not known whether anybody has heard either limiter act (the sandbox fight Tijmen listened to on 12-11-2026 does not show it, and nothing counts it).
* Fight sounds (WP11.3b, WP11.3c): twelve events still have one file, so their only
  variety is `pitch_variation`; six have three files since WP11.3c (the variants above). Not heard by the author (WP11.3c). Tijmen listened to the release build of PR #157 on 18-11-2026 and said "Ja, merge"; the new files and volumes are what he heard. The volumes come from a measured mix, the character of each sound from the table in 3c above.
  Tijmen listened on 12-11-2026 in `sf_game --sandbox --fight` without `--ra-audio` and judged the sounds good (`progress.md`, WP11.3b
  notes). Still open: `weapon.fire.demolition`, since WP11.3c a satchel thrown down and a burning fuse (three variants), for any likeness to a Red Alert sound (B11); Tijmen approved the WP11.3c sounds on 18-11-2026 ("Ja, merge"); whether this one played in the fight he listened to is not recorded. The sim has no event for a structure being hit, so there is no such sound.
* The sim emits no event for a structure placed or finished, or a harvester unloading; nothing was added to `sf_sim`. The replies
  to selecting and ordering a unit are WP11.2 (`voices.md`), raised on the UI side. **Announcements** ("Unit ready", "Base under
  attack", "Power low", ...) are played since WP11.2c (`announcements.md`): raised on the UI side from the snapshot, the shown
  effects events and the sim's refusals, with a brake and a priority; our own Piper lines without Red Alert files, a mapped Red
  Alert line with `--ra-audio`.
* Unit voices (WP11.2b): **no longer silent without Red Alert files.** The bank has three lines per event (Piper voice
  `en_US-joe-medium` through our walkie-talkie filter, `tools/sf_voice_gen`, CC0), picked per play as variants (`variants` in `bank.ron`, never the same twice in a
  row); `voices.md` section 3a. Since WP11.2d `unit.select` has nine lines and the Veyran side has its own voice (cori, the `unit.*.veyran` events); `voices.md` section 3c. Tijmen listened to the filtered lines on 15-11-2026 and judged them good; not yet heard in the game. `building.placed` still has no CC0 sound (no code plays it).
  Music without Red Alert files is WP11.4b, not this WP.
* The volumes are set in the file (`mix`) and, since WP11.5b, on the settings screen's Sound page (see "Volumes").
* `audio.ron` is read once at start; no hot reload.
* A moving unit's sound stops at the place it had when the event happened (no Doppler, no following).
* Music is no longer silent without the player's Red Alert files (WP11.4b, section 3d). Tijmen listened to both tracks on 15-11-2026 (head a0f43ec) and said "muziek is goed". What stays open there: the two own tracks are decoded on the calling thread at the first mood change to each (a hitch of the size
  printed in section 3d, once per track per run); each mood has one track, so the same loop repeats every 68 s of a long fight or build-up; the
  unit voices are WP11.2b, not part of this.
* Red Alert music is decoded whole on a background thread, not streamed from disk: 30 MiB per running track (normally two
  in memory, running + fading; briefly three plus the 16-bit copy when the mood flips back during a fade), and the track starts a
  few tens of milliseconds after the mood changes (the old one plays on until then). A streaming decoder would cut the memory.
* With the shipped empty `music` section of `ra_mapping.ron` calm and combat draw from the same pool of Red Alert tracks, so a mood change plays another track of
  it. Tune by ear in the F1 sound browser and fill the lists in `ra_mapping.ron`.
* The wiring in `Runner::frame` (one `music_frame` call) is covered by the live run only; the pieces it calls are unit-tested.
* The end of a fight and a track change share one fade time (`music.crossfade_ms`).
