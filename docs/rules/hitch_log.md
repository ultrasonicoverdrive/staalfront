# The hitch log and the command record (WP1.16a)

Why: a play of M1 on the reference laptop stuttered (follow-up 35 in `docs/progress.md`: frames of 0.6 to 1.6 s with vsync on,
125 late snapshots) and the game could not say what a long frame was made of. WP1.16a measures; WP1.16b was to fix what the
measurement shows. The laptop run of 11-11-2026 showed nothing to fix and Tijmen closed WP1.16 that day, so WP1.16b is not built
(follow-up 35). Code: `crates/sf_game/src/hitch.rs`, `record.rs`, `runner.rs` (the laps), `tools/sf_headless/src/scenario/`
(`exact.rs`, `game.rs`).

## Flags

| Flag | What it does |
|---|---|
| `--hitch-log FILE` | writes the hitch log (JSON, below) when the game ends. Works in every scene. |
| `--record FILE` | with `--match`: writes every command of the play as an `sf_headless` scenario when the game ends. |
| `--replay FILE` | with `--match`: sends the commands of a record into the running game at the ticks they ran at. |

The exit summary (always printed) has a line `summary: hitches: ...` with the total number of frames, the number over
`sf_game::bench::HITCH_MS` (25 ms) and over 50 ms, with and without the first `WARMUP_FRAMES` (90) frames, the longest frame, the
late snapshots, the sim ticks over 20 ms and the swap-chain acquires that gave no normal image. Both files are written only when
the game ends normally (window closed, Esc); a crash writes neither.

## What a frame is, and the parts

A **frame** is the time from the start of one `render` callback to the start of the next (the number `FrameStats` and the
benchmark use). It is the callback plus the **gap** after it, in which the event loop was in the window system, the driver or the
operating system. A frame over 50 ms (`hitch::LOG_MS`) writes one line, made when the next frame starts, so it carries the parts of the
frame that ran long:

| Field | Meaning |
|---|---|
| `frame`, `at_s`, `tick` | the number of the long frame (first = 1), seconds since start, the sim tick of the snapshot it drew |
| `frame_ms`, `gap_ms`, `callback_ms` | the whole frame, the time after the callback, the callback |
| `parts.snapshot_ms` | taking the newest snapshot: convert for the instancer, the fog upload to the minimap (`sync_fog`), the ore |
| `parts.ui_ms` | HUD, menus, camera |
| `parts.scene_ms` | building the scene: instances, effects, overlays, bars, cursor |
| `parts.upload_ms` | the instance upload (a buffer that has to grow is made here) |
| `parts.audio_ms` | `AudioSystem::update` (the music hand-over), the mood, starting the sounds |
| `parts.acquire_ms`, `record_ms`, `submit_ms`, `present_ms` | the draw call split: waiting for the swap chain image (with vsync: for the display), recording the passes, `queue.submit`, `queue.present` |
| `parts.input_ms` | the input handlers after the draw, and the once-a-second console line |
| `cpu_ms` | CPU time of the main thread over `frame_ms`; `null` where it is not measured |
| `sounds_started` | sounds started in the callback |
| `events` | window events (resize, move, focus, occlusion, scale factor), `on_resize` and its time, a music track that started or was decoded (with its decode time), instance buffers that grew, a swap-chain acquire that gave no image (`timeout after 1000 ms`) |
| `sim_slow_ticks` | `(tick, ms)` of sim ticks over 20 ms that ran since the long frame's snapshot |

The parts are laps of one callback in the order it runs, so they add up to `callback_ms` (the draw lap is split by what the renderer
measured; `record_ms` is the rest). The file also has the totals, the sim thread's numbers (late snapshots, behind passes, clock
restarts, slow ticks), `buffer_growth` (every frame in which instance buffers were created or grown, with the upload time) and
the cost of the CPU clock.

**Reading it.** `acquire_ms` near 1000 with a `timeout` event: the display did not give an image (wgpu caps the DX12 wait at 1,000 ms).
A long frame with small `cpu_ms`: the thread was not running (other program, antivirus, a modal window loop). A long frame with
`cpu_ms` of the same size: our code was busy; the part says where. A big `gap_ms` with `resized` or `moved` events: the window system.

**CPU time.** On Windows from `GetThreadTimes` through the small crate `sf_thread_cpu` (it holds the one `unsafe` call of the workspace, which forbids unsafe code everywhere else; a direct, Windows-only `windows-sys` 0.61 dependency with the features
`Win32_Foundation` for the `FILETIME` and `HANDLE` types in the signature and `Win32_System_Threading` for the call). It is read once per
frame; the file reports the number of calls and what they cost in all. Elsewhere `std` has no thread CPU time and the field is
`null` (`cpu_measured: false`). The Windows code cannot be run in the container; its conversion (`filetime_duration`) is tested.

**Cost.** A frame that is not slow: one comparison against `HITCH_MS`, seven clock reads (the laps), one comparison for the buffer count,
one for the acquire outcome and, on Windows, one `GetThreadTimes` call. Measured by the test `the_bookkeeping_of_a_quiet_frame_is_small`
(`--nocapture`): see the PR for the number. A line is built only for a frame over 50 ms, at most 2,000 of them.

## The command record

The sim thread keeps, with `--record`, every command it was handed (`SimConfig::record_commands`): who sent it, the tick it runs at,
and the number of ticks the world had run when it got it (`arrived`). `record::write` turns them into a scenario for `sf_headless`:

```
Scenario(
    name: "recorded_match", seed: 1, map: (width: 96, height: 96),
    game: Some((assets: "/abs/path/assets", match_path: "matches/m1.ron", speed_raw: 98304)),
    ticks: 8710,
    recorded: [
        (arrived:12,tick:13,player:0,command:BuildStructure(kind:1,cell:(8,62))),
        (arrived:2400,tick:2401,player:0,command:Order(ids:[(15,0),(16,0)],order:AttackMove((4390912,1441792)),queued:false)),
    ],
)
```

* `game` says the world is the one `sf_game --match` makes: the unit definitions from `<assets>/config`, the match file and its map under
  `assets`, every other rule from the settings **compiled into the build** (so a record replays with the build that wrote it or one with
  the same rules), in the order of `matchplay::configure`, the starting objects scheduled and one tick run (as `match_scene` does before the
  sim thread starts). `ticks` is the world tick the run ends at. With `game` the rule fields of the scenario (`unit_defs`, `combat`, ...,
  `spawns`, `commands`) must be left out (a problem is reported if not).
* `recorded` holds the commands in the order the world got them. Values are exact: positions are raw fixed-point numbers (`Fx::raw`, 16
  fractional bits), ids are `(index, generation)`. `ExactCommand` has one variant per `sf_sim::Command` (the conversion is exhaustive, so a new
  command does not compile until it is covered) and `every_command_round_trips` writes and reads one of each, fixed-point bits included.
  WP6.10 added `SetRepeat(factory: (14, 2), on: true)` and `SetRepeat(factory: (14, 2), on: false)` (the ids and both `on` values `every_command_round_trips` uses) to that list (and to the scenario file's `commands`, `Action::SetRepeat`).
* **`arrived` matters.** The world's checksum includes the commands waiting for their tick, so `sf_headless` schedules each recorded command
  when the world has run `arrived` ticks (`Scenario::arrivals`, used by `run::run`), not before tick 0 as it does for `commands`. Without
  it the checksums differ (`the_arrival_tick_is_what_makes_the_checksums_equal`). A command that arrived after the last tick is not written.
* Replay: `sf_headless FILE [--every 1 --out log.json]`. The test `tests/record_replay.rs` of `sf_game` plays 7,000 ticks of M1 on the
  game's sim thread (the production world builders, `SimHandle::send`; the pinned `match` script decides the match, the human wins), writes the record with the production writer, loads it with
  `sf_headless` and compares the checksum at every tick (`a_recorded_match_replays_to_the_same_checksum_at_every_tick`).
* `game.assets` is the absolute path of the machine that wrote the record (for example `C:/...` on the laptop); `GameSpec` also takes
  a path relative to the record file. To replay a record on another machine, set `assets` to that machine's `assets/` (absolute, or
  relative to where the record lies) and use a build with the same rules.
* The summary's `scheduled` count includes the recorded commands (`commands_scheduled` = scheduled before tick 0 + `recorded`).
* A restart from the menu ends the record of that game: it is written to `FILE` with `.restartN` before the extension, and the new game
  is recorded under `FILE`.
* `--replay` is not tick-exact: the game's sim has its own clock and the commands are sent a tick ahead of the snapshot, like a player's.
  It is for repeating a play for a measurement, not for checksums.
