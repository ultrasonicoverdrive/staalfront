# Destruction (WP5.5)

Until WP5.5 a unit or structure shot to zero hit points stayed in the world: it held where it fell, was nobody's
target and fired no more ([`weapons.md`](weapons.md), [`targeting.md`](targeting.md)). Since WP5.5 a world that is
given the **destruction settings** removes it, frees its cells, may leave a wreck or rubble, and settles who is
credited with the kill. The rules and every number are ours (clean-room, B5); `docs/red-alert-analysis.md` only
says that a destroyed object leaves the map and may leave debris.

**It is opt-in per world**, like targeting. `World::with_destruction(config)` turns it on,
`sf_bridge::destruction_config` makes the config from `sim.ron`, and the headless runner gives it to a scenario
that says `destruction: true` (which needs `combat: true`). A world that is not given one behaves exactly as it
did: a unit at zero stays where it fell, nothing about destruction is hashed, and every checksum that was pinned
before is what it was (below). `sf_game` gives its world the settings (below, "The game").

## The settings (data)

`assets/config/sim.ron`, read by `sf_data::sim_settings`:

| Field | Meaning | Bounds | Shipped |
|---|---|---|---|
| `wreck_ticks` | how long a vehicle wreck lies where the vehicle was destroyed (20 ticks are a second) | 1 to 72000 | 600 (30 s) |
| `rubble_ticks` | how long the rubble of a structure lies where it stood | 1 to 72000 | 1200 (a minute) |
| `wreck_min_size_tenths` | the smallest body, in tenths of a cell across (`size` of `unit_defs.ron`), of a mobile unit that leaves a wreck; 0 means every mobile unit does | 0 to 80 | 10 (one cell) |

`DestructionConfig::DEFAULT` is the same three numbers; a test keeps them equal. Worlds, tests and scenarios that
are not given settings use the defaults. Which units leave a wreck is the one rule that is not a class of its own:
the simulation has no "vehicle" category (the unit table knows mobile, structure and object), so a mobile unit
leaves a wreck when its body is at least `wreck_min_size_tenths` across. In the shipped data that is every
vehicle (1.2 to 1.5 cells) and no infantryman (0.4 to 0.6). Objects (a crate, an ore field) have zero hit points
and are never destroyed.

## Death: one point in the tick

Death happens at **one defined point of the tick**: `World::reap`, right after the weapon stage and before the
return fire. The weapon stage keeps the two-pass rule ([`projectiles.md`](projectiles.md)): everything is decided
from the state at the start of the tick, then all the damage of the tick is applied at once, summed per victim
and saturating at zero. Nothing in a tick has seen a death, so two units that shoot each other to zero on the same
tick both fire and both die (test `damage_of_the_whole_tick_is_in_before_anyone_dies_...`). The stage:

1. removes the wrecks that are due (a wreck made on tick *t* lasts `wreck_ticks` ticks: it is in the world after
   ticks *t* to *t* + N - 1 and gone after tick *t* + N);
2. finds the dead: every entity made from the unit table whose kind has hit points and whose hit points are now
   zero, in slot order;
3. works out, from the state as it stands (nobody is removed yet), what each one was, where, and who is credited
   (below);
4. counts the credit, leaves the wrecks, and only then removes the dead with the same `despawn` a `Despawn`
   command uses: the slot's generation goes up (every id issued for it is stale), a structure frees all its cells
   in the same call, and the routes and flowfields over those cells are rebuilt;
5. replaces the list of the tick's deaths (`World::deaths`, `Snapshot::deaths`).

### Zero hit points no longer mean two things

The WP5.1 follow-up: zero hit points meant both "cannot be damaged" (a kind with no hit points) and "shot
down". In a world with destruction an entity at zero is **always** the first (a crate, a legacy entity that was
not made from the table); a shot-down entity does not exist any more, so no flag is needed. A world without
destruction still has the old reading and the old workaround (`Targeting` holds a unit at zero where it fell).

### What pointed at the dead

All of it already ended cleanly for a despawned entity, because ids are generation-checked; WP5.5 adds tests
that it does for a destroyed one:

* an `Attack` order, a `Pursue` engagement, a `Guard` of an entity, a turret aim, a queued attack: the target is
  no longer a live entity, so the order ends (the unit goes idle or begins the order it had) and nothing is fired;
* a command that names the dead id is ignored;
* a projectile in the air that was aimed at it: a guided one goes on to the last place it saw, a straight or
  arcing one to where it was aimed, and goes off there with no victim (`ImpactCause::Arrived`); it was already
  so for a `Despawn`;
* a formation loses the member and goes on with the rest;
* a shell that outlives its gunner still counts for the gunner's side (below).

## Cells

A destroyed structure frees **every cell of its footprint** (`occupied_cells()` goes down by the footprint, the
cells can be built on at once), and the pathfinding graphs of the sectors they lie in are rebuilt (the epoch
changes, which makes walking units ask for a new route and a flowfield through the area recompute). Several
structures that fall on the same tick give the same result in every order (test
`structures_that_fall_on_the_same_tick_free_the_same_cells_in_every_spawn_order`).

## Wrecks and rubble

What dies may leave a **`Wreck`** (`World::wrecks`, `Snapshot::wrecks`):

| Dead | Leaves | Size |
|---|---|---|
| a mobile unit whose body is at least `wreck_min_size_tenths` across | a vehicle wreck (`WreckKind::Vehicle`), lying for `wreck_ticks`, with its facing | the diameter of the unit |
| a structure | rubble (`WreckKind::Rubble`), lying for `rubble_ticks` | its footprint and anchor cell |
| a smaller mobile unit (infantry) | nothing | |

**A wreck blocks nothing.** Decision: a wreck or a pile of rubble is a record in the world, not an entity and not
an occupant of a cell. It has no slot and no id, takes no order, cannot be shot, does not push anyone in the
crowd pass, and a unit walks straight over it; the rubble's cells are free the moment the structure falls.
Reasons: it is the simplest rule that cannot trap anyone (a field of wrecks after a big fight must not become a
maze), it keeps the pathfinding graphs as they are, and it costs nothing per tick. The price is that a unit can
stand on top of a wreck, which is what the picture shows (the models are low). A slowing or blocking wreck would
be a data field of the settings and a use of the occupancy; nothing needs it yet.

**A building clears what lies under it.** `place_footprint` removes the rubble that overlaps the new footprint
and the wrecks whose middle is in it, so a base can be rebuilt on its own ruins without the old picture showing
through.

**Order.** The wrecks are kept sorted by a total key (where they lie, y then x, then kind, unit, owner, born,
expires), not in the order the dead were found in (the slot order), so nothing about the list depends on which
slot a unit had. The same holds for the list of a tick's deaths.

**They expire.** After the number of ticks of the settings (`Wreck::expires`, `ticks_left`): the wreck simply
leaves the list; the presentation may fade it over its last ticks (`WreckView::ticks_left` counts the states of
the world it is still in, the last being 1).

## Kill credit

Recorded with the death (`Death::killer`, `Death::assists`), counted per player (`World::tally_of`: kills,
assists, losses) and per unit (`World::kills_of`), all of it **state**, hashed.

The rule, which does not read `Shot::hp_after`, `Impact::hp_after` or the order of the shots (the WP5.1 and
WP5.4 follow-ups: that order follows the slots and would give the kill to whoever has the lowest slot):

* The weapon stage notes every piece of damage with its shooter (`Hurt`), once, whatever hit it came from
  (an instant shot, a projectile, a splash). For a victim that died this tick the pieces are summed **per
  shooter**.
* Only damage of an **enemy** counts (another owner). Damage of the victim's own side, and of the victim itself
  (a blast that reaches its shooter), is no one's kill: a victim that only its own side hurt has no killer and
  scores nobody a kill.
* The shooter with the **most damage** in that tick is the killer; the others are assists, in the same
  order. Ties are broken by where the shooter stood, `(y, x)` (north first, then west), then by player, and only
  then by the shooter's id, which can decide only between two shooters of one player at exactly the same spot
  that did exactly the same damage. A shooter that is gone (a shell that outlived its gunner) has no place and
  comes after those that have one; it still scores for its player.
* The killer's player gets a kill, each assist's player an assist, the victim's player a loss. The killer's
  unit gets one kill in its own count if it is still in the world (a unit that dies on the same tick keeps its
  credit with its player but loses its own count with its slot): that is the number a later veterancy rule reads.

Only the killing tick's damage counts. A unit that took 90 percent of a victim's hit points on earlier ticks
gets nothing if another finishes it. That is the brief's rule and the simplest; an accumulated share (damage
per attacker kept in the victim until it dies, or until it has not been hurt for a while) is a column and a
hash section, and the first rule that needs it (veterancy, score) can ask for it.

## Effects hook

`Snapshot::deaths()` lists, for the last tick, each death with its tick, the victim's (stale) id, kind, owner,
class (mobile or structure), position, facing, size (a diameter) and footprint, what it left (`wreck`), and
the killer and assists. WP5.6 draws the explosions from it. It is a report: replaced by every step, not hashed.
`StepReport::deaths` counts them. In the simulation the list is complete per tick. `sf_game`'s
`SimSnapshot::deaths` is not: the sim thread runs up to `1 + max_catch_up` steps in one pass and publishes one
snapshot after them, so it carries only the last step's deaths, and the deaths of the earlier steps of a
catch-up pass reach no snapshot at all; reading every snapshot does not help. Lossless delivery to the
presentation is WP5.6a's effects event queue in `sf_game`, filled in the sim thread right after each
`world.step()`.

## The checksum

A new section `X` is written **only in a world with destruction settings**:
the settings, every wreck (kind, unit, owner, place, facing, size, footprint and anchor, born, expires), the
tally of each player, and the kills of each live unit. It comes after the targeting section and before the buildings section `B`. The deaths of the last tick are not hashed. A world
without the settings hashes as before. Tests: `destruction::tests::every_part_of_the_state_is_in_the_checksum`
and `..._report_of_the_last_tick_is_not_in_the_checksum`.

## Order independence

Everything in a tick that has to choose between units is decided by position, not by slot: the dead are found
in slot order but the wrecks and the list of deaths are sorted by place, the killer is chosen by damage and
place, and the structures that fall together give the same cells in every order. Permutation tests spawn the
cast in every order and compare the outcome by starting place: a fight with deaths (5 units, 120 orders), equal
damage from three shooters (24 orders), two walls that fall together (720 orders). The known exception from
WP5.4 stays: a mobile unit is born with a random facing drawn from its spawn serial, so these tests use units
that turn in one tick.

## Deferred

1. **Death blasts.** `fuel_depot` has `death_blast: (radius 3, damage 200)` in the definitions; destroying it
   does nothing special yet. A blast would be a hit at the structure's place applied in the same death stage (and
   would have to settle chains of deaths in one tick, in rounds).
2. **Veterancy, score, a kill feed:** the data is there (`kills_of`, `tally_of`, `Death`); nothing reads it.
3. **Damaged and destroyed models of structures** (`_damaged`, `_foundation` in the art pack), slowing rubble,
   wrecks that can be repaired or salvaged, vessel wrecks (a vessel takes the heavy vehicle wreck for now).
4. **Accumulated credit** (above).
5. **Visual effects** (explosions, smoke, a fade of the wreck): WP5.6.

## The game (`sf_game`)

The sandbox world gets the weapon table, the targeting settings and the destruction settings
(`playfield::with_combat`, the numbers of the shipped `sim.ron`), so the units fire and fall in the viewer. The
sandbox used to put the two gatherings six cells apart, which with units that pick their own targets would have
started a battle on the first tick. **In a world that has a weapon table the bases stand ten cells farther out
from the middle (24 instead of 14), so that the gatherings are about 24 cells apart, more than any mobile unit's
reach (the howitzer's 10)**; a world without weapons keeps the old layout, which the tests of formations in a
base are measured on (`formation_base.rs`). The sandbox test `the_sandbox_does_not_attack_itself_once_the_world_has_weapons`
runs a minute of game time on three maps and requires no shot. A fight starts when the player sends units
across, or with the `--fight` option of `sf_game` (every armed mobile unit attack-moves to the other side's
base), which `--shots-tick N` waits for before taking the screenshots of `--shots`.

The wrecks and rubble are drawn from the models of the art sets (the vehicle wrecks `neutral_wreck_light_vehicle`,
`..._medium_...` and `..._heavy_...` of set v3, which has them, and `neutral_rubble_1x1` to `4x5` of set v4): a
vehicle's by its size (in the shipped data up to 1.2 cells light, up to 1.3 medium, the rest heavy), rubble by the nearest footprint
area, scaled to fit the structure's real footprint; a dark primitive of the right size stands in if a model does
not load (with a warning). The model files, the size bounds and the colour are data in
`assets/config/wrecks.ron` (compiled in like `units.ron`, checked when parsed, `sf_game::wreck_looks`). They are drawn in an added colour of the style (`looks::with_wreck_colour`, dark grey,
after the four players': the players' palette in `units.ron` is unchanged), as instances that are not entities:
they have an id no entity has, so a click on one selects nothing. The `fx` nodes of these models are missing (the
checker's finding), so the visual effects stay WP5.6. The screenshots are in `docs/screenshots/wp5.5/`.
