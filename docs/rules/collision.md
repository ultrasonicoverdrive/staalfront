# Collision and local avoidance

Work packages WP4.4 and WP4.4b (PLAN.md phase 4). Code: `crates/sf_sim/src/crowd.rs` (the crowd
pass; its tests in `crowd/tests.rs`), `orders.rs` (`go_to`: arrival in a crowd, the sidestep),
`behavior.rs` (`walk`: sliding along what a step grazes), `path/slots.rs`, `path/claim.rs` (the
slot a unit takes when it comes near, WP4.4b), `path/field.rs`, `path/field_build.rs` and
`path.rs` (arrival slots made for bodies, the hook for a new route). Data: `assets/config/sim.ron`.
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

### Bounds that hold whatever presses on the crowd (WP4.4b)

The passes bring a pair of bodies apart when they can. They cannot always: a crowd pressed into a
dead end or a gap squeezes the units in the middle from both sides, and the idle ones among them
have used up their allowance. The review of WP4.5 found the phase's overlap bound (a quarter of a
cell) met only by the luck of the crowd. Three rules make it hold by construction; all three are
decided from the positions the passes ended with and written after, so the order of the units
does not matter.

* **A shove is not a squeeze.** An idle unit is not shoved into another idle unit that could give
  way by more than `COMPRESS` (an eighth of a cell) beyond the slack. A shove that would is
  refused; the unit is treated as one that cannot move for the rest of the tick and the mover
  takes the push.
* **A mover goes back instead of going in.** A mover that ends the tick deeper than
  `MOVER_OVERLAP` (3/16 of a cell) in another body, and that was less deep where it began the
  tick, goes back to where it began it (`hold_back`) and waits there; waiting counts as being
  blocked, so the wait, the detour and the stuck rule take over.
* **The last resort** (`separate`). Two bodies of the units this tick touched that still overlap
  by more than `HARD_OVERLAP` (3/16 of a cell) are pushed apart by the excess, at most a quarter
  of a cell for each, to where the terrain allows. A unit holding position is never moved (the
  other takes it all), and an idle unit is never moved by an enemy. It runs after the passes and
  `hold_back`, and does nothing in a crowd that has settled.

A unit that has just **stopped** (its order ended) is looked at once by the pass (`settle`), with
its allowance back: if it came to rest against a neighbour that cannot give way any more, it is
the one that moves clear, not a pair that overlaps for good because nobody is pushing.

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

A crowd sent to one point cannot all stand on it. Four things make the end of a move work for a
crowd.

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
* **A unit takes its slot when it comes near** (`path/claim.rs`, WP4.4b). The rings are a plan, and
  the order the units arrive in is not the order of the classes: a queue on a bridge mixes them. Slots
  handed out by rank (the n-th id gets the n-th slot) left a slow rifleman that came early standing
  in the outer ring, which the tank that came after it had to push through to reach its slot in the
  ring inside; the crowd spread out, the holes were filled by others, and the units that came last
  ended up cells from where they belonged. Now a unit that comes within six cells of the outermost
  slot of its class and holds none takes one:
  * it looks at the slots nobody holds, nearest the goal first, and skips every one that lies nearer
    to a slot already held, **of any class of the order**, than the two bodies keep apart;
  * it takes the **first one it can walk to straight**: it sees it, and no slot already held lies
    nearer the line to it than the two bodies keep apart. The crowd fills from the middle outwards on
    the side the units come from, and what is left for the last one is never behind the others;
  * when none of those looked at is open it takes the nearest it can see, and when it sees none the
    first one that is free. When all are taken (a group bigger than 256) it shares the slot its rank
    gives, as before; the slot its rank gives is also where it goes before it has taken one;
  * the units look in the order of their entity ids, one at a time (a claim is a write the next unit
    sees), at most 16 in a tick. A list of ids in any order gives the same result.

  The claim is kept after the order ends, until the unit gets a new order or is removed. A unit that
  gets **stuck** on its way (the sidestep of "Wait, detour, stuck") lets go of its slot and takes
  another when it can: a slot that is walled off is not waited for. A rebuilt field starts with an
  empty table.
* **A crowd stands on the spot.** A unit that has been blocked for `wait_ticks` by a friend that has
  arrived (an idle friendly body overlapped it last tick), within a cell and a half of its slot, has
  **arrived**; so has one that is as near the goal as its slot is, or nearer (it walked in through the
  crowd and its slot is behind it), within the same. The slack grows by two cells for each sidestep it
  has tried since, or for each sixty ticks (`pressed`) it has been held up by units that have arrived,
  all told, whichever is more: a unit that presses on a crowd standing on its destination, a single
  walker that has no slot among them, counts as arrived farther and farther out and cannot go on
  circling the rim for ever. A unit that is blocked by units that are still walking has not arrived:
  a queue is not an arrival. A plain move only, not a reach (an attack).
* **A crowd that has stopped closes up.** The pushes that let a late unit through a crowd leave the
  units they moved where the pushes left them. A settled unit of a group (`home`: the point the group
  was sent to) that has been left alone for `HOME_WAIT` (a second) steps toward that point, a sixteenth
  of a cell a tick, to a spot that overlaps nobody beyond the slack: straight at it or, when that is
  taken, turned away from it by up to about 83 degrees to either side, always a spot nearer it. A unit
  that cannot move rests until something touches it or moves next to it. Whoever stands where, the
  crowd ends as close to the point as its bodies allow, and a unit never ends far out because of how it
  was pushed on the way. (Without it the farthest unit of `bridge_200` ended up to 10.5 cells out at
  several radii; with it 8.1 to 9.5 at all the radii tried.) The units walk to their slots all the same: this
  is the cleaning up after the walk, not a second destination.

Why there is no stuck rule that declares a unit arrived wherever it stands: "stuck" is one of the
failures the tests look for, and a rule that makes it disappear would hide it. A unit that is stuck is
still stuck (`stuck_ticks_of`, the request for a new route and its mark); what changed is that it no
longer waits for a slot it cannot reach (it takes another), that a unit pressed against the crowd that
stands on its destination counts as arrived at a distance that grows with the time it has pressed, and
that `bridge_200` asserts that every unit's order is done and that none stands still for half a minute.

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

After WP4.5 the criterion held at a wheeled turning radius of 1.25 cells and failed at 1.5, 1.6, 1.75
and 2.0: it had been met by the luck of the crowd. The tests `phase_four_holds_at_a_wheeled_radius_of_*`
now run the scenario at each radius the review swept (1.25, 1.5, 1.6, 1.75 and 2.0) and ask for every
way the run misses the criterion at once (`Run::misses`). It also held, when the work was done, at
every wheeled radius from 1.05 to 2.0 cells in steps of a twentieth and at 2.1, 2.2, 2.3, 2.5 and 3.0;
at hover radii of 1, 1.5, 2, 2.5, 3 and 4 cells; with the wheeled turn on the spot at 60 and at 1000
thousandths of the turn rate; and with the kinds of the block dealt out in four other orders (the
same cells, the mix shifted by 7, 33 and 101 places and reversed). Those runs are probes, not tests;
the five radii are the tests.

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
| a unit takes its slot within | 6 cells of the outermost slot of its class | `claim::CLAIM_MARGIN` |
| units that take a slot in a tick | 16 | `claim::CLAIMS_PER_TICK` |
| free slots one claim looks at | 400 | `claim::LOOK` |
| a shove may squeeze an idle friend by | 1/8 cell beyond the slack | `COMPRESS` |
| a mover may end a tick deeper in a body than | 3/16 cell (else it goes back) | `MOVER_OVERLAP` |
| two bodies may end a tick deeper than | 3/16 cell (else pushed apart, at most 1/4 each) | `HARD_OVERLAP`, `SEPARATE_CAP` |
| a settled unit closes up after | 20 ticks alone, 1/16 cell a tick | `HOME_WAIT`, `HOME_STEP` |
| held up, all told, per extra two cells of arrival slack | 60 ticks | `PRESSED_PER_STEP` |

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

WP4.4b adds to section `K` a unit's slot (the field and the place), the point it closes up toward and
whether it rests, and how long it has been held up in all; to section `L` the table of who holds
which slot of a field. A unit that never took a slot adds nothing, so `wander_5000`, `command_mix`,
`terrain_256` and `units_spawn` did not move. **The re-pins of WP4.4b**, once, with the wheeled
radius of the frozen definitions going from 1.25 to the shipped 1.5 cells (`assets/config/` and the
four frozen copies that carry the block):

| Scenario | Before (WP4.5) | After | Moved by |
|---|---|---|---|
| `wander_5000`, `command_mix`, `terrain_256`, `units_spawn` | | unchanged | |
| `orders` | `0x0dcb52425f1adc60` | `0xf8fbe06cc8a51962` | the radius only |
| `terrain_costs` | `0xdc9d5c4a65a60508` | `0xacd34a784a0fd0be` | the radius only |
| `pathing` | `0x606eb6feeffa964c` | `0x398d625625dce5c1` | the radius only |
| `pathing_256` | `0x7c4e31d024ed4cf9` | `0x016c2b5ef7c7e055` | the radius only |
| `turning` | `0xb07ba8066dd59884` | `0xd894752d6d192f8d` | the radius only |
| `bridge_200` | `0x03b3145755a9081e` | `0x831cd1eaff2b1307` | the arrival logic and the radius |
| `crowd_jam` | `0x3d8fab0ab9a564e6` | `0x4ac3f990f7070f48` | the arrival logic and the radius |

"The radius only" was checked: with the frozen radius put back to 1.25 the five scenarios give their old
checksums bit for bit. `terrain_costs.rs` pins the arrival ticks of the wheeled units again (only those
moved: 152, 185, 183, 164 and 176 for grass, ore, bridge, water and rock; the rock lane was 109)
and `bridge_200.ron` runs 4,600 ticks instead of 3,800 (below).

## Measured

* **`bridge_200`** (re-pinned at WP4.4b): 200 units, 90 riflemen, 60 light tanks, 45 light
  transports, 5 hover craft, across a bridge three cells wide and six long and through a wall with a
  gap five cells wide, at the shipped wheeled radius of 1.5 cells. **All arrive by tick 3,411 (170 s
  of game time; 106 s when the units went through each other).** The last unit crosses the river on tick
  1,837. Is that plausible? The crowd is about 170 cells of bodies and crosses a gate three cells wide at
  the pace of the slowest class: 170 / (3 x 0.7) = 81 s for the crossing alone. The gap in the wall
  behind the bridge is the slower of the two: the field leads everybody to the same corner of it, so the
  traffic is little wider than one lane and passes 6 to 13 units in a hundred ticks (the bridge passes
  12 to 20), the last one (a hover craft) on tick 2,966; the slowest riflemen then walk 14 cells to
  their slots, the last of them arriving on tick 3,411. Nobody is stuck at the end and nobody stood still for more than
  76 ticks (3.8 s) with its order not done; 74 sidesteps and 5 requests for a new route in all; **the
  worst overlap of two bodies from the first step on is 0.19 of a cell (the first tick, where the
  scenario spawns tanks a cell apart), and none beyond a sixteenth at the end**; none is ever on
  ground its class cannot enter; the path service spent 27,515 nodes in all, the budget held every
  tick. The units walk 1.25 times the best grid walk at the median. **None of the 200 ends further than
  9.58 cells from the goal (the criterion: at most 2); the farthest is 9.08**, 94 of the 200 outside five
  cells.
  **The same run at every radius the review swept** (the tests `phase_four_holds_at_a_wheeled_radius_of_*`,
  release build; "last" is the tick the last unit arrives, "farthest" the farthest from the goal):

  | wheeled radius | last | farthest | worst overlap | longest stood still | sidesteps / new routes |
  |---|---|---|---|---|---|
  | 1.25 | 3,475 | 8.99 | 0.19 | 55 | 85 / 4 |
  | 1.5 (ships) | 3,411 | 9.08 | 0.19 | 76 | 74 / 5 |
  | 1.6 | 3,409 | 8.10 | 0.19 | 120 | 111 / 6 |
  | 1.75 | 3,396 | 9.46 | 0.19 | 101 | 109 / 7 |
  | 2.0 | 3,587 | 9.14 | 0.19 | 123 | 125 / 7 |

  Before WP4.4b the same sweep (the reviewer's, on the code of WP4.5): 1.5 left 4 riflemen who never
  arrived, 1.6 left 9 and put 6 beyond 9.58 cells, 1.75 overlapped by 0.286, 2.0 left a hovercraft
  stuck. The limit of the test is tick 4,400 and the scenario runs 4,600 ticks: the last arrival
  moves between 3,200 and 4,200 over the radii and mixes tried (it depends on how the queue at the gap
  falls) and the criterion is that everybody arrives, not when; with the old 3,800 a unit that was a
  few ticks from its slot counted as one that never arrived.
* **`crowd_jam`** (re-pinned at WP4.4b): two groups of 18 pass head-on through a three-cell gap eight
  cells long, 8 tanks go along a five-cell corridor past a heavy tank on `HoldPosition`. Every unit
  has done its order by tick 1,548; the groups are in the gap together on 469 ticks, the eastbound
  units keep to the south half of it and the westbound to the north half; the holder never moves a
  hair; one sidestep in all; at most 0.07 of a cell of overlap.
* **Performance** (`tools/sf_headless/tests/crowd_perf.rs`): 2,000 units with bodies on 256 by 256
  cells of grass, in two dense blocks of 1,000 (riflemen and light tanks, a cell apiece) that start a
  cell apart and each get one group order (flowfields and arrival slots) to the point between them, so
  they run into each other for the whole run: 400 ticks, **66 ticks a second in a release build
  (15.2 ms a tick, the slowest 27.4 ms), against the 20 the game needs and the 50 ms of a tick**. The
  code of WP4.4 measured 87 ticks a second (11.5 ms, the slowest 19.2) on the same machine: the
  checks after the passes (`hold_back`, `separate`) and the slots taken on approach cost about a third
  more. The test asserts that the blocks really met: 69,251 idle units shoved aside in all, 115
  sidesteps, and up to 125 pairs of bodies from the two blocks overlapping at once. The wanderers of
  the `sf_demo` benchmark are not bodies and are not touched.
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
* `crates/sf_sim/tests/collision.rs`, "the arrival logic of a group" (WP4.4b): a column of tanks takes
  its slots in the order it comes (the nearest the innermost), a mixed-speed group ends inside the disc
  that holds its bodies, a settled crowd closes up again after a tank has driven through it and its
  members are at home (and a new order lets go of home), and a crowd pressed into a dead end never
  overlaps by a quarter of a cell.
* `tools/sf_headless/tests/bridge_200.rs`: `phase_four_holds_at_a_wheeled_radius_of_*`, one test for each
  radius of the review sweep, each asking `Run::misses` for every way the run falls short of the
  criterion; the radius is replaced in the movement table by a test-only knob.
* `crates/sf_sim/src/path/field_tests.rs`: the ring layout, a later class skipping an earlier class's
  slots, a stranded member recovering when the map changes and not keeping its field alive, an idle
  field expiring while it is being built.
* `tools/sf_headless/tests/bridge_200.rs`, `crowd_jam.rs`, `crowd_perf.rs`: the scenarios above.
