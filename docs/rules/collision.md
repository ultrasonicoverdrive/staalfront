# Collision and local avoidance

Work package WP4.4 (PLAN.md phase 4). Code: `crates/sf_sim/src/crowd.rs` (the crowd pass; its
tests in `crowd/tests.rs`), `orders.rs` (`go_to`: arrival in a crowd, the sidestep), `behavior.rs`
(`walk`: sliding along what a step grazes), `path/slots.rs`, `path/field.rs`, `path/field_build.rs`
and `path.rs` (arrival slots made for bodies, the hook for a new route). Data: `assets/config/sim.ron`.
Scenarios: `assets/scenarios/bridge_200.ron` and `crowd_jam.ron`. Idea: `docs/red-alert-analysis.md`
section 6 (a moving unit checks whether the next cell is accessible; a blocked object is moved to a
free nearby cell; the destination is not the route). The scheme is position-based separation with a
spatial grid from the general literature (push the overlap out, in a few passes), plus a few rules for
who gives way; written from scratch.

Until WP4.4 units were points that walked through each other: 200 units sent to one place stood on
top of one another, and a column was as wide as one unit. Now a unit has a **body**, and the bodies of
two units never overlap beyond a small slack. A crowd that has to squeeze through a gap queues,
units that meet head-on get by, a unit holding position is a wall, and a unit that cannot get on
steps aside, and in the end asks for a new route. Everything is integer, deterministic and bounded.

## The body

The body of a unit is a **disc**: its radius is half the `size` of its definition
(`unit_defs.ron`, "diameter in cells for collision and selection": 0.4 for infantry, 1.2 for a light
tank, 1.5 for a heavy one). Nothing else is data of the body: a new unit with a `size` has one, no
Rust change.

* **Who has one.** A unit made from the unit table, whose movement class terrain restricts (foot,
  wheeled, tracked, hover and water as shipped). **Aircraft have none**: a class that can enter every
  terrain type flies over everything, and nothing flies into it. A structure is not a body but cells
  (`terrain.md`): a unit can never be pushed into one (below). An object has none.
* **Who does not.** The legacy `Wander` and `MoveTo` walks, and everything without a table entry
  (the wanderers of `Spawn`), are not bodies at all: they neither push nor are pushed. That is how
  `wander_5000`, `command_mix` and `units_spawn` keep their checksums.
* **The distance two bodies keep** is the sum of their radii, with one exception: two **small**
  bodies (a `size` of 0.75 cells or less, so infantry) keep three quarters of it. A crowd of riflemen
  packs closer than a crowd of tanks; a rifleman and a tank keep the full sum.
  Bodies may be closer than that by a **slack** of a thirty-second of a cell (`SLOP`) before anything
  is done, and the push brings a pair to half the slack, so a settled crowd does not twitch.
* **Mass** is the size rounded up to a whole cell, one to four: a rifleman 1, a light tank 2, a ferry
  3. Movers share a push by it (below).
* **A vessel under a bridge is a body on the bridge's cells.** Bridge cells are one layer
  (`movement.md`); a gunboat passing under a bridge while tanks cross it collides with them. The
  sandbox has no vessels; layering is a change of its own (see "Not here").

## One tick

After the path service has served its requests and every unit has taken the step its order asked for
(`behavior::advance_all`), the **crowd pass** runs (`Crowd::resolve`). It is skipped when no unit
walks an order and no idle unit is still settling. Otherwise, up to `PASSES` (6) times:

1. the **spatial index** is rebuilt from the positions as they are: a uniform grid of buckets, a
   counting sort in slot order into fixed arrays. A bucket is as wide as the widest body, so the nine
   buckets round a unit hold everything that can overlap it. No hash map.
2. every overlapping pair that has a unit that moves (or one that was pushed in the pass before)
   shares the overlap by **weight** (below), as a push along the line between the middles.
3. on the first pass only, the **steering** is added: keep right, and the detour (below).
4. every unit that was asked to move is moved by the sum of its pushes, **if it may**: the step is
   one `Ground::may_step` allows (nothing the class cannot enter, no structure cell, no cut corner,
   inside the map; the guard of `movement.md` stays), and an idle unit is not moved into a body that
   does not give way. When the whole push is refused the two sides of it are tried (a unit pressed
   against a wall slides along it), then half of it.

When no pair overlaps the passes stop. After the last pass the units that make no progress are looked
at (below).

### Who gives way, and how much

Every participant has a **role**, from its order:

| Role | Is | Gives way |
|---|---|---|
| mover | an order that walks (move, attack-move, attack, guard of an entity or of a point) | to other movers, by mass (a push is shared in proportion to the inverse of the mass, so the heavier moves less) |
| idle | no order (or one that ended) | to a **friendly mover** only |
| hold | `HoldPosition` | **never**: it is never pushed |

* **Units moving the same way follow.** When two movers have headings less than 60 degrees apart,
  the one behind gives way completely and the one in front is not pushed on. A queue is therefore
  not a line of units shoving each other forward, and the front unit walks at its own pace. A unit
  that gave way like this is *in line*, and waiting in line is not being stuck.
* **Units that meet head-on keep right.** When two movers have headings more than 120 degrees apart
  and one is ahead of the other, each slips to **its own right** (the heading turned a quarter, so
  for two units that face each other that is two opposite sides: they pass each other, they do not
  mirror each other) by half a step a tick, on top of the push that keeps them apart. The rule is
  fixed, not drawn: the same meeting always ends the same way. (`y` grows downward, so going east the
  right hand is south.)
* **An idle friendly unit is pushed aside** by a mover, as a body of a movement class it may enter
  and only to a spot where it overlaps no body that does not give way: a unit holding position, an
  enemy, an idle unit that cannot move either. The push is **bounded**: at most an eighth of a cell a
  tick (`IDLE_TICK_CAP`, about the step of a light tank) and at most three cells in all
  (`SHOVE_LIMIT`) until the unit has been left alone for two seconds (`SHOVE_CALM`; being overlapped
  by a mover counts as not left alone, so a stream of units cannot take the allowance back). A unit
  that gets less than half of what it was asked (it is stopped, or pushed from two sides at once) is
  treated as not moving for the rest of the tick, and the units pressing on it give way instead.
  Idle units are never pushed by enemies and never push each other except that a unit that was just
  pushed may push its neighbour in the passes that follow (a crowd opens a little).
* **An enemy** gives way as a mover when it moves and as a wall when it is idle.

### Order does not matter

This is the part that makes it deterministic and safe for lockstep. Every pass reads **one snapshot**
of the positions. For each unit it sums the pushes of every overlapping pair; a pair's push is a
function of the two units alone, each share rounded on its own, so it does not matter which of the two
is looked at first, and integer addition does not care in which order the pairs come. Every unit is
then judged (may it go there?) against the same snapshot, and only when all are judged are the moves
written. So the result does not depend on the order the units, the pairs or the buckets are visited
in. The one place a slot index is used is two bodies on exactly the same point, where something has to
say which way each goes: units that start on exactly the same point are **split by slot index** (a hash
of the two indices picks the direction; the lower slot goes that way, the higher the opposite). A slot
index is the spawn order, so a crowd spawned on one single point can end differently when it is spawned
in another order; units that start apart cannot. A group order lists its ids in any order: the world
sorts them by slot before it makes the groups, so the same ids give the same result.

`crates/sf_sim/tests/collision.rs` proves it by spawning one crowd of 28 mixed units (two sides,
friends and enemies, fast and slow, some against the stream) in three orders (forward, reversed, a
fixed shuffle) and comparing where every unit ends: the same, unit for unit. A second test gives one
group order (flowfields, arrival slots) to 24 units with the ids listed forward, reversed and
shuffled, and compares the final positions, the slots and the checksum. It also runs the same
meeting twice with the two units' roles exchanged, and a clone of a world mid-run.

## Wait, detour, stuck

A unit that is blocked does not push harder and does not give up at once.

* **Progress** is a new best distance to the point it is steering at (its waypoint), by at least a
  thirty-second of a cell (`PROGRESS_EPS`). A tick in which the unit tried to move and did not make
  progress is a **quiet** tick (`World::stuck_ticks_of`); progress or a new waypoint ends the count.
  Turning on the spot is neither, and neither is waiting for a route from the path service, and
  neither is waiting in line behind a unit that goes the same way.
* **Wait, then detour.** After `wait_ticks` (6 as shipped, a third of a second) of being blocked by
  something that does not give way (a unit holding position, an enemy standing there), a mover slips
  to its right like a head-on unit does (to its left on every second sidestep), by half a step a tick,
  and goes round. A unit in line behind a mover does not: it waits for it.
* **Stuck.** After `stuck_ticks` (60 as shipped, three seconds) the unit **steps to a free
  neighbouring spot**: the middle of one of the eight cells round it, tried right of its heading
  first, then left, then the diagonals, then forward and back, the first its class may enter with no
  body in it. For a few ticks the walk to the waypoint is replaced by the walk to that spot (a
  sidestep, not a jump), then the order goes on. When no spot is free the attempt is counted all the
  same.
* **Asks for a new route.** After `stuck_attempts` (3 as shipped) sidesteps in a row that did not
  help, the next stuck period **asks for a new route**: `Pathing::request_replan` (a unit with a route
  of its own gets a new search from where it stands and goes on along the waypoints it has until it
  is ready; a member of a flowfield forgets the leg it chose and recovers from a stranded route and
  stays on the field) and the unit is **marked** (`World::replan_requested`, until its order changes
  or ends). This is the hook of WP4.7: the replan that knows about blocked routes and vanished targets
  will start from this mark and this request path; WP4.4 adds nothing to it but the request.
* A unit that is **stranded** by terrain with a goal that can be reached (two refused steps, or a
  cell-by-cell route that was refused) counts quiet ticks too, so it gets the same help. A route that
  ends short because its goal cannot be reached at all (`unreachable`, `pathfinding.md`) does not:
  that is WP4.7's.
* **A step the terrain refuses slides**: when a unit that was pushed off the line its route was
  smoothed along grazes a corner, it moves by the part of its step along the obstacle (the longer part
  first) instead of being refused and asking for a cell-by-cell route. The legacy walks do not slide:
  they stand at the edge (`movement.md`). A unit that has been blocked for a while within a cell of a
  waypoint of its route takes it as reached when the route has another to go on to (the waypoints are
  the middles of cells and a body is as wide as one: the unit in front is standing on it).

The data are `wait_ticks`, `stuck_ticks` and `stuck_attempts` in `assets/config/sim.ron` (and the
scenario field `collision: Some(false)` or `World::with_collision` for a world that has none of it).

## Arrival

A crowd sent to one point cannot all stand on it. Two things make the end of a move work for a crowd.

* **Slots are made for the bodies** (`path/slots.rs`). The arrival slots of a group (WP4.3) are
  points on a square lattice round the goal. The lattice of a class is spaced for the widest body it
  holds, plus a sixteenth of a cell: 0.36 for infantry, 1.26 for a light tank, so that a unit standing
  on its slot is not pushed by its neighbours. The classes of one order take **rings**, the fastest
  first (it arrives first, and the slower ones then walk to the rim without crossing it): a class
  starts where the area of the classes before it ends (a lattice point is one `spacing * spacing` of
  ground), and the field of a later class skips every point that is the same as, or nearer than the
  sum of the two bodies' radii to, a slot of an earlier class of the same order (`slots::keep_apart`:
  0.8 of a cell for infantry beside a tank, not the 0.75 of the two spacings less the gap), so that no
  two classes share a slot even when blocked points were left out of the earlier one. (WP4.3 handed out stretches of one spiral by counting units; with
  rock beside the goal the stretches ran into each other.)
* **A crowd stands on the spot.** A unit that has been blocked for `wait_ticks` by a friend that has
  arrived (an idle friendly body overlapped it last tick), within a cell and a half of its slot, has
  **arrived**; so has one that is as near the goal as its slot is, or nearer (it walked in through the
  crowd and its slot is behind it), within the same. Each sidestep it has tried since widens that by
  two cells. A unit that is blocked by units that are still walking has not arrived: a queue is not an
  arrival. A plain move only, not a reach (an attack).

### Five cells cannot hold 200 bodies

PLAN.md's "done when" for phase 4 used to ask for at most 1 % of the units outside 5 cells (20 m) of
the goal. That was written when units were points. With bodies of the shipped sizes it cannot hold for
`bridge_200`: the 110 vehicles of the mix (a tank is 1.2 cells across) need about 150 cells of
ground, even on the square lattice the slots use, and a disc of five cells has 78. About half the
vehicles must stand further out however well they are packed, and 200 units need a disc of **7.4
cells** (the radius whose area is one `spacing * spacing` for every unit). The orchestrator decided on
18-10-2026 that the criterion is now: all arrive, none stuck; at most 1 % end further from the goal
than **1.3 times that radius**; no two bodies overlap by more than a quarter of a cell during the run
or 1/16 of a cell at the end (PLAN.md, phase 4). `bridge_200` asserts exactly that. A hundred
riflemen do fit in five cells and a hundred tanks do not (`tests/collision.rs`).

## Numbers

| Number | Value | Where |
|---|---|---|
| radius of a body | half the `size` of the definition | `unit_defs.ron` |
| two small bodies (`size` up to 0.75) keep | 3/4 of the sum of the radii | `SOFT_SIZE`, `SOFT_NUM`, `SOFT_DEN` |
| slack before a push | 1/32 cell | `SLOP` |
| passes per tick | 6 | `PASSES` |
| most a mover is moved by pushes in one pass | 1/4 cell | `MOVER_PASS_CAP` |
| most an idle unit is pushed in a tick, and in all | 1/8 cell, 3 cells | `IDLE_TICK_CAP`, `SHOVE_LIMIT` |
| an idle unit's allowance is back after | 40 ticks left alone | `SHOVE_CALM` |
| progress | 1/32 cell closer than ever before | `PROGRESS_EPS` |
| wait before a detour | 6 ticks | `wait_ticks` in `sim.ron` |
| stuck: sidestep to a free spot | 60 ticks | `stuck_ticks` in `sim.ron` |
| sidesteps before a new route is asked for | 3 | `stuck_attempts` in `sim.ron` |
| a crowded unit has arrived within | 1.5 cells (+2 per sidestep) | `CROWD_ARRIVE`, `CROWD_ARRIVE_STEP` |
| a waypoint is reached when blocked within | 1 cell | `WAYPOINT_CROWD` |
| slot spacing | the widest body (3/4 of it for infantry) + 1/16 | `slots::spacing_for` |

They are placeholders for the balance rounds in the sense that a number may change; the shape (the
rules above) is decided here.

## Data, scenarios and the checksum

`assets/config/sim.ron` has `collision` (on), `wait_ticks`, `stuck_ticks` and `stuck_attempts`
(`sf_data::sim_settings`, handed over by `sf_bridge::collision_config` to `World::with_collision`; a
world that is given none uses `CollisionConfig::DEFAULT`, the same numbers, so the tests and the
frozen scenarios are not moved by turning a knob in the file). The headless scenario field
`collision: Some(false)` turns bodies off for a scenario that pins routes or terrain costs with exact
positions and has units that start side by side or share a goal: `terrain_costs` (four units
**spawned on one cell**) and `pathing` (three units sent to the same unreachable goal, each asserted
to stand in the middle of its nearest shore cell) set it, and their checksums are the ones pinned at
WP4.2 and WP4.3. `orders` and `pathing_256` run with bodies and were re-pinned.

The checksum has a section `K`, written only while some live unit remembers something about a crowd
(it was blocked, sidestepping, shoved, crowded or asked for a new route): the settings, then each such
unit in slot order. A unit that never met a crowd is not in it, so a world where nobody collides hashes
as before, and so does one with collision turned off. The arrival slots joined section `L`: each
field also hashes its group order, its ring, its spacing and its inner radius.

## Measured

* **`bridge_200`** (re-pinned): 200 units, 90 riflemen, 60 light tanks, 45 light transports, 5 hover
  craft, across a bridge three cells wide and six long and through a wall with a gap. **All arrive by
  tick 3,526 (176 s of game time; 107 s when the units went through each other).** The last unit
  crosses the river on tick 1,976. Is that plausible? The crowd is about 170 cells of bodies and
  crosses a gate three cells wide at the pace of the slowest class (0.7 cells a second, and nobody
  overtakes on a bridge a tank wide): 170 / (3 x 0.7) = 81 s for the crossing alone (at most 30 on
  the bridge at once), and the slowest riflemen then walk 30 cells to the goal and wait for the ring
  slots of the classes before them to fill. Nobody is stuck at the end and nobody stood still for
  more than 73 ticks (3.7 s) with its order not done; 91 sidesteps and 6 requests for a new route in
  all; at most 0.22 of a cell of overlap at the worst moment of the run (checked on every tick; squeezed
  against the crowd that has arrived) and none beyond a sixteenth at the end; none is ever on ground its
  class cannot enter; the path service spent 28,429 nodes in all, the budget held every tick. The units
  walk 1.2 times the best grid walk at the median. 140 of the 200 end outside five cells of the goal and
  the farthest 9.55, inside the 9.58 of the criterion (1.3 x 7.37). (**Re-pinned and re-measured at WP4.5**,
  when the hull turning rules came in: all arrive by tick 3,676, 1 of the 200 ends beyond 9.58 and the farthest
  is 9.72, the worst overlap is 0.19; `turning.md`, "`bridge_200`, measured".)
* **`crowd_jam`** (new): two groups of 18 pass head-on through a three-cell gap eight cells long, 8
  tanks go along a five-cell corridor past a heavy tank on `HoldPosition`. Every unit has done its
  order by tick 1,598; the groups are in the gap together on 471 ticks, the eastbound units keep to the
  south half of it and the westbound to the north half; the holder never moves a hair; one sidestep
  in all; at most 0.06 of a cell of overlap.
* **Performance** (`tools/sf_headless/tests/crowd_perf.rs`): 2,000 units with bodies on 256 by 256
  cells of grass, in two dense blocks of 1,000 (riflemen and light tanks, a cell apiece) that start a
  cell apart and each get one group order (flowfields and arrival slots) to the point between them, so
  they run into each other for the whole run: 400 ticks, **79 ticks a second in a release build
  (12.7 ms a tick, the slowest 19.5 ms), 11 in a debug build (93 ms a tick, the slowest 163 ms)**,
  against the 20 the game needs. The test asserts that the blocks really met: 56,000 idle units
  shoved aside in all, 213 sidesteps, and up to 137 pairs of bodies from the two blocks overlapping at
  once. (An earlier version of this test started the halves 138 cells apart; they never met in its
  300 ticks and its 522 ticks a second were the cost of a crowd that does not collide.) The
  wanderers of the `sf_demo` benchmark are not bodies and are not touched.
* `pathing_256`: the 200 tanks, each sent to its own goal 300 cells away across two rivers, all
  arrive (by about tick 6,300).

## Not here

* **Layers**: a vessel under a bridge collides with what crosses it (above). A second layer needs a
  definition (which classes pass under which cells) and is a change of its own.
* **Turning and the turret** (WP4.5, `turning.md`): a pushed unit still slides without turning (the pushes
  are corrections of the position and bypass the turn constraint, by design), and the facing is the
  one of the order's walk; what a unit drives itself in a crowd (a sidestep, a detour) goes through
  the turning rules of its class.
* **Formations** (WP4.6): slots are rings of a lattice, not a shape that keeps; the slowest does not
  set the pace; a group arrives in the order it can.
* **Replanning** (WP4.7): the request exists and is marked; a replan that knows about blocked routes,
  a vanished target or a better route is that work package's. Routes and fields still do not know the
  other units, so a crowd is walked round by pushing and sidestepping, not by a better route.
* **Crushing, melee reach and weapons**: an attacker whose weapon reach is less than the sum of the
  radii cannot get in reach of a body (no shipped weapon is that short); infantry does not get run
  over.
* **Dynamic slots**: a unit whose slot is behind the crowd walks through it (pushing idle friends
  aside, as far as they may go) instead of taking a free slot at the rim. The measured tail of
  `bridge_200` (the last units arrive 20 s after the last crossing) is that.
* The pushed units are not animated: a unit that is shoved slides, it does not step.

## Where it is tested

* `crates/sf_sim/src/crowd/tests.rs`: the settings, the shapes, the distance two bodies keep, the
  spatial index against a plain scan of every pair, the state a unit remembers.
* `crates/sf_sim/tests/collision.rs`: the radius, the settings, infantry closer than vehicles, a
  crowd that never overlaps beyond the slack and arrives, following, head-on passing and the mirrored
  meeting, a holder never pushed, wait then detour, an idle friend pushed and an enemy not, the push
  bounded in a tick and in all, an idle unit not pushed into rock or a structure, stuck (a sidestep,
  then the request and its mark), **the outcome independent of the order the units were spawned in**,
  reproducibility and a clone, collision off, aircraft and legacy walks not bodies, the slots of two
  classes all different and a body apart, a hundred riflemen in five cells and a hundred tanks not.
* `crates/sf_sim/src/path/field_tests.rs`: the ring layout, a later class skipping an earlier class's
  slots, a stranded member recovering when the map changes and not keeping its field alive, an idle
  field expiring while it is being built.
* `tools/sf_headless/tests/bridge_200.rs`, `crowd_jam.rs`, `crowd_perf.rs`: the scenarios above.
