# Laptop benchmarks (M0, M0b, M0c)

The fps requirement (p99 frame time <= 16.7 ms with 5,000 units in view, PLAN.md section 2.4) cannot be measured in the
container or in CI (no real GPU, a shared CPU), so Tijmen runs the benchmark of `sf_demo` on the reference laptop
(GTX 1650, 1920 x 1080). The three runs use the **same camera flight** (about 75 s, nine segments, `sf_game::bench`)
over the **same 5,000 wandering units**; what is added between them is what makes them different:

| Run | Command | What is in the scene | Result file |
|---|---|---|---|
| M0 (22-10-2026) | `sf_demo --bench` | the crowd, the test pack's models, nothing else | `m0-2026-10-22.json` (p99 3.895 ms) |
| M0b (after WP2.4) | `sf_demo --bench` | the same, with the terrain materials and the cell grid | (not in this folder) |
| **M0c (after WP5.6b)** | `sf_demo --bench --fight` | the same crowd and path, plus the **armed skirmish**: shots, hits, deaths, wrecks, smoke and tracers | `m0c-2026-10-28-1.json` (p99 4.42 ms), `m0c-2026-10-28-2.json` (p99 4.35 ms); see "M0c result" |
| Plain, same day as M0c | `sf_demo --bench` | as M0b, on the main of 28-10-2026 | `plain-2026-10-28.json` (p99 3.993 ms) |

## M0c result (28-10-2026): met

Run on the reference laptop (GTX 1650, DX12, 1920 x 1080 borderless, no vsync, release) from main `ddf6113` (WP11.3),
built with `cargo build --release -p sf_demo -p sf_game -p sf_ra_extract`, then started after Tijmen's cool-down pause
(its length was not timed); the two fight runs one after the other, then the plain run. Whether the laptop was on mains power and the Windows GPU setting were not recorded (the
adapter in the JSON is the NVIDIA card). All three runs reached tick 1500 with the pinned end values (`FIGHT` and `PLAIN`
below).

| | M0 (22-10) | plain (28-10) | M0c run 1 | M0c run 2 |
|---|---|---|---|---|
| frames in 75 s | 26,880 | 26,874 | 24,110 | 24,458 |
| frame ms p50 / p95 / **p99** / max | 2.955 / 3.634 / **3.895** / 10.27 | 2.967 / 3.779 / **3.993** / 5.558 | 3.206 / 4.056 / **4.42** / 16.743 | 3.148 / 4.012 / **4.35** / 6.046 |
| cpu ms p99 | 3.677 | 3.777 | 4.208 | 4.156 |
| gpu ms p99 | 0.726 | 0.588 | 0.600 | 0.599 |
| hitches > 25 ms | 0 | 0 | 0 | 0 |
| sim tick ms p50 / **p99** / max | 1.38 / **2.126** / 3.044 | 1.505 / **2.224** / 2.65 | 2.085 / **3.238** / 6.789 | 2.063 / **3.233** / 5.611 |
| late publishes / behind passes | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |

* **Criterion met**: frame p99 4.42 and 4.35 ms against 16.7 ms; every phase's p99 is between 4.06 and 4.56 ms, the fight
  phases (`close_up`, `mid_hold`, `low_pass`) included. `sim.tick_ms` p99 3.2 ms against the 50 ms tick.
* **What the fight costs**, against the plain run of the same day: about +0.4 ms frame p99, almost all CPU (+0.4 ms cpu
  p99; gpu p99 unchanged at 0.6 ms), and +1.0 ms sim tick p99.
* **Effects**: about 12,360 events per run, `fx_dropped`, `particles_dropped`, `smoke_refused` and `tracer_dots_dropped`
  all 0; `particles_peak` 2,027 and 2,014 of 4,096; 3,436 and 3,428 flashes at an anchor against 12 by the rule; up to
  491 effect instances in a frame. No reason to lower `max_particles`.
* **The 16.7 ms max of run 1** is a single frame in `close_up` (frame 411, cpu 3.2 ms), early in the flight; run 2's max is
  6.0 ms. It stays under the 25 ms hitch threshold.

## M0c, step by step (Tijmen, Windows laptop, PowerShell)

1. Plug the laptop into **mains power**. Make sure Windows uses the **NVIDIA GPU** for `sf_demo.exe` (Settings, System,
   Display, Graphics) and **close other GPU-heavy programs** (browsers with video, games, recorders).
2. Update to the commit that has WP5.6b and build it: `git pull`, then `cargo build --release -p sf_demo`.
3. **Leave the laptop idle for at least 5 minutes after the checkout or the build** (nothing running, no compiler, no
   indexer, no updates). The build heats the CPU and the first minutes after it are not representative; a run in that
   window can show a p99 that is not the game's.
4. Run it **twice**, one after the other, each with its own output file (the second run shows whether the first was
   a cold start):

```powershell
cd C:\Users\Tijmen!\staalfront
cargo run --release -p sf_demo -- --bench --fight --out C:\Users\Tijmen!\bench-m0c-1.json
cargo run --release -p sf_demo -- --bench --fight --out C:\Users\Tijmen!\bench-m0c-2.json
```

   Do not touch the mouse or keyboard during the flights. Each opens in real fullscreen at 1920 x 1080 without vsync, flies
   the path (about 75 s plus a start-up of a few seconds, in which the map is generated), writes its JSON and quits.
5. **Paste back both JSON files** and the last lines each run printed (`bench: ...`).

For a comparison with M0 on the same laptop on the same day, run the plain benchmark once as well, before or after,
with the same idle rule: `cargo run --release -p sf_demo -- --bench --out C:\Users\Tijmen!\bench-plain.json`.

## What to read in the JSON

* `frame_ms.p99` and `criterion` (target 16.7 ms), `gpu_ms`, `hitches` as in M0. `phases[]` gives the same numbers per
  segment of the flight; the segments that look at the fight are **`close_up`, `mid_hold` and `low_pass`**.
* `sim.tick_ms` (the budget is 50 ms; p99 should be well inside it), `sim.late_publishes`, `sim.behind_passes`.
* **New in WP5.6b:** `scene.combat` (true), `scene.skirmish_units` (720: the units of all waves, not all alive at once),
  and the block `effects`:
  * `fx_events`: shots, hits and deaths the simulation recorded for the picture over the flight; `fx_dropped` (the
    queue of 4,096 overflowed: it should be 0);
  * `particles_peak` (the most particles alive at once, of 4,096), `particles_dropped` (pushed out of the full pool: it
    should be 0), `smoke_refused`, `tracer_dots_dropped`;
  * `flashes_at_anchor` / `flashes_by_rule`: muzzle flashes that started at the model's `muzzle` node and flashes that used
    the bounding-box rule (the test pack's models, wrecks);
  * `effect_instances` (average and largest per frame), and the same per phase in `phases[].effect_instances`.
* If the frame p99 does not hold, the reviewer's advice (WP5.6a): lower `max_particles` in `assets/config/effects.ron`
  to 2048 before touching `max_px` (a smaller `max_px` makes the close-up explosions smaller than the tank they come from).

## The checksums, and why M0c is comparable

The simulation's checksum after every 100th tick is in `sim.checksums`. The headless twin of the benchmark scene
(`crates/sf_game/tests/bench_scene.rs`, no window, no GPU) pins them, so a run on any machine must show the same list for
the ticks it reached:

* **`sf_demo --bench --fight`** (M0c): the list `FIGHT` of that test, ending `0x568d257d092daa7b` at tick 1500;
* **`sf_demo --bench`** (the plain benchmark, as the game builds it today): the list `PLAIN`, ending
  `0x8e57352fc4bdec7b` at tick 1500.

**M0's own list is not the plain benchmark's list any more.** M0 ended at `0x9e196a6ced2ef024` at tick 1500. Since WP7.1
(27-10-2026) the world holds the ore of the map and the ore is in the checksum, so the plain benchmark of today ends at
`0x8e57352fc4bdec7b`. Measured (`the_plain_benchmark_without_ore_is_the_world_m0_recorded`): the same world built
**without the ore** gives all 15 of M0's checkpoints again, `0x31dd7bed928a9fbd` at tick 100 up to `0x9e196a6ced2ef024`
at tick 1500. So the simulation of the crowd is what M0 measured; only the checksum covers more. A run of today's
`sf_demo --bench` will not reproduce M0's numbers in `sim.checksums`, and that is expected. The timings are what is compared.

How many ticks a flight reaches depends on the clock (a 75 s flight is about 1,500 ticks); compare the checksums at equal
ticks, as above.

**M0b and M0c: compare the timings, not the checksums.** An M0b run (plain `sf_demo --bench`) on today's main ends at
`PLAIN`'s value (`0x8e57352fc4bdec7b` at the time of writing), **not** at M0's `0x9e196a6ced2ef024`; that is expected.
M0b and M0c are compared with M0 on `frame_ms`, `gpu_ms` and `sim.tick_ms`; the checksums only show that the run played
the pinned world.

**Re-pinning.** Any WP that moves the world's checksum (for example WP7.2, #78) re-pins `PLAIN` and `FIGHT` in
`crates/sf_game/tests/bench_scene.rs` in its own PR, with a measured cause, and updates the end values quoted above in the
same change. The test `the_readme_quotes_the_pinned_end_values` fails if this README and the constants disagree.

## The skirmish (`assets/config/skirmish.ron`, placed by `crates/sf_game/src/skirmish.rs`)

Fixed, no randomness, every number in the RON file (checked at load, pinned by `sf_data::skirmish` tests): three sites, one under each phase of the flight that is to show a fight (the map centre for
`close_up`, the place of `mid_hold`, the middle of the `low_pass`), and at each site every 100 ticks from tick 0 to 1400 a wave
of 8 units per side (3 riflemen, 2 rocket troopers, a heavy tank, 2 light tanks), face to face at 3.6 cells, inside the
reach of the weapons. Nobody gets an order: the units pick their own targets and fire back (WP5.4), so the fight goes on all
flight and a wave that has died is replaced. The units are made by `SpawnUnit` commands scheduled on the world through
`playfield::with_combat`; nothing writes to the world directly. The wanderers have no unit definition and take no part in it.
In the headless twin the fight makes between 143 and 536 shots and 6 to 44 deaths per hundred ticks, up to 32 events in one
tick (`bench_scene.rs` asserts that every hundred ticks has shots, hits and deaths).

## Without the laptop

`sf_demo --bench --fight --seconds 45 --windowed` runs on any machine with a Vulkan, DX12 or software adapter (lavapipe in
the container) and writes the same JSON; the frame times of a software adapter say nothing about the GTX 1650
(`criterion.caveats` says so), but the `effects` block, the checksums and the phases are real, and
`sf_demo --fight --shots <dir> --shots-tick 330` writes the three screenshots of the fight (`docs/screenshots/wp5.6b/`).
