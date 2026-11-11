# Production: factory queues, exit, rally point

Work package WP6.5 (PLAN.md phase 6). Built before WP6.4 (power) by the orchestrator's decision of
31-10-2026: milestone M1 needs units, not power. Rules in our own words; code in
`crates/sf_sim/src/production.rs`, data in `assets/config/production.ron` (`sf_data::production`),
`unit_defs.ron` (`cost`, `build_seconds`, `requires`) and `production_look.ron` (the picture).

## What a factory builds

* **Which factory** is derived from the unit's category, not written in each record (a second field
  could only disagree): infantry from an `InfantryFactory`, vehicles from a `VehicleFactory`, vessels
  from a `VesselFactory` (`sf_data::units::produced_by`, checked by
  `every_shipped_unit_is_made_by_the_factory_of_its_category`). Structures and objects are built by no
  factory (structures: `BuildStructure`, `construction.md`). A unit needs a price (`cost`) to be
  producible.
* **`requires`** on a unit is the same list as on a structure: ids of structures the owner must have
  **standing**. Validated on load (a unit that needs a unit or itself, a repeated id, a prerequisite
  nobody can build, and a unit whose factory function no structure of any faction of the unit has are
  refused with the record named). The shipped units list none yet: the tech tree for units is a balance
  matter of phase 15.
* **Who**: the faction of the sender must be in the unit's `factions` (`WrongFaction`), and the factory
  must be the sender's own (`NotYours`).
* **What it costs**: `cost` credits over `build_ticks` ticks (`build_seconds` x 20, then the build speed
  below).

## Build speed (WP6.11)

_Decision Tijmen 08-11-2026, after playing M1: everything builds 1.5 times faster, structures and units._

* **One factor**, `build_speed: (num: 3, den: 2)` in `assets/config/production.ron`: an exact fraction, both
  terms 1 to 100, the factor within 4 times faster or slower (`sf_data::production`, `sf_sim::BuildSpeed::is_valid`;
  a file without the line means 1 / 1). It lives in `production.ron` because that file already holds the numbers of
  how things are built (queue, exit) and reaches the world through the same call, `World::with_production`; it is
  the one number for units *and* structures.
* **Rounding rule.** A build of `B` ticks (from `build_seconds` x 20) takes `ceil(B * den / num)` ticks
  (`BuildSpeed::ticks`): never less than the factor allows, and never zero for a build that has a time. At 3 / 2:
  45 -> 30, 80 -> 54 (53.33), 14 -> 10 (9.33), 100 -> 67 (66.67), 1 -> 1, 0 -> 0
  (`ticks_are_the_base_times_den_over_num_rounded_up`). Shipped: rifleman 100 -> 67, harvester 600 -> 400, power plant
  300 -> 200, barracks 400 -> 267, refinery 800 -> 534, silo 240 -> 160 (`a_few_shipped_builds_in_ticks`).
* **Where it is applied: once.** `World::with_production(rules, table)` scales the build ticks of the production table
  *and* of the building table and takes their digests again; from then on every tick count in the sim (the progress
  total, the payment per tick, the scaffold of `BuildStructure`, `construction_of().total`, the view's `front_ticks`) is
  the scaled one and no other code knows the factor. A world **without** production rules in the headless scenarios (no `production` block) and the bench scenes keeps the
  data's own ticks; the headless `production` block takes `build_speed: (num, den)` and leaves it out as `(1, 1)`.
  **The plain sandbox of `sf_game`** (no `--harvest`) has building rules but no production rules, so since the 11-11-2026
  follow-up the game scales its building table itself, by the same `build_speed` of `production.ron` and the same
  `BuildSpeed::ticks` (`playfield::with_sandbox_buildings(world, defs, production)`, which hands `sf_bridge::building_table_at`
  the shipped speed): the power plant takes 200 ticks there, not 300. With `--harvest` (and in `--match`) the table keeps the
  data's ticks until `with_production` scales it once, so it is never scaled twice (`the_plain_sandbox_builds_at_the_shipped_speed_and_is_scaled_once`:
  plain 200 / 267, data 300, after `with_production` the same table digest as the plain one). The sim has no change for this.
* **The sidebar shows the same ticks.** `Catalog::build(defs, faction, look, speed)` scales the data's `build_seconds` by the
  shipped speed (`playfield::shipped_build_speed`), so an entry's `ticks` equal what the production and building tables count
  (`the_catalogue_times_are_the_scaled_ticks_the_world_counts`: all 22 entries, and 67 / 200 / 267 for the rifleman, power plant
  and barracks).
* **The money stays exact.** The price is not scaled; it is paid over the scaled ticks with the same rules as before
  (`C*(k+1)/T - C*k/T` for a structure, `paid_at` for a unit). The sum over the ticks is the price for every
  price, base time and factor tried (`the_money_adds_up_for_every_price_and_every_factor`: 8 factors x 8 prices x
  8 times) and for every shipped unit and structure (`every_shipped_price_is_paid_exactly_in_the_scaled_ticks`);
  a unit of 100 base ticks and 333 credits costs 333 in 67 ticks, a plant of 45 base ticks and 301 credits 301 in 30
  (`units_are_built_in_the_scaled_ticks_and_cost_exactly_their_price`, `structures_are_built_in_the_scaled_ticks_and_cost_exactly_their_price`).
* **Power on top.** The shortage speed (below) multiplies the scaled progress: a rifleman of 14 base ticks is 10 ticks
  at 3 / 2 and 27 ticks in a shortage of 384 thousandths (`a_shortage_slows_the_build_speed_on_top_of_it`).
* **1 / 1 is the old game.** At 1 / 1 the tables are the data's own (same digests), and every pin of every earlier
  phase came back unchanged when the shipped file said 1 / 1 (workspace test run, below in the checksum section).

## Queues

* One queue per factory structure, at most `queue_limit` items (shipped 5, 1 to 20), the one being
  built included. A sixth `QueueUnit` is refused (`QueueFull`).
* The front item is built; the others wait. When it comes out the next one starts at zero progress.
* **`QueueUnit { factory, kind }`** (tag 13 in the pending-command hash) adds to the end.
  **`CancelUnit { factory, index }`** (tag 14) takes the item at place `index` out (0 is the one being built).
  **`SetRally { factory, cell }`** (tag 15) sets or (with `None`) clears the rally point.
  **`SetRepeat { factory, on }`** (tag 18, WP6.10) switches the repeat of the factory (see "Repeat" below). All four are player
  commands: they are checked at their tick and a refusal is recorded (`World::production_rejections`,
  reasons in `ProductionError`), never a panic. They are **not** on the phase-14 setup-only refusal
  list, which holds only the commands that make the world (`Spawn`, `PlaceStructure` and the like).
* A factory that is under construction, or has no hit points left, accepts nothing and builds nothing
  (`NotStanding`, `a_factory_under_construction_builds_nothing`).

## Repeat (WP6.10)

*Decision Tijmen 10-11-2026 ("herhaalknop"); the details are the orchestrator's, under his rule that a proposal holds if he does not answer.*

* **`SetRepeat { factory, on }`** (tag 18) switches a per-factory flag, **off by default**. With it on, a unit that has come out of the
  factory goes back at the **end of the same queue at zero progress**, in the same tick: the finished item has left, so the queue is as
  long as before and `queue_limit` (5) is never passed (`a_full_queue_on_repeat_never_passes_the_limit_and_keeps_its_mix`: five items
  stay five over 300 ticks; a mix of two kinds comes round again in order). The copy goes through the checks of a new `QueueUnit` that
  can change while it is built: the unit is still in the table, built by this kind of factory, open to the owner's faction, every
  structure it `requires` stands (`World::repeatable`; `a_repeat_does_not_add_a_copy_whose_requirement_no_longer_stands`). The queue-full
  check is kept as well, but it cannot be the one that stops a copy, since the item has left first; it is a guard, not a tested rule.
  When a check fails nothing is added and the flag stays on.
* **Money: no new rule.** A repeated item pays per tick like any other and waits in `WaitingForMoney` when the credits run out. Repeat does
  **not** switch itself off without money and never runs into debt (`a_repeating_factory_waits_for_money_and_resumes_when_it_is_back`:
  waiting with the flag still on, resuming after a cancelled tank gave its credits back).
* **Not repeated:** the free unit of a structure (the refinery's harvester; `item.free`; test `the_free_harvester_is_never_repeated`, which
  uses a table where a vehicle factory brings a free unit so that the rule can be reached) and a **cancelled** item (`CancelUnit` takes
  it out; `a_cancelled_item_does_not_come_back`). A refinery has no unit tab and is no factory, so `SetRepeat` on it is refused.
* **Turning it off** lets the queue run out: nothing is removed (`turning_repeat_off_lets_the_queue_run_out`). An empty queue on repeat
  does nothing until something is queued. A factory that is gone loses its queue and its flag with it.
* **Refusals** are those of `SetRally`: not the sender's (`NotYours`), not a factory (`NotAFactory`), a stale id (`NoSuchFactory`), under
  construction or without hit points (`NotStanding`). No new refusal kind. A refusal counts as ignored and makes no record
  (`a_repeat_is_refused_for_what_is_not_the_senders_standing_factory`).
* **Checksum.** The flag is in the factory section next to the rally point, as part of its tag byte: 0 or 1 as before (rally point none or
  set), **plus 2 while the factory repeats**. So a world where nothing repeats hashes exactly as before WP6.10 (no pin moved, see
  `progress.md`), and the flag is hashed when it is on (`the_repeat_flag_is_part_of_the_checksum_and_a_world_that_never_repeats_hashes_as_before`).
  `SetRepeat { on: false }` for a factory that has no record changes nothing and makes none, so switching off a switch that was never on
  is invisible. Tag 18 of the pending-command hash is pinned as literal bytes (`set_repeat_has_the_permanent_tag_18_in_the_pending_hash`).
* **Scenario** `production_repeat` (`tools/sf_headless/tests/production_repeat.rs`): one barracks on repeat from tick 1 to tick 1500
  with 150 credits and two harvesters; the queue stays at two, the factory waits for money and goes on, and after the switch-off the
  two queued riflemen come out and nothing more.
* **Not done:** a key for it (a mouse button only; see `sidebar.md`), repeat of structures (they are not built from a factory queue), a
  count of loops, repeat across the loss of a factory.

## Pay, wait, speed

* Progress is counted in thousandths of a tick. At full speed a tick adds 1000. The credits taken in a
  tick are `paid_at(k+1) - paid_at(k)` where `paid_at(done) = cost * done / (ticks * 1000)` (whole
  credits): the same exact rule as a structure (`C*(k+1)/T - C*k/T`, `construction.md`), so the
  ticks add up to exactly the price (`a_queued_unit_appears_after_its_build_time_and_the_price_is_paid_tick_by_tick`).
* The credits come through `World::pay`, which `docs/rules/economy.md` describes. With too few
  credits for the next tick nothing is taken and the progress does not move: the factory is
  `WaitingForMoney`. When several factories wait for the same credits the lower slot pays first
  (`the_lower_slot_is_paid_first_when_money_is_short`). **Structures first, then factories, each in
  slot order**: structures under construction pay in `advance_construction`, before the commands of the
  tick, and the factories pay in `advance_production`, after them. So with money for one payment a
  structure under construction is paid before a factory, even a factory in a lower slot
  (`a_structure_under_construction_pays_before_a_factory_in_a_lower_slot`). A world **without** money rules builds for free.
* **The speed (power, WP6.4).** `World::production_speed_permille(player)` is the speed power gives
  (`power.md`): `FULL_SPEED_PERMILLE` (1000) in a world without power rules and for a player with enough power,
  `made * 1000 / used` (not below the floor of `power.ron`) in a shortage; at 0 the status is `Stopped`, nothing is paid
  and no progress is lost. A unit of 14 build ticks at 384 takes 37 ticks and its 70 credits are paid exactly
  (`a_shortage_slows_a_factory_and_the_price_is_still_paid_exactly`, `tests/power.rs`; the progress per tick and the status
  in `the_progress_and_the_status_follow_the_speed_and_a_rifleman_costs_nothing_more_to_wait`). Without power rules the speed
  is full and the price adds up at any speed (`without_power_rules_the_speed_is_full_and_the_price_adds_up_at_any_speed`).
* **Cancelling gives back exactly what was paid** for that item (`paid_at(done)`), into the store, even
  above its capacity (`cancelling_gives_back_exactly_what_was_paid`). A waiting item has paid nothing.
  The free harvester of a refinery cannot be cancelled (`NothingToCancel`).
* **A destroyed factory loses its queue and nothing is refunded**, as with a structure destroyed half
  built (`economy.md`, "Money"). The record is dropped at the start of the next production tick
  (`a_destroyed_factory_loses_its_queue_and_nothing_is_refunded`).
* If a structure the front unit needs no longer stands, the factory waits with
  `MissingPrerequisite(kind)` and pays nothing; it goes on when the structure stands again.

## Exit and the free spot

* A finished unit appears on the **exit cell** of its factory (`exit_cell`, `buildings.md`: the cell
  next to the middle of the `exit_side` of the footprint) if that is free, else on the first free cell of
  a deterministic search: rings of growing chessboard distance from the exit cell (ring 0 is the
  exit cell itself), each ring row by row from the north-west, out to `exit_radius` (shipped 4, 1 to 8).
* A cell is free when it is on the map, no structure stands on it, a unit of that movement class can
  enter it, and no unit stands on it or is walking to it (`a_unit_standing_on_the_spot_is_not_built_on_top_of`).
* If no cell in reach is free the finished unit **waits inside**: status `WaitingForExit`, the queue
  holds, nothing more is paid and nothing is lost; it comes out on the tick a spot is free
  (`a_blocked_exit_holds_the_queue_and_loses_nothing`).
* **Vessels need water in reach.** The same search applies to a vessel factory: a cell is free for a
  vessel only when a vessel can enter it (water). A shipyard with no water within `exit_radius` of its
  exit cell (it can stand on land, `buildings.md`) still takes the vessel and is paid in full; the
  finished vessel then waits in `WaitingForExit` until it is cancelled (refund as above) and nothing is
  lost (`a_vessel_comes_out_on_water_and_without_water_in_reach_it_waits`).
* **The refinery's free harvester** (`comes_with`): when a refinery is *built* (it completes under
  `BuildStructure`) a harvester is put at the back of its queue at no price and no build time and comes
  out the same way. A refinery that is *placed* (`PlaceStructure`, the scenarios) brings none
  (`a_refinery_that_is_built_brings_a_harvester_out_and_one_that_is_placed_does_not`).

## Rally point

With a rally point set, a new unit gets `Order::Move` to the middle of that cell through the ordinary
orders, path and movement systems. Without one it stays on its
spot. The free harvester gets no rally order (it finds its ore by itself). The point must be on the map
(`OffMap`) and the sender's own factory. A rally point stays when the queue is empty.

## Checksum (section `Y`)

**WP6.11:** after `queue_limit` and `exit_radius`, the build speed in lowest terms (two `u32`, 3 and 2) is written
**only when it is not 1 / 1**, so a world at the data's own build times hashes as before. The scaled tick counts are
in the digests of the two tables. Two factors that scale every build of a world alike still hash differently
(`the_factor_is_in_the_checksum`: 3 / 2 against 2 / 1 on a world of one-tick builds; 6 / 4 against 3 / 2 equal).

Written last, after `M`, **only in a world given production rules** (`World::with_production`), so every
earlier world hashes as before. It holds the rules (queue limit, exit radius), the table's digest, the
number of factory records and for each in slot order: id, rally point, progress, queue. Integers only,
slot order, no hash map. Status, rejections and the "produced in the last step" list are derived and
not hashed. Every number has its own test (`queues_progress_rally_and_the_tuning_numbers_are_in_the_checksum`).
The commands have permanent numbers in the pending-command hash
(`the_commands_have_permanent_numbers_in_the_pending_queue_hash`). WP6.10: the repeat flag is the +2 on the rally tag, see "Repeat".

## The picture (the sandbox's readout and bars; the sidebar is WP11.1, `sidebar.md`)

The player's way to queue units is the **sidebar** (WP11.1, `sidebar.md`): a click on a unit item sends
`QueueUnit`, a right-click `CancelUnit`, with the progress, the queue count and the waiting words on the item. What
follows is the sandbox's own readout and debug keys, which stay (the keys as debug keys). All numbers in
`production_look.ron`. A **progress bar** under the front edge of every factory with a
timed item (the construction bar's place and size; its fill turns to the `waiting` colour while it does not
advance: money, exit or prerequisite). A **readout** of the selected factory on the left edge: status,
the queue (the front item with its percentage and the credits paid), the rally cell and the unit the
queue key would add with its price and ticks. A **rally flag** on every rally cell, in its own colour for
the selected factory. Keys (`input.ron`; Q and E rotate the camera): **U** queues the unit on offer,
**I** picks the next unit on offer, **O** takes the last item out of the queue, **G** sets the rally
point to the cell under the cursor. Production is on in the sandbox with `--harvest` (it needs money).

## Rally by right-click (WP6.9)

UI only: the sim command is the `SetRally` of WP6.5 (tag 15), nothing in `sf_sim` changed and no pin moved. Code:
`crates/sf_game/src/rally.rs` (the decision), `commands::rally_clicks`, `runner.rs` (`handle_orders`, `draw_production`),
`production_view::draw_rally_lines`. Tests: `rally/tests.rs`, `tests/production_screenshot.rs`.

* **Whom it is for.** The selected **own factories that stand** (`rally::selected_factories`: a structure with a
  production queue of the player, finished, not being sold, with hit points left). A factory under construction, being
  sold or at 0 hit points is not one; the right-click on it is ground (`a_factory_being_sold_takes_no_rally_point`).
* **Factories only selected.** A right-click on the ground sends `SetRally { factory, cell: Some(cell) }` for each of
  them (in the selection's order, one command each); a right-click on **one of the selected factories** sends
  `SetRally { factory, cell: None }` for **that** factory only, the other selected ones keep theirs
  (`a_right_click_on_a_selected_factory_clears_its_rally_point_and_only_its`). Over an enemy or a neutral thing nothing
  is sent (the cursor is Inspect); off the map nothing is sent (NoGo; the sim would refuse it: `OffMap`); with no ground
  point (the sky, the UI) nothing is sent. A cell in shroud or fog is allowed, like a move order (the code never reads
  visibility). A cell no unit can enter (the river) is allowed too: the sim's only check is the map
  (`a_cell_nobody_can_walk_to_is_still_a_rally_cell`); a rally cell that a structure stands on is also accepted (the
  walking unit goes as near as it can).
* **Mixed selection** (units and factories). The units get the order of their cursor, exactly as without the factories
  (`commands::right_click`), and the factories follow the same cursor kind (`rally::plan`): Move, Queue and AttackMove over
  ground set the rally point of each selected factory to the same cell (the rally point is never queued); Attack (an
  enemy) sets nothing; NoGo sets nothing, so the picture stays true over water the units cannot enter; Guard over a
  selected factory leaves its rally point alone (the units guard it; the cursor says Guard, nothing about the rally)
  and over anything else sets the rally cells to that cell. A rally point is cleared only with factories alone
  selected, under the RallyClear cursor (fix round of PR #116, 11-11-2026).
* **Voices.** A rally right-click raises no unit voice (`order_voice` knows only `Command::Order`); a mixed selection
  speaks the units' order voice as for a plain order.
* **The line.** From the middle of the factory's exit cell (`rally::exit_cell_centre`: `sf_sim::exit_cell` of the anchor,
  the footprint and the exit side of the building table, which `ProductionMenu` now carries; the test
  `the_exit_cell_of_the_snapshot_is_the_one_the_sim_gives` compares it with the sim's own for every factory) to the foot of
  the flag, drawn **only while that factory is selected** and only when it has a rally point, as one straight screen line
  under the flags (`draw_rally_lines`; colour and width are `rally.line` and `rally.line_width_px` of
  `production_look.ron`, pinned in `the_shipped_look_is_pinned`). It is not bent along the terrain. The flag stays drawn
  for every factory; **own flags are always shown** (`SimSnapshot::seen` is true for the player's own objects, fog or not).
* **The debug key G** still sets the rally point of the first selected factory to the cell under the mouse.

## Pinned scenario `production`

`assets/scenarios/production.ron` (frozen `harvest_defs`; numbers written out): two factories building
in their own queues, the money gone after about 180 ticks and production waiting for the harvesters, a
rifleman on the exit cell walking to its rally point, a power plant placed on the free cells beside the
barracks (exit radius 1) so that the next rifleman waits inside until it is removed, and a barracks
destroyed with its queue. `tools/sf_headless/tests/production.rs` asserts each of these. Checksum in
`production.expected`, in the CI list.

## Not done, on purpose

Defences (WP6.7, done); repair and sell (WP6.6, done: a factory being sold accepts nothing and its queue is lost with it, nothing refunded, `repair_sell.md`); the AI's
use of production (phase 9); the hotkeys of the finished game (the sidebar itself is WP11.1);
unit `requires` in the shipped data (balance, phase 15); a queue that survives a lost factory;
choosing which factory of several gets a unit; production of structures through a factory queue; a rally point on
ore or on a unit, waypoints, a rally line bent along the terrain; the repeat toggle (WP6.10, `PLAN.md`).
