# Announcements (WP11.2c)

Short status lines about the player's own base: "Unit ready", "Base under attack", "Power low". Presentation only:
nothing here reaches `sf_sim`, nothing is hashed, no pinned checksum can move. Asked for by Tijmen on 15-11-2026
("bouw dat in. en zet ze zelf bij de juiste"): the moments are chosen from **our own game's rules**, not from Red
Alert (when each Red Alert line plays there has not been checked).

* **Without the player's Red Alert files** our own lines play: the Piper voice `en_US-joe-medium` through the same
  walkie-talkie filter as the unit voices (`voices.md` section 3a, `tools/sf_voice_gen`, the filter unchanged: the nine
  unit voice files and their pins are byte-equal).
* **With `--ra-audio`** a mapped Red Alert line found in the player's own files wins and plays as-is (volume 1, no pitch
  spread), exactly like the unit voices. A mapped name whose file is not found, and the one event with no Red Alert
  line, play our own line.

## 1. The 19 events

All 19 are raised. For each, the moment, found in the data the game already has, only for the **local player**
(`commands::LOCAL_PLAYER`; `Facts::read` looks at the entities it owns, the shown deaths are filtered on the owner,
the refusals on the sender).

| Event | Our line | Red Alert file | Raised when (the first snapshot where it is true) |
|---|---|---|---|
| `announce.structure_ready` | "Structure ready" | `CONSCMP1.AUD` | a structure of the player that was going up is complete (still there, not being sold) |
| `announce.unit_ready` | "Unit ready" | `UNITRDY1.AUD` | a mobile unit of the player appears that was not there (leaves a factory, the refinery's free harvester included) |
| `announce.building` | "Building" | `ABLDGIN1.AUD` | a structure of the player starts going up (the sim took the placement) |
| `announce.training` | "Training" | `TRAIN1.AUD` | a standing factory of the player has more paid items in its queue |
| `announce.on_hold` | "On hold" | `ONHOLD1.AUD` | a factory starts to hold: its finished unit finds no free spot at the exit, or a structure its unit needs is gone (`FactoryStatus::WaitingForExit`, `MissingPrerequisite`) |
| `announce.cancelled` | "Cancelled" | `CANCLD1.AUD` | the factories lost more paid items than units came out (a unit taken out of the queue) |
| `announce.base_under_attack` | "Base under attack" | `BASEATK1.AUD` | a structure of the player has fewer hit points than in the snapshot before (not while it is being sold) |
| `announce.harvester_under_attack` | "Harvester under attack" | none, our own line only | a harvester of the player has fewer hit points than before |
| `announce.low_power` | "Power low" | `LOPOWER1.AUD` | the player's power speed falls below 1000 (use above output), when it starts |
| `announce.insufficient_funds` | "Not enough credits" | `NOFUNDS1.AUD` | a factory waits for money (`WaitingForMoney`) or a structure going up does (`waits_for_money`), when it starts |
| `announce.storage_full` | "Storage full" | `SILOND1.AUD` | the ore store holds at least its capacity (the same definition as the storage bar), when it starts |
| `announce.unit_lost` | "Unit lost" | `UNITLST1.AUD` | a shown `Death` event of a mobile unit of the player |
| `announce.structure_lost` | "Structure lost" | `STRCKIL1.AUD` (closest match only, medium confidence) | a shown `Death` event of a structure of the player |
| `announce.new_options` | "New options available" | `NEWOPT1.AUD` | the sidebar offers a kind it has not offered in this game (`sidebar::available`); each kind counts once per game |
| `announce.cannot_deploy` | "Cannot build there" | `NODEPLY1.AUD` | the simulation refused a placement of the player (`PlacementRejection`, the same refusals the top bar words) |
| `announce.victory` | "Victory" | `MISNWON1.AUD` | the match result becomes `Won(player)` |
| `announce.defeat` | "Defeat" | `MISNLST1.AUD` | the match result becomes `Won(someone else)` |
| `announce.structure_sold` | "Structure sold" | `STRUSLD1.AUD` | a sale of a structure of the player starts (the structure stops standing from that tick, `repair_sell.md`) |
| `announce.repairing` | "Repairing" | `REPAIR1.AUD` | a repair of a structure of the player starts |

Notes on the moments:

* **Edges, not states.** An announcement is raised when something *starts*; a state that stays (low power, a full store) is
  said once. `changes(prev, now)` compares the facts of two snapshots; `Facts::read` is the only reader of
  `SimSnapshot` and is called once per new tick.
* **Under attack is the hit points.** The simulation has no "structure hit" event (`audio.md`), so the attack is read as
  a lower hit-point count than in the previous snapshot. A unit that is hit says nothing (that is a fight, not the base);
  a harvester has its own line. A one-shot kill shows as a loss, not as an attack.
* **Losses come from the shown effects events** (`FxPlayer::shown`, the same list the fight sounds and the music mood
  read), so a loss is heard in step with its explosion. A structure that is sold is not a `Death` (no "lost").
* **`storage_full` is the game's own full.** The store of the player is full when stored >= capacity > 0, the rule of the
  storage bar (`economy.md`). It also holds at the moment a refinery completes while the player holds more credits than
  the new capacity, which is true to the game (the harvesters cannot unload) and the first thing a player may hear in a
  match; it can switch on and off while a harvester unloads, which the repeat gap (30 s) thins out.
* **`cannot_deploy` reads the sim's refusals**, not the ghost: every left click on the ground in the build mode sends a
  `BuildStructure`, the simulation says no, the refusal of the local player (and only of the local player) is announced. It is
  read from `SimHandle::rejected_builds`, the same list as the top bar.
* **Draw.** When the last players fall in the same tick nobody has won (`match.md`): no victory and no defeat line.

Not announced because the game has no such moment, and no sim state was invented for them: see section 6.

## 2. Nothing for the starting objects

`announce.settle_ticks` (100 ticks, 5 s) after a game starts: the first facts and every one in that time only set the
baseline, so the objects the match file puts into the world (they enter through commands on the first tick) are not news.
The options the sidebar offers at that moment are the baseline of `new_options`. Only a match speaks
(`Scene::Match`): the sandbox and the crowd scenes have no base of the local player. `Announcer::reset` (a new game)
forgets the baseline, the queue and the gaps.

## 3. The brake (numbers in `assets/config/audio.ron`, section `announce`, pinned by `config::tests`)

| Number | Shipped | Meaning |
|---|---|---|
| `min_gap_ms` | 2000 | After an announcement has started, no other one starts within this many ms. |
| `max_wait_ms` | 4000 | An announcement that has waited this long for a free voice is dropped (old news is not news). |
| `queue_len` | 4 | At most this many wait; when it is full the lowest priority, then the oldest, goes. |
| `settle_ticks` | 100 | Nothing is raised in this many ticks after a game starts. |
| `fallback`, `events` | priority 2, 5000 ms | Per event: `priority` (higher first) and `repeat_gap_ms`, the time before the *same* announcement may start again. |

Per event (priority, repeat gap): victory and defeat 10, 60 s; base under attack 8, **20 s**; harvester under attack 7,
**15 s**; structure lost 7, 6 s; low power 6, 30 s; unit lost 5, 6 s; not enough credits 5, 15 s; storage full 4, 30 s;
cannot build there 4, 3 s; structure ready, unit ready, structure sold 3, 2 s; on hold 3, 10 s; new options 2, 10 s;
building, training, cancelled 2, 2 s; repairing 2, 5 s.

* **One at a time.** `Announcer::speak` starts at most one announcement per call, and only when `min_gap_ms` has
  passed since the last one started.
* **The same event is not queued twice.** `raise` refuses an event that is already waiting, and one that started inside
  its own `repeat_gap_ms` (counted from when it started). A base that is hit for two minutes says so every 20 s, not
  every second (`the_shipped_brake_thins_a_long_attack_to_one_announcement_every_20_seconds`: 150 s of attack in a real
  match world gave five "base under attack", 400 ticks apart).
* **A waiting queue, by priority.** What cannot be said at once waits (it is not lost like a skipped unit voice): the
  highest priority first, equal ones in the order raised.
* **A silent event** (nothing in `bank.ron`, no Red Alert file) is dropped without a message and uses up no gap.

### Priority against the unit voices (decision)

Unit voices and announcements share **one voice channel** (`announce::voice_channel()`: the 3 unit events and the 19
announcements, what `AudioSystem::is_playing` is asked):

1. An announcement **never starts over a voice that is sounding**, a unit reply or another announcement; it waits (up to
   `max_wait_ms`) and starts when the channel is quiet.
2. A unit voice that is asked for **while an announcement sounds is skipped** (the unit voices already drop a request while
   one of them sounds, `voices.md` section 2; the announcement is at most 1.31 s). The order itself is always given;
   only its spoken reply is lost.
3. Nothing is cut off.

Why: a click's reply is the shorter, more frequent line and "Base under attack" must not be drowned or doubled by it, but
an announcement that waits behind a reply loses nothing because its news is still true half a second later.

The announcements go through `AudioSystem::play_as(.., Category::Voice)`: full volume, so the **unit voices volume** of the
settings (`mix.voice_volume`, the "Unit voices" row of the sound page) and the master volume apply, not the effects volume.
There is no separate slider for announcements.

## 4. Sources

* The 19 raw Piper renders (the same piper-tts 1.8.0 and voice as the unit voices) are in
  `tools/sf_voice_gen/voice_src/announce_*.wav`, the texts in `tools/sf_voice_gen/piper_lines.py`; `cargo run -p
  sf_voice_gen` filters them into `assets/audio/voice/announce_*.wav` (pinned: length and hash of every source and every
  output). The filter is untouched; the 19 files are 0.51 to 1.31 s, so the unit voices' 1.5 s limit holds for them as well and no
  word was cut (`every_announcement_is_an_unclipped_mono_wav_of_at_most_1_5_s_with_the_same_peak`).

| Event | File | Length | Words |
|---|---|---|---|
| `announce.structure_ready` | `announce_structure_ready.wav` | 1.013 s | "Structure ready" |
| `announce.unit_ready` | `announce_unit_ready.wav` | 0.794 s | "Unit ready" |
| `announce.building` | `announce_building.wav` | 0.511 s | "Building" |
| `announce.training` | `announce_training.wav` | 0.543 s | "Training" |
| `announce.on_hold` | `announce_on_hold.wav` | 0.68 s | "On hold" |
| `announce.cancelled` | `announce_cancelled.wav` | 0.664 s | "Cancelled" |
| `announce.base_under_attack` | `announce_base_under_attack.wav` | 1.101 s | "Base under attack" |
| `announce.harvester_under_attack` | `announce_harvester_under_attack.wav` | 1.31 s | "Harvester under attack" |
| `announce.low_power` | `announce_low_power.wav` | 0.66 s | "Power low" |
| `announce.insufficient_funds` | `announce_insufficient_funds.wav` | 1.225 s | "Not enough credits" |
| `announce.storage_full` | `announce_storage_full.wav` | 0.958 s | "Storage full" |
| `announce.unit_lost` | `announce_unit_lost.wav` | 0.88 s | "Unit lost" |
| `announce.structure_lost` | `announce_structure_lost.wav` | 1.157 s | "Structure lost" |
| `announce.new_options` | `announce_new_options.wav` | 1.228 s | "New options available" |
| `announce.cannot_deploy` | `announce_cannot_deploy.wav` | 1.018 s | "Cannot build there" |
| `announce.victory` | `announce_victory.wav` | 0.629 s | "Victory" |
| `announce.defeat` | `announce_defeat.wav` | 0.546 s | "Defeat" |
| `announce.structure_sold` | `announce_structure_sold.wav` | 1.019 s | "Structure sold" |
| `announce.repairing` | `announce_repairing.wav` | 0.655 s | "Repairing" |

* **Bank.** Each event has one entry in `assets/audio/bank.ron`: category `Voice`, `max_instances: 1`, `original_as_is:
  true`, volume **0.35**, the volume of the unit voices (their level was measured against the loudest fight sound,
  `voices.md` section 3a; the files have the same peak). Pinned as literals by
  `the_shipped_bank_speaks_every_announcement_without_red_alert_files`. **Not measured separately:** the announcement
  lines have not been put through the fight-loudness test of the unit voices; they share file peak and volume with them, and
  are longer.
* **Red Alert.** The 18 names are in `assets/audio/ra_mapping.ron`, one per event, all `.AUD` files of `SPEECH.MIX`,
  checked one fact at a time by `onderzoeker` (EA's source and OpenRA agree); `STRCKIL1.AUD` for "structure lost" is the
  closest match only. `announce.harvester_under_attack` has none. No Red Alert file is in the repository. The names are
  pinned by `shipped_mapping_names_are_pinned`; with a fake install, the original wins where found and our line plays
  otherwise: `the_original_announcement_wins_where_found_and_our_line_plays_otherwise`.

## 5. Where it is wired

* `crates/sf_game/src/announce.rs`: `Announcement` (the 19 events), `Facts` and `Facts::read`, `changes`, `deaths`,
  `refusal`, `Announcer` (the brake and the queue), `AnnounceOut` and `voice_channel`.
* `crates/sf_audio/src/announce_config.rs`: `AnnounceConfig`/`AnnounceRule` (`announce` in `audio.ron`).
* `runner.rs`: in the audio part of the frame, after the music mood and before the fight sounds (the shown events
  are still there): `Facts::read` when the snapshot's tick is new, `announce::deaths(self.fx.shown())`, `speak`; and in
  `handle_build` the refusals of the local player through `announce::refusal`. `Announcer::reset` with a new game.
* `unit_voice.rs`: `busy()` asks the whole voice channel.

## 6. Tests

`sf_game` `announce::tests` (the 19 names as literals, the voice channel, every event has a rule and a sound, each moment
and its negatives with stated facts, the brake: min gap, repeat gap, not queued twice, priority order, queue overflow, old
news, silent event, a voice that is sounding, the settling time, new options once per game, reset);
`crates/sf_game/tests/announcements.rs` (the first match played in a real `World`, a snapshot after every tick, the
production functions: starting objects silent; building, structure ready, new options, training, unit ready, cancelled; an
enemy tank: harvester and base under attack, repair, sale and low power, losses, and the opponent's unit says nothing;
victory and defeat in the tick the match is decided; a refused placement of the local player and of the opponent; running out
of credits; the shipped brake over a 150 s attack); `sf_audio` `config::tests` (the shipped numbers),
`ra::tests::shipped_mapping_names_are_pinned`, `system::tests` (the bank, the original wins where found);
`tools/sf_voice_gen/tests/generator.rs` (sources and outputs pinned, committed files equal a fresh run, the nine unit voice
pins unchanged, lengths, words).

**Checked by reading only:** the runner's calls (they need a window): `Facts::read` once per new tick, `deaths` of the shown
events, `speak` once a frame, the refusal call in `handle_build`. A hearing test needs the player's own Red Alert files and
a sound device, neither of which CI has.

## 7. Not done and gaps

* **Not every event can be tested in a real world.** Not in the world tests: `on_hold` (a factory whose unit finds no exit),
  `storage_full` as an edge by itself (it does show in the played match at the moment the refinery completes), `new_options`
  for the second time; these are covered by the stated-facts tests of `changes` only. A factory that **repeats** its item
  refills at once, so a cancel of that factory is not seen (a unit that comes out of it is).
* **`announce.insufficient_funds`** is not raised for a repair that waits for money (`RepairStatus::WaitingForMoney`), nor for a
  player who simply has too little when clicking: the production queue accepts the item and waits, so the line comes when the
  waiting starts.
* **No per-unit or per-faction voice**, no separate volume slider, no announcement for the opponent's actions, no "reinforcements
  have arrived", "building captured", "mission accomplished" or timers: the game has none of these moments.
* A skipped unit voice is lost, not delayed (section 3).
* **Heard by nobody yet** in a running game: neither the 19 files nor the mapped Red Alert lines (the loudness of the lines against
  a fight is by construction, section 4).
* The numbers (gaps, priorities) are a first guess from the rules above; they are in `audio.ron` to be tuned by ear.
