# Formations

Work package WP4.6 (PLAN.md phase 4: "group keeps shape, slowest sets the pace"). Code:
`crates/sf_sim/src/formation.rs` (the formations, the lattice, the tick), with small parts in
`orders.rs` (a unit in a formation walks to its slot instead of the order's route),
`world.rs` (`apply` makes the formations, `step` updates them), `path.rs` and `path/slots.rs`
(the arrival lattice turned across the way the group came), `movement.rs` (which classes cross
the same ground) and `checksum.rs` (section `G`). Data: `assets/config/sim.ron`. Scenario:
`assets/scenarios/formation_move.ron` (map `assets/maps/formation`). Idea: the ordinary one of an
RTS group move: the player selects a handful of units of mixed speed, gives one move order, and
they arrive together and in the shape they set off in, not strung out by speed.

## What it does in one paragraph

One `Move` order that sends enough units that cross the same ground to one point makes them a
**formation**. The group's current layout becomes a **lattice** with room for the widest body;
each unit gets an **offset** in it and so a **slot** that hangs from the **leader** (the unit
nearest the middle) and turns with the way the leader goes. The leader follows the order's route
or flowfield no faster than the slowest member; the others walk straight to their slots. Where
the ground is narrow the shape lets go and the units file through, **at the pace of the formation**
(a narrow place loosens the shape and never the pace, WP4.6b); on open ground the leader waits
for stragglers and the shape forms up again. Near the goal the formation is let go and the
members take the arrival slots of the goal, as the units of every group order do.

## Who forms a formation

| | |
|---|---|
| Order | `Order::Move` to a point, **not** attack-move, guard, hold, stop, scatter or an attack (they keep their behaviour). A single unit is no group. The units must start the order now (a queued order behind another does not start one). |
| Ground | Units of the **same family**: classes that may enter the same terrain types. Foot, wheeled and tracked units are one family (so a mixed ground group is one formation); a hovercraft, a vessel are each a family of their own; aircraft (no terrain stops them) form none. A vessel and a land unit sent together do **not** form one formation: the family that can reach the goal forms its own (the goal must be a cell the class may enter), the other follows the order as it always did. |
| Number | At least `formation_min_units` (3 as shipped) units of one family, and the order sends at most `formation_max_units` (19, the largest size at which the sweep below is clean for every shape and mix; see "Sizes") units **in all**, of whatever families: a bigger selection is a crowd and makes no formation at all. |
| World | `formation: true` and the world has pathfinding (terrain and a movement table). |

**Decision: formation-move is the default for a group move order** (`sim.ron`: `formation: true`),
**for orders of 3 to 19 units**. It is on because it does what the phase asks for those, and it leaves the
criterion of phase 4 alone (`formation: Some(false)` in a scenario file gives the old behaviour back); an
order above the limit moves as every group order did before. The 200-unit crowd of `bridge_200` forms **no**
formation, not even among its five hover craft: it is a crowd with flowfields and arrival slots, and its
checksum and all its numbers are what they were before WP4.6.

**The limit counts the order, not the family.** The first version counted the family: the five hover craft of
`bridge_200` formed a formation inside the crowd of 195. With the round-1 rules one of the 112 variants of
`sweep_of_variants` then overlapped two bodies of the crowd (wheeled vehicles that arrived among the hover craft
standing on their slots) by 0.270 of a cell, over the quarter of a cell of the criterion; the margin of the crowd
is thin and chaotic (twelve seeds of that variant gave 0.182 to 0.270). A formation inside a crowd buys nothing
the criterion measures and costs it margin, and a selection of 200 is a mob whatever its families.

**Why 19, measured** (not assumed). The first version had a limit of 48, picked because one formation of 195
did not work (a lattice 31 cells across on a map where the whole block is 20 wide and the first rock outcrop is
14 cells away: its leader was at x = 26.6 after 4,600 ticks, from 17.7, against a crowd that has crossed the
river by tick 2,225). Review round 1 swept the size and the shape of the group on the `formation_move` map
(`tools/sf_headless/tests/formation_sweep.rs`, "Sizes" below), found the crawl of groups of 44 to 48 and set the
limit to 24. Review round 2 found that groups of only tanks and transports of 11 to 24 arrived later in formation
than without one (up to 2.09 times their walk, as a group order up to 1.54), a delay of 200 to 800 ticks at the
passage that the mixed groups paid too, hidden in the riflemen's long walk. Round 3 made the sweep assert the
formation's time against the same group's as a group order for every run and mix, found the causes and fixed them
(each rule below that says "review round 3"), and then set the limit to the largest size at which the sweep is
clean for every shape and mix: **19**. Above it groups of 20 to 24 tanks and transports miss the bound of 1.4
times their walk in 8 of 30 runs (1.40 to 1.50), while mixed and riflemen-only groups are clean up to 24; no
single cause of that is left ("Sizes"). The limit is data (`formation_max_units`, at most 256).

## The shape

When the order is given, from the positions the units stand in:

1. **The leader** is the unit nearest the middle of the group (ties to the lower slot). It is the
   origin of the lattice: its offset is zero. The leader changes when it dies, gets another order or
   gives its order up (the member nearest the middle of those left; the offsets stay, so the shape
   does not move).
2. **The frame** points the way to the goal from the leader: `x` forward, `y` to the right. Every
   unit's position is turned into it.
3. **The lattice** has a cell of `formation_spacing_permille` (1250 as shipped: a quarter more) of the
   widest body's spacing (`slots::spacing_for`: the body, three quarters of it for infantry-sized
   bodies, and a sixteenth of a cell). One cell for all, so that any unit may stand on any point and
   two bodies never overlap on their points.
4. **Pulled in**: if the farthest unit is farther from the leader than `formation_spread_permille`
   (1500) of the radius of a disc that holds all the bodies (`pi r^2` = the sum of the spacings
   squared), every offset is scaled so that it is; a group scattered over 30 cells does not keep a
   30-cell shape.
5. **Snapped**: in the order of distance from the leader (ties by slot), each unit takes the lattice
   point nearest its own spot, or the nearest free one (rings outwards, ties to the lower point).
   A line stays a line, a block a block, and the order of the units along each row is the order they
   stood in.

The offsets are state (hashed). At every tick the **slot** of a member is the leader's position plus
its offset (minus the leader's) turned by the **heading** of the formation.

### The heading

It starts as the way from the leader to the goal. It follows where the leader actually goes (its last
step, when that is at least a quarter of the pace), **one facing step at a time** and **only from the
leader's own steps**: while the leader waits (mode `Wait`) the heading stays, however it is pushed about
by bodies that come through (`tests/formation.rs::the_heading_follows_the_leaders_own_steps...`), because
a formation that turns because somebody pushes its leader sideways turns every slot with it and no faster than
the outermost slot can be followed at the pace of the formation: one step is `2 pi / 256` of a turn,
so a step is made every `ceil(extent * 0.025 / pace)` ticks (1 to 40) and the outermost slot moves
at most as fast as the formation does. The offsets turn by it with the fixed-point rotation of
`sf_math` (`Vec2::rotate`); there are no floats.

## The pace

The **leader never steps faster than the slowest member**: the speed, **now**, of the slowest member, each
member's speed per tick scaled by the terrain it stands on (`movement::scale_step`); the leader's step is held
to it before the terrain scales it. While some member is **behind its slot** by more than half a cell (along
the heading) the leader keeps a part back: it goes at `formation_pace_permille` (900) of that speed. The
others walk to their slots at up to `formation_catchup_permille` (1000) of their **own** speed, so:

* a unit behind its slot catches up, at its own speed at most, never faster; the slowest has a tenth of its
  speed to catch up with, because the leader slows to 0.9 while it is behind;
* a unit on its slot follows it: the slot comes at most at the speed of the slowest member, so each reaches it
  and keeps up with it; one that drops behind by half a cell slows the formation until it is back;
* a unit ahead of its slot waits for it ("What a member does").

**Review round 3.** The pace was 0.9 of the slowest all the time, a tenth of the whole march for nothing while
every member stands on its slot; on the 91 cells of the sweep's march that was about a hundred ticks for a group
of tanks, a third of what the bound of 1.4 times their walk left them. The sweep's `mixed` groups went from 1.08 to
1.21 times the walk to 1.01 to 1.15 with this change alone.

A formation sets the pace of the *order*: in `formation_move` the leader goes 0.0349 cells a tick in the west
field, the rifleman's 0.035, and not faster, however fast the tanks are. What this costs is in "Sizes": a mixed
group arrives in 1.02 to 1.14 times what the slowest would need alone to walk the same way (1.01 to 1.05 as a group
order without a formation, 1.00 to 1.09 times the group order's time).

**The pace holds where the shape does not (WP4.6b).** A loose formation (below) used to mean "everybody at their
own speed": the leader at its own, the members following the order's way at theirs, nobody waiting for anybody. In
a base, among structures, the formation is loose for half of the march, and the tanks raced ahead of the
riflemen at 1.4 to 2 cells a second. Now a narrow place loosens the shape and never the pace:

* **the leader** steps at most the formation pace loose or not (the slowest member's speed, now);
* **a member that follows the order's way** (`Flow`: the formation is loose, or the member does not see its
  slot) runs at its own speed while it is no farther **ahead of its slot along the heading** than
  `formation_ahead_tenths` (2.0 cells as shipped, at most the re-form distance), and **goes no faster than the
  formation** beyond that. Behind its slot, or on it, nothing holds it back: it catches up at its own speed, and
  the slot, which comes at the pace of the formation, never gets away from it. A member standing in a narrow
  place is held to the pace, not stopped: a unit that stood in a gap would hold up the straggler the leader is
  waiting for. (Standing is for open ground, below.)

## What a member does in a tick (`MemberMode`)

| Mode | When | What |
|---|---|---|
| `Lead` | the leader | follows the order's flowfield or route (`path/`), at the pace of the formation (the slowest's speed, or 0.9 of it while a member is behind), **loose or not** |
| `Direct` | the member sees its slot (the thick line of sight of `path/los.rs`, one sector at most, no rock, water or structure within a quarter of a cell) and the formation is not loose | walks straight to the slot (`behavior::walk`, so the turning of its class applies), snaps onto it when within a step; on its slot it turns to the heading the way its class turns on the spot. It asks for no route. |
| `Flow` | the member cannot see its slot, or the formation is loose | follows the order's flowfield or route as every unit of a group does, at its own speed while it is no farther ahead of its slot than `formation_ahead_tenths`, and at the pace of the formation beyond that. When it was walking to a slot before, it chooses its way again from where it stands (a flowfield member from its cell; a route of its own is kept if its first waypoint is still ahead, within 12 cells, otherwise asked for again). |
| `Wait` | the leader waiting for stragglers; or a `Direct` or `Flow` member that is ahead of its slot (farther along the heading than the slot, by more than the re-form distance) **and on its column** (not farther to the side of the slot than the re-form distance) on open ground, **also while the formation is loose** | stands (it does not walk back to its slot; the slot comes to it). While the formation is loose this keeps the units that came out of a gap first from running on to the goal: in `formation_move`, before review round 3, the tanks did, 28 cells ahead of their slots once the formation had become loose for longer (below). A unit that is ahead **and off its column** does not stand (WP4.6b): its line of sight to the slot was blocked, it took the longer way round and came out ahead and to the side, and standing there it stood 5 to 7 cells off its column in the way of the units coming out behind it, with nobody waiting for it. It walks back to its slot if it sees it (`Direct`), and follows the order's way at the pace of the formation if it does not (`Flow`, beyond one sector or behind a structure). It is no straggler either way: it is ahead, the slot comes to it. |

### Narrow places and re-forming

Ground is **open** round a point when no rock, water or structure (by the leader's class's own costs) lies
within three cells of it in any of eight directions. The formation is open when the ground is open round the
leader **and round the front of the lattice** (the point `extent` ahead of the leader along the heading, with a
clear line to it). When it is not, the formation is **loose** for at least 20 ticks more: everybody is `Flow`
(except units ahead of their slots on open ground, which wait), the leader goes at the pace of the formation
(not at its own speed: WP4.6b, "The pace" above), nobody waits for anybody. So a bridge, a gap in a wall, a
funnel between outcrops, **a structure** lets the units file through without a formation standing in the
doorway. (Structures are narrow places on purpose: a base is not open ground, and a formation that stood
round a barracks would hold up everything behind it. What changed in WP4.6b is that the pace survives the
loosening.)

**The front, review round 3.** Looking from the leader alone (the middle of the group) let a wide block reach the
wall with its front rows still in their slots; the traffic at the mouth then pushed units off it and pinned them
against the rock beside the passage. In the worst run of the sweep (17 tanks and transports, a block 8 deep) it was
the leader, for 560 ticks, sidestepping away along the wall and back, while the others ran on to the goal. With the
front looked at as well, a block lets go before it reaches the wall and files in as a group order does; that change
alone took the failing runs of the sweep from 35 to 28. A look-ahead of 3 or 6 cells more was tried and not kept
(18 and 19 failing runs against 23 at the time).

On open ground the formation is whole again:

* a member farther than `formation_reform_tenths` (3.0 cells) from its slot is a **straggler** when it is
  **behind its slot, or that far to the side of it**; a unit ahead of its slot is none (its slot comes to it),
  also when it is ahead by more than the re-form distance and off its column (WP4.6b).
  The leader **waits** (`Wait`) for stragglers, if the leader itself is on open ground **and the members that
  stand in their places (within the re-form distance of their slots) have no rock, water or structure within three
  cells of them** (WP4.6b; the edge of the map is no obstacle here): members standing on their slots in the mouth
  of a gap held up the very stragglers the leader waited for, the rear rows of a wide block in the sweep among them
  (a straggler is not asked: it is on its way, or shut in). (Review round 3: counting
  the distance alone made a unit 2.9 cells ahead of its slot and 1.5 to the side a straggler, 3.3 cells off; after the
  passage the units that had come through first became stragglers one by one as their slots came near, and the
  leader stopped for each, for some 200 ticks. Only "not ahead" was tried first, and then a soldier shut in 10
  cells to the side, a little ahead of its slot, was no straggler at all: the formation counted as calm, refilled its
  patience and waited for it a second time, which `a_broken_formation_is_whole_again_only_after_a_calm_stretch...`
  caught.) The leader's
  `formation_patience_ticks` (300) run down one a tick while some straggler is **stalled**: has not
  come closer to its slot in a window of 20 ticks by at least a 32nd of a cell (compared with the
  nearest it has been since it fell off its slot, so a unit jostled by a crowd does not look as if it
  were on its way). While every straggler is coming closer they run down at a third of that rate: a
  straggler that is on its way is waited for three times as long, not for ever (without the third
  the five hover craft of `bridge_200`, which formed a formation there in the first version, waited
  1,336 ticks for each other in one variant of the sweep and arrived 112 ticks after the limit).
* When the patience is gone the formation is **broken**: the leader goes on whoever is behind.
* **Hysteresis.** Whole or broken is not flipped by a single good tick. The formation counts as *calm*
  while it has no straggler (whole) or at most a tenth of the members are stragglers (broken). The
  patience is **filled up again** only after `CALM_TICKS` = 60 calm ticks in a row (and not while the
  formation is loose), and a **broken formation is whole again** only after 60 calm ticks in a row off
  loose ground (`reforms` in the stats); the count starts over when a tick is not calm. Without it a
  count of stragglers that touches zero for a tick between two stragglers refilled the patience every
  time, so a formation could wait for ever in 300-tick instalments (`tests/formation.rs::a_broken_formation_is_whole_again_only_after_a_calm_stretch...`
  asserts that no broken stretch is shorter than 60 ticks).
* A member that is farther than the re-form distance from its slot and stalled for
  `formation_patience_ticks` (once the patience, not twice) **leaves** the formation and goes on as a unit
  of its own (`expelled`). Nothing waits for ever: a stuck member can hold the leader for 300 ticks and
  is then left behind (`tests/formation.rs::a_straggler_that_cannot_come_holds_the_leader_for_its_patience...`:
  a soldier shut in by four units on hold; the formation breaks, the soldier is expelled, the leader goes on).
* When the formation forms up again after being loose, and every 100 ticks while some member is stalled,
  the slots are **handed out again** (`reassign`): the assignment of units to slots with the least sum of
  squared distances (the Hungarian method over all pairs, ties to the lower slot; it replaced a greedy
  nearest-pair-first choice that could leave one unit with a slot in front of the leader's own column),
  the leader keeping its place. Units that came through a gap in another order than they went in would
  otherwise have to pass each other to reach their slots. (Without it the sweep failed 18 runs, against 10.)
* **No swaps** (review round 3). Rounds 1 and 2 also let a stalled member swap slots with the member standing in
  its way. With a lattice snapped from a block that has more units than lattice points in a row, the swaps handed a
  lone slot two rows ahead of the rest from one unit behind to the next (three times in the first 80 ticks of one
  march), each new holder a straggler the leader waited for. Without the swaps the failing runs of the sweep went
  from 15 to 10; the reassignment above does what they were for.

**Formed up again, as first written.** Rounds 1 and 2 had found a unit that came through the passage last, with a
slot in front of the leader's own column, standing up to 3 cells behind its slot until the end, and had loosened the
check "formed up again" of `formation_move` from "every member walks straight to its slot and is within 1 cell of it"
to "within 3 cells, or waiting". With the rules of round 3 the first check holds again (tick 1,370) and the test is
back to it.

### The end

The formation is let go (`released`) when the leader is within `max(extent, d)` cells of the
goal, `extent` being the farthest a member's offset lies from the leader's and `d` the radius of the
disc the bodies of the **whole order** fill (`CrowdState::disc`: the units of other families sent with
it stand there). Or when the leader has come to the end of the way to a goal it cannot reach.
After that every member is an ordinary unit of a group order: it follows its field or route
and **takes an arrival slot** when it comes near (`path/claim.rs`, WP4.4b): the first slot it can
walk to that no slot already taken lies too near, so a slot that is blocked (rock, a structure, a
crowd) is not taken. The lattice of those slots is the class rings of `path/slots.rs`, **turned about
the goal by the heading of the formation** (`Layout::turn`; nothing for a group that did not form up,
so the slots of every other order are exactly as they were): the rows of the arrived group stand across
the way it came. Arrival quality is the one of WP4.4b because the logic is: the claims, the bounds on
the crowd that counts as arrived, the closing up of a settled crowd.

**Review round 3**: the margin of 2 cells is gone. With it a block took its arrival slots while still about
eight cells from the goal, all its units in the same few ticks and in the order of their ids (the claim logic's
order), so the units of one side claimed slots on the other and crossed the block; letting go when the front is
about at the goal took the failing runs of the sweep from 23 to 15. Later was worse (half the extent: 16; one cell:
31). Taking the slots of a formation nearest first instead of in the order of the spiral was tried in `path/claim.rs`
and not kept: it shortened the end of the run it was made for by 52 ticks and left the mean of the sweep where it was.

A formation also ends when fewer than two members are left (`dissolved`): a new order, a stop, a
death takes a unit out at once (`orders::begin` and `clear` reset its column).

## Interplay with the rest

* **Collision** (WP4.4): bodies are as they were. The lattice leaves room (a quarter more than the
  widest body) so that members on their slots do not touch; the crowd pass pushes apart what does
  touch. Direct members do not set `walking`, so the stuck-unit machinery does not apply to them: the
  stall rule above does (they are expelled after the patience, and then it does).
* **Replanning** (WP4.7): the leader is an ordinary unit of its order: if it is stuck it sidesteps, asks
  for a new route round the bodies that stand still, and gives its order up after `replan_max` of them;
  it then leaves the formation (the order ended) and the next unit leads. A member that is expelled
  or released asks for routes as any unit.
* **Flowfields** (WP4.3): the fields are made as before, with one change for classes that are wholly
  formation members (`group_order` in `path.rs`: every unit of the class that starts the order is a member of a
  formation made by it): a class of two or more such units gets a field, below the usual threshold of 8.
  Members of a formation ask for no route of their own while they walk to their slots, so a small class of a
  small formation left to routes scrambled to one point at the arrival (a group of 12 in two columns arrived at
  tick 3,898 against 3,171 with the field). **A unit that forms no formation (an aircraft, a family of one such as
  the amphibious transport among tanks and soldiers, a vessel) does not take the fields away from the classes that
  did** (WP4.6b: until then the rule was "the order is wholly formations", and in the sandbox's order of 12 one
  amphibious transport switched off the fields and the arrival slots of the foot, tracked and wheeled units, which
  were sent to the one point). A class with a unit outside every formation, and a small class of a crowd with a
  formation inside it, are judged by the threshold as ever. A formation that marches without its members
  asking the field for a direction would let it expire after `FIELD_IDLE_TICKS`; the formation touches the
  fields of its members every tick (`Pathing::keep_field`), so they are there when the members go back
  to them.
* **Turning** (WP4.5): a member walks to its slot with the turning of its class. A unit that snaps
  onto a slot faces the heading afterwards by the class's turn on the spot.

## Data

`assets/config/sim.ron`, read by `sf_data::sim_settings`, handed to the world by
`sf_bridge::formation_config` and `World::with_formation`; the simulation's own defaults
(`FormationConfig::DEFAULT`) are the same numbers, which is what tests and frozen scenarios use. A
scenario file can say `formation: Some(false)`, and (for probes above the limit, as the sweep's record does)
`formation_max_units: Some(n)`.

| Setting | Shipped | Range | |
|---|---|---|---|
| `formation` | true | | false moves a group as a group order did before WP4.6 |
| `formation_min_units` | 3 | 2 to `max` | units of one family one order must send |
| `formation_max_units` | 19 | `min` to 256 | an order that sends more is a crowd and makes no formation; 19 is the largest size at which the sweep is clean for every shape and mix |
| `formation_spacing_permille` | 1250 | 1000 to 3000 | lattice cell, of the widest body's spacing |
| `formation_spread_permille` | 1500 | 1000 to 4000 | widest the group may be, of the radius of its disc |
| `formation_pace_permille` | 900 | 500 to 1000 | of the slowest member's speed now, while a member is behind its slot (otherwise the formation goes at that speed) |
| `formation_catchup_permille` | 1000 | pace to 1000 | of its own speed, for a member getting back to its slot |
| `formation_reform_tenths` | 30 | 5 to 100 | how far from its slot a member may be (tenths of a cell) |
| `formation_patience_ticks` | 300 | 20 to 2000 | the leader's patience; a stalled straggler's too |
| `formation_ahead_tenths` | 20 | 5 to `formation_reform_tenths` | how far ahead of its slot (tenths of a cell, along the heading) a member that follows the order's way may get before it goes no faster than the formation (WP4.6b). The sweep is clean with 20 and with 30 (5 was tried under an earlier version of the rules and cost it two runs); the unit test `the_bound_on_how_far_ahead_a_member_may_get_is_data` runs 5 and 30 |

Constants that are not data (`formation.rs`): the open-ground probes (three cells, eight ways, round
the leader and round the front of the lattice), how long a formation stays loose (20 ticks), the window
of the progress check (20 ticks, a 32nd of a cell), how close counts as on the slot (half a cell; also how
far behind its slot a member is before the leader keeps its pace back), how often the slots are handed out
again (100 ticks), the size of the pool (64 formations, 256 members).

## Sizes

`formation_move` is one group (16 units, a block of 4 by 4). A formation that works for that block and
crawls for another works for the test, so `tools/sf_headless/tests/formation_sweep.rs` (`#[ignore]`d, about
half a minute in release for both tests: `cargo test --release -p sf_headless --test formation_sweep --
--ignored --nocapture`) runs the same march on the same map for every size from 3 to `formation_max_units`
(19), for blocks 2, 3, 4, 5, 6 and 8 cells deep along the way (a line abreast to a deep column), and for three
mixes: `mixed` (riflemen, tanks and transports as in `formation_move`), `foot` (riflemen only) and `wheels`
(tanks and transports only). 273 runs, each with the formation and again as the group order the same units
make with `formation: false`. It asserts **for every run up to the limit, every depth and every mix** (nothing
there is only printed):

* **the 1.4 bound** of `formation_move`: every unit idle within 1.4 times the time the slowest alone needs to
  walk from the middle of the block to the goal;
* **not much slower than the group order**: idle within 1.3 times the tick the same group is idle without a
  formation (review round 3; a group of tanks has a walk of 920 ticks, and a delay of a few hundred at the
  passage that hides inside 1.4 times a rifleman's walk does not hide inside a tank's);
* **no crawl**: in no window of 600 ticks, while the rear of the group is short of the goal area, does the
  unit farthest west advance by less than a quarter of what the slowest would walk in it.

| Mix | Size | Arrival, times the walk of the slowest | Without a formation | Times the group order | Rear in a window (share of the slowest's walk) |
|---|---|---|---|---|---|
| `mixed` | 3 to 19 | 1.02 to 1.14 | 1.01 to 1.05 | 1.00 to 1.09 | 0.72 and more |
| `foot` | 3 to 19 | 1.01 to 1.05 | 1.01 to 1.05 | 0.99 to 1.02 | 0.88 and more |
| `wheels` | 3 to 19 | 1.02 to 1.37 | 1.04 to 1.84 | 0.62 to 1.20 | 0.55 and more |

(WP4.6b moved `mixed` from 1.02 to 1.18 and 1.00 to 1.12, and `wheels` from 0.62 to 1.24 times the group order; the
other columns are what they were. The two rules that cost time in a loose stretch, the pace and the bound on the lead,
cost the sweep one run over the bound at first: a block of 19 vehicles 6 deep at 1.43, on a unit that took 130 ticks
longer to settle at the goal than the rest. The rule for the leader's wait above is what got the sweep clean again.)

So: **from 3 to 19 units, at every depth of block, for all three mixes, all three bounds hold.** The margin of
`wheels` against 1.4 is small, and not only near the limit (the worst run of a size is 1.37 at 7 and at 19, 1.35 at
16): the runs are chaotic, and a change anywhere in the march moves which run is the worst. A group of fast units
can be faster as a formation than as a group order (0.62: nine vehicles in a block 2 deep, which as a group order
jam at the passage for 1.84 times their walk).

**Above the limit.** `sizes_above_the_limit_for_the_record` makes the same march as a formation for 20 to 24
units, with the limit raised for the probe, and asserts nothing: `mixed` takes 1.06 to 1.15 times the walk and
`foot` 1.03 to 1.11, both clean; `wheels` takes 1.23 to 1.55, and **6 of its 30 runs are over 1.4** (22 units in a
block 2 deep at 1.45 is the first; the worst, 24 units in a block 8 deep, at 1.55). None is over 1.33 times its group
order, which itself takes 1.17 to 1.45 times the walk there. What is left is spread over the start of a deep block (the lattice is
wider than the block the units stand in, and the units at the back stand ahead of their slots and walk back to them),
the queue at the passage (chaotic: from 180 ticks faster to 133 slower than the group order in the runs looked at),
the wait behind it and the arrival. No single cause is left, so the limit is 19.

What it costs in time, honestly: within the limit a formation arrives up to 1.09 (`mixed`) and 1.20 (`wheels`)
times later than the same group as a group order, because it keeps the shape: the front waits for the back at
the far end of the passage, and the block arrives together instead of one unit after the other.

## In a base (WP4.6b)

WP4.6 proved formations on a field with one passage and no structures. In the viewer, in the sandbox
(`crates/sf_game/src/sandbox.rs`: a few structures and units of each side on the map `assets/maps/formation`), the
same order looked broken: box-select the 12 mobile Meridian units, one right-click, and the tanks raced ahead at
1.4 to 2 cells a second while the riflemen (0.7) trailed and no shape was to be seen. The cause was not a wiring bug.
The structures are narrow places (the ground is not open within three cells of one), so the formation was loose for
12 to 51 % of the march, and loose meant "own speed"; units that took a longer way round a structure came out ahead of
their slot and stood off their column; and the order had one amphibious transport, a family of one, which switched
off the shared fields and arrival slots of the other eleven. The rules above are the fixes: the pace holds in narrow
places; a unit ahead and off its column does not stand; a unit outside every formation takes nothing from the classes
that formed. **Structures are still narrow places** (a formation that stood round a barracks would hold up everything
behind it); the pace is what survives them.

The test is `crates/sf_game/tests/formation_base.rs`: the world built as the viewer builds it (`playfield`,
`sandbox::schedule`, the viewer's world speed), the twelve mobile Meridian units selected, one `commands::move_command`
to each of four goals, (56, 44), (90, 44), (90, 10) and (100, 40). Measured (debug and release give the same numbers),
before and after:

| goal | formation loose | all members within 3 cells of their slots | members within 3 cells | windows of 20 ticks above 1.2 times the slowest's speed, in loose stretches | last unit idle |
|---|---|---|---|---|---|
| (56, 44) | 12 % → 60 % | 19 % → 18 % | 89 % → 92 % | 44 % → 39 % | tick 825 → 784 |
| (90, 44) | 51 % → 48 % | 9 % → 14 % | 62 % → 59 % | 23 % → 6 % | 2,172 → 2,496 |
| (90, 10) | 35 % → 71 % | 42 % → 38 % | 77 % → 75 % | 49 % → 14 % | 1,689 → 1,397 |
| (100, 40) | 38 % → 38 % | 37 % → 25 % | 70 % → 68 % | 16 % → 9 % | 2,317 → 2,206 |

Over the four goals together the loose stretches have 29 % of their windows above 1.2 times the slowest's speed
before and 14 % after. The peak of a member's speed over twenty ticks is 2.7 to 2.9 times the slowest's both before and
after: a unit behind its slot catches up at its own speed, and one within the bound of its slot runs for a moment,
on purpose. The leader of the sandbox's group is a rifleman, so its pace cannot show a difference there; the leader's
pace in a loose stretch is asserted with a tank for a leader in `tests/formation.rs`
(`a_loose_formation_lets_go_of_its_shape_and_not_of_its_pace`: 0.1000 cells a tick before, 0.0350 after).

**The shape in the sandbox is not good, and the test says what it can say.** The structures keep the formation loose for
40 to 70 % of the march and it forms up again between them; the numbers above are what holds, and the test asserts the
bounds that hold with the shipped settings (half the members near their slots, a tenth of the ticks off loose ground
with every member near, the pace in loose stretches, the shared fields, the arrival). Over ten goals, not four, the
means are:

| | members within 3 cells | all within 3 cells | loose | mean arrival tick |
|---|---|---|---|---|
| before WP4.6b | 71 % | 33 % | 41 % | 1,723 |
| pace, off-column and wait rules, without the shared fields | 77 % | 40 % | 44 % | 1,699 |
| all of WP4.6b | 73 % | 35 % | 54 % | 1,610 |

So the rules that keep the pace and the shape help a little (71 → 77 % of the members near their slots), and the shared
fields of the classes that formed cost some of that back (77 → 73 %) and bring the arrival forward (1,699 → 1,610):
the field makes the members of a class follow the same cell-level directions through the narrow places among the
structures, where single routes spread them, and it gives them the arrival slots of the goal. That is what decision C
asked for (a unit outside every formation must not take the fields away), and it is kept. **What is left is not the
formation's:** two heavy tracked units (the harvester and the siege artillery, 1.5 cells across) are stuck among the
structures of the base for 500 to 1,500 ticks in several runs, with or without a formation (the paths are made on cells of
one body's width, and the gaps the sandbox leaves between structures are one cell), and the leader waits for them until
its patience is gone. Bodies as a part of the routes are a work package of their own; until then a base is a place
where a formation holds its pace and loses its shape.

## State and checksum

Section `G` of the checksum, written only while a formation exists (every world whose move orders
never formed one hashes as before): the serial counter, and for each formation its goal, leader,
heading, the ticks to the next heading step, whole or broken, the leader's patience, how loose it
is, its age, and for each member its slot index, offset, stall ticks, nearest approach and what it did
last tick. What a tick works out from those (slots, the pace, what each member does) is derived and
not hashed. `Layout::turn` of a field is hashed after the rest of the layout and only when it is not
zero. A group order that forms no formation has exactly the checksum it had.

## Measured

`formation_move` (map `assets/maps/formation`, 112 by 48 cells; 16 units: 8 riflemen, 4 light tanks,
4 light transports in a block of 4 by 4, one order to a point 91 cells east and behind a 3-cell wide,
8-cell long passage in a wall; it had 20 units in a block of 4 by 5 until review round 3 set the limit to 19):

| | |
|---|---|
| ticks on open ground (formation whole, leader outside 8 cells of the wall) | 1,852 |
| of them with every member within 3.0 cells of its slot | 1,852 (100 %) |
| farthest any member was from its slot there | 0.93 cells (the lattice is 4.46 cells from the leader) |
| leader's speed in the west field | 0.0349 cells a tick (the rifleman's top speed 0.035; nobody behind, so the pace is the slowest's own) |
| longest the group was along its heading | 5.3 cells |
| loose at the passage | 521 ticks, up to 15 members following the order's way at once |
| formed up again behind the passage (every member walking straight to its slot and within 1 cell of it) | tick 1,356 |
| leader's waits (ticks) / breaks / expelled | 0 / 0 / 0 |
| formation let go | tick 2,597 |
| everybody arrived | tick 2,840 (1.08 times the 2,629 the riflemen need alone); worst overlap during the run 0.043 cells, at the end 0.020 |

(WP4.6b moved the last four rows from 504 loose ticks, tick 1,370, tick 2,586, tick 2,812 and overlap 0.048 / 0.029: the
leader goes at the pace in the loose stretch too, and waits only while the members in their places have open ground.)

`bridge_200` is not changed by WP4.6 (a crowd of 200 forms no formation) and meets the done-when of phase 4
as before: all 200 arrive by tick 3,456, 0 end beyond 1.3 times the packing radius, the worst overlap is 0.190 of
a cell, with the sweep of 112 variants in `docs/progress.md`.

## What a path budget does and does not decide

Before WP4.6 `sf_sim/tests/flowfield.rs` held that the path budget decides *when* a group arrives and never
*where* it ends. That test (`the_budget_decides_when_the_group_arrives_and_never_where`) now runs with
formations off, and the claim is not a rule of this simulation: **with bodies it is already not a guarantee,
and a formation makes it plainer.** The arrival order decides who claims which arrival slot
(`path/claim.rs`), the crowd pass pushes by where the bodies are, and a formation's slots hang from a leader
whose route arrives when the budget lets it. So a different budget can change where units end. This is
**not a determinism problem**: for one budget (a setting, the same for every player and every run) the run is
exact and its checksum is reproducible on both systems, which is all lockstep needs; what the old test
pinned was independence from a tuning number, a wish and not a requirement. It stays asserted for the group
(of 20 soldiers, formations off) that it was written for.

## Not here

* **Attack-move in formation, guard, queued moves**: they keep their behaviour. A formation made from
  an attack-move would have to fight as a group (phase 5).
* **Formations the player chooses** (line, wedge, column): the shape is the group's own, kept. A named
  shape would be a different offset generator; the rest would not change.
* **A formation of more than `formation_max_units`** (19): a crowd. Groups of 20 to 24 tanks and transports miss
  1.4 times their own walk in 8 of 30 runs of the sweep ("Sizes").
* **Other ground than one passage**: the sweep and `formation_move` are one map, an open field, a passage three
  cells wide and another open field. Rock outcrops, a river and its bridge, several gaps and turns have single
  cases in `tests/formation.rs` and no sweep. A base (the sandbox) has `formation_base` (above): four goals, one
  layout, bounds that hold and numbers that are low.
* **Bodies in the routes**: a heavy unit (1.5 cells across) is sent through a one-cell gap between two structures
  because paths and fields are made on cells, gets stuck, sidesteps, asks again; in the sandbox that is what keeps two
  of the twelve from the formation for a long time ("In a base"). Not a formation rule; a movement work package.
* **Aircraft and mixed ground**: aircraft do not form; hovercraft and vessels form their own families but
  their formations have been tested only with three boats, and the hover class not at all
  (`tests/formation.rs`).
* **Rejoining**: a member that left (expelled) does not rejoin; a member whose order was replaced takes
  the new order.
* **Reasons for the speed of the formation in the UI**, a drawn outline of the slots: presentation, not
  here.
