# Staalfront — build plan

*Working title. A real-time strategy game in Rust, 2.5D, with seamless zooming from
close-up to the whole map and cartoon cutscenes. Inspired by the game rules of
Command & Conquer: Red Alert, without its name, world, units or media.*

Drawn up 14-10-2026. Source: `docs/red-alert-analysis.md` (functional analysis of
the EA source code). Execution: Opus agents at effort low, medium for the WPs marked [xhigh] (see §12).

---

## 0. Decisions up front (record before phase 0)

| # | Question | Proposal | Why |
|---|---|---|---|
| B1 | Engine | **Own thin engine on wgpu + winit**, no off-the-shelf engine (see §1.0) | An RTS has one camera, one kind of world and thousands of identical objects: exactly what you control tightly yourself with instancing. Full control over determinism, tick order and memory. |
| B2 | What "2.5D" means here | **True 3D world, fixed camera angle, low-poly models** | Sprites (like the original) get ugly with seamless zooming. 3D models stay sharp up close and simplify cleanly from afar. Gameplay stays flat (2D grid). |
| B3 | Simulation | **Deterministic, fixed tick (20 Hz), fixed-point math, separate from rendering** | Enables replays, saving, multiplayer (lockstep) and automated tests. Adding it afterwards is nearly impossible. |
| B4 | Cutscenes | **In-engine cartoon player (cutout/vector animation, scripted) + video as fallback** | Small, sharp at any resolution, lip-sync and translation adjustable afterwards. See §9. |
| B5 | License of the source-code reference | **Clean-room**: use only the functional analysis, never read-and-retype EA code | EA code is GPL; copying it verbatim makes Staalfront GPL. Own name, units, graphics, music are mandatory (trademark). Exception for original audio: B11. |
| B6 | Where it is built | **GitHub + cloud agents + laptop**, never on the home server (see §13) | The home server runs live services and has no room (RAM, GPU) for a Rust game build. |
| B7 | Target platform v1 | Windows + Linux desktop. Web and macOS later. | Mouse + keyboard; an RTS on a phone is a different game. |
| B8 | Language of the game | **English** (menus, UI texts, voices, subtitles); all game texts in language files, not in code | Decision Tijmen 14-10-2026. Language files keep translating possible later. |
| B9 | Language of code and documentation | **English**: code, comments, docs, commit messages, PRs | Decision Tijmen 14-10-2026. |
| B10 | Reference hardware | **Windows laptop with NVIDIA GTX 1650**, 1920×1080 | Decision Tijmen 14-10-2026, card corrected 15-10-2026 (the laptop has a GTX 1650, not a 1050). The fps requirements apply on this card; VRAM budget 2 GB kept (headroom below the 1650's 4 GB). |
| B11 | Original Red Alert audio | **Optional, loaded at runtime from the player's own Red Alert files** (like OpenRA); never in the repo, a release or a CI artifact | Decision Tijmen 14-10-2026. EA released the code, not the media; the 2008 freeware download does not allow redistribution. Reading the player's own copy keeps the repo clean. Staalfront always works without it (CC0 sounds are the default). See WP11.7. |

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
| Cartoon video fallback | `ffmpeg` as a separate process that delivers frames | No heavy bindings in the build. |
| Tests | `proptest`, `insta` (snapshots) | Properties and regressions. |

### 1.1 Cargo workspace

```
staalfront/
├─ crates/
│  ├─ sf_math      fixed-point numbers, vectors, angles, deterministic RNG
│  ├─ sf_sim       the complete game simulation — no GPU, no I/O, no clock
│  ├─ sf_data      unit/building/weapon definitions (RON), validation, loading
│  ├─ sf_ai        computer opponent, talks to sf_sim only via commands
│  ├─ sf_engine    main loop, window, input, time, assets (no game rules)
│  ├─ sf_render    wgpu: terrain, models, camera, zoom, effects, LOD
│  ├─ sf_ui        own UI: widgets, text, minimap; later sidebar, selection, menus
│  ├─ sf_audio     sound, voices, music
│  ├─ sf_cutscene  cartoon player + script format
│  ├─ sf_net       lockstep network (later)
│  └─ sf_game      binary app: glues everything together
├─ tools/
│  ├─ sf_editor    map and mission editor
│  ├─ sf_headless  simulation without a screen: tests, balance, AI-vs-AI
│  └─ sf_demo      engine demo and benchmark (phase 1)
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

- Gameplay on a **cell grid** (e.g. 1 cell = 1 m, maps 64×64 up to 256×256),
  positions within a cell in fixed-point ("leptons", like the original).
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
| Strategic | **icons in player color** (triangle = vehicle, square = building, etc.), constant size on screen | map-like display | area overlays, route arrows |

Transition between model and icon with a short crossfade, not a hard swap.

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
- WP1.7 Terrain: engine map format (RON + heightmap, material index per cell), mesh in chunks of 32×32 cells, procedural materials via a texture array.
- WP1.8 **[xhigh]** Terrain LOD and culling: LOD levels per chunk based on screen error, seams closed (skirts or stitching), frustum culling; zoomed out on 256×256 ≤ 300 draw calls for terrain.
- WP1.9 **[xhigh]** Model LOD and strategic icons: LOD chain per model, and at strategic zoom screen-fixed icons (shape + player color) with crossfade (§2.2); click volume per zoom (ray on model, icon quad on screen).
- WP1.10 glTF models: loading of mesh, base-color material and node hierarchy (turret separate from hull), rigid node animation; hooking into instancing and model LOD. Skinned animation deliberately not yet. Axis and unit conversion happens once, in the loader: glTF (right-handed, Y up) is converted to the engine convention, which this WP fixes in `docs/conventions.md` (proposal: metres, Y up, front +X, ground at Y=0). The conversion is applied to vertices, normals, node matrices and rotations; the renderer and shaders never correct axes. A test model with a known orientation must prove front, up and handedness. The loader also merges a part's per-material nodes into one mesh (one draw per part per LOD) and strips `_LOD*` suffixes from part names (`assets/testpack_v1`). (Idea borrowed from BRUTAL's `GltfLoader.Transform.cs`; no code taken, licence not compatible.)

*Interface and sound*
- WP1.11 Debug panels with `egui` (via `egui-wgpu`), toggled with F1: frame time graph, draw calls and instances, camera settings adjustable live, sim tick and checksum, LOD coloring, cell grid overlay.
- WP1.12 `sf_ui` — own UI on wgpu: rectangles, nine-slice, text (glyph atlas), buttons, panels with anchors, UI scale, input that the UI receives first; minimap (render to texture, click = move camera, frame of the field of view).
- WP1.13 `sf_audio` with `kira`: sound bank in RON, positional sound relative to the camera (volume and panning, quieter when zooming out), music with crossfade; without an audio device (CI) run on silently instead of crashing. One CC0 test sound with a license line.

*Demo*
- WP1.14 `sf_demo` — the engine demo: generated 256×256 map with relief, 5,000 wandering placeholder units (3 mesh kinds, 4 player colors, a few glTF test models), minimap, egui panels, sound on clicks. Benchmark mode `--bench`: fixed camera flight from close-up to full overview and back, writes frame times (p50/p95/p99) and the sim checksum to JSON. In CI: runs on lavapipe/WARP as a smoke test with screenshots at three zoom levels (no fps requirement, that hardware is software).

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

**Done when:** a 256×256 playfield loads in sim and presentation; headless test checks terrain types and occupancy per cell.

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
- WP4.6 Formations (group keeps shape, slowest sets the pace).
- WP4.7 Replan on blocked route or disappeared target; destination ≠ route.

**Done when:** 200 units cross a bridge without getting stuck in a headless scenario (≤ 1% end up outside 5 cells of the goal).

### Phase 5 — Combat
- WP5.1 Weapons and bullet types as data: range, reload time, damage, damage type, projectile type.
- WP5.2 Projectiles as separate entities: straight, arcing, guided, falling (analysis §8), with fuse/detonation.
- WP5.3 Armor table: damage × armor type → percentage; splash damage with falloff.
- WP5.4 Target selection and automatic return fire, priorities, range check.
- WP5.5 Destruction: free up cells, wrecks/rubble, effects.
- WP5.6 Visual effects: muzzle flash, impact, explosions, smoke (particles), scale with zoom.

**Done when:** headless "10 tanks vs 20 infantry" gives the same outcome and checksum every run; it looks that way in the game too.

### Phase 6 — Buildings, building and power
- WP6.1 Building definitions: footprint, build time, power, requirements (tech tree), function.
- WP6.2 Placement: validity (terrain, proximity to own base), ghost preview under the cursor.
- WP6.3 Build animation tied to state — animation before mission logic (analysis §10).
- WP6.4 Power: generation/consumption, shortage slows production and switches off defenses; damage lowers generation.
- WP6.5 Production: queues per factory type, progress, exit + free spot, rally point.
- WP6.6 Repair and sell as processes with progress.
- WP6.7 Defensive buildings with turrets.
- WP6.8 Damage display: damaged and heavily damaged model, smoke, fire.

### Phase 7 — Economy
- WP7.1 Ore on the map: fields, amount, slow regrowth.
- WP7.2 Harvester: search, harvest, drive back, check in at refinery via **messages** (analysis §11, §13), unload, repeat.
- WP7.3 Storage capacity and silos, fill level visible.
- WP7.4 Money in the UI, deducting costs during production.

**Milestone M1 — vertical slice (end of phase 7):** one skirmish map, you against
a static opponent: build a base, harvest, make units, attack,
win. First moment to really play and adjust the design.

### Phase 8 — Vision
- WP8.1 Shroud (never seen) and fog of war (seen, not current) per player, cell-based in the sim.
- WP8.2 Vision range per unit, height bonus.
- WP8.3 Presentation: soft edges, last-seen buildings as a "ghost".
- WP8.4 Minimap and strategic icons respect vision. Icons are drawn last and without depth, so a fog layer cannot hide them: unseen units must be removed from the icon list on the CPU (see `sf_render::icons`).

### Phase 9 — Computer opponent
Layered (analysis §12, §22G):
- WP9.1 **[xhigh]** Strategic: build order, economy, tech choice, attack timing.
- WP9.2 Operational: composing armies, choosing a target, gathering.
- WP9.3 **[xhigh]** Tactical: target priority, retreating, focusing.
- WP9.4 Difficulty levels without cheating (reaction time, efficiency), plus one "unfair" level.
- WP9.5 AI-vs-AI in headless as a balance tool: 100 games, win percentage and duration per faction.

**Done when:** the normal AI beats a passive player and loses to a good one; headless tournament runs without hangs.

### Phase 10 — Missions and editor
- WP10.1 (groundwork: `docs/mission-inspiration.md`) Scenario format: starting objects, players, goals, triggers (event → condition → action), teams.
- WP10.2 Trigger system in the sim (deterministic): time, entering an area, object destroyed, money, etc.
- WP10.3 Actions: messages, reinforcements, start cutscene, camera, victory/defeat.
- WP10.4 `sf_editor`: painting terrain, placing objects, editing triggers, testing from the editor.
- WP10.5 Campaign structure: mission sequence, briefing (cartoon), choices.

### Phase 11 — Presentation and UI
- WP11.1 Sidebar à la classic RTS: build tabs, queue, money, power bar.
- WP11.2 Voice responses on selecting, moving, attacking (analysis §15), with a brake against spam.
- WP11.3 Sound: positional, volume by zoom (farther away = quieter and less).
- WP11.4 Music: dynamic (calm/combat).
- WP11.5 Menus, settings (keys, sound, graphics), accessibility (colorblind colors, UI scale).
- WP11.6 Real models and textures replace the placeholders (see §10 assets).
- WP11.7 Original Red Alert audio (B11), optional, in `sf_audio`: reader for `.mix` archives (including Red Alert's encrypted headers: RSA-wrapped Blowfish key) and decoder for `.aud` (Westwood ADPCM and IMA ADPCM) to PCM; a mapping file in RON from Red Alert sound names to Staalfront sound-bank events; setting "Use original Red Alert audio" with the path to the player's install or freeware download. Missing or unknown files fall back to the CC0 sounds without error. Tests use only fixtures generated in the test itself (own encoder), never real Red Alert files; CI never needs them.

### Phase 12 — Cartoon cutscenes
See §9 for the design.
- WP12.1 Script format (RON) for scenes: layers, characters, poses, movement, camera, dialogue, sound, timeline.
- WP12.2 Player in `sf_cutscene`: cutout animation (jointed 2D figures), easing, transitions, subtitles, skipping.
- WP12.3 Lip-sync: mouth shapes from speech files (Rhubarb Lip Sync, offline), stored as a time series.
- WP12.4 Video fallback: WebM/AV1 playback for scenes delivered as video.
- WP12.5 One complete demo scene (briefing mission 1) with placeholder figures.
- WP12.6 Preview tool: reload scene live on save (for the artist).

### Phase 13 — Saving, loading, replays
- WP13.1 **[xhigh]** Serialize the full world state (including running commands, production, projectiles) — equal checksum after loading is the test.
- WP13.2 Replays = start state + command log; playback at various speeds.
- WP13.3 Quick save, autosave, version migration of save files.

### Phase 14 — Multiplayer (optional, after M2)
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
F0 ─ F1 ═ M0 ─ F2 ─ F3 ─ F4 ─ F5 ─┬─ F6 ─ F7 ═ M1 ─┬─ F8 ─ F9 ─ F10 ─ F15 ═ M2 ─ F14 ─ F16
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
| Zoom breaks performance | Stuttering when zoomed out | LOD/instancing/icons in the engine (F1), fps benchmark from M0 at every phase |
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
| Voices | text-to-speech placeholder | voice actors |
| Music | silence/CC0 | composer |
| Cartoons | geometric figures | artist |

Record the license per asset in `assets/LICENSES.md` — from the first file onward.

---

## 12. Execution with agents

### 12.1 Agent type
Two agents, both in the repo (they travel to every environment), both model `opus`, same instructions:

- `.claude/agents/bouwer-opus.md` — effort `low`: the default for every WP not marked **[xhigh]**.
- `.claude/agents/bouwer-opus-medium.md` — effort `medium`: for the hard WPs, marked **[xhigh]** in §3 (determinism, simulation core, terrain LOD seams, picking/LOD, pathfinding, AI, full-state serialization, lockstep).

Decision Tijmen 17-10-2026 (was Sonnet at medium, and Sonnet at xhigh for [xhigh]
WPs, from 14-10-2026). Each agent executes one work package, works in its own git
worktree/branch, opens a PR. If a WP fails review twice on `bouwer-opus`, the
orchestrator reruns it on `bouwer-opus-medium`.

Two supporting agents, both model `opus`, both read-only (no edit, commit or push):

- `.claude/agents/reviewer.md` — effort `medium`: the review gate per PR (§12.2 step 4).
- `.claude/agents/onderzoeker.md` — effort `low`: research questions (crates, APIs, algorithms, asset licenses); does not review.

Decision Tijmen 14-10-2026.

### 12.2 Workflow per work package
1. Orchestrator (main session) starts an agent with: WP number, the acceptance criteria from this plan, relevant `docs/`.
2. Agent works on branch `wp/<number>-<name>`, writes tests first where possible, runs `cargo fmt`, `clippy -D warnings`, `cargo test`, headless scenarios.
3. Agent opens a PR with: what was done, how it was tested, what was deliberately not done.
4. Review gate: the `reviewer` agent checks out the PR, runs the checks itself, assesses it against the WP's acceptance criteria, §5 and §12.4, and posts `APPROVE` or `CHANGES REQUESTED` as a PR comment. The orchestrator merges only after `APPROVE`; on `CHANGES REQUESTED` the builder fixes the findings on the same branch and the reviewer reviews again.
5. After each phase: short playable build + report in `docs/progress.md`.

### 12.3 Parallelism
At most 3 agents at a time, only on WPs without overlap in crates (see §4).
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
3. Agent type `bouwer-opus` active (was `bouwer-sonnet` until 17-10-2026), start phase 0 (WP0.1 through WP0.3), then phase 1 (engine).
4. At M0 (engine demo): benchmark on the laptop, view the zoom and tune the camera sensitivity together. Only then phase 2.
