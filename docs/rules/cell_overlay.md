# Cell grid overlay (WP2.4)

A debug and planning view, off by default. The F1 panel's **Cell grid (4 m)** checkbox
(or `sf_game --debug-grid`) switches it on together with the grid lines. It is a view only:
it reads the simulation and changes nothing in it.

## What a cell shows

Every 4 m gameplay cell gets exactly one state. The highest one wins:

| State | Drawn as | Means |
|---|---|---|
| structure | `structure` colour, filled and outlined | The sim's occupancy holds a structure on the cell (`World::occupant_at`). A structure with a footprint marks every cell of it. |
| units | `units` colour, a disc in the cell | At least one entity's current position lies in the cell. |
| impassable | type colour with dark diagonal hatching | The terrain type is not open ground: at least one ground movement class (foot, wheeled, tracked) cannot enter it (`TerrainFlags::ground` is false; it is true only when every ground class can). Water is hatched although boats sail on it. |
| open | type colour, translucent (`fill_percent`) | None of the above. |

## What "occupied" means

Only **structures occupy cells** in the simulation (WP2.1, `sf_sim::terrain`). Units have
bodies (WP4.4) and do not claim cells, so any number of them can share one; the sim keeps
no per-cell record of them. The "units" state is therefore derived by the presentation
from the snapshot's positions: it says where units are, not what blocks a route. A cell
with a unit is not blocked for anything.

## Data

`assets/config/cell_overlay.ron`: an sRGB colour for every terrain type (by name; a
missing or unknown name is refused), the `units` and `structure` colours and the opacity
of the tint. The single source of per-type colour is to be `assets/config/terrain_look.ron`
(PR #66); the minimap and this overlay both derive from it once it is merged (follow-up 30
in `docs/progress.md`). Until then this table stands alone.

Fading: below about 8 screen pixels per cell the patterns fade out, so the far zoom shows
flat colours instead of moire. That holds for the hatching, the unit disc (it becomes a
flat tint of the whole cell) and the structure outline (it becomes a flat fill).

## Cost

One texel per cell, rebuilt on the CPU at most once per sim tick while the overlay is on;
the upload covers the span from the first changed row to the last (one write). Drawing
repeats the terrain's level-of-detail draws (one call per run of chunks, counted in the F1
panel) with a shader that reads one texel per fragment.

When off, nothing is rebuilt, uploaded or drawn, but it is not free: the CPU layer, the
texture and the pipeline are built at start-up, and the sim thread lists the structure
cells in every snapshot whether the overlay is on or not (only skipped when the world has
no structures). Measured with 5000 entities: about 26 us per tick with 1 to 200
structures, about 40 us with 2000; about 2 % of a tick step and about 0.05 % of the
budget.
