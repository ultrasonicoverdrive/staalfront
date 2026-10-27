# Projectiles (WP5.2)

A weapon whose `projectile` is not `Instant` does not hurt anyone in the tick it fires. It launches a
**projectile**: a small record in a store of its own that flies for some ticks, goes off, and then, and only
then, takes the weapon's damage off the one unit it hit, if it hit one. This file is how the four kinds fly,
when a projectile goes off, what happens when the target moves or dies, and how a tick keeps all of it
independent of the order of the slots. The fields of the weapon are in [`weapons.md`](weapons.md); the idea
(a projectile is a thing with a life of its own, so the shooter may be gone before it lands) is section 8 of
`docs/red-alert-analysis.md`. Everything is in our own words and numbers (clean-room, B5).

## What a projectile is

| It carries | Meaning |
|---|---|
| owner | the id of the shooter. It may be stale by the time the projectile lands; **nothing reads it to decide anything**, so a shell in the air lands when its shooter is shot down or removed |
| player | the shooter's player when it fired (the colour of a tracer; the credit of WP5.5) |
| weapon | the weapon's kind number; the damage, speed and the rest are read from the weapon table when needed |
| target | an entity, or a point on the ground |
| aim | the point it flies to. Fixed for `Straight`, `Arc` and `Falling` (the place the target stood when the shot was fired); for `Guided` the last place its target was seen |
| position, height | where it is on the ground, and how high above it (cells; the height is for the picture, except that a falling shell lands when it reaches the ground) |
| heading | the direction it flies; a guided projectile steers it |
| span | the ground distance from the launch to the aim point (an arc's height follows from how much of it is covered) |
| fuse | the ticks it may still fly. At zero it goes off where it is, hitting nobody |

The previous position and height are kept for interpolation and are not part of the checksum.

## The tick

A projectile launched in tick *T* is in the store at the end of *T* and **first moves in *T* + 1**. So the
flight time of a shot is the number of steps it needs, counted from the tick after the shot, and a projectile
never moves, or goes off, in the tick it was fired. The order inside a tick is the two-pass rule of
[`weapons.md`](weapons.md#a-tick-of-shooting-in-two-passes): first every decision (the projectiles in flight
move and decide whether they go off; the weapons count down and decide whether they fire), reading the hit
points and positions of the start of the weapon stage and changing none, then one pass that applies all the
damage.

## The four kinds

Distances are in cells, speeds in cells per tick (`projectile_speed` of the file over 20, rounded once).

### Straight

Constant speed toward the **aim point**, which is the middle of the target as it stood when the shot was
fired. It does not lead a moving target. Each tick it steps `speed` closer (the last step is clipped to the
aim point) and then asks: did the *path of this step* come within reach of the target? If so it goes off
(**proximity**). The test is on the whole step (the point of the step nearest the target's middle against a
circle), not on where the step ends, so a fast shell cannot jump over a small unit. If the path never touches
the target, the shell goes off at the aim point (**arrived**) and hits nobody.

* The *reach* of a target is its radius (half the `size`, the shorter half side for a structure: the same
  radius a weapon's range is measured to) plus `HIT_SLACK`, a quarter cell, because a shell is not a point.
* So a straight shell **can miss a moving target**: one that has walked out of the line by more than its
  reach by the time the shell passes. With the shipped shells (30 cells a second, 3 to 5 ticks in the air at the
  weapon's range) a unit that walks (at most 2.5 cells a second, an eighth of a cell a tick) is hit; a slow shell
  (a test uses two fifths of a cell a tick over eight cells) is not. A target that walks *into* the path is hit
  sooner.
* Only the target can be hit. A unit that happens to stand in the path is not (no friendly fire, no body
  blocking: neither is a rule of this work package). Armor and splash (WP5.3) are in [`armour.md`](armour.md): the
  target's armor decides how much of the damage it takes, and the shell's burst also hurts what stands near it.
* Flight time: the shell covers the distance less the target's reach, so it is `ceil((distance - reach) / speed)`
  ticks, which is within a tick or two of the plain `distance / speed` (the light tank's shell on a heavy tank at
  the weapon's range: 4 ticks by the formula, 4 by the quotient). The tests assert the formula exactly and the
  quotient within two ticks. At the weapon's range the distance is the range plus the target's radius, so the
  flight is `ceil((range - 1/4) / speed)` whatever the target.

### Arc

The same ground path as a straight shell, but lobbed over everything between, so it cannot touch anything on
the way and goes off **only where it lands**. Its height is a parabola, `4 * peak * t * (span - t) / span^2`
with `t` the ground distance covered and `peak` the weapon's `flight_height`: zero at both ends and `peak` in
the middle, in integers on the fixed-point values with one rounding step. Height is the picture's business;
the flight time follows from the ground distance: `ceil(span / speed)`.

On landing the target is hit if it is alive, can be hurt, and the landing point is within its reach (radius
plus `HIT_SLACK`) of its middle. So an arc **can miss a moving target**, and the miss is the normal thing for a
walking one: the howitzer's shell takes 15 ticks over ten cells, in which a scout (1.3 cells a second) walks
about a cell, more than its reach of 0.45. A target that has only walked a little (less than its reach) is
still hit. Splash (WP5.3) is what makes the near misses of an artillery shell matter: a shell that lands where
the walker stood hurts whatever is within its radius of the landing point.

### Guided

Launched in the direction the launcher *points* (the hull, or the turret for a turret mount: up to
`fire_arc` steps off the target), and from then on, every tick, turned **at most `guide_turn` facing steps**
toward the target's *current* place, then moved `speed` along the new heading. Its path is tested against the
target's reach like a straight shell's, so it goes off by proximity.

* **The target dies.** If the target is gone (removed) or has no hit points left, the projectile gives it up
  and flies on to the **last place it knew**: where the target stood in the last tick in which it was still
  there to be hurt (the tick it was brought to zero, or the tick before it was removed), turning toward it at the
  same limited rate. It goes off there when its step comes within `HIT_SLACK` of the
  point (**arrived**), hitting nobody; a dead unit that happens to be there is not hit either (it has no hit
  points).
* **The target cannot be caught.** A guided projectile has a fuse of `GUIDED_FLIGHT_FACTOR` (2) times the time
  to fly its weapon's range. A runner faster than the projectile, or a point it cannot turn onto (a turning
  circle larger than the distance), ends in a **fuse** detonation, a miss.
* **The turning circle.** A projectile that moves `v` cells a tick and turns `a` steps of 256 a tick flies a
  circle of radius `v * 256 / (2 pi a)`. A rocket that cannot turn inside its own range cannot come round onto
  a target beside its line. See "The rocket" below for the shipped value.

### Falling

Starts right above the aim point, `flight_height` up, and drops `projectile_speed` a tick until it reaches
the ground: `ceil(height / speed)` ticks, whatever the distance to the shooter (the weapon's range is a
matter between the shooter and the target; a bomb is released over its target). The ground position never
changes, so the target is hit if it still stands within its reach of the aim point when the shell lands, and a
unit that walks away during the drop is missed.

## Fuse and detonation

A projectile goes off for exactly one of three reasons (`ImpactCause`):

| Cause | When | Direct victim |
|---|---|---|
| `Proximity` | the path of a straight or guided projectile came within reach of its target | the target |
| `Arrived` | an arc or a falling shell landed; a straight shell reached its aim point without touching its target; a guided shell reached the last known point | the target, if it is alive, can be hurt, and is within reach of the landing point (never for a straight or guided one that arrives: if its target had been near enough it would have gone off by proximity) |
| `Fuse` | the fuse ran out | nobody |

The fuse of a straight or arcing shell is its flight time plus one tick (a backstop it never needs), a falling
shell's the drop plus one, and a guided projectile's twice the time to fly the weapon's range. It counts down
each tick the projectile flies and the projectile goes off, where it is, the tick it reaches zero (arriving
or touching its target in that same tick counts first).

A victim must be **alive and have hit points at the start of the tick** the projectile goes off. A shell that
lands on a unit another shell shot down to zero in an earlier tick finds it at zero and is a miss (the damage
that would be over is not counted); a shell that lands in the same tick as the shot that brings it to zero is
decided from the same start-of-tick hit points and **counts**, whatever the order of the slots.

## What a detonation does (the hook, filled in by WP5.3)

`weapons::damage_of` is the one function that turns a detonation (or an instant hit, which is a detonation
at the target) into damage. Until WP5.2 it pushed `(victim slot, weapon damage)` for the direct victim. Since
WP5.3 it pushes the direct victim's damage through the armor table (`damage_type` times the victim's armor),
and everyone else within `splash_radius` of the detonation point too, with the falloff; nothing around it
changed. It reads the state of the start of the tick only; whatever it pushes is summed per victim
afterwards, so the order it pushes in does not matter. The detonation point (`Hit::at`, `Impact::at`) is
where the splash is centred: the aim point for an arrival, and the point of the step nearest the target for a
proximity hit. A fuse that runs out bursts too, where the projectile is. The rules are in
[`armour.md`](armour.md).

## Order independence

Everything above reads the entities as they were at the start of the weapon stage and writes only the
projectile itself, so the projectiles cannot see each other and the slot a projectile has decides nothing.
The second pass sums the damage per victim (the hits are sorted by victim, a sum is commutative) and takes it
off once, saturating at zero. Tests:

* `crates/sf_sim/tests/projectiles.rs` spawns the same fights in every order (two, three and four units:
  mutual kills, an overkill, a ring, a tangle, two shells landing together on a victim that either would kill,
  overlapping volleys with slots freed and reused) and compares the outcome **by position**, not by slot;
* `crates/sf_sim/src/projectiles/tests.rs` creates six shells in all 720 orders and four states of the free
  list, so that they take different slots, and compares what happens to each target.

Both fail if a hit point is changed between two decisions of a tick (checked by making the change on purpose).

## The store, the checksum and the snapshot

Projectiles live in `Projectiles`: a slot vector, a generation per slot (bumped when the slot is freed), a
free list used as a stack, and iteration in ascending slot order, like the entities but separate from them: a
projectile is not an entity (no hit points, no orders, no cell). A `ProjectileId` is a slot and a generation. A
slot whose generation is used up (`u32::MAX`) is **retired**: it is not put back on the free list, so it is never
reused and its last id stays stale for ever (the rule for entity slots in `World::despawn`).

The store is hashed in checksum section `J`, **written for as long as the world has a weapon table** (and only
then, like the weapons section `W` it follows): the slot count, the free list, and every projectile in flight in
slot order with all it carries but the previous position and height. A world without a weapon table cannot
have a projectile and hashes exactly as before, so no pinned checksum of an earlier phase moved. The shots and
impacts listed for a tick are reports and are not hashed.

`Snapshot::projectiles()` lists the projectiles in flight as `ProjectileView`s (id, weapon, kind, owner,
player, target, aim, previous and current position, previous and current height, heading) for a later work
package to draw. Key a render cache by the `ProjectileId`, not the slot: a slot is reused with another
generation. Nothing is drawn yet.

## Reports and the `hp_after` trap

`Shot` records every pull of a trigger, `Impact` every detonation of a projectile (`World::shots()`,
`World::impacts()`, `StepReport::launched`, `StepReport::impacts`). Both are in the order of the slots, so
**treat the shots and impacts of one tick as a set**.

`hp_after` (on both) is the victim's hit points **at the end of the tick** and, since WP5.2, the same for
every shot and impact at that victim in that tick. (Until WP5.1 it was a running total in the order of the
attackers, which is slot order.) It is a report: **WP5.5's kill credit must not be read from it, or from
the order of the shots**. The credit has to be settled from the set of the tick's hits, with a rule that does
not look at slots (for example: all who hit it in the tick it reached zero share it, or the one with the lowest
player number among them). The unit's `Impact::owner`/`player` are there for it.

## The rocket

WP5.1 left the follow-up that the shipped rocket's turning circle (12 cells a second, 4 steps a tick: radius
about 6.1 cells) was larger than its range of 5 cells. A probe with the real flight (a rocket thrower firing
as soon as its hull is within 8 steps of the target, a target 1, 2, 3 or 5 cells off that crosses the line,
crosses and turns back four ticks later, runs off on a diagonal or walks toward the thrower, 60 world seeds, so
60 launch headings, for each) gave this, hits out of 60 in the worst of those geometries:

| target speed (cells/s) | guide_turn 2 | 3 | 4 (as shipped) | 6 | 8 |
|---|---|---|---|---|---|
| 1.3 (a scout) | 59 | 60 | 60 | 60 | 60 |
| 2.5 (the fastest shipped unit) | 51 | 56 | 60 | 60 | 60 |
| 4.0 (faster than any unit) | 0 | 35 | 48 | 60 | 60 |

So with 4 the rocket already hit everything a shipped unit does: a rocket launched within 8 steps of its
target does not need to turn far. But the turning circle was larger than the range, and a faster target (or a
heavier unit data may bring) was missed. **`guide_turn` is now 8**: a circle of about 3.1 cells, inside the
range, and all 60 launches hit in every geometry up to 4 cells a second. The speed (12) and the range are
unchanged. `crates/sf_data/tests/weapons.rs` checks that every shipped guided weapon has a turning circle inside
its range (with 2 pi taken as 6.28, a hair low, so the check errs the safe way), and
`tools/sf_headless/tests/projectiles.rs` fires the shipped rocket from 24 launch headings at a transport
(2.5 cells a second) that breaks away across the line at every distance from 1 to 5 cells: 120 of 120 hit.

## Data: is `projectile_speed` required?

**For the simulation, yes; for the loader, no.** `WeaponTable::new` refuses a weapon that flies (any kind but
`Instant`) with no speed (`WeaponsError::NoSpeed`) and a falling weapon with no height (`NoHeight`), so a world
cannot be made with a projectile that never moves. `sf_data` still accepts the file without them, because the
frozen copies of the definitions under `assets/scenarios/*_defs/` (five of them) predate the field and are not
to be touched. The only code that builds a table from a frozen copy is the headless runner, and it now builds
it only for a scenario with `combat: true` (the one that gives it to a world), so those copies, comments
included, are as they were and no scenario that does not fire has moved. The shipped file is complete (a test
sees to it). `weapons_defs` (WP5.1) has the speeds already; `projectiles_defs` is a new frozen copy.

## The shipped weapons in flight

| Weapon | Kind | Speed (cells/tick) | Flight at the weapon's range |
|---|---|---|---|
| `cannon`, `twin_cannon`, `naval_cannon` | Straight | 1.5 | 4 ticks (the light tank on a heavy tank is measured) |
| `anti_tank_gun`, `emplacement_gun` | Straight | 1.8 | 4 ticks (by the formula) |
| `rocket` | Guided, turn 8 | 0.6 | 8 ticks |
| `howitzer`, `battery_shell` | Arc, height 3 | 0.7 | 15 ticks at ten cells, 18 at twelve |
| `bomb` (scenario only) | Falling, from 3 | 0.2 | 16 ticks |

## Not here (and where it goes)

* Firing at the ground (not done in WP5.4 either), and a structure that chooses a target: WP6.7, see
  [`targeting.md`](targeting.md) (a projectile already has a
  ground-point target: a guided one whose target died is one).
* Armor, splash, damage types: done in WP5.3 ([`armour.md`](armour.md)).
* A destroyed unit leaving the world, kill credit: done in WP5.5 ([`destruction.md`](destruction.md); the credit
  is settled from the damage of the tick, never from `hp_after`).
* Drawing projectiles, muzzle flashes, explosions: done in WP5.6a ([`effects.md`](effects.md)): the snapshot's
  projectiles are drawn as tracers and the shots and impacts become particles. The flash starts at the real muzzle
  node of the model in WP5.6b.
* Projectiles hitting what is in the way (a wall, a friendly unit), line of sight, terrain height: later.
* `sf_game` has a weapon table since WP5.5; the projectiles are drawn since WP5.6a.

## Tests

* `crates/sf_sim/src/projectiles/tests.rs`: the numbers (rounding up of flight times, the fuse of each kind,
  the parabola), the step that passes through a small target, launching each kind, the store (the free list,
  generations, the hash covering every field), the order independence of projectile slots.
* `crates/sf_sim/tests/projectiles.rs`: the table's refusals, flight times against the data (straight, arc,
  falling), a miss on a walking target for the kinds that can miss, a hit on a target that walks into the path,
  a guided hit on a runner that dodges where a straight shell misses (the same rocket with and without steering),
  the turn cap, the launch heading, the last known point, the fuse, a shooter shot down or removed with a shell
  in the air, shells landing together, a shell on a victim already at zero, a burst, the order independence of
  the fights, the snapshot, the checksum.
* `tools/sf_headless/tests/projectiles.rs`: the `projectiles` scenario, and the shipped weapons.
* `assets/scenarios/projectiles.ron` (frozen definitions `assets/scenarios/projectiles_defs/`, which add the
  `bomb` weapon and the `bomb_team` unit, kind 90, so that every kind of projectile takes part), pinned in
  `projectiles.expected` and run by CI on Linux and Windows.
