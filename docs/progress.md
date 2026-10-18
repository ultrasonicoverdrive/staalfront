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

Status: in progress. WP2.1 is done (PR #39). WP2.2 to WP2.4 (what is drawn from the terrain
types) wait for the M0 laptop benchmark: the Phase 1 report starts phase 2 proper when
the p99 number is in. Phase 3 and the start of phase 4 were built on WP2.1 meanwhile.

- WP2.1 Terrain types per cell (`sf_data` table and map layer, generator, `sf_sim`
  terrain and occupancy, `Place`, `terrain_256` scenario): done (PR #39, 6d28bff)
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

Status: done (2026-10-18). All six work packages are merged; the report is below the notes.

- WP3.1 Unit definitions (`sf_data::units`: movement classes, armor and damage types,
  weapons, 43 roles of `content/units.md` with their design-target stats, validation
  with file and line, conversion to fixed point for the sim): done (PR #40, c459634)
- WP3.2 Units and structures spawn in the sim from the definitions (`sf_sim::UnitTable`,
  `Command::SpawnUnit` and `Command::PlaceStructure`, footprints in the occupancy grid,
  hit points and per-unit speed), the `units_spawn` scenario, a data-driven look per role
  in `sf_game` (`roster.rs`) and the `sf_game` sandbox: done (PR #41, 55e12cd)
- WP3.3 Selection (`sf_game::selection`: click, drag rectangle, shift add and toggle,
  double-click for the same kind in view, control groups 1 to 9, inspecting an enemy, the dead
  leaving by the generation check; `Instancer::pick_rect` and `mark_selected` in `sf_render`;
  rings, selected icons, health bars and the drag rectangle on a new `sf_ui` world layer, one draw
  call), `docs/rules/selection.md`: done (PR #44, e788d34)
- WP3.4 Commands as state (`sf_sim::orders`: idle, hold position, move, attack, attack-move,
  guard, stop, scatter, a bounded queue per unit, per-player command sequence numbers), the `orders`
  scenario, right-click / shift / C / X / H in `sf_game`, `docs/rules/orders.md`: done (PR #43, 0b65302)
- WP3.6 Icons per unit type (`sf_render::icon_atlas`: a texture array of signed distance fields
  made from `assets/icons/*.png`, uploaded once, one draw call as before; the roster gives each
  role its icon, the base shape by category as the fallback; the placeholder set of 41 pictures
  from `tools/sf_icon_gen`; the quad margin and the outline widths now scale with the UI scale),
  `docs/rules/icons.md`: done (PR #45, 96f6dc4)
- WP3.5 Cursor context (`sf_game::cursor`: `cursor_context` is one pure function from the
  selection, the hovered entity and ground point, the held modifiers and the terrain flags to a
  `CursorKind`; a right-click gives exactly the order of that cursor through `order_for`, which
  makes the right-click attack, guard, attack-move (new held action `OrderAttackMove`, F) or move,
  and refuses ground no selected unit can enter; 24 software cursors drawn by `sf_ui`
  (`CursorSet`, `Ui::draw_cursor`, the atlas read as sRGB colour), placeholders generated by
  `sf_icon_gen --cursors` with a dark outline and hotspots in `cursors.ron`; the eight scroll
  cursors from `sf_engine::Edges`; the system cursor hidden over the game view only; capture,
  repair, enter, deploy, harvest, infiltrate, sell and place exist as pictures and enum
  variants and never order), `docs/rules/cursor.md`: done (PR #46, 8bfa7fa)

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
of WP2.1 by class name; WP4.1 removed `Passability` and the cursor asks `World::passable`).

### Phase 3 report

Not numbered work packages, merged in the same days:
- Art set v3 (Tijmen's third delivery): the 30 glTF models in `assets/set_v3/` and the
  checker `tools/sf_asset_check` (PR #42, 44c7ef1). The checker reports 2 files pass, 21
  with warnings only, 7 fail (the sniper's team colour is 19 % of the top view against
  20-40 %, two rocks lie below the ground, four wrecks lack nodes in LOD2); the list is
  in the PR description, for Tijmen. No model was changed.
- Wiring of the set (PR #48, 5509225): the glTF loader now treats a plain mesh `turret`
  or `barrel` as a part, so `hull -> turret -> barrel` can tilt its barrel; `unit_defs.ron`
  takes an optional `models` per faction; ten infantry roles (14 models) show their set
  v3 model, Meridian and Veyran each with their own. Vehicles, buildings and decor are
  not wired.

Done when (PLAN.md §3): you select and move units at any zoom; commands are sim commands
and thus reproducible in headless. **Met.**
- Select at any zoom: click, rectangle and double-click pick through the model box close
  up and through the icon quad zoomed out (PR #44 tests the real picking at both zooms).
- Move: right-click, shift-queue, stop, hold and scatter only schedule `Command::Order` or
  `Command::Stop` for the next tick. The `orders` scenario covers every order, the queue,
  the refusals and the sequence numbers; it is pinned (`0xbdcd8bcd753b5829`) and runs in
  the Linux/Windows `headless` job. `wander_5000`, `command_mix`, `terrain_256` never
  moved in phase 3; `units_spawn` moved once, in WP4.1, on purpose.
- What the check was: tests, the scenarios on Linux and Windows, and the game on software
  rasterizers (lavapipe, WARP) with injected mouse and key events under Xvfb. There is no
  record yet of Tijmen playing it on the laptop.

Playable state. `cargo run --release -p sf_game` opens a sandbox: a base of structures and
units for Meridian and Veyran and neutral objects on a generated 256 x 256 cell map, every
role of the unit table placed by faction. Nothing moves until it is ordered. At any zoom
(models close up, strategic icons far out):
- **Look**: wheel zooms toward the cursor; W A S D, arrows, the screen edge or middle-mouse
  drag pan. F1 shows the debug panels (the hovered unit's role, order and hit points),
  F11 fullscreen, Esc quits.
- **Select**: click; drag a rectangle (mobile units); shift+click toggles; double-click takes
  the same kind in view; ctrl+1..9 assigns a group, 1..9 recalls it (twice centres the
  camera); a click on an enemy or neutral unit only inspects it.
- **Order**: right-click does what the cursor shows (move, attack, guard, or nothing on
  ground the units cannot enter); shift+right-click queues (up to 8); F+right-click is
  attack-move; C stops, X scatters, H holds position. The cursor changes with the
  context (16 placeholder cursors and 8 scroll arrows at the screen edges).
- Options: `--crowd` (the old 2,000 wanderers), `--units N`, `--models none`,
  `--map-cells N`, `--shots <dir>` (screenshots; `--hover x,y` draws the cursor there).
  Every key is an action in `assets/config/input.ron`.

Known gaps (owner in brackets):
- No pathfinding or avoidance: a unit drives a straight line, through other units, and stops
  at the edge of ground its class cannot enter, order kept (WP4.2 to WP4.4, WP4.7).
- No combat: attack and attack-move walk to weapon reach; nothing acquires a target, fires
  or takes damage; only the first weapon's range is read (phase 5, WP5.1).
- No ownership check in the sim and no lockstep packet (WP14.1).
- The terrain types are not drawn from the type layer: no per-type material, no minimap
  colours, no cell overlay (WP2.2 to WP2.4, after the M0 laptop run).
- No sidebar, build menu, economy or power; the HUD is a top bar with the selected count
  (phases 6, 7, 11).
- No vessels in the sandbox (WP4.1 follow-up: they need water to stand on).
- All icons (41) and cursors (24) are generated placeholders; vehicles and buildings still
  show test-pack models or primitives; set v3 has no `heavy_gun_team` model.
- Nothing of phase 3 has been measured on the GTX 1650 (the M0 benchmark is still the only
  laptop run to do). Icon drawing costs a few percent more frame time on lavapipe
  (+3 % frame, +4 % GPU, within noise); on the card it is estimated far below 0.1 ms.

## Phase 4 — Movement and routes

Status: in progress.

- WP4.1 Movement classes with terrain costs (`sf_data::movement` and the `terrain_costs` list of
  every class in `unit_classes.ron`: for each of the six terrain types impassable or a speed in
  thousandths; `sf_sim::MovementTable` with `World::cost` and `World::passable`; a unit moves at its
  speed times the cost of the cell it stands in and stops at the edge of a cell it cannot enter,
  order kept; vessels pass under a bridge, land crosses it; the cursor's no-go reads the sim's table
  and `Passability` is gone; the `terrain_costs` scenario on a new hand-drawn map),
  `docs/rules/movement.md`: done
- WP4.2 Hierarchical A* (sectors and portals) for single units (`sf_sim::path`: sectors of 16 by 16
  cells, portals on their borders, an abstract graph per terrain-bound movement class built when the
  world is made and rebuilt one sector at a time by the tick when a structure is
  placed or removed; abstract A*, lazy refinement inside the sectors, line-of-sight string pulling,
  a fixed-size ring of waypoints per unit; requests served in the order of the entity ids inside a
  node budget per tick; an unreachable goal ends in the nearest reachable cell and says so;
  `Order::Move` and the other walking orders now follow routes; the `pathing` and `pathing_256`
  scenarios and the load test), `docs/rules/pathfinding.md`: done
- WP4.3 to WP4.7: open

WP4.1 notes. The three `passable` flags of `terrain_types.ron` are replaced by the costs of the
movement classes; the types keep `buildable` and `harvestable`. In the sim `TerrainFlags` has a
derived `ground` summary (all three ground classes can enter) in the place of the three bits the
terrain digest used to hash, so `terrain_256` and every other pinned map did not change; the
classes that count are marked `ground: true` in the data, and the headless runner derives the bits
from the costs of the scenario's own definitions (a frozen copy or, without one, the shipped file),
so editing the shipped costs moves only scenarios on the shipped definitions. The movement table's
own digest joins the checksum while a unit with a movement class is on a map with terrain.
**Terrain costs are a new rule and `units_spawn` moved once, on purpose** (`0x0576f4e3b9fcff04` to
`0x6c369f42be150d6e`): its two legacy `MoveTo` walkers cross a rock patch to a rock cell and now
stop at the rock. Frozen definitions protect against balance tweaks, not against new rules, so the
frozen copies carry the real costs as shipped at WP4.1 and the scenario was re-pinned. `orders`,
`wander_5000`, `command_mix` and `terrain_256` are unchanged. A unit spawned on ground its class
cannot enter is now refused. Slope (uphill slowing) waits, with reasons, for WP4.3 (`movement.md`, "Slope: later"; WP4.2 did not take it). Not
done on purpose: pathfinding, collision, replanning a blocked route (a blocked move is a unit
standing at the edge forever), reading a role's `water_speed` (the hover class's cost says it), and
vessels in the `sf_game` sandbox.

WP4.2 notes. A unit that walks an order and is bound by terrain asks for a route on the first tick
and stands still until the service has made it (one tick at least); aircraft (a class that no
terrain stops), the legacy `MoveTo` and the wanderers still walk the straight line, so
`wander_5000`, `command_mix`, `terrain_256`, `units_spawn` and `orders` did not move. **`terrain_costs`
moved once, on purpose** (`0x8c43b875a877488c` to `0xe98b2eed90616d3f`): its `Move` orders now find
their way. The units sent along the river lane take the bridge and come back down instead of
stopping at the bank (the destination can be reached), everybody goes round the two-cell rock wall,
the gunboat sent onto the bank ends in the middle of the water cell nearest to it (marked
unreachable) instead of at the edge of it, and the lanes in the open each take one tick longer (the
request). The scenario file's commands and its frozen definitions are the same; the asserts of
`tools/sf_headless/tests/terrain_costs.rs` and of `crates/sf_sim/tests/movement.rs` that described
"stops at the edge" were rewritten for the new rule, the legacy `MoveTo` keeps its test. Choices:
one portal on a short run, three on a run of 6 to 11 and four from 12 on (measured against the
best any-angle route: the tail of the distribution, not the median, decided it); a leg that crosses a border is searched over both sectors so the route
crosses where it suits it; a thick line of sight of a quarter cell and a sector long; the budget is in
nodes (cells expanded or looked at), 4,096 per tick by default, in `assets/config/sim.ron`; a structure on a unit's own cell with no
free neighbour leaves it with nothing to reach (WP4.4's stuck unit). Measured: about 1,100 random routes
against the best any-angle route by the game's own line of sight, median 1.006, 99th percentile
1.05, worst 1.075; `pathing_256`, 200 requests served after 96 ticks. Not done on purpose: flowfields,
collision, replanning when a route is blocked or a target vanishes (the graph is kept right; a route
is only asked for again when a change touches it), a cache for unreachable goals, uphill slowing.

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
    when it grows logic of its own. Add `sf_bridge` at the same time (PR #41 review): it
    only copies fields at world creation today.
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
21. **Plain `--shots` still highlights the unit under the X pointer.** The game cursor is
    drawn only with `--hover` since the close-out of phase 3, but the hover highlight and the
    F1 hover line still follow the real pointer, so a screenshot taken with a mouse over a
    unit differs. Pass no cursor to `build_units` while `--shots` has no `--hover` (it also
    changes the `sf_demo` shots, which share the runner). Owner: WP4.5, the next WP that
    changes how `sf_game` looks.
22. **A right-click on a unit inside the 4 px edge band gives no order** (PR #46; the band
    wins over everything and the cursor shows the scroll arrow, documented in `cursor.md`).
    Owner: WP11.1, when the HUD and the edge scrolling are revisited.
23. **Icon set.** Confirm the cost of the SDF icon quad on the GTX 1650 (PR #45; owner: the
    M0 laptop run, `bench.json` overview phases). The weakest pairs of the placeholders
    (power_plant meets the 6-pixel blob rule exactly; barracks / power_plant differ by 17 of
    196 pixels) are for the artist's set: owner, the delivery that replaces the placeholders.
24. **Art set v3.** Seven of 30 models fail `sf_asset_check` (list in PR #42): owner Tijmen,
    in the next delivery or by deciding on a rule exception (rocks sunk on purpose). The
    delivery's licence text ("eigen werk, vrij te gebruiken in Staalfront") is not an SPDX
    licence: confirm a precise one before any release (phase 16).
25. **Models not wired.** Decor (trees, rocks, ore stages) and wrecks are not unit roles and
    have no place in the map yet (decor placement WP2.2 / WP2.3, wrecks WP5.5); vehicles and
    buildings have no set v3 model (the next art delivery, then `models` in `unit_defs.ron`).
26. **Decisions of WP3.3 to confirm** (PR #44): a double-click on a structure acts as a single
    click; a shift-click on an enemy is ignored; ctrl+number with nothing selected clears the
    group. Owner: Tijmen; change in `selection.rs` and `selection.md` together.
