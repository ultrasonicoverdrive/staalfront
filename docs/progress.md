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
- WP1.15 The simulation on its own thread (an engine follow-up, not a numbered WP in PLAN.md; see
  "WP1.15 notes" below): done (PR #53)

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
  --bench` (README, "Benchmark (M0)"), which has to be taken on the code of WP1.15
  or later: the sim runs on its own thread there and is no longer in the frame,
  so a number from before it is not comparable. The benchmark exists, runs without vsync
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

Status: 4.1–4.5 + 4.4b done, 4.6/4.7 open. The "done when" of PLAN.md (200 units across a bridge: all
arrive, none stuck, at most 1 % beyond 1.3 times the packing radius, overlap within a quarter of a cell
on the way and a sixteenth at the end) holds in `bridge_200` for every wheeled turning radius the review
swept, so it no longer depends on the luck of the crowd. Phase 4 counts as done when WP4.6 (formations)
and WP4.7 (replan) are in as well.

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
  a fixed-size ring of waypoints per unit; requests served going round the entity ids inside a
  node budget per tick; an unreachable goal ends in the nearest reachable cell and says so;
  `Order::Move` and the other walking orders now follow routes; the `pathing` and `pathing_256`
  scenarios and the load test), `docs/rules/pathfinding.md`: done
- WP4.3 Flowfields for groups (`sf_sim::path`: one `Move`/`AttackMove` that sends at least
  `flowfield_threshold` units of a class to one point, 8 as shipped in `sim.ron`, builds one field per
  class instead of a route each: an integration field (Dijkstra from the goal with the routes' costs)
  and a direction per cell with a fixed tie-break, over the sectors of the abstract corridor and
  one ring of margin, built by a resumable job that shares the node budget with the routes; units
  follow it to the farthest cell of the chain they can see and arrive spread over a spiral of
  slots round the goal; a fixed pool of 8 fields with least-recently-used eviction and idle expiry;
  a structure in the region makes a successor while the units go on with the old field where it is
  still valid; the `bridge_200` scenario), `docs/rules/pathfinding.md`: done
- WP4.4 Local avoidance and collisions (`sf_sim::crowd`: a unit made from the unit table has a body, a
  disc of half its `size`, three quarters of the sum of the radii between two infantry-sized ones; after
  the units have taken their steps a crowd pass over a deterministic spatial grid pushes overlapping
  bodies apart in six passes that read one snapshot, so the result does not depend on the order the
  units are visited in; units moving the same way follow, units meeting head-on keep right, a unit that
  has waited detours, an idle friend is shoved aside a little and never into rock or a structure, a unit
  on `HoldPosition` never; a unit that does not get closer for `stuck_ticks` steps to a free neighbouring
  spot and after `stuck_attempts` of those asks for a new route, the hook of WP4.7; the arrival slots of
  WP4.3 are spaced for the bodies, in rings, one class after the other; the `crowd_jam` scenario and a
  2,000-unit performance check; the #51 follow-ups), `docs/rules/collision.md`: done
- WP4.5 Turning (`sf_sim::turning`, `turret`: every movement class has a turning profile in
  `unit_classes.ron`; infantry turn at once, tracked units pivot in place and drive within a
  tolerance of 16 steps, wheeled, hover and water have to move to turn, in an arc of a smallest radius
  carried in thousandths of a step, with a turn on the spot at a share of the turn rate for a target
  inside the turning circle; the shortest way round with the fixed tie at 128; a unit with a
  `turret_rate` has a turret facing of its own that follows `Command::AimTurret` or an attack order's
  victim or the hull, is interpolated for the renderer's `turret` yaw joint and is in a new checksum
  section; pushes of the crowd bypass the turn constraint, driven steps do not; the `turning` scenario,
  a screenshot test of a tank with its turret turned off its hull; the pinned `orders`,
  `terrain_costs`, `pathing`, `pathing_256`, `bridge_200` and `crowd_jam` re-pinned once),
  `docs/rules/turning.md`: done
- WP4.4b Robust group arrival (follow-up of the review of WP4.5; `sf_sim::path::claim`, `crowd`,
  `orders`: a unit of a group takes its arrival slot when it comes near, in the order the units
  arrive, and not by the rank of its id; a unit that is stuck lets go of its slot and takes another;
  a unit held up by the crowd on its destination counts as arrived farther out the longer it has
  pressed; the overlap bound holds by construction (a shove is not a squeeze, a mover that ended a tick
  deep in a body goes back, a last resort pushes apart what is still deeper than 3/16 of a cell), a unit
  that has just stopped is looked at once, and a settled crowd closes up toward its point; the wheeled
  radius is a realistic 1.5 cells (6 m) in the shipped data and the frozen copies; `bridge_200` runs 4,600
  ticks and has one test for each wheeled radius of the review sweep; `bridge_200` and `crowd_jam`
  re-pinned, `orders`, `terrain_costs`, `pathing`, `pathing_256` and `turning` re-pinned for the radius
  alone), `docs/rules/collision.md`, `docs/rules/turning.md`: done
- WP4.6 and WP4.7: open

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
1.05, worst 1.075; `pathing_256`, 200 requests served after 85 ticks. Not done on purpose: flowfields,
collision, replanning when a route is blocked or a target vanishes (the graph is kept right; a route
is only asked for again when a change touches it), a cache for unreachable goals, uphill slowing.

WP4.3 notes. A group order is a `Move` or `AttackMove` to a point given to units that start it now;
per movement class with at least `flowfield_threshold` of them one field is built and its units are
members (not a guard of a point, which walks back to its exact post; not an order queued behind
another; not a goal the class cannot enter, whose units are sent to the nearest reachable cell one by
one as before). **`bridge_200`** (new, pinned `0x5db78c7d785670f0`): 90 riflemen, 60 light tanks,
45 light transports and 5 amphibious transports cross a river with one bridge and a wall with a
gap to a point 60 cells away on one order. Three fields are finished by tick 6, all 200 units are
moving by tick 17 and have arrived by tick 2137 (107 s; the slowest is a rifleman at 0.7 cells a
second), none ends outside 5 cells of the goal (the farthest is 3.95), none is ever on ground its
class cannot enter, and the service spent 24,712 nodes in all, against 266,587 and 448 jobs for the
same crowd routed one by one (every unit moving by tick 59). **Units overlap** at the goal and on
the bridge until collision (WP4.4): the arrival slots are half a cell apart, closer than a vehicle
is long, because 200 units one to a cell do not fit in five cells. **No golden moved.** `pathing`
has a group order (24 transports to one cell) and sets `flowfield_threshold: Some(0)` to stay
the test of the queue of single requests it is pinned on; on a flowfield its checksum would be
`0xd2f0d2dcc39d0f9e` instead of `0x09f819e7bfeb22a2` (`tools/sf_headless/tests/flowfield.rs` runs
that, the 24 arrive by tick 681 with a structure placed across their way). Choices: the region is
the abstract corridor plus one ring of sectors (160 at most), not the map; a unit waits for its own
cell to be integrated and leaves the field for a route of its own when the field cannot serve it;
the checksum covers a field by a running hash of what was integrated, not its arrays. Not done on
purpose: collision and pushing (WP4.4), formations and a shape that keeps (WP4.6), replanning a
target that vanished (WP4.7), a substitute goal for a group sent to a cell its class cannot enter,
queued group orders, uphill slowing.

WP4.4 notes. **`bridge_200` re-pinned** (`0x5db78c7d785670f0` to `0x3cbf8e8c2fe9f78b`, and after the
review of PR #52 to `0xa55348bf0b519c50`, see below): with bodies the
bridge is a real bottleneck; all 200 units arrive by tick 3,526 (176 s, against 107 s when they went
through each other; the crowd is about 170 cells of bodies and crosses a gate three cells wide at the
pace of the slowest class, 170 / (3 x 0.7) = 81 s, and the last crossing was on tick 1,976), nobody is
stuck at the end (91 sidesteps and 6 requests for a new route in all), the worst overlap on any tick of
the run is 0.22 of a cell and there is none beyond a sixteenth at the end. **The "done when" of the
phase was changed** (orchestrator decision of 18-10-2026, PLAN.md): at most 1 % of the units outside 5
cells (20 m) of the goal was written for points; the 110 vehicles of the mix need about 150 cells of
ground and a disc of five cells has 78, so 140 of the 200 end outside it (the farthest 9.55). The
criterion is now at most 1 % beyond 1.3 times the radius of the disc that holds the bodies (7.37
cells, so 9.58), no overlap beyond a quarter of a cell during the run and none beyond 1/16 at the end
(`collision.md`, "Five cells cannot hold 200 bodies"). **Review of PR #52:** the slots of two classes
of one order are now at least the sum of the radii apart (`slots::keep_apart`; the old
`(s1 + s2) / 2 - GAP` was 0.05 of a cell short for infantry beside a tank, which moved the `bridge_200`
checksum); the perf test now makes two dense blocks, a cell apart, meet (the old halves started 138
cells apart and never touched); a group order with the ids listed in another order gives the same
result. `crowd_jam` (new, pinned
`0xd5e9d6e45a14b0ef`): two groups of 18 head-on through a three-cell gap, 8 tanks past a unit on hold in
a five-cell corridor; every order is done by tick 1,598, the holder never moves, one sidestep.
**Re-pinned, with the reason:** `orders` (`0xbdcd8bcd753b5829` to `0x38b6882e777306bb`: its units have
bodies, the ones sent to the same point no longer stand on it together) and `pathing_256`
(`0xa52ecf79cfd7b1ea` to `0x78acbca87df9a5f0`: 200 tanks push each other apart where their ways cross;
they all arrive). **Not moved:** `wander_5000`, `command_mix`, `terrain_256` and `units_spawn` (legacy walks
and wanderers are not bodies) and, with `collision: Some(false)` in their files because they start
units side by side or share goals and assert exact positions, `terrain_costs` and `pathing`. 2,000 units
with bodies, in two dense blocks that run into each other, run at 79 ticks a second in release (the
slowest tick 19.5 ms; 11 in debug). **Choices:** idle-versus-mover pushes
give the idle friend the whole share while it can move and none once it cannot (it is then a wall for
the pass); a waiting unit is "in line" and not stuck behind one that goes the same way; a step the
terrain refuses slides along the obstacle (for walks of an order only, so the legacy stand-at-the-edge
scenarios are as they were); a unit that is blocked near a waypoint of its route takes it as reached; a
unit blocked by arrived friends near its slot, or as near the goal as its slot, has arrived.
**#51 follow-ups fixed:** a stranded flowfield member recovers when the map changes and no longer keeps
its field alive; the slots of the classes of one order are unique and a body apart (rings, later classes
skip earlier slots; WP4.3's counted stretches overlapped when goal spots were blocked); the comment of
`Fields::tick` now says what the code does (the build in flight expires too); the slot spacing is the
width of the widest body. **Not done on purpose:** layers (a vessel under a bridge collides with traffic
on it), turning and animation of a pushed unit (WP4.5), formations (WP4.6), the replan itself (WP4.7;
the request and the mark are here), dynamic slot assignment (the last units arrive about 20 s after the
last crossing, walking through the crowd; done in WP4.4b), crushing.

WP4.5 notes. **Re-pinned once**: `orders`, `terrain_costs`, `pathing`, `pathing_256`, `bridge_200` and
`crowd_jam` (old and new values in `docs/rules/turning.md`); `wander_5000`, `command_mix`,
`terrain_256` and `units_spawn` did not move (the first three have no class; the frozen definitions of
`units_spawn` have no `turning` block and a class that says nothing turns as the walk always did).
**`bridge_200` still meets the criterion of phase 4, with thin margins**: all 200 arrive by tick 3,676
(183 s; 3,526 before: 150 ticks slower), 1 of 200 ends beyond 1.3 times the packing radius (the
allowance is 2; the farthest 9.72 against 9.58), the worst overlap is 0.19 of a cell, nobody is stuck.
The wheeled turning radius (1.25 cells) is a value that passes the test, not a claim about real trucks;
the review's sweep found 1.5 (4 riflemen never arrive), 1.6 (9 never arrive), 1.75 (overlap 0.286) and
2.0 (a hovercraft stuck) failing, from the crowd and arrival logic of WP4.4 (table in `turning.md`). Choices: the drive tolerance of every shipped class is 16 steps, the
margin the route smoothing stands on (a hover class at 32 drove into rock); a unit that has to move to
turn counts a waypoint reached a quarter of a cell short; a pushed unit slides without turning;
a unit with a turret leaves its hull alone when it is in reach of an attack target and the turret
turns. The hover highlight of `--shots` (known issue 21, which names WP4.5 as owner) is not part of
this work package and was not touched. Not done on purpose: reversing, turret limits, the barrel's
elevation (a hook in the renderer), formations (WP4.6), replanning (WP4.7). (The thin margins and the
failing radii of this note are WP4.4b's: below.)

WP4.4b notes. **The criterion no longer depends on the radius.** The cause the review of WP4.5 found
was the arrival logic, not the turning: the n-th id got the n-th slot of its class's ring whenever it
came, a queue mixes the classes, so a slow rifleman that came early stood in the outer ring and walled
it off, the tank behind it pushed through, the crowd spread and was left spread, and idle units that
were pressed together stayed overlapped because nobody was moving them. Now a unit takes a slot when it
comes within six cells of the outermost slot of its class: the first one nearest the goal that no
held slot of any class lies near and that it can walk to straight; a stuck unit lets go and takes
another; a unit pressing on a crowd that stands on its destination (the five hover craft are single
walkers) counts as arrived farther out the longer it has pressed; a unit that has just stopped is
pushed clear if it came to rest against a neighbour; a settled crowd (unit left alone for a second)
closes up toward the point it was sent to, a sixteenth of a cell a tick, straight at it or slid up to
83 degrees away from straight; and the overlap bound holds whatever presses (`COMPRESS`,
`MOVER_OVERLAP`, `HARD_OVERLAP`). The wheeled radius is 1.5 cells. **Tested:** one `bridge_200` test for
each of 1.25, 1.5, 1.6, 1.75 and 2.0 with the radius replaced in the movement table (all pass, last
arrival tick 3,396 to 3,587, farthest 8.1 to 9.5 cells against 9.58, worst overlap 0.19 at the spawn
tick); in probes, when the work was done, also every radius from 1.05 to 3.0 (25 values), hover radii 1
to 4, the turn on the spot at 60 and 1000 thousandths, and four other mixes of the block: all pass.
`crowd_perf`: 66 ticks a second in release (15.2 ms a tick, the slowest 27.4 ms) against 87 (11.5, 19.2)
before. **Re-pinned once:** `bridge_200` (`0x03b3145755a9081e` to `0x831cd1eaff2b1307`) and `crowd_jam`
(`0x3d8fab0ab9a564e6` to `0x4ac3f990f7070f48`) by the arrival logic and the radius; `orders`
(`0x0dcb52425f1adc60` to `0xf8fbe06cc8a51962`), `terrain_costs` (`0xdc9d5c4a65a60508` to
`0xacd34a784a0fd0be`), `pathing` (`0x606eb6feeffa964c` to `0x398d625625dce5c1`), `pathing_256`
(`0x7c4e31d024ed4cf9` to `0x016c2b5ef7c7e055`) and `turning` (`0xb07ba8066dd59884` to
`0xd894752d6d192f8d`) by the radius alone (with 1.25 put back they give their old values);
`wander_5000`, `command_mix`, `terrain_256` and `units_spawn` did not move. **Choices:** the scenario
runs 4,600 ticks and the test limit is tick 4,400 (the last arrival is 3,200 to 4,200 over the radii
and mixes tried: the gap in the wall behind the bridge passes about one unit in ten ticks, and the
criterion is that everybody arrives, not when); a claim is kept after the order ends and let go at the
next order; at most 16 units take a slot in a tick (the cost of a crowd that arrives at once). **Tried
and not kept**, so nobody tries them again: slots beyond the ring for a class whose ring is walled off
(the overflow ended farther out than it saved); a pool of lattice points for all classes taken in
arrival order (an elongated blob toward the entrance, about half the packing density); idle units as
obstacles that a mover goes round first and shoves only after waiting (pile-ups, overlap 0.5); a
smaller shove allowance of 1.5 to 2 cells (movers could not get through; units never arrived and
many ended outside the disc); the settled units walking back to their own slots instead of closing up (their slots
were taken by the units that had been pushed in). **Not done on purpose:** formations (WP4.6, which
will replace the rings), the replan (WP4.7), a better route through the gap of `bridge_200` (the field
leads everybody to the same corner: it is the bottleneck of the scenario and a pathing matter), and
slots for a class below the threshold of 8 for a flowfield (the five hover craft are single walkers
with no slot among the crowd; they rely on the arrival slack that grows with the time they have pressed
on it).

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

## WP1.15 notes: the simulation on its own thread

Why: the review of PR #52 measured 2,000 colliding units at 12.7 ms per tick on average and 22 ms at
worst, stepped inside `fixed_update` on the main thread in the same frame as `render`: a hitch in
every third frame at 60 Hz, against the M0 target (p99 <= 16.7 ms, PLAN.md section 2.4).

What it is (`docs/architecture.md` section 18 has the full picture): `World` lives on a thread
`sf-sim` (`sf_game::sim_thread`), with its own 20 Hz clock. The main thread sends `Command`s over an
mpsc channel, stamped with the tick of the newest snapshot plus one (`INPUT_DELAY_TICKS`; the
single-threaded game stamped the tick about to run, so an order now starts up to 50 ms later), and
reads an immutable `SimSnapshot` (`sf_game::sim_snapshot`) that the sim thread publishes after
each tick through one slot behind a mutex held for a pointer swap: per entity id and generation,
kind, owner, previous and current position and facing, hit points and order, plus the unit table,
the passability of every cell per movement class, the tick, the checksum and how the tick went.
Everything that read `World` reads the snapshot now: selection, picking results, the cursor context,
right-click orders, the selection rings and health bars, the HUD and minimap, the F1 panels. If the sim
falls behind it never takes more than 1 + 3 ticks per 50 ms, restarts its clock when it is 20 ticks
behind (the game slows, no tick is skipped), and the F1 panel says "sim behind"; rendering carries on
with the newest snapshot. The picture is drawn 30 ms behind the sim (`INTERP_DELAY`) so a tick that
takes up to that long does not show. std only, no new dependency; `sf_sim` is not touched.

Tests (`crates/sf_game/tests/sim_thread.rs`, unit tests in the two modules): the shipped scenarios
`orders`, `command_mix`, `units_spawn`, `terrain_256`, `pathing_256` and `crowd_jam` through the sim
thread (commands over the channel, stepped one tick at a time) and through `sf_headless`, equal
checksums after every tick and equal entities in every snapshot; a free-running sim with live
commands replayed from its log in a single-threaded world; a frame loop against ticks of 150 ms
(frames stay within a few ms); bounded catching up; a stall beyond the limit; late commands; the
thread joining in every state. `crowd_perf`'s release bound for the slowest tick is 50 ms (the 20 Hz
budget), as the review asked.

Things to know:
- **M0 numbers taken before this change are not comparable with numbers after it.** `frame_ms` and
  `cpu_ms` no longer contain the sim; `bench.json` has `sim.tick_ms` (the sim thread's own time per
  tick) next to them, plus `sim.behind_passes`, `sim.clock_resets`, `sim.late_commands`,
  `sim.late_publishes`. **Tijmen's laptop bench has to run on this code** (README, "Benchmark").
- The world's checksum includes the commands that wait for their tick, so a run is reproduced
  from its command log only if each command also goes in at the tick it arrived at
  (`LoggedCommand::arrived`). That is a real problem for lockstep, not a matter of a deadline: see
  the open decision 27 below. A command that is moved to a later tick is always logged (requested
  and effective tick, `SimHandle::moved_commands`); the full command log is off in the game.
- Latency from click to visible movement: up to 50 ms input delay plus 30 ms `INTERP_DELAY`,
  about 80 ms. Follow-up: an adaptive `INTERP_DELAY` that follows the measured lateness of the
  snapshots (and so is smaller on a fast machine), once the laptop's `sim.late_publishes` is known.
- The sleep jitter of Windows timers is not known on the laptop; `INTERP_DELAY` (30 ms) is a guess
  made on Linux. If `sim.late_publishes` in the laptop's `bench.json` is not near zero, raise it or
  make it follow the measured lateness.
- `World::passable` looks at terrain only (not at structures), so the snapshot reads the ground once.
  A unit test fails if that ever changes.

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
27. **Pending commands are in the world checksum (open decision for phase 14, owner Tijmen).**
    `sf_sim::checksum` hashes the commands that wait for their tick, so two peers that receive the
    same command for the same tick at different times get different checksums in between: a false
    desync, which a command deadline does not remove. It changes a determinism rule (PLAN.md section
    12.4), so it is not done in WP1.15. Options: (a) hand commands to `World::schedule` only just
    before their tick, so the queue is empty whenever a checksum is taken; (b) leave the pending
    commands out of the checksum (moves every pinned checksum that has commands waiting at a tick
    end). Decide before WP14.1; see `docs/architecture.md` section 18.
