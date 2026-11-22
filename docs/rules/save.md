# Saving and restoring the world

WP13.1a, extended by WP13.1b (pathing, flowfields, formations, turning), WP13.1c-1 (weapons,
projectiles, targeting, destruction) and WP13.1c-2 (ore, harvesters, stores and money, construction,
production, power). The code is `crates/sf_sim/src/save.rs` (format, sections,
gates, the weapon and engagement columns), the `save`/`restore` functions next to each struct of
`crates/sf_sim/src/path/`, `formation.rs`, `projectiles.rs` and `destruction.rs`, and
`crates/sf_math/src/bytes.rs` (the byte writer and reader). Tests:
`crates/sf_sim/src/save/tests.rs`, `crates/sf_sim/src/save/path_tests.rs`,
`crates/sf_sim/src/save/combat_tests.rs`, `crates/sf_sim/src/save/economy_tests.rs`,
`tools/sf_headless/tests/save_paths.rs`, `tools/sf_headless/tests/save_combat.rs`,
`tools/sf_headless/tests/save_economy.rs` (all run by `tools/sf_headless/tests/common/saves.rs`)
and the tests in `bytes.rs`.

## What a save is

A save holds the **dynamic state** of a world, never its rules. The configuration, the terrain, the
unit, movement, weapon and building tables and every setting stay out of the file. To load, the host
builds a fresh tick-0 world from the same match and assets exactly as for a new match, and calls

```rust
World::restore(fresh, &bytes) -> Result<World, SaveError>
```

after `World::save(&self) -> Result<Vec<u8>, SaveError>` made the bytes. `sf_sim` does no I/O: the
file is the host's (WP13.1e). Why: the tables are fixed when the world is made, untrusted bytes
never install rules, and a replay (WP13.2) needs the same header. The price: a save made with other
assets does not load (`SaveError::Rules`); migration is WP13.3.

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
| 4 | `format_version`, `u32`, now **4** (1 was WP13.1a, without `P`, `F`, `M` and `prev_pos`; 2 was WP13.1b, without `X`, `D` and the combat columns of `E`; 3 was WP13.1c-1, without `O`, `H`, `C`, `U`, `I` and `build_left`) |
| 8 | `rules_digest`, `u64` (`World::rules_digest`) |
| 8 | `tick`, `u64`: the number of the next tick to run |
| 4 | the number of sections, `u32`, now 13 |

Then the sections, each a tag byte and a `u32` length, in this order:

* `W` world scalars: `spawn_serial` (`u64`), the free list (count, then each slot index in stack
  order, the last freed last), the number of occupied cells (a corruption check, see below).
* `E` entities: the slot count, then per slot its generation and a live flag; a live slot then has
  kind, owner, position (two raw `Fx`), the position before the last step (`prev_pos`, WP13.1b),
  facing, behaviour tag, target (flag + point), wander area
  (two points, as stored), random stream (state and increment), anchor cell, unit-table position
  (`def`), hit points, order, the queued orders (count + orders), `guard_walking`, turret facing,
  turret target (flag + id), the whole crowd memory (every field of `CrowdState`), the turn carry
  (`u16`, thousandths of a step) and the formation memory (every field of `Member`; member modes 0
  none, 1 lead, 2 direct, 3 flow, 4 wait); then (WP13.1c-1) the four weapon slots (each the reload
  count `wait`, `u32`, and the shots left of a burst `burst_left`, `u8`), the engagement memory
  (every field of `Engage`: the order to resume as a flag and an order, `chase`, `anchor`, `held`,
  `rest`, `aim`, `reach`, `cool`) and the kills (`u16`); then (WP13.1c-2) the construction ticks
  left (`build_left`, `u32`). A free slot
  holds the neutral values `Entities::clear` writes, so only its generation is written.
* `Q` command queue: every pending command with its `(tick, player, sequence)` key, then every
  player's next sequence number.
* `X` projectiles (WP13.1c-1): the store whole: the generation of every slot (count, then each),
  every slot (a flag, then for a live one every field of `Projectile`: weapon, owner, player,
  target (0 a point, 1 an entity and its id), aim, position, the position and height of the tick
  before (not hashed, saved so that a restored store is equal by `PartialEq`), height, heading,
  span, fuse), then the free list in stack order. `live` is counted.
* `D` destruction (WP13.1c-1): every wreck in its order (kind 0 vehicle, 1 rubble; unit kind,
  owner, position, facing, size, footprint as a flag and two `u8`, anchor as a flag and two `u32`,
  born, expires), then the tally (count, then player, kills, assists, losses). Before `P`: the
  epoch rule of the path service counts the rubble of the last tick.
* `P` path service (WP13.1b): `epoch`, the sector epochs, `next_ticket`, `next_slot`, `scan`, the
  planner's memory (`Work`: the local search arrays and heap, the abstract search, the start and
  goal links, the leg's cells, the portal path, the raw route being smoothed), the job in flight
  (flag + every field), and the route column: its length (the service looks at the slots below it,
  so the length is state even though `Routes` compares equal whatever its length), then each route
  with its ring and portal path written from the front.
* `F` flowfields: every pool entry whole (identity, layout, region sectors, the `cost` and `dir`
  arrays, slots, holders, the running digest of the integration), the build in flight (`Cur`), the
  builder's memory (`Scratch`, with its local search, corridor search and integration heap) when it
  was made, and the counters and the clock (`next_serial`, `next_born`, `next_order`, `clock`,
  claims and looks left, the look lap, `built`, `nodes_last_tick`). Saved whole because the checksum
  sees the arrays only through the running digest.
* `M` formations: `next_serial`, then every formation (every field).

The sections of WP13.1c-2 follow, each empty in a world without that rule set (the rules digest says
which rule sets the fresh world has, so the reader knows):

* `O` ore: the number of field cells, the amount of each (`u16`, in field order), the `version`
  (`u64`). The running sum is derived (recomputed).
* `H` harvest: the record count, then per slot a bound flag and for a bound harvester `generation`,
  `load`, the phase (the checksum's tag, 0 search ... 10 manual), `wait`, `cell`, `focus`, `bad`,
  `dock` (flag + id), `own` (an order), `tries`, `spot` and the mail (count + replies; always empty,
  see below); then every dock of the last sync (id, owner, point, exit cell as two `i64`, side 0..3,
  bay: 0 free, 1 reserved + who + `left`, 2 unloading + who + `wait`; the queue as ids; the inbox as
  sender and message 0 request, 1 arrived, 2 cancel); then the ore delivered per player and, with
  storage rules, the store and the capacity per player (each a count and `(player, u64)` pairs).
* `C` money: what each player spent (pairs).
* `U` production: every factory record (id, rally as flag + cell, `repeat`, the items as kind and
  free flag, `done`, the status 0 idle, 1 building, 2 waiting for money, 3 waiting for exit, 4
  missing prerequisite + kind, 5 stopped, 6 not standing).
* `I` power: the totals of the last recount (player, made, used).

Order tags are the checksum's (0 idle, 1 move, 2 attack, 3 attack-move, 4 guard point, 5 guard
entity, 6 scatter, 7 hold, 8 pursue). Command tags: 0 `Spawn`, 1 `Place`, 2 `SpawnUnit`, 3
`PlaceStructure`, 4 `BuildStructure`, 5 `QueueUnit`, 6 `CancelUnit`, 7 `SetRally`, 8 `SetRepeat`, 9
`Repair`, 10 `Sell`, 11 `Order`, 12 `Stop`, 13 `AimTurret`, 14 `TakeOre`, 15 `Despawn`, 16
`SetWanderArea`, 17 `MoveTo`; the fields follow in declaration order. The binary command encoding
(`save::write_command` / `read_command`) is public: the replay of WP13.2 and the command log of
WP13.1e use it. All tags are pinned by `command_tags_are_pinned` and `the_header_is_pinned`.

## The rules digest

`World::rules_digest()` hashes (FNV-1a 64) the configuration (seed, size, speed, turn rate), the
terrain (whether it has a layer and its digest), the unit-table digests (stats, reach, turret), the
movement table, the weapon table, the building table and its placement digest, who is which faction,
every collision and formation setting (each struct destructured without `..`, so a new setting is a
compile error until it is hashed), the path budget, the flowfield threshold and the replan settings,
the targeting and destruction settings (or that there are none; both `write_hash` destructure their
settings without `..`, WP13.1c-1), and for each optional rule set a flag whether the world has it
and its numbers (WP13.1c-2, each destructured without `..`): the ore (`Ore::write_rules`: `start`,
`max`, `regrow_amount`, `regrow_period` and the number of field cells; which cells are fields comes
from the terrain digest), the harvest (`Harvest::write_rules`: all 26 numbers of `HarvestRules`,
the harvester kinds, and with storage the capacity table and the price of a unit of ore), the money
(`credits_per_ore`), production (`queue_limit`, `exit_radius`, the build speed `num` and `den`, and
the production table by its stored digest, `ProductionTable::rules_digest`, which `new` takes over
every field of every `UnitBuild`, destructured), power (`shortage_floor_permille`,
`defence_cutoff_permille`) and repair and sale (all four numbers). The building table enters by its
two digests as since WP13.1a; `digest_of` now destructures `BuildingStats` without `..` (the same
bytes, no pin moves). Until WP13.1c-2 the ore entered by its fingerprint (the current amounts, so
only an ore at its start could restore) and harvest, production and power only by their presence;
that is replaced by the numbers above. Of vision and the match only the presence is hashed: a world
with either does not save yet (WP13.1c-3). The starting credits are not rules: they are the stores
at tick 0, state in section `H`. The AI profile digests are
checked by each AI part on its own (WP13.1d, "The AI" below). The weapon table enters through `WeaponTable::rules_digest`, which destructures the
table without `..` and returns its stored digest (nothing is recomputed on save or restore); that
digest is made by `digest_of` when the table is built, which destructures every weapon and loadout
and hashes the armor table's stored digest (the bytes are the ones the checksum always hashed, so no
pin moves). The armor table's `rules_digest` destructures its fields and works the digest out again,
but only a `debug_assert_eq!` against the stored digest uses it. The test
`every_weapon_loadout_and_armor_field_is_in_the_rules_digest` changes each weapon, loadout and armor
field on its own and asserts the digest changes, and asserts the armor work-out equals the stored
digest (so dropping a field fails it in a release build too). A different digest on restore is `SaveError::Rules`
(`other_assets_or_settings_are_refused`: other hit points in the unit table, another collision
setting, another seed; `other_ore_money_or_repair_numbers_are_other_rules`: another ore start
amount, another `credits_per_ore`, another sale time; `other_combat_rules_are_refused`: another
weapon damage or fire arc, another scan cooldown or pursuit reset, another wreck time or wreck
size, a world without targeting or without destruction).

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
  outside it, a `def` that is not `NO_DEF` and not a table entry of the entity's kind, more hit
  points than the kind has (zero for an entity without a table entry), a table structure without an
  anchor cell or a table entry that is not a structure with one, an entity that cannot move (a
  structure, an object) with a behaviour other than idle or an order, an unknown behaviour, order or
  command tag, a current order `Scatter` (`begin` turns it into a move), a `Pursue` without an
  engagement (see *Restore checks of WP13.1c-1*), a queue longer than its ring, a random stream with
  an even increment;
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
* `prev_facing`, `prev_turret`: set equal to the current values. They exist for render
  interpolation; the movement stage and the turret pass set them before anything reads them.
  `prev_pos` was derived the same way in WP13.1a, which was wrong once formations save: the
  formation pass runs **before** the movement stage and reads how far the leader moved
  (`pos - prev_pos`, `formation.rs`). Restoring it as `pos` moved the checksum of `formation_move`
  and `crowd_jam` after the first save (measured: final `0x3b18e217c26aaaa4` and
  `0xd3e4fac5d4184e80` instead of the pins); saving it gives the pins back. It is not hashed, so
  this is state the checksum does not see.
* The path graphs (WP13.1b): every cell of every class graph is read again from the terrain and the
  structures and every sector rebuilt (`Pathing::rebuild_graphs`), as a placement rebuilds the
  sectors it touches; the heuristic floor stays the one the fresh world built at the start, as in
  the saved world. **Proof** (risk 1 of the design memo, portal numbering after incremental
  rebuilds): the restored graphs equal the original ones by `PartialEq` after each of three
  placements made one at a time and after one structure was taken away again, and the fresh
  world's graphs do not (`rebuilt_graphs_equal_the_ones_placements_made`); also at every save tick
  of `a_moving_world_round_trips_at_every_save_tick`, and implicitly in the CI scenarios below
  (a wrong graph diverges the checksum). So the graphs are rebuilt, not saved. The rebuild uses a
  scratch local search, not the saved planner memory, which it would otherwise overwrite.
* `Field::sector_slot`: the places of the field's region sectors, rebuilt from `sectors`.
* The heap places (`IndexedHeap::pos`): rebuilt from the heap order.
* The flowfield spiral (a constant table) and the held-slot grid of a claim (scratch of one call;
  any two compare equal).
* The occupancy grid: every live structure claims its footprint again, in slot order (the anchor
  is the `cell` column, the footprint the table entry's or one cell for a legacy `Place`). The count
  in section `W` is compared afterwards.
* The checksum history is not saved. A restored world starts a new history at its tick:
  `World::checksum_history_start()` says which (zero for a world that ran from the start), and
  `checksum_after(tick)` counts from it. The history was never hashed, so this changes no checksum
  (`tests/golden.rs` and every `assets/scenarios/*.expected` are unchanged).
* The reports of the last tick (shots, impacts, attacks, deaths, completions, stalled builds,
  placement and repair refusals) and the stats of the path, crowd and formation passes start empty;
  they are replaced by the next step and are not hashed. For the four of the weapon stage
  (WP13.1c-1): `weapons::update_all` clears `shots`, `impacts`, `attacks` and `hurt` before it does
  anything else, and nothing in a step reads them before that call (the targeting stage before the
  weapons reads the entities; the return fire after it reads the `attacks` the same call filled;
  `reap` reads the `hurt` of the same call and empties it, so `hurt` is empty between ticks
  anyway). `reap` clears `deaths` first. Only the host reads them between ticks (the presentation,
  `sf_game/src/fx_events.rs`). Shown: `the_battle_restores_after_every_tick` and the CI scenarios
  of `save_combat.rs` restore with the reports empty and go on with the same checksums, with saves
  at ticks where the reports are not empty (claim "reports of the weapon stage").

`def` is saved although it looks derived: a legacy `Spawn` or `Place` entity has `NO_DEF` whatever
its kind, so the kind alone does not say which.

## Not saved yet: the gates

Repair and sale, vision and the outcome are not in the file (WP13.1c-3). `World::save` refuses with `SaveError::Unsupported(section)` every world in which one of
them holds anything that is not its tick-0 state, so a half-saved world can never load silently.
The exact test per section (checked in this order, the first one that fails names the section):

| Section | `save` refuses when |
|---|---|
| `vision`, `outcome` | the world has those rules at all (even at tick 0: their state starts moving in the first step; WP13.1c-3 lifts this) |
| `repair` | a structure is repairing or being sold |

The gates `economy`, `production`, `power` and `buildings` are gone (WP13.1c-2).

The subsystem gates come first; then, slot by slot, the column gates (the last two rows). The
pathing, flowfield, formation and turning gates of WP13.1a are gone (WP13.1b saves those sections),
and so are the projectile, weapon, destruction and targeting gates (WP13.1c-1 saves them; a world
with a weapon table, targeting and destruction saves, `save_combat.rs`). Only the state of the
building and repair columns is gated, not the presence of the building and repair rules.

| Part | WP13.1c-1 |
|---|---|
| weapon slots (`wait`, `burst_left`) | saved, `E` |
| shots, impacts, attacks, `hurt` of the last tick | derived (reports, see above) |
| turret facing and turret target | saved, `E` (since WP13.1a); `prev_turret` derived |
| weapon table, `by_def`, `armor_of_def`, `widest` | rules (digest) and derived from them |
| splash index, targeting scratch | scratch (compare equal whatever they hold) |
| projectiles (generations, rows, free list) | saved, `X`; `live` counted |
| engagement memory (`Engage`) | saved, `E` |
| wrecks, tally | saved, `D` |
| deaths of the last tick | derived (report) |
| kills | saved, `E` |
| targeting and destruction settings | rules (digest) |

Not checked on restore, on purpose: an owner against a player count (a world without match or
placement rules has no player count; nothing outside the gated sections indexes by owner), and a
live slot whose generation is `u32::MAX` (legal: its death retires the slot through `checked_add`
instead of putting it on the free list, so nothing overflows). The bit-flip test restores every
single-bit change of a save and steps each world that restores four ticks; none panics
(`every_truncation_is_an_error_and_no_flip_panics`). Every `Corrupt` check above has a case in
`every_impossible_world_is_corrupt`, every gate one in
`a_section_that_is_not_saved_yet_refuses_the_save`.

**Crowd is saved in this WP**, not in 13.1b as the split planned: its whole state is the per-entity
`CrowdState` (the `Crowd` struct holds settings, shapes derived from the tables, scratch and stats),
and even a world with the crowd pass switched off changes it in the first tick of a unit, so without
it no world with a unit would save at all. `the_crowd_memory_round_trips_with_the_crowd_pass_on`
pins it, and `a_restored_jam_continues_like_the_original` saves in the middle of a jam (three units
ordered one by one into a block of idle units; `crowd::any_state` holds at every save tick) and
compares the checksums of the rest of the run.

`restore` refuses as `Corrupt` crowd memory the simulation cannot reach, because the next step could
overflow it: `quiet < stuck_ticks`, `attempts <= stuck_attempts`, `calm < SHOVE_CALM`, `replans <=
ReplanConfig::max`, `cool <= ReplanConfig::cooldown`, `retarget <= ReplanConfig::retarget_cooldown`,
`home_wait <= HOME_WAIT`, sidestep ticks in `1..=SIDESTEP_TICKS`, `0 <= shoved <= SHOVE_LIMIT +
Fx::EPSILON` (a push is clamped to the room left, and the clamp rounds per component, so the sum can
end one raw unit over), `best >= 0`, `disc >= 0`, `gave_up_at` not after the tick, and `mark`,
`home` and the sidestep spot on the map. The flags, `pressed` (saturating) and the claim (looked up,
never indexed) take any value. `crowd_counters_at_their_maximum_restore_and_step` saves every
counter at its maximum, restores and steps 80 ticks with the crowd pass on, and refuses each one set
one past it.

## Restore checks of WP13.1b

Refused as `Corrupt` (each with a case in `every_impossible_path_or_formation_state_is_corrupt`):

* **Path service**: sector epochs of another count; `next_slot` neither 0 nor below
  `min(route length, slots)` (the service sets it to the slot after one it looked at, modulo that
  count, which only grows); more routes than slots; a route cell (`want`, `goal`, `frontier`, ring,
  overlay) off the map, a portal node out of range, rings or overlays longer than they hold, overlay
  cells not strictly ascending, an unknown route state.
* **Job**: a slot or graph that does not exist, a banned sector out of range or more than `BAN_CAP`,
  a point or cell off the map, `legs > REFINE_LEGS`, `near_stage > 1`, in the smoothing stage a raw
  route that is empty or cursors past it, an unknown mode or stage, and a state the planner never
  saves (below, *Job state*). A job of another epoch than the service's: "a job of another epoch
  than the path service".
* **Job state** (`Job::restore`; cases in `every_impossible_job_state_is_corrupt`, each from a real
  world in the stage it needs). `Pathing::serve` keeps a job only after it yielded, so a saved job
  is one the planner stopped in. Refused: the stage Begin or Finish, or a leg ready (it is used in
  the call that made it): "a job stopped where the planner never yields"; `searching` other than
  exactly in ConnectStart, ConnectGoal, Nearest and Refine (the stages that yield inside a search):
  "a job's search flag does not match its stage"; Abstract or Nearest without the abstract search
  begun, Refine without it unless the job is a Refill, an overlay on a job that is not Fresh,
  `strict` without an overlay: "a job's abstract search was never begun".
  A job with ticket 0: "a job ticket at zero". The ticket counter skips 0 when it wraps
  (`next_ticket_after`), and every route a command writes (`Route::IDLE`, a flowfield member in
  `Pathing::group_order`) has ticket 0. So a route that is not owned by the saved job cannot become
  owned before the next `serve` (commands run first), and skipping the checks of such a job is
  safe. No job is made from a ticket-0 route: `next_job` takes `Wanted` routes (always given a
  ticket) and `Active` routes that are not complete (made by a job, with its ticket); flowfield
  routes are complete. A saved `next_ticket` of 0 stays valid: it is the state before the first
  request.
* **Epoch** (rewritten in WP13.1c-1): only `refresh_sectors` moves the service's epoch, by one per
  call. It is called from the command loop (before the service, which then replans a job of the old
  epoch at once: the budget is at least 512 and the fields take at most half, so the job always
  runs, and `Job::run` answers `Replan` to another epoch before it reads anything) or, after the
  service, from the death stage: `World::reap` despawns every structure the weapons took to zero,
  and `World::despawn` refreshes the cells of each one once (`refresh_cells`). Nothing else after
  the service changes the map. So a job saved after tick T is behind the service by exactly the
  number of structures destroyed in T, and each of them left rubble born on T (a structure always
  leaves rubble, `reap`; only a placement in the commands of a later tick clears rubble). The
  restore counts that rubble (`Destruction::rubble_born(tick - 1)`, which is why section `D` comes
  before `P`) and refuses a job whose distance to the service's epoch (`epoch - job epoch`,
  wrapping) is anything else: "a job of another epoch than the path service". The bound K is the
  rubble count; the reachable distance is exactly K, not a range. A job behind the service is
  replanned by the next service without reading its memory, and its memory was made on a graph the
  restore does not rebuild (the map before the destruction), so **the memory checks below
  (`Job::search_fits`, `Job::memory_fits`) are skipped for it** (`Pathing::check_in_flight`); the
  checks of `Job::restore` (sizes, stages, ticket) still apply. A command before the next service
  can only move the epoch further on, never back to the job's, so such a job never reads its memory.
  Shown by `a_job_behind_the_service_restores_and_replans` (the battle of `combat_tests.rs` saved on
  a tick a structure fell while a route was planned; restored, it runs 120 ticks with the same
  checksums as the original; the service's epoch one more, or the rubble of that tick taken away,
  is refused), by `a_job_two_epochs_behind_restores_and_runs_on` (two structures and more fall on
  that tick, so the job is K >= 2 epochs behind; it restores with K and runs on equal, and one
  rubble of the tick forgotten is refused) and by `the_battle_restores_after_every_tick`, which
  saves after every tick and holds a job behind the service at two save ticks.
  **Why the skip stays without a test that needs it** (fix round 0 of WP13.1c-1): no case was found
  in which a lagging job's memory fails the rebuilt graph (with the skip removed every test still
  passes, also with every structure of the battle falling on that tick). It is kept because nothing
  ties the old memory to the new graph: a structure that falls opens cells, and the portals of the
  sectors it stood in are made again, so a portal the memory names may not exist on the rebuilt
  graph; judging that memory could refuse a save the game wrote, and the job never reads it.
* **Search memory** (the local, abstract and corridor searches, the field builder): arrays of
  another size than the map gives, a heap element out of range, in twice or out of heap order, a
  local parent outside the window or a parent chain that goes round, a local heap element never
  reached, fewer wanted cells left than are marked (the counter is decremented per mark), a node of
  the search going on (`seen == generation`) whose parent is not of it or whose chain goes round, a
  mark from a later search than the current one, a cost above `(nodes + 2) × u32::MAX` (one edge per
  node at most; the sums stay below `u64::MAX`), a corridor sector reached from a node of another
  sector, a walk mark from a later walk, a sector mark other than 0..=3, a wanted cell outside the
  window or more than `SLOTS`.
* **Flowfields**: more entries than `FIELD_POOL`, a graph or cell out of range, region sectors not
  strictly ascending or more than `FIELD_SECTORS`, `cost`/`dir` not the size of the region (or not
  empty without one), a direction other than 0..=8 and `UNSET`, more slots or holders than
  `MAX_SLOTS`, more cells integrated than the region has, a build of an entry that does not exist,
  a build cursor past what its stage walks (slots, sectors, the spiral), a chain node out of range,
  an integration heap element outside the region of the field being built, a serial, born or order
  counter at 0 (they skip it), more claims or looks left than a tick gives, more units found than
  the build looked at (at most the cursor while gathering, the slots after).
* **A search in flight** (checked after the graphs are rebuilt, `Pathing::check_in_flight`): a job
  whose local search is begun and not over (`searching`) goes on with the rectangle its stage
  computes (the start sector, the goal sector, the sector of the nearest portal, or the leg of
  `next_leg`), without beginning it again; a field build searching from its goal goes on in the
  goal's sector. The cells in the local heap and the parent of every cell of that rectangle must lie
  in it. Otherwise a cell number past the rectangle gives a row below it, and on a rectangle at the
  bottom edge of the map the step costs are read past the grid (a panic); refused as "a job's search
  does not fit its stage" / "a field build's search does not fit its stage". `searching` must
  belong to a stage that goes on with a search: a job searching in a stage that runs none (Begin,
  Direct, Abstract, Smooth, Finish, or a Refine leg that pops, steps, arrives or has used up its
  plan) or in a Refine leg whose cells are ready (which skips the search) is refused the same way;
  the planner clears `searching` whenever a search ends, so no valid save has it (the stage rule is
  now in *Job state*; the rectangle rule keeps the Refine legs that need no search). Then the
  memory the job will read (`Job::memory_fits`): once the abstract search is begun, every node of
  the start and goal links and every node the search reached (its heap, parents and nearest node;
  not the goal node) must be a portal of the job's graph, and in ConnectGoal the start links must:
  "the planner's memory names a portal that does not exist" (otherwise `NO_PORTAL` is taken as a
  cell: an index past the grid). The abstract search's nearest node must be one the search going on
  reached (`AbstractSearch::restore`, "an abstract search node or sector"; otherwise `path_to`
  walks parents never checked). Without the abstract search begun a job may carry an earlier job's
  links and reads none of them: those are not judged (a Refill job; `a_refill_job_carries_links_it_never_reads`).
  The job that reached its goal in its own sector (Smooth, not a Refill, no abstract search) must
  smooth a leg that ends its route: no abstract path, not cut short, the raw route ending on the
  route's goal: "a job's leg does not end its route".
  The two checks that read the unit's route (the rectangle of a Refine leg and this one) apply
  only to a job its route still belongs to (`Job::owns`: the route has the job's ticket and is
  Wanted or Active). After the service an arrival (`orders`), a formation (`forget_way`) or a
  stuck replan may reset or re-ticket the route while the job stays; the next service drops that
  job unread (`Job::run`), so its save is valid (`a_job_whose_route_was_reset_restores`). The
  stage rules and the epoch rule apply to every job, the portal checks to every job of the
  service's epoch; they rest on the epoch rule (see *Epoch* above: a job behind the service skips
  them).
* **Formations**: the serial counter at 0, a formation numbered 0 or twice, `turn_wait` at or above
  the longest turn period (40), `patience` above the setting, `loose` above 20 (`LOOSE_TICKS`),
  members not strictly ascending or not slots, a leader that is not a member, a slot whose `form`
  names no formation that lists it, an unknown state or member mode. A listed member whose own `form`
  names no formation (or another) is **not** refused: the formation pass prunes its list before the
  movement stage, and an order that ends afterwards (arrival, give-up) clears the unit's `form`, so
  for one tick the list still names it (found with a save after every tick of `crowd_jam`, tick 219;
  `formation_scenarios_restore_after_every_tick`).
* **Turning**: a turn carry of 1000 or more (`turn_carrying` keeps the rest of a division by 1000).
* `prev_pos` outside the map.

`path_and_formation_counters_at_their_maximum_restore_and_step` restores a world with the turn carry
at 999, every formation counter at its most, a listed member that left, the field builds counted at
`u64::MAX` and a local search's expansions at `u32::MAX`, and steps it 60 ticks. Those two counters
only count up and are now saturating (`built`, `Run::expanded`; the same values below their maximum,
so no checksum moves); before, a restored maximum overflowed on the next increment.

Not checked, on purpose: the wrapping counters (`epoch`, sector epochs, tickets, field clock,
`version`, `born`, the generations), which reach every value; the costs in the search arrays (added
with saturation); flags. **A known gap**: beyond the rectangle above, the memory of a search in
flight is checked for structure, not for being the search its job describes (for example that its
costs are the costs of that search). A hand-made save can therefore make a job plan a strange route.
Single-bit flips of real saves are tried in `no_flip_in_the_path_sections_panics` (every bit
of the first 48 bytes of `P`, `F`, `M`, then one bit in every 509th byte; each restored world steps
four ticks) and none panics; that is a sample, not a proof for every byte string.

## Restore checks of WP13.1c-1

Refused as `Corrupt`; each bound is what the stage that writes the value leaves at the end of a
tick (the writers are named), and each check has a case in `every_impossible_combat_state_is_corrupt`.

* **Weapon slots** ("a weapon slot the weapon stage never leaves"; `weapons::update_all` is the
  only writer besides `Entities::fill`/`clear`, which write rest). A slot the entity's table entry
  has no weapon for (no table entry, an unarmed kind, a slot past the loadout) is at rest. A slot
  with weapon `w`: `burst_left < w.burst` (a shot takes one off at once: `remaining = left - 1`);
  inside a burst (`burst_left > 0`) `wait <= w.burst_delay_ticks`, else `wait <= w.tail_ticks()`
  (the last value set, counted down from the next tick on; a dropped burst sets the tail).
* **Engagement memory** ("an engagement the targeting stage never leaves"; writers: `targeting.rs`
  `engage`, `end_engagement`, `update`, `scan`, `return_fire`, `check`, `fall`, and `Engage::forget`
  from `orders::begin`/`clear`). Without the targeting stage for the entity (no targeting settings,
  no weapon table, an unarmed kind, no table entry, an object) the memory is at rest and the order is
  no `Pursue`. Otherwise `cool <= scan_cooldown` (set to it, counted down), `anchor` and `aim` on the
  map. A mobile unit: its order is a `Pursue` exactly when it has an order to resume (`engage` sets
  both, every other order change `forget`s); the order to resume is one a unit scans from (idle,
  hold, guard, attack-move) with its point on the map; when engaged `reach` is the range of one of
  its weapons (`best_against` gives `w.range`), `held` is set and `rest` is 0 (only a unit that is
  not pursuing counts rest; `engage` and `return_fire` set it to 0); when not engaged `chase` is off
  and `aim` and `reach` are zero (`forget`). `rest < pursuit_reset` (at the setting it starts again
  from 0 and drops `held`), and without `held` `rest` is 0 and the anchor zero. A structure (a
  defence): no order to resume, no chase, not held, `rest` 0, anchor zero, `reach` zero or the range
  of one of its weapons.
* **Kills** ("kills without a weapon or destruction"; "more kills than the tally"). Kills only on an
  armed entity of a world with destruction; per player the kills of the live entities add up to at
  most the player's tally of kills (each kill counts in both, `reap`; a death or a new entity in the
  slot clears the entity's count). A tally at `u32::MAX` is not compared (it saturates). The check
  assumes **an entity never changes owner**: its kills are summed under its owner now, which is the
  player whose tally counted them. Nothing in the simulation changes an owner today; a rule that
  does (capture) must move or clear the kills with it, or this check refuses a real save.
* **Projectiles** ("a projectile the weapon stage never leaves in flight", `projectiles::reachable`;
  writers `launch`, `fly` and the `fly_*` functions, `Projectiles::insert`/`remove`). The weapon is
  in the table and launches one (not instant); a point target only for a guided one (it gives its
  entity up); the aim on the map (where a target stood); `1 <= fuse <=` the fuse it was launched with
  (`fuse_for(w, span)`; at zero it goes off); the span of a falling shell zero, of the others at most
  width + height; the height (and the height of the tick before) zero for a straight or guided one,
  between zero and the flight height for an arc, above zero and at most the flight height for a
  falling one; a falling shell (and its place of the tick before) on its aim point; a straight or
  arcing one within the map grown by `STEP_SLACK` (a 256th of a cell: `with_length` rounds each
  component of a step, so a shell can stand a few raw units past the edge; not measured, the error
  does not add up as each step starts from where the last ended); a guided one within the map grown
  by `speed × (launched fuse - fuse)`, the most it can have flown. The store: generations and rows of
  one length ("projectile slots of two lengths"), the free list with no live slot, no slot out of
  range or twice ("the projectile free list"), every free slot on it unless retired at `u32::MAX`
  ("a free projectile slot is missing from the free list"), an unknown target tag.
* **Wrecks** ("a wreck the death stage never leaves", `destruction::wreck_reachable`; writers
  `reap`, `clear_under`). Only in a world with destruction; born before the saved tick, `expires`
  not before it (`reap` drops it on its `expires` tick), `expires = born + wreck_ticks` (vehicle) or
  `+ rubble_ticks` (rubble); a kind of the unit table: a vehicle wreck of a mobile kind at least
  `wreck_min_size` across, of that kind's size, without footprint and anchor; rubble of a structure
  kind, size zero, facing zero, its footprint the kind's, an anchor with the footprint on the map;
  the position on the map. In key order, equal keys allowed ("the wrecks are out of order");
  an unknown kind tag.
* **Tally** ("a tally out of order, all zero or without destruction"): only with destruction,
  players strictly ascending, no tally of all zero (`tally_of` adds one only to count something).
* **Epoch**: see *Epoch* above.

Saturating now (WP13.1c-1, a behaviour change in sf_sim, R7): the tally counters (`kills`,
`assists`, `losses`, were `+= 1`). Reason: with `+= 1` a restored tally at `u32::MAX` panics in a
debug build and wraps to 0 in a release build, so the two builds would differ; a saturated counter
is the same in both. A real game never gets there (four billion deaths). Below
their maximum the numbers are the same, so no checksum moves (`a_tally_at_its_most_saturates_on_one_more_death`
pins it, in a release build too); a restored tally at `u32::MAX`
steps on (`combat_state_at_its_bounds_restores_and_steps`, which also restores every new bound at
its value: a burst at its delay, a reload at its tail, a cooldown at the setting, `rest` one below
`pursuit_reset`, kills at `u16::MAX`, an arc at its peak with its launch fuse, a guided missile as
far off the map as it could fly, and steps 80 ticks). Not checked, on purpose: the projectile and
engagement ids (looked up, a stale one finds nothing), a projectile's owner, player and heading, a
wreck's owner, a turret target (since WP13.1a). Single-bit flips of a real save:
`no_flip_in_the_combat_sections_panics` (every bit of the first 64 bytes of `X` and `D`, one bit in
every 97th byte of `E`; each restored world steps four ticks). A sample, not a proof.

## WP13.1c-2: economy, buildings, production, power

| Part | WP13.1c-2 |
|---|---|
| ore amounts, `version` | saved, `O`; the running sum derived |
| harvester records (`units`), docks, delivered, stores, capacity | saved, `H` |
| claims on ore cells | derived: rebuilt as the inverse of the bound harvesters' cells (`apply` and `sync` keep them so) |
| `live` (harvester slots of the tick) | derived: the bound slots ascending (only `sync` binds and unbinds, and it lists exactly the slots it binds) |
| harvester mail | saved as a count, always 0 between ticks: `apply` clears the mail of every bound harvester, and `run_dock` only answers bound ones |
| harvest rules, kinds, `harvester_def`, `dock_def`, storage table, `per_def`, price | rules (digest) and derived from them |
| money spent | saved, `C` |
| `build_left` | saved, `E` |
| completions, stalled builds | derived (reports): `advance_construction` clears both first; nothing before it in a step reads them (`refresh_power` reads entities only) |
| placement rejections, production rejections | derived (reports): `World::step` clears them before the commands |
| units put out (`produced`) | derived (report): `advance_production` clears it first; nothing earlier in the step reads it |
| factory records (id, rally, repeat, items, done, status) | saved, `U`; `status` is not hashed and no step reads it before writing it, but it is the readout between ticks, so it is saved |
| power totals | saved, `I`: recounted at the start of every step before anything reads them, but the readout between ticks (`power_of`) and `PartialEq` see the last count, and a plant that fell after it is no longer in a recount |
| unpowered defences | derived inside the step from the totals |
| occupancy | derived: rebuilt from the structures as since WP13.1a (a structure under construction occupies its cells, so the same rebuild covers it) |

**Host side, lost for one tick after a load (for WP13.1e):** the reports above start empty in a
restored world. A host that shows "structure completed", "unit ready", "build stalled: no money" or a
refusal of the last tick before the load does not show it again after the load. Nothing in the
simulation reads them across a tick.

**Restore checks** (each the value the writer can leave, with the writer; each with a case in
`every_economy_bound_restores_and_one_past_it_is_refused`, the value at the bound restoring and
stepping 30 ticks, one past refused with the message):

* Ore: another number of field cells ("another number of ore cells", a byte edit in
  `another_number_of_ore_cells_is_refused`); an amount above `max` ("more ore in a cell than the
  rules allow": `start <= max`, `regrow` stops at `max`, `take` only lowers). The `version` takes any
  value: it now wraps (`Ore::set`, was `+= 1`; a behaviour change in sf_sim with the reason that a
  restored `u64::MAX` overflowed on the next change, panicking in a debug build; below the maximum the
  numbers are the same and the version is not hashed, so no pin moves;
  `an_ore_version_at_its_maximum_restores_and_wraps`).
* Harvesters: more records than entity slots (`sync` grows them to the slot count, which never
  shrinks; a record past it would not be unbound by `sync` and `run_dock` would index the entities
  past their end); `load > capacity` (`decide` takes at most the room left); `wait` above
  `max(retry_period, recheck_period, call_timeout, mine_period - 1)` (every value `decide`,
  `search`, `park`, `go_return`, `dock_walk_failed` and `apply` set; otherwise it counts down);
  `cell`, `focus` or `bad` neither `NO_CELL` nor a field cell (chosen from cells with ore, a
  player's order onto a cell with ore, a claimed cell); `own` with a point off the map (cell centres,
  dock, queue and park points on the map, or the unit's own order, checked in `E`); `tries >
  max_tries` (at `max_tries` the walk is given up or the counter reset; it can stay at `max_tries`
  after a search that parks); `spot > 2 * queue_rows` (`queue_walk`: `1 + 2 * row + side`); a
  non-empty mail; two bound harvesters on one cell ("two harvesters claim one ore cell"); an
  unknown phase.
* Docks: ids not ascending by slot (`sync` builds the list in slot order; `dock_index` searches
  it); an exit cell off the map or a point that is not its centre (`new_dock` clamps the exit onto
  the map); a dock whose refinery still lives in its slot with another owner, point, exit or side
  than `new_dock` gives it (owner and anchor never change; a dock whose refinery died after the
  harvest stage is not compared and is dropped by the next `sync`); `left > grant_timeout` (set to
  it, counted down from the next tick); unloading `wait >= unload_period` (set to `unload_period`
  on arrival and counted down once in the same `run_dock`, or set to `unload_period - 1`); a queue
  with a harvester twice or with the holder (`run_dock` queues only one that is neither); an inbox
  not strictly ascending by sender slot (`apply` sends at most one message per harvester, in slot
  order, and `run_dock` empties it every tick); an unknown side, bay or message.
* Per-player lists (delivered, stores, capacity, spent): players not strictly ascending ("player
  totals out of order"; `add_to`, `pay` insert sorted); power totals the same ("power totals out of
  order", `refresh_power`).
* Construction: `build_left` above the build ticks of the kind's building entry, or non-zero on an
  entity without a table entry or a cell ("a construction the construction stage never leaves";
  `apply_from` sets the scaled `build_ticks` right after `place_structure`, `advance_construction`
  counts down).
* Production: factories not ascending by slot (`get_or_insert`); a rally cell off the map
  (`set_rally`); more items than `queue_limit` (`queue_unit` and the repeat refuse a full queue; the
  free unit of a finished structure goes into the queue of a structure that could take no order
  before it was finished, so it is alone, and `queue_limit >= 1`); a paid item whose kind is not in
  the table, a free item that is not the unit of a `comes_with` pair or not first ("a queue item the
  production stage never holds"); `done` above `build_ticks * 1000` of a paid front item, or non-zero
  on an empty queue or a free front item ("progress the production stage never leaves":
  `advance_production` clamps at the total, and every change of the front item resets it); an
  unknown status.

Not checked, on purpose (looser than reachable, not needed for safety; for 13.1h): that a harvester
in ToOre or Mine has a cell and one in ToDock a dock; a harvester's `generation` against its slot;
that a harvester waits in at most one dock queue; the bay holder and queue entries being harvesters;
`MissingPrerequisite(kind)` naming a building; the free item's kind against the factory's own
`comes_with`; the power totals against the standing structures (they are the count of the start of
the last step, which the restore cannot redo); the stores and capacities against the structures.
None of them is reached by a save the game writes; each only by a hand-made file.

**Coverage record** (lesson 3; `the_coverage_record_is_pinned` in `save_economy.rs`): over the saves
every 97 ticks of the eleven scenarios below, these saved values are non-default at some save tick:
the ore amounts and version; bound and unbound harvester records, load, phases to ore, mine, no
dock, driving, queued, granted and unload, wait, cell, dock, own, spot; docks, their free, reserved
and unloading bays, queues and an `arrived` message in an inbox; delivered, stores, capacity, money
spent; `build_left`; factory records with rally, repeat, items, progress and the statuses idle,
building, waiting for money, waiting for exit; power made and used. Never non-default there: a
harvester's `generation` (every scenario spawns harvesters into fresh slots), `focus`, `bad`,
`tries`, the phases search, park, called and manual at a save tick, the mail (empty by the rule
above), a dock's owner other than 0 and side other than +x (in the eleven scenarios), request and
cancel messages, a free queue item, the statuses missing prerequisite, stopped and not standing.
`fields_no_scenario_reaches_round_trip` (sf_sim) sets every one of them (the mail excepted) and
round-trips the world by `PartialEq` with a byte-equal resave.

**Mutations** (each made in a scratch edit, the named test failing, then undone): listed in the PR
of WP13.1c-2.

## The CI scenarios through saves

`tools/sf_headless/tests/save_paths.rs` runs `bridge_200`, `replan`, `formation_move`, `crowd_jam`,
`turning`, `wander_5000`, `command_mix`, `terrain_256`, `units_spawn`, `orders`, `terrain_costs`,
`pathing` and `pathing_256` with a save every 97 ticks (plus, per claimed state, up to four saves at ticks where it
holds, at least three ticks apart), each restored onto
`Scenario::build_fresh_world` (the rules without any entity or command), the resave byte-equal and
the run continued with the restored world; the final checksum is the committed `.expected` pin. At
the save ticks it asserts the state is there: `bridge_200` a route being planned, a field
being built, fields, routes, turn carries; `formation_move` formations, a field being built, turn
carries, routes; `replan` routes, fields, formations and a stuck unit's request with an overlay;
`crowd_jam` routes, fields and formations; `turning` turn carries. Each claimed state is asserted at
each of its save ticks, in the world and in the restored one (`save_contents` equal). `formation_move`
and `crowd_jam` also run with a save after every one of their first 400 ticks.
`tools/sf_headless/tests/save_economy.rs` (WP13.1c-2) does the same for `placement`,
`construction`, `ore_256`, `harvest`, `storage`, `money`, `production`, `production_repeat`,
`power`, `defences` and `silo`, each with its claims (a structure half built and the reports of
construction or placement; ore taken; harvesters to ore or mining, on their way, unloading, with a
load, in a dock queue, a message in a dock inbox, a bay granted, ore delivered, stores, money
spent; a factory, a unit half built, a rally point, a repeating factory; power totals, a player
short of power, defences without power; and the combat state of `power` and `defences`).
`every_tick_<name>` (eleven tests, `#[ignore]`, release: `cargo test --release -p sf_headless
--test save_economy -- --ignored every_tick`) saves, restores and resaves after every tick of each to
its pin; `silo` only to tick 4400, because from tick 4401 it sells a structure and the save is
refused as `repair` (WP13.1c-3; found by the first run of the probe). On the container (4 cores,
shared with other builds, load 3 to 7) the eleven took 65 minutes side by side: a save and restore
costs tens of milliseconds on these maps (the fresh world is cloned and the path graphs rebuilt), so
`harvest` alone took 38 minutes, `money` 45; the rerun of `silo` to tick 4400 passed in 17 minutes; `the_first_ticks_of_harvest_and_production_restore_after_every_tick`
does the first 300 ticks of `harvest` and `production_repeat` in every build, and in sf_sim
`the_economy_world_restores_after_every_tick` saves after every one of 700 ticks of a world with
all of it (a dock queue, a full queue mid-item, a structure half built, player 0 short of power)
and compares by `PartialEq` and checksum. `probe_which_scenarios_still_refuse` pins the rest:
`match` (vision), `repair_sell` (outcome), `vision` and `vision_targeting` (vision).

`tools/sf_headless/tests/save_combat.rs` (WP13.1c-1) does the same for the combat scenarios
`weapons_reload`, `projectiles`, `armour_splash`, `tanks_vs_infantry` and `destruction`, with the
claims: weapons_reload a reload, a burst, a projectile, reports; projectiles and armour_splash a
projectile, a reload, reports (armour_splash fires no burst at a save tick); tanks_vs_infantry an
engaged unit, engagement memory, a reload, a projectile, wrecks, a tally, kills, reports (its units
walk no routes); destruction a reload, a projectile, wrecks, a tally, kills, reports, routes. The
reports are asserted in the saved world only (they are derived and start empty). The restored
world's `SaveContents` equals the original's apart from the reports.
`every_combat_scenario_restores_after_every_tick` saves, restores and resaves after **every** tick
of all five to their pins (the every-tick probe: a save the game writes must always restore). It is
`#[ignore]`d because the workspace tests run in debug; in release on the container it takes about
30 s for the whole file (`cargo test --release -p sf_headless --test save_combat -- --include-ignored`).
In `sf_sim` itself `the_battle_restores_after_every_tick` does the same in every build, over the
battle of `combat_tests.rs` (a 64x48 walled map with the smallest path budget, every projectile
kind, a burst weapon, units choosing targets, a defence tower, HQs shot down while 18 walkers keep
the path service busy), comparing each restored world by `PartialEq` and every checksum with an
uninterrupted run.

## Guards against a forgotten field

* `World::save` destructures `World` without `..`; every field is bound by name or as `_` with a
  comment that says why it is rules, derived, a report or gated. `save_entities` does the same for
  the entity columns, `CommandQueue::parts`, `Pathing::save`, `Fields::save`, `Field::save`,
  `Work::save`, `Job::save`, `Route::save`, `Local::save`, `AbstractSearch::save`, `Corridor::save`,
  `Scratch::save`, `IndexedHeap::save`, `Formations::save`, `put_member`,
  `Arsenal::save_parts`, `Projectiles::save`, `Destruction::save` (and each `Wreck` and `Tally`),
  `put_weapon` (`SlotState`), `put_engage` and `Combat::engage_fits` (`Engage`),
  `TargetingConfig::write_hash`, `DestructionConfig::write_hash`, `WeaponTable::rules_digest`,
  the weapon digest (`WeaponStats`, `Loadout`), `ArmorTable::rules_digest`,
  `Ore::at_start`, `Crowd::has_state`, `Targeting::has_state`,
  `check_unsaved_sections` (money, repair, placement), `check_crowd`, `put_crowd`,
  `rules_digest` (the collision, formation and replan settings) and `Turning::write_hash` (in the
  movement digest) for theirs. A field added to any of them is
  a compile error until it is handled here (shown in the PR of WP13.1a: a field `pub(crate) probe:
  u8` on `World` gives `error[E0027]: pattern does not mention field `probe``).
* `restore(fresh, save(w)) == w` by `PartialEq`, and a resave byte-equal to the save.
* Per-tick checksums after a restore equal an uninterrupted run
  (`a_restored_world_continues_like_the_original`, saves after 1, 7, 33 and 97 ticks, with and
  without the crowd pass).

## The AI (WP13.1d)

The state of a computer player (`sf_ai::AiState`) lives outside the world and is saved apart from it:
`crates/sf_ai/src/save.rs`, tests in `tools/sf_headless/tests/save_ai.rs`. The host (WP13.1e) puts one
such part per AI into the save file next to the world's.

```rust
Ai::save(&self) -> Result<Vec<u8>, SaveError>
Ai::restore(fresh: Ai, bytes: &[u8], world: &World) -> Result<Ai, SaveError>
// the same one level down, on a writer and a reader:
AiState::save(&self, w: &mut ByteWriter) -> Result<(), SaveError>
AiState::restore(fresh: AiState, r: &mut ByteReader, bounds: &SaveBounds) -> Result<AiState, SaveError>
```

`fresh` is the AI the host makes for the same player, profile and start sites as for a new match; its seed
does not matter, because the random stream is restored from the bytes
(`Scenario::ais`, `sf_ai::match_ais`); `world` is the world restored from the same save. `SaveBounds::of(world)`
(in `observe.rs`, the one file that names the world) takes the world's tick, map size, entity slot count and
unit-table kinds. The error type is the world's `SaveError`.

Format: one section, tag `A`, a `u32` length, then the AI format version (`u32`, now **1**), the profile
digest (`Profile::digest`, `u64`), the player, the start sites, and then every field of the state in
declaration order (options as a flag byte and the value; maps and lists as a count and the entries, maps in
key order; kinds as `u16`, cells as two `u32`, ids as index and generation, ticks as `u64`). Posture tags
0 gather, 1 attack, 2 defend; group role 0 strike; group stages 0 staging, 1 advancing, 2 engaged, 3
returning (the digest's numbers).

| Field | Saved | Why |
|---|---|---|
| `profile` | digest only | rules: another profile is `SaveError::Rules` |
| `player`, `start_sites` | written, compared | the match hands them over: another one is `SaveError::Rules` |
| `manager_runs` | no (`_`) | derived: one manager per think in rotation, rebuilt from `thinks` |
| every other field (42) | yes | the AI's memory |

Restore refuses, besides `Truncated`, a wrong tag and trailing bytes:

* `NotFresh`: `fresh` differs from a new state of its player, profile and start sites in anything but its
  random stream (it has thought, or holds a memory). `Version(n)`: another AI format version.
* `Corrupt` with its own message for: an entity id whose slot index is not below the world's slot count; a
  cell (gather cell, anchors, goals, staging cells, threat cell, refused places, seen cells) outside the
  map, or a structure seen that does not fit it or has a zero side; a kind not in the world's unit table;
  an even random-stream increment; more thinks than the world has run ticks, a last think that disagrees
  with the number of thinks or is not before the world's tick; a structure seen of the AI's own player; any
  tick of something that happened (seen, sent, hit, launched, staged, peak, bounce) after the last think; a
  backoff or block (`blocked_until`, `refused`, `repair_backoff`, `rally_backoff`, `rescue_after`) later
  than the last think plus `blocked_ticks` (every one of them is set that way); more than `REFUSED_CAP`
  refused places; maps or id lists not strictly ascending (a key twice would merge); unit orders not
  strictly ascending by factory; the scout past its last waypoint (the other start sites plus four); more
  scouts sent than `max(scouts_max, 1)`; more than `GROUPS_MAX` groups, groups not ascending by id, a group
  id not below `next_group`, a merge into itself or into an id never given out, a unit in two groups, a
  recalled or closing unit that is not a member of its group; a
  jitter outside plus or minus `timing_jitter_permille`; a cleared site that is not another player's start
  site; a tally with the counter mix off, a bounce ledger with the escalation off, harvesters seen with the
  harvester threat off; an unknown posture, group role or stage.

Dead entities: the AI keeps the ids of structures and units it has seen and drops them at its next look, so
a remembered id may name a dead entity or a reused slot. Restore keeps them as they are
(`a_dead_entity_the_ai_remembers_is_kept_as_the_live_ai_keeps_it` proves the round trip of such a state;
that the run then goes on as before is shown by the continue-equals runs, whose save points include such
states). Known gap: the generation of a remembered id is not checked against its slot (the world has no
public accessor for a slot's generation, and this WP does not change `sf_sim`); a generation higher than
the slot's would be a future id. A recycled slot (an id with the same index as a live entity and an older
generation) is restored unchanged and is not tested on its own. The
counters the live AI never bounds (`previews`, `full_turns`, `stalled_turns`, `attacks_launched`,
`launch_value`, `bounce_value`, the per-unit order counts, the resumption skips) are not bounded either.

Bounds left for WP13.1h (looser than the live AI may keep, but not proven from the AI code, so not checked
now; a check a real save could fail is worse than a loose one): `thinks` against the number of ticks
`think_interval` allows (the host, not the AI, decides when it thinks); `last_think` against
`schedule::due`; the order, uniqueness and single tick of `sales`; `merge_into` naming a group that still
exists (the strike may be dissolved before its reinforcements notice).

Save point: the AI state is saved at a tick after the arrivals of that tick are scheduled and before the
AIs think, that is right before the AIs think ahead of `World::step` (`sf_game` `sim_thread.rs`). The
headless runner's `run_hooked` puts its hook there.

Guard 1: `AiState::save` destructures `AiState` (and `Known`, `Group`, `Directive`) without `..`. A field
added by a later phase-9 work package is a compile error there until it is saved or named as derived
(shown: a field `probe: u8` gives `error[E0027]: pattern does not mention field `probe``).

Tests (`save_ai.rs`): every 97 ticks of the 15 AI scenarios of CI each AI round-trips by `PartialEq` and
resaves byte-equal, with a wave forming, an order in flight, an attack under way, a defence and scouting
memory among the saved states; continue-equals (the AI digest after every tick, the AI commands and the
final world checksum equal the uninterrupted run) with every AI restored every 97 ticks in all 15, at every
tick of `ai_raid`, at the single ticks 101, 1009, 2503 and 4001 of `ai_raid`, and at other spacings of
`ai_raid_guard` and `ai_bounce`; every check above with the value at its bound passing and one past it
refused with its message. The world is not saved in these tests (WP13.1c is not done): it runs on.

## The test world

WP13.1a's tests build their own world (no CI scenario saved then): a 64x48 open map with five wanderers, four
units (two with a turret) on move, guard and queued orders, a legacy structure and a 2x2 structure,
three deaths with a slot reused, and pending commands of every variant from three players, one with
a sender's sequence number. WP13.1b adds a walled 96x64 map with the smallest path budget (512): twelve foot soldiers in one
group order (a flowfield and a formation), six trucks that turn in arcs on their own routes, three
structures placed one at a time and one taken away (`save/path_tests.rs`). WP13.1e runs every CI
scenario through save and load once every section saves.
