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

Status: the work packages 4.1–4.5, 4.4b, 4.6, 4.6b and 4.7 are in. The "done when" of PLAN.md (200 units across
a bridge: all arrive, none stuck, at most 1 % beyond 1.3 times the packing radius, overlap within a quarter
of a cell on the way and a sixteenth at the end) holds in `bridge_200` (formations on, a crowd above the limit makes none), for every wheeled
turning radius the review swept and for all 112 variants of `sweep_of_variants`; `bridge_200` is a crowd and
forms no formation, so it is exactly as it was before WP4.6. **What is proven of
formations is narrower than that**: groups of 3 to 19 units, blocks 2 to 8 cells deep, three mixes (riflemen with
tanks and transports, riflemen only, tanks and transports only), on the one map of `formation_move` (an open
field, a passage three cells wide, an open field): every run within 1.4 times the walk of the slowest, within 1.3
times the time the same group takes as a group order, none crawling (`formation_sweep`). A group above 19 is not a
formation; between 20 and 24 groups of only tanks and transports miss the 1.4 bound in 8 of 30 runs, which is why
the limit is 19. See "Phase 4 close-out" below the notes of WP4.7 for the open items.

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
  alone; second round after the review of PR #55: a block of units is separated on the tick it is
  placed, the slack of a crowd held up on its destination is bounded by the disc of the group, the
  slots held are looked up in a grid, the crowd pass is back at the speed of WP4.4, and `bridge_200`
  gains the 90-tank block and the goal one cell west as tests; third round: settled units that rest
  in each other are parted, no slot is taken that the unit cannot walk to, and the whole sweep of 112
  variants is clean), `docs/rules/collision.md`,
  `docs/rules/turning.md`: done
- WP4.7 Replan on a blocked route or a disappeared target, destination ≠ route (`sf_sim::path::replan`,
  `path`, `crowd`, `orders`: a unit that is stuck after its sidesteps asks for a new route through the
  budgeted request path with the cells of the bodies that stand still near it (a unit on hold, an enemy
  with no order) blocked **for that request only** (the grid is shut for the slices of its job and opened
  again, the graph is never touched; the abstract search is kept from crossing the sectors the overlay
  cuts), at most `replan_max` times in an order, `replan_cooldown` ticks apart, and then the order is
  given up (idle, or the next queued order); a target that keeps moving is searched for at most once
  in `retarget_cooldown`, and a unit in reach keeps its route; a target that is gone ends the order by
  the generation of its id; a member of a flowfield that a body stops leaves the field; a field whose
  goal a structure covers leads to the nearest cell the class can enter, the slots taken again by
  the claim logic; the looks at the slots go round; the `replan` scenario, in CI on both systems),
  `docs/rules/pathfinding.md` ("Replanning"), `orders.md`, `collision.md`: done
- WP4.6 Formations (`sf_sim::formation`, `orders`, `world`, `path`: one `Move` order that sends 3 to 19
  units that cross the same ground to one point makes a formation; the layout of the group is turned into a
  lattice with room for the widest body (a quarter more), pulled in if the group is scattered, with the unit
  nearest the middle as the leader; every member has a slot that hangs from the leader and turns with the
  heading of the formation, one facing step at a time; the leader goes at the speed of the slowest member,
  terrain included, and at 0.9 of it while a member is behind its slot; the others walk straight to their
  slots at up to their own speed, and a unit ahead of its slot waits for it; the shape lets go ("loose")
  where the ground round the leader or round the front of the lattice is not open, so units file through a
  gap, and forms up again on open ground, where the leader waits for stragglers (behind their slots or to the
  side, not ahead) for a patience that runs down faster when they do not close in, then the formation is
  "broken" until they are back, and a stalled straggler leaves; the patience is filled up and a broken
  formation is whole again only after 60 calm ticks in a row; the heading follows only the leader's own steps;
  slots are handed out again (least sum of squared distances) when the formation forms up and while a member
  is stalled; the formation is let
  go within its extent and the disc of the whole order from the goal and its members take the arrival
  slots of WP4.4b, whose lattice is turned across the heading; the numbers are in `sim.ron`
  (`formation_*`, `World::with_formation`), the state is in a new checksum section `G`; the
  `formation_move` scenario, the `FormationConfig` and `scenario.formation` knobs, `tests/formation.rs`),
  `docs/rules/formations.md`: done
- WP4.6b Formations that hold in a real base (follow-up of WP4.6, found in the viewer: the sandbox of `sf_game` on
  the map `assets/maps/formation`, the 12 mobile Meridian units, one Move; a loose formation keeps the pace of its
  slowest member and a member that follows the order's way may get at most `formation_ahead_tenths` ahead of its
  slot; a unit ahead of its slot and off its column walks back or goes on at the pace instead of standing; the
  leader waits only while the members in their places have no obstacle near; `group_order` judges the shared
  fields per class, so a unit in no formation takes nothing from the classes that formed; `crowd_jam` and
  `formation_move` re-pinned; `tests/formation_base.rs` in `sf_game`), `docs/rules/formations.md` ("In a base"): done

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
walkers) counts as arrived farther out the longer it has pressed, up to 1.3 times the radius of the
disc the group's bodies fill; a unit that has just stopped is
pushed clear if it came to rest against a neighbour; a settled crowd (unit left alone for a second)
closes up toward the point it was sent to, a sixteenth of a cell a tick, straight at it or slid up to
83 degrees away from straight; and the overlap bound holds whatever presses (`COMPRESS`,
`MOVER_OVERLAP`, `HARD_OVERLAP`), the tick the units are placed on included. The wheeled radius is 1.5
cells. **Tested:** one `bridge_200` test for each of 1.25, 1.5, 1.6, 1.75 and 2.0 with the radius
replaced in the movement table (all pass, last arrival tick 3,239 to 3,703, farthest 8.3 to 9.0 cells
against 9.58, worst overlap 0.19), three for a block of 90 tanks in place of the 90 riflemen
(1.25, 1.5 and 2.0: farthest 9.8 to 10.3 against 10.65, worst overlap 0.19), one for the goal one
cell west at 1.5 (farthest 9.27; it was 9.66 in the first round, over the limit) and one for the kinds
dealt out 7 places further on at 1.25 (end overlap 0.031; it was 0.072); in probes, when the
first round was done, also every radius from 1.05 to 3.0 (25 values), hover radii 1 to 4, the turn on
the spot at 60 and 1000 thousandths, and four other mixes of the block: all pass (not repeated after the
second round). A sweep of 112 variants (`sweep_of_variants`, ignored, asserts the criterion for each) is
**clean in all 112** (last arrival 2,974 to 4,176, worst in-run overlap 0.24, worst end overlap 0.031).
**Review round 2 (PR #55).** (1) A block of 90 tanks overlapped by 0.27 on the first tick at every
radius: a unit placed this tick (`CrowdState::fresh`) now makes the pass run `separate` again and again,
up to 64 times, until it moves nobody, so a block placed on a grid is within 3/16 of a cell on the tick
it is placed; the 9 by 9 block of bodies a cell and a half wide is a test. (2) The slack of a crowd held
up on its destination grew without end: it is bounded by 1.3 times the radius of the disc of the group
(every member knows it: `CrowdState::disc`, worked out when the group order is given, so it outlives the
fields; 4 cells for a unit ordered with too few others to be a group), and the cell and a half round a
unit's own slot counts whatever the disc (`orders::crowd_arrival_radius`); the test asks that nobody
counted as arrived beyond that. (3) The arrival docs said 3,200 to 4,200; the sweep says 2,998 to 3,979
(the reviewer saw 4,318 in the first round), the limit stays at 4,400 and is justified in
`bridge_200.rs` and `collision.md`. (4) `LOOK`, 400 against at most 256 slots, never limited anything
and is gone. (5) The cost of a claim: the slots held are sorted into a grid for the claim (exactly the
answers of looking at every one, tested against that), a few thousand steps where it was up to 65,000
for every class; but a profile of `crowd_perf` (callgrind, then timers) showed the claims were under one
per cent of it, and the time was in the neighbour loops of the passes and of the checks WP4.4b added:
the index now hands out a row of three buckets as one stretch and keeps position and body beside each
slot, most neighbours are dropped by their distance along one axis, the weights are worked out only for
a pair that overlaps, and the two checks are skipped on a tick whose last pass found nothing
overlapping. None of it changes a checksum. `crowd_perf`: **84 to 88 ticks a second** in release
(11.4 to 11.9 ms a tick, the slowest 17 ms once 29 ms) against 69 to 71 (14.1 to 14.5 ms, the slowest
18 to 27 ms) in the first round, and 85 to 98 for WP4.4 measured in the same hour. (6) The margin of the
goal one cell west was 0.8 cells instead of -0.08 (0.3 after round 3); it and the 90-tank block are tests.
**Review round 3 (PR #55).** (1) The kinds dealt out 7 places further on, at radius 1.25, ended with
0.072 of a cell of overlap, over the 1/16: two settled units resting between 1/16 and 3/16 apart were
never pushed apart (`separate` acts above 3/16, the passes look only at units that move). A settling
unit that nothing touches and that rests deeper than 3/64 of a cell in a body it may give way to
now steps away from it by half of what is over the slack (`ease_apart`, `REST_OVERLAP`), and a unit stops
settling after 40 calm ticks, so it cannot oscillate; the variant is a test (end overlap 0.031).
(2) The unit that stayed on the west face of the wall in 3 of the 112 sweep runs was taking slots on the
far side of the wall: the claim margin (six cells beyond the slots) reaches across rock and `choose` fell
back to the first free slot when it could see none, so the unit held one against the units that could walk
to it and let go of it as it moved. A slot the unit cannot walk to straight is never taken now (the
same terrain sight test that sends it straight to its slot); a unit that sees none takes none and looks
again next tick (`LOOKS_PER_TICK`, 64 a tick); a wall test is in `field_tests.rs`. **All 112 runs of the
sweep are clean**, so nothing is left for WP4.7 from this. `crowd_perf` 79 to 83 ticks a second after the
parting of settled units (it was 84 to 88).
**Re-pinned** (old to new, once per round): after the first round `bridge_200`
(`0x03b3145755a9081e` to `0x831cd1eaff2b1307`) and `crowd_jam` (`0x3d8fab0ab9a564e6` to
`0x4ac3f990f7070f48`) by the arrival logic and the radius; `orders` (`0x0dcb52425f1adc60` to
`0xf8fbe06cc8a51962`), `terrain_costs` (`0xdc9d5c4a65a60508` to `0xacd34a784a0fd0be`), `pathing`
(`0x606eb6feeffa964c` to `0x398d625625dce5c1`), `pathing_256` (`0x7c4e31d024ed4cf9` to
`0x016c2b5ef7c7e055`) and `turning` (`0xb07ba8066dd59884` to `0xd894752d6d192f8d`) by the radius alone
(with 1.25 put back they give their old values); after the second `bridge_200`
(`0x831cd1eaff2b1307` to `0x381782548cc1782a`) and `crowd_jam` (`0x4ac3f990f7070f48` to
`0x09e9bf488ce69401`), after the third `bridge_200` (`0x381782548cc1782a` to `0x03971fcc3548fc0c`) and
`crowd_jam` (`0x09e9bf488ce69401` to `0x69e7a06fe17a33e5`); `wander_5000`, `command_mix`,
`terrain_256` and `units_spawn` did not move, nor did the other five in the later rounds. **Choices:** the scenario runs 4,600 ticks and the test
limit is tick 4,400 (the gap in the wall behind the bridge passes about one unit in ten ticks, and the
criterion is that everybody arrives, not when); a claim is kept after the order ends and let go at the
next order; at most 16 units take a slot in a tick (the cost of a crowd that arrives at once). **Tried
and not kept**, so nobody tries them again: slots beyond the ring for a class whose ring is walled off
(the overflow ended farther out than it saved); a pool of lattice points for all classes taken in
arrival order (an elongated blob toward the entrance, about half the packing density); idle units as
obstacles that a mover goes round first and shoves only after waiting (pile-ups, overlap 0.5); a
smaller shove allowance of 1.5 to 2 cells (movers could not get through; units never arrived and
many ended outside the disc); the settled units walking back to their own slots instead of closing up (their slots
were taken by the units that had been pushed in); a bound on the slack by the disc of the fields alive
(the fields are gone by the time the last single walkers press on the crowd). **Not done on purpose:**
formations (WP4.6, which will replace the rings), the replan (WP4.7), a better route through the gap of
`bridge_200` (the field leads everybody to the same corner: it is the bottleneck of the scenario and a
pathing matter), and slots for a class below the
threshold of 8 for a flowfield (the five hover craft are single walkers with no slot among the crowd;
they rely on the arrival slack that grows with the time they have pressed on it, within the bound).

WP4.7 notes. **No pinned checksum moved.** `wander_5000`, `command_mix`, `terrain_256`, `units_spawn`
(the four legacy goldens) and `orders`, `terrain_costs`, `pathing`, `pathing_256`, `bridge_200`,
`crowd_jam` and `turning` end on the values they ended on; the only new value is the new scenario `replan`
(`0x2f05c63b9046ecc8`; pinned at `0x7fd2b8f9ff2dede3` when it was added and moved once in review when the per-unit record of a given-up order, `World::gave_up_at`, joined the checksum). What the replans add to the state is hashed only while it is
there (a unit's replan counters, an overlay, a ban, a cursor of the looks, settings that are not the
defaults), so the checksums of the ticks in between differ from before only where a unit asked for a
replan (`bridge_200` has 5 or 6 of them; its final state is the same) or stood in reach of its target
with the route it had (no pinned scenario ends differently for it). **`bridge_200` still meets the
criterion of phase 4** with no change in its numbers (last arrival, farthest, worst overlap as in
WP4.4b); the sweep of 112 variants (`sweep_of_variants`, ignored) is **clean in all 112** (last arrival 2,974 to 4,176, worst in-run overlap 0.241, worst end overlap 0.031, 1 to 13 requests for a new route in a run, nobody given up), the same numbers as before WP4.7.

What it does (`pathfinding.md`, "Replanning"; `orders.md`; `collision.md`):

* **A stuck unit's request goes round the bodies that stand still.** After `stuck_attempts` sidesteps the
  crowd pass hands the cells that the units on hold and the idle enemies near the unit (within 8 cells)
  fill, with the two radii and a quarter of a cell added, to the request as an **overlay** (at most 32
  cells, in the route record). The job of that request sees them as blocked: `Pathing::serve` shuts the
  cells in the class grid for the length of the slice and opens them again before anything else is
  served, so the graph, the portals and every other unit's request are untouched (tests: `sight_clear` and
  `plan_route` through the holder give the same answer on every tick of the request). The abstract
  search may not cross the start and goal sectors by the edges of the graph if the overlay is in them (their
  links were made with it), and bans a sector the overlay cuts when a leg in it cannot be refined, and
  plans again. Structures are in the graph already: a structure on the route needs no overlay, the
  unit asks again at once (`map_replans`).
* **Limits and giving up.** At most `replan_max` (3) per order, `replan_cooldown` (100 ticks) apart; a
  request that finds no way round fails and the unit stands, stuck, until it asks again. When it would
  ask once more the order is **given up**: it ends as if done, the next queued order starts or the unit is
  idle where it stands. Idle, because the nearest reachable cell is where it is and waiting for ever
  would hold up the queue and make a deadlock of two units permanent. Not counted against the limit:
  a map change (the graph is right) and a target that moves.
* **A target that moves** is searched for at most once in `retarget_cooldown` (20 ticks): the re-search of
  WP4.2 behind a cooldown, in the one place that decides it (`Pathing::steer`). **A unit in reach of a
  target keeps its route** (it used to be dropped): a tank that catches up with a rifleman, stands, and
  sees it step out of reach asked for a new route every four ticks (about 400 jobs in 1,500 ticks for
  one hunter in the test); now it steers on after it at once and asks again only when the target has
  moved three cells from where the route was asked for, once in the cooldown (about 20 jobs).
* **A target that is gone** ends the order by the generation of its id (WP3.4, verified and kept, in the
  scenario and in `tests/replan.rs`, with a new unit in the slot and a queued order behind the attack).
  **A target that cannot be reached** leaves the unit at the nearest cell, waiting, the order kept
  (WP4.2, verified with a target that paces up and down an island).
* **Groups.** A member of a flowfield that a body stops leaves the field for a route of its own with the
  overlay (the field leads through the gap the body stands in). A structure built over the **goal
  cell** of a field: the successor leads to the nearest cell the class can enter within a sector (least
  squared distance, ties to the lower cell; not a pocket the structure shuts in; `Field::want_goal` is the
  order's cell and the goal is worked out again at every build, so taking the structure away gives it
  back), the slots lie round that cell and the units that are on their way take them by the claim logic
  of WP4.4b (a rebuilt field starts with no holders).
* **Follow-ups of the review of PR #55.** The 64 looks per tick that units make at the free slots go round
  like the requests of the path service (`Fields::look_from`, `look_denied`, hashed while they are not
  at rest): the first unit turned away when a tick's looks run out is where the next tick begins, so a
  unit that looks again every tick cannot shut out the ones with higher ids (a test with 100 units
  behind a wall: each looks twice in four ticks; before, the last 36 never). **The 0.24 against 0.25
  margin is thin and stays so:** the sweep's worst overlap during a run is 0.24 of a cell against the
  quarter of a cell of the criterion (0.25), a margin of 0.01 that a change to the crowd pass can use up
  without any other number moving; the end overlap (0.031 against 1/16) is not thin.
* **Measured.** The `replan` scenario (23 units, 3,000 ticks, six rooms): 4 stuck replans (all with an
  overlay, 3 of them found nothing), 2 map replans (the tower's, and the next room's tank, whose route lay in
  a sector beside the one that changed), 8 target replans, one order given up, 25 jobs, the budget never
  exceeded; `crowd_perf` 83 to 86 ticks a second in release (11.6 to 12.0 ms a tick, the slowest 17.7 to 22.6 ms; WP4.4b: 79 to 83), one request for a new route and no overlay in it.

Not done on purpose: formations (WP4.6, below); bodies that walk are still no part of a route (a crowd is walked
round by pushing and sidestepping); the overlay looks at the bodies of the tick it is made in (no
prediction) and within 8 cells; a goal that is rock or water when the order is given still sends every
unit to its own nearest cell (a substitute goal is found only for a goal a structure covers); a
better route when a structure is removed and the old one is still walkable is not looked for;
the replan counters and the cursor of the looks are in memory like everything else of the world (nothing
new is lost on a restart: they are in the checksum and the world is cloned whole).

WP4.6 notes. **The four legacy goldens did not move** (`wander_5000` `0x38d5e3c43358a889`, `command_mix`
`0x43f6583004258ceb`, `terrain_256` `0xb577112845e0097e`, `units_spawn` `0x6c369f42be150d6e`), and neither
did `orders`, `terrain_costs`, `pathing_256` and `turning`. A world in which no move order forms a
formation hashes exactly as before: section `G` of the checksum is written only while a formation exists, a
field's `turn` only when it is not zero. **`bridge_200` did not move either** (`0x03971fcc3548fc0c`): the 200
are a crowd, and an order that sends more than `formation_max_units` units forms no formation at all, not even
among its five hover craft (see the decision below for why). **Two scenarios were re-pinned, because their
behaviour changed on purpose** (the old values are the ones on main before WP4.6; the new ones are those after
review round 3, which changed the formation rules again, so they differ from the earlier heads of the PR):

| Scenario | Old | New | Why |
|---|---|---|---|
| `crowd_jam` | `0x69e7a06fe17a33e5` | `0x11dfc55cfc2c2c34` | the two groups of 18 and the 8 tanks of the corridor are formations; its assertions are unchanged and hold |
| `replan` | `0x2f05c63b9046ecc8` | `0xb0fa2fa70c34bc99` | its group order makes a formation (1 made, 1 let go); the assertions of `tests/replan.rs` are unchanged and hold |

New: `formation_move` (`0x83315f6388469125`; 16 units since review round 3). `pathing` keeps its pinned value by `formation: Some(false)`
(like `flowfield_threshold: Some(0)` and `collision: Some(false)` in the same file: it is the test of the queue of
single requests, which a formation, whose members ask for no route, is not). Two tests were told the same
and nothing else changed in them: `sf_sim/tests/pathing.rs` (the queue and the budget of single requests) and
`sf_sim/tests/flowfield.rs::the_budget_decides_when_the_group_arrives_and_never_where`, which asserts that a
path budget changes only when a group arrives and never where it ends: true of fields and of the arrival
slots, **not true of a formation**, whose arrival order, and so the claims of the slots, depend on when the
leader gets its route; it stays true for a group that is not one. In review round 3 one more test was told the
same: `sf_sim/tests/collision.rs::a_unit_takes_its_slot_when_it_comes_near_so_the_crowd_fills_in_the_order_it_arrives`
(twelve tanks in a column, the first to come takes the innermost slot) pins the claims of a group order; a formation
keeps the column closed (the front units wait for their slots), it reaches the slots as one block, and the claims of
one tick go in id order, so there is no first to come.

**Decision: formation-move is the default for a group move order, for groups of 3 to 19 units that cross the
same ground** (`formation: true`, `formation_min_units: 3`, `formation_max_units: 19`). The first version had
no upper limit and `bridge_200` is what showed why one is needed: the 195 foot, wheeled and tracked units
formed one lattice 31 cells across; its leader was at x = 26.6 after 4,600 ticks (from 17.7), the leader
waited for stragglers or sat at the rock outcrops with most of its slots inside rock, and most of the
200 had not arrived when the run ended. A crowd of that size is the group order of WP4.3 and WP4.4b, which the phase-4 criterion is
made for, and it stays that. The limit was first 48, untested; review round 1 swept the size and the shape of the group
(below) and set it to 24; review round 3 made the sweep assert the time against the group order as well, for
every mix, fixed four causes of delay it found (below) and set it to 19, the largest size at which the sweep is
clean for every width and mix. Mixed classes: foot, wheeled and tracked are one family (they cross the same
ground; a group of them is one formation, whatever their speeds); a hovercraft and a vessel are each a family
of their own; a land unit and a vessel sent together do not form one formation (the family that can reach the
goal forms its own, the other follows the order as it always did); aircraft form none.

**The limit counts the whole order, not the family** (review round 1). In the first version the five hover craft
of `bridge_200` were a family of five and formed a formation inside the crowd of 195. With the round-1 rules one
of the 112 variants of `sweep_of_variants` (`tanks=false`, goal (80, 33), radius 1.5, other seed) overlapped two
bodies of the crowd by 0.270 of a cell at tick 2430 against the quarter of a cell of the criterion; the pair were
two wheeled vehicles of the crowd, arriving among hover craft that stood on their slots early (the margin is thin and is
chaotic: 12 seeds of the same variant gave 0.182 to 0.270, 3 of 36 runs over 0.21). A formation inside a crowd
buys nothing the criterion measures and costs it margin, and a selection of 200 is a mob whatever its families,
so an order that sends more than `formation_max_units` makes no formation. `bridge_200` is then bit for bit what
it was before WP4.6 and so is every number of its sweep. The hover class has no formation test of its own any
more (the gunboats of `tests/formation.rs` are the only water family that forms up).

What it does and the numbers are in `docs/rules/formations.md`. How it was found (the things that did not
work first, because they are the reasons for the rules; the findings about the hover craft of `bridge_200` were made
when they still formed a formation there):

* A route a unit had from before it walked to a slot is stale when it goes back to following the order
  (the tanks of `formation_move` walked back to a waypoint of the passage after the release); a member goes
  back to its way from where it stands: a flowfield member chooses its leg again, a route of its own is kept
  only if its first waypoint is ahead of it and near (otherwise `bridge_200`'s hover craft asked for 65 routes
  and `the_groups_start_within_a_few_ticks` allows 40), else it is asked for again.
* A leader held to the pace of the slowest while it stood in a gap, and members that saw their slots standing
  in the doorway, jammed the five hover craft at the wall of `bridge_200` for 1,000 ticks. Hence "loose": where
  the ground round the leader is not open (no rock, water or structure within three cells in eight directions)
  the leader goes at its own speed, nobody waits and everybody follows the order's own way; the leader also
  waits only on open ground.
* A rifleman that has to pass the tank standing on its own slot, in the same row, never does (two straight walks
  in a line): the slots are handed out again (least sum of squared distances) when the formation forms up after a
  loose stretch and every 100 ticks while a member is stalled. (Rounds 1 and 2 also let a stalled member swap its
  slot with the one in its way; round 3 took that out, see below.)
* A leader that waits for stragglers for as long as they are closing in (the first version) waited 1,336 ticks
  for four hover craft in one variant of the sweep, which arrived at tick 4,512 against the 4,400 of the
  criterion. Now the patience runs down a third as fast while every straggler closes in.
* The formation is let go within the disc the bodies of the **whole order** fill (not only its own extent): the
  five hover craft were released inside the crowd of the 195 and the sweep showed a hover craft that never
  arrived and an end overlap of 0.084.

**Review round 3: a formation of fast units was slower than the same group without one.** The sweep of round 2
asserted the 1.4 bound for groups with riflemen only; groups of only tanks and transports of 11 to 24 missed it in
33 of 84 runs (1.18 to 2.09 times their walk; as a group order 1.13 to 1.54), and the delay was there for the mixed
groups too, hidden in the riflemen's long walk (`mixed` up to 1.34 times its group order). The sweep now asserts,
for every run up to the limit and every mix, the formation's time against the same group's as a group order (at
most 1.3 times) next to the 1.4 bound and the no-crawl bound; with that, round 2's code failed 35 runs. The causes,
each measured in the runs (the tick the rear of the group passes x = 20, 40, 48, 60, 80 and 90 and is idle, with
and without a formation), and what was done:

* **The leader at the mouth of the passage.** The formation let go only when the ground round the *leader*, in the
  middle of the block, was not open; by then the front rows of a wide block stood at the wall in their slots, and
  the traffic pushed the leader off the mouth: in the worst run (17 vehicles, a block 8 deep) it was pinned against
  the rock beside the passage for 560 ticks, sidestepping away and back, while the rest ran on to the goal. The
  formation is now also loose while the ground round the **front of the lattice** (its extent ahead of the leader)
  is not open, so a block lets go before it reaches a wall and files in as a group order does. 35 failing runs
  became 28.
* **Units that came through first were taken for stragglers.** A unit ahead of its slot was a straggler as soon as
  its slot came within the re-form distance along the heading, if it was off to the side a little (2.9 cells ahead
  and 1.5 aside is 3.3 from the slot): the leader stopped, the unit walked back, and the next one did the same, for
  200 ticks behind the passage. A straggler is now a unit behind its slot, or as far to the side of it as the
  re-form distance; a unit ahead of it is none. (Only "not ahead" was tried first; a unit shut in 10 cells to the side
  but a little ahead was then no straggler, and `a_broken_formation_is_whole_again_only_after_a_calm_stretch` caught it.)
* **The pace was always 0.9 of the slowest**, a tenth over the whole march, for nothing while every member is on its
  slot. The leader now goes at the slowest member's own speed, and at `formation_pace_permille` (0.9) of it only while
  some member is more than half a cell behind its slot. `mixed` went from 1.08 to 1.21 times the walk to 1.01 to 1.15
  (the sweep before and after this change alone).
* **The block came to the goal and took its slots from eight cells off.** It was let go within its extent and two cells of
  the goal; now within its extent (the front is about at the goal): 23 failing runs became 15. Later still was worse
  (half the extent: 16; one cell: 31).
* **The swaps passed far slots around.** A stalled member swapped slots with the one in its way; with a lattice snapped
  from a block it handed a lone slot two rows ahead from one unit behind to the next (three swaps in 80 ticks at the
  start of a march), and each new holder was a straggler. Taken out: 15 failing runs became 10 (the reassignment by
  least squares stays; without it they were 18).
* **The fast units ran to the goal while the rest filed through.** With the formation loose for longer (the first
  item) the tanks of `formation_move` came out of the passage and drove on to the goal, 28 cells ahead of their
  slots (only 59 % of the ticks on open ground in shape). A unit ahead of its slot by more than the re-form
  distance on open ground now waits for it also while the formation is loose. `formation_move` is in shape on
  100 % of those ticks, the farthest 0.93 cells from its slot.

Tried and not kept, with the numbers: a look-ahead of 3 or 6 cells beyond the front (18 and 19 failing runs, against
23); a unit ahead of its slot by half a cell waiting (the mixed groups deadlocked: 52); one that waits only when its slot
is straight behind it (two mixed runs failed); a tighter lattice (spacing 1100: 11) and a pace of 950 (10), both against
8; the arrival slots of a formation taken nearest first instead of in the order of the spiral (it cut the end of one run
by 52 ticks and left the mean of all runs where it was, 1.241 against 1.243 times the walk, so the shared claim code was left
alone). **What is left above 19**: groups of 20 to 24 tanks and transports miss the 1.4 bound in 8 of 30 runs (1.40 to
1.50; none is over 1.26 times its group order, which itself takes 1.17 to 1.45 times the walk there). The time is spread
over the start of a deep block (the lattice is wider than the block the units stand in, and the units at the back
stand ahead of their places and walk back to them), the queue at the passage (chaotic: in the runs looked at, from 180 ticks faster to 133 slower than the group order), the
wait behind it, and the arrival; no single cause is left, so the limit is 19.

**`bridge_200` meets the criterion of phase 4, unchanged by WP4.6** (shipped wheeled radius 1.5, `cargo test --release -p
sf_headless --test bridge_200`; the 200 are a crowd above `formation_max_units` and form no formation, so the run is
bit for bit the one before formations, `0x03971fcc3548fc0c`): all 200 arrive by tick 3,456 (172 s), 0 of 200 end
beyond 1.3 times the packing radius (the farthest 8.29 against 9.58 cells), the worst overlap during the run is
0.190 of a cell (limit 0.25) and at the end 0.031 (limit 0.0625), nobody is stuck at the end, the longest a unit
stood still with its order not done is 165 ticks (limit 600). All 19 tests of the file pass. **`sweep_of_variants`
(112 variants) is clean in all 112** on the final code, run once: last arrival 2,974 to 4,176 (the limit is 4,400),
worst overlap during a run 0.241, at the end 0.031, the longest any unit stood still 514 ticks (limit 600), 1 to 13
requests for a new route in a run. (History: with the five hover craft as a formation inside the crowd, the sweep
had 5 misses in the first run, 1, then none in the first PR, and 1 again after the round-1 rules, 0.270 against the
quarter of a cell; the order-level limit above is the answer to that last one, and these are the numbers of the
sweep without it, which are those of the sweep before WP4.6.)

**`formation_move`** (map `assets/maps/formation`; 16 units since review round 3, under the limit of 19: 8 riflemen
at 0.7 cells a second, 4 light tanks at 2.0 and 4 light transports at 2.5 in a block of 4 by 4, one order to a point
91 cells east, behind a wall with a passage 3 cells wide and 8 long): on open ground the formation is whole for 1,854
ticks and in **all of them every member is within 3 cells of its slot** (the farthest 0.93 cells; the lattice reaches
4.46 cells from the leader); the leader goes 0.0349 cells a tick in the west field against the rifleman's 0.035 (nobody
is behind, so the pace is the rifleman's own) and the group is never more than 5.3 cells long along its heading; the
formation is loose for 504 ticks at the passage (up to 15 members following the order's way at once) and is **formed up
again at tick 1,370 by the check of the first version**: every member walking straight to its slot and within 1 cell of
it (rounds 1 and 2 had loosened that to 3 cells or waiting; it holds again as first written); it is let go at tick
2,586; everybody has arrived by tick 2,812, 1.07 times the 2,629 the riflemen need alone (the bound of the test is
1.4); the leader never waited, no break, nobody expelled; the worst overlap during the run is 0.048 of a cell and at
the end 0.029. `crowd_jam` and `replan` are marched by formations too and keep all their assertions.

**Sizes (review rounds 1 and 3).** `tools/sf_headless/tests/formation_sweep.rs` (`#[ignore]`d, about half a minute in
release for both of its tests) runs the march of `formation_move` for every size 3 to 19, blocks 2, 3, 4, 5, 6 and 8 cells deep
and three mixes (`mixed`, `foot`, `wheels`), 273 runs, each with and without a formation, and asserts **for every run**:
idle within 1.4 times the slowest's walk, within 1.3 times the same group's time as a group order, and the rear of the
group making at least a quarter of the slowest's walk in every window of 600 ticks. Nothing within the limit is only
printed. On the final code:

| Mix | Times the walk | Without a formation | Times the group order | Rear in a window |
|---|---|---|---|---|
| `mixed` | 1.02 to 1.18 | 1.01 to 1.05 | 1.00 to 1.12 | 0.72 and more |
| `foot` | 1.01 to 1.05 | 1.01 to 1.05 | 0.99 to 1.02 | 0.88 and more |
| `wheels` | 1.02 to 1.37 | 1.04 to 1.84 | 0.62 to 1.24 | 0.55 and more |

The margin of `wheels` against 1.4 is small at many sizes, not only near the limit (1.37 at 7 and at 19, 1.35 at 16):
the runs are chaotic, a change anywhere in the march moves which run is worst. `sizes_above_the_limit_for_the_record`
makes the same march as a formation for 20 to 24 units (the limit raised for the probe) and asserts nothing: `mixed`
1.06 to 1.16 and `foot` 1.03 to 1.11 times the walk, clean; `wheels` 1.24 to 1.50, 8 of 30 runs over 1.4 (20 units in a
block 2 deep at 1.41, the worst 24 in a block 4 deep at 1.50), none over 1.26 times its group order.

Cost: `crowd_perf` (2,000 colliding units, no formation among them) 66 to 69 ticks a second on this machine
against 65 to 67 without the change. The formation pass costs a thick line of sight per member per tick and
sixteen for the leader (round 3: eight round it and eight round the front of the lattice, and the line between); a
group of 19 is a few dozen line-of-sight calls a tick.

Not done on purpose, and why: attack-move, guard and queued moves in formation (they keep their behaviour; a
formation that fights is phase 5); a shape the player chooses (line, wedge, column: the shape is the group's own,
kept; a named shape is another offset generator); formations of more than 19 (a crowd); a member that left
rejoining; hover and vessel formations beyond three gunboats tested; the arrival slots follow the group's
rings, not its lattice (below); presentation (an outline of the slots).

**Arrival, as built.** The slots of the goal are the class rings of WP4.4b turned about the goal by the heading
of the formation, and the members take them by the claim logic on approach, so a blocked slot is simply not
taken and the bounds of the arrival are the ones that hold for `bridge_200`. The lattice of the march is not the
lattice of the arrival: the march is one lattice for all classes (the widest body's spacing for everybody), the
arrival packs each class on its own (the criterion of phase 4 is a packing: 200 bodies on one lattice of the tank's
spacing would need a disc about 1.4 times the radius of the one the criterion measures, which allows 1.3). So the group goes from one shape into the
other when the formation is let go, within the disc of the order.

WP4.6b notes (formations that hold in a real base). **The base-start gap was found in the viewer, after the phase-4
close-out below, and is fixed here.** Everything WP4.6 proved was proved on a field with one passage and no
structures (`formation_move`, `formation_sweep`) or on a crowd (`bridge_200`); nothing built the world the way the game
does and gave an order in it. In `sf_game`, on the sandbox map `assets/maps/formation`, box-selecting the 12 mobile
Meridian units and giving one Move sent the tanks ahead at 1.4 to 2 cells a second with the riflemen (0.7) behind and no
shape to be seen. The diagnosis found no wiring bug (the settings reach the world, there is one group order, the sim
thread passes it through); the formation forms, with 11 members. Three things degrade it among structures, all
confirmed by measurement:

1. A structure within three cells makes the ground not open, so the formation is **loose** (12 to 51 % of the ticks
   of the four goals measured), and loose meant "the leader and the members at their own speed": no pace at all.
2. A unit whose line of sight to its slot a structure blocks takes a longer way round and comes out ahead of its slot
   **and to the side of its column**; the "ahead" test ignored the sideways offset, so it stood there 5 to 7 cells off
   its column in the way of the units behind it, and the leader did not wait for it.
3. The order has one **amphibious transport**, a family of one, in no formation; `all_formed` (every unit that
   starts the order is a member) was then false and no class got a shared field or the arrival slots of the goal.
   (Read in the code, now measured: the three classes of 5, 4 and 2 units had no field before and have one after.)

What changed (orchestrator decisions A to D, 18-10-2026; the rules are in `docs/rules/formations.md`):

* **A. The pace holds in narrow places.** The leader steps at most the formation pace loose or not; a member that
  follows the order's way runs at its own speed while it is at most `formation_ahead_tenths` (new, `sim.ron`, 2.0
  cells as shipped, 0.5 up to the re-form distance) ahead of its slot along the heading, and goes no faster than the
  formation beyond that. The docs said a loose formation means own speed; they now say it does not.
* **B. A unit ahead of its slot and off its column does not stand.** It walks back if it sees its slot, follows the
  way at the pace of the formation if it does not, and is no straggler either way (it is ahead). One more rule came
  out of the sweep, not out of the diagnosis, and is kept because the sweep is not clean without it: the leader waits
  for stragglers only while the members that stand in their places have no obstacle within three cells of them (the
  edge of the map is no obstacle for this), because rear rows standing on their slots in the mouth of a gap held up
  the stragglers the leader waited for.
* **C. A unit outside every formation takes nothing from the classes that formed.** `group_order` in `path.rs` judges
  each class: all its units in a formation is the same rule as before ("a class of two or more gets a field, below the
  threshold"), a class with a unit outside every formation is judged by the threshold as ever.
* **D. Structures stay narrow places.** Not touched.

**Tests (the missing coverage).** `crates/sf_game/tests/formation_base.rs` builds the sandbox as the viewer does
(`playfield::make_world`, `with_movement`, `sandbox::schedule`, the viewer's world speed) on `assets/maps/formation`,
selects the 12 mobile Meridian units and sends one `commands::move_command` to (56, 44), (90, 44), (90, 10) and
(100, 40): one formation of 11; the three formed classes have shared fields and the amphibious transport none; the
leader's pace and the share of windows above 1.2 times the slowest's speed in loose stretches (29 % of the windows
before, 14 % after, bound 20 %); the shape; everybody arrives, nobody stuck, overlap 0.049 at most during the run (bound
0.25) and 0.030 at the end (bound 1/16); reproducible. Run on the head of main (670cf33) it fails 3 of its 7 tests
(pace, shape and fields). `tests/formation.rs` has five new tests, each of which fails on the old rules: the leader of a
loose formation keeps the pace (0.1000 cells a tick before, 0.0350 after), the bound on the lead is data, the
off-column tank walks back (it stood 45 ticks), a far tank is not waited for, and the unit outside every formation
leaves the fields to the classes that formed.

**Measured** (the table is in `docs/rules/formations.md`, "In a base"). Over four goals the loose stretches have 14 %
of their windows above 1.2 times the slowest's speed against 29 % before. **The shape in the sandbox is not good:** all
members within 3 cells of their slots 14 to 38 % of the ticks (9 to 42 % before), members within 3 cells 59 to 92 %
(62 to 89 %), the formation loose 38 to 71 % of the ticks (12 to 51 %). Over ten goals the means of the members within
3 cells are 71 % before, 77 % with the pace, off-column and wait rules alone, and 73 % with the shared fields of
decision C as well (which also bring the mean arrival from 1,699 to 1,610 ticks). The structures are narrow places by
decision D, and two heavy tracked units (1.5 cells across) are stuck among them for hundreds of ticks in several runs,
with or without a formation: paths and fields are made on cells, and the sandbox leaves one-cell gaps between
structures. That is a movement problem (bodies in the routes), not a formation rule, and it is listed in the close-out.

**Re-pinned, with the reason** (the pace in loose stretches, the bound on the lead, the wait rule and the per-class
fields are changes of the specified behaviour; the assertions of both scenarios' tests are unchanged and hold):

| Scenario | Old | New |
|---|---|---|
| `formation_move` | `0x83315f6388469125` | `0x67abfe8bf07e8f05` (loose 521 ticks, formed up again on tick 1,356, let go on 2,597, everybody arrived on 2,840) |
| `crowd_jam` | `0x11dfc55cfc2c2c34` | `0xc7da0f15ce1bf3ce` (its groups of 18 and 8 are formations that pass the gap) |

**Not moved:** `wander_5000` `0x38d5e3c43358a889`, `command_mix` `0x43f6583004258ceb`, `terrain_256`
`0xb577112845e0097e`, `units_spawn` `0x6c369f42be150d6e`, `orders`, `terrain_costs`, `pathing`, `pathing_256`, `turning`,
`weapons_reload`, `replan`, and `bridge_200` (`0x03971fcc3548fc0c`: a crowd, no formation, must not move and does not).
`formation_sweep` (273 runs) is clean with the bounds as they were: `mixed` 1.02 to 1.14 times the walk (1.02 to 1.18
before), `foot` 1.01 to 1.05, `wheels` 1.02 to 1.37 (the same worst, 1.37). It took four rounds to get there: the
pace in loose stretches costs time in the sweep's loose stretch (a leader that was a truck at 2.5 cells a second goes at
the tank's 2.0), and the first versions of the rules put one run of 273 over the bound by 2 to 56 ticks (the runs are
chaotic and the margin of `wheels` is thin, as the notes of WP4.6 say). `sweep_of_variants` (112 variants) is clean:
the crowd forms no formation.

Not done on purpose: bodies in routes and fields (the cause of the stuck heavy units); `open_ground` still treats the
edge of the map as an obstacle for the loose rule (only the new wait rule ignores it); a unit left out of every
formation (the amphibious transport) still walks the order at its own speed beside the formation, which is what the
viewer shows of it; presentation of the slots; a second sandbox layout, other maps and other orders.

Screenshots (Xvfb with Mesa lavapipe, the real `sf_game` release build, the 12 units box-selected and one right-click to
(56, 44), the same camera and the same ticks before, on the head of main, and after):
`docs/screenshots/wp4.6b/contact_sheet_before.png` and `contact_sheet_after.png`. Honestly: the units are small at the
zoom that shows the whole way, and the two sheets look much alike (the difference is in the pace of the units that
follow the order's way, which a still does not show well, and in the numbers above); they show that the order, the camera
and the run are the same, and the amphibious transport reaching the goal alone first in both.

## Phase 4 close-out

Status of the work packages: 4.1 movement classes, 4.2 hierarchical A*, 4.3 flowfields, 4.4 local avoidance and
collisions, 4.5 turning, 4.4b robust group arrival, 4.7 replanning, 4.6 formations and 4.6b formations in a real base
are in
(`docs/rules/movement.md`, `pathfinding.md`, `collision.md`, `turning.md`, `orders.md`, `formations.md`). **Amended
for WP4.6b:** the claim below was made for a field, and the viewer showed the gap: formations in the sandbox, among
structures and with a unit in no formation, had no pace and no fields until WP4.6b (see its notes above). The
criterion of phase 4 is met by `bridge_200`. For formations the claim is the one the sweep supports and no
more: groups of 3 to 19, of riflemen with tanks and transports, of riflemen only and of tanks and transports only, at
every depth of block tried (2 to 8), on the map of `formation_move`, arrive within 1.4 times the slowest's walk and
within 1.3 times the same group's time as a group order, and never crawl; the first item below is the limit.

**The done-when of phase 4, asserted again with everything in** (`cargo test --release -p sf_headless --test
bridge_200`, the shipped scenario, wheeled radius 1.5, formations on; the crowd forms none): 200 units cross the
bridge; all 200 arrive (by tick 3,456); none is stuck (no unit with a stuck count, a request for a new route or an
order left at the end; the longest any unit stood still with its order not done is 165 ticks); 0 of 200 end beyond
1.3 times the radius of the disc whose area is the sum of their spacings squared (the farthest 8.29 cells against
9.58; 109 are outside the 5 cells the criterion had before bodies); the worst overlap of two bodies during the run is
0.190 of a cell (limit 0.25) and at the end 0.031 (limit 1/16). The same holds in the 19 tests of the file and in
all 112 variants of the sweep (last arrival 2,974 to 4,176, worst overlap 0.241, end overlap at most 0.031).

Open follow-ups, carried over:

1. **The limit of a formation is 19 units**, the largest size at which the sweep is clean for every mix: groups of
   20 to 24 tanks and transports miss the 1.4 bound in 8 of 30 runs (1.40 to 1.50 times their walk, at most 1.26 times
   their group order); mixed and riflemen-only groups are clean up to 24. The margin of the fast groups is small
   at every size (up to 1.37 within the limit) and chaotic. The time that is left is spread over the start of a deep
   block, the queue at the passage, the wait behind it and the arrival (review round 3, above). Raising the limit
   would take a start that does not walk units back to their places (a leader at the back of the block, for
   instance) and an arrival that keeps the shape.
2. **Proven on one map**: the sweep and `formation_move` march across an open field, through one passage three
   cells wide and across another field. Rock outcrops, a river with a bridge, several passages and turns are not
   swept (`tests/formation.rs` has single cases of them). A base of structures is measured since WP4.6b
   (`sf_game/tests/formation_base.rs`, items 14 and 15).
3. **The margin of the overlap during a run is thin and stays so**: 0.24 against 0.25 in the worst variant of the
   sweep (WP4.7 said the same). It is chaotic: a small change of the arrival of five hover craft among the crowd
   took one variant to 0.270 (and twelve seeds of it gave 0.182 to 0.270), which is why a crowd forms no formation.
4. **A selection of more than 19 forms no formation** (`formation_max_units`, counted over the whole order): a big
   selection moves as a group order.
   A formation of hundreds would need a lattice that bends round rock (slots on the map, not in a frame), a
   leader that does not wait for hundreds, and the arrival packing in the lattice itself.
5. **The march lattice is one cell for all classes** (the widest body); riflemen among tanks stand 1.7 cells from
   each other. A lattice of rows by class, or one cell per row, would pack tighter and look better.
6. **Formations of hover craft and vessels** are tested with three gunboats (hover craft: not at all since round 1); amphibious groups that cross
   water and land in one order may want a looser shape or none (the hover craft of `bridge_200` were the last to arrive
   in a formation, 394 ticks after the last unit of the run without, and since round 1 they do not form one there); `formation: false` per family is not a setting yet.
7. **No attack-move or guard in formation, no player-chosen shape, no rejoining** (above).
8. **Presentation**: nothing draws the slots or the formation; the renderer sees only positions.
9. **Saving** (phase 13): a world rebuilt from a save has to restore the formation column (`Entities::form`,
   including the mode of the last tick, which is hashed) and the formation list with the rest.
10. **A real-time check on the reference laptop** of 2,000 units with several formations has not been made (the
   headless numbers above are for this machine).
11. **`formation_sweep` has one geometry.** Add a second one: the one-cell corridor gives vehicle-only groups
   of 12 and 16 at 1.34 and 1.32 times their no-formation time (WP4.6 review). The sweep's bounds were chosen on
   the one map of `formation_move` (an open field, a passage three cells wide, an open field), so a narrower
   passage is where the next margin is likely to be thin.
12. **Strung-out starting positions are not swept.** A 24-cell column takes 1.34 times the time while it closes
   up (WP4.6 review); the sweep starts every group in a block. Add column and ragged starts of several lengths.
13. **The end-overlap margin is thin.** 19 vehicles into a 5-wide corridor end at an overlap of 0.060 of a cell
   against the limit of 0.0625 (1/16) (WP4.6 review): a small change of the arrival moves it over. Either widen
   the margin (the arrival slots) or sweep corridor widths 3 to 8 for the end overlap, as the sweep does for the
   time.
14. **Bodies are not part of the routes** (found in the viewer, WP4.6b). A heavy unit (1.5 cells across) is sent through
   a one-cell gap between two structures, gets stuck, sidesteps, asks again; in the sandbox two of the twelve selected
   units (the harvester and the siege artillery) are stuck among the structures for 500 to 1,500 ticks in several runs,
   with or without a formation, and the shape of a formation in a base is poor mostly because of them. A movement work
   package: clearance in the sector graph and the cost grid of the fields, or a minimum gap per class.
15. **The sandbox is one layout and `formation_base` has four goals.** The bounds hold for those; a second base, other
   selections (the Veyran side, a selection of only vehicles, of 19) and goals inside the base are not measured.
   `open_ground` also treats the edge of the map as an obstacle for the loose rule (the wait rule ignores it): a
   formation along the edge of the map is loose all the way.

## Phase 5 — Combat

Status: WP5.1 is in; WP5.2 to WP5.6 are not started.

- WP5.1 Weapons and bullet types as data (`weapon_defs.ron` gained the shape of a burst, the parameters of
  the projectile, the mount and the firing arc, and a fifth projectile kind, `Instant`; `sf_bridge::weapon_table`
  turns the definitions into `sf_sim::WeaponTable`, which a world is given with `World::with_weapons`; every
  entity keeps a reload and a burst counter per weapon slot, in a new last checksum section `W` that is written
  only while one is running; a unit with an `Attack` order fires an instant-hit placeholder; the
  `weapons_reload` scenario; no pinned checksum moved), `docs/rules/weapons.md`: done

### WP5.1 notes

**What existed and was extended, not rebuilt.** `weapon_defs.ron` (range, minimum range, reload, damage,
damage type, projectile kind, splash), the permanent `kind` and the `retired_weapons` tombstones, the validation
with file and line, `SimWeaponDef`, `UnitStats::weapon_range` (the first weapon's range, what `Order::Attack`
walks to) and the weapon reach digest in the orders section of the checksum were all there from WP3.1 to WP3.4.
New: the fields below, a tombstone that also protects the **id** (for every list with tombstones), the
turret-mount check on the units, the sim side (`sf_sim::weapons`), the bridge function and the scenario.

**Data.** `burst` and `burst_delay` (the reload counts from the first shot of a burst, so the cadence is the
reload whatever the burst; a burst that leaves no rest is refused), `projectile_speed`, `flight_height` and
`guide_turn` (only for the kinds that use them; `Instant` takes none), `mount` (`Hull` or `Turret`; a turret mount
needs a unit with a `turret_rate`) and `fire_arc` (half the arc in facing steps, 0 to 128). All optional with
defaults that change nothing: the five frozen copies of the definitions under `assets/scenarios/*_defs/` predate
them and still load, which `crates/sf_data/tests/weapons.rs` checks. Times are refused unless they are whole ticks
(no silent rounding); distances round to the nearest 1/65536; a speed per tick is rounded once from the exact decimal.
The placeholder set got speeds, heights, a rocket that steers, the turret mounts of the tanks and the howitzer,
and two bursts that keep the damage of a reload (the twin cannon: 2 x 30 where it fired 60; the automated gun: 4 x 5
where it fired 20). Seven hitscan weapons are now `Instant` (they were `Straight` placeholders).

**Sim.** The weapons are a table of their own (`WeaponTable`: the weapons and which kind carries which, in slot
order), not new fields of `UnitStats`, so the digests that the pinned scenarios fix (`UnitTable::digest`, the
reach digest, the turret digest) are untouched; `World::with_weapons` checks the table against the unit table (a
loadout for a missing kind, a first weapon whose range is not the kind's `weapon_range`, a kind with a reach and
no weapon, a turret mount on a kind without a turret). A world that is not given a table has no weapons and hashes
nothing about them. State per entity and slot: `wait` (ticks until it may fire) and `burst_left`. They count down
whatever the unit is doing, so changing orders does not reload a weapon; a burst that cannot go on is dropped and
the weapon waits the tail of the reload.

**The placeholder, and why it is in WP5.1.** The brief asked for the data to be exercised. Reload state with
nothing that fires could only be tested by poking the entity columns; a unit with an `Attack` order that is in
reach, reloaded and aimed fires an instant hit that takes `damage` off the victim's hit points (the hits of a tick
land together, saturating at zero; every shot is decided from the hit points at the start of the tick, so the outcome
of a fight does not depend on the slot order, review round 1), and a `Shot` is recorded for the tick (`World::shots`, `StepReport::shots`; derived, not hashed). Not
done there on purpose: the projectile kind, the damage type, the splash and the armor (WP5.2, WP5.3); any order
other than `Attack`, any choice of target (WP5.4); removing a victim at zero (WP5.5: it stays, is not shot again,
and the attacker keeps its order); a check of whose unit the victim is. `sf_game` does not give its world a table
yet: nothing there would show a shot and a unit shot to zero would stand as before.

**Checksums.** The weapon section is last and written for as long as the world has a weapon table (the table's
digest, then every entity with a weapon that is not at rest), so peers with different tables differ from the first
tick (review round 1; it first appeared only while a weapon was busy). A world without a table hashes as before. No pinned value
moved: `wander_5000` 0x38d5e3c43358a889, `command_mix` 0x43f6583004258ceb, `terrain_256` 0xb577112845e0097e,
`units_spawn` 0x6c369f42be150d6e, `bridge_200` 0x03971fcc3548fc0c, and `orders`, `terrain_costs`, `pathing`,
`pathing_256`, `crowd_jam`, `turning`, `replan` and `formation_move` (`orders`, `turning` and `replan` have attack orders in them but no
`combat: true` and so no table). New: `weapons_reload` (below).

**`weapons_reload`** (`assets/scenarios/weapons_reload.ron`, a frozen copy of the definitions in
`assets/scenarios/weapons_defs/`, 64 x 64 cells, 600 ticks, `combat: true`): three riflemen on a power plant, a
light tank and a heavy tank on a heavy tank, a howitzer on a heavy tank ten cells off, a howitzer two cells from a
rifleman, a rifleman on a scout. Final checksum `0x5a1bca1ebf1efa16`. The run, read off the shots: each rifleman
fires every 20 ticks (27 shots each from tick 79 or 72, 81 hits on the plant, which ends at 114 of 600); the
light tank every 40 ticks from tick 23 (35 each, 11 shells); the heavy tank in pairs, the shells 5 ticks apart and
the pairs 40 (30 each, 18 shells, the last pair at 396 and 401); the first heavy tank is at 10 hit points after
tick 401 and at **0 at tick 423** by the light tank's shell (925 points of fire into 900, 25 over), after which the heavy
tank's next pair, due at 436, is not fired; the howitzer every 100 ticks from tick 9 (6 shells of 90, the target
at 360 of 900); the rifleman on the scout shoots at ticks 1, 21, 41, 61, 81 (25 to 19, 13, 7, 1, 0) and no more; the
howitzer inside its minimum range never fires; and a rifleman that is stopped at tick 200 and told to attack again
at tick 205 fires at 219 with the others, not at 205. The same scenario without `combat` fires nothing.

**Deferred, and why.**

1. **Projectiles, armor, splash, target choice, destruction** are WP5.2 to WP5.5.
2. **`projectile_speed` is not required** for a kind that flies. The frozen copies predate it, and making it a
   load error would mean editing five frozen directories in this PR; the shipped file is complete (a test checks it).
   WP5.2, the first to read it, decides whether to require it and refresh the copies.
3. **A hull-mounted weapon on a unit with a turret is never aimed** by the placeholder: `Order::Attack` leaves the
   turning to the turret for a unit that has one. No shipped unit has such a weapon. WP5.4 decides who turns what.
4. **Zero hit points mean two things**: a kind that cannot be damaged (hp 0 in the table) and one that has been
   shot down. The placeholder treats both as "not a target"; WP5.5 needs to tell them apart (a flag, or a removal).
5. **`UnitStats::weapon_range` and the weapon table both carry the first weapon's range.** The world refuses a
   table where they differ; one of them goes when the target choice (WP5.4) reads the table for reach.
6. **`sf_game` has no weapons yet** (above): one line in the world setup once WP5.4 and WP5.5 land.
7. **For WP5.2: the rocket cannot turn inside its own range.** With `guide_turn: 4` facing steps a tick at 12 cells
   a second the turning radius is about 6.1 cells, larger than the rocket's range of 5; a guided rocket that has to
   turn more than a few steps will miss. Raise `guide_turn`, lower `projectile_speed`, or let WP5.2 give the rocket a
   launch direction; decide when the flight is built (review round 1).
8. **`orders`, `replan` and `turning` will move once combat is on.** They have attack orders in them and run
   without `combat`, so they are pinned as before; with `combat: true` the reviewer measured them ending at
   `0x2397af78accc9214`, `0xbffd31b0686836f6` and `0x10f846dd7d437bdb`. Whichever WP turns combat on for them
   (WP5.4, when units choose targets and sf_game gets its table) re-pins them with the reason (review round 1).
9. **The three follow-ups of the WP4.6 review** are items 11 to 13 of the open items in the Phase 4 close-out.

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
