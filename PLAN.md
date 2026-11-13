# Staalfront — build plan

*Working title. A real-time strategy game in Rust, 2.5D, with seamless zooming from
close-up to the whole map and cartoon cutscenes. Inspired by the game rules of
Command & Conquer: Red Alert, without its name, world, units or media.*

Drawn up 14-10-2026. Source: `docs/red-alert-analysis.md` (functional analysis of
the EA source code). Execution: Sonnet 5 agents at effort medium, [xhigh] work packages on Opus at effort low, fix rounds on Opus at effort low (see §12; fix rounds since 24-10-2026, [xhigh] since 25-10-2026).

---

## 0. Decisions up front (record before phase 0)

| # | Question | Proposal | Why |
|---|---|---|---|
| B1 | Engine | **Own thin engine on wgpu + winit**, no off-the-shelf engine (see §1.0) | An RTS has one camera, one kind of world and thousands of identical objects: exactly what you control tightly yourself with instancing. Full control over determinism, tick order and memory. |
| B2 | What "2.5D" means here | **True 3D world, fixed camera angle, low-poly models** | Sprites (like the original) get ugly with seamless zooming. 3D models stay sharp up close and simplify cleanly from afar. Gameplay stays flat (2D grid). |
| B3 | Simulation | **Deterministic, fixed tick (20 Hz), fixed-point math, separate from rendering** | Enables replays, saving, multiplayer (lockstep) and automated tests. Adding it afterwards is nearly impossible. |
| B4 | Cutscenes | **In-engine cartoon player (cutout/vector animation, scripted) + video as fallback (deferred, WP12.4)** | Small, sharp at any resolution, lip-sync and translation adjustable afterwards. See §9. |
| B5 | License of the source-code reference | **Clean-room**: use only the functional analysis, never read-and-retype EA code | EA code is GPL; copying it verbatim makes Staalfront GPL. Own name, units, graphics, music are mandatory (trademark). Exception for original audio: B11. Since 26-10-2026 `onderzoeker` and `reviewer` may check single facts against EA/OpenRA sources and pass them on in their own words, never text, tables or code; a builder never reads those sources and may write a single checked fact into the repo, but never a list or table taken over as a whole or in its structure (docs/conventions.md §8). |
| B6 | Where it is built | **GitHub + cloud agents + laptop**, never on the home server (see §13) | The home server runs live services and has no room (RAM, GPU) for a Rust game build. |
| B7 | Target platform v1 | Windows + Linux desktop. Web and macOS later. | Mouse + keyboard; an RTS on a phone is a different game. |
| B8 | Language of the game | **English** (menus, UI texts, voices, subtitles); all game texts in language files, not in code | Decision Tijmen 14-10-2026. Language files keep translating possible later. |
| B9 | Language of code and documentation | **English**: code, comments, docs, commit messages, PRs | Decision Tijmen 14-10-2026. |
| B10 | Reference hardware | **Windows laptop with NVIDIA GTX 1650**, 1920×1080 | Decision Tijmen 14-10-2026, card corrected 15-10-2026 (the laptop has a GTX 1650, not a 1050). The fps requirements apply on this card; VRAM budget 2 GB kept (headroom below the 1650's 4 GB). |
| B11 | Original Red Alert audio (sound effects, voices and music) | **Optional, loaded at runtime from the player's own Red Alert files** (like OpenRA); never in the repo, a release or a CI artifact | Decision Tijmen 14-10-2026; music included by decision Tijmen 28-10-2026. EA released the code, not the media; the 2008 freeware download does not allow redistribution. Reading the player's own copy keeps the repo clean. Staalfront always works without it (CC0 sounds are the default; music falls back to the default music: CC0, or silence if there is none). See WP11.7 and WP11.4. |
| B12 | Cell size | **1 gameplay cell = 4 m**; maps 64×64 up to 256×256 cells (256 m up to 1,024 m). The visual terrain keeps its 1 m resolution (heightmap and materials per metre); gameplay data (terrain type, occupancy, pathfinding, building footprints, fog) is per 4 m cell. | Decided by the orchestrator on Tijmen's instruction, 17-10-2026. Vehicles are 4–8 m long and buildings 4.5–14 m (test pack v1 `manifest.json`), so a tank takes about one or two cells and a building 2×2 to 4×3 cells, the proportion RTS games are built on. At 1 m per cell a 256-cell map is only ~35 tanks wide and buildings take 20–154 cells; growing the map instead would multiply pathfinding and fog work by 16. The engine already handles a 1,024 m terrain (WP1.8 test on mirrored synthetic hills: 17 terrain draw calls at whole-map zoom). **Scale at the boundary:** sim positions are in cells (`Fx::ONE` = 1 cell); the presentation converts with one constant, 4 m per cell, at the sim→render boundary, and the terrain is 4 samples per cell side (done in WP1.14). Revisit only with a measured reason. |

---

## 1. Architecture

### 1.0 Technology choice

No Bevy or Fyrox: those are general-purpose engines. Bevy breaks its API every
quarter, schedules systems itself (hard for bit-exact determinism) and compiles
slowly; Fyrox brings a complete editor and scene graph that an RTS does not
need. An RTS is narrow by nature: one camera, one terrain, many identical
objects. Hence separate, stable building blocks:

| Component | Choice | Why |
|---|---|---|
| Window/input | `winit` | The standard, stable. |
| GPU | `wgpu` | Vulkan/DX12/Metal/WebGPU from one codebase; instancing for thousands of units. |
| Render math | `glam` | Fast, only in the presentation (sim uses fixed-point). |
| Simulation storage | **own struct-of-arrays with generation IDs** (no ECS framework) | Fixed, predictable order = determinism; cache-friendly; no magic. |
| Models | `gltf` | Open format, Blender export. |
| In-game UI | own, drawn with wgpu (sidebar, buttons, minimap) | Small and tight in style. |
| Editor/debug UI | `egui` (via `egui-wgpu`) | Quick to build, only for tools and debug panels. |
| Audio | `kira` | Mixing, positional, transitions for dynamic music. |
| Data | `serde` + `ron` | Readable game data. |
| Cartoon video fallback | deferred (WP12.4); when picked up: `rav1d` + a Matroska demuxer + libopus, benchmarked against VP9 on the reference laptop | Research 17-10-2026: an `ffmpeg` process means shipping an LGPL binary and installer work; no clip exists yet. |
| Tests | `proptest`, `insta` (snapshots) | Properties and regressions. |

### 1.1 Cargo workspace

```
staalfront/
├─ crates/
│  ├─ sf_math      fixed-point numbers, vectors, angles, deterministic RNG
│  ├─ sf_sim       the complete game simulation — no GPU, no I/O, no clock
│  ├─ sf_data      unit/building/weapon definitions (RON), validation, loading
│  ├─ sf_bridge    copies the sf_data definitions into the sf_sim tables (unit stats, terrain,
│  │               movement costs) at world creation; sf_sim and sf_data may not know each other
│  ├─ sf_ai        computer opponent: acts only via commands, reads only the fog-filtered observation
│  ├─ sf_engine    main loop, window, input, time, assets (no game rules)
│  ├─ sf_render    wgpu: terrain, models, camera, zoom, effects, LOD
│  ├─ sf_ui        own UI: widgets, text, minimap; later sidebar, selection, menus
│  ├─ sf_audio     sound, voices, music
│  ├─ sf_cutscene  cartoon player + script format
│  ├─ sf_net       lockstep network (later)
│  ├─ sf_thread_cpu  CPU time of the calling thread (Windows `GetThreadTimes`); the only crate allowed unsafe code
│  └─ sf_game      binary app: glues everything together
├─ tools/
│  ├─ sf_editor    map and mission editor
│  ├─ sf_headless  simulation without a screen: tests, balance, AI-vs-AI
│  ├─ sf_demo      engine demo and benchmark (phase 1)
│  ├─ sf_asset_check  checks delivered .glb models against the art brief (§10)
│  ├─ sf_icon_gen  writes the placeholder icons and cursors (assets/icons, assets/ui/cursors)
│  └─ sf_ra_extract  turns the player's own Red Alert sounds into WAV (runs only on their machine)
├─ assets/  data/  docs/
```

**Hard rule:** `sf_sim` does not depend on `sf_render`. The simulation must be able
to run thousands of ticks per second in `sf_headless` without a GPU. That is
what lets agents test gameplay without a screen.

### 1.2 Simulation and presentation separated

- The simulation runs at 20 ticks/s with a fixed step. Input becomes a **command**
  (`Command`) that is executed on tick N — never a direct world mutation
  from the UI.
- Rendering runs at the screen refresh rate and **interpolates** between the previous and
  current tick.
- Every tick yields a **checksum** of the world state (for desync detection
  and regression tests).
- Game objects are entities in the struct-of-arrays storage (§1.0) with columns
  (position, owner, health, mission, movement, weapon…). Behavior per type via data, not via subclasses —
  that is the modern translation of the object hierarchy from the analysis (§3).

### 1.3 Coordinates

- Gameplay on a **cell grid of 4 m cells** (B12), maps 64×64 up to 256×256 cells
  (256 m up to 1,024 m); positions within a cell in fixed-point ("leptons", like the
  original). The visual terrain stays at 1 m resolution: 4×4 terrain samples per cell.
- Height is **visual only** plus a few gameplay effects (vision from a
  hill, slower uphill). No true 3D ballistics in v1.

---

## 2. Camera and seamless zooming (core requirement)

Goal: with the scroll wheel, smoothly go from **close-up on a single tank** to **the whole
map in view**, without jolts and without the controls breaking.

### 2.1 Camera
- Perspective camera with a fixed view direction (north up). Rotating with Q/E
  optional and off by default.
- Zoom = camera distance on a logarithmic scale (e.g. 12 m → enough to show the
  whole map). Scrolling zooms **toward the cursor** (the world position
  under the mouse stays under the mouse).
- **Angle follows zoom:** up close ~45° (cinematic, you see the sides
  of buildings), from afar toward ~80° (almost top-down, easy to read).
- Smooth damping, panning with edge scroll, WASD/arrow keys and
  middle mouse button drag. Pan speed scales with zoom.
- Double-click on minimap or space = go to last event.

### 2.2 What changes per zoom level
| Zoom | Units | Terrain | UI |
|---|---|---|---|
| Close-up | full models, animations, particles, shadows | full detail, decoration | health bars on selection |
| Middle (default play height) | full models, fewer particles | medium LOD | bars on damage |
| Far | simplified models (LOD) | coarse chunks | group bars |
| Strategic (the last 5 % of the zoom range, up to the whole map plus a full map size of margin) | **the same models, with strategic icons on top** (player color; triangle = vehicle, square = building, etc.), constant size on screen, fading in over the band | map-like display | area overlays, route arrows |

Models are drawn at every zoom; the icons are an overlay that fades in over the farthest band, not a replacement
for the models (see the note below).

*Note 08-11-2026 (decision Tijmen, after playing M1; `docs/rules/camera.md`): "I must be able to zoom out further,
and I keep seeing the buildings and the little soldiers." The farthest zoom now shows the whole map plus a quarter
map of margin on every side (`camera.far_margin`, `assets/config/camera.ron`). The model-to-icon swap of the first
version of this table is gone: models stay at every zoom (a modest, capped scale-up toward the far end keeps them
readable), and the strategic icons are drawn on top of them in the last ~15 % of the zoom range, fading in
(`zoom.icons_from`/`icons_full`). The click volume is the icon quad where the icon is mostly opaque, the model
elsewhere (section 2.3 is unchanged in that). All numbers are in RON.*

*Note 10-11-2026 (fix round 1 of the zoom PR, orchestrator decision following Tijmen's "follow your own
proposals"): the icons are drawn at most 60 % opaque (`zoom.icons_max_alpha`), so the model stays visible through
them at the farthest zoom; the switch of click volume, ring and bars and the effects follow the uncapped band
position. The click volume, the ring and the bars follow the drawn (scaled) model size instead of the true size.*

*Note 10-11-2026 (fix round 2 of the zoom PR; Tijmen after playing: "I want to zoom out much farther; they still
turn into icons far too soon"; orchestrator decision under his rule "your proposal holds if I do not answer"): the
farthest zoom shows the map plus a full map size of margin on every side (`camera.far_margin` 1.0); the icons come
only in the last 5 % of the zoom range (`zoom.icons_from` 0.95) and at most 40 % opaque (`icons_max_alpha` 0.4);
the models grow up to 2.5 times toward the far end (`model_scale_max`, 2.5 is the cap); the key K (`ToggleIcons`,
on by default) switches the icons off, models only at every zoom.*
*Fix round 3, same day: structures have their own far-zoom scale (`zoom.structure_scale_max` 1.6; units stay at 2.5),
because at 2.5 neighbouring structures overlapped by up to 13 m and a click could take the wrong one.*

### 2.3 Controls work at every zoom
- Selecting via ray → terrain + a click volume per entity; at
  strategic zoom the **icon quads** on the screen count.
- Drag selection works in screen space and thus at every zoom.
- Commands (move, attack) work unchanged even fully zoomed out.

### 2.4 Performance
- Terrain in chunks with LOD and frustum culling.
- Units via GPU instancing per model type.
- Goal: **60 fps with 1,000 units** in view, zoomed out, on the
  reference hardware (B10, GTX 1650); first measurement at the engine demo (phase 1, 5,000 units), final measurement in phase 16.

---

## 3. Phases

Every phase ends with something you can **play or see**. Work packages (WP)
are cut so that one agent finishes one in a single session. Acceptance is
always testable: a test, a headless scenario or a screenshot.

Engine first, then the game: phases 0 and 1 contain **no gameplay**. Only when
the engine demo (milestone M0) demonstrably runs does phase 2 begin.

### Phase 0 — Foundation
- WP0.1 Workspace with all crates (empty), `cargo fmt`/`clippy`/`test` green, GitHub Actions (Linux + Windows), lint that rejects `f32`/`f64` and `HashMap`/`HashSet` in `sf_sim`.
- WP0.2 **[xhigh]** `sf_math`: fixed-point type, vectors, angles (256 directions), deterministic RNG; property tests (proptest) and a test proving that the same sequence is bit-identical on Linux and Windows.
- WP0.3 `docs/`: architecture, conventions for agents (see §12), the analysis as source.

**Done when:** CI green on two OSes; `sf_math` bit-identical on Linux and Windows.

### Phase 1 — Engine
A thin, own engine (§1.0) with everything the game will need later, tested
with placeholders. Engine crates know no game rules: no "tank", no
"ore", only meshes, instances, entities, commands and checksums.

*Core*
- WP1.1 `sf_engine` — main loop and input: winit window (windowed/fullscreen, DPI scaling), loop with fixed sim tick (20 Hz) and rendering at screen refresh rate with an interpolation factor, input layer (keys/mouse/wheel → actions, mapping in RON), frame time measurement, asset paths and hot-reload of RON in debug.
- WP1.2 **[xhigh]** `sf_sim` core: world as struct-of-arrays with generation IDs, fixed tick, `Command` queue on tick N, checksum per tick. One placeholder behavior "wander" (fixed-point, deterministic RNG) so thousands of entities move without gameplay.
- WP1.3 **[xhigh]** `sf_headless`: runs N ticks of a scenario file, writes checksums and a JSON log; CI compares the checksums of Linux and Windows (10,000 ticks, 5,000 wanderers).

*Presentation*
- WP1.4 `sf_render` basics: wgpu device and surface, depth buffer, one directional light + ambient light, WGSL shaders with hot-reload in debug, offscreen rendering to PNG for screenshot tests (Linux: Mesa lavapipe; Windows: DX12 WARP).
- WP1.5 Camera per §2.1: logarithmic zoom from 12 m to the whole map, zoom to the cursor, angle follows (~45° → ~80°), damping, panning (edge scroll, WASD/arrow keys, middle mouse button), pan speed scales. Unit test: the world point under the cursor stays within 1 cm at every zoom step.
- WP1.6 Instancing: one instance buffer per mesh, positions interpolated per frame from two sim ticks, placeholder primitives (cube, cylinder, cone) in player color, CPU frustum culling per map tile. Stress test: 10,000 instances.
- WP1.7 Terrain: engine map format (RON + heightmap, material index per 1 m terrain sample), mesh in chunks of 32×32 samples, procedural materials via a texture array.
- WP1.8 **[xhigh]** Terrain LOD and culling: LOD levels per chunk based on screen error, seams closed (skirts or stitching), frustum culling; zoomed out on a 256×256-sample terrain ≤ 300 draw calls (also checked on 1,024×1,024 samples, the B12 maximum).
- WP1.9 **[xhigh]** Model LOD and strategic icons: LOD chain per model, and at strategic zoom screen-fixed icons (shape + player color) with crossfade (§2.2); click volume per zoom (ray on model, icon quad on screen).
- WP1.10 glTF models: loading of mesh, base-color material and node hierarchy (turret separate from hull), rigid node animation; hooking into instancing and model LOD. Skinned animation deliberately not yet. Axis and unit conversion happens once, in the loader: glTF (right-handed, Y up) is converted to the engine convention, which this WP fixes in `docs/conventions.md` (proposal: metres, Y up, front +X, ground at Y=0). The conversion is applied to vertices, normals, node matrices and rotations; the renderer and shaders never correct axes. A test model with a known orientation must prove front, up and handedness. The loader also merges a part's per-material nodes into one mesh (one draw per part per LOD) and strips `_LOD*` suffixes from part names (`assets/testpack_v1`). (Idea borrowed from BRUTAL's `GltfLoader.Transform.cs`; no code taken, licence not compatible.)

*Interface and sound*
- WP1.11 Debug panels with `egui` (via `egui-wgpu`), toggled with F1: frame time graph, draw calls and instances, camera settings adjustable live, sim tick and checksum, LOD coloring, cell grid overlay.
- WP1.12 `sf_ui` — own UI on wgpu: rectangles, nine-slice, text (glyph atlas), buttons, panels with anchors, UI scale, input that the UI receives first; minimap (render to texture, click = move camera, frame of the field of view).
- WP1.13 `sf_audio` with `kira`: sound bank in RON, positional sound relative to the camera (volume and panning, quieter when zooming out), music with crossfade; without an audio device (CI) run on silently instead of crashing. One CC0 test sound with a license line.

*Demo*
- WP1.14 `sf_demo` — the engine demo: generated 256×256-cell map (1,024 m, B12) with relief; introduces the one sim→render scale constant (4 m per cell) and a terrain of 4 samples per cell side, replacing the current cell = metre assumption in `sf_game` (map size passed to the sim, `render_bridge`, instancing, debug-UI grid labels); 5,000 wandering placeholder units (3 mesh kinds, 4 player colors, a few glTF test models), minimap, egui panels, sound on clicks. Benchmark mode `--bench`: fixed camera flight from close-up to full overview and back, writes frame times (p50/p95/p99) and the sim checksum to JSON. In CI: runs on lavapipe/WARP as a smoke test with screenshots at three zoom levels (no fps requirement, that hardware is software).

**Order and parallelism:** WP1.1 and WP1.2 first (parallel). Then three tracks
side by side: *sim* (1.3), *presentation* (1.4 → 1.5 → 1.6 → 1.7 → 1.8 →
1.9 → 1.10) and *interface* (1.11, 1.12, 1.13, as soon as 1.4 exists). WP1.14 as
the last.

**Done when (milestone M0 — engine demo):**
- `sf_demo --bench` achieves on the reference laptop (GTX 1650, 1920×1080) **p99 frame time ≤ 16.7 ms (≥ 60 fps)** over the entire camera flight with 5,000 units in view, from close-up to the whole map;
- smooth zooming to the cursor without jolts, judged by Tijmen on the laptop;
- headless: 10,000 ticks with 5,000 wanderers, checksum equal on Linux and Windows;
- CI green on two OSes, including screenshot tests;
- egui panels show frame time, draw calls and checksum live.

### Phase 2 — Playfield
The first gameplay work: the map gets meaning for the game.
- WP2.1 Map format (RON) extended with terrain type per cell (grass, road, water, rock, ore, bridge) in `sf_data`; `sf_sim` knows terrain type and occupancy per cell.
- WP2.2 Materials per terrain type in the presentation (on the engine terrain of WP1.7).
- WP2.3 Minimap shows terrain types (on the minimap of WP1.12).
- WP2.4 Cell grid overlay colors by terrain type and occupancy.

**Done when:** a 256×256-cell (1,024 m) playfield loads in sim and presentation; headless test checks terrain types and occupancy per cell.

**Laptop benchmark M0b (decision Tijmen 22-10-2026):** after WP2.4 Tijmen runs `cargo run --release -p sf_demo -- --bench` again on the reference laptop (GTX 1650, 1920×1080); the p99 frame time must stay ≤ 16.7 ms with the terrain materials, minimap and overlay in. Baseline: M0, p99 3.9 ms (`docs/bench/m0-2026-10-22.json`). Met: no run was made right after WP2.4; the plain run of 28-10-2026 (same scene, later main, p99 3.993 ms, `docs/bench/plain-2026-10-28.json`) counts as M0b (decision Tijmen 31-10-2026).

### Phase 3 — Units, selection and commands
- WP3.1 `sf_data`: unit definitions (speed, turn rate, movement class, health, armor, weapons, cost, model).
- WP3.2 Units spawn in the sim from `sf_data` definitions; presentation via the engine (instancing, interpolation, LOD from phase 1).
- WP3.3 Selection: click, drag, shift-add, double-click = all of this type in view, groups Ctrl+1–9.
- WP3.4 Commands as state (analysis §5): stand still, move, attack, attack-move, guard, stop, scatter. Queue with shift.
- WP3.5 Cursor context: the cursor shows which command a click gives.
- WP3.6 Icons per unit type and faction on the icon system of WP1.9.

**Done when:** you select and move units at any zoom; commands are sim commands and thus reproducible in headless.

### Phase 4 — Movement and routes
- WP4.1 Movement classes (tracked, wheeled, foot, hover, water, air) with terrain costs.
- WP4.2 **[xhigh]** Hierarchical A* (sectors/portals) for single units.
- WP4.3 **[xhigh]** Flowfields for groups toward one goal.
- WP4.4 **[xhigh]** Local avoidance and collisions (units push, wait, detour); stuck unit to a free neighbor cell (analysis §6).
- WP4.5 Turn before driving for vehicles, turrets turn separately from the hull.
- WP4.4b **[xhigh]** Robust group arrival (follow-up of the WP4.5 review): the "done when" below holds for every wheeled turning radius from 1.25 to 2.0 cells, not by chance; the shipped radius becomes a realistic 1.5.
- WP4.6 Formations (group keeps shape, slowest sets the pace).
- WP4.7 Replan on blocked route or disappeared target; destination ≠ route.

**Done when:** 200 units cross a bridge in a headless scenario: all arrive, none stuck; ≤ 1 % end further from the goal than 1.3× the radius of a disc whose area is the sum of the units' spacing² (bodies on a square lattice); no two bodies overlap by more than ¼ cell during the run or 1/16 cell at the end.

_Note: this replaces "≤ 1 % outside 5 cells (20 m)" by the orchestrator decision of 18-10-2026, because bodies make 5 cells physically impossible (the packing radius for `bridge_200` is 7.37 cells)._

### Phase 5 — Combat
- WP5.1 Weapons and bullet types as data: range, reload time, damage, damage type, projectile type.
- WP5.2 Projectiles as separate entities: straight, arcing, guided, falling (analysis §8), with fuse/detonation.
- WP5.3 Armor table: damage × armor type → percentage; splash damage with falloff.
- WP5.4 Target selection and automatic return fire, priorities, range check.
- WP5.5 Destruction: free up cells, wrecks/rubble, effects.
- WP5.6a Visual effects, part one: an effects event queue (lossless, sim thread to picture), a particle pool and one instanced pipeline, `assets/config/effects.ron` (muzzle flash, impact, explosions, smoke by damage type and weapon id), zoom behaviour (pixel clamp, fade with the icon cross-fade, fewer particles at middle zoom, a death flash kept at strategic zoom) and projectile tracers. Presentation only: `sf_sim` is not touched.
- WP5.6b Visual effects, part two: muzzle and fx anchors in the glb loader (the flash starts at the model's muzzle node) and `sf_demo --bench --fight` (the combat scene for the M0c benchmark).

_Note 27-10-2026: WP5.6 ("visual effects: muzzle flash, impact, explosions, smoke (particles), scale with zoom") is split into WP5.6a and WP5.6b. Decided by the orchestrator with Tijmen's approval. Part one needs nothing from the model loader and can be built and tested on the finished combat; part two needs the loader and the benchmark scene. The M0c benchmark (below) runs after WP5.6b._

**Done when:** headless "10 tanks vs 20 infantry" gives the same outcome and checksum every run; it looks that way in the game too.

**Laptop benchmark M0c (decision Tijmen 22-10-2026):** after WP5.6b (was: after WP5.6) the same laptop run again, with combat and effects in the demo scene; p99 frame time ≤ 16.7 ms and `sim.tick_ms` p99 well within the 50 ms tick, compared with M0 and M0b. Met 28-10-2026: p99 4.42 and 4.35 ms, `sim.tick_ms` p99 3.2 ms, compared with M0 and with the plain run of the same day that counts as M0b (`docs/bench/README.md`, "M0b and M0c result").

### Phase 6 — Buildings, building and power
- WP6.1 Building definitions: footprint, build time, power, requirements (tech tree), function.
- WP6.2 Placement: validity (terrain, proximity to own base), ghost preview under the cursor.
- WP6.3 Build animation tied to state — animation before mission logic (analysis §10).
- WP6.4 Power: generation/consumption, shortage slows production and switches off defenses; damage lowers generation.
- WP6.5 Production: queues per factory type, progress, exit + free spot, rally point.
- WP6.6 Repair and sell as processes with progress.
- WP6.7 Defensive buildings with turrets.
- WP6.8 Damage display: damaged and heavily damaged model, smoke, fire.
- WP6.9 Controls for production buildings: a rally point by right-click and a sidebar tab that follows the selected factory (UI only, no `sf_sim` change).
- WP6.10 Repeat toggle ("herhaalknop") per factory: a finished item goes to the back of the queue (sim: a command, hashed state; no existing scenario is re-pinned, the flag hashes only while it is on, one new scenario `production_repeat`).

_Note 11-11-2026 (WP6.10, decision Tijmen 10-11-2026, details by the orchestrator under his rule that his proposal holds if he does not answer): a per-factory repeat toggle, default off. With repeat on, a finished unit goes back at the end of that factory's queue at zero progress, through the checks of a new `QueueUnit` (requirements, queue limit, which stays 5: the finished item leaves before its copy goes in). Money has no new rule: items pay per tick and wait in `WaitingForMoney`; repeat does not switch itself off without money. The free harvester is never repeated; a cancelled item does not come back; turning repeat off lets the queue run out. `SetRepeat { factory, on }` (tag 18) is refused for anything that is not the sender's own standing factory (refusals of `SetRally`, no new kind); `repeat` is hashed with the rally point of each factory. UI: a loop button in the sidebar row next to repair/sell, for the primary factory of the open tab and lit when on; a repeating factory shows a small loop sign on its queue item. No key._

_Note 13-11-2026 (WP9.1e, orchestrator): `hp_per_tick` is 1 now, not 3 (see the Phase 9 note and `docs/rules/repair_sell.md`, "The rate and the wave"): repair of a structure without a price (the construction centre) is free, and at 3 it out-healed up to five idle light tanks (six took 6,052 ticks; a best-case fixture). A repair of a priced structure costs the same as before; it takes three times as long._

_Note 10-11-2026 (Tijmen after playing M1: "ik mis dat je een gebouw aanklikt en dat je een marker neer kunt zetten waar ze heen moeten als ze gebouwd zijn" and "als ik barak kies kan ik ook gelijk menu voor de soldaten"; the decisions below are the orchestrator's, under Tijmen's rule that his proposal holds if he does not answer): **WP6.9** is UI only and uses the existing sim command `SetRally`. (1) With own standing factories selected, a right-click on the map sends `SetRally { cell: Some(cell) }` for each of them; a right-click on one of the selected factories sends `cell: None` (clear); the cursor shows a rally cursor over the ground (and a struck-through flag over the factory), a cell in shroud is allowed like a move order, off the map gives no command. (2) A mixed selection: the units get their normal order and the factories rally to the same cell (as in StarCraft II); a right-click on an enemy makes the units attack and the factories set nothing; a right-click on a selected own factory makes the units guard it and leaves its rally point alone (a clear is only done with factories alone selected; review of PR #116, 11-11-2026). (3) The rally flag stays; a line from the factory's exit cell to the flag is drawn only while that factory is selected, its colour and width in `production_look.ron`; own flags are always shown. (4) When the selection changes and holds an own factory, the sidebar shows that factory's tab (the first one in the stable selection order, which is also its primary); only on a change of the selection, never every frame, so a tab picked by hand stays; units only leave the tab as it is. (5) The debug key G stays. (6) A rally right-click raises no unit voice; with a mixed selection the units' order voice plays as for a plain order. **WP6.10** (repeat) follows, decided by Tijmen on 10-11-2026: a "herhaalknop" per factory (a repeat toggle, not a longer queue); it needs a sim command and re-pins, so it is a separate work package._

_Note 31-10-2026: WP6.5 (production) is built before WP6.4 (power), decided by the orchestrator. Milestone M1 ("build a base, harvest, make units, attack, win") needs units to be made and does not need power. WP6.4 later hooks into production through a speed factor that WP6.5 leaves as a documented hook, fixed at 1 (`docs/rules/production.md`)._

### Phase 7 — Economy
- WP7.1 Ore on the map: fields, amount, slow regrowth.
- WP7.2 Harvester: search, harvest, drive back, check in at refinery via **messages** (analysis §11, §13), unload, repeat.
- WP7.3 Storage capacity, fill level visible: a storage capacity per building in the building data (the refinery has one), the player's stored ore capped by the capacity of its standing buildings, a fill bar on the refinery and a stored / capacity readout.
- WP7.3b The silo as a building (a structure whose job is storage), after the tech tree in `docs/rules/buildings.md` is approved.
- WP7.4 Money in the UI, deducting costs during production.
- WP7.5 Refinery choice and queues: a harvester picks the refinery by expected wait (queue length plus travel), not by straight-line distance alone; queued harvesters wait clear of the approach, so many harvesters keep delivering.
- WP7.6 The M1 match: one small map with two bases, a static opponent and a fixed win and loss rule in the sim, so that M1 can be played: a match file (`assets/matches/m1.ron`: map, players, credits, starting objects), a defeat and win rule in `sf_sim` (a player is defeated when it owns no standing structure and no armed unit; the last one left wins; its own checksum section, only in a world with match rules), a result banner ("Victory" / "Defeat") and `sf_game --match`; the pinned headless scenario `match`.

_Note 27-10-2026: WP7.5 added by the orchestrator after the WP7.2 review measured that throughput collapses with many harvesters (6 harvesters deliver 3200 ore in 4000 ticks, 48 deliver 618): the straight-line choice sends every harvester to the same bay and the queue blocks its approach. WP7.2 records it as a gap; WP7.5 touches `sf_sim` and comes after WP7.3 and WP7.4._

_Note 28-10-2026: WP7.3 ("storage capacity and silos, fill level visible") is split into WP7.3 and WP7.3b. Decided by the orchestrator (Tijmen delegated WP splits to the orchestrator on 27-10-2026). The silo is a new structure role: the campaign uses none (`content/production.md`) and the tech tree in `docs/rules/buildings.md` still waits on Tijmen's approval. WP7.3 builds the storage mechanism and a storage capacity per building in the data, so a silo later is a record in `unit_defs.ron` with a `storage` number; WP7.3b adds that silo once the tech tree is approved._

_Note 01-11-2026: WP7.6 added by the orchestrator. PLAN.md put M1 at the end of phase 7, but no work package built the map, the opponent or the win condition. The full scenario format with triggers is phase 10 (WP10.1 to WP10.3) and M1 does not need triggers, so WP7.6 builds only the minimum: a match map file, a fixed win and loss rule in the sim and a static opponent (no AI before phase 9). WP10.1 later absorbs the match file into the scenario format. WP7.6 is built after WP6.5 and stacked on it (sf_sim work packages are built one after another, section 12.3); see `docs/rules/match.md`._

**Milestone M1 — vertical slice (end of phase 7):** one skirmish map, you against
a static opponent: build a base, harvest, make units, attack,
win. First moment to really play and adjust the design.

*08-11-2026 (Tijmen): **M1 reached.** Tijmen played the M1 match on the reference laptop (GTX 1650) with the original
Red Alert audio on (`--ra-audio`) and won; "went well". Decisions from that play:*
- *Loss rule: a player is defeated when it has **lost its construction center** (no standing `construction_center`),
  instead of "no standing structure and no armed unit" (hunting the last unit was tedious). Built in **WP7.7**.*
- *`acquire_needs_sight` (WP8.2a) goes **on** in the M1 match and as the default of the vision rules. Built in **WP7.7**
  together with the loss rule, because both move the pinned `match` scenario.*
- *The WP3.3 selection decisions (PR #44) are **confirmed** as built.*
- *Unit voices on select, move and attack were missed: **WP11.2 is next**, before phase 9.*

*08-11-2026 (Tijmen, after playing M1; and the orchestrator under his rule "your proposal holds if I do not answer"; built in **WP6.11**): (1) everything is built **1.5 times faster**, structures and units: one exact fraction `build_speed: (num: 3, den: 2)` in `assets/config/production.ron`, build ticks rounded up, the price still paid exactly (`docs/rules/production.md`, "Build speed"). (2) The sandbox demo base (`--harvest`) gets enough power plants that it makes at least what it uses (it stood at 270 used of 100 made). (3) The ore silo's place in the tree, **requires refinery**, was approved on 08-11-2026 (WP7.3b follow-up): the sandbox now places it.*

*11-11-2026 (orchestrator, follow-up of #115; the date of the note above is corrected: Tijmen decided on 08-11-2026, PR #115 review point 5): his decision was "everything builds 1.5 times faster", so the **plain sandbox** (no `--harvest`, no production rules) builds at 3 / 2 as well. The proposal holds unless Tijmen says otherwise: the game scales the sandbox's building table by the `build_speed` of `production.ron` (`playfield::with_sandbox_buildings`), the sim is untouched, and the sidebar shows the same scaled ticks (`docs/rules/production.md`, "Build speed"). The headless scenarios without a `production` block keep the data's ticks.*

**Queue for the orchestrator (set 08-11-2026), in this order:**
1. **WP7.7** The loss rule and sight-gated targeting from the M1 play, in `sf_sim`: (a) a player is defeated when it owns no standing `construction_center`; the last player left wins; a player that never had one in its match file is a match-file error, checked when the file loads (`sf_data::match_def`); (b) add a `construction_center` to the opponent in `assets/matches/m1.ron` (it has none today), placed so the base still makes sense; (c) `acquire_needs_sight` on by default and in `m1`; (d) re-pin `match.expected` and update `tools/sf_headless/tests/match_m1.rs`, with the change of the winning tick explained in the PR; (e) `docs/rules/match.md` and `docs/rules/vision.md` follow. The old rule stays nowhere (no flag). Checksum section unchanged in layout.
*08-11-2026 (orchestrator, with Tijmen: "follow your own proposals"; built in WP7.7): the #105 measurement showed that with the gate on a unit whose sight equals its weapon reach loses targets at the limit of its reach (the rocket trooper, sight 5 and reach 5, did not pick a tower at squared distance 26). Decision: **the visibility test for target acquisition is measured to the target's edge, like the reach** (a structure: any footprint cell or a visible cell beside it; a unit: its own cell or a neighbour cell its body reaches), in integers, before the gate goes on, so M1 does not get that regression. Rules: `docs/rules/vision.md`, "Measured to the edge".*

2. **WP11.2** *(built 09-11-2026, PR in review; `docs/rules/voices.md`)* Voice responses (see Phase 11), built now: `sf_game` raises `unit.select`, `unit.move` and `unit.attack` (the bank events, mapped to Red Alert voices in `assets/audio/ra_mapping.ron`) on the player's own selection and orders, from the UI side (no sim event needed: these are the player's own actions), with a brake against spam (one voice at a time, a minimum gap, not on every box-select frame). Without the Red Alert files the events stay silent unless the bank has a CC0 sound. Test: the brake and which event each action raises, headless where the runner allows.
3. ~~**WP1.16 Stutter in play**~~ **Closed 11-11-2026** (see the note at the end of this item) *(10-11-2026, orchestrator, with Tijmen: "make a plan to fix it and put it on GitHub",
   after an M1 play on the laptop that stuttered; evidence in `docs/progress.md` follow-up 35: frames of 0.6 to 1.6 s
   with vsync on, 125 late snapshots)*. Built **before phase 9**. Measure first, then fix only what the measurement
   shows; no fix on a guess.
   - (a) **A hitch log in ordinary play**, not only in `--bench`: every frame over 50 ms writes one line with its parts.
     The runner times some of them today (snapshot convert, build+cull, instance upload with the buffer allocations,
     `render_ms`, which includes present). **New in this WP:** the event-loop gap before the frame (the time the
     main thread was not in the frame callback), surface acquire and present measured apart from the rest of
     `render_ms`, and the CPU time of the main thread in that frame. `std` has no thread CPU time, so on Windows it comes
     from `GetThreadTimes`, through a direct `windows-sys` dependency with the feature
     `Win32_System_Threading`, for Windows only. *(11-11-2026, WP1.16a as built: that dependency sits in the
     new crate `crates/sf_thread_cpu`, not in `sf_game`, because the workspace forbids unsafe code; the crate
     has `unsafe_code = "deny"` and one function with `#[allow(unsafe_code)]` and a SAFETY comment, every
     other crate keeps the workspace's `forbid`. `sf_game` now also has a normal dependency on `sf_headless`
     for the record format.)* That crate is already in `Cargo.lock` as a transitive dependency, and it
     is motivated here because suspect 5 needs to tell a thread that was not running from one that was busy. On other
     platforms the field reads "not measured". Plus what happened in the frame: window events (resize, move, focus,
     occlusion), a music track start or decode result with its time, the number of sound starts, and the tick number of
     a sim tick over 20 ms. The hitch log and the play's exit summary also report the **total number of frames and the
     number over `sf_game::bench::HITCH_MS`** (25 ms), as the benchmark does, so the done-when below can be read from
     the run. Also `--hitch-log FILE` (JSON), so Tijmen's next play hands back a file. Cost when nothing is slow: one
     comparison per frame.
   - (b) **The sim side, without breaking `sf_sim`'s rules.** The clock stays out of `sf_sim`: the sim thread in
     `sf_game` already times each tick and logs a tick over 20 ms with its number. A split per system is made on a
     **headless replay** with a sampling profiler (no code in `sf_sim`). A timing hook inside `sf_sim` is not part of
     this WP; if one turns out to be needed, it is Tijmen's decision (section 12.4). No checksum, pin or `.expected`
     moves. The replay comes from a **command record**, `sf_game --match --record FILE`:
     - The file is an `sf_headless` scenario. That format already has `match_file` and commands at later ticks
       (`tools/sf_headless/src/scenario.rs`).
     - Each command is written on the sim-thread side with the tick it **actually ran at** and its exact fixed-point
       values.
     - Every command the game can send is covered, or the WP adds it to the scenario format.
     - A test plays a recorded match to its end in the game's sim thread and in `sf_headless` and gets the **same
       checksum at every tick**.
   - (c) **Suspects to rule in or out, each with its own measurement:**
     1. **Instance buffers.** They grow during play, because `reserve_units` runs only at start and on a scene swap.
        This is likely only a few allocations (follow-up 35).
     2. **The music.** The allocation, the decode and the 16-bit copy already run on the decode thread. The main
        thread does the hand-over (`inject`, `play_music`) and frees the old track, about 50 MiB. Measure those against
        the 0.12 ms main-thread figure in `docs/rules/audio.md`, with a forced track switch every 10 s in a test scene,
        and say why one decode took 804 ms instead of about 40.
     3. **Windows window handling.** Resizing and moving the window block the event loop.
     4. **The sim thread** in M1 with many units (fog of war, targeting with sight): the 62 ms tick and the late
        snapshots.
     5. **The machine itself.** A long frame with little main-thread CPU time means the thread was not running
        (another program, antivirus). In the 1,628 ms and 885 ms frames the sim kept its pace, so only the main thread
        stood still; that counts against suspects 4 and 5 for those frames.
   - (d) **Fix** what (a) to (c) show, each with a test that fails without it where the runner allows one, and a
     measured before/after in the PR.
   - **Done when:** in a 10-minute M1 play on the GTX 1650 laptop with vsync on and `--hitch-log`:
     - no frame is over 50 ms, except during a window resize or move;
     - fewer than 0.1 % of the frames are over 25 ms (the benchmark's hitch threshold, `sf_game::bench::HITCH_MS`);
     - late snapshots are 0, or each one is explained.

     And Tijmen does not feel it stutter. A p99 frame time is not the criterion with vsync on, because a quiet second
     already sits at about 17 ms. The laptop play is Tijmen's; the PR gives him the command.
   *10-11-2026 (orchestrator): **WP1.16 is split.** **WP1.16a** builds the measuring half: (a) the hitch log with `--hitch-log`, (b) the
   command record with `--record` and its replay test, and from (c) the suspects that can be measured in the container (1, 2, 4) with the
   fixes that a measurement proves; it also records the swap-chain acquire outcome and the `on_resize` time, and reuses the minimap texture
   (confirmed waste). **WP1.16b** fixes what Tijmen's hitch-log file from the laptop shows (suspects 3 and 5 and anything new). Rules:
   `docs/rules/hitch_log.md`.*
   *11-11-2026 (Tijmen's laptop run with the hitch log, main `a0acd79`): **the stutter did not come back in a play of 5 min 44 s**
   (2 frames over 50 ms: frame 1 and the window maximise; 0.039 % over 25 ms; 0 late snapshots; Tijmen felt none). The play
   stopped at tick 6887, before the part of the match in which the worst frames of 10-11 fell (ticks 7476 to 8003). The log gives
   **WP1.16b nothing to fix**. The cause of 10-11 is not proven; two candidates are the minimap texture reuse of WP1.16a and the
   reviewer agent that ran the full gate on the laptop during that play. The done-when is not met as written: frame 1 (69.3 ms)
   is over 50 ms without a resize, and the play was shorter than 10 minutes. Proposal for Tijmen to decide: the done-when counts
   frames after the first 90, as the hitch log does. Open: one 10-minute play to close the WP. Numbers and files:
   `docs/progress.md` follow-up 35.*
   ***Closed 11-11-2026 (Tijmen: "Sluit WP1.16" to the orchestrator; confirmed in chat the same day: "Yes, close it").** WP1.16
   is closed on this run and on Tijmen not feeling any stutter; the 10-minute play is not required and WP1.16b is not built.
   Tijmen was asked about the two proposals in chat on 11-11 and did not answer them, so the orchestrator applies his rule "your
   proposal holds if I do not answer": no agent builds or tests on the laptop during a measured play, and this run is read after
   its first 90 frames. A warm-up rule for later measured plays is not decided here. If stutter comes back, the hitch log and the
   record are the first step.*
4. Then phase 9 (computer opponent) as planned.
   *10-11-2026 (Tijmen: "fase 9 mag eerder beginnen"): phase 9 starts before WP1.16 is fully done; WP1.16b waits for Tijmen's laptop log
   and does not block it. The AI parts edit `ci.yml` and `pinned.rs`, so they are built in sequence with the `sf_sim` work packages:
   WP6.11 (build speed), then WP9.1a, then WP6.10 (repeat toggle), then WP9.1b, then WP9.1c and WP9.1d.*

### Phase 8 — Vision
- WP8.1 Shroud (never seen) and fog of war (seen, not current) per player, cell-based in the sim.
- WP8.2 Vision range per unit, height bonus.
- WP8.3 Presentation: soft edges, last-seen buildings as a "ghost".
- WP8.4 Minimap and strategic icons respect vision. Icons are drawn last and without depth, so a fog layer cannot hide them: unseen units must be removed from the icon list on the CPU (see `sf_render::icons`).

*05-11-2026 (orchestrator, with WP8.1): WP8.2 is split. **WP8.2a** is the targeting gate (auto-acquisition only of cells the owner sees, retaliation always allowed), the sight rules of structures and the allied-vision hook, on top of `World::visibility` that WP8.1 builds; **WP8.2b** is the height bonus and waits until the sim has integer height levels and a map with relief exists (both maps are flat and the sim has no elevation today). WP8.1 computes what each player sees and changes no behaviour (`docs/rules/vision.md`).*

*06-11-2026 (orchestrator, WP8.2a): the targeting gate is **behind a flag in the vision rules (`acquire_needs_sight`), off by default and off in the M1 match** until Tijmen has played M1 and decides; WP8.2a measures what turning it on would change in `match` (won at tick 8067 instead of 5967, 8 units lost instead of 6) and does not ship it on. The headless `match` scenario has the same vision rules as `sf_game --match` (review finding 1 of #104), `match.expected` re-pinned for that. Shared (allied) vision is a per-player list in the rules, empty by default; no diplomacy system.*
*07-11-2026 (orchestrator, with WP8.3): **WP8.3 and WP8.4 are built together in one PR.** Both read the same snapshot fields from WP8.1 (`fog_version`, the two bitsets, `seen` per view) and neither touches the sim, so they share one presentation layer: the soft shroud and fog on the terrain, ghosts of structures last seen, and everything an unseen object of the other side is removed from on the CPU (models, the icon list, picking, the cursor, bars, effects, wrecks, tracers, the minimap). WP8.2a (vision-gated targeting, in the sim) is built in parallel on its own branch; this PR stays out of `crates/sf_sim/` and moves no pin (`docs/rules/fog_look.md`).*

### Phase 9 — Computer opponent
Layered (analysis §12, §22G):
- WP9.1 **[xhigh]** Strategic: build order, economy, tech choice, attack timing.
  *10-11-2026 (orchestrator, design memo `wp9.1-ai-ontwerp`; Tijmen: "fase 9 mag eerder beginnen"): WP9.1 is built in four parts, none of which
  touches `crates/sf_sim/`. **9.1a** the frame (built): `sf_ai` with the observation fence, the last-seen memory, the state and its digest, the
  think schedule; `sf_data::ai_profile` and `assets/ai/normal.ron`; the `ai` field and the driver in `sf_headless`; the scenario `ai_fog`;
  the lints. The AI observes and sends no command. **9.1b** [xhigh] base and economy (goal list, placement, power, harvesters, silo, repair,
  rebuild; scenario `ai_base`). **9.1c** army and timing (production mix, scouting, minimal operations, attack timing, defence;
  `ai_vs_passive`; the rules in `docs/rules/ai.md`). **9.1d** the game hookup (`controller` field in the match file, the sim thread calls
  the driver). Architecture rule 2 now reads "acts only via commands and reads only through the fog-filtered observation".*
  *12-11-2026 (orchestrator; the open choices of the WP9.1b design memo of 11-11-2026, which Tijmen did not answer, so the
  recommendations hold): 9.1b is split in **9.1b-1** (observation additions and the fenced `Lens`, the budget, the goal list, the power
  rule, placement, the construction manager; scenarios `ai_base`, `ai_noore`, `ai_boxed`) and **9.1b-2** (the economy manager:
  harvesters, silo, repair; `ai_economy`, `ai_rebuild`). Decisions: (1) the full price is saved before a normal build starts; emergencies
  (no refinery, a power shortage) start unfunded; (2) ore field locations are map knowledge (terrain), amounts only when seen; (3) the AI
  builds only on ground it sees now (`Lens::placement` answers only for a fully visible footprint and exit); (4) `placement_cap` 256 until
  measured, 32 if the worst think exceeds 1 ms: measured with 5,000 entities, a think whose every question fails takes 2.4 to 4.9 ms
  at 256 and 0.36 to 0.82 ms at 32 (64: up to 1.3 ms), so 9.1b-1 set it to 32; (5) lane 1 cell, refinery apron 3 x 5, factory apron 1 x 3, new profile numbers with defaults so the frozen `ai_fog`
  profile still reads; (6) a blocked power plant does not hold up the goal list; (7) the AI never sells in 9.1 (**reworded 12-11-2026, Tijmen: the AI may rescue itself from a money deadlock once per match and in that one rescue sell as many structures as it takes to cover the shortfall**, see the note below).*
  *12-11-2026 (WP9.1b-2, decisions taken while building the economy manager, no new profile number): (8) a harvester is ordered for every
  refinery up to `harvesters_per_refinery` (2) at once, ignoring `reserve` (that is for the army), so the goal list waits 2,200 credits for
  them: the whole list of `ai_base` stands at tick 8078, not 6518, and that scenario runs 10,000 ticks; (9) the silo is built only when no goal entry is
  open (a blocked entry counts as open), the store has been 90 percent full for two economy turns, and there are fewer silos than refineries;
  (10) a repair that never shows (a world without repair rules) is asked again after `blocked_ticks` only, which doubles as the backoff of a refused
  harvester order; (11) repair order is anchor, power plants and factories, harvester dock, the rest, two per turn, and only the anchor while an
  emergency build waits for money. Known consequence, which is what decision 7 now answers: a refinery lost at zero credits cannot be paid.*
  *12-11-2026 (Tijmen, decision 7 reworded; spec: design memo WP9.1c, "One sale per match"): **one rescue per match.** When an emergency build (the
  first refinery, a needed power plant) has been stalled for lack of money on two economy turns in a row and no income can arrive (no harvester or no
  complete dock), the AI sends `Sell` for the structures that cover what is still owed, once: the one with the smallest sufficient refund, else the
  smallest refunds added until the sum covers it, else nothing. Never the anchor, a harvester dock, a power plant, what the stalled structure
  requires or the stalled structure itself. If the simulation refuses every sale it tries again after `blocked_ticks` and the rescue stays unspent.
  It replaces "never sells"; selling for any other reason stays out of 9.1 (retreat and economy choices are WP9.4).*
  *12-11-2026 (Tijmen: the orchestrator's recommendations for WP9.1c hold; design memo `wp9.1c-ontwerp`): **9.1c is split in two**: **9.1c-1** production and scout
  (the production manager, the rally point and gather cell, one scout; the army only gathers) and **9.1c-2** [xhigh] the army (threat and defence,
  attack timing, target, marching and finishing; `ai_vs_passive`, `ai_raid`). A dedicated `scout` unit (role `scout`, 400 credits), one per match and at
  most one replacement (`scouts_max` 2, `scout_retry` 3000). The attack target will be the enemy construction centre (it is the win rule), only units at home
  defend and the attack is recalled only for the construction centre (all 9.1c-2). `gather_distance` 6 becomes **10** (inside the base before);
  `defend_ticks` 400 is for 9.1c-2, and the builder's measured values win over the memo's guesses. Production holds back the price of the next goal entry the
  construction turn is saving for, not just `reserve`. The frozen profiles of the older AI scenarios get `unit_mix: []`, so only their AI digests move, not
  their checksums. Built in 9.1c-1: nothing in `crates/sf_sim/` changed; the measured consequence of the holdback on M1's credits (first army unit at tick 15,020 in `ai_scout`, 17,867 in `m1_ai`)
  is in `docs/rules/ai.md`, "Production".*
  *12-11-2026 (decisions of the orchestrator after the first sweep of PR #128): **the scout is held back by `reserve` only**, a deviation from P3 (which holds back the next goal's price
  for every unit): it has to be bought early to find the enemy, and it delays the goal list by 1,960 ticks on `ai_scout` (measured). **The missing army on M1** (first army unit at tick
  15,020 in `ai_scout` and 17,867 in `m1_ai`, the real seat) **is solved in 9.1c-2 by an army floor**: `army_floor_value`, start value 1,000; below it production holds back only the `reserve`
  (not the next goal's price); the army value is the cost of the own army units, the queued mix items and what this turn has ordered already. Also decided: a factory that can make a harvester gets
  no rally point (a rally point on the vehicle factory cost 15 percent of the ore in 20,000 ticks, measured, more than the 10 percent limit); the gather cell and the 8 cells round it are kept free of
  the AI's own structures.*
  *12-11-2026 (decisions of the orchestrator for WP9.1c-2, built there): (1) **the army floor**: profile number `army_floor_value`; while the army value
  (unit costs of the own army units, the queued mix items and what the production turn running has ordered) is below it, production holds back only the
  `reserve`; the floor applies only while no attack has gone out yet or while the army defends. Start value 1,000; **built with 2,000** (measured on
  `ai_vs_passive`: 1,000 wins at tick 26,387, 2,000 at 14,659) and `max_wait` **6,000** (was 3,600: the first wave left before the floor was reached).
  (2) **The target is the enemy construction centre**; anything else only when it shoots back on the way. (3) **Only units at home defend**; the attack is
  called back only when the construction centre is hit (known gap, in `ai.md`: with the whole army out a hit elsewhere is answered by the towers only).
  (4) `defend_ticks` 400, the rest as in `normal.ron`, measured values win. (5) **A defeated AI stops**: no think once its player is defeated or the match
  is decided (`Observation::out_of_play`). (6) A structure the AI sells is dropped from `hp_seen` when the sale is sent, so a rescue sale is no attack.*
  *12-11-2026 (decisions of the orchestrator after the sweep of PR #129, built in its fix round 1): (1) a hit on the construction centre is an
  order change: every former attacker gets the defence `AttackMove` once, whatever its order (a marching wave too), under `resend_ticks` and
  the command cap. (2) The defeat clause of `out_of_play` is pinned by a three-player run (defeat while the match goes on). (3) **The army floor
  applies only while no attack has gone out and the tick is before `max_wait`, or while the army defends**, so it never holds the goal list past
  `max_wait` outside a defence. (4) The money around the first attack is traced and explained (one harvester's income), and the goal list with
  the floor and no attack is measured to 30,000 ticks (it does not complete: a gap). (5) An attack also ends when the structure being finished
  has not lost hit points for `attack_patience` while an attacker stands at the target. (6) The defence cell rule gets unit tests and a fence
  test. (7) The finishing `Attack` is sent and taken on the real world in a test; the owner filter is pinned. (8) Survivors of an ended attack
  walk home with `Move`, a retreat on purpose.*
  *12-11-2026 (decisions of the orchestrator, built in WP9.1c-3 and its fix round 1: economy before army): the goal list of both factions puts the
  **vehicle factory right after the refinery** (and with it the second harvester), before the barracks (option A of the opening design). Fix round 1, adopted on
  measurement: the **second refinery before the first tower**, **`attack_value` 2,000 = `army_floor_value`** (the first wave is value-driven, seat-independent) and
  `max_wait` **12,000** (was 6,000; it ends the floor and is the fallback; also the least gap between later waves unless `attack_value` is reached). Accepted risk: no barracks before
  about tick 2,900, so an early infantry rush by a human beats the opening. The target "goal list complete by 20,000 with the attack off" is met in `m1_ai` (19,777) and missed in
  `ai_vs_passive` (22,730); decision: the miss is accepted, its cause is the map's ore (174 cells in six fields of 3,480; the near field is mined out at about 9,000 to 10,000). Numbers and
  tables: `docs/rules/ai.md`, "The opening (WP9.1c-3)".*
  *12-11-2026 (orchestrator; design memo `wp9.2-9.5-ontwerp`): **the rest of phase 9 is built in the order 9.5a, 9.2a, 9.2b, 9.3a [xhigh], 9.3b [xhigh], 9.4, 9.5b.**
  9.5 is split: **9.5a** the AI-vs-AI tournament runner (`sf_headless tournament`, the match `m1_duel`, built first because every later WP changes army
  behaviour and today the only evidence is two scenarios against a passive opponent) and **9.5b** the 100-game report and tuning. The tournament runs
  on **shipped assets** through the game's loader, not on frozen copies; the match-to-AIs loop moved from `sf_game` into `sf_ai` (`match_ais`); a seeded
  jitter profile number is added **only if** fewer than about half the games are distinct, then in 9.2a. Built in 9.5a: the baseline of 100 games has 100
  distinct final checksums, which prove nothing (see the fix-round notes below: on coarse outcome keys 27 and 51 of 100, so the jitter is needed); it also shows that the normal AI decides only 7 of 100 games against itself (`docs/rules/ai.md`,
  "AI against AI (WP9.5a)").*
  *12-11-2026 (orchestrator, fix round 1 of WP9.5a, after the read-only sweep): (1) the seats of a tournament game are **read from the world and the AIs**, not from the schedule; (2) a decided
  game is part of CI (a fixture, `tests/tournament_decided.rs`); (3) every tick number of the record is a **count of ticks run**; (4) the **divergence of the games is measured by distinct outcome keys**
  (ending, ticks, last loss, attacks), per seating, not by distinct checksums, which spawn facing alone makes distinct; (superseded by the next note: the exact key is about as weak as the checksum);
  (5) after WP9.1c-3 merges, the 100 games are run again and both baselines stay in `docs/rules/ai.md`.*
  *13-11-2026 (orchestrator, fix round 2 of WP9.5a): the exact outcome key holds the exact last-loss tick and 92 of 100 games are capped, so it is about as weak as the
  checksum. On coarse keys Baseline A is **27 of 100** distinct for (ending, ticks, attacks per AI) and **51 of 100** with the last loss rounded to 1,000 ticks, at or below
  the memo's "about half". **The seeded jitter is needed and goes into WP9.2a** (profile number `timing_jitter_permille`, default 0 for frozen profiles, set in the
  shipped normal profile); its effect is measured there on the coarse key.*
  *13-11-2026 (orchestrator, WP9.1e): a read-only diagnosis found that the construction centre has no price, so its repair is free, and that at `hp_per_tick` 3 it out-healed up to five idle light tanks for ever and six needed 6,052 ticks (measured in a best-case fixture: tanks in range, never shot at, no towers, a defender with 0 credits). The rate is measured and changed to **1** in `assets/config/repair.ron` (data only). This is a **game rule that also affects the human player**: a power plant takes 30 s to repair instead of 10 and the centre 75 s from nothing instead of 25, the price of a repair is unchanged. The AI's attack has no kill deadline: it ends when the victim makes no new hit-point low for `attack_patience` (`stuck`) or no victim is in sight for that long; at rate 1 the measured stretch without a new low is 31 to 40 ticks for 2 to 12 tanks. A repair price for the centre and a pause of repair under fire were considered and are left to Tijmen (`docs/rules/repair_sell.md`, "The rate and the wave").*
- WP9.2 Operational: composing armies, choosing a target, gathering.
- WP9.3 **[xhigh]** Tactical: target priority, retreating, focusing.
- WP9.4 Difficulty levels without cheating (reaction time, efficiency), plus one "unfair" level.
- WP9.5 AI-vs-AI in headless as a balance tool: 100 games, win percentage and duration per faction.

**Done when:** the normal AI beats a passive player and loses to a good one; headless tournament runs without hangs.

### Phase 10 — Missions and editor
- WP10.1 (groundwork: `docs/mission-inspiration.md`) Scenario format: starting objects, players, goals, triggers (event → condition → action), teams. It absorbs the match file of WP7.6 (`assets/matches/m1.ron`: map, players, credits, starting objects) and turns the fixed win and loss rule of `sf_sim::outcome` into one goal among others.
- WP10.2 Trigger system in the sim (deterministic): time, entering an area, object destroyed, money, etc.
- WP10.3 Actions: messages, reinforcements, start cutscene, camera, victory/defeat.
- WP10.4 `sf_editor`: painting terrain, placing objects, editing triggers, testing from the editor.
- WP10.5 Campaign structure: mission sequence, briefing (cartoon), choices.

### Phase 11 — Presentation and UI
- WP11.1 Sidebar à la classic RTS: build tabs, queue, money, power bar. The build menu needs its own picture field in `unit_defs.ron`, separate from `icon` (since WP3.6 `icon` is the name of the strategic map icon). *Brought forward before M1 by decision of the orchestrator, 01-11-2026: M1 is "the first moment to really play", and the WP6.5 debug keys (U, I, O, G) are not a way to play. It needs production (WP6.5) and is stacked on it; power (WP6.4) is not built yet, so the power bar is a hook that shows a neutral bar until WP6.4 fills it.*
- WP11.2 Voice responses on selecting, moving, attacking (analysis §15), with a brake against spam. *Brought forward after the M1 play (Tijmen, 08-11-2026): next after WP7.7, see the queue under M1.*
- WP11.3 Sound: positional, volume by zoom (farther away = quieter and less).
  *WP11.3b (decision of the orchestrator, 11-11-2026, under Tijmen's rule "your proposal holds if I do not answer"): CC0 fight sounds so the shipped game is not silent in a fight without the player's Red Alert files. Source: **generated by our own tool in the repo** (`tools/sf_sound_gen`, integer maths, seeded noise, filters and sweeps; the files are committed, a test checks they match a fresh run), not downloaded packs: no network dependency, no third-party licence, reproducible. A downloaded CC0 pack stays the fallback only if a generated sound proves unusable. Every fight event of `bank.ron` gets a sound, also the four Red Alert has none for; with `--ra-audio` the Red Alert sound still wins where `ra_mapping.ron` maps one. Volumes are set so a 20-tank fight is not louder than the music at the default settings; all new files together stay under 2 MB.*
- WP11.4 Music: dynamic (calm/combat). Optionally plays original Red Alert music (B11) through the WP11.7 loader. The existing `ra_audio` setting (`assets/config/audio.ron`, `--ra-audio`) turns it on too; there is no separate music switch. A `music: (calm: [...], combat: [...])` section in `assets/audio/ra_mapping.ron` assigns tracks (names or `#id`s) to the moods. The shipped section is empty: Staalfront ships no list of Red Alert track names (B5; at most a single name checked by `onderzoeker`), and the player fills it in with the F1 sound browser. A mood with an empty list plays all tracks of the Red Alert music archive (`SCORES.MIX`). Several tracks per mood play shuffled (presentation side, not in `sf_sim`). A missing or unreadable track is skipped with one warning, and a mood with no playable track uses the default music (CC0, or silence if there is none), without error; the same holds without Red Alert files. Music is streamed or decoded when needed, never decoded up front with the sound effects. Tests use only generated fixtures, never real Red Alert files. The WP updates the README section on original Red Alert audio and docs/architecture.md §16.
- WP11.5 Menus, settings (keys, sound, graphics), accessibility (colorblind colors, UI scale). *Split by decision of the orchestrator, 03-11-2026: **WP11.5a** (menus: start menu, pause menu with restart, settings for keys, graphics and accessibility, `docs/rules/menus.md`) and **WP11.5b** (the sound settings: volume sliders and the Red Alert audio switch). The sound belongs to the audio work Tijmen does in another chat, so 11.5b is built later in agreement with it; 11.5a leaves a marked empty "Sound (WP11.5b)" section in the settings screen with no controls. M1 needed a way to start a match, pause and restart it, which 11.5a gives.* *11-11-2026: the audio is built in this chat (Tijmen, 09-11-2026), so the condition "in agreement with the audio chat" is met and **WP11.5b** is built: master, music, effects and unit-voice volumes (the voices split from the effects as their own group), the original Red Alert audio switch with the folder and `--ra-audio` winning for a run, saved in the settings file (`docs/rules/menus.md` "Sound").*
- WP11.6 Real models and textures replace the placeholders (see §10 assets).
- WP11.7 Original Red Alert audio (B11), optional, in `sf_audio`: reader for `.mix` archives (including Red Alert's encrypted headers: RSA-wrapped Blowfish key) and decoder for `.aud` (Westwood ADPCM and IMA ADPCM) to PCM; a mapping file in RON from Red Alert sound names to Staalfront sound-bank events; setting "Use original Red Alert audio" with the path to the player's install or freeware download. Missing or unknown files fall back to the CC0 sounds without error. Tests use only fixtures generated in the test itself (own encoder), never real Red Alert files; CI never needs them.

### Phase 12 — Cartoon cutscenes
See §9 for the design.
- WP12.1 Script format (RON) for scenes: layers, characters, poses, movement, camera, dialogue, sound, timeline.
- WP12.2 Player in `sf_cutscene`: cutout animation (jointed 2D figures), easing, transitions, subtitles, skipping.
- WP12.3 Lip-sync: mouth shapes from speech files (Rhubarb Lip Sync, offline), stored as a time series.
- WP12.4 Video fallback: WebM/AV1 playback for scenes delivered as video. **Deferred** (orchestrator, 17-10-2026) until an artist delivers a clip: the scripted cutout path covers every scene, and video would add three native or immature dependencies (decoder, demuxer, Opus). When a clip arrives, first try converting it to a cutout or sprite sequence; otherwise build this WP with the stack in §1.0.
- WP12.5 One complete demo scene (briefing mission 1) with placeholder figures.
- WP12.6 Preview tool: reload scene live on save (for the artist).

### Phase 13 — Saving, loading, replays
- WP13.1 **[xhigh]** Serialize the full world state (including running commands, production, projectiles) — equal checksum after loading is the test.
- WP13.2 Replays = start state + command log; playback at various speeds.
- WP13.3 Quick save, autosave, version migration of save files.

### Phase 14 — Multiplayer (optional, after M2; **parked** by decision Tijmen 22-10-2026)
- WP14.1 **[xhigh]** Deterministic lockstep over UDP (or QUIC), command delay, ping-compensated.
- WP14.2 Desync detection via checksums + automatic dump.
- WP14.3 Lobby, direct connection, later a small relay server.

### Phase 15 — Content and balance
- Two factions with ~12 units and ~14 buildings each, own character (not a copy of Allies/Soviets).
- 8–12 skirmish maps, campaign of 10 missions per faction.
- Balance rounds with headless AI tournaments + own play sessions.

**Milestone M2 — playable alpha:** skirmish against AI on all maps, first 3 campaign missions with cartoon briefings.

### Phase 16 — Polish and release
- Hit the performance budget (§2.4), crash reporting, installer, itch.io/Steam, trailer.

---

## 4. Dependencies

```
F0 ─ F1 ═ M0 ─ F2 ─ F3 ─ F4 ─ F5 ─┬─ F6 ─ F7 ═ M1 ─┬─ F8 ─ F9 ─ F10 ─ F15 ═ M2 ─ F16   (F14 parked, 22-10-2026)
                                  │                └─ F13
F1 ─────── F12 (player can run in parallel from F1; demo scene after F3)
F1 ─────── F11 (UI components in parallel as soon as the relevant system exists)
```

Parallel possible (each in its own git worktree): F12 alongside F4–F7; UI WPs
alongside the system they belong to; editor (WP10.4) alongside F8–F9.

---

## 5. Quality rules (apply to every WP)

1. No world mutation outside the sim tick. UI issues commands.
2. No `f32`/`f64` in `sf_sim`; only `sf_math` types. CI checks this with a lint.
3. No `HashMap` iteration order in the sim (not deterministic) — `BTreeMap` or sorted vectors.
4. Every gameplay WP delivers a headless scenario that pins down the behavior (checksum regression).
5. Every visual WP delivers a screenshot test or a recorded short clip.
6. Game data in RON, not in code; adding a new unit = no Rust change.
7. Document every non-obvious game rule in `docs/rules/`.

---

## 6. Dangers and how to catch them

| Risk | Consequence | Countermeasure |
|---|---|---|
| Determinism leaks (float, hash order, threads) | Replays/multiplayer broken, discovered only late | From F0 checksum tests on Linux and Windows in CI |
| Pathfinding with many units | Hangs, slow | Flowfields + headless stress tests from F4 |
| Zoom breaks performance | Stuttering when zoomed out | LOD/instancing/icons in the engine (F1), fps benchmark from M0, re-run on the laptop as M0b (after WP2.4) and M0c (after WP5.6b) |
| Art is missing | Game keeps looking "programmer-made" | Decide early who makes models and cartoons (§10) |
| Agents diverge | Inconsistent code | Fixed conventions in `CLAUDE.md`, review gate per phase (§12) |
| Legal | Trademark/GPL | Clean-room, own name and content (B5) |
| Scope | Never finished | M1 is the first real decision point; only then expand |

---

## 9. Cutscenes as cartoon

**Choice:** scripted 2D cutout animation in the engine (like South Park / Rive /
Spine style), with video as fallback.

**Why not video alone:** video is large (hundreds of MB), fixed at one
resolution, and every text change or translation means re-rendering.
Scripted scenes are kilobytes plus the drawings, razor-sharp at 4K, and
dialogue/lip-sync can differ per language.

**Pipeline:**
1. Script and storyboard (text + sketches per shot).
2. Artist delivers characters as separate parts (head, eyes, mouths, arms…) as PNG or SVG, plus backgrounds.
3. Voice recording per line (WAV).
4. Rhubarb extracts mouth shapes from each line.
5. Scene RON describes the timeline: who stands where, which pose, camera movement, which line.
6. Player shows it in the engine; preview tool lets the maker adjust live.

**Style:** clear cartoon with thick lines and flat colors — matches
the low-poly 3D world and is feasible with cutout animation.

**Honestly:** agents build the player, the format and the tooling. The drawings,
voices and direction must come from a human or a licensed generative tool.
Placeholders (geometric figures) keep the work going in the meantime.

---

## 10. Assets: who makes what

| Asset | v0 (agents) | Real |
|---|---|---|
| 3D models | primitives (cube/cylinder) in faction color | Blender, artist, or CC0 packs (Kenney, Quaternius) as an interim step |
| Terrain textures | procedural | CC0 (ambientCG, Poly Haven) |
| Effects | own GPU particles | same, tuned |
| Sound | CC0 (freesound) or Pixabay sounds (Pixabay Content License); record license, source URL and creator per file. No rips from Red Alert or other games in the repo, even if they are on Pixabay. Exception: original Red Alert audio via the runtime loader (B11, WP11.7), read from the player's own files | own recordings/purchase |
| Voices | text-to-speech placeholder. Exception: original Red Alert voice lines via the runtime loader (B11, WP11.7), read from the player's own files | voice actors |
| Music | silence/CC0. Exception: original Red Alert music via the runtime loader (B11, WP11.4), read from the player's own files | composer |
| Cartoons | geometric figures | artist |

Record the license per asset in `assets/LICENSES.md` — from the first file onward.

---

## 12. Execution with agents

### 12.1 Agent type
Two building agents, both in the repo (they travel to every environment):

- `.claude/agents/bouwer-sonnet.md` — model `sonnet`, effort `medium` (since 24-10-2026, with a done-checklist in the file): the default for every WP. The hard ones marked **[xhigh]** in §3 (determinism, simulation core, terrain LOD seams, picking/LOD, pathfinding, AI, full-state serialization, lockstep) use the same agent, started with the Agent tool's per-call `model: opus` and effort `low`, so they keep the same build instructions and done-checklist (since 25-10-2026).
- `.claude/agents/bouwer-opus-medium.md` — model `opus`, effort `low` in the file (the name is historical): every fix round after a review asks for changes, started with the Agent tool's per-call effort set to `low` (since 24-10-2026). It works on the WP's existing branch and pushes to the existing PR.

Decision Tijmen 17-10-2026, second change that day (Sonnet medium/xhigh from
14-10-2026, then Opus low/medium earlier on 17-10-2026); fix rounds changed by
decision Tijmen 21-10-2026; build effort, checklist and fix-round effort changed by decision Tijmen 24-10-2026; [xhigh] WPs on Opus low by decision Tijmen 25-10-2026. `bouwer-sonnet` builds one work package in its own git
worktree/branch and opens a PR; `bouwer-opus-medium` continues that branch and PR.

Two supporting agents, both model `opus`, both read-only (no edit, commit or push):

- `.claude/agents/reviewer.md` — effort `medium`: the review gate per PR (§12.2 step 4).
- `.claude/agents/onderzoeker.md` — effort `low`: research questions (crates, APIs, algorithms, asset licenses); does not review.

Decision Tijmen 14-10-2026.

### 12.2 Workflow per work package
1. Orchestrator (main session) starts an agent with: WP number, the acceptance criteria from this plan, relevant `docs/`.
2. Agent works on branch `wp/<number>-<name>`, writes tests first where possible, runs `cargo fmt`, `clippy -D warnings`, `cargo test`, headless scenarios.
3. Agent opens a PR with: what was done, how it was tested, what was deliberately not done.
4. Review gate: the `reviewer` agent checks out the PR, runs the checks itself, assesses it against the WP's acceptance criteria, §5 and §12.4, and posts `APPROVE` or `CHANGES REQUESTED` as a PR comment. The orchestrator merges only after `APPROVE`; on `CHANGES REQUESTED` a fix round of `bouwer-opus-medium` (§12.1) fixes the findings on the same branch and the reviewer reviews again.
5. After each phase: short playable build + report in `docs/progress.md`.

### 12.3 Parallelism
Normally 3-4 agents at a time, 5 at most (decision Tijmen 22-10-2026; was 3), only on WPs without overlap in crates (see §4).
Work packages that change `sf_sim` (and with it the shared `crates/sf_sim/src/checksum.rs`, `crates/sf_sim/src/world.rs`, `crates/sf_sim/src/lib.rs` and the
scenario lists in `.github/workflows/ci.yml` and `tools/sf_headless/tests/pinned.rs`) are built one after another, not in parallel; the other slots go to work in other crates, reviews
and research (decision Tijmen 27-10-2026: on 27-10 four merge rounds between parallel sf_sim WPs cost about 380k tokens).
Conflicts in `Cargo.lock`/shared types are resolved by the orchestrator.

### 12.4 What an agent does not do
- No change to sim determinism rules without approval.
- No new dependency without motivation in the PR.
- No assets without a license line.
- Never original Red Alert files (`.mix`, `.aud`, `.vqa`, extracted WAVs) in the repo, a release or a CI artifact (B11).

---

## 13. Build environment

The project lives **only on GitHub**; nothing goes on the home server
(live services, no GPU, too little RAM).

- **Cloud agents** (Claude Code remote, linked to the GitHub repo) build,
  test headless and open PRs.
- **GitHub Actions** checks every PR on Linux and Windows (fmt, clippy,
  tests, determinism checksums, screenshot test via Mesa lavapipe) and delivers
  a playable build per PR as an artifact.
- **Session's cloud container**: Rust (stable) and Mesa lavapipe (software Vulkan, no GPU) are present — enough for building, tests, headless and screenshot tests, **not** for fps measurements.
- **Laptop (Windows, NVIDIA GTX 1650)** for playing and reviewing, and the reference machine for the fps requirement of M0; possibly also as
  build machine for agents when visual work has to happen locally.

## 14. First steps

1. Link cloud agents to the GitHub repo (§13).
2. Confirm decisions B1–B7.
3. Agent type `bouwer-sonnet` active (Sonnet 5; effort high from 17-10-2026, medium since 24-10-2026), start phase 0 (WP0.1 through WP0.3), then phase 1 (engine).
4. At M0 (engine demo): benchmark on the laptop, view the zoom and tune the camera sensitivity together. Only then phase 2. (22-10-2026: the benchmark is met; the zoom look and camera sensitivity are still to be judged by Tijmen, and phase 2 goes ahead meanwhile, see `docs/progress.md`.)
