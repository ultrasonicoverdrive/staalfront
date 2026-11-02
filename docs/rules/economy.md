# Economy: ore on the map and harvesters

Work packages WP7.1 to WP7.5 (PLAN.md phase 7). This page holds the rules of the **ore on the
map** (WP7.1): how much a cell holds, how it is taken, how it grows back, how a harvester finds
it; and of the **harvesters** (WP7.2, from "Harvesters" below): the loop of search, mine, drive
back, check in at the refinery by messages, unload. Storage (WP7.3, "Storage" below) caps what a
player keeps; money (WP7.4, "Money" below) spends from that store. Rules in our own words.

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
harvester (WP7.2) takes ore. `Command::TakeOre { cell, amount }`
(scenario: `TakeOre(cell: (x, y), amount: n)`) makes the same take, for scenarios and
tests. The command counts as **ignored** in a world without ore, on an empty cell, off a
field and off the map, and with an amount of 0. It changes no money.

**Decision (WP7.2): `Command::TakeOre` stays as it is, as test and scenario scaffolding, and the
harvester does not use it.** It is lockstep-visible (hashed in the queue, tag 12) and a player
seat could send it to empty a field, exactly as a seat can send `SpawnUnit` and `PlaceStructure`
today (`buildings.md`, "Not done"). The simulation has no notion of a scenario seat that a command
could be refused for, and `ore_256` (pinned) uses it, so it is not removed or restricted here. It
joins `Spawn`, `Place`, `SpawnUnit` and `PlaceStructure` in the list of **setup commands that must
be refused from player seats before any untrusted command stream** (phase 14, replays from
outside). The harvester calls `Ore::take` inside the tick, never a command.

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

WP7.1 built only this API; the harvester (WP7.2, below) is its user: it searches with
`nearest_ore_where`, and a harvester's **claim** on a cell is what its filter skips.

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

`Command::TakeOre` is hashed in the queue (tag 12; 11 is `BuildStructure`).

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
is never given ore levels keeps the plain ore look. The minimap texture is created anew each time
a cell crosses a level; its cost under a real harvesting load is measured in WP7.2 ("The picture",
below).


## Harvesters (WP7.2)

A harvester is a unit whose role has the `harvests` flag (`unit_defs.ron`; the shipped `harvester`,
kind 11). A refinery is a structure whose building function is `HarvesterDock` (the shipped
`refinery`, kind 2, exit side `PosX`). A world has them only if it was given
`World::with_harvest(rules, harvester_kinds)` after its unit and building tables
(`sf_bridge::harvest_rules` and `harvester_kinds` make both from the data; otherwise
`ConfigError::Harvest`, also for a missing or empty building table, for invalid numbers and for
a kind that is not a mobile unit). The harvest state takes which kinds harvest and which dock from
the tables it is given, so a `with_units` or `with_buildings` after `with_harvest` is refused with
`ConfigError::Harvest` too (`the_rules_must_be_valid_early_and_name_mobile_kinds`). A world
without it is untouched, checksum included, and its harvesters just stand.

### The numbers (`assets/config/harvest.ron`, pinned by `sf_data/tests/harvest.rs`)

| field | shipped | meaning |
|---|---|---|
| `capacity` | 200 | Units of ore a harvester carries. |
| `mine_amount`, `mine_period` | 4 per 8 ticks | One bite and the ticks between two bites: 10 ore a second, a full load in 400 ticks (20 s). One ore cell of 120 lasts 12 s. |
| `unload_amount`, `unload_period` | 10 per 4 ticks | What the dock takes off in one bite, and the ticks between two bites (and before the first): 50 a second, a full load in 80 ticks (4 s). |
| `search_radius` | 40 | Cells a harvester looks for ore. |
| `retry_period` | 100 | Ticks between two searches of a harvester that found nothing, and between two looks for a refinery of one that has none. |
| `call_range` | 6 | A harvester asks the refinery for a place at this many cells from its dock point. |
| `grant_timeout` | 400 | Ticks a harvester that was granted the bay has to arrive. Must outlast the drive from `call_range` (a test holds the shipped numbers to twice the drive). |

The tuning of the loop (in the file since the WP7.2 fix round, PLAN section 5 rule 6; until then
constants in `harvest.rs`, with the same values):

| field | shipped | meaning |
|---|---|---|
| `call_timeout` | 10 | Ticks a harvester waits for the answer to its request before it asks again. 1 to 1000. |
| `max_tries` | 3 | Walks to one goal (an ore cell, the dock point) that end short of it before the harvester gives that goal up. 1 to 20. |
| `park_distance`, `park_spread` | 4, 5 | A harvester with nothing to do waits `park_distance` cells out from the dock point along the exit side, `slot % park_spread - park_spread / 2` cells sideways. 1 to 32 each. |
| `park_near_tenths` | 20 | A parked harvester within this many tenths of a cell of its spot stays. 1 to 320. |
| `mine_start_tenths`, `mine_reach_tenths`, `mine_lose_tenths` | 10, 15, 25 | Reach round the middle of the ore cell in tenths of a cell: stop and mine, count as there when the walk is over, drive back when pushed farther while mining. `1 <= start <= reach < lose <= 320`. |
| `dock_reach_tenths` | 20 | How near to the dock point (tenths of a cell) a harvester counts as docked when its walk is over. 1 to 320. |

The choice of refinery and the queue (WP7.5, "Refinery choice and queues" below):

| field | shipped | meaning |
|---|---|---|
| `travel_cost` | 16 | Ticks one cell of straight-line distance to a dock point is worth (about the harvester's speed). 1 to 1000. |
| `queue_cost` | 100 | Ticks every harvester already committed to a refinery adds to the expected wait there (a full unloading is 80 ticks, and the way in and out). 0 to 10000; 0 chooses by distance alone. |
| `switch_margin` | 100 | Ticks another refinery has to be cheaper by before a driving harvester switches. 0 to 10000. |
| `recheck_period` | 20 | Ticks between two looks of a driving harvester at the other refineries. 1 to 1000. |
| `queue_gap` | 3 | Cells to the side of the approach where the queue waits. 0 to 32; 0 leaves queued harvesters where they stand (the rule of WP7.2). |
| `queue_start`, `queue_step`, `queue_rows` | 1, 2, 8 | The first row of queue spots is `queue_start` cells out from the dock point along the exit side, the rows are `queue_step` cells apart, and from `queue_rows` rows on the places share the last row. 0 to 32, 1 to 8, 1 to 32. |

Distances are tenths of a cell so that the file holds whole numbers; the simulation turns them into
fixed point (`Fx::from_ratio(n, 10)`), no floats. Every number is at least 1, a bite no bigger than
the load, the tuning within the bounds above (`HarvestRules::is_valid` and
`HarvestSettings::from_ron` check the same; tests per field in `sf_sim/tests/harvest.rs` and
`sf_data/tests/harvest.rs`). `sf_bridge/tests/harvest.rs` pins the data-to-sim mapping with
literals. All are placeholders for phase 15 and the playtest of M1. One unit of ore is one credit
for now.

### The loop

| Stage (`HarvestStage`) | What the harvester does | Leaves it when |
|---|---|---|
| `Searching` | Looks for ore (below). Never seen for more than a tick: the search runs in the same turn. | It finds a cell: `ToOre`. None, load 0: `Parked`. None, some load: back to the refinery. Full: back to the refinery. |
| `ToOre` | Drives to the middle of its claimed cell. | Within 1 cell of the middle (it stops there, whatever is left of its walk): `Mining`. Its walk ended short: asks for a new route; after 3 tries it skips that cell and searches. The cell is empty: searches. |
| `Mining` | Takes a bite every `mine_period` ticks (`min(mine_amount, room, what the cell holds)`). | Full, or the cell is empty. Pushed more than 2.5 cells from the middle: `ToOre` again. |
| `ToRefinery` | Drives to the dock point of the standing refinery with the lowest expected wait (below); at `call_range` it sends its request. Until then it looks at the other refineries again every `recheck_period` ticks. | Granted: `Granted`. Queued: `Queued`. |
| `Queued` | Walks to its queue spot beside the approach and waits there (below); walks on when the queue moves up. | The dock grants it the bay. |
| `Granted` | Drives into the bay. | Within 2 cells of the dock point: sends `Arrived`, `Unloading`. |
| `Unloading` | Sits in the bay while the dock takes the load off in bites. | `Released`: back to the search, in the same turn. |
| `Parked` | Stands 4 cells out from its refinery along the exit side (spread sideways by `slot % 5`, so that they do not stand on one spot), and looks again every `retry_period`. | The next search. |
| `NoRefinery` | Holds its load where it stands, and looks for a standing refinery every `retry_period`. | One stands. |
| `Manual` | Obeys a player order that was not a field. | The order ends: back to the search. |

**The dock point** is the middle of the exit cell of the refinery (`buildings.md`, "The exit":
for `PosX` the cell next to the middle of the east side). The harvester drives to it and unloads
there.

**Choosing a refinery** (WP7.5, "Refinery choice and queues" below): among the **standing** refineries of the
harvester's own player, the one with the lowest expected wait: the straight-line distance times `travel_cost`
plus `queue_cost` for every harvester already committed to it. "Standing" is `World::is_standing` (the rule `placement.rs` and the tech tree use): hit
points left, or a structure that cannot be damaged, and since WP6.3 also complete: a refinery a
player builds with `BuildStructure` is no dock while it is under construction and becomes one when
it is complete (`a_refinery_under_construction_is_not_a_dock_and_becomes_one_when_complete`); the
zero-hit-point half is
`harvest::tests::a_refinery_at_zero_hit_points_is_not_a_refinery`. A refinery of another player
is not a refinery (`a_refinery_of_another_player_is_not_a_refinery`).

**Choosing ore:** `nearest_ore_where` from the harvester's own cell, within `search_radius`, among
the cells that pass the filter. The filter is the **claim**: a harvester claims the cell it
drives to and mines; no other harvester may choose a claimed cell. First a cell with room round it
(none of its eight neighbours claimed by another, because a harvester's body is wider than a
cell), then, if there is none, any cell nobody has claimed. A cell a harvester could not reach
three times is skipped by that harvester until it has mined somewhere. Claims are kept in a
`BTreeMap`, released when a harvester moves on, dies or is replaced in its slot. Tests:
`harvesters_claim_different_cells`, `harvesters_do_not_claim_cells_next_to_each_other_while_there_is_room`,
`when_there_is_no_room_a_harvester_takes_any_cell_nobody_has`,
`a_destroyed_harvester_releases_its_claim_on_an_ore_cell`.

### The messages

Harvester and refinery do not call each other (analysis sections 11 and 13): a harvester writes
into the **inbox** of a dock, a dock writes into the **mailbox** of a harvester, and each reads
its own box on its own turn.

| To the dock | Sent when | The dock |
|---|---|---|
| `Request` | The harvester is within `call_range` cells of the dock point and has no answer (it asks again after 10 ticks of silence). | Grants the bay if it is free and nobody waits, otherwise puts the harvester at the back of its queue. A harvester it already knows gets the same answer again. |
| `Arrived` | A granted harvester is within 2 cells of the dock point. | Starts unloading, if it granted that harvester. |
| `Cancel` | A player order took the harvester away from the dock. | Frees the bay or drops it from the queue. |

| To the harvester | Sent when | The harvester |
|---|---|---|
| `Granted` | The bay is its own. | Drives in (a queued harvester starts again). |
| `Queued(place)` | The bay is taken. | Walks to its queue spot and waits (a queue spot is beside the approach, below). |
| `Revoked` | The grant ran out before it arrived. | Asks again. |
| `Unloaded(n)` | The dock took `n` off its load in this bite. | Lowers its load by `n`. |
| `Released` | The load is gone, the bay is free. | Looks for ore. |

The dock owns the unloading: it adds `n` to the player's counter (below) and tells the harvester;
it never writes a harvester's state. Messages are kept as plain vectors in the order they were
sent (harvesters write in ascending slot order), and they are part of the checksum.

**One bay.** A refinery unloads one harvester at a time, `Reserved` from the grant to the arrival
and `Unloading` after it. The queue is first come, first served (`the_queue_is_first_come_first_served`,
`two_harvesters_on_one_refinery_take_turns_in_the_order_they_asked`). A grant that is not used
within `grant_timeout` ticks is revoked and the next in the queue gets the bay
(`a_grant_that_is_not_used_in_time_is_revoked_and_the_next_in_line_gets_the_bay`: with a timeout
of 10 ticks the first grant runs out before anyone unloads and before any ore is delivered, the first
of the queue holds the bay in the same tick, and the revoked harvester asks again and gets in later).
A holder that a player orders away (granted and driving in, or unloading) frees the bay with its
`Cancel`, the first of the queue gets it within two ticks, and nothing more is taken off the departed
harvester (`a_granted_holder_ordered_away_frees_the_bay_for_the_next_in_line`,
`an_unloading_holder_ordered_away_frees_the_bay_for_the_next_in_line`). A holder or a
queued harvester that has been destroyed is dropped from the dock on its next turn, and the bay
goes on (`a_destroyed_harvester_frees_the_bay_and_its_ore_cell`).

**Timing.** A request sent in tick `t` is read in the dock turn of `t + 1`, and the answer is read
by the harvester in the same tick: one tick for the request, none for the answer
(`a_request_is_answered_in_the_tick_after_it_was_sent`). The first unloading bite comes
`unload_period` ticks after the dock read `Arrived`, then one every `unload_period`
(`a_full_load_is_unloaded_in_bites_over_ticks`: 100, 80, 60, 40, 20, 0 at 3 ticks).

### A tick of harvesting, in two passes

Right after the commands and the ore's regrowth, before anyone moves (so the orders a harvester
gives are walked in the same tick):

1. **Sync.** The harvesters and docks of the world are found, in ascending slot order; the state of
   one that is gone, or whose slot was reused (the generation differs), is dropped and its claim
   released.
2. **Docks.** In ascending slot order each dock: checks its holder, drops the dead from the queue,
   reads its inbox in the order it was sent, counts the unloading down, and grants a free bay to
   the head of the queue.
3. **Decide.** Every harvester, in ascending slot order, works out what it does from the state at
   the start of this pass, and **changes nothing**: the result is a plan (its next state, a
   change of its order, a bite of ore, a message).
   One exception: the committed count per dock (how many harvesters have chosen it). A choice
   made earlier in this same decide pass counts at once for the harvesters after it. It is a
   local scratch vector, not world state, updated in ascending slot order, so it is deterministic.
4. **Apply.** The plans are carried out in ascending slot order. Where order could matter, it is
   decided here and by one rule: a cell two harvesters chose in the same tick goes to the lower
   slot (the other keeps its state and searches again next tick:
   `a_harvester_is_not_sent_to_a_cell_another_one_is_driving_to`); two harvesters biting one
   cell that cannot feed both are served in slot order.

### The cases

Each has a test (`crates/sf_sim/tests/harvest.rs` unless named otherwise):

* **No ore:** the harvester parks near its refinery and looks again every `retry_period`; when
  ore appears it goes
  (`without_ore_a_harvester_idles_near_its_refinery_and_looks_again_later`).
* **No refinery:** it mines a full load and holds it (`NoRefinery`), delivers nothing, and
  delivers when a refinery stands (`without_a_refinery_a_full_harvester_holds_its_load_and_waits`).
* **The refinery is destroyed** while the harvester drives to it, is queued at it, or is docked:
  the load it still carries stays with it, only what the dock had taken counts, and it chooses
  another refinery or waits for one
  (`a_refinery_destroyed_under_a_docked_harvester_leaves_it_the_rest_of_its_load`,
  `a_refinery_destroyed_under_a_queued_harvester_sends_it_to_the_next_one`,
  `harvest::tests::a_refinery_that_stops_standing_under_a_harvester_on_its_way_sends_it_to_the_other`).
* **The harvester is destroyed:** its load is lost (never delivered, gone from the world), its claim
  and the bay are freed (`a_destroyed_harvester_loses_its_load`).
* **Two or more harvesters on one refinery:** they take turns, one in the bay, the rest queued and
  standing (`two_harvesters_on_one_refinery_...`; the `harvest` scenario has three).
* **Several refineries:** the one with the lowest expected wait, which is the nearest one while nobody is
  committed to it (`the_nearest_standing_refinery_wins`; the zero-hit-point case above); the rule and its
  tests are in "Refinery choice and queues".
* **A player order to a field overrides the search.** A harvester whose current order is not its
  own (it compares what it ordered with what it has) was given one. A `Move` to a cell that holds
  ore, while the harvester is not full, makes it search **from that cell**, so it mines there (or at
  the nearest cell nobody has claimed), and it lets go of whatever it had with a dock (`Cancel`).
  Any other order (a move elsewhere, a full harvester, hold, guard) is obeyed as it is
  (`Manual`) and when a `Move` ends the loop resumes where the harvester stands
  (`a_player_order_to_a_field_overrides_the_search`,
  `a_player_order_that_is_not_a_field_is_obeyed_and_the_loop_resumes_after_it`,
  `a_player_order_cancels_the_check_in`). Two harvesters sent to one ore cell in one tick: the
  first keeps the cell, the second loses it in that tick's claim check and keeps the field as its
  search focus, so it mines in the field the player chose and not in the one it stands next to
  (`two_harvesters_ordered_to_one_ore_cell_in_one_tick_both_keep_the_field`). **Gap:** `Stop` ends a walk like any other, so a
  stopped harvester goes back to work at once; there is no way to park one.
* **A harvester stuck in traffic re-plans.** The harvester asks for no route itself: the crowd
  pass (WP4.4, WP4.7) notices a unit that does not get on and asks for a new route round the
  bodies that stand still, and gives the order up after `replan_max` tries. If the order is given
  up the harvester sees a walk that ended short and orders it again, three times, then skips that
  ore cell (or gives that refinery up and waits a `retry_period`). Test
  (`tools/sf_headless/tests/harvest.rs`, `a_harvester_stuck_behind_a_holder_asks_for_a_new_route_and_still_delivers`):
  a wall with two openings one cell wide, a heavy tank on hold in the upper one. The harvester,
  on the shipped data with pathing, stood against the tank for about 430 ticks, asked for one new
  route, crossed by the lower opening, never crossed by the upper one, and delivered; without the
  tank it crossed by the upper one and never replanned. The 430 ticks are WP4.7's stuck timing
  (`stuck_ticks`, `replan_cooldown`), not the harvester's. The harvester's own re-issue of a walk
  that was given up (and the three tries) is **not** covered by a test of its own.

### Where the ore goes: `ore_delivered`

`World::ore_delivered(player) -> u64` is the ore the refineries have taken off the harvesters of
a player so far: a **statistic**, a plain counter kept per player in a sorted vector, hashed in the
checksum. Since WP7.3 the ore also goes into the player's **store** (`World::ore_stored`, "Storage"
below), which is capped; with storage rules every unit delivered is a unit stored, so the two are
equal until money rules (WP7.4, "Money") spend from the store.

### Checksum

A new section **`H`**, after `E`, written **only in a world given harvest rules**: the rules, the
harvester kinds, the ore delivered per player, then every harvester in slot order (load, stage,
timer, claimed cell, search focus, skipped cell, refinery, the order it expects, tries, unread
messages) and every dock (bay, queue, inbox). Tests: `the_checksum_covers_the_harvest_state`,
`the_checksum_has_a_harvest_section_only_with_harvest_rules`. **No pinned value of an earlier
phase moved:** `golden.rs`, the `.expected` files of `wander_5000`, `command_mix`, `terrain_256`,
`units_spawn`, `placement`, `destruction`, `tanks_vs_infantry` and `ore_256` and the WP6.1 digest
`0x53aeb53ab44157d1` pass unchanged (none of those worlds has harvest rules, so none has the
section; `wander_5000` ends on the same checksum, `0x38d5e3c43358a889`, in the `main` binary and in
this branch). The new scenario `harvest` is pinned (`assets/scenarios/harvest.expected`,
`tools/sf_headless/tests/harvest.rs`, the CI job `headless`). **Re-pin in the fix round:**
`0x60e1c19e00a28c45` became `0x655fd8c1472d1611` because the nine tuning numbers that moved from
`harvest.rs` into the rules are hashed with the other rules; a build that leaves just those nine out
of the hash gives `0x60e1c19e00a28c45` again (measured), so the run itself did not change. It runs on `harvest_defs`, a **new**
frozen copy of the shipped definitions of WP7.2 (no existing copy was touched), and writes its ore
and harvest rules in the scenario file, so editing `ore.ron` or `harvest.ron` never moves it.

### The scenario

`harvest` (4500 ticks, the playfield): a refinery, three harvesters of player 0 that go round a
rock patch to a field of 28 cells and queue on the one bay, and a harvester of player 1 that has no
refinery. The test asserts that every harvester of player 0 runs the whole loop in order between
two unloadings (`ToOre`, `Mining`, `ToRefinery`, `Granted`), that never two are in the bay, that
somebody queued, that the player without a refinery delivers nothing and ends holding 200, that
what the refinery took equals the sum of the unloading bites the test saw, and the delivered
total: **1680** (eight whole loads of 200 and 80 of the ninth, the bay being mid-unload at tick
4500). It also asserts conservation: the ore left in the cells plus the loads carried plus the ore
delivered never falls, and rises only by the regrowth (at most 2 a tick).

### The picture

* **Cargo bar.** `sf_game::cargo_view`: a small dark bar under every harvester, filled from the
  left in ore colour by `load / capacity` (brighter when full), on the world layer of the UI, drawn
  for every harvester whether selected or not. The load comes from `SimSnapshot::cargo_of`. A
  frame with no harvester skips the pass. Test: `crates/sf_game/tests/harvest_screenshot.rs`
  (real renderer on a software adapter: no fill before the first bite, fill while mining, the
  brighter fill when full, the cells under the harvesters drain in the picture, the picture at the
  queue and at the bay), with the unit tests of `filled_px`. `SF_SAVE_SHOTS=<dir>` writes the
  frames; the ones in `docs/screenshots/wp7.2` are described in its README.
* **`sf_game --harvest`** (sandbox): the world gets the harvest rules, and Meridian a refinery and
  three harvesters beside the ore field nearest the middle of the map. Without the flag the sandbox
  has no harvest rules and stands as it did. `--focus x,z` (metres) now also decides where `--shots`
  look.
* **The ore drains in the picture** through the levels of WP7.1: a cell the harvesters empty goes
  from the full look to the bare look and the minimap follows.

**The minimap upload under a real harvesting load** (the gap WP7.1 left): every tick in which some
cell crossed a fill level is one `UiRenderer::set_minimap`, which makes a new texture and bind
group. `crates/sf_game/tests/harvest_minimap_cost.rs` counts them in the game's own path
(`SimSnapshot`, `OreView`) for the playfield with the shipped ore and harvest rules, three minutes
of game time (3600 ticks), and times one upload on the software adapter (Mesa llvmpipe, a CPU):

| harvesters | ticks with a change (uploads) | at most in one second | cells changed | ore taken |
|---|---|---|---|---|
| 4 | 126 | 8 | 128 | 2380 |
| 50 | 152 | 10 | 159 | 3348 |

(Re-measured in WP7.5 with the choice of refinery and the queue spots; the WP7.2 numbers, with the
jammed bay, were 105, 7, 105, 2216 and 205, 10, 214, 3168.) The 50-harvester row is still **not a
harvesting load of 50 working harvesters**: the sandbox has one refinery and one near field, and with that
many harvesters most are crowded in the field (see "Refinery choice and queues", "Throughput"). The upload
stays sound anyway: even one upload every tick would be about 20 x 0.22 ms = 4.4 ms a second on llvmpipe.

One upload of the 256 x 256 image took **209 to 237 microseconds** (five runs). The comparison of the
levels, `OreView::changes`, which the presentation does for every new snapshot, took **1.4 to 1.5
microseconds** a tick for the 215 field cells of the playfield (copying those cells into the snapshot
is not timed on its own; it is the same 215 tuples). So the worst
second seen costs about 2 ms on a software adapter (0.2 percent of the second), and the three
minutes 0.02 to 0.05 s in all. **It stays as it is**: nothing here is a reason to keep the
texture and only write to it. (A real GPU's `create_texture` is not a software adapter's; the
frequency, at most ten a second here, is what carries over. The laptop benchmarks are where to
look if it ever shows.)

### Cost of harvesting

Release build, five alternating runs of the same scenario (`harvest_cost` in
`tools/sf_headless/tests/harvest.rs`, 48 harvesters of one player and three refineries on the
playfield, 4000 ticks), on a machine with a load average of 1.5 to 2 from other work:

* with the harvest rules **776.8 ms** (mean of 743 to 830), the same units without them (they
  stand where they are put) **160.2 ms**: 154 microseconds more per tick, **but that is the cost
  of 48 harvesters moving, routing and pushing each other**, not of the harvest stage. (These 48
  harvesters were a jammed system in WP7.2: one refinery got them all. Since WP7.5 they use all
  three, see "Refinery choice and queues", "Cost".);
* the harvest stage itself, timed from inside the world for the same runs (a scratch build with a
  clock around the stage, not committed, since the simulation has no clock): **23 to 30 ms for the
  4000 ticks, about 6 microseconds per tick** with 48 harvesters, every `nearest_ore_where` search
  they made included, against the 50,000 microseconds of a tick;
* `wander_5000` (no harvest rules, so the cost is one `Option::take` per tick): five alternating
  runs of the release binary, `main` 786, 772, 752, 704, 792 ticks/s, this branch 781, 764, 767,
  780, 797 (means 761 and 778), machine load average 4 to 7 from other builds: no difference
  beyond the noise, same final checksum.

These are relative numbers; the fps requirement is for the laptop benchmarks (PLAN section 13).

### Not done

* Since WP6.5 the refinery's `comes_with: "harvester"` flag gives one harvester when the refinery is built
  (`production.md`, "The refinery's free harvester"). Storage and credits are WP7.3 and WP7.4.
* A harvester is not ordered **onto a refinery** by the player to unload there, nor held on a
  field; `Stop` cannot park one.
* A refinery is chosen by the expected wait since WP7.5 ("Refinery choice and queues"); what is still
  not done there: the drive is the straight-line distance, not the length of the route, so a refinery
  behind a rock patch looks nearer than it is; and one small ore field cannot feed many harvesters (the
  delivery of 48 harvesters on the shipped field is still below that of 12; measured in "Throughput").
* No alliances: a harvester uses only the refineries of its own player.
* The refinery's exit cell is only checked when it is placed; one that is blocked later makes
  harvesters give that refinery up after three short walks (that path has no test of its own).
* Fields are not fitted to the mission totals (WP7.1's gap), and there is no fog (WP8.x): a
  harvester sees all ore within `search_radius`.

## Cost (the ore, WP7.1)

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
* **WP7.2** (harvester, built): `nearest_ore_where`, `Ore::take`, `ore_at`, the version counter.
* **WP7.3** (storage) and **WP7.4** (money): what a unit of ore is worth; they replace or extend
  `World::ore_delivered` (above), the unload bite is where they hook in (`Unloaded(n)` in the dock turn).
* **WP8.x** (vision): whether the ore a player has not seen is hidden on the minimap (the
  minimap shows all ore now).
* **M1** (the vertical slice): a skirmish map with fields the player and the static
  opponent harvest.

## Storage (WP7.3)

A player's ore goes into one **store**, held together by all the player's standing buildings that
store ore. The store has a **capacity**; a refinery takes off a harvester only what still fits.

### What stores how much

* **The data:** the building block of a structure in `unit_defs.ron` has `storage`, the units of
  ore one such standing building holds for its owner (a whole number, default 0, at most
  `MAX_STORAGE` = 1 000 000; `sf_data::units::building`, refused at the line otherwise). As
  shipped only the **refinery** stores, **2000** (ten harvester loads; a placeholder for phase 15).
  A silo (WP7.3b, after the tech tree is approved) will be a record with a `storage` number and no
  Rust change. Pinned: `sf_data/tests/buildings.rs`
  (`only_the_refinery_stores_ore_and_it_stores_the_shipped_amount`,
  `storage_is_a_whole_number_up_to_the_maximum`); the frozen copies have no `storage` and store
  nothing (`the_frozen_copies_store_nothing`).
* **To the world:** `sf_bridge::storage_capacities(defs)` gives `(kind, units)` for every structure
  with `storage` above zero, ascending (pinned with the literal `[(Kind(2), 2000)]` in
  `sf_bridge/tests/harvest.rs`), and `World::with_storage(&table)` turns storage on. It must come
  after `World::with_harvest` and before the world starts, once, with kinds of the building table,
  each once and above zero (`ConfigError::Storage` otherwise;
  `storage_is_given_once_after_the_harvest_rules_and_before_the_start`). **A world without it has
  no limit** and hashes as before: that is every scenario before WP7.3, `harvest` included. The
  sandbox gives it with `--harvest` (`sf_game::playfield::with_storage`).
* **The capacity** of a player is the sum of the storage of its **standing** buildings: complete
  and with hit points (`World::is_standing`, as for the docks). A building under construction holds
  nothing until it is complete; a building at zero hit points holds nothing. It is summed in the
  sync pass of the harvest stage (before the docks run), in ascending slot order, every tick
  (`World::storage_capacity`). Tests: `storage_counts_only_the_standing_buildings_of_the_owner`,
  `a_refinery_under_construction_stores_nothing_until_it_is_complete`,
  `harvest::tests::a_refinery_at_zero_hit_points_stores_nothing`.

### When the store is full

The dock works out its bite as before (`unload_amount`, at most the load) and **cuts it to the room
left** (capacity minus stored). So the last bite before full is short
(`the_last_bite_is_cut_to_the_room_left`: 20, 20, then 10 into a store of 50). A bite of zero
takes nothing and sends no `Unloaded`; the harvester **stays in the bay, docked, holding the rest of
its load**, and the dock tries again every `unload_period`. The bay stays its own, so the harvesters
behind it **wait in the queue** as they do behind any unloading harvester. As soon as there is room
(a new refinery, a silo, later spending) the next bite goes in, and the loop goes on
(`a_full_store_takes_nothing_and_the_harvester_waits_in_the_bay_with_its_load`,
`a_second_refinery_raises_the_capacity_and_the_waiting_harvester_unloads`). Why: it is the
smallest change to the dock (one `min`), it keeps every unit of ore somewhere (in the store or on a
harvester), and the player sees the cause at the refinery: a full bar and a harvester that stays.

### When the store shrinks

A refinery that is destroyed, sold or removed takes its capacity with it from the next sync, but
**the ore stays in the store**: nothing is clamped or lost, the store may hold more than its
capacity, and nothing more goes in until it is below the capacity again (money, WP7.4, spends from it). Why:
it is the simplest deterministic rule (no choice of what to throw away, no rounding), and no ore
vanishes. Test: `a_lost_refinery_lowers_the_capacity_and_the_store_keeps_its_ore` (120 stored, the
capacity drops to 60, after 1500 ticks still 120).

### Checksum

A new section **`V`**, written right after `H` and **only in a world given storage rules**: the
capacity table (kind and units), then per player the ore stored and the capacity of the last sync
(`checksum.rs` table; `harvest::tests::the_storage_state_is_in_the_checksum` changes each in turn,
`a_world_without_storage_rules_has_no_storage_section` shows the hash without the section is the
old one). `ore_delivered` stays in `H` as it was. **No pinned value moved:** `harvest` ends on
`0x655fd8c1472d1611` as before (it has no storage rules), and so do all the other `.expected` files
and `golden.rs`.

### The scenario

`storage` (6000 ticks, the harvest playfield, field and rock patch; `harvest_defs` with the
capacities written in the scenario as `storage: Some([(2, 600)])`, so the shipped data never moves
it): one refinery of 600, two harvesters; a second refinery at tick 2600, removed at tick 4200. The
test (`tools/sf_headless/tests/storage.rs`) asserts: the store reaches 600 before tick 2400 and
stays there; from tick 2400 to 2600 harvester 2 waits in the bay holding its full 200; the second
refinery makes the capacity 1200 at tick 2601, the waiting harvester unloads, and the store is at
1200 by tick 4199; from tick 4201 to the end the capacity is 600 and 1200 stay stored; at the end
one harvester is in the bay with 200 and the other queued with 200; stored equals delivered; no
rise ever ends above the capacity and the store never falls. Pinned in
`assets/scenarios/storage.expected`, run in the CI job `headless`.

### The picture

* **Storage bar.** `sf_game::storage_view`: under the front edge of every standing structure that
  stores ore, where the construction bar lies (which a complete structure no longer has), a bar
  filled by the owner's stored / capacity (one store, so every refinery of a player shows the same
  fill), in its own colour when full. A store with ore and no capacity left is full. The look
  (colours, sizes) is `assets/config/storage_look.ron` (`sf_data::storage_look`, pinned in its unit
  tests). The snapshot carries the store per player and the storing structures
  (`SimSnapshot::storage_of`, `stores`). Test: `crates/sf_game/tests/storage_screenshot.rs` (real
  renderer on a software adapter: empty, half, full in the full colour, then a second refinery
  halves both bars); frames in `docs/screenshots/wp7.3`.
* **Readout.** The F1 panel, section Simulation, has one line per player with a store:
  `ore store player 0: 600 / 2000`, with `(full)` when nothing more fits
  (`the_panel_shows_each_ore_store`).

### Not done

* Nothing spends the store in a world without money rules. With the shipped 2000 and no spending, such a
  world's store fills after ten loads and the harvesters then wait at the refinery for good; with money
  rules (WP7.4) spending frees room, see "Money".
* No silo (WP7.3b). The construction centre stores nothing.
* The storage readout is in the F1 debug panel; the player HUD (WP7.4) shows the credits, not the capacity.
* A harvester waiting in a full refinery's bay keeps that bay; it never leaves for another
  refinery by itself (WP7.5 chooses a refinery on the way, not from the bay). The store is one per
  player, so another refinery would be full too.

## Money (WP7.4)

Credits come from the ore store; a structure is paid for while it is built. The module is
`sf_sim::money`, the world turns it on with `World::with_money`.

### The pool

* **The store is the money pool.** A unit of ore the refinery takes goes into the player's store as
  `credits_per_ore` credits (shipped 1, `assets/config/money.ron`, one unit of ore is one credit for
  now), and what is spent comes out of the same number. `World::credits(player)` is that number (and
  `World::ore_stored`, which it equals; with a price above 1 the store counts credits, not units).
  Why: one number, no conversion rule, and the WP7.3 "harvesters wait for good" state ends by itself:
  spending makes room, so the next bite goes in. Test
  (`spending_makes_room_and_a_waiting_harvester_unloads`): a store of 100 full of starting credits, a
  harvester waiting in the bay with 100 ore; a tower is built and ore goes in while it goes up.
* **The capacity is counted in credits.** The dock's bite is cut to the whole units of ore that fit,
  `room / credits_per_ore` rounded down (`a_unit_of_ore_is_worth_its_price_and_the_store_counts_credits`:
  price 3, capacity 400, the store ends at 399 with 133 units delivered).
* **Starting credits** are put into the store when the world is made, per player
  (`World::with_money(rules, &[(player, credits)])`; a player not listed starts with 0). The shipped
  `starting_credits` is 1000 for each seat of the sandbox (`playfield::with_money`); a scenario writes
  its own (`money: Some((credits_per_ore: 1, starting: [(0, 300)]))`). The store may then hold more than
  its capacity (no refinery yet, or a refinery lost): nothing is clamped and nothing goes in until
  spending brings it below (`starting_credits_above_the_capacity_stay_and_fill_nothing_more`).
* **Configuration.** After `with_storage` (the store must exist), before the start, once; refused with
  `ConfigError::Money` otherwise, for a price outside 1 to 1000, more than 1,000,000,000 starting credits
  or a player twice (`money_is_given_once_after_the_storage_rules_and_before_the_start`). **A world without
  it builds for free and hashes as before** (below). In the sandbox it exists with `--harvest`, which is
  what gives the world a store; without `--harvest` structures stay free and the HUD shows no credits.

### Paying while building

A structure of price `C` and `T` build ticks costs, for the tick that takes it from `k` to `k + 1` ticks
done, `C * (k + 1) / T - C * k / T` (integer division; `money::tick_cost`). The costs add up to exactly
`C` (`the_ticks_of_a_build_add_up_to_exactly_the_price`, including `u32::MAX` over 5000), no tick costs
more than `C / T` rounded up, and the remainder falls on the later ticks (7 over 3 is 2, 2, 3). The cost
of a tick depends on `k` alone, so **no state is kept next to `build_left`**.

* **When.** In `advance_construction`, the first thing a step does, in ascending slot order, each
  structure pays its tick before it advances. The structure placed by a command of tick `t` pays first in
  tick `t + 1`.
* **No money, no progress.** If the owner's store holds less than the tick costs, nothing is paid (not a
  part), `build_left` does not move, and the structure is listed in `World::stalled()` (replaced by every
  step, not hashed: it follows from the hashed state); it tries again next tick, so it resumes when ore
  arrives. With too little for several, the lower slot is served first, and a higher one that still
  fits in what is left is served too (`with_too_little_money_for_two_the_lower_slot_is_paid_first`). Slots are reused, so the lower slot
  is not necessarily the older structure. The rule is greedy and deterministic, not fair: with a trickle
  of income the lowest slot is served every tick it can pay, and a higher slot only gets what is left.
* **The owner pays**, from its own store (`it_is_the_owner_who_pays`). `BuildStructure` is still accepted
  with no money at all: it is placed and waits (gap: placement is not refused for lack of funds).
* **Destroyed or removed mid-build: no refund.** What was paid stays spent
  (`what_was_paid_for_a_structure_destroyed_half_built_is_not_refunded`: removed after 21 ticks, 52 of 100
  stay spent).
* **`PlaceStructure` costs nothing** (mission objects and start states, `mission_objects_cost_nothing`).
* **A hit-point-less wreck under construction** does not advance and so does not pay.
* The building table guarantees a structure has a price exactly when it has build ticks, so "free" can
  only be a structure that cannot be built.
* **Units** cost money when production exists (WP6.5). The hook is `World::pay(player, n)`
  (`pub(crate)`): it takes all of `n` or nothing. Production pays through it; unit costs, the refund on
  cancel and no refund for a lost factory are in `production.md`. **Structures first, then factories, each in
  slot order**: `advance_construction` pays before the commands of a tick, `advance_production` after them.
* `World::credits_spent(player)` is a statistic (hashed).

### Checksum

A new section **`M`**, after `V`, written **only in a world given money rules**: the price per unit of
ore, then the credits each player has spent (ascending). The credits themselves are the stored amount of
`V`. Tests: `harvest::tests::the_money_state_is_in_the_checksum` (price, spent, who spent),
`a_world_without_money_rules_has_no_money_section`. **No pinned value moved**: `golden.rs` and every
`.expected` of an earlier scenario, `harvest` and `storage` included, pass unchanged (none has money
rules); the bench PLAIN and FIGHT lists are pinned by `tests/bench_scene.rs`, unchanged. The new
scenario `money` is pinned (`assets/scenarios/money.expected`, `tools/sf_headless/tests/money.rs`, CI job
`headless`).

### The scenario

`money` (5400 ticks, the storage playfield, refinery capacity 600, two harvesters, player 0 starts with
300 credits): two power plants (300 credits over 300 ticks each) at tick 1; a barracks (400 over 400) at
tick 4650. Asserted: the credits fall 2 a tick for 150 ticks to 0 (`the_credits_drain...`); then both
plants stand still at 150 ticks to go, with 0 credits, until the first ore (after tick 800 on this layout), go on in bursts as
loads are unloaded, and are complete before tick 1500; at every tick credits = 300 + delivered - spent and
spent = one credit per tick of construction done; the store is full (600) with a harvester waiting in the
bay with 100 for 50 ticks before tick 4650, and ore goes in within 20 ticks of the barracks being placed.

### The picture

* **Readout.** `Credits 1000`, in the warning colour when below `low_below` or while a build waits.
  In the sandbox it is the top block of the sidebar (WP11.1, `docs/rules/sidebar.md`); there the
  strip `build paused: no credits` is gone, and the warning colour of the figure plus "no credits" on
  the waiting item say it instead. Without a sidebar (the crowd scene) `sf_game::money_view` draws the
  old panel in the top right corner, with that strip under it while a structure of the local player
  waits. Both are drawn by the HUD (`Hud::frame`) from the snapshot (`SimSnapshot::credits_of`,
  `waits_for_money`); only in a world with money rules.
* **Paused bar.** A structure in `World::stalled()` gets the construction bar in other colours and two
  small pause strokes at the right end (`construction_view::draw_bars`, `money_view::pause_strokes`).
* **Look.** Every number, colour and word is `assets/config/money_look.ron` (`sf_data::money_look`,
  pinned). Test `crates/sf_game/tests/money_screenshot.rs` (real renderer on a software adapter: building,
  paused, resumed; pixel checks of the colours; the readout is drawn by `money_view::draw_panel`, the
  function the HUD calls, not through the window); frames in `docs/screenshots/wp7.4`.

### Cost

`money_cost` (ignored test, release, 50 harvesters and twelve plants going up, 7 alternating rounds,
load average 1 to 4 from other work, so relative): the means with and without money rules differ by
-10 to +47 microseconds per tick between runs of the same binary, i.e. **below the run-to-run noise** of
about 5 percent (about 35 ms of 750); no extra cost can be shown at this resolution. The work added per
tick is, per structure under construction, one table lookup and one store update.

### Not done

* Unit costs, the refund on cancel and the lost factory (no refund) are WP6.5's, see `production.md`. No refund for structures, sell or repair (WP6.6), no silo (WP7.3b), no sidebar (WP11.1). (The refinery choice is WP7.5.)
* `BuildStructure` is not refused for lack of funds; it waits.
* Money exists only with `--harvest` in the sandbox, and the HUD shows a player's credits also when it owns
  no entity (they are read from `World::credits`, not from the views).
* The model of a structure under construction is not part of the screenshot test (the harness draws the
  roster's models, not the build stages).

## Refinery choice and queues (WP7.5)

WP7.2 measured the collapse: with three refineries, 4000 ticks, the first n harvesters of the crowd
scenario deliver 2400 (3), 3200 (6), 3200 (12), 2271 (24), 618 (48). Every harvester took the nearest
refinery by straight line, so one bay got them all (in the 48 run the bays of the other two refineries
were never used), and the queue stood in the approach to that bay. Two rules fix both halves; the dock
protocol (request, grant, queue, revoke, cancel, release) is unchanged.

### Choosing a refinery

A harvester that is full (or has found no more ore and carries some) compares the standing refineries
of its player by an integer cost in ticks and goes to the lowest:

`cost = round(distance in cells x travel_cost) + committed x queue_cost`

* **Distance** is the straight-line distance from the harvester to the dock point (`Vec2::length`, an
  integer square root: no floats, no A\*; the same number on every platform). It is an estimate: it does
  not see a rock patch in between. The queue term has to do the spreading, and does.
* **Committed** is the number of harvesters bound for that dock at that moment: driving to it, called,
  queued, granted or in the bay (`Harvest::committed`, one count over the harvesters at the start of the
  decide pass). A harvester that chooses in the pass is counted at once, so the next one in slot order
  sees it: two harvesters that set out in the same tick do not both take the same bay
  (`harvesters_that_set_out_in_one_tick_see_each_others_choice`). There is no separate "bay busy" term:
  the harvester in the bay is one of the committed.
* **Ties** go to the smaller cost, then the smaller squared distance, then the lower slot of the
  refinery (`the_cheapest_refinery_is_chosen_and_a_tie_goes_to_the_nearer_then_the_lower_slot`: cost 136
  against 136 goes to the nearer one; equal cost and distance to slot 0).
* With `queue_cost: 0` this is exactly the rule of WP7.2 (nearest, a tie to the lower slot).

**When it is chosen again.** On leaving the field (`go_return`), and then every `recheck_period` ticks
while the harvester is still driving, not yet within `call_range` of the dock point and not known to the
dock. It switches only if the best other refinery is cheaper than its own (its own cost does not count
the harvester itself) by **more than `switch_margin` ticks**: that is the hysteresis, so a harvester does
not change its mind every time a queue gains or loses one
(`a_driving_harvester_switches_only_when_the_other_refinery_is_cheaper_by_more_than_the_margin`: cheaper
by 241 switches at margin 240 and stays at margin 241). Once it has asked (`Request`), it keeps its
place: a harvester in the queue never leaves it for another refinery, because that would give up the
first come, first served order and put it at the back of another queue. A refinery that stops standing
sends its harvesters to a new choice at once, as before.

### The queue beside the approach

A harvester that is told `Queued` walks to a **queue spot** and waits there; when the queue moves up it
walks to its new spot. The spots lie on both sides of the **approach** (the line out from the dock
point along the exit side), `queue_gap` cells to the side, in rows `queue_step` cells apart, the first
row `queue_start` cells out (shipped 3, 2 and 1: the first row is 3 cells to the side and 1 cell out,
the next 3 cells out, and so on up to `queue_rows` rows).

* **Its own side.** A harvester keeps the side it came from when it was queued, so it never crosses the
  approach, and takes the next row after the harvesters ahead of it in the queue on that side
  (`a_queued_harvester_takes_a_spot_on_its_own_side_and_the_next_row_after_those_ahead`). From
  `queue_rows` rows on the places share the last row.
* **The holder reaches the bay.** The harvester that is granted the bay drives from its spot straight to
  the dock point; the approach holds only harvesters that are driving in. A queued harvester that is
  granted while still walking to its spot turns to the dock point at once
  (`a_queued_harvester_called_in_while_walking_to_its_spot_turns_to_the_dock_point`). The test
  `queued_harvesters_wait_beside_the_approach_and_everyone_still_unloads` (five harvesters on one
  refinery) records every standing queued harvester within 8 cells of the dock: none stood closer than 2
  cells to the line of the approach, all five held the bay and at least five loads went in.
* **A walk that ends short** of the spot (a jam gives it up) is tried again `max_tries` times for the
  same spot and then given up: the harvester stands where it is until its spot changes.
* `queue_gap: 0` switches the spots off: queued harvesters stand where they were told to wait, as in
  WP7.2 (`without_queue_spots_queued_harvesters_stand_in_the_approach`).
* The harvester's `spot` (row and side) is part of the checksum.
* **In the picture** (`queued_harvesters_stand_beside_the_approach_in_the_picture`, real renderer,
  models, seven harvesters on the one refinery; `docs/screenshots/wp7.5`): measured with the shipped
  numbers, the four queued harvesters stood 1.3 to 4.4 cells off the line of the approach (one that the
  crowd had stopped short of its spot at 1.3); with `queue_gap: 0` one stood 0.3 cells from it, in it.

**Gap, not done:** parked harvesters (no ore in reach) still stand `park_distance` cells out along the exit
side, spread over `park_spread` cells including the middle, i.e. partly in the approach.

### Checksum and pins

The eight new rules and the `spot` of a harvester are hashed with the other `H` section data, so every
world with harvest rules hashes differently from before; worlds without harvest rules hash as before
(`golden.rs`, `wander_5000` `0x38d5e3c43358a889`, `command_mix`, `terrain_256`, `units_spawn`,
`construction`, `placement`, the benchmark lists: all unchanged). Test
`the_new_numbers_are_in_the_checksum_each_on_its_own`.

| scenario | before | after | measured cause |
|---|---|---|---|
| `harvest` | `0x655fd8c1472d1611` (1680 delivered) | `0xfdd484efbd0c9d55` (1800) | The queue spots. With `queue_gap: 0` the scenario delivers 1680 again, and `queue_cost: 0` against 100 changes no harvester (one refinery): `without_queue_spots_the_scenario_delivers_the_old_total`. |
| `storage` | `0xe8092c6911bdf391` | `0x55dc456c2724d89d` | The queue spots (one refinery). |
| `money` | `0xdc71e0b9c80a1e78` | `0x5d73660fc2a02030` | The queue spots (one refinery). |

How the cause was measured: a scratch build that leaves the eight rules, the harvester's `spot` and the
recheck timer (`wait` of a driving harvester) out of the hash, with `queue_gap: 0` and `queue_cost: 0` in
the three scenario files, gives the old values (`0x655fd8c1472d1611`, `0xe8092c6911bdf391`,
`0xdc71e0b9c80a1e78`); the same build with `queue_gap: 3` (and `queue_cost` 0 or 100) gives other values,
and with `queue_gap: 0` and `queue_cost: 100` the old ones again. The release binary of the previous
commit (`origin/wp/7.4-money`) gives the three old values on the old files. Only the scenario RON files
wrote the harvest rules, so only they got the eight new lines; no `*_defs` copy was touched.

### Throughput

The review's measurement (`crowd_scenario`: a lattice of harvesters east of the rock patch, three
refineries, 4000 ticks, the first n harvesters; release; the ignored test `throughput_table`), ore
delivered to the player:

| harvesters | 3 | 6 | 12 | 24 | 48 |
|---|---|---|---|---|---|
| shipped ore (120 a cell), before | 2400 | 3200 | 3200 | 2271 | 618 |
| shipped ore, after | 2310 | 3642 | 3807 | 2805 | 2321 |
| 2000 ore a cell, before | 2990 | 3270 | 2800 | 2800 | 800 |
| 2000 ore a cell, after | 2900 | 4750 | 6400 | 5600 | 2392 |
| bay-limited, before | 3800 | 3600 | 3000 | 2600 | 1400 |
| bay-limited, after | 5180 | 7510 | 9050 | 9990 | 7310 |

(Bay-limited = 2000 ore a cell and a load mined in 10 ticks, so that the refineries and not the mining
limit the delivery. "Before" is the release binary of `origin/wp/7.4-money`, same scenarios.) Ticks of
unloading of the three bays in the 24-harvester bay-limited run: before only the middle one was ever used;
after `[1119, 1760, 1120]` of 4000 ticks.

**What the table says.** The collapse at the refineries is gone: all three bays work, and in the
bay-limited run the delivery rises with the number of harvesters from 3 to 24 (5180, 7510, 9050, 9990);
48 harvesters deliver 7310 against 1400 under the old rule. On the shipped numbers 48 deliver 2321
instead of 618, and 24 deliver 2805 instead of 2271.

**What it does not say.** With the shipped mining, the delivery of 24 and 48 harvesters is still lower
than that of 12 (3807). The cause is not the refineries. The near field has 28 cells and 3360 ore: 12
harvesters take nearly all of it (3807 delivered, 101 carried, 12 left of the 3920 in the field over
4000 ticks), so more harvesters cannot deliver more; and with 48 harvesters 22 to 28 stand in the field
(`ToOre`, stuck behind each other: bodies 1.5 cells wide on cells of 1) while each bay is busy at most 15
percent of the time (`[168, 608, 216]` ticks unloading of 4000). That crowding of one
small field is **not fixed here**. The chaotic traffic there also makes the numbers move by 10 to 30
percent with any change to the rules: a sweep of `queue_cost` 0 to 400, `queue_gap` 0 to 4 and
`switch_margin` 0 to 300 gave 48 shipped-ore harvesters between 1293 and 2625, and every setting with
`queue_cost` above 0 kept the bay-limited 24 harvesters between 8000 and 10,630 (`queue_cost: 0` gave
4850, `queue_gap: 0` 8000, both off, the old rule, 2600). The shipped weights were not tuned to a
peak.

**Pinned:** `many_harvesters_keep_delivering` (`tools/sf_headless/tests/harvest.rs`, 5 runs of 4000
ticks): shipped ore, 24 harvesters at least 2500 and 48 at least 2000 (measured 2805 and 2321); bay-limited,
24 harvesters deliver more than 6 and at least 8500, 48 at least 5500 (measured 9990 against 7510, and
7310).

### Mutations

Each was made in the source, run against the tests, and reverted:

| mutation | the tests that fail |
|---|---|
| a tie on cost goes to the last of the equals (`<=` for `<`) | `the_cheapest_refinery_is_chosen_and_a_tie_goes_to_the_nearer_then_the_lower_slot` |
| a tie on cost goes to the farther dock | the same |
| the queue term ignored (`committed x 0`) | `a_cost_is_the_cells_times_travel_cost_plus_the_committed_times_queue_cost`, `the_cheapest_refinery_is_chosen_...`, `harvesters_that_set_out_in_one_tick_see_each_others_choice`, `a_driving_harvester_switches_only_...` |
| hysteresis off (`cost >= here` for `cost + margin >= here`) | `a_driving_harvester_switches_only_when_the_other_refinery_is_cheaper_by_more_than_the_margin` |
| the queue spot on the approach (lateral 0) | `queue_spots_lie_in_rows_beside_the_approach`, `a_queued_harvester_takes_a_spot_on_its_own_side_...`, and in the world `queued_harvesters_wait_beside_the_approach_and_everyone_still_unloads` ("stood 0.0006 cells off the approach") |
| a choice not counted in its own pass | `harvesters_that_set_out_in_one_tick_see_each_others_choice` |
| only the queued count as committed | `harvesters_bound_for_a_dock_count_against_it_whatever_their_step` |
| `queue_gap` left out of the hash | `the_new_numbers_are_in_the_checksum_each_on_its_own` |
| the side by the parity of the place (crosses the approach) | `a_queued_harvester_takes_a_spot_on_its_own_side_and_the_next_row_after_those_ahead` |
| a granted harvester keeps walking to its spot | `a_queued_harvester_called_in_while_walking_to_its_spot_turns_to_the_dock_point` |
| `sf_bridge` maps `queue_gap` from `queue_start` | `sf_bridge` `each_field_maps_to_its_own_field` and `the_sim_numbers_equal_the_data_numbers` |
| the scenario maps `queue_cost` from `travel_cost` | `scenario::tests::each_harvest_field_maps_to_its_own_rule` |

The mapping tests give every field its own value (119 to 126 for the new ones) so that a swap or a field
fed from the wrong place fails. **Not covered by a test of its own:** `recheck_period` (that a driving
harvester looks again after that many ticks; the switch itself is tested through `recheck`).

### Cost

Release, alternating runs on one machine (load average 1.2 to 3 from other work, so relative):

* `harvest_cost` (48 harvesters, 3 refineries, 4000 ticks), three alternations of the binary of
  `origin/wp/7.4-money` and this branch: the time of the whole run **786, 732, 727 ms (base) and 816, 868,
  825 ms (this branch)**; the cost of the harvesting over the same units without harvest rules (the
  difference inside one binary, so the crowd's movement is in both) **157.6, 146.2, 143.5 against 153.6,
  162.0, 156.5 microseconds per tick**, i.e. about +8 microseconds a tick on 150, inside the 12 percent
  spread between runs of the same binary. The run is also not the same work: since the harvesters use
  three bays and queue beside the approach, they deliver about 3.8 times more ore in it (2321 against 618).
* `wander_5000` (no harvest rules, same checksum `0x38d5e3c43358a889`), eight alternating runs of the
  release binary: base 805, 798, 800, 779, 740, 798, 770, 791 ticks/s (mean 785), this branch 778, 769,
  755, 768, 734, 739, 744, 771 (mean 757): **3.5 percent slower, in all eight pairs**. That world's
  code path did not change (the harvest stage is one `Option::take`), so it is not the rules. Measured
  cause (cachegrind, reviewer of #92): +3.1 percent instructions, because the compiler stopped inlining
  `turning::steer` into `behavior::walk` once `harvest.rs` grew. Fix: `#[inline]` on `steer`, which gives
  the instruction count of main (cachegrind, `--ticks 400`: 2,584,654,955 against 2,584,650,596) and
  the same checksum. Eleven alternating release runs after the fix: main 983.6, this branch 969.5 ticks/s
  mean (1.4 percent, PR slower in 7 of 11 pairs), i.e. within the run-to-run spread.

### Not done

* The drive in the cost is the straight line: a refinery behind a rock patch looks nearer than it is.
  A route length would need the path service per harvester per choice, which is what this rule avoids.
* One small ore field cannot feed many harvesters (above); no cap on the harvesters that go to one
  field, no second field chosen when the first is crowded.
* Parked harvesters still stand partly in the approach (above).
* A queued harvester never leaves for another refinery (above, on purpose).
* No cost for the time a harvester needs to turn round at the dock, and no separate weight for a bay
  that is mid-unload (the harvester in the bay is one of the committed).
* `recheck_period` has no test of its own.
* Queue spots are not checked for passability (a spot can land on rock or a building; the harvester
  then stands where its walk stopped), and the map-edge clamp in `queue_point` can put a spot back
  on the approach. Nobody gets stuck: the grant always sends the harvester to the dock point.
