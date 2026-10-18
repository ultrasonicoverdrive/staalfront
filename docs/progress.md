# Progress

One entry per phase, written when the phase ends (PLAN.md §12.2 step 5): a short
report and a pointer to the playable build.

## Phase 0 — Foundation

Status: done.

- WP0.1 Workspace, CI, lint: done (PR #2, 8267a83)
- WP0.2 `sf_math`: done (PR #5, daccd6a)
- WP0.3 `docs/`: done (PR #3, 995da1c)

Done when (PLAN.md §3): CI green on two OSes; `sf_math` bit-identical on Linux and Windows.

Report: the workspace with all crates builds, and CI (fmt, clippy, tests, the
determinism lint) runs on Linux and Windows for every merged PR. `sf_math` ships
fixed-point `Fx` (Q47.16), `Vec2`, 256 facings and a PCG32 generator; its golden
hashes (`crates/sf_math/tests/golden.rs`) pass with identical values on Linux and
Windows, in debug and in release. No playable build yet (phase 0 has no gameplay).

## Phase 1 — Engine

Status: all work packages done; M0 waits for the benchmark run on the laptop.

- WP1.1 `sf_engine` main loop, input and frame timing: done (PR #4, aa5cf7b)
- WP1.2 `sf_sim` core (world, commands, checksums, wander): done (PR #9, 4df4759)
- WP1.3 `sf_headless` (scenario runner, checksum log, Linux/Windows comparison in CI): done (PR #12, 305a9e3)
- WP1.4 `sf_render` basics: done (PR #6, 0ae43bc)
- WP1.5 camera, log zoom toward the cursor: done (PR #8, 4e7b88a)
- WP1.6 instancing (placeholder meshes, interpolation, tile culling): done (PR #11, ab98531)
- WP1.7 terrain (map format, chunk meshes, material texture array): done (PR #10, e2f920c)
- WP1.13 `sf_audio`: done (PR #7, afad8d2)
- WP1.8 terrain LOD, seams, culling, ray-terrain picking: done (PR #15, 8f64071)
- WP1.9 model LOD and strategic icons: done (PR #16, d3c7f99)
- WP1.11 egui debug panels: done (PR #13, 76ea42c)
- WP1.12 `sf_ui` (quads, nine-slice, text, buttons, anchors, minimap): done (PR #14, 58d3eb9)
- WP1.10 glTF models (loader, axes, part hierarchy, per-part merge, instancing hook): done (PR #22, 07b4cb7)
- WP1.14 `sf_demo` (engine demo, 1 km map at B12 scale, `--bench` without vsync): done (PR #30)

Done when: milestone M0 (engine demo), the list under "Done when" in PLAN.md §3, phase 1.

M0 status:
- Headless 10,000 ticks with 5,000 wanderers, checksum equal on Linux and
  Windows: met (CI `headless-compare`, since PR #12). Unchanged by WP1.14: the
  B12 scale change lives entirely in the presentation, `sf_sim` was not touched
  and `wander_5000.expected` and the golden checksums are identical.
- Smooth zooming to the cursor: met, judged by Tijmen on the laptop on
  2026-10-15 (main at 8f64071). Judged before the zoom-out hitches (below) were
  looked at; to be re-judged with the demo, together with the benchmark.
- CI green on two OSes including screenshot tests: met (PR #30 adds the demo
  smoke test with screenshots on lavapipe and WARP).
- egui panels show frame time, draw calls and checksum live: met (F1, WP1.11).
- p99 frame time <= 16.7 ms with 5,000 units in view over the whole camera
  flight: **pending Tijmen's laptop run** of `cargo run --release -p sf_demo --
  --bench` (README, "Benchmark (M0)"). The benchmark exists, runs without vsync
  in real fullscreen at 1920 x 1080 and writes `bench.json`; on lavapipe it only
  proves the path works (about 70 ms per frame, software rendering, no meaning
  for the GTX 1650). First laptop run before this WP (2026-10-15, `sf_game
  --units 5000 --auto-zoom`, DX12, vsync): steady 60 fps, p99 17.4-18.2 ms
  that was vsync jitter, two hitches of 34-40 ms when zooming out. That laptop
  has a GTX 1650; Tijmen made it the reference card (B10, 2026-10-15).

Report: _see "Phase 1 report" below._

## Phase 2 — Playfield

Status: in progress.

- WP2.1 Terrain types per cell (`sf_data` table and map layer, generator, `sf_sim`
  terrain and occupancy, `Place`, `terrain_256` scenario): done (PR #39)
- WP2.2 Materials per terrain type: open
- WP2.3 Minimap shows terrain types: open
- WP2.4 Cell grid overlay by terrain type and occupancy: open

Done when (PLAN.md §3): a 256 x 256 cell playfield loads in sim and presentation;
headless test checks terrain types and occupancy per cell. The headless half is met
(`terrain_256`, `tools/sf_headless/tests/terrain.rs`); the presentation half waits for
WP2.2 to WP2.4.

WP2.1 notes. The six terrain types are RON data (`assets/config/terrain_types.ron`),
a map stores them as `terrain.png` (one pixel per 4 m cell), the generator emits water,
rock, ore, a road and bridges, and the sim keeps a type and an occupant per cell. The
terrain and occupancy are in the checksum only when present, so `wander_5000`,
`command_mix` and the sf_sim goldens are unchanged. The rules and the placeholder flags
are in `docs/rules/terrain.md`.

## Phase 3 — Units, selection and commands

Status: in progress.

- WP3.1 Unit definitions (`sf_data::units`: movement classes, armor and damage types,
  weapons, 43 roles of `content/units.md` with their design-target stats, validation
  with file and line, conversion to fixed point for the sim): done
- WP3.2 Units and structures spawn in the sim from the definitions (`sf_sim::UnitTable`,
  `Command::SpawnUnit` and `Command::PlaceStructure`, footprints in the occupancy grid,
  hit points and per-unit speed), the `units_spawn` scenario, a data-driven look per role
  in `sf_game` (`roster.rs`) and the `sf_game` sandbox: done
- WP3.3 Selection (`sf_game::selection`: click, drag rectangle, shift add and toggle,
  double-click for the same kind in view, control groups 1 to 9, inspecting an enemy, the dead
  leaving by the generation check; `Instancer::pick_rect` and `mark_selected` in `sf_render`;
  rings, selected icons, health bars and the drag rectangle on a new `sf_ui` world layer, one draw
  call), `docs/rules/selection.md`: done
- WP3.4 Commands as state (`sf_sim::orders`: idle, hold position, move, attack, attack-move,
  guard, stop, scatter, a bounded queue per unit, per-player command sequence numbers), the `orders`
  scenario, right-click / shift / S / X in `sf_game`, `docs/rules/orders.md`: done
- WP3.6 Icons per unit type (`sf_render::icon_atlas`: a texture array of signed distance fields
  made from `assets/icons/*.png`, uploaded once, one draw call as before; the roster gives each
  role its icon, the base shape by category as the fallback; the placeholder set of 41 pictures
  from `tools/sf_icon_gen`; the quad margin and the outline widths now scale with the UI scale),
  `docs/rules/icons.md`: done
- WP3.5 Cursor context (`sf_game::cursor`: `cursor_context` is one pure function from the
  selection, the hovered entity and ground point, the held modifiers and the terrain flags to a
  `CursorKind`; a right-click gives exactly the order of that cursor through `order_for`, which
  makes the right-click attack, guard, attack-move (new held action `OrderAttackMove`, F) or move,
  and refuses ground no selected unit can enter; 24 software cursors drawn by `sf_ui`
  (`CursorSet`, `Ui::draw_cursor`, the atlas read as sRGB colour), placeholders generated by
  `sf_icon_gen --cursors` with a dark outline and hotspots in `cursors.ron`; the eight scroll
  cursors from `sf_engine::Edges`; the system cursor hidden over the game view only; capture,
  repair, enter, deploy, harvest, infiltrate, sell and place exist as pictures and enum
  variants and never order), `docs/rules/cursor.md`: done

WP3.1 notes. The data is `assets/config/unit_classes.ron`, `weapon_defs.ron` and
`unit_defs.ron`; every record has a permanent `kind` number (order in the file does not
matter, removed records leave tombstones), so numbers are never reused. Tests compare every role with the stats, capacities and tech-tree factions of
`content/units.md`. The simulation does not use the definitions yet and its checksums
are unchanged. Fields, units of measure and the placeholders: `docs/rules/units.md`.

WP3.2 notes. The simulation takes its own `UnitStats` (hit points, movement class, speed
per tick, turn rate, size, footprint) in a `UnitTable` that the game and the headless
runner build with `sf_bridge` from `sf_data` (the same way the terrain is handed over;
`sf_sim` still does not depend on `sf_data`). `units_spawn` is pinned against a frozen
copy of the definitions; the sandbox places every role of the shipped file by faction. Two new commands make units and structures; the old `Spawn` and
`Place` stay as the placeholders they were, so `wander_5000`, `command_mix`, `terrain_256`
and the sf_sim goldens are byte-identical. A mobile unit occupies no cell; a structure
occupies its whole footprint, all or nothing. `sf_game` starts in a sandbox (a few units and
structures of each side, `--crowd` brings the old 2,000 wanderers back); `sf_demo` and its
benchmark are the crowd scene, untouched. Rules: `docs/rules/units.md`.

WP3.4 notes. A unit has a current order and a ring of eight queued ones in its own
columns; `Command::Order { ids, order, queued }` and `Command::Stop { ids }` set them, a new
behaviour `Ordered` carries them out with the same steering step as the wanderers (no
pathfinding, no avoidance: those are WP4.2 and WP4.4). The order holds the destination; the
point the unit steers at is a separate waypoint that phase 4 replaces. Attack, attack-move
and guard are state and movement only (approach to weapon reach, follow a moving target, idle
or the next queued order when the target dies); acquiring targets and firing are phase 5, with
the hook `orders::would_acquire_targets_here` marking the place. The old `MoveTo` is
unchanged, so `wander_5000`, `command_mix`, `terrain_256` and `units_spawn` are byte-identical:
the new state joins the checksum only while some unit has an order. Follow-up 4 is done:
commands run by `(tick, player, player sequence)`, and `World::schedule_sequenced` takes the
number the sender gave (the lockstep packet of WP14.1 only has to carry it). The `orders` scenario is pinned on Linux and Windows. In `sf_game`,
right-click moves, shift queues, C stops, X scatters, H holds position, all on the selection
of WP3.3; all are actions in `input.ron`. Rules: `docs/rules/orders.md`.

WP3.5 notes. No sim code changed and no golden moved. The decision table is in
`docs/rules/cursor.md`; open decisions made there: guard on a friendly unit needs no modifier,
a neutral unit is not attacked by a right-click, a click onto impassable ground or off the map is
refused (it used to be sent or clamped), attack-move and queue are held modifiers. The bench
scene and the engine demo show no game cursor and are unchanged. Not done on purpose: the
capture, repair, enter, deploy, harvest, infiltrate, sell and place behaviours (phases 6 to 10),
a forced-move modifier, and the terrain costs of WP4.1 (until then `Passability` reads the flags
of WP2.1 by class name).

## Phase 1 report (draft, WP1.14)

Phase 1 delivers the engine. `sf_engine` runs the window and a fixed 20 Hz
tick; `sf_render` draws a terrain in chunks with level of detail, seams and
culling, instanced units with model levels of detail, glTF models with parts
and strategic icons in player colour, picking and a log-scale camera that zooms
toward the cursor; `sf_ui` draws the HUD and the minimap; `sf_audio` plays
sounds (and, optionally, the player's own original Red Alert sounds); the F1
egui panels show frame time, draw calls, instancing and the sim checksum; the
deterministic `sf_sim` and `sf_headless` give identical checksums on Linux and
Windows.

The engine demo (`tools/sf_demo`, `cargo run --release -p sf_demo`) shows it all
on a generated 1,024 m map (256 x 256 cells of 4 m, decision B12) with 5,000
wandering units (cube, cylinder, cone and the vehicles of the glTF test pack,
four player colours) and the pack's buildings standing in a base at the centre;
the map has a minimap and a sound on clicks. B12 is in: one constant
(`sf_math::CELL_METRES`, 4) converts the sim's cells to the metres of the
terrain, camera, instancer, minimap and sound, applied at the sim-to-render
boundary; the terrain has four samples per cell side. `--bench` flies a fixed
75 s camera path and writes frame times (p50/p95/p99/max), CPU and GPU frame
time, units in view, draw calls and triangles per phase, the slowest frames and
the sim checksums to `bench.json`.

M0 criteria: headless checksum met, CI on two OSes with screenshots met, panels
met, smooth zoom judged on 2026-10-15, **p99 <= 16.7 ms on the GTX 1650: pending
Tijmen's laptop run.** Phase 2 starts when that number is in and the zoom has
been looked at again.

## Open follow-ups

Findings from the phase 1 work packages that are not fixed yet. Each is meant for
the work package named in it (or the first one that touches the code); when one is
fixed, delete the entry in the same PR.

5. **Release clippy runs on Linux only in CI.** The step
   `cargo clippy --workspace --release --all-targets -- -D warnings` is guarded with
   `if: runner.os == 'Linux'`, so a lint that appears only in release mode on
   Windows (for example behind `cfg(windows)`) would not be caught. Run it on both
   systems once the CI time allows.
7. **The minimap is baked once.** It does not follow terrain changes. Re-bake it when
   the terrain changes (phase 2: terrain types in WP2.1 to WP2.3, later editing).
   WP2.1 put the terrain types into the map data (`Map::terrain`) and the sim; the
   bake does not read them yet, that is WP2.3.
8. **The minimap bake shows relief weakly.** Its slope shading is faint next to the
   terrain in the main view. Use a stronger relief shading in the bake (WP1.12
   follow-up, together with WP2.3).
9. **Release packaging must ship the egui font licence texts.** `assets/LICENSES.md`
   lists the default egui fonts (OFL-1.1, Ubuntu-font-1.0); the texts themselves are
   in the `epaint_default_fonts` crate and have to be part of the third-party notice
   of any release build (installer, phase 16).
10. **The `draw_indexed` source test matches exact helper text.**
    `renderer.rs` pins the single `draw_indexed` call site by the text of
    `CountedPass::draw`. If that starts getting in the way of a harmless refactor,
    loosen it to match `.draw_indexed(` only.
11. **The determinism lint does not scan `tools/sf_headless`.**
    `scripts/check-sim-determinism.sh` covers `sf_sim` and `sf_math`; the headless
    runner is outside it. That is fine while it only calls `sf_sim`; extend the lint
    when it grows logic of its own.
14. **Zoom-out hitches of 34-40 ms** (laptop run, 2026-10-15). Cause found
    in WP1.14 as far as software rendering shows it: the per-mesh instance
    buffers were created on the first frame their list was not empty and
    regrown on the way (66 creations during one slow zoom out, up to 2 in a
    frame; the new `buffer_allocs` per frame in `bench.json` shows it), and are
    now allocated once at load (`Renderer::reserve_units`; `--no-reserve` gives
    the old behaviour). On lavapipe a creation costs under a millisecond, so
    whether this explains 34-40 ms on DX12 is **not proven**: delete this entry
    when the laptop's `bench.json` shows no hitch over 25 ms; if it does, the
    `hitches.worst` list names the phase and the part of the frame.
15. **Vulkan hangs on the test laptop.** Adapter creation hung with Vulkan
    (NVIDIA GTX 1650, Windows); the game window now uses DX12 on Windows.
    `WGPU_BACKEND` overrides it, `SF_GPU_TRACE=1` prints each GPU startup step.
19. **Exclusive fullscreen is untested.** `--bench` asks for 1920 x 1080 and
    uses borderless when the monitor already runs it, an exclusive video mode
    when the monitor is bigger, borderless at the native size when there is no
    such mode (`ScreenMode::Fixed`). Only the first and last paths ran (Xvfb
    has no window manager and grants no fullscreen at all); the exclusive path
    with wgpu on DX12 has not run anywhere. `bench.json` records the mode used
    (`display.screen_mode`, `display.note`).
20. **The demo map is gentle.** `generate` keeps the relief at most 6 m high at
    every map size (hills of 5 to 10 m radius, more of them on a bigger map),
    so at the whole-map zoom of the 1,024 m map the relief shows only as
    shading. Add broad hills for big maps when the look of the map matters
    (phase 2, WP2.3).
