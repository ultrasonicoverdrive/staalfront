# Orders

Work package WP3.4 (PLAN.md phase 3). Code: `crates/sf_sim/src/orders.rs`,
`command.rs` (the commands and the queue), `world.rs` (`apply`), `behavior.rs` (the
tick). Scenario: `assets/scenarios/orders.ron`. Idea: `docs/red-alert-analysis.md`
section 5 (an order is stored state that the unit carries out on its own; receiving
it is not completing it) and section 6 (the destination is not the route).

A **command** is what a player asks (`Command::Order`, `Command::Stop`); it arrives in
the command queue and runs at the start of tick N. An **order** is what a unit has: a
current order and a queue of the orders behind it, stored in the unit's own columns,
hashed in the checksum and independent of the click that made them.

## The orders

| Order | The unit... | Ends when |
|---|---|---|
| `Idle` | has no order: it is what a unit has when nothing was ordered, and `Stop` puts it there. Phase 5 lets an idle unit react (return fire, auto-acquire). | never by itself; any other order replaces it |
| `HoldPosition` | stands still **on purpose** and never moves, not to chase and not to get closer. Phase 5 may fire from it at what is in range, but it never moves by itself. | never by itself; a queued order behind it waits (an idle unit would start it at once) until the hold is replaced or stopped |
| `Move(point)` | goes to the point | it is within `ARRIVE_TOLERANCE` (an eighth of a cell, half a metre) of the point |
| `Attack(entity)` | goes toward the entity until it is within the reach of its first weapon, then stands and turns to face it | the target is gone: dead, despawned, or an id whose slot now holds another entity (generation check) |
| `AttackMove(point)` | goes to the point; on the way it would engage what it meets (phase 5) | it is within `ARRIVE_TOLERANCE` of the point |
| `Guard(point)` | goes to the point and stays; walks back if it is ever moved off | never by itself: a queued order behind it waits until the guard is replaced or stopped |
| `Guard(entity)` | follows the entity and stays near it | the entity is gone |
| `Scatter` | takes a short step in a deterministic direction | it is a `Move` as soon as it starts (below) |
| `Stop` (a command) | clears the current order and the queue; the unit stands where it is, idle (a wandering placeholder stops wandering too) | not an order: the unit is `Idle` afterwards |

A point is in cells (`Fx::ONE` = 4 m, B12) and is clamped into the map when the command
runs. In a scenario file and in the game a point is the middle of a cell.

## What can take an order

A command names units; each unit takes it or is skipped, and the command counts as
ignored (`StepReport::commands_ignored`) only when no unit took it. A unit is skipped
when it:

* is a structure or an object, or a stale id: **deterministically refused**, nothing
  changes;
* has no speed (cannot happen for a mobile unit of the unit table; checked anyway);
* is ordered to `Attack` and has no weapon (`UnitStats::weapon_range` is zero), or the
  target is itself or gone; an unarmed unit may still `AttackMove` (it simply walks) and
  `Guard`;
* is ordered to `Guard` itself or a gone entity;
* is given `Order::Idle`: that is not an order to give, use `Stop`;
* is given a queued order while its queue is full.

Ownership is **not** checked here ("whether a player may command an entity is a game
rule", `command.rs`); the game only ever commands the local player's own units, and the
lockstep package (WP14.1) is where a peer's commands get validated.

Legacy wanderers (`Command::Spawn`) have no unit table entry and no weapon: they can be
ordered to move, attack-move, guard, scatter and **hold position**, at the world's own
speed, and cannot attack. (`HoldPosition` is not for table units only: `accepts_order`
takes it for every mobile entity that has a step.)

## The queue

Every unit has a ring of `ORDER_QUEUE_CAP` (8) orders in its own storage: no allocation,
no growth. A constant, not data, because it sizes the storage of every slot.

* `queued: false` (a plain right-click) **replaces** the current order and empties the
  queue.
* `queued: true` (shift) **appends**. A unit with no order starts on it at once. A
  full queue refuses the append for that unit and nothing else changes.
* When the current order ends, the next one **starts on the following tick** (the tick
  of the arrival is spent arriving); with an empty queue the unit goes idle.
* `Stop`, and any non-queued order, empty the queue. The legacy `MoveTo` and
  `SetWanderArea` do too (they replace the unit's behaviour).

## Destination and route

The order holds the **destination**: where the unit wants to be (a point, or the live
position of an entity). The **route** is how it gets there and is not part of the order: it is a
record of the unit of its own (`Entities::route`, a fixed-size ring of waypoints and the portals of
the way still to refine; `docs/rules/pathfinding.md`), reset whenever the order changes or ends. The
unit keeps the point it steers at, the next **waypoint**, in its `target` column
(`World::waypoint_of`), and `Pathing::steer` (`crates/sf_sim/src/path.rs`) is the only function that
decides it; `orders::go_to` calls it once a tick. Arrival is judged against the destination, never
against the waypoint. Nothing else reads `target`: the guard's walk-or-wait switch has a column of
its own (`guard_walking`), so the route could take `target` over.

* **Since WP4.2** a unit of a movement class that terrain restricts (everything but aircraft) that
  walks an order asks for a route on the first tick of the walk and **stands still until it has one**
  (one tick at least: the request is served at the start of the next). The route goes round what
  the class cannot cross and the structures, through the waypoints of the ring; while there is a
  waypoint `waypoint_of` is it, when the ring is empty and the route complete the destination
  itself. Aircraft, the legacy `MoveTo` and the wanderers steer straight at the destination, and
  `waypoint_of` is the destination, as before. A destination that moves (an attack, a guard of an
  entity) by more than 3 cells from the cell the route was asked for makes the unit ask again.
* **A destination that cannot be reached** (blocked ground, or cut off) is not an error and does not
  end the order: the route leads to the reachable cell nearest to it, the unit stands in the middle of
  it, and `World::route_of(id)` says `unreachable` and `Stranded` until the order is replaced. The
  order, the queue behind it and the arrival test stay as they are, as they did when the unit stood
  at the edge of a river at WP4.1; ending such an order ("gives up", "stands as near as it gets")
  is WP4.4's outcome and WP4.7's replan.
* **WP4.3** adds the flowfield step for groups to the same seam: when one `Move` or `AttackMove` to a
  point sends at least `flowfield_threshold` units of a class somewhere (8 as shipped), those units
  do not ask for routes; they are **members of one flowfield** (`docs/rules/pathfinding.md`,
  "Flowfields for groups"), `Pathing::steer` returns their waypoint from it, and they wait where
  they stand until the field has integrated their cell. A member has its own **arrival slot** around the
  destination, and **arrival is judged against the slot** (`go_to`), not against the order's point, so
  that 200 units do not pile up on one spot; the order still ends as for any move. A member whose
  field is gone, or cannot serve it, is routed on its own. A guard of a point is not a group order (it walks
  back to its exact post). **WP4.7** recomputes the route when it is blocked or the target has vanished, the
  order staying.
* **Terrain (WP4.1).** `walk` returns whether the unit arrived, is on its way, or was blocked by
  terrain; `go_to` counts only an arrival, and a blocked step on a route makes the unit ask for a
  cell-by-cell route (a second refusal leaves it standing, order kept). The speed of a step is
  the unit's speed per tick times the multiplier of the cell it stands in.
* **WP4.4** adds collisions.
* **WP4.5** makes the turn-before-drive and the separate turret part of the same step
  (`walk`); the present rule is: turn at the unit's turn rate and drive only when the
  facing is within `ALIGN_TOLERANCE` (16 of 256 steps) of the wanted direction.

Steering is the same step as the legacy `Wander` and `MoveTo` (`behavior::walk`): at
the unit's own `speed_per_tick` and turn rate from the unit table. A unit within one step
of its waypoint snaps onto it.

## Attack, attack-move and guard: what exists and what phase 5 fills in

There is **no combat** in this work package: nothing fires and no hit point changes.
What exists is the movement part and the state:

* `Attack`: the unit walks until its distance to the target's middle is at most
  *weapon range + the target's radius* and then stops and turns toward it. The target's
  radius is half its `size` for a unit and half the **shorter** side of its footprint for a
  structure, so a unit is never counted as in reach when it is not. It follows a target
  that moves (the destination is re-read every tick; in slot order, so a target in a lower
  slot has already moved this tick) and resumes when the target steps out of reach.
  The range is `UnitStats::weapon_range`, the range of the **first** weapon, copied by
  `sf_bridge` from `weapon_defs.ron`.
* `Guard(entity)` stops within `GUARD_CLOSE` (one and a half cells) of the entity's edge and
  starts walking again beyond `GUARD_LEASH` (two and a half), so a guard does not shuffle
  at the boundary.
* `AttackMove` and both guards call `orders::would_acquire_targets_here` once per tick.
  It does nothing.

**Phase 5 must fill in:**

1. `would_acquire_targets_here`: look for an enemy in sight (WP5.4 target selection and
   priorities), and for an attack-move or a guard turn the unit toward it, keeping the
   interrupted order (for example by putting it at the front of the queue). A guard
   returns to its post afterwards (the point guard already walks back).
2. Idle units are skipped by the behaviour pass (`advance_all`), so the automatic return
   fire of an idle unit needs a pass of its own.
3. Firing: reload timers, the minimum range of the howitzer (`min_range` is in the
   weapon data, not in the table yet), more than the first weapon, damage. The order
   `Attack` then also needs an end: the target dying is already one; leaving sight is
   WP8's.
4. `UnitStats::weapon_range` becomes the weapon list when WP5.1 puts the weapons into the
   sim; the table's digest then covers them (today they are hashed with the orders, below).

## Scatter

`Scatter` moves each unit a short step in a deterministic direction, to spread a bunched
group. The step is `scatter_offset(unit id, tick)`: a pure function of the entity id
(slot and generation) and the tick the scatter **starts** on: one of 256 directions and a
length between `SCATTER_MIN` (1 cell) and `SCATTER_MAX` (3 cells). It uses no random
stream, so scattering never changes what a unit's own generator draws next. The spot is
kept inside the map.

A scatter is resolved when it starts, not when it is given: given to an idle unit it
starts at once; queued behind other orders it waits in the queue as `Scatter` and picks
its spot, from where the unit stands then, on the tick it comes up. When it starts the
current order becomes the `Move` to the spot, so a unit never has `Scatter` as its current
order (`World::order_of` reads `Move`). A spot that the map edge clamps onto the unit's own
position is reached at once.

## Commands and their order

Commands are scheduled for a tick and run by **(tick, player, player sequence)**
(PLAN.md follow-up 4, closed in this work package). The sequence number is the player's
own counter:

* `World::schedule(tick, player, command)` numbers the command with the next number of
  that player (each player counts from zero). Nothing changes for the callers of before:
  a player's commands run in the order they were scheduled. A counter that has no number
  left is refused (`ScheduleError::SequenceExhausted`), never reused.
* `World::schedule_sequenced(tick, player, sequence, command)` takes the number the
  sender gave the command. Two machines that receive the same commands in different
  orders run them in the same order, because the order is the numbers', not the arrival's.
  A number already used by that player for that tick is refused
  (`ScheduleError::DuplicateSequence`, the first stays; a key is never overwritten), and
  so is `u64::MAX`, which is reserved (`ScheduleError::ReservedSequence`). An unnumbered
  command scheduled afterwards gets a number past every one the player brought along.
* **Gaps are allowed.** Numbers need not be consecutive: commands with the numbers 2, 7
  and 900 run in that order and nothing waits for the missing ones. Checking that no
  number is missing is the network layer's job (WP14.1), not the queue's.

Queues compare equal by commands in execution order; the numbers are not hashed (the order
is). `crates/sf_sim/tests/orders.rs` (`two_machines_agree_whatever_order_the_commands_arrive_in`)
runs one script in three arrival orders and compares the checksum histories.

## The checksum

A world that never received an order hashes exactly as before: `wander_5000`,
`command_mix`, `terrain_256`, `units_spawn` and the sf_sim goldens did not change.
`Behavior::Ordered` is tag 3 and `HoldPosition` is order tag 7. The orders add one section `D` after the others, written
only while some live entity has an order or a queue: the **weapon reach digest** of the
unit table (`UnitTable::reach_digest`; kept out of `UnitTable::digest` so the pinned
`units_spawn` value stands), the number of such entities, and for each its slot, current
order, the guard's walk switch and queue. Pending `Order` and `Stop` commands are hashed with the other pending
commands (tags 8 and 9).

## The game (`sf_game`)

`crates/sf_game/src/commands.rs`. The UI only builds commands and calls `World::schedule`
for the next tick; it never changes the world.

Every key and button is an action in `assets/config/input.ron`, so each is rebindable
there, and the runner asks the action, never a key.

| Action | Default | Command |
|---|---|---|
| `OrderMove` | right-click | the order the cursor shows (WP3.5, `docs/rules/cursor.md`): `Order::Move` on the ground, `Attack` on an enemy, `Guard` on one of your own, `AttackMove` with the modifier |
| `OrderQueue` | shift, held | queues the order, scatter or hold instead of replacing the orders |
| `OrderAttackMove` | F, held | a right-click on the ground attack-moves (WP3.5) |
| `OrderStop` | C | `Stop` |
| `OrderScatter` | X | `Order::Scatter` |
| `OrderHold` | H | `Order::HoldPosition` |

The camera keeps WASD, the arrow keys and the screen edge, so stop is not on S (a test of
the engine checks that no key or button is bound to two actions, except the shift of
`OrderQueue` and `SelectAdd`). The orders go to the selected own mobile units
(`Selection::orderable`, `docs/rules/selection.md`; the temporary F2 select-all is gone). The top bar shows how
many units are selected; the F1 panel shows the order of the hovered unit. Right-click picks what
the cursor shows (WP3.5): `cursor_context` decides the cursor and the order from the same inputs,
so a click on an enemy is an `Attack`, on one of your own a `Guard`, on ground a `Move`
(`docs/rules/cursor.md`). Ground that no selected unit can enter gives no order.

## Terrain

A unit made from the unit table moves at its speed times the cost of the cell it stands in and
does not enter a cell its movement class cannot cross; the order, the queue and the arrival test
are unchanged (`docs/rules/movement.md`), and the route round what it cannot cross is
`docs/rules/pathfinding.md`. The `orders` scenario has no terrain layer (every cell is open
ground, and no unit is routed), so its checksum did not change at WP4.1 or WP4.2; its frozen
definitions carry the costs as shipped at WP4.1.

## Tuning constants

`ORDER_QUEUE_CAP` 8, `ARRIVE_TOLERANCE` 1/8 cell, `GUARD_CLOSE` 1.5 cells, `GUARD_LEASH`
2.5 cells, `SCATTER_MIN` 1 cell, `SCATTER_MAX` 3 cells: placeholders for the balance
rounds (phase 15), in `crates/sf_sim/src/orders.rs`.

## Where it is tested

* `crates/sf_sim/src/orders/tests.rs`: the ring (first in, first out, bounded, wraps,
  equality independent of where the ring starts), the hash tags, the pinned scatter step.
* `crates/sf_sim/tests/orders.rs`: every order, the tolerance, the destination/waypoint
  split, the shift queue and its bound, stop, refusals, the attack on a moving target and on
  a dying one (and on a new entity in its slot), guard, scatter, the legacy commands, the
  checksum, and the sequence numbers.
* `crates/sf_sim/src/command.rs`: the per-player sequence numbers.
* `assets/scenarios/orders.ron` with `tools/sf_headless/tests/orders.rs`: the whole
  scenario with the states and positions asserted at the ticks that matter; the checksum
  is in `orders.expected` (checked by `pinned.rs` and, on Linux and Windows, by CI). It
  runs on a frozen copy of the definitions (`orders_defs/`), like `units_spawn`.
* `crates/sf_game/src/commands.rs`: that a right-click, shift, C, X and H become the right
  commands for the selected units only and nothing changes before the tick runs them.
