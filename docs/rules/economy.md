# Economy: ore on the map

Work package WP7.1 (PLAN.md phase 7). This page holds the rules of the **ore on the
map**: how much a cell holds, how it is taken, how it grows back, how a harvester will
find it. The harvester itself (WP7.2), storage (WP7.3) and money (WP7.4) are later work
packages and will add their rules to this page. Rules in our own words.

## What is where

* **Where ore can be** is the terrain: a cell whose terrain type is `harvestable`
  (`terrain_types.ron`; as shipped the type `ore`, id 4, see `terrain.md`). Those cells,
  taken at the moment the world is made, are the **field cells**. Nothing in Rust names
  the type; the flag decides.
* **How much ore is there now** is the ore state of the world (`sf_sim::Ore`): one amount
  (`u16`, whole units of ore) per field cell, and zero everywhere else.
* **The numbers** are data, `assets/config/ore.ron`, read by `sf_data::ore`, handed to the
  world by `sf_bridge::ore_rules` and `World::with_ore`:

| field | shipped | meaning |
|---|---|---|
| `start` | 120 | What every field cell holds when the world is made. |
| `max` | 160 | The most a cell can hold. Growth stops here. At least 1, not below `start`. |
| `regrow_amount` | 1 | What a cell below `max` gains in its turn. 0 turns regrowth off. |
| `regrow_period` | 200 | Ticks between two turns of the same cell (10 seconds). 1 to 72,000. |

The values are placeholders (pinned by `sf_data/tests/ore.rs`, so changing the file means
changing the test on purpose). `start` was chosen so that the sizes of the mission
fields come out about right at 120 per cell: F2-M03 names a 5 x 5 field of "roughly 3000
credits" (25 x 120 = 3000), F1-M04 a field of 6000 (a box of 8 x 6 cells), F1-M09 two of about 10,000
(about 80 cells). One unit of ore is one credit for now; what a unit is worth is the
refinery's rule (WP7.4).

A world has ore state **only if it was given ore rules** (`World::with_ore`, before the
first tick, valid numbers; otherwise `ConfigError::OreRules`). Every world without them,
which is every scenario of the earlier phases, is untouched, checksum included. A world
without a terrain layer has ore state with no field cells.

## Taking ore

`Ore::take(cell, want)` gives `min(held, want)` and lowers the cell by that. It is the
only way an amount goes down, it happens only inside the tick, and it is how the
harvester (WP7.2) will take ore. Until then `Command::TakeOre { cell, amount }`
(scenario: `TakeOre(cell: (x, y), amount: n)`) makes the same take, for scenarios and
tests. The command counts as **ignored** in a world without ore, on an empty cell, off a
field and off the map, and with an amount of 0. It changes no money.

## An emptied cell

**The terrain type does not change.** A cell that has been emptied is still an `ore`
cell: not buildable, the same movement cost, harvestable, and still a **field cell that
grows back**. Only its amount is zero. Reasons: the terrain never changes after the world
is made (its digest is hashed once), pathfinding and the movement costs read the type, and
a field that can be mined out for good would be a different, harsher rule than the "slow
regrowth" the plan asks for. The picture shows the amount, not the type (below). Tests:
`an_empty_cell_stays_an_ore_cell_and_grows_back` (sf_sim: still `ore`, not buildable, a
`Place` is refused, grows one step after a period).

## Regrowth

Every `regrow_period` ticks each field cell below `max` gains `regrow_amount`, clamped to
`max`. No random numbers. The cells are spread over the period by their place in the
ascending (row-major) list of field cells: in tick `t` the cells at places `t % period`,
`t % period + period`, ... have their turn. So

* a cell has its turn in the same ticks in every game, whatever happens elsewhere;
* a tick touches about `field cells / period` cells, not the map. Measured below.

Regrowth runs right after the commands of the tick, so a take and the growth of the same
cell in one tick are seen in that order (the cell is taken from first). An emptied cell and
a cell at the maximum are no different from the others: the first grows, the second is
skipped. Tests: `every_field_cell_grows_once_per_period_on_its_own_tick` (the exact ticks of
every cell of a 12-cell field over two periods), `regrowth_stops_at_the_maximum_...`,
`a_step_that_would_pass_the_maximum_is_clamped`, `zero_regrowth_amount_means_no_regrowth`,
and `depletion_and_regrowth_follow_the_rule` (sf_headless: all 215 cells of the playfield
after 600 ticks against a formula worked out from this paragraph alone).

## Finding ore (the API for the harvester)

* `World::ore_at(cell) -> u16`: what the cell holds (zero off the map, off a field, in a
  world without ore).
* `World::nearest_ore(from, max_radius) -> Option<Cell>` and
  `World::nearest_ore_where(from, max_radius, accept)`: the nearest cell with **amount > 0**
  that `accept` takes (a harvester will skip cells another one has claimed). "Nearest" is
  the smallest squared distance between cell indices (`dx*dx + dy*dy`); a tie goes to the
  **smaller row, then the smaller column**. The search walks square rings outward from
  `from` and stops as soon as no later ring can beat the best (or equal it and win the
  tie), so it costs about the area of a disc up to the answer, not the map; a search with no
  answer costs the area within `max_radius` (at most the map), so the caller gives a radius
  and should not search every tick. `from` off the map finds nothing. Tests: equal to a
  brute-force search for more than 500 combinations of start, radius and filter on a ragged map,
  the tie order, the radius limit, and the two ring cases (a nearer cell on a later ring;
  an equal distance on a later ring that wins the tie).
* `World::ore()` gives `&Ore` with `amount_at`, `is_field`, `cells()` (field cells with
  their amounts, ascending), `total()`, `version()` and `rules()`.

Only the API exists. There is no harvester, no refinery and no claiming of cells.

## Checksum

A new last section **`E`**, written **only when the world has ore state**: the four rules,
the number of field cells and one running sum. The sum is `sum(mix(cell index, amount))`
over the field cells, wrapping, kept up to date in constant time whenever an amount
changes (subtract the old term, add the new one), so the section costs 24 bytes of hashing
per tick whatever the map size. `the_running_sum_equals_a_full_recount` checks it against
a recount after 600 random takes and growths. A different rule is a different world from
the first tick (`the_checksum_has_an_ore_section_only_with_ore_rules`).

No earlier pin moved: `wander_5000`, `command_mix`, `terrain_256`, `units_spawn` and the
golden constants have no ore rules, so no section. `terrain_256` has ore *terrain* and its
checksum is unchanged. The new scenario `ore_256` is pinned
(`assets/scenarios/ore_256.expected`, test `tools/sf_headless/tests/ore.rs`, CI job
`headless`, Linux and Windows). It writes its ore rules **in the scenario file** (a short period, 20
ticks, so 600 ticks show regrowth), so editing `ore.ron` never moves it; the shipped
numbers are pinned separately.

`Command::TakeOre` is hashed in the queue (tag 11).

## What the player sees

The terrain type of a cell never changes, so the **look** does: `terrain_look.ron` has a
`resource` block for the type `ore` with four **fill levels** (cell fill in percent of
`max`: 0, up to 33, up to 66, up to 100). Each level has its own ground colour, a share of
the bright flecks (0, 30, 65, 100 percent) and its own flat colour on the minimap. A cell
shows the first level whose bound is at least its fill, `amount * 100 / max` rounded
down, and a non-empty cell shows at least the second level (never "empty"). So the
amounts are seen as: empty bare ground, a few flecks, half the flecks, the full look. A
fresh cell (120 of 160, 75 percent) shows the full look.

The terrain texture array gets one extra layer per level after the layers of the types;
`sf_game::OreView` compares the levels of a snapshot with what was drawn and reports only
the cells whose level changed; the renderer rewrites those texels (one write covering the
rows that changed) and the minimap repaints those cells and is uploaded again. A cell that
grows by one unit does not change the picture until it crosses a bound. Tests:
`sf_render/tests/ore_look.rs` (levels, textures against the minimap colours, flecks,
validation), `sf_ui/tests/minimap_paint.rs`, `sf_game/src/ore_view.rs`,
`sf_game/tests/ore_screenshot.rs` (the picture of a field full, part emptied and grown
back; saved to `docs/screenshots/wp7.1` when `SF_SHOT_DIR` is set).

Not done: the cell grid overlay (`cell_overlay.md`) does not show the amount; a renderer that
is never given ore levels keeps the plain ore look; the minimap texture is created anew each time a cell crosses a level (not measured
under a harvesting load, which does not exist yet).


## Cost

Measured on the same machine, alternating runs, under load (load average 6 to 7 from other
builds, so the numbers are relative):

* the worst case, a 256 x 256 map that is entirely ore (65,536 field cells, all growing):
  1.2 to 1.3 microseconds per tick against 0.1 with no ore rules
  (`cargo test --release -p sf_sim --test ore -- --ignored --nocapture ore_cost`);
* `wander_5000` (no ore rules, so the extra work is one `Option` test per tick): five
  alternating release runs of 10,000 ticks, baseline (main) 12,895 ms on average, this
  branch 13,205 ms (+2.4 percent, inside the 4 percent spread between runs of the same
  binary); the checksum is the pinned one.

## Who uses this

* **Missions** with ore: F1-M01 (3000 on the ridge), F1-M04 (6000), F1-M08 (about 6000 and
  about 12,000, an enemy field), F1-M09 (two of about 10,000), F1-M10 (three fields, one
  outside the terrace), F2-M01, F2-M03 (3000), F2-M04, F2-M06, F2-M08, F2-M09 (an enemy
  field), F2-M10. Missions without ore (F1-M03, F1-M05, F2-M02, F2-M05) have no ore cells in
  the terrain, so no field cells. **Gap:** the missions give a *total* per field (3000 in an
  80-cell ridge, 12,000 in a hollow); the world has one `start` for every cell. Fitting a
  field to its total (a zone and a number, spread over its cells in a fixed order) belongs
  to the mission loader and is not built.
* **WP7.2** (harvester): `nearest_ore`, `Ore::take`, `ore_at`, the version counter.
* **WP7.3** (storage) and **WP7.4** (money): what a unit of ore is worth, the cargo it fills.
* **WP8.x** (vision): whether the ore a player has not seen is hidden on the minimap (the
  minimap shows all ore now).
* **M1** (the vertical slice): a skirmish map with fields the player and the static
  opponent harvest.
