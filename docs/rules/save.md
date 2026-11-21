# Saving and restoring the world

WP13.1a, extended by WP13.1b (pathing, flowfields, formations, turning). The code is
`crates/sf_sim/src/save.rs` (format, sections, gates), the `save`/`restore` functions next to each
struct of `crates/sf_sim/src/path/` and `formation.rs`, and `crates/sf_math/src/bytes.rs` (the byte
writer and reader). Tests: `crates/sf_sim/src/save/tests.rs`, `crates/sf_sim/src/save/path_tests.rs`,
`tools/sf_headless/tests/save_paths.rs` and the tests in `bytes.rs`.

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
| 4 | `format_version`, `u32`, now **2** (1 was WP13.1a, without `P`, `F`, `M` and `prev_pos`) |
| 8 | `rules_digest`, `u64` (`World::rules_digest`) |
| 8 | `tick`, `u64`: the number of the next tick to run |
| 4 | the number of sections, `u32`, now 6 |

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
  none, 1 lead, 2 direct, 3 flow, 4 wait). A free slot
  holds the neutral values `Entities::clear` writes, so only its generation is written.
* `Q` command queue: every pending command with its `(tick, player, sequence)` key, then every
  player's next sequence number.
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
the targeting and destruction settings (or that there are none), and the numbers of every optional
rule set a world that saves may have: the ore (its rules and the start amount of every field cell,
through the ore fingerprint, which at `version == 0` is the start), the money rules
(`credits_per_ore`) and the repair and sale rules (all four numbers). Of the other optional rule
sets (harvest, production, power, vision, match) only whether the world has them is hashed: a world
with any of them does not save yet, and their numbers join in WP13.1c; the AI profile digests are
checked by each AI part on its own (WP13.1d, "The AI" below). A different digest on restore is `SaveError::Rules`
(`other_assets_or_settings_are_refused`: other hit points in the unit table, another collision
setting, another seed; `other_ore_money_or_repair_numbers_are_other_rules`: another ore start
amount, another `credits_per_ore`, another sale time).

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
  command tag, a current order `Scatter` (`begin` turns it into a move) or `Pursue` (current only
  with an engagement, which a save never holds), a queue longer than its ring, a random stream with
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
  they are replaced by the next step and are not hashed.

`def` is saved although it looks derived: a legacy `Spawn` or `Place` entity has `NO_DEF` whatever
its kind, so the kind alone does not say which.

## Not saved yet: the gates

Sections of WP13.1c (weapons, projectiles,
targeting, destruction, buildings, economy, production, power, repair, vision, outcome) are not in
the file. `World::save` refuses with `SaveError::Unsupported(section)` every world in which one of
them holds anything that is not its tick-0 state, so a half-saved world can never load silently.
The exact test per section (checked in this order, the first one that fails names the section):

| Section | `save` refuses when |
|---|---|
| `projectiles` | the projectile store is not equal to a never-used store (`Projectiles::default()`) |
| `weapons` | a slot is reloading or in a burst, or the last tick left shots, impacts, attacks or damage records (`hurt`) |
| `destruction` | a wreck, a tally or a death report exists, or a unit has kills |
| `economy` | the ore changed since the start (`version != 0`), the world has harvest rules, or money was spent |
| `production`, `power`, `vision`, `outcome` | the world has those rules at all (even at tick 0: their state starts moving in the first step; WP13.1c lifts this) |
| `targeting` | a unit remembers an engagement (`Engage` not at rest) |
| `buildings` | a structure is under construction (`build_left != 0`) |
| `repair` | a structure is repairing or being sold |

The subsystem gates come first; then, slot by slot, the column gates (the last three rows, and the
weapon and kills columns of the rows above). The pathing, flowfield, formation and turning gates of
WP13.1a are gone (WP13.1b saves those sections). Only their state is gated, not the
presence of the targeting, destruction, building and repair rules; a world with a weapon table is
not tested here yet (WP13.1c).

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
* **Epoch**: only `refresh_sectors` moves the service's epoch, called from the command loop (before
  the service, which then replans a job of the old epoch at once: the budget is at least 512 and the
  fields take at most half, so the job always runs) or from a destruction after it (whose tick saves
  nothing: `Unsupported("destruction")`). So a saved job is of the service's epoch, and its memory
  was made on the graph the restore rebuilds. **For WP13.1c**: once a save can follow a map change
  after the service, a job one epoch behind must be allowed (a bound K on the distance), and then
  the memory checks below must be skipped for it, as it replans without reading its memory.
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
  stage rules, the epoch rule and the portal checks apply to every job; the portal checks rest on
  the epoch rule (see *Epoch* above, for WP13.1c).
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
`probe_which_scenarios_save` pins which section refuses each of the other non-AI CI scenarios.

## Guards against a forgotten field

* `World::save` destructures `World` without `..`; every field is bound by name or as `_` with a
  comment that says why it is rules, derived, a report or gated. `save_entities` does the same for
  the entity columns, `CommandQueue::parts`, `Pathing::save`, `Fields::save`, `Field::save`,
  `Work::save`, `Job::save`, `Route::save`, `Local::save`, `AbstractSearch::save`, `Corridor::save`,
  `Scratch::save`, `IndexedHeap::save`, `Formations::save`, `put_member`,
  `Arsenal::unsaved_section`, `Ore::at_start`, `Crowd::has_state`, `Targeting::has_state`,
  `check_unsaved_sections` (destruction, money, repair, placement), `check_crowd`, `put_crowd`,
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
