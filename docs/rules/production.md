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
* **What it costs**: `cost` credits over `build_ticks` ticks (`build_seconds` x 20).

## Queues

* One queue per factory structure, at most `queue_limit` items (shipped 5, 1 to 20), the one being
  built included. A sixth `QueueUnit` is refused (`QueueFull`).
* The front item is built; the others wait. When it comes out the next one starts at zero progress.
* **`QueueUnit { factory, kind }`** (tag 13 in the pending-command hash) adds to the end.
  **`CancelUnit { factory, index }`** (tag 14) takes the item at place `index` out (0 is the one being built).
  **`SetRally { factory, cell }`** (tag 15) sets or (with `None`) clears the rally point. All three are player
  commands: they are checked at their tick and a refusal is recorded (`World::production_rejections`,
  reasons in `ProductionError`), never a panic. They are **not** on the phase-14 setup-only refusal
  list, which holds only the commands that make the world (`Spawn`, `PlaceStructure` and the like).
* A factory that is under construction, or has no hit points left, accepts nothing and builds nothing
  (`NotStanding`, `a_factory_under_construction_builds_nothing`).

## Pay, wait, speed

* Progress is counted in thousandths of a tick. At full speed a tick adds 1000. The credits taken in a
  tick are `paid_at(k+1) - paid_at(k)` where `paid_at(done) = cost * done / (ticks * 1000)` (whole
  credits): the same exact rule as a structure (`C*(k+1)/T - C*k/T`, `construction.md`), so the
  ticks add up to exactly the price (`a_queued_unit_appears_after_its_build_time_and_the_price_is_paid_tick_by_tick`).
* The credits come through `World::pay`, which `docs/rules/economy.md` describes. With too few
  credits for the next tick nothing is taken and the progress does not move: the factory is
  `WaitingForMoney`. When several factories wait for the same credits the lower slot pays first
  (`the_lower_slot_is_paid_first_when_money_is_short`). A world **without** money rules builds for free.
* **The speed hook for WP6.4.** `World::production_speed_permille()` returns `FULL_SPEED_PERMILLE`
  (1000) and nothing else yet. Power will make it smaller; at 0 the status is `Stopped`. The hook is
  tested at full speed only (`the_speed_factor_is_a_hook_fixed_at_full_speed`).
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
* **The refinery's free harvester** (`comes_with`): when a refinery is *built* (it completes under
  `BuildStructure`) a harvester is put at the head of its queue at no price and no build time and comes
  out the same way. A refinery that is *placed* (`PlaceStructure`, the scenarios) brings none
  (`a_refinery_that_is_built_brings_a_harvester_out_and_one_that_is_placed_does_not`).

## Rally point

With a rally point set, a new unit gets `Order::Move` to the middle of that cell through the ordinary
orders, path and movement systems. Without one it stays on its
spot. The free harvester gets no rally order (it finds its ore by itself). The point must be on the map
(`OffMap`) and the sender's own factory. A rally point stays when the queue is empty.

## Checksum (section `Y`)

Written last, after `M`, **only in a world given production rules** (`World::with_production`), so every
earlier world hashes as before. It holds the rules (queue limit, exit radius), the table's digest, the
number of factory records and for each in slot order: id, rally point, progress, queue. Integers only,
slot order, no hash map. Status, rejections and the "produced in the last step" list are derived and
not hashed. Every number has its own test (`queues_progress_rally_and_the_tuning_numbers_are_in_the_checksum`).
The three commands have permanent numbers in the pending-command hash
(`the_commands_have_permanent_numbers_in_the_pending_queue_hash`).

## The picture (sandbox only; the sidebar is WP11.1)

All numbers in `production_look.ron`. A **progress bar** under the front edge of every factory with a
timed item (the construction bar's place and size; its fill turns to the `waiting` colour while it does not
advance: money, exit or prerequisite). A **readout** of the selected factory on the left edge: status,
the queue (the front item with its percentage and the credits paid), the rally cell and the unit the
queue key would add with its price and ticks. A **rally flag** on every rally cell, in its own colour for
the selected factory. Keys (`input.ron`; Q and E rotate the camera): **U** queues the unit on offer,
**I** picks the next unit on offer, **O** takes the last item out of the queue, **G** sets the rally
point to the cell under the cursor. Production is on in the sandbox with `--harvest` (it needs money).

## Pinned scenario `production`

`assets/scenarios/production.ron` (frozen `harvest_defs`; numbers written out): two factories building
in their own queues, the money gone after about 180 ticks and production waiting for the harvesters, a
rifleman on the exit cell walking to its rally point, a power plant placed on the free cells beside the
barracks (exit radius 1) so that the next rifleman waits inside until it is removed, and a barracks
destroyed with its queue. `tools/sf_headless/tests/production.rs` asserts each of these. Checksum in
`production.expected`, in the CI list.

## Not done, on purpose

Power and the speed factor other than 1 (WP6.4); repair and sell (WP6.6); defences (WP6.7); the AI's
use of production (phase 9); the sidebar with icons and the hotkeys of the finished game (WP11.1);
unit `requires` in the shipped data (balance, phase 15); a queue that survives a lost factory;
choosing which factory of several gets a unit; production of structures through a factory queue.
