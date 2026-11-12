# The look of shroud and fog, ghosts and what is hidden (WP8.3, WP8.4)

WP8.1 worked out what the local player sees (`vision.md`). This page is what the **picture** does with it. It is presentation
only: nothing in `crates/sf_sim/` changed, no checksum depends on any of it, and a world without vision rules is drawn exactly as
before. Code: `crates/sf_game/src/fog_view.rs` (ghosts, the upload bookkeeping, the minimap's dots), `crates/sf_render/src/fog.rs`
(the fog texture), `terrain.wgsl` (the shading), `crates/sf_ui/src/minimap.rs` (the fogged minimap), and a few one-line gates named
below. Data: `assets/config/fog_look.ron` (read by `sf_data::fog_look`, pinned in its tests). Screenshots: `docs/screenshots/wp8.3/`.

**Only with vision rules.** Matches turn vision on (`matchplay::configure`, `docs/rules/match.md`), and so does `--debug-fog` for the
sandbox. Every function below does nothing when the snapshot has no `fog_version` (the sandbox, the crowd, the benchmark, the demo):
nothing is hidden, there are no ghosts, no fog is uploaded, the terrain shader skips its fog branch (`fog_on = 0`). The test
`without_vision_rules_the_fog_upload_is_a_no_op_for_the_pixels` renders a sandbox-like world twice with the same renderer, once through the fog upload step of
the runner and once without it, and finds 0 pixels differ: it shows that the fog upload does nothing without vision rules, not that
the picture equals the one before WP8.3 (there is no second, fog-free renderer to compare with).

## The look: shroud and fog on the terrain

| State | Picture |
|---|---|
| visible | the terrain as it always was |
| fog (explored, not visible) | the terrain pulled towards `fog_tint` by `fog_dim` (in linear light): darker and a little blue |
| shroud (never seen) | `shroud` (a near-black), the terrain does not show through at all (so neither does the ore) |

The two bitsets of the snapshot become an `Rg8` texture of one texel per gameplay cell: red is *explored*, green is *visible*
(`sf_render::fog::texels`). Before the upload the two maps are **softened**: `softness` passes of a 3 by 3 box blur, edges clamped, then
the shader samples the texture with a bilinear filter at the cell centres and runs the result through `smoothstep`. So the edge between
two states is a ramp of about `2 * softness + 1` cells, not a staircase of cells (`the_edge_between_seen_and_unseen_is_a_ramp_and_not_a_step`
measures a row of pixels across the edge). Because every visible cell is also explored and the blur is monotone, the blurred visible
never exceeds the blurred explored (`a_blurred_visible_cell_is_never_brighter_than_explored`), so the shader's `mix`es never produce
"visible but unexplored".

The shader (`terrain.wgsl`): `fogged = mix(color, tint, fog_dim)`; `shown = mix(fogged, color, visible)`;
`out = mix(shroud, shown, explored)`. One texture fetch per fragment, only when `fog_on` is 1.

**Updated only when `fog_version` changes.** `fog_view::fog_step(applied, current)` is the whole decision (a table in the tests): the
terrain and the minimap each remember the version they show, upload when the snapshot's differs (`FogStep::Upload`), take the fog off
when the world has no version (`Clear`), and do nothing otherwise (`Keep`). A new scene forgets what was shown (`FogApplied::forget`,
called by `attach_scene` and `on_start`) because the terrain and the minimap are made new.

`fog_look.ron` (every number pinned by `sf_data::fog_look` tests):

| Field | Meaning |
|---|---|
| `shroud`, `fog_tint` | sRGB colours of the terrain in shroud and the tint of fog |
| `fog_dim` | how far fog pulls towards the tint, 0 to 1 |
| `softness` | blur passes, 0 to 4 |
| `ghost_colour` | the team colour a ghost's model gets (palette entry 5, after the wreck colour) |
| `ghost_dot` | a ghost's dot on the minimap |
| `minimap_shroud`, `minimap_fog_dim` | the minimap's shroud colour and how far fog pulls its cells towards it |

The debug view of WP8.1 (`--debug-fog`, the F1 checkbox, `vision_look.ron`) is unchanged and stays behind its flag; with the flag the
real look is also on (the flag gives the sandbox vision rules), and the film lies over it.

## Ghosts (WP8.3)

A **ghost** is the last view the local player had of a structure that is not its own (the other side's, or a neutral one).
`GhostBook::update` runs once per new snapshot (`Runner::pull_snapshot`):

| Case | What happens |
|---|---|
| a structure of someone else is **seen now** | its record is written or rewritten (kind, owner, footprint, middle, facing, damage stage, the shape the picture drew it with: the damage stage or the stage of construction is in the shape) |
| a record whose structure is **not seen** and whose footprint cells are all not visible | it is drawn: the model of its last shape in the ghost colour, at its place |
| a record whose structure is **not seen** and **a footprint cell is visible** | the player looks at the place and the structure is not there (destroyed, sold): the record is removed |
| the structure is seen again and alive | it is drawn as itself; its record only waits (and is rewritten) |
| units, own structures, a structure never seen | no record |
| a new scene, a snapshot of an earlier tick than the last one, a world without vision rules | the book is cleared |

A structure destroyed while nobody looks keeps its ghost: the player does not know. The ghost is a motion for the instancer in a
slot after every entity and every wreck (`runner::scene_motions`), with the generation `u32::MAX - 1`, so a pick of it names no entity
(the same device the wrecks use): hovering or clicking a ghost behaves like the bare ground (a move order is given, nothing is selected or
inspected). It is dimmed by its palette colour only; a model keeps its own materials, so the ghost reads as "grey-blue" rather than
"desaturated". The minimap shows a ghost as a dot in `ghost_dot`.

## What is hidden (WP8.4)

An object of someone else that the local player does not see (`SimSnapshot::seen`: its cell, a structure: any footprint cell, is not
visible) is **removed on the CPU**, before it reaches anything that draws or picks. The local player's own objects are always shown.
The decision is made once in the snapshot (`view_shown`, `wreck_shown`, `projectile_shown`, `shown_at_metres`) and the rest follow it:

| Where | How |
|---|---|
| model drawing, the strategic icon list, hover, click and drag picking, double-click "same kind on screen" | `runner::motions_of` does not give the instancer a view that is not shown, so the instancer neither builds, draws, iconises nor picks it. Icons are drawn last and without depth, which is why this is done on the CPU |
| clicks, hover, the cursor, the inspected panel, the attack cursor | `selection::facts` answers `None` for an entity that is not seen: a hidden enemy is ground. An inspected enemy that goes out of sight is let go (`forget_the_dead`) |
| bars, signs and flags over structures (construction, storage, repair, sale, production, rally flag) | `construction_view::front_edge` (the one function they share) returns `None`, and `draw_rally_flags` skips, for a structure that is not seen |
| harvester cargo bars | they hang on the units the instancer drew (`visit_drawn`), so a hidden harvester has none |
| muzzle flashes, hits, deaths | `FxPlayer::release` does not spawn an event whose cell (`pos` in metres divided by 4) is not visible and whose `player` is not the local player; counted in `FxCounters::hidden` |
| wrecks and rubble | not given to the instancer, and no wreck smoke, when no cell of them is visible and the owner is not the local player |
| tracers (projectiles) | skipped when the ground cell under them is not visible and the shooter is not the local player |
| damage smoke and fire | `damage_smoke` skips a view that is not shown |
| the minimap | the dots are `fog_view::minimap_dots`: the shown views, then the ghosts; the image carries shroud and fog (`Minimap::set_fog`, `fog_states`, `fogged_color`); ore painted under shroud is kept and shows when the fog goes |
| scenery | the development landmarks (`world_markers`) are not put in a world with vision rules: they would stand out of the shroud |

A **move order** onto a shroud or fog cell is fine (the ground point is not hidden information); an **attack cursor** over a hidden
unit does not appear, since the unit is not there for the picture (see `cursor.md` and `selection.md`).

An effect is decided by the cell it happens on, so an enemy's shot *at* the player's own unit shows its impact (the cell is visible) but
not the muzzle flash far away; the player's own shot into the shroud shows both (its `player` is local). That is a presentation rule,
not an information rule: the simulation still knows everything.

## The placement preview (follow-up to #106)

The ghost of a structure being placed (`placement_view`, `docs/rules/buildings.md`) asks the sim for its verdict per cell. A cell the sim
calls a **structure** or a **unit** is drawn red only when the player sees that cell now (`SimSnapshot::is_visible`). **Every** on-map
cell the player cannot see now, free, structure or unit alike (also an own structure outside own sight), is drawn in the neutral grey `placement_unknown` of `vision_look.ron` (`placement_view::tints`, `tint_seen`). The top-bar words for
"another structure is in the way" / "a unit is in the way" are only said when a cell of exactly that kind is in sight, else the line says
"cannot build here" (`refusal_words`). Terrain and map-edge cells are not hidden. **The verdict is the sim's and is untouched**:
a refused placement is still refused and the outline and the amber cells say so, but no cell says where the hidden blocker is: grey
means "not in sight", never "something hidden stands here". The 4-second top-bar line after a refused click uses the same rule
(`click_refusal_words`): "another structure / a unit is in the way" only when every footprint cell is in sight, else "cannot build here", and "the exit is blocked" only when every cell of the ring round the footprint (where any exit cell lies) is in sight.
A factory's **exit cell** follows the same rule (`exit_look`): out of sight it is drawn in the same `placement_unknown` grey, fill and
line, whether the sim calls it usable or not (the sim calls it unusable when a structure stands on it); in sight it is blue or red. The
hover words for `ExitBlocked` say "the exit is blocked" only when the exit cell is in sight, else "cannot build here". Only refusals of
the local player reach its top bar.
Without vision rules every cell is seen, so nothing changes. Tests: `placement_view::tests::a_structure_the_player_cannot_see_is_not_drawn_as_a_red_cell`, `a_click_refused_by_a_hidden_blocker_does_not_name_it`,
`the_unknown_colour_comes_from_the_look_file_and_is_neither_red_nor_green`, `sf_data::vision_look::tests::the_shipped_look_is_pinned`.
Tests also: `an_exit_cell_out_of_sight_looks_the_same_free_or_blocked`, `a_blocked_exit_out_of_sight_is_not_named`.
Left: that the placement is refused at all (outline, amber cells) is still shown; the console log line of a refused click prints the
same generic words as the top bar.

## Gaps (known and left)

* **Sound.** The audio plays `FxPlayer::shown()` as before, so a shot or an explosion in the shroud is still heard. **Current rule (WP11.3b): sounds of hidden events are audible**, at their
  distance volume (`audio.md` 3c); since the CC0 fight sounds exist this is true in the shipped game. Tijmen may change this ("no sound
  from what I cannot see" is a design choice); nothing in `sf_audio` or `audio.ron` changed for fog.
* **Ghosts are only structures**, and a ghost does not show damage that happened out of sight, or the real progress of something
  under construction that was seen once.
* **A ghost is a team colour and not a desaturated model.** The model keeps its materials.
* **No allied vision**, no radar or "reveal map" (WP8.2a and later).
* The F1 cell grid overlay (`cell_overlay::unit_cells`) still marks every unit's cell (debug view, off by default).
* The old debug film (`--debug-fog`) is still a film over the whole picture.
* The softness is a blur of the cell maps, so a one-cell-wide visible stripe (a scout in a corridor) is dimmer than a wide one.

## Tests and mutations

Tests (all call production code):

* `crates/sf_render/src/fog/tests.rs`: the texels are the bits (red explored, green visible: a swap fails), the row stride, the blur is
  monotone and keeps the sides, visible never exceeds explored at any softness, short bitsets read as clear.
* `crates/sf_data/src/fog_look.rs`: every number of the shipped look, a broken file refused.
* `crates/sf_ui/tests/minimap_fog.rs`: each state's colour, the visible columns bit for bit the bake, taking the fog off gives the bake
  back, the same fog twice changes nothing (no upload), a wrong size refused, ore under shroud.
* `crates/sf_game/src/fog_view/tests.rs`: the lists the instancer gets (own always, enemy in sight, enemy out of sight, own legacy structure in
  fog), icon list and model lists counted, picking, selection facts and the inspected enemy, the ghost life (seen, left, destroyed
  unseen, seen empty, seen again, never for units or own structures, cleared by a new game), the minimap's states and dots, the upload
  table, the version, effects by cell (including the 4 m cell boundary and the map edge), projectiles, wrecks, the bars' front edge.
  `fx::tests::effects_on_cells_out_of_sight_are_not_drawn_but_still_reach_the_audio_list`.
* `crates/sf_game/tests/fog_screenshot.rs` (real renderer on a software adapter): shroud is the look's colour, fog is the clean terrain
  pulled by `fog_dim` (worked out from the picture without fog), visible is untouched, the edge is a ramp, the refinery's ghost appears
  and goes, the minimap with fog, and without vision rules the fog upload changes no pixel.

Mutations: see the table "Mutations (each applied, tests run, reverted)" in the body of PR #106.

## Cost

Release build, the container's software adapter (llvmpipe), load average 1.1 to 1.3 before and after the runs (the machine was lightly
busy), the medians of 21 timings (15 for frames), three rounds each where a line says so, off and on alternating. The container and CI
cannot measure the frame-rate requirement (PLAN section 13); the laptop benchmarks M0b and M0c do. Measured by the ignored tests
`fog_view::tests::measure_the_cost_of_hiding` and `fog_screenshot::measure_the_cost_of_the_fog` (`--ignored --nocapture`).

| What | Cost |
|---|---|
| `fog::texels`, 96 x 96 cells (the first match), softness 0 / 2 / 4 | 21 / 81 / 142 us |
| `fog::texels`, 256 x 256 cells, softness 0 / 2 / 4 | 197 / 618 / 1,226 us |
| `Renderer::set_fog` (the texture write, 96 x 96) | 11 us |
| frame on the software adapter, 96 x 96 map, fog off / on (3 alternating rounds) | 7.7 / 8.4, 7.9 / 8.4, 8.5 / 8.5 ms (the per-fragment fetch is inside the noise of a CPU rasteriser) |
| `SimSnapshot::fill`, 200 views (sim thread), vision off / on | 5.5 / 6.5 us |
| `SimSnapshot::fill`, 5,000 views (the size of `wander_5000`), vision off / on | 134 / 199 us (+65 us: the `seen` pass and the footprints) |
| `motions_of` (main thread), 200 views, off / on | 3.4 / 3.4 us |
| `motions_of`, 5,000 views of which 438 are seen, off / on | 84 / 10 us (hidden views are skipped before the motion is built) |
| `GhostBook::update`, 5,000 views / `minimap_dots` | 3.9 us / 10 us |

The texture is rebuilt only when the version changes, which is when the visible or explored set of the local player changes: with units
moving that is most ticks, so the 96 x 96 map pays about 80 us a tick for the blur and one 18 KB upload; the 256 x 256 map about 0.6 ms. The
shader's cost on a real GPU was not measured here.
