# Laptop benchmarks (M0, M0b, M0c)

The fps requirement (p99 frame time <= 16.7 ms with 5,000 units in view, PLAN.md section 2.4) cannot be measured in the
container or in CI (no real GPU, a shared CPU), so Tijmen runs the benchmark of `sf_demo` on the reference laptop
(GTX 1650, 1920 x 1080). The three runs use the **same camera flight** (about 75 s, nine segments, `sf_game::bench`)
over the **same 5,000 wandering units**; what is added between them is what makes them different:

| Run | Command | What is in the scene | Result file |
|---|---|---|---|
| M0 (22-10-2026) | `sf_demo --bench` | the crowd, the test pack's models, nothing else | `m0-2026-10-22.json` (p99 3.895 ms) |
| M0b (after WP2.4) | `sf_demo --bench` | the same, with the terrain materials and the cell grid | (not in this folder) |
| **M0c (after WP5.6b)** | `sf_demo --bench --fight` | the same crowd and path, plus the **armed skirmish**: shots, hits, deaths, wrecks, smoke and tracers | the files Tijmen runs below |

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

## The skirmish (`crates/sf_game/src/skirmish.rs`)

Fixed, no randomness: three sites, one under each phase of the flight that is to show a fight (the map centre for
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
