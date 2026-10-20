# Armor and splash (WP5.3)

A hit used to take the weapon's `damage` off the one victim (WP5.1, WP5.2). Since WP5.3 it goes through
two more steps, both data: **what the victim's armor takes of that kind of damage** (the armor table), and
**who else is near the burst** (splash, with a falloff). This file is the rules; the fields of a weapon are
in [`weapons.md`](weapons.md), when and where a projectile goes off is in [`projectiles.md`](projectiles.md).
The idea (a hit is more than a number: the kind of weapon and the kind of target decide) is section 8 and 9 of
`docs/red-alert-analysis.md`, which says its own authors had not read the formulas out, so every number and
rule here is ours (clean-room, B5).

A note on spelling: the repository says `armor` in files, fields and code (`armor_types`, `armor_table`,
`sf_sim::armor`) and has done since `unit_classes.ron` was written in WP3.1; this file is called `armour.md`
after the work package's name for it.

## The armor types

Every unit has one armor type, the `armor` field of its record in `unit_defs.ron` (WP3.1; it is checked
against `unit_classes.ron` when the files load). The set is five names with permanent numbers, in
`unit_classes.ron`:

| kind | name | what wears it |
|---|---|---|
| 0 | `none` | what cannot be damaged: the ore field. It takes nothing from anything (a row of zeros) |
| 1 | `infantry` | every person: riflemen, rocket throwers, engineers, the sniper, civilians |
| 2 | `light` | light vehicles, vessels and small objects: the light tank, the artillery, the trucks, the gunboat, the evidence crate |
| 3 | `heavy` | heavy vehicles: the heavy tank and the harvester |
| 4 | `structure` | every building, the bridge segment, the defences |

Five are enough to tell the weapons of the first set apart (a rifle, a rocket, a shell and a satchel have
four different opinions about them) and a unit that is neither fits the nearest one. A sixth (the
suggested `medium`, for a main battle tank between the light tank and the heavy one) is a data change, not
a Rust one: a record in `armor_types`, a cell for it in every row of the table, and the new `armor:` in the
units that wear it. The numbers are permanent like every other list of `unit_classes.ron`
([`units.md`](units.md)); a removed type leaves a tombstone.

The simulation does not take the armor type from the unit table (`UnitStats` is pinned by earlier
scenarios): the armor table carries the armor type of every unit kind as well, and the bridge fills it
from the converted definitions (`sf_bridge::armor_table`).

## The armor table

For every damage type and every armor type, a whole number of percent: how much of the weapon's damage that
armor takes. It is the `armor_table` of `unit_classes.ron`:

```ron
armor_table: [
    (damage: "bullet", percent: [
        ("none", 0), ("infantry", 100), ("light", 40), ("heavy", 10), ("structure", 20),
    ]),
    ...one row for every damage type
],
```

| Number | Meaning |
|---|---|
| `100` | the armor takes the weapon's damage as it is |
| `0` | the armor is **immune** to that kind of damage: nothing comes off and the minimum of 1 does not apply |
| `1` to `399` | it takes that share |
| `400` | the most: four times the damage (`MAX_ARMOR_PERCENT`, also the simulation's `MAX_PERCENT`) |

The rows are lists of pairs, like `terrain_costs`, so a name written twice is an error and not "the last one
wins".

**The table has to be complete.** When the file has an `armor_table`, every damage type has exactly one row
and every row has exactly one number for every armor type. A new damage type or armor type is therefore not
finished until the table has its row or its cell. The same is checked a second time when the simulation
builds its own table (`ArmorTable::new`).

**The table is optional in the file.** The frozen copies of the definitions that the earlier scenarios are
pinned on (`assets/scenarios/*_defs/`) predate the table and must keep loading, so a file without an
`armor_table` loads and a hit then does the weapon's damage whatever it hits (100 percent). The shipped file
has the table (a test sees to that), and a world is given one with the rest of its weapons.

### What is refused when the files load

Each error names the file and the line, for instance
`assets/config/unit_classes.ron:<line>:<column>: armor_table, row "bullet": "heavy" is 401 percent, which must be 0 to 400`.

* a row for a damage type that does not exist, or a second row for one;
* a damage type with no row (the message says the table has to be complete);
* in a row: an armor type that does not exist, one listed twice, one that is missing;
* a percentage below 0, above 400, or not a whole number (a decimal point is a syntax error: no float ever
  exists between the file and the simulation);
* a weapon whose damage type is not in the table, and a unit kind of the world that the table gives no armor
  type, when the table is handed to the simulation (`WeaponsError`).

`crates/sf_data/tests/armor.rs` has one test for each, checking the message and the line; the
simulation's own checks (`ArmorError`, `WeaponsError`) are tested in `crates/sf_sim/src/armor/tests.rs` and
`crates/sf_sim/tests/armor.rs`.

## What a hit does

`weapons::damage_of` is the one function that turns a detonation (or an instant hit, which is a detonation at
the victim) into damage. It pushes `(victim, damage)` for everyone the hit hurts; the second pass of the tick
sums them per victim and takes them off once ([`weapons.md`](weapons.md#a-tick-of-shooting-in-two-passes)).

### The number, and how it is rounded

```
damage = round( weapon damage * armor percent / 100 * falloff )
```

worked out as **one** exact fraction of integers and rounded **once** (`sf_sim::armor::scaled_damage`):

* The falloff is a fraction `num / den` of two integers (raw fixed-point distances, see below); a direct hit
  has `1 / 1`. No intermediate value is rounded, so a splash on armor does not round twice.
* **To the nearest hit point, a tie going up.** `7 * 50 / 100` is 3 and a half: 4. `90 * 75 / 100` is 67 and a
  half: 68.
* **A hit never rounds down to nothing unless a factor is zero.** If the damage, the armor's percentage and
  the falloff are all above zero, the hit does **at least one hit point**. That is the minimum of 1. A rifle
  of 4 on heavy armor (10 percent) would be 4/10 and round to 0; it does 1. The consequence is a floor under
  the weakest weapons: a pistol never stops hurting a tank, a little. A percentage of **zero** is how data
  says "cannot be hurt by this": the sniper's `precision` is 0 against everything but infantry, and then
  nothing comes off (no minimum), and a splash that reaches a victim at the very edge of its radius, where
  the falloff is zero, does not reach it at all.
* Arithmetic is `u128` internally (a damage of a million at 400 percent and a falloff of 64 cells fits with
  room), the result saturates at `u32::MAX`, and the sum per victim saturates at zero hit points as before.

The shot (`Shot::damage`) still reports the weapon's damage **before** the armor, the number the data says;
`Impact::damage` is what the burst did to its direct victim **after** the armor (zero for a miss, and for a
victim whose armor is immune). Neither is state.

### Splash

A weapon with a `splash_radius` above zero also hurts what is near the place where it goes off. The fields
(all cells, all measured from the **edge** of a body, the way a weapon's range is):

| Field | Meaning | Default |
|---|---|---|
| `splash_radius` | everything whose edge is **nearer** than this to the burst is hurt (an edge exactly on the radius is not) | 0 (no splash) |
| `splash_inner` | inside this the splash does its **full** damage; from there the damage falls in a straight line to nothing at `splash_radius`. Below the radius | 0 (it falls from the start) |
| `friendly_fire` | whether the splash also hurts the units of the shooter's own player | `true` |

The falloff for a victim whose edge is `d` from the burst:

```
d <= splash_inner                  full: 1
splash_inner < d < splash_radius   (splash_radius - d) / (splash_radius - splash_inner)
d >= splash_radius                 nothing: the victim is not touched
```

For the shipped howitzer (radius 1.5, full strength to 0.5): a victim whose edge is 0.5 away takes all,
1 away takes half, 1.25 away a quarter, 1.5 and beyond nothing. The falloff is one straight line; "a few
steps" is not built (it would be a list of radii and percentages in the same place, if a weapon ever wants
it). `splash_inner` makes the plateau, and with an inner radius of zero it is a cone from the middle.

**Where the burst is.** At the point where the detonation happened ([`projectiles.md`](projectiles.md)):
the middle of the victim for an instant hit, the aim point for a shell that lands (`Arrived`), the point of
the step nearest the target for a straight or guided shell that touched it (`Proximity`), and wherever the
projectile was when its fuse ran out (`Fuse`). **All three causes burst** — an explosion explodes whether
or not it found its target. So a shell that lands where a walking target stood and misses it still hurts
whatever is near that place, including the target if it has not walked out of the radius; that is what
makes the near miss of an artillery shell matter.

**Who is hurt.** Every entity that has hit points at the start of the weapon stage (an object that cannot be
damaged, a unit already shot down to zero and an entity that is not made from a unit kind have none, and are
skipped), whose edge is nearer than the radius. The edge of a body is its middle minus its radius: half the
`size` of a unit, the shorter half side of a structure (the radius a range is measured to: `orders.md`). A
structure is therefore treated as a disc of its shorter half side; its far corners are not reached by a
splash that would just reach them. Height and walls do not matter (no line of sight in v1, as for a shell:
[`projectiles.md`](projectiles.md#not-here-and-where-it-goes)).

#### The decisions

* **The direct victim is hurt once, at full strength.** The unit a shot was fired at (when the shot hit it)
  takes the weapon's damage times its armor's percentage, **not** the splash on top: the direct hit is the
  centre of its own splash and is not counted twice. It would otherwise take twice the damage of the data
  at the middle of every burst. Everyone else in the radius takes the falloff. The report says how many
  others were hurt (`Impact::splash_victims`). A victim whose edge is within the radius but that the shell
  did not hit (it walked off, the shell was too slow) is a bystander like any other and takes the falloff.
* **Friendly fire: on, as in a classic RTS, and a flag per weapon.** `friendly_fire` defaults to `true`: the
  splash hurts everyone, the shooter's own units and the shooter included. `friendly_fire: false` spares the
  units of the shooter's own player (the player the shot was fired for, as it was when it was fired), the
  shooter among them. It cannot spare a victim the shooter was told to attack: the direct victim is always
  hurt. There are no alliances in the simulation yet; "own side" is "same player". The flag is only for
  weapons with a splash (the loader refuses it elsewhere, since a direct hit has no side to spare).
* **The shooter is no exception.** A splash that reaches the shooter (a weapon fired at something close,
  or with a radius larger than its range) hurts it like anyone, by the position it had at the start of the
  weapon stage. A projectile in flight does not care whether its shooter is alive, and neither does its burst.
* **The minimum of 1 applies to a splash too.** Strictly inside the radius, with a percentage above zero, a
  victim takes at least one hit point; at the edge, where the falloff is zero, it is not touched.

#### Finding the victims: the spatial index

A splash is a radius query. It uses the **crowd pass's spatial index** (`sf_sim::crowd::Index`, a uniform
grid of buckets filled by a counting sort in slot order), built here from everything that can be hurt and
queried by the rectangle that holds the radius plus the widest body, so the query reads a few buckets and not
the whole world. The index is built only on a tick in which some burst has a splash. Buckets are
four cells wide.

The query is **independent of the order** it finds things in: what it hands back depends on the layout of
the buckets, never on who stands in which slot, and the damage it makes is summed per victim afterwards, so
no outcome depends on the order of the query, of the entities or of the projectiles. A test places some
eighty units over several buckets, across their borders, and compares every one with a plain scan (no index) in
the test itself.

### The two passes still hold

Splash and direct damage of a tick are decided first (every burst reads the hit points and positions of the
start of the weapon stage and changes nothing), then collected, summed per victim and applied once,
saturating at zero ([`weapons.md`](weapons.md#a-tick-of-shooting-in-two-passes)). The consequences, each a
test, played in every spawn order and compared by position (`crates/sf_sim/tests/armor.rs`):

* two bursts that overlap on a victim add up (two grenades on one trooper, the unit between them takes both,
  and a unit that either burst alone would leave standing goes down);
* two shooters that kill each other by splash alone both go down, and both fired on the tick they went down;
* a splash that reaches its own shooter, and a friendly-fire flag that spares it, do the same in any order;
* two bombs that land on the same tick on a corner between four buckets add up.

A victim at zero at the start of the stage is not in a splash (it is neither hurt nor counted); one that
another burst brings to zero on this very tick is, from the same start-of-tick state, like the direct hits
of [`projectiles.md`](projectiles.md#fuse-and-detonation).

## The checksum

The weapon table's digest (the first thing the weapons section `W` hashes, for as long as the world has a
weapon table) covers, since WP5.3, each weapon's `splash_inner` and `friendly_fire` and the whole armor
table with the armor type of every unit kind (and a bit that says whether there is one). Its tag moved from
`sf_sim/weapons/v1` to `v2`. So two peers with different tables differ from the first tick, and a change of
one percentage changes the checksum of every world that has weapons.

**No pinned checksum of an earlier phase moved**: they are the worlds without a weapon table
(`wander_5000`, `command_mix`, `terrain_256`, `units_spawn`, `bridge_200`, `orders`, `terrain_costs`,
`pathing`, `pathing_256`, `crowd_jam`, `turning`, `replan`, `formation_move`). The two pinned scenarios
with weapons were re-pinned, and **measured** (`docs/progress.md`, WP5.3 notes, has the numbers):

* `weapons_reload` moved only because of the new layout of the digest. With the old layout (and the new
  code) the run has the old value at every one of its 30 checkpoints, so the splash of its howitzers
  and the (absent) table changed nothing in it.
* `projectiles` moved for the same reason **and** because its frozen weapon definitions lost
  `splash_radius: 1.5` on the howitzer: with splash real, the shell that lands where its walking scout stood
  would hurt it, and the scenario is the one that shows an arc missing a walker. With the old layout and the
  old code the edited definitions end where the new code does at all 24 checkpoints.

## The shipped placeholders

All invented, for the balance rounds of phase 15, consistent with the 4 m cell. The percent table of
`assets/config/unit_classes.ron` (damage type down, armor type across):

| | none | infantry | light | heavy | structure |
|---|---|---|---|---|---|
| `bullet` (rifle, pistol, carbine, the gun and tower machine guns) | 0 | 100 | 40 | 10 | 20 |
| `precision` (long rifle) | 0 | 100 | 0 | 0 | 0 |
| `rocket` (rocket thrower) | 0 | 40 | 130 | 100 | 70 |
| `cannon` (tank guns, the anti-tank gun, the naval cannon, the emplacement) | 0 | 40 | 100 | 85 | 60 |
| `shell` (howitzer, battery) | 0 | 100 | 75 | 50 | 150 |
| `demolition` (satchel) | 0 | 100 | 100 | 100 | 100 |

The intent: rifles are poor against heavy armor (and nearly useless against buildings), the sniper hurts
infantry and nothing else (and is the case of an immune armor), rockets are good against vehicles and a
waste on people, cannons are for vehicles, artillery is good against structures and soft targets, and a
satchel takes down anything but what takes nothing from anything (its 5000 are far above the 1200 of the
sturdiest building).

Splash: the `howitzer` and the `battery_shell` have a `splash_radius` of 1.5 cells (6 m) with `splash_inner`
0.5 (2 m): a body within 2 m of the burst takes the full shell, and nothing 6 m out. Both hurt everyone
(`friendly_fire` left at its default). Nothing else splashes yet; the bombs of the `armour_splash` scenario
(frozen definitions only) have a radius of 2 cells with full strength to 1, one that hurts everyone
and one with `friendly_fire: false`.

Hit points a shot, with the shipped weapons (to the nearest, minimum 1): rifle (6) 6 on infantry, 2 on a light
vehicle, 1 on a heavy one; rocket (45) 18, 59, 45; cannon (35) 14, 35, 30; howitzer (90) 90, 68, 45; long rifle
(40) 40, 0, 0.

## Tests

* `crates/sf_sim/src/armor/tests.rs`: the table and every refusal (empty, a type twice, an unknown id, a
  pair twice or missing, a percentage above the bound, a unit twice or with an unknown armor), the order of
  the input, the digest, and the arithmetic (rounding to the nearest with a tie up, one rounding for the
  falloff, the minimum of 1, immunity, saturation).
* `crates/sf_sim/tests/armor.rs`: the percentage on direct hits (instant and projectile), the minimum and
  immunity, a world without a table, the hand-worked blast (falloff, armor of the bystanders, a wall, the
  radius itself), the direct victim hurt once, a splash that misses its target, friendly fire as a flag, the
  shooter in its own blast, what cannot be hurt, the spatial query against a plain scan, order independence
  in every spawn order (overlapping splashes, a mutual kill by splash, the shooter in its own blast, bombs
  landing together), what the world refuses, and the checksum.
* `crates/sf_data/tests/armor.rs`: the shipped table (complete, bounded, says what its comments say), a file
  without a table, the frozen copies, every way the table can be wrong with its message and line, and the
  splash fields.
* `crates/sf_bridge`: the table arrives with every percentage and every unit's armor.
* `assets/scenarios/armour_splash.ron` (frozen definitions `assets/scenarios/armour_splash_defs/`, which
  add two bombs and their teams), pinned in `armour_splash.expected` and run by CI on Linux and Windows,
  with `tools/sf_headless/tests/armour_splash.rs`: bullets, rockets and the sniper against heavy, light
  and no armor, a howitzer shell into a crowd by hand calculation, two bombs (one careful), and a bomb
  team in its own blast.

## Not here (and where it goes)

* Steps instead of a straight falloff: a list of radii and percentages in `weapon_defs.ron`, if a weapon
  needs it.
* A splash that stops at a wall, or that reaches the far corners of a large structure: later, with the rest
  of the geometry (a structure is a disc of its shorter half side here).
* Alliances (`friendly_fire` means the same player); kill credit and what a destroyed unit leaves (WP5.5:
  settle credit from the set of the tick's hits, never from `hp_after`).
* Choosing what to shoot at, and splash from a unit's own death (`death_blast` of `unit_defs.ron`): WP5.4 and
  WP5.5.
* Damage to terrain, and the game itself firing (`sf_game` still gives its world no weapon table).
