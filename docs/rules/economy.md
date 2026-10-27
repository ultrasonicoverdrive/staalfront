# Economy: ore on the map and harvesters

Work packages WP7.1 and WP7.2 (PLAN.md phase 7). This page holds the rules of the **ore on the
map** (WP7.1): how much a cell holds, how it is taken, how it grows back, how a harvester finds
it; and of the **harvesters** (WP7.2, from "Harvesters" below): the loop of search, mine, drive
back, check in at the refinery by messages, unload. Storage (WP7.3) and money (WP7.4) are later
work packages and will add their rules to this page. Rules in our own words.

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
| `ToRefinery` | Drives to the dock point of the nearest standing refinery; at `call_range` it sends its request. | Granted: `Granted`. Queued: `Queued`. |
| `Queued` | Stands where it is and waits. | The dock grants it the bay. |
| `Granted` | Drives into the bay. | Within 2 cells of the dock point: sends `Arrived`, `Unloading`. |
| `Unloading` | Sits in the bay while the dock takes the load off in bites. | `Released`: back to the search, in the same turn. |
| `Parked` | Stands 4 cells out from its refinery along the exit side (spread sideways by `slot % 5`, so that they do not stand on one spot), and looks again every `retry_period`. | The next search. |
| `NoRefinery` | Holds its load where it stands, and looks for a standing refinery every `retry_period`. | One stands. |
| `Manual` | Obeys a player order that was not a field. | The order ends: back to the search. |

**The dock point** is the middle of the exit cell of the refinery (`buildings.md`, "The exit":
for `PosX` the cell next to the middle of the east side). The harvester drives to it and unloads
there.

**Choosing a refinery:** the nearest **standing** refinery of the harvester's own player: the
smallest squared straight-line distance from the harvester to the dock point, a tie to the lower
slot. "Standing" is `World::is_standing` (the rule `placement.rs` and the tech tree use): hit
points left, or a structure that cannot be damaged. **WP6.3 (#76) extends that function to
"complete", so a refinery under construction stops being a dock the day it merges; nothing here
builds one, so that half is untested here** (the zero-hit-point half is:
`harvest::tests::a_refinery_at_zero_hit_points_is_not_a_refinery`). A refinery of another player
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
| `Queued(place)` | The bay is taken. | Stops and waits. |
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
* **Several refineries:** the nearest standing one (`the_nearest_standing_refinery_wins`; the
  zero-hit-point case above).
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
a player so far. It is the **placeholder** for the economy: WP7.3 (storage and silos) will cap it
by the storage a player has, and WP7.4 (money) will turn it into credits with the price of a unit
of ore and replace it (or keep it as a statistic). Until then it is a plain counter, kept per
player in a sorted vector, hashed in the checksum, and nothing spends it.

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
| 4 | 105 | 7 | 105 | 2216 |
| 50 | 205 | 10 | 214 | 3168 |

**The 50-harvester row measures a jammed system, not a harvesting load:** with that many harvesters
every one picks the same nearest refinery and most stand queued or blocked in its approach (see "Not
done"), so it moves less ore per harvester than a working economy would and the cells change slower.
The upload stays sound anyway: even one upload every tick would be about 20 x 0.22 ms = 4.4 ms a second
on llvmpipe.

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
  of 48 harvesters moving, routing and pushing each other**, not of the harvest stage. **These 48
  harvesters are a jammed system** (one refinery gets them all, see "Not done"), so this measures a
  crowd queueing at one bay, not 48 harvesters at work;
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

* No harvester is made: the refinery's `comes_with: "harvester"` flag is data nothing reads yet
  (production is WP6.5). Storage and credits are WP7.3 and WP7.4.
* A harvester is not ordered **onto a refinery** by the player to unload there, nor held on a
  field; `Stop` cannot park one.
* A refinery is chosen by straight-line distance to its dock point, not by the length of the
  route, and not by how busy it is: a harvester queues at the nearest standing one even if another
  has no queue. **With many harvesters this collapses the throughput**: all of them go to one bay
  and the queue stands in its approach. Measured by the reviewer of #78 with `crowd_scenario` (three
  refineries, 4000 ticks, the first n harvesters): 3 deliver 2400, 6 deliver 3200, 12 deliver 3200,
  24 deliver 2271, 48 deliver 618 (13 `Queued`, 26 `ToOre`, only one bay ever granted). Not fixed
  in WP7.2; the fix is a choice by route length and queue length.
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
