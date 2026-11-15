# Saving and restoring the world

WP13.1a. The code is `crates/sf_sim/src/save.rs` (format, sections, gates) and
`crates/sf_math/src/bytes.rs` (the byte writer and reader). Tests: `crates/sf_sim/src/save/tests.rs`
and the tests in `bytes.rs`.

## What a save is

A save holds the **dynamic state** of a world, never its rules. The configuration, the terrain, the
unit, movement, weapon and building tables and every setting stay out of the file. To load, the host
builds a fresh tick-0 world from the same match and assets exactly as for a new match, and calls

```rust
World::restore(fresh, &bytes) -> Result<World, SaveError>
```

after `World::save(&self) -> Result<Vec<u8>, SaveError>` made the bytes. `sf_sim` does no I/O: the
file is the host's (WP13.1e). Why: the tables are fixed when the world is made, untrusted bytes never
install rules, and a replay (WP13.2) needs the same header. The price: a save made with other assets
does not load (`SaveError::Rules`); migration is WP13.3.

The test of a save is `restore(fresh, save(w)) == w` by `PartialEq` (with the reports of the last
tick, the stats, the interpolation columns and the checksum history put equal on both sides, see
"Derived" below), a resave that is byte for byte the first save, and a restored world whose
checksums tick for tick equal an uninterrupted run.

## Format

Everything is little-endian and fixed width. A `usize` (a length, a count) is written as a `u32`;
one that does not fit is an error, so the bytes are the same on every platform.

Header:

| Bytes | Field |
|---|---|
| 4 | magic `SFSV` |
| 4 | `format_version`, `u32`, now **1** |
| 8 | `rules_digest`, `u64` (`World::rules_digest`) |
| 8 | `tick`, `u64`: the number of the next tick to run |
| 4 | the number of sections, `u32`, now 3 |

Then the sections, each a tag byte and a `u32` length, in this order:

* `W` world scalars: `spawn_serial` (`u64`), the free list (count, then each slot index in stack
  order, the last freed last), the number of occupied cells (a corruption check, see below).
* `E` entities: the slot count, then per slot its generation and a live flag; a live slot then has
  kind, owner, position (two raw `Fx`), facing, behaviour tag, target (flag + point), wander area (two
  points, as stored), random stream (state and increment), anchor cell, unit-table position (`def`),
  hit points, order, the queued orders (count + orders), `guard_walking`, turret facing, turret
  target (flag + id) and the whole crowd memory (every field of `CrowdState`). A free slot holds the
  neutral values `Entities::clear` writes, so only its generation is written.
* `Q` command queue: every pending command with its `(tick, player, sequence)` key, then every
  player's next sequence number.

Order tags are the checksum's (0 idle, 1 move, 2 attack, 3 attack-move, 4 guard point, 5 guard
entity, 6 scatter, 7 hold, 8 pursue). Command tags: 0 `Spawn`, 1 `Place`, 2 `SpawnUnit`,
3 `PlaceStructure`, 4 `BuildStructure`, 5 `QueueUnit`, 6 `CancelUnit`, 7 `SetRally`, 8 `SetRepeat`,
9 `Repair`, 10 `Sell`, 11 `Order`, 12 `Stop`, 13 `AimTurret`, 14 `TakeOre`, 15 `Despawn`,
16 `SetWanderArea`, 17 `MoveTo`; the fields follow in declaration order. The binary command
encoding (`save::write_command` / `read_command`) is public: the replay of WP13.2 and the command log
of WP13.1e use it. All tags are pinned by `command_tags_are_pinned` and `the_header_is_pinned`.

## The rules digest

`World::rules_digest()` hashes (FNV-1a 64) the configuration (seed, size, speed, turn rate), the
terrain (whether it has a layer and its digest), the unit-table digests (stats, reach, turret), the
movement table, the weapon table, the building table and its placement digest, who is which
faction, the collision and formation settings, the path budget, the flowfield threshold and the
replan settings, the targeting and destruction settings (or that there are none), and the numbers of
every optional rule set a world that saves may have: the ore (its rules and the start amount of every
field cell, through the ore fingerprint, which at `version == 0` is the start), the money rules
(`credits_per_ore`) and the repair and sale rules (all four numbers). Of the other optional rule sets
(harvest, production, power, vision, match) only whether the world has them is hashed: a world with
any of them does not save yet, and their numbers join in WP13.1c; the AI profile digests join in
WP13.1d. A different digest on restore is `SaveError::Rules` (`other_assets_or_settings_are_refused`:
other hit points in the unit table, another collision setting, another seed;
`other_ore_money_or_repair_numbers_are_other_rules`: another ore start amount, another
`credits_per_ore`, another sale time).

## Errors

`SaveError`: `Truncated` (the bytes end early), `BadMagic`, `Version(found)`, `Rules`,
`Corrupt(what)`, `Unsupported(section)`, `NotFresh` (the world given to `restore` has run a tick, or
has entities or commands). The reader (`sf_math::bytes::ByteReader`) returns an error on every short
or bad input and never panics; a count is checked against the bytes that are left before anything
is reserved, so a forged length cannot make it allocate more than the file holds. `restore` also
refuses, as `Corrupt`:

* a tick of `u64::MAX` (the counter could not advance);
* an entity outside the map (`0 <= x <= width`, `0 <= y <= height`), a walk target, a wander-area
  corner or an order point (the current order and every queued one) outside it, an anchor cell
  outside it, a `def` that is not `NO_DEF` and not a table entry of the entity's kind, more hit points
  than the kind has (zero for an entity without a table entry), a table structure without an anchor
  cell or a table entry that is not a structure with one, an entity that cannot move (a structure, an
  object) with a behaviour other than idle or an order, an unknown behaviour, order or command tag,
  a queue longer than its ring, a random stream with an even increment;
* a free list with a slot that is live, out of range or listed twice, and a free slot that is not
  on the list without being retired (generation `u32::MAX`);
* a command queue out of key order (or with a key twice), sequence counters out of player order, a
  command for a past tick, with the reserved number `u64::MAX`, or numbered
  at or past its player's counter;
* two structures on one cell, a footprint off the map, and an occupied-cell count that is not the
  one the file gives;
* a wrong section count, a section with another tag, a section with bytes left over, and bytes after
  the last section.

## Derived, rebuilt on restore

* `live` (the number of live slots): counted from the live flags.
* `prev_pos`, `prev_facing`, `prev_turret`: set equal to the current values. They exist for render
  interpolation. The movement stage of the next step (`behavior::advance_all`) sets them to the
  current values for every live entity before anything reads them: the crowd pass reads `prev_pos`
  after that stage (`crowd.rs`, the step of the tick and the position before it), the formation pass
  too (formations do not save yet). The continuation test with the crowd pass on shows it.
* The occupancy grid: every live structure claims its footprint again, in slot order (the anchor
  is the `cell` column, the footprint the table entry's or one cell for a legacy `Place`). The count
  in section `W` is compared afterwards.
* The flowfield clock: it goes up by one at the start of every step, so in a world without fields it
  is the tick modulo 2^32.
* The checksum history is not saved. A restored world starts a new history at its tick:
  `World::checksum_history_start()` says which (zero for a world that ran from the start), and
  `checksum_after(tick)` counts from it. The history was never hashed, so this changes no checksum
  (`tests/golden.rs` and every `assets/scenarios/*.expected` are unchanged).
* The reports of the last tick (shots, impacts, attacks, deaths, completions, stalled builds,
  placement and repair refusals) and the stats of the path, crowd and formation passes start empty;
  they are replaced by the next step and are not hashed.

`def` is saved although it looks derived: a legacy `Spawn` or `Place` entity has `NO_DEF` whatever
its kind, so the kind alone does not say which.

## Not saved yet: the gates

Sections of WP13.1b (pathing, flowfields, formations, turning) and WP13.1c (weapons, projectiles,
targeting, destruction, buildings, economy, production, power, repair, vision, outcome) are not in
the file. `World::save` refuses with `SaveError::Unsupported(section)` every world in which one of
them holds anything that is not its tick-0 state, so a half-saved world can never load silently.
The exact test per section (checked in this order, the first one that fails names the section):

| Section | `save` refuses when |
|---|---|
| `pathing` | `Pathing` has a job, a non-zero epoch, a non-zero sector epoch, a non-zero ticket or start slot, or the scan flag; or a live or free slot has a route that is not idle |
| `flowfields` | the field pool is not equal to a new pool with the same threshold and the clock at the tick |
| `formations` | a formation exists, the serial counter moved, or a slot has a formation membership |
| `projectiles` | the projectile store is not empty and never used (`Projectiles::default()`) |
| `weapons` | a slot is reloading or in a burst, or the last tick left shots, impacts or attacks |
| `destruction` | a wreck, a tally or a death report exists, or a unit has kills |
| `economy` | the ore changed since the start (`version != 0`), the world has harvest rules, or money was spent |
| `production`, `power`, `vision`, `outcome` | the world has those rules at all (even at tick 0: their state starts moving in the first step; WP13.1c lifts this) |
| `turning` | a unit has an unfinished arc turn (`turn_carry != 0`) |
| `targeting` | a unit remembers an engagement (`Engage` not at rest) |
| `buildings` | a structure is under construction (`build_left != 0`) |
| `repair` | a structure is repairing or being sold |

The subsystem gates come first; then, slot by slot, the column gates (the last four rows, and the
route, membership, weapon and kills columns of the rows above). Only their state is gated, not the
presence of the targeting, destruction, building and repair rules; a world with a weapon table is
not tested here yet (WP13.1c).

Not checked on restore, on purpose: an owner against a player count (a world without match or
placement rules has no player count; nothing outside the gated sections indexes by owner), and a
live slot whose generation is `u32::MAX` (legal: its death retires the slot through `checked_add`
instead of putting it on the free list, so nothing overflows). The bit-flip test restores every
single-bit change of a save and steps each world that restores four ticks; none panics
(`every_truncation_is_an_error_and_no_flip_panics`). Every `Corrupt` check above has a case in
`every_impossible_world_is_corrupt`, every gate one in `a_section_that_is_not_saved_yet_refuses_the_save`.

**Crowd is saved in this WP**, not in 13.1b as the split planned: its whole state is the per-entity
`CrowdState` (the `Crowd` struct holds settings, shapes derived from the tables, scratch and stats),
and even a world with the crowd pass switched off changes it in the first tick of a unit, so without
it no world with a unit would save at all. `the_crowd_memory_round_trips_with_the_crowd_pass_on`
pins it.

## Guards against a forgotten field

* `World::save` destructures `World` without `..`; every field is bound by name or as `_` with a
  comment that says why it is rules, derived, a report or gated. `save_entities` does the same for
  the entity columns, `CommandQueue::parts`, `Pathing::unsaved_section`, `Formations::at_start` and
  `put_crowd` for theirs. A field added to any of them is a compile error until it is handled here
  (shown in the PR of WP13.1a: a field `pub(crate) probe: u8` on `World` gives `error[E0027]:
  pattern does not mention field `probe``).
* `restore(fresh, save(w)) == w` by `PartialEq`, and a resave byte-equal to the save.
* Per-tick checksums after a restore equal an uninterrupted run
  (`a_restored_world_continues_like_the_original`, saves after 1, 7, 33 and 97 ticks, with and
  without the crowd pass).

## The test world

No CI scenario uses only what this WP saves (each has a unit table with crowd, pathing, weapons or
economy state behind it), so the tests build their own: a 64x48 open map with five wanderers, four
units (two with a turret) on move, guard and queued orders, a legacy structure and a 2x2 structure,
three deaths with a slot reused, and pending commands of every variant from three players, one with
a sender's sequence number. WP13.1e runs the CI scenarios through save and load once every section
saves.
