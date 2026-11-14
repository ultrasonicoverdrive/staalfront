# The computer opponent

WP9.1a (the frame), WP9.1b-1 (the base), WP9.1b-2 (the economy) and WP9.1c-1 (production and the scout). Design memos of 10-11-2026 and 11-11-2026 (orchestrator, Fable);
Tijmen: "fase 9 mag eerder
beginnen". WP9.1a builds what every later part stands on: the AI **observes through the fog, remembers what it saw and keeps its schedule**.
WP9.1b-1 adds the first manager with rules, **construction** (the goal list, the power rule, the money rule, placement), so the AI builds
its base. WP9.1b-2 adds the **economy** manager (harvesters, the ore silo, repair; a lost structure is rebuilt by the goal list). WP9.1c-1 adds
**production** (the unit mix, the rally point, the money rule) and the first half of the army manager: the gather cell, **one scout** and the gathering
of army units, which **never attack** yet (WP9.1c-2). This page says what exists and what is a stub.

Code: `crates/sf_ai/src/` (`observe.rs`, `memory.rs`, `state.rs`, `schedule.rs`, `profile.rs`, `sites.rs`, `ai.rs`, and since WP9.1b-1
`budget.rs`, `base.rs`, `placement.rs`, since WP9.1b-2 `economy.rs`, and since WP9.1c-1 `production.rs` and `army.rs`), the profile format `crates/sf_data/src/ai_profile.rs`, the data
`assets/ai/normal.ron`, the driver
`tools/sf_headless/src/run.rs` (`run_full`, `run_watched`), the scenarios `assets/scenarios/ai_fog.ron`, `ai_base.ron`, `ai_noore.ron`,
`ai_boxed.ron`, `ai_economy.ron`, `ai_rebuild.ron`, `ai_deadlock.ron`, `ai_scout.ron`. Tests: `crates/sf_ai/tests/fence.rs`, the unit tests in each module,
`tools/sf_headless/tests/ai_fog.rs`, `ai_base.rs`, `ai_rules.rs`, `ai_economy.rs`, `ai_production.rs`, `ai_perf.rs`.

## Where it lives: outside the simulation, to the simulation's rules

The AI is a **player outside `sf_sim`**: a pure function of (the observation at tick `t`, its own state, its seed). It acts only by
scheduling the commands a human can send, so its commands are in the command queue and the world checksum like any other, and it can do
nothing a human cannot. No file of `crates/sf_sim/` changed for it. It follows the simulation's determinism rules: no floats, no
`HashMap`/`HashSet`, no clock, no I/O, randomness only from `sf_math::Rng` (stream `AI_STREAM + player`, seeded by the `seed` of its
`ai` entry). Enforced three ways: `scripts/check-sim-determinism.sh` scans `crates/sf_ai/src` for float and hash-collection tokens and
also for `Instant`, `SystemTime` and `std::fs/io/env/time/thread/process` (the host reads the profile file and hands it over); the float
lints of `crates/sf_ai/Cargo.toml` (`float_arithmetic`, `float_cmp`, `float_cmp_const`, `cast_precision_loss`); `crates/sf_ai/clippy.toml`
(a copy of the one of `sf_sim`: `HashMap` and `HashSet` are disallowed types).

The AI state is **not** in the world checksum, so `Ai::digest()` hashes it (player, random state, profile, start sites, think count, last
think, manager counts, every remembered structure in id order) and `sf_headless` prints it beside the checksum (`ai 0: 0x...`) and writes
it into the log (`"ai": [{"player": 0, "digest": "0x..."}]`, only for a scenario with `ai`). CI compares the logs of Linux and Windows
except the `host` block, so the digest is compared too. `--expect` checks only the world checksum; the AI digest is pinned in
`tests/ai_fog.rs` and compared across platforms by that log comparison. The state is plain data (integers, vectors, `BTreeMap`), ready to be saved
(WP13.1).

## The fence: no cheating on information

`sf_ai::observe` is the **only file of the crate that names `sf_sim::World`**. It builds an `Observation` and everything else takes only
that. `crates/sf_ai/tests/fence.rs` reads the files of `src/` and fails when another file contains the word `World`, the path
`sf_sim::world` or a glob import of `sf_sim`.

| In the observation | Rule |
|---|---|
| own entities | in full: id, kind, cell (a unit: the cell its position is in; a structure: its anchor), footprint, hit points, order, `standing` (a structure complete and with hit points left) |
| credits, ore store and capacity, power made and used | a player knows its own |
| entities of others | **only when their cell is `Visible`** for this player (`World::visibility`, which includes shared vision): a unit by the cell its position is in, a structure by **any** cell of its footprint; the same cell test as the snapshot's `seen` (`vision.md`, "For the player's side") |
| nothing else of an unseen entity | not its existence, hit points or order |
| `gone` | a remembered structure with a visible cell that is not among the visible entities. Decided from the picture alone; it never asks whether the entity is alive, which would tell the AI about structures it cannot see |

**Entity ids are opaque handles.** An id (in `Seen`, `Known` or the AI's own entities) is a slot index plus generation, which hints at how
many entities the world holds; no rule may reason from its value, only compare and look it up.

A world without vision rules shows everything to everyone, as it does for a human.

**WP9.1b-1 additions.** `Own` also carries `max_hp` (the unit table's), `building` (`Construction { left, total }` while going up),
`repairing`, and `factory` (`OwnFactory { queue: [(kind, free)], front_paid, status, rally }`): a player knows all of that of its own.

**The lens** (`observe::Lens`, also in `observe.rs`, so the fence test covers it): the managers get `(&Observation, &mut Lens)`. It wraps
the world and the player and answers only what a player may know beyond the observation:

| Question | Rule |
|---|---|
| `building(kind)`, `buildings()`, `harvester_kinds()`, `unit_cost(kind)`, `storage_of(kind)`, `faction_bit()`, `unit_factory(kind)`, `sale_refund(kind, hp, max_hp)`, `has_ore_ground()` | the public rules (building table, the kinds that harvest, a unit's price, what a structure stores, the player's own faction, which factory function makes a unit that the faction may have; the last six since WP9.1b-2: which factory function makes a unit the faction may have, what a sale pays at given hit points by the simulation's own rule, and whether the map has any ore ground) |
| `buildable(cell)`, `ground(cell)`, `harvestable(cell)` | the terrain of the map: a human sees the map. **Ore field locations are map knowledge; the amount of ore is not in the lens** (decision 2 of the WP9.1b memo) |
| `visible(cell)` | whether the player sees the cell now |
| `placement(kind, cell)` | the simulation's own rule (`World::check_placement`), **asked only when every cell of the footprint and the exit cell (when it is on the map) is `Visible` now**; `None` otherwise, without asking. A preview in the fog or shroud would reveal a hidden unit (`UnitInTheWay`) or structure. The lens counts the questions it passes on (`previews`) |

Tests: `the_lens_does_not_ask_about_ground_the_player_does_not_see` (a repair truck of player 1 in the shroud at (4, 60): the simulation says
`UnitInTheWay`, the lens says `None` and asks nothing, also for a footprint with one cell in the shroud; visible ground is asked) and
`a_unit_hidden_in_the_reach_changes_nothing_the_ai_does` (the same truck in `ai_base`: until that corner first comes into view, tick 3958,
the AI sends the same commands and has the same digest as without it, while the world checksums differ). **Not in the lens**: the ore
amounts, the explored bit, anything about other players' production or money.

**Memory.** `AiState::known_structures: BTreeMap<EntityId, Known { kind, owner, anchor, width, height, seen_tick }>` is written at every
think for each visible structure of another player and an entry is dropped when the observation says it is gone (the ghost rule of
`fog_look.md`, on the AI's side). Units are not remembered. A structure that was visible only between two thinks is missed (half a
second). The AI is also given **map knowledge**: `start_sites`, the cell of every player's construction centre from the match file
(`sf_ai::start_sites`), as a human who knows the map would know them. It is the only thing it learns about others without seeing.

## The base (WP9.1b-1)

### The budget

`budget::committed` = the unpaid part of every own structure going up (`cost * left / total`, rounded up, which is exactly what the
simulation still charges: it pays `cost * done / total` rounded down after `done` ticks, `tick_cost` in `money.rs`) plus the unpaid price of every
queued unit that is not free (the front item minus `front_paid`). `budget::free` = credits minus that, at least zero. **Stateless**: worked
out from the observation at every turn, so there is no ledger to drift. The simulation never refuses for lack of money (`economy.md`), so
this is the AI's own count. Credits are the ore store (`World::credits`).

### The construction turn

Every 40 ticks (manager 1 of the rotation), `base::turn`:

1. **Verify.** `pending_build = (kind, cell, tick)`, the build sent at the last turn, must be an own structure of that kind anchored on that
   cell (standing or going up). If not, the simulation refused it (a unit on the cell, say), and `(kind, cell)` goes into `refused` until
   `now + blocked_ticks`. The latch is cleared either way. Expired entries of `refused` and `blocked_until` are dropped. `refused` holds at
   most 64 entries (`REFUSED_CAP`); a new one pushes out the one that expires first.
2. **Parallel cap.** With `max_parallel_builds` own structures going up, nothing starts.
3. **Choose** the first goal entry `(kind, count)` with fewer than `count` standing or going up, not blocked (`blocked_until[kind] > now`),
   whose `requires` all stand.
4. **Power first** (only in a game with power rules). With the table's numbers, `made` = the `power` of own producers standing or going up,
   `used` = the same for consumers. If `made < used + use(choice) + power_margin`, the goal list's first power plant is built instead,
   **unless that plant is blocked (decision 6): then the choice stands**; the shortage floor of `power.md` means slower, never stuck.
   Nominal on purpose: a damaged plant is repair's job. On the shipped list this puts a third power plant before the radar: 140 used, the
   radar's 50 and the margin of 20 make 210, over the 200 made.
5. **Afford: the full price is saved first (decision 1).** Free credits at least the cost, or, with storage, at least
   `capacity * silo_percent / 100` when that is less (the store cannot hold more). **Emergencies start unfunded**: a refinery when none
   stands or goes up, a power plant when less power is made than used. With no harvester at all (none queued either) and a refinery
   standing, a normal build also needs its full price plus a harvester's, **not capped** (the memo's rule, section 2 step 5), so a broke AI
   does not lock its last credits into a structure it cannot cancel. Tests (`tools/sf_headless/tests/ai_rules.rs`, each where the rule
   decides): `a_refinery_starts_unfunded_when_none_stands` (400 credits), `a_power_plant_starts_unfunded_when_power_is_short` (a barracks,
   no plant, 100 credits), `a_normal_build_waits_for_its_full_price`, `without_a_harvester_a_build_also_keeps_a_harvesters_price` (1400
   credits wait, 1500 build), `a_build_saves_up_no_more_than_the_store_can_hold` (refineries of 1000: the vehicle factory is sent with
   900 to 1499 credits), `the_budget_counts_what_a_structure_going_up_still_costs` (two builds at once; and `committed` equals the sum of
   the simulation's `tick_cost` over the ticks still to build).
6. **Place** (below). Found: `BuildStructure { kind, cell }` is sent and latched in `pending_build`. Nothing fits:
   `blocked_until[kind] = now + blocked_ticks`, and the next turn tries the next entry.

Rebuilding has no rule of its own: a lost structure makes its goal entry unsatisfied again (tested by WP9.1b-2's `ai_rebuild`, "The
economy"). The AI sells only in the one rescue per match (decision 7 as reworded 12-11-2026, "The rescue").

### Placement

`placement::search` narrows the candidates with what the player knows and leaves the verdict to the simulation; only a `Some(Ok)` of
`Lens::placement` lets a build be sent.

* **Window.** The anchor cells within the build reach of each own standing anchor: x in `[ax - r - w, ax + aw + r]`, the same for y,
  clipped to the map, and the reach (`gap <= build_radius`, as in the simulation's `placement.rs`) checked per candidate.
* **Mask**, built once per search over the window grown by the footprint and the widest apron. A footprint cell must be buildable,
  **visible now (decision 3: the AI builds only on ground it sees)**, not under any structure it knows (its own, and others' that are
  visible), not under a unit it sees, not on an **apron** of an own refinery or factory, and not within the **lane** of an own structure
  (at least `lane` free cells between two of its structures). An apron is `(width, depth)` cells from the exit cell outwards, `width`
  centred on it: `refinery_apron` (3, 5; a parked harvester stands 4 cells out, `economy.md`), `factory_apron` (1, 3). A refinery or factory
  also needs its own apron on the map, open ground (`ground`), visible and free of structures.
* **Refused** `(kind, cell)` pairs are left out until they expire.
* **Score**, lowest first, ties by `(y, x)`; squared distances in half cells, integers only. A refinery: its exit cell to the nearest ore
  ground (`ore_cells`, the harvestable terrain within the reach plus 16 cells, listed again at every search, so it follows the anchors the
  player has now). A defence: its middle to
  the nearest start site of another player. Everything else, and a refinery without ore or a defence without other sites: its middle to the
  middle of the nearest own anchor.
* **Preview**, best first, at most `placement_cap` questions per search: `Some(Ok)` wins, `Some(Err)` goes into `refused`, `None` (not all
  visible) is skipped. **The resumption** (`AiState::resume`): a search that used up its `placement_cap` questions without a yes makes the
  next search of that kind skip that many candidates (its refusals expire together with its block, so the list is the same again), so the
  places behind the first `cap` are reached; back to the start when the list is shorter, cleared when a place is found. The skip is an
  **index into the candidate list** (`placement.rs`, the `skip` of the preview loop): if the list changes between two searches (a structure
  goes up, a unit moves) the index may jump over new candidates for one round, and they are reached after the wrap to the start
  (`the_search_asks_at_most_the_cap_remembers_the_refusals_and_resumes_behind_them`: two hidden units, cap 2, the second search asks one
  question and lands on the third place). **`placement_cap` is 32** (was 256; decision 4: 32 if the worst think exceeds 1 ms). Measured
  (`a_think_whose_every_placement_question_fails`: the observation and a search whose every question is refused, `ai_base` with 5,000
  wanderers, release, slowest of 5, three runs at load 4.8): cap 32 360 to 822 us, 64 637 to 1278 us, 128 1116 to 2266 us, 256 2351 to
  4893 us. So 256 exceeds 1 ms and 64 does on a busy machine; 32 stays under. **The 1 ms cap-32 worst think is a printed measurement with a thin
  margin, not a guarantee**: the reviewer of WP9.1b-1 saw 429 to 457 us on a quiet machine and 1037 us once under load. In the scenarios
  the mask makes the first question the answer
  (at most 1 per think).

### The numbers (profile, data)

New in the `economy` block of `assets/ai/normal.ron`, with defaults so that a profile written before WP9.1b-1 (the frozen
`ai_fog_defs/normal.ron`) still reads (decision 5): `lane` (0 to 4, default 1), `refinery_apron` and `factory_apron` (`(width, depth)`,
width odd, both 1 to 9; defaults (3, 5) and (1, 3)). `placement_cap` is 32. Pinned by `the_shipped_profile_is_pinned` (literals, of the file
and of the defaults), `every_number_is_checked_at_both_ends_when_the_file_is_read` and
`a_profile_without_the_base_numbers_of_wp9_1b_reads_with_the_defaults`. Acting since WP9.1b-1: the schedule numbers, `power_margin`,
`max_parallel_builds`, `silo_percent` (as the cap of what a build saves up), `blocked_ticks`, `placement_cap`, `lane`, the aprons.
Acting since WP9.1b-2: `harvesters_per_refinery`, `repair_below_percent`, `silo_percent` (now also the silo rule) and `blocked_ticks`
(also the backoff of repair and of a refused harvester order). `reserve` acts since WP9.1c-1 (production), the army numbers in part (see "The army in WP9.1c-1").

### State

New in `AiState` with WP9.1b-1, all in the digest in this order: `pending_build`, `blocked_until: BTreeMap<Kind, Tick>`,
`refused: BTreeMap<(Kind, Cell), Tick>`, `resume: BTreeMap<Kind, u32>`, `previews` (questions passed to the simulation in all thinks).
Integers, ordered maps and sorted vectors; no random draw (ties by `(y, x)`). Pinned by `the_digest_of_the_base_state_is_pinned` (a
literal) and `every_new_part_of_the_state_moves_the_digest`. The profile digest hashes `lane` and the aprons. WP9.1b-2 adds four more
parts after these ("The economy").

### Scenarios

All on `ai_base_defs` (a frozen copy, WP9.1b-1, of the shipped definitions, the profile and a match file of its own): player 0 (Meridian,
the AI) with a construction centre at (12, 68) and 6000 credits on the first match's map, player 1 a lone static construction centre at
(72, 14). The numbers of ore, harvest, storage (refinery 2000, silo 1500), money, production (build speed 3/2), power and vision are the
shipped ones of that day, written out. No scripted command.

| Scenario | Map | Ticks | Checksum | AI digest |
|---|---|---|---|---|
| `ai_base` | `m1` | 10,000 (8000 until WP9.1b-2) | `0xb38833330ef15577` | `0x11dde9a00dc14f23` |
| `ai_noore` | `m1_noore` (no ore) | 40,000 | `0x4c0e8ed5d1f89874` | `0xe10155cd63724d45` |
| `ai_boxed` | `m1_boxed` (rock round the base, a pocket x 5..17, y 66..73) | 8000 | `0x212468ddebc97ff5` | `0x781485e55788ab34` |

(WP9.1b-1 pinned `0x1b2eb3765910e496` / `0xa1bdd2e16f8db63b`, `0x4c0e8ed5d1f89874` / `0x94c928755b799c2f` and `0x212468ddebc97ff5` /
`0x5c40c9e2637e1194`; the causes of the moves are under "Pins that moved" in "The economy".)

The maps are drawn by `tools/sf_headless/examples/gen_ai_maps.rs` from `gen_match_map.rs` (a test draws them again and compares).
What `tools/sf_headless/tests/ai_base.rs` checks:

* `ai_base`: the goal list in order (centre, plant, refinery, barracks, plant, vehicle factory, tower, refinery, **plant** by the power
  rule, radar, tower, advanced defences); everything up to the vehicle factory standing by **T1 = tick 1998** and the whole list by **T1b =
  tick 8078** (measured; the test allows 2400 and 9700; 6518 before WP9.1b-2, which buys two harvesters); 11 builds and 2 harvester
  orders, none refused; power made at least power used after every tick; no stall
  after every tick; the lane between every two structures and no structure on any of the four aprons (from `placement::apron`); four
  harvesters (one free and one bought per refinery) and ore delivered.
* `ai_noore`: the 40,000 ticks end; at most 25 commands per 1000 ticks; the last build before tick 3000 (8 builds, what 6000 credits pay
  for), then nothing; under 900 credits left (the radar, the next goal, costs 900); no stall, nothing refused. Since WP9.1b-2 the
  vehicle factory stands at tick 1998 and a harvester is wanted, but **none is ordered: the map has no ore ground** (`Lens::has_ore_ground`,
  terrain, map knowledge: `no_harvester_is_ordered_on_a_map_without_ore_ground`).
* `ai_boxed`: the refinery fits nowhere (`blocked_until` holds it) and the later goals that do not need it are built (barracks, the second
  plant, both towers); one placement question per build.
* A build the simulation refuses (a unit of player 0 put on the plant's cell at tick 10, driving off at tick 20) is noticed at the next turn
  (tick 50) and the plant is built elsewhere, although the cell is free again by then.
* The search keeps a plant off the apron of a refinery the AI believes it has (`the_search_keeps_off_the_apron_of_an_own_refinery`).
* The whitelist: every command the AI sends in the three scenarios is `BuildStructure`, `QueueUnit`, `Repair` or `Sell` (the design's whitelist plus the rescue's sale; the
  last two come with WP9.1b-2, `ai_economy.rs` checks it for its two scenarios as well).
* A hidden unit changes nothing: the truck at (4, 60) is compared with the same truck at (92, 92), not with no truck, since a spawned entity
  takes an id and the ids of the AI's own things (which `QueueUnit` names) shift by one.

`ai_fog` moved with this part: its AI now builds a power plant (tick 10) and a refinery (tick 330, an emergency; its 1500 credits pay for no
more). New pins `0x768b77651509db8c` and `0x630c0979a8ae4749`; with the construction manager off and the new state and profile numbers left
out of the digests, both old values come back (measured).
WP9.1b-2 moved its digest again (checksum unchanged): `0xce7a0b6fb57c9cee`, because the digest hashes the economy and rescue state; "The economy" has the
measurement.

## The economy (WP9.1b-2)

`economy::turn`, every 40 ticks (manager 0 of the rotation: thinks 0, 4, 8, ... of the AI). Three rules in this order. No new profile
number: the numbers are `harvesters_per_refinery` (2), `silo_percent` (90), `repair_below_percent` (70) and `blocked_ticks` (600, also the
backoff of a repair and of a refused harvester order), all of them in `normal.ron` since WP9.1a. The ones the economy does not use yet are
`reserve` (it is for production since WP9.1c-1; the harvester order ignores it) and the army numbers.

1. **Harvesters.** `have` = own harvester units + harvester items in own factory queues (the free one that comes with a refinery counts);
   `want` = standing refineries x `harvesters_per_refinery`. When `have < want`, **no harvester is queued** (at most one in flight), a
   standing factory of the right function exists, no emergency build is stalled for lack of money and **the map has ore ground at all** (`Lens::has_ore_ground`: terrain, map knowledge; without it a harvester has nothing to mine), one
   `QueueUnit { factory, kind }` is sent: the first harvester kind of the table that the player's faction may have and whose factory function
   (`Lens::unit_factory`, from the production table) the player has standing, on the factory with the **shortest queue, the lowest id among
   equals**. The order is latched in `harvester_sent = (kind, tick)`; at the next economy turn, when no harvester item is in any queue the
   simulation refused it (a prerequisite, the queue limit), and the kind goes into `blocked_until` for `blocked_ticks`. A harvester that
   was lost is replaced by the same rule (`have` fell). The price (1100) is not checked: the order is accepted and waits, the sim pays
   structures before factories, and the budget counts the unpaid part of a queued unit, so the goal list waits for it (this is why the
   whole list of `ai_base` now stands at tick 8078, not 6518: the two harvesters cost 2,200).
   **Emergency stalled** (`economy::emergency_stalled`, also used by repair): no free credits (`budget::free` is 0) and an own structure
   going up that the base cannot do without: a harvester dock while no dock stands, or a power plant while less power is made than used.
2. **Silo.** `full_turns` counts the economy turns in a row with the ore store at least `silo_percent` full of what the stores can hold
   (`ore_stored * 100 >= capacity * silo_percent`, capacity above 0), and goes back to 0 otherwise. `emergency = Some(silo kind)` when all
   hold: `full_turns >= 2`; **no goal entry is open** (every `(kind, count)` of the list has `count` standing or going up, blocked or not,
   so a blocked entry keeps the silo back: `ai_boxed` never builds one); fewer silos (standing or going up) than standing refineries
   (**at most one silo per refinery**, decision 6 of the WP9.1b memo); the table has a silo kind (`Lens::buildings`: the first kind, ascending,
   with `storage > 0`, no dock function, a price and open to the faction). Otherwise `emergency` is `None` (it is worked out again every turn).
   The construction turn then builds it **before the goal list** (`base::choose`: the emergency first, if not blocked and its `requires`
   stand), through the normal power, money and placement steps; the silo is not an unfunded emergency (a full store pays for it).
3. **Repair.** Own complete structures (not going up) with `hp * 100 < max_hp * repair_below_percent` (exactly 70 percent is not below), not
   `repairing`, not in `repair_backoff`, in the order **anchor, power plants and factories, harvester dock, the rest**, then by id; at most
   `REPAIRS_PER_TURN` = 2 `Repair { on: true }` per turn. The AI never stops a repair (the simulation stops it at full hit points). While
   an emergency build is stalled (above) only the anchor is repaired (**decision: the anchor is repaired even then, so its repair may take
   credits the stalled build waits for**: losing the anchor ends all building, and a full repair costs at most `full_cost_permille`
   (shipped 250) thousandths of its price, paid per tick). **A running repair is not in `budget::committed`**: the repair price is a rule
   of the simulation that the Lens does not carry, so a repair shrinks the free credits only after it has paid; for the others this is a
   small, unmeasured error. A sent repair is in `repair_backoff[id] = now + blocked_ticks`; the
   entry is dropped when the structure is seen repairing, repaired above the threshold, gone, or the time is up. So a repair that runs is
   never repeated, a repair that never shows (a world without repair rules ignores `Repair` silently, `repair_sell.md`) is asked again
   at most once per `blocked_ticks`, and a structure that is hurt again after a repair is asked at once.
4. **Rebuild** has no rule of its own: a lost refinery or barracks makes its goal entry unsatisfied (the refinery as an unfunded emergency,
   the construction turn's step 5), a lost plant likewise, a lost silo or harvester is seen by rules 2 and 1.

**State** (hashed after the base state, in this order): `harvester_sent: Option<(Kind, Tick)>`, `full_turns`, `emergency: Option<Kind>`,
`repair_backoff: BTreeMap<EntityId, Tick>`. The design memo's `ore_cells` cache was not built (9.1b-1 lists the ore ground at every search).
Pinned by `the_digest_of_the_base_state_is_pinned` (a literal, now `0xb3c29db95d8d85f4`; it holds two backoff entries, so an iteration in
reverse fails it) and `every_new_part_of_the_state_moves_the_digest` (seventeen changes of the new parts, the rescue state included).

### The rescue (decision 7, reworded 12-11-2026)

**Once per match the AI may rescue itself from a money deadlock, and in that one rescue it sells as many structures as it takes to cover the
shortfall** (Tijmen, 12-11-2026; it replaces "the AI never sells"; spec: design memo WP9.1c, "One sale per match", with several structures instead
of one). The case it answers: an emergency build (the first refinery, a power plant while less is made than used) has started with no credits
to pay it and nothing can bring credits in. The sim never refuses for lack of money (`economy.md`), so such a build waits for ever.

* **Trigger** (economy turn; the observation and the budget only). `emergency_stalled` (above) on **two economy turns in a row**
  (`stalled_turns >= 2`, 80 ticks: a harvester may be about to unload), the rescue not used, **no income possible** (no own harvester unit, or no
  complete harvester dock), and the world has sale rules (`Lens::sale_refund` answers; `ai_base`'s world has none, so nothing is sold there).
  `need` = what is still owed on the stalled structure, `cost * left / total` rounded up (the formula of `budget::committed`), less the credits;
  nothing is sold when it is 0.
* **Eligible**: an own structure that is complete (standing, not going up, not already being sold) and has a price, **never** the anchor (selling it
  is defeat), a harvester dock, a power plant (`economy::sellable`, by function), a kind in the `requires` closure of the stalled kind
  (`requires_closure`; with the shipped data the plant, which is excluded by function already, so this is a guard), or the stalled structure
  itself. Its refund is `Lens::sale_refund(kind, hp, max_hp)`: the simulation's own `sale_refund` (`price * refund_permille * hp / (max_hp * 1000)`,
  `repair_sell.md` "Sale") at the **current** hit points, which is what it pays when the sale ends if the structure is not hurt more before.
* **Choice** (`economy::pick_sales`). If one eligible structure's refund covers `need`, the one with the **smallest sufficient refund** (ties: the
  latest in the goal list, a kind that is not in the list counting as latest, then the lowest id). Otherwise structures are added in ascending
  refund order (the same ties) until the sum covers `need`. If all of them together fall short **nothing is sold** and the rescue stays unused (a
  sale that does not finish the build buys nothing). All the `Sell { structure }` of the rescue go out in the same economy turn.
* **Latch.** `sales: Vec<(EntityId, Tick)>` holds them until the next economy turn: when any shows `selling` (`Own.selling`, from `World::sale_of`)
  or is gone, the sale started and `rescue_used = true` (for ever: a second deadlock in the match is not rescued); when the simulation refused all of
  them (not seen selling and not gone) `rescue_after = now + blocked_ticks` and the rescue stays unspent. A partly refused rescue counts as used.
* The construction turn does not count a structure that is being sold (`base::have` and the standing kinds skip `selling`), so a goal entry whose
  structure was sold is open again and is rebuilt normally, at full price, when the money comes.
* New state, hashed after the economy state in this order: `stalled_turns`, `rescue_used`, `sales`, `rescue_after`.
* The whitelist gains `Sell`: `BuildStructure`, `QueueUnit`, `Repair`, `Sell`. (WP9.1c-1 adds `SetRally` and `Order { Move }`: "The commands the AI
  sends now".)

**Arithmetic with the shipped prices** (refund 500 thousandths at full hit points): vehicle factory 750, radar 450, defence tower 250, barracks 200, silo
125, power plant 150 (never sold). A refinery costs 1,200, so one sale rescues a refinery lost at 0 credits only when at most 750 of it is
unpaid; at exactly 0 it takes the barracks, the radar and the vehicle factory (1,400) together. That is why the rescue adds structures.

**Scenario `ai_deadlock`** (`ai_deadlock_defs/`, a copy of `ai_rebuild_defs` with radar added to the goal list; the script is in the file):
player 0 queues four light tanks at tick 3000, which drains its credits to about 0 by tick 3400; at tick 3601 the refinery and both harvesters are
removed (credits 2). Measured (`a_money_deadlock_is_rescued_once_by_selling_what_covers_the_refinery`): the refinery is ordered at 3610; the
economy turn sees it stalled at 3640 and 3680 and **sends the three `Sell` at tick 3680** (barracks, radar, vehicle factory, in that order:
refunds 200, 450, 750); the refund pays the 1,200 and the **refinery stands again at tick 4314 (T = 713 ticks after the loss)**, with its free
harvester; the ore delivered rises again (800 at tick 3601 to 2,000 at 8,600). At tick 8601 the rebuilt refinery and its harvester are removed again
(1,002 credits): the refinery is ordered at 8610, paid as far as the credits go (to 0) and **stays unfinished to the end of the run**, with no
`Sell`. Pins: checksum `0x368ee3baa5362c04`, AI digest `0x261396ec364c3dc4`.

Unit tests (`tools/sf_headless/tests/ai_economy.rs`): the refunds at the current hit points (750, 450, 200; 75 for a factory at a tenth of its
hit points; none in a world without sale rules); `pick_sales` (smallest sufficient, a sum only when none suffices, nothing when short, the
ties); nothing on the first stalled turn and the sales together on the second; the sum of several; nothing when all together fall short; the
anchor, dock, power plant (by function, and in a deadlock where the plants would be chosen first) never sold; a standing dock with a harvester
means income and no sale; a refused sale does not spend the rescue and a started one does, also one that is gone; a structure being sold is not
counted by the construction turn. Mutations (each fails the named tests, see the PR): latch not set, anchor, dock or plant exclusion removed,
`sum >= need` removed, the two-turn wait removed, accumulation removed (the 0-credit refinery stays stuck: `ai_deadlock`), the income check
removed, a refused sale spending the rescue, the largest instead of the smallest sufficient refund, `have` counting a structure being sold, the
refund ignoring hit points, the sale ticks left out of the digest.

**Not covered.** A rescue for anything but a stalled emergency build (a harvester that cannot be paid, a goal entry that waits for ever) is not
made. A sale that is refused for a reason the observation cannot show (the structure was hit to 0) simply looks like a refusal and is tried
again after `blocked_ticks`. The refund is counted at the hit points seen when the sale is sent; a structure that is hurt during the 100 ticks of
the sale pays less, and the rescue may then fall short without noticing (the build stays stalled and no second rescue is made).

### Scenarios

`ai_economy` and `ai_rebuild` (and `ai_deadlock`, described under "The rescue"): the first match's map, player 0 the AI with 4,000 (`ai_economy`) or 6,000 credits (`ai_rebuild`), player 1 a lone
static construction centre; each on its own frozen copy of `ai_base_defs` (`ai_economy_defs/`, `ai_rebuild_defs/`) whose profile has a goal list
of four: power plant, refinery, barracks, vehicle factory. Both have the world rules of `ai_base`; `ai_economy` has richer ore (160 per cell,
regrowth 8 per 40 ticks) and 12,000 ticks, `ai_rebuild` has the repair rules of `repair_sell` (3 hit points per tick, a full repair costs 250
thousandths) and 12,000 ticks.

| Scenario | Script | Checksum | AI digest |
|---|---|---|---|
| `ai_economy` | tick 8001: the free harvester (id 4) removed | `0x432124f0b362c856` | `0xc79c0cbab7c6ddb7` |
| `ai_rebuild` | 3001 the refinery removed, 6001 the barracks removed, 8001 two Veyran light tanks spawned west of the plant and ordered to shoot it, 8250 they are removed | `0x92e27c78699ea7dd` | `0x15afee5ac92be396` |
| `ai_deadlock` | see "The rescue": tanks drain the credits at 3000, refinery and harvesters removed at 3601 and again (rebuilt) at 8601 | `0x368ee3baa5362c04` | `0x261396ec364c3dc4` |

Measured (`tools/sf_headless/tests/ai_economy.rs`, release; every number is printed by its test):

* **Delivered ore** at ticks 2000, 4000, ..., 12000: 310, 1000, 2200, 3400, 4230, 5560: it rises in every window of 2,000 ticks (the test asks at
  least 250 more each window). The store is full again at the end (3,410 of 3,500).
* **The silo.** The store holds 90 percent for 636 ticks in all at the start, from tick 785 (the starting credits, with the goal list open: nothing
  is built for it), and again from tick 6554 on; the silo is ordered at **tick 6610** (two economy turns, the construction turn 10 ticks
  later), is the only one
  (one refinery, one silo; `at_most_one_silo_per_refinery_and_one_going_up_counts` and the scenario), and stands.
* **Harvester replacement.** Orders at 2000 (the vehicle factory stood at 1998) and **8040** (the first economy turn after the loss at 8001),
  nothing else; the new harvester is out of the factory at tick **8440**; two harvesters again at the end.
* **T3, rebuild latency.** The refinery lost at 3001 is ordered at 3010 (the first construction turn, unfunded) and stands at **3545**
  (T3 = 544 ticks); the barracks lost at 6001 is ordered at 6010 and stands at **6278** (T3 = 277). Each is ordered once.
* **Repair.** The plant falls to 378 of 600 hit points (below 420), exactly one `Repair` is sent, at tick 8200, and the plant is full again at
  tick 8299. In the same scenario without repair rules the AI asks at 8200, 8800, 9400, 10000, 10600, 11200, 11800 (every 600 ticks) and the
  simulation ignores all seven.
* No command of the AI is refused in either scenario; every command is `BuildStructure`, `QueueUnit`, `Repair` or `Sell` (`ai_deadlock`: the three sales of the rescue).

`ai_base` now runs 10,000 ticks (was 8,000) because the harvesters delay the list; its checksum and digest are in the table of the base, and
so are those of `ai_noore` and `ai_boxed`.

All the AI digests of the tables moved a second time with the rescue state (layout only; no checksum moved with it).

### Pins that moved (WP9.1b-2) and why

| Pin | Before | After | Cause, measured |
|---|---|---|---|
| `ai_base` (8000 ticks before, now 10000) | `0x1b2eb3765910e496` / `0xa1bdd2e16f8db63b` | `0xb38833330ef15577` / `0x8630b1327e0feeb9` | the economy turn buys two harvesters; with the economy manager off (and, for the digest, its state left out of `AiState::digest`) both old values come back at 8000 ticks |
| `ai_noore` | `0x4c0e8ed5d1f89874` / `0x94c928755b799c2f` | checksum same / `0xe10155cd63724d45` | no harvester is ordered (no ore on the map), so only the digest layout moved; with the economy state left out of the digest the old digest comes back |
| `ai_boxed` | `0x212468ddebc97ff5` / `0x5c40c9e2637e1194` | checksum same / `0x31c680678a61e05c` | no refinery, so no harvester: only the digest layout moved; old digest back with the economy state left out |
| `ai_fog` | `0x768b77651509db8c` / `0x630c0979a8ae4749` | checksum same / `0xce7a0b6fb57c9cee` | likewise (no vehicle factory in 800 ticks); with the manager off and the state hashed it is `0x08e8a7b3e1e2b2e3`, since the economy turn counts `stalled_turns` of its stalled refinery |

With the economy turn switched off but the new state still hashed, the checksums of all four are the old ones and the digests are the new
ones for `ai_boxed` and `ai_noore` (their state stays at its start values and only the layout differs; `ai_noore` has the same
digest with the economy on, because the ore guard keeps it from ordering anything) and different again for `ai_base` and `ai_fog` (the
latter's stalled refinery makes the economy turn count `stalled_turns`). WP9.1b-2 changed the layout twice in this PR (the rescue state); the
values of the tables are the last, and the experiment above was repeated on it. No pin of an earlier phase moved (`golden.rs`, `wander_5000`, `command_mix`, `terrain_256`, `units_spawn` and the others).

### Cost of the economy turn

`ai_perf.rs`, release, this container, alternating the same test binary built with and without the economy turn (three pairs, then two more;
load average 0.8 to 4.4 from other jobs): the means are the same within noise, 7.6 to 9.2 us per AI per tick with 5,000 wanderers either way (the
observation of 5,000 entities per think dominates) and 0.10 to 0.17 us in the scenarios alone. The economy turn on its own
(`what_one_economy_turn_costs_on_its_own`, 20,000 turns, two repairs sent per turn): mean 0.26 us, worst 52.7 us, which is 0.007 us per tick. The
worst think in the crowd was 190 to 730 us in most runs and **1,173 us and 4,130 us in two runs of the build with the economy turn**, while
the build without it also had a spike of 940 us; the economy turn costs microseconds, so those look like scheduling spikes of a shared
machine, but that is not proven: the 1 ms worst think is a printed measurement and not a guarantee (see above).

## Production (WP9.1c-1)

`production::turn`, every 40 ticks (manager 2 of the rotation: thinks 2, 6, 10, ... of the AI, ten ticks after the construction turn). Off
(`Profile::makes_units()` false) for a profile with an empty `unit_mix` and no scout: the frozen profiles of the WP9.1b scenarios, whose
checksums therefore stay what they were. Three steps in order:

1. **Verify.** `units_sent` holds, per unit order of the last turn (ascending by factory id), `(factory, kind, tick, how many units of the kind
   the player had, how many items of the kind that factory had queued, both before the order)`. The order was **accepted** when that factory has more items
   of the kind queued now, or the player has more units of the kind (the item came out within the 40 ticks). Otherwise the simulation refused it (a
   prerequisite, the queue limit, a factory sold meanwhile) and the kind goes into `blocked_until` for `blocked_ticks`; a refused scout is also taken back
   from `scouts_sent`. A unit that died meanwhile is neither a refusal nor an acceptance (the first version compared the live count of the kind and read a
   death as a refusal: 600 ticks blocked, and a third scout possible). An accepted scout is never taken back, so `scouts_max` is a hard cap on scouts that
   entered a queue (`an_order_whose_kind_lost_a_unit_meanwhile_is_not_read_as_refused`, `a_scout_that_entered_a_queue_is_never_taken_back`). A weak
   spot left: a refused order is read as accepted when another unit of the kind comes out in the same 40 ticks.
2. **Rally** (P1). Every complete factory **that makes no harvester** (barracks, ...; `Own.factory` with no record counts as "no rally point") whose
   rally point is not the gather cell gets one `SetRally { factory, cell: Some(gather) }`; `rally_backoff[factory] = now + blocked_ticks`
   holds it back until the observation shows the point, the factory is gone or the time is up. The gather cell is
   [`army::ensure_gather`](#the-gather-cell). The sim sends each new unit (the free harvester of a refinery excepted) to the rally point with
   `Order::Move`; **the vehicle factory gets none**: a harvester sent to the gather cell first mines later, measured on `ai_scout` by forcing the point on
   the vehicle factory by hand (a scripted `SetRally` at tick 2,100): 8,330 ore delivered in 20,000 ticks against 9,800 with the AI's rule, 15 percent less, so the rule
   (decision of the orchestrator: more than 10 percent, no rally on a factory that can make a harvester) is in force (`a_rally_point_on_the_vehicle_factory_costs_ore_so_the_ai_gives_it_none`).
   Units from such a factory stand at its exit until the gathering rule of the army turn (idle, farther than `gather_radius`) orders them to the cell.
3. **Queue** (P3). One unit at a time, until nothing can be queued:
   * **Economy first.** Nothing while `economy::harvester_wanted` (fewer harvesters, queued ones included, than standing docks times
     `harvesters_per_refinery`, and a factory, ore on the map and the kind not blocked: the economy turn, which runs one think earlier in the rotation, buys it). An emergency build stalled for lack of money needs no check of its own: it is stalled only with no free credit at all.
   * **The scout first.** `scout_due`: the profile has a scout kind, `scouts_sent < scouts_max`, no scout is alive or queued, and it is the first
     (`tick >= scout_tick`, 600) or a replacement (`scout_retry`, 3,000 ticks after the AI saw the last one lost). Until the first scout is ordered
     **no unit of the mix is queued** (unless the scout kind is blocked), so the first unit made is the scout.
   * **Else the kind furthest below its share.** Among the kinds of `unit_mix` that are not blocked and have a factory with room, the one with
     the smallest `have_k / weight_k` (cross-multiplied, `have_k * w_j < have_j * w_k`; integer division would tie them), ties by list
     order. `have_k` counts the player's units and queue items of the kind and what this turn has ordered already.
   * **Which factory.** The complete factory of the kind's function (`Lens::unit_factory`) with the shortest queue, the lowest id among
     equals, with fewer than `max_queue` (2) items, and **at most one order per factory per turn**.
   * **How much money.** `free = budget::free` less what this turn has ordered; an order needs `free >= holdback + price`. `holdback` is 0 with
     **no goal entry open** (`base::goal_open`: some `(kind, count)` of the build list has fewer than `count` standing or going up, blocked
     or not); else the `reserve` (300) plus **what the construction turn is saving for** (`base::next_build`: its choice and the power rule,
     priced as step (5) of the construction turn does, `base::saving_for`: the price, or what the stores can hold at `silo_percent` when that
     is less). So production and construction read the same stateless `budget::free`, a queued unit is booked once, and **units of the mix cannot starve
     the goal list**. **The scout is held back by the `reserve` alone** (not by the next goal's price: it has to go while the list is open); it
     does cost the list: measured on `ai_scout`, the last goal structure is ordered at tick 14,610 with the scout and 12,650 without
     (`what_the_scouts_400_credits_cost_the_goal_list`), 1,960 ticks. With no goal open nothing is held back.

Decisions this follows (Tijmen, 12-11-2026, the orchestrator's recommendations): a dedicated `scout` unit (400 credits), one per match and at
most one replacement; production holds back the price of the next goal entry, not just `reserve`.

**Measured, and the consequence** (`ai_scout`, M1's map and credits: 4,000, release): the list of ten entries (7,700 credits of structures at list
prices, a third power plant by the power rule, two more harvesters of 1,100) has its last structure ordered at **tick 14,610**, so with the holdback the first army unit is queued at **tick 15,020** and the army at the gather cell is 0, 0, 0, 2, 10
at ticks 4,000, 8,000, 12,000, 16,000, 20,000. **In the real game seat** (`m1_ai`, the AI as player 1, `the_army_timing_in_the_game_seat` in `sf_game`): last goal
structure at **17,817**, first army unit at **17,867**, army at the gather cell 0, 0, 0, 0, 2 at the same ticks (one rally point, the barracks'; the first measurements of this section, 15,650 and 15,700,
were taken before the vehicle factory lost its rally point and are superseded). The same AI with the goal price left out of the holdback (the `reserve` alone, an experiment,
reverted) buys a rifleman at 1,140, has 7 army units at the gather cell at tick 2,000 and 31 at 20,000, and **builds no vehicle factory**: no tank and
one harvester (the free one of the first refinery) at tick 20,000, because every credit above 300 goes into infantry. So the decision
(hold the goal's price back) is what keeps the base growing, and it is also why there is no army for the first 15,000 to 18,000 ticks on a 4,000-credit
start. (This is the cost of the order of the list, which puts two refineries, a vehicle factory, two towers, a radar and the advanced defences before any
unit.) **WP9.1c-2's attack timing (`first_attack_tick` 2,400, `max_wait` 3,600, `min_units` 4) would see no army before tick 15,020 (`ai_scout`) or 17,867 (`m1_ai`) with
this profile**. Decision of the orchestrator, built in WP9.1c-2: an **army floor** (`army_floor_value`, start value 1,000, shipped 2,000): below it production holds back only the `reserve`; the army value is
the cost of the own army units, the queued mix items and what this turn has ordered already ("The army in WP9.1c-2", "The army floor").

## The army in WP9.1c-1

`army::turn`, every 40 ticks (manager 3: thinks 3, 7, 11, ...). Also off for a profile that makes no units. **No army unit attacks in this
WP.** `AiState::directive` (`Directive { posture: Posture::Gather, target: None }`, hashed) is the seam for WP9.1c-2: the army manager of
that WP writes the posture and a function turns it into orders; here the posture is always `Gather` and nothing reads the target.

### The gather cell

`army::ensure_gather` (rule P2), computed once into `AiState::gather` and worked out again only when it is `None` (no anchor yet, or no cell
found), its cell is no longer open ground (`Lens::ground`), or it lies under a structure the AI knows, its own, a visible or a remembered one
(`a_gather_cell_under_a_structure_is_worked_out_again`). The other way round, `placement::search` keeps the gather cell and its **8 neighbours**
free of the AI's own structures (they are blocked in the placement mask while `gather` is set;
`the_gather_cell_and_its_eight_neighbours_are_kept_free_of_the_ais_own_structures`). From the middle of the own standing anchor (the lowest id) `gather_distance`
cells towards the middle of the map, an integer step on the larger axis (a tie goes to x; the other axis is `d * dist / |d_large|`, truncated;
clamped to the map), then the first cell in the order **ring, y, x** over rings 0 to 3 (49 cells at most) that is open ground, under no
structure the AI knows (own, visible or remembered), **off the apron** of an own factory or dock (`placement::apron`, with the
profile's apron sizes) and **more than `lane + 2` cells (`placement::gap`) from every own structure**. On M1 for the base at (12, 68):
the middle is (14, 70), the map's (48, 48), so ten cells along x and 22 * 10 / 34 = 6 up: **(24, 64)** (pinned,
`the_gather_cell_is_ten_cells_from_the_base_towards_the_middle_of_the_map`; the lane, the apron and the "kept" rules have their own tests with
the expected cells worked out by a short script of the rule, apart from the code). `gather_distance` is **10** now (was 6, a placeholder
inside the base: the anchor is 4 x 4 and the build radius 8). No `plan_route`: an unreachable cell shows as units going `Idle` elsewhere,
which the gather rule below counts.

### The scout

`army::scout` (rule A2). The first own unit of the profile's scout kind that appears becomes `scout` (leg 0). The legs (`army::waypoints`) are the start
sites of the other players in ascending player id, then the centres of the four map quarters (NW, NE, SW, SE): on M1 `(72,14) (24,24) (72,24)
(24,72) (72,72)`. **Each leg is ordered once** (`scout_leg_sent`): `Order { [scout], Move(waypoint middle) }`, whatever the scout's order is
(the one exception to "only an idle unit is ordered", because a waypoint on a structure is a `Move` that never ends: measured, the scout
stands beside the enemy centre for ever with its `Move`). The leg advances when the scout is within 3 cells (Chebyshev) of the waypoint (and the next leg is
ordered in the same turn) or when `scout_patience` (1,200) ticks have passed since the order; after the last leg nothing is ordered, ever (no
loop). A scout that is gone from the observation sets `scout_gone_at`; so does an ordered scout that is neither a unit nor in a queue
(lost before it came out); the production turn buys a replacement `scout_retry` (3,000) ticks later, at most `scouts_max` (2) in the match.
What the scout learns needs no code: `memory::update` records every structure it sees.

### Gathering

`army::gather_units` (rule A7). An **army unit** (`army::is_army`) is an own unit with no footprint that is armed and mobile (`Own.armed`,
`Own.mobile`: the weapon table's loadout and the movement class of the kind), not a harvester kind and not the scout. One that is **`Idle`**
and farther than `gather_radius` (8, Chebyshev) from the gather cell gets `Order { Move(gather middle) }`, `gather_group` (19, the largest
formation of `sim.ron`) units to a command in ascending id order, **at most `GATHER_COMMANDS_PER_TURN` = 2 commands per turn**. `sent[id] = (tick of
the last order, how many without arriving)`: a unit is not ordered again within `resend_ticks` (120), and after `gather_tries` (3) orders
without arriving it is left where it is; an entry goes when the unit is within the radius (so a unit that wanders off later starts again
with a first order) or is dead (`sent` is pruned to the live units every turn). A unit with any order but `Idle` is never ordered: how
the AI learns that an order had no effect is always the observation (still `Idle` far away: refused or given up after `replan_max`).

### State

New in `AiState` with WP9.1c-1, all in the digest in this order after the rescue state: `gather: Option<Cell>`, `rally_backoff: BTreeMap<EntityId,
Tick>`, `units_sent: Vec<UnitSent>` (`UnitSent = (factory, kind, tick, units of the kind before, items of the kind that factory had queued before)`, i.e. `(EntityId, Kind, Tick, u32, u32)`), `scout: Option<EntityId>`, `scout_leg: u8`, `scout_leg_sent: Option<Tick>`, `scouts_sent:
u8`, `scout_gone_at: Option<Tick>`, `directive` (posture number and target), `sent: BTreeMap<EntityId, (Tick, u8)>`. Pinned by
`the_digest_of_the_base_state_is_pinned` (a literal, now `0xaec61e0952ceb855`, with two entries in every map and list so that an iteration in reverse
fails it) and `every_part_of_the_production_and_army_state_moves_the_digest` (25 changes); the profile digest hashes the new numbers
(`the_digest_sees_every_number_and_list`).

### The numbers (profile, data)

New in `assets/ai/normal.ron`, all with defaults so that the frozen profiles still read (the five of WP9.1b have an empty `unit_mix` now, see
"Pins that moved"): economy `max_queue` 2 (1 to 5); army `gather_radius` 8 (1 to 32), `gather_group` 19 (1 to 64), `gather_tries` 3 (1 to
10), `resend_ticks` 120, `scouts_max` 2 (0 to 4), `scout_tick` 600, `scout_patience` 1200, `scout_retry` 3000; per faction `scout: Some("scout")`
(a role checked like a unit of the mix: infantry or vehicle, open to the faction, with a cost, its factory in the build list; no default:
**left out is no scout**, where the design memo had "the first rifleman is the scout"). `gather_distance` is 10. `unit_mix` may be empty
(`an_empty_unit_mix_is_a_profile_that_makes_no_units`). Pinned by `the_shipped_profile_is_pinned` (literals), `every_number_is_checked_at_both_ends_when_the_file_is_read`,
`a_profile_without_the_numbers_of_wp9_1c_1_reads_with_the_defaults` and `the_scout_is_checked_like_a_unit_of_the_mix` (`crates/sf_data/src/ai_profile/tests.rs`).
Not acting yet: `attack_value`, `max_wait`, `min_units`, `first_attack_tick`, `defend_ticks` (WP9.1c-2).

### The commands the AI sends now

`BuildStructure`, `QueueUnit`, `SetRally`, `Repair`, `Sell` (the rescue) and `Order { ids, Move(..), queued: false }`: no `Attack`, no
`AttackMove` (WP9.1c-1; since WP9.1c-2 also `AttackMove` and `Attack`, at most `ARMY_COMMANDS_PER_TURN` orders per army turn). `the_ai_of_ai_scout_sends_only_what_the_list_allows_and_at_a_bounded_rate` checks every command of 20,000 ticks, and
`over_forty_thousand_ticks_...` of 40,000: the most in any 1,000 ticks is **6** in both (bound 10), every `Order` is a plain `Move`. (Historical, WP9.1c-1. Since WP9.1c-2 the bound is 15 and `AttackMove` and `Attack` are allowed: "The army in WP9.1c-2".)

### Scenario `ai_scout`

`assets/scenarios/ai_scout.ron`, `ai_scout_defs/` (a frozen copy of the shipped definitions and of `assets/ai/normal.ron` as 9.1c-1 leaves it, and a
match file of its own): M1's map, player 0 the AI (Meridian, 4,000 credits as in `m1.ron`), player 1 a static construction centre at (72, 14) with a
power plant at (78, 20) and nothing that shoots (the real opponent of M1, with its tower, is the test of WP9.1c-2). No scripted command. 20,000
ticks. Checksum `0xf2be53002209e2f6` (`ai_scout.expected`), AI digest `0x584e81c7938e7859`; in the CI list and so in the Linux/Windows log
comparison, which includes the AI digest.

Measured (`tools/sf_headless/tests/ai_production.rs`, every number printed by its test):

| What | Tick |
|---|---|
| the scout is queued (the first unit; `scout_tick` 600) and the barracks gets its rally point | 1,100 |
| the scout is sent to the first leg, (72, 14) | 1,270 |
| the AI's memory holds the enemy construction centre (the scout's sight is 8) | **2,351** |
| the scout stands within 3 cells of the start site | 2,439 |
| the other four legs are ordered | 2,470, 3,190, 3,870, 4,870 |
| the first harvester is queued (the vehicle factory stands; it gets no rally point) | 2,080 |
| the last structure of the goal list is ordered | 14,610 |
| the first army unit is queued | **15,020** |

Without a scout (`scouts_max` 0) the memory never holds the enemy centre in 20,000 ticks (nothing else leaves the base, the army only
gathers), and no `Order` goes anywhere but to the gather cell. A scout lost at tick 3,000 is replaced at **7,980** (`scout_retry` 3,000 puts it after 6,000; the
money decides the rest: free credits stay at 0 while the goal list spends everything, and the scout needs the reserve and its 400), the
replacement is sent to leg 0 again, and with the second one lost as well nothing is bought again (`scouts_max` 2).

### Pins that moved (WP9.1c-1) and why

Every AI digest moved (the profile digest hashes the new numbers, and the state layout has the new parts); **no checksum of an earlier phase
moved** (`golden.rs` and every `.expected` but the new `ai_scout`). The frozen profiles of the seven AI scenarios of 9.1a and 9.1b got
`unit_mix: []` (and have no `scout`), a deliberate edit of the frozen copies (a note at the top of each file): with the unit mix they had, production
and rally would move their checksums, and "the managers off" is what keeps them meaningful. The cause is measured by
`the_managers_stay_off_for_the_frozen_profiles_and_the_old_mix_would_move_every_checksum`: each runs on its frozen profile to the pinned
checksum with no `SetRally`, no `Order` and no unit but a harvester ordered and the state of the new parts at its start values, and with the
old unit mix put back in the profile ends on another checksum:

| Scenario | Checksum (pinned, unchanged) | With the old unit mix (managers on) | AI digest before | AI digest after |
|---|---|---|---|---|
| `ai_fog` | `0x768b77651509db8c` | `0x55dccaee5a3b70f9` | `0xce7a0b6fb57c9cee` | `0xacb2c8e5fb876451` |
| `ai_base` | `0xb38833330ef15577` | `0xf185f21cef6de9ac` | `0x8630b1327e0feeb9` | `0x4dd11cc733dd4b46` |
| `ai_noore` | `0x4c0e8ed5d1f89874` | `0xe5b92f46ee9b48db` | `0xe10155cd63724d45` | `0x94e97ccd07405cba` |
| `ai_boxed` | `0x212468ddebc97ff5` | `0x7b02a0328176580c` | `0x31c680678a61e05c` | `0x5b7d1adaf26aa90f` |
| `ai_economy` | `0x432124f0b362c856` | `0xdd0ec233993b6920` | `0xc79c0cbab7c6ddb7` | `0xf94be57bd5f168e4` |
| `ai_rebuild` | `0x92e27c78699ea7dd` | `0x91685b8297c593fa` | `0x15afee5ac92be396` | `0xdf9bfa7c2721ec51` |
| `ai_deadlock` | `0x368ee3baa5362c04` | `0xa468d64dc6714a5a` | `0x261396ec364c3dc4` | `0x3cba4a139131b094` |

(The "with the old mix" column puts `rifleman 4, rocket_trooper 2, light_tank 3` into each frozen profile, even where the frozen profile had no
rocket trooper; it shows that the managers' commands move the checksum, not the exact old value. The `ai_fog` run moves because the scripted
scout is an armed unit the army turn gathers.) `sf_data/tests/buildings.rs` lists `ai_scout_defs` among the copies with building blocks.

### Cost

`ai_perf.rs`, release, this container, load average 0.7 to 2.0 (other jobs ran), two runs: `ai_scout` alone 0.21 and 0.43 us per AI per tick (worst think 52.6
and 63.2 us, 900 thinks), with 5,000 wanderers 9.82 and 9.34 us (worst think 292.5 us in `Construction`, 371.5 us in `Production`), against 0.33 and 0.21 us alone and 9.14 and
8.03 us with the crowd for `ai_fog` (managers off) in the same runs: the observation of 5,000 entities per think dominates, as before, and the
difference of `ai_scout` to `ai_fog` is inside the noise between the two runs. A whole run of `ai_fog` with 5,000 wanderers, alternating, medians of three: 564 and 603 ticks/s
with the AI against 666 and 620 without. The production turn and the army turn on their own
(`what_one_production_turn_and_one_army_turn_cost_on_their_own`, 20,000 turns each on the observation of `ai_scout` at tick 17,000 with 200 more idle soldiers
added, a rich player so that production runs its whole loop; the state is cloned outside the clock): production **mean 5.7 us, worst 184 us** (two orders per
turn; 2.8 us when no order is sent), army **mean 2.9 us, worst 270 us** (two gather orders of 19 and the bookkeeping of 200 units; 2.2 us in the first run), that is about 0.14 and 0.07 us per tick
(a turn every 40 ticks). Budget: mean 20 us per AI per tick, worst think 1 ms; the worst thinks above are spikes of a shared machine that the turns themselves
(microseconds) do not explain, but that is not proven.

## The army in WP9.1c-2

The attack half of the army manager: threat and defence, the attack timing, the target, marching and finishing, the army floor of production,
and an AI that is out of play. Code: `crates/sf_ai/src/army.rs` (`watch`, `decide`, `target`, `beside`, `operations_minimal`, `finishing_target`),
`production.rs` (`army_value`, the floor at the holdback), `ai.rs` (`think_with`), `observe.rs` (`Observation::out_of_play`). Tests:
`tools/sf_headless/tests/ai_army.rs` (scenarios `ai_vs_passive` and `ai_raid`, and every rule on an observation the test builds), the profile
tests in `crates/sf_data/src/ai_profile/tests.rs`, `crates/sf_ai/src/state.rs` and `ai.rs` (unit tests), `crates/sf_game/tests/ai_in_game.rs`.
Decisions of 12-11-2026 this follows (the orchestrator; dated in PLAN Phase 9): the target is the enemy construction centre; only the units at
home defend and an attack is called back only for the construction centre; `defend_ticks` 400; the army floor; a defeated AI stops; a sold
structure is no hit.

**The seam.** The rules write `AiState::directive` (`Posture` `Gather` 0, `Attack` 1, `Defend` 2, hashed, and a target cell);
`army::operations_minimal(state, obs, lens, out, gather, home, budget)` turns it into orders. WP9.2 replaces that function.

### Threat (A3, every think)

`army::watch`, called by `Ai::think_with` after the memory update and before the manager of the turn, so **every think** (every 10 ticks, not
only the army turn). `hp_seen` holds every own structure that is not being sold, `(kind, anchor cell, hit points)`, refreshed at every think. A
**hit** is a structure whose hit points fell since the last think, or one that is gone from `obs.own` and is not in the economy's `sales`. A
repair only raises hit points; a structure being sold (`Own.selling`) is left out of `hp_seen`, and **the economy drops a structure from
`hp_seen` when it sends its sale** (decision 6), so the rescue sale never reads as an attack (`a_hit_turns_the_posture_to_defend_a_repair_or_a_sale_does_not`).
On a hit: `threat = (cell, tick)`, `defend_since` is set at the first hit of a defence, and the posture is `Defend` at once. The cell is the cell of
the nearest **armed unit of another player that is seen** within `gather_radius` of the hit structure's footprint (a cell, never a unit id), else open
ground beside the footprint facing the gather cell (`army::beside`). (Measured on `ai_raid`: with the cell beside the structure alone the defenders
stood idle out of reach of two of the three raiders, which shot the plant to the end of the run; with the raiders' cell all three die.) Every new
hit renews the tick. **`defend_ticks` (400) after the last hit** the threat is cleared at the next think and the posture goes back to `Attack`
(if an attack is still out) or `Gather`. A hit on a **construction centre** while an attack is out **calls the attack back**: the attackers are
no longer attackers and defend with the rest. *Fix round 1 (decision of 12-11-2026):* the former attackers go into `recalled` and get the
defence `AttackMove` **once whatever their observed order is** (a wave on its `AttackMove` too; still under `resend_ticks` and the command cap);
the first version only turned the idle ones, so a marching wave marched on
(`a_hit_on_the_centre_calls_back_a_wave_that_is_still_marching`). `recalled` empties when the order is sent or the threat ends. A hit on any other structure leaves the attackers on their attack.

**Who defends.** Every army unit that is not an attacker: one `AttackMove(threat cell)` for all of them, given to a unit that is `Idle` (and
farther than `arrive_radius` from the threat cell, not ordered within `resend_ticks`) or on a `Move` it got before the defence began
(`defend_since`, so a unit walking to the gather cell is turned round once). A unit with any other order (an `AttackMove`, a `Pursue` of its own)
is left alone. While a threat stands nothing is gathered and no attack is launched.

**Known gap** (decision 3): while the whole army is out on an attack, a hit on a structure that is not the construction centre is answered by
nothing but the towers (no unit is at home, and the attackers are not called back). *13-11-2026 (WP9.2a): the rule `home_guard_permille` closes it in the code, but the shipped profile has it off (measured harmful); see "Groups".*

### The attack (A4, A5, A6)

**Trigger** (army turn, posture `Gather`, no threat, no attack out). `gathered` = the army units that are `Idle` and within `gather_radius` (8)
of the gather cell, and those given up on after `gather_tries` orders (counted where they stand). `value` = the sum of `Lens::unit_cost` over
them. Attack when `tick >= first_attack_tick` and (`value >= attack_value` or (`tick >= last_attack_tick + max_wait` and at least `min_units`
gathered)); `last_attack_tick` starts at 0, so `max_wait` also times the first attack. Then `attackers = gathered` (ascending ids),
`attack_target`, `last_attack_tick = attack_started = tick`, `attacks_launched += 1`, posture `Attack`. Only gathered idle units count and go:
units made later wait for the next wave. Pinned: `the_attack_goes_at_attack_value_and_not_one_credit_below`,
`never_before_first_attack_tick_and_max_wait_needs_min_units`, `only_idle_units_within_the_gather_radius_count_and_go`; every wave of
`ai_vs_passive` has at least `min_units`.

**Target** (`army::target`, decision 2). The construction centre of another player in the AI's memory (`known_structures`), the nearest to
the gather cell by squared distance (the lowest id among equals); else the start site of another player whose cell the AI has not seen empty
(`sites_cleared`: the site's cell visible and no construction centre of that player on it), the nearest (the lowest player among equals),
as a 4 x 4 footprint (the building table's anchor); else the nearest structure of another player it remembers; else none (the army stays
gathered). The cell marched on is `army::beside`: open ground just outside the footprint, rings 1 to 3, nearest to the gather cell (the first in
ring, y, x order among equals). A cell under the structure would be a move that never ends. Pinned: `the_target_is_the_known_centre_then_the_start_site_then_any_known_structure`.

**Marching and finishing** (`operations_minimal`), each army turn while an attack is out:
* attackers that are `Idle` (and not ordered within `resend_ticks`) farther than `arrive_radius` (6, Chebyshev) from the target cell: **one**
  `AttackMove(target)` with all their ids (an `AttackMove` stops to fight what it meets and marches on; a `Move` never fights);
* attackers that are `Idle` within `arrive_radius`: **one** `Attack(id)` on the **visible** construction centre of another player nearest to the
  target cell, else on the nearest visible structure of another player (the finishing rule: an idle unit only fires at what is in its reach,
  `targeting.md`). The id comes only from `obs.visible`, and only a structure: never a unit, never an id from memory
  (`idle_attackers_at_the_target_finish_the_visible_centre_and_never_an_unseen_one`);
* *fix round 2*: an idle attacker within `arrive_radius` that sees no structure to finish gets one `AttackMove` onto the middle of the
  target structure (or the start site), once per attacker per attack (`AiState::closing`, cleared at the end), under `resend_ticks` and the
  command cap, so it closes in and sees it (same test);
* an attacker with any other order is left alone (`attackers_are_ordered_only_when_idle_and_not_again_within_resend_ticks`).

**The end.** No attacker alive (back to `Gather`); no target left; `attack_patience` (2,400) ticks with no attacker within `arrive_radius` of
the target **with a victim in sight** (*fix round 2, decision of 12-11-2026*: `attack_started` is moved to the turn only when an attacker
stands there **and** `finishing_target` sees a structure to finish; attackers that stand there seeing nothing no longer keep the attack alive,
`attackers_at_the_target_that_see_nothing_close_in_once_and_the_attack_still_ends`); or (*fix round 1, decision of 12-11-2026*) `attack_patience` ticks
in which the structure being finished (`finishing_target`, seen) has not lost hit points while an attacker stands at the target (`target_hp`;
units there that cannot kill it no longer keep the attack alive for ever,
`an_attack_ends_when_the_target_stops_losing_hit_points_for_attack_patience`). The survivors get one `Move(gather)` and the posture is `Gather`.
**That `Move` is a retreat on purpose**: a `Move` never fights, so the survivors walk home without stopping for what shoots at them; an
`AttackMove` home would stop them at every enemy on the way, the fight the attack just gave up. They are gathered again and go with the next wave
(`an_attack_ends_with_its_attackers_its_target_or_its_patience_and_the_survivors_go_home`). The target is worked out again every turn, so a
centre seen gone moves the attack to the next target.

**The rate.** At most `ARMY_COMMANDS_PER_TURN` = 4 commands per army turn (the scout's included; of which at most `GATHER_COMMANDS_PER_TURN` = 2
gather orders), each carrying many ids; a unit is ordered only when its observed order is `Idle` (or once, on a `Move`, when a defence began),
and not again within `resend_ticks`. How the AI learns that an order had no effect is the observation (the unit is `Idle` again where it was).

### The army floor (decision 1)

`army_floor_value` (profile, **2,000**, measured; the start value was 1,000). *Fix round 1 (decision of 12-11-2026): the floor applies only while
no attack has gone out yet **and the tick is before `max_wait`**, or while the posture is `Defend`*, so it can never hold the goal list past
`max_wait` (6,000; 12,000 since WP9.1c-3) outside a defence, also when units die at the gather cell with no structure hit and no attack ever goes
(`the_army_floor_ends_at_max_wait_before_the_first_attack`). The army's value (`production::army_value`) is the price of every own
army unit (`army::is_army`) plus every item of a kind of the unit mix in an own queue plus what the production turn running has ordered (without
that last part each factory would overshoot by one: `the_army_value_counts_the_queue_and_what_this_turn_ordered`). While it is below the floor,
**and** no attack has gone out yet (`attacks_launched == 0`) and the tick is before `max_wait`, **or** the posture is `Defend`, a unit of the mix is held back by the `reserve` alone
(like the scout); otherwise by the `reserve` and the next goal's price, as in WP9.1c-1. One condition at the holdback, no second money path
(`below_the_army_floor_production_holds_back_only_the_reserve`).

**The money, measured (fix round 1).** In `m1_ai` the floor is reached by the 12th unit (about tick 4,400) and is off after the first attack
(6,037), yet nothing is built until the win at 8,396. The credits trace (`the_money_of_the_ai_in_the_game_seat_around_the_first_attack`,
every 250 ticks from 5,500 to 8,500): credits = free = 600 at 5,500, 800 at 6,500, 1,000 at 7,250, 1,200 at 8,250; nothing queued; one
harvester; ore delivered 1,200 → 1,800. The next goal is the vehicle factory (1,500), so production holds back 1,800 (reserve 300 + 1,500) and
construction saves for 1,500: the gap is the income of **one harvester, about 200 credits per 750 ticks**. The floor spent the 4,000 starting
credits on twelve units (2,000) and the first four goals, and the second harvester comes only with the vehicle factory. With the attack switched
off (no win) **the goal list does not complete in 30,000 ticks**: `ai_vs_passive` makes 9 of its builds (the vehicle factory at 11,650, the last at
29,090), `m1_ai` its last at 29,777; the army stays at 12 (`the_goal_list_with_the_floor_and_no_attack_over_thirty_thousand_ticks`,
`the_goal_list_in_the_game_seat_with_the_floor_and_no_attack`). That is the cost of the floor on a 4,000-credit start, stated as a gap. *(WP9.1c-3: with the final opening the list completes at 22,730 in `ai_vs_passive` (the 20,000 target missed, the cause is the map's ore) and at 19,777 in `m1_ai` (met); see "The opening".)* *14-11-2026 (WP9.2a-2): superseded, 20,297 since WP9.2a-2, see "The start-site advantage".* The guard against the known risk (under steady losses the army stays below the
floor and the goal list stalls): once an attack has gone out, the floor holds only while the army defends.

Measured on `ai_vs_passive` (the AI as player 0 at (12, 68) with 4,000 credits against m1's static opponent, release, 40,000 ticks, the tick the
match names the AI the winner):

| `army_floor_value` | `max_wait` 3,600 | `max_wait` 6,000 | `max_wait` 9,000 |
|---|---|---|---|
| 0 (no floor) | 24,745 | 25,744 | |
| 1,000 | 26,387 | 26,387 | |
| 1,500 | 35,943 | 34,828 | 35,628 |
| 2,000 | 35,426 | **14,659** | 21,062 |
| 3,000 | 35,426 | 9,260 (goal list stalled at its 4th entry) | 12,229 (stalled) |

The first wave goes at `max_wait` (or at `attack_value`, which the floor never lets the army reach while goals are open). With 3,600 it leaves
before the army reaches the floor and bounces off m1's tower, four riflemen and tank; the goal list then holds the money (the floor is off after
the first attack) and the second wave comes 20,000 to 30,000 ticks later. With a floor of 3,000 the list stalls until the attack wins: the AI wins
sooner on this map but has no base. So **`army_floor_value` 2,000 and `max_wait` 6,000** (was 3,600; both builder's measured values, decision 4). *Superseded in part by WP9.1c-3: `max_wait` 12,000 and `attack_value` 2,000 with the vehicle factory second in the list, see "The opening"; the table above is the WP9.1c-2 order.*

### A defeated AI (decision 5)

`Observation::out_of_play` is true when `World::defeat_tick(player)` is set or `World::match_result()` is decided (both public: a player knows it
has lost and that the match is over). `Ai::think_with` then returns at once: no command, no memory update, no count, so the digest stands still.
In the game a refused command is only an `eprintln`; after this rule none is sent. Tests: `an_ai_out_of_play_sends_nothing_and_its_state_stands_still`
(unit), `a_defeated_ai_sends_nothing_and_its_state_stands_still` (the AI's centre removed at tick 3,001: no command after it, the digest equal at
3,100 and 6,000), `the_ai_of_ai_vs_passive_sends_..._nothing_after_the_win`, and in the game seat `the_army_timing_in_the_game_seat` (nothing sent
once the match is decided).

### State and numbers

New in `AiState`, hashed after the scout state in this order: `hp_seen: BTreeMap<EntityId, (Kind, Cell, u32)>`, `threat: Option<(Cell, Tick)>`,
`defend_since: Option<Tick>`, `directive` (posture number, target cell), `attackers: Vec<EntityId>` (ascending), `attack_target: Option<Cell>`,
`last_attack_tick`, `attack_started`, `attacks_launched: u32`, (fix round 1) `target_hp: Option<(EntityId, u32, Tick)>`, `recalled: Vec<EntityId>` (ascending), `sites_cleared: Vec<PlayerId>` (ascending), then `sent` as before (now the tick of
any army order; the tries count only gather orders). Pinned: `the_digest_of_the_base_state_is_pinned` (`0x392b1f3256aa3c19` since fix round 2 (`closing` hashed), two entries in every
map and list) and `every_part_of_the_threat_and_attack_state_moves_the_digest` (32 changes, the posture numbers 0, 1, 2 as literals).

New profile numbers, with serde defaults so that every frozen profile still reads: `arrive_radius` 6 (1 to 32), `attack_patience` 2,400 (1 to
72,000), `army_floor_value` 2,000 in `normal.ron` (0 to 1,000,000; **default 0, no floor**, so the frozen profiles of 9.1c-1 and earlier keep the
production they had). Changed in `normal.ron`: `defend_ticks` 200 → 400, `max_wait` 3,600 → 6,000. Acting from WP9.1c-2: `attack_value`,
`max_wait`, `min_units`, `first_attack_tick`, `defend_ticks`. All hashed by `Profile::digest`.

### Scenarios

**`ai_vs_passive`** (`assets/scenarios/ai_vs_passive.ron`, `ai_vs_passive_defs/`: frozen copies of the shipped definitions and of
`normal.ron` as WP9.1c-3 leaves it (refrozen then; WP9.1c-2's copy is `ai_raid_defs`), and a match file with m1's static opponent exactly as `m1.ron`
lists it). 16,000 ticks (T2). Checksum `0xd39c2261cc0ad09a`, AI digest `0x5817c87452d633f1` (WP9.1c-2: `0x3b5a509e49e43343`,
`0x74e38ea1d028c406`; cause in "Pins that moved (WP9.1c-3)"). Measured and pinned (`the_ai_beats_the_static_opponent_of_m1_before_t2`):

| Wave | Launched | Units | Value | Survivors | Ended |
|---|---|---|---|---|---|
| 1 | 11,150 | 6 | 2,000 | 6 | the win |

(WP9.1c-2: wave 1 at 6,030 with 12 units / 2,000, 5 survivors, ended by `attack_patience` at 8,430; wave 2 at 12,030 with 5 units / 1,100 won at 14,659. Those paths, a wave
that fails and an `attack_patience` end, are no longer run in a scenario; the unit tests `an_attack_ends_with_its_attackers_its_target_or_its_patience_...` and
`an_attack_ends_when_the_target_stops_losing_hit_points_for_attack_patience` hold them.)

**The AI wins at tick 13,839** (the opponent's construction centre falls), T2 = 16,000. Army at the gather cell 0, 4, 0, 0 at 4,000, 8,000,
12,000, 16,000; the last build before the win at 2,890 (the barracks: the list is not complete when the match ends). 18 commands in all,
the most in any 1,000 ticks 4 (bound 15), at most one order per army turn, nothing after the win. The finishing `Attack` is not needed there:
the wave's `AttackMove` fights the centre on its own (the rule has its own unit test).

**The finishing `Attack` on the real world** (fix round 1): `the_finishing_attack_is_sent_on_the_real_world_and_the_simulation_takes_it` runs the
army turn on the world of `ai_vs_passive` with four riflemen of player 0 put idle beside the enemy centre (arrive radius 12 there): it sends one
`Attack` on the centre, the simulation takes it (nothing ignored) and every one of them has that order after the step. The owner filter is
pinned by `the_finishing_attack_never_goes_to_an_own_structure`. **The defence cell** (`the_defence_goes_to_the_nearest_seen_armed_raider_...`:
the nearer of two armed raiders, not an unarmed one, not one 9 cells off the footprint, beside the structure with none) and its fence
(`a_hidden_raider_in_reach_of_the_raided_plant_changes_nothing_until_it_is_seen`: a fourth rifleman in fog at (10, 52), within the 8-cell reach of the plant (fix round 2; it was 16 cells off before), never seen in the run, against
the same far away: commands and digest equal over the whole run). **A defeated AI while the match goes on**: `a_defeated_ai_stops_also_while_the_match_goes_on`
(three players; player 0's centre removed at 6,501, after its first units exist since WP9.1c-3, the match undecided).

**`ai_raid`** (`ai_raid.ron`, `ai_raid_defs/`: the definitions and profile of `ai_vs_passive_defs`, the match file of `ai_scout`): at tick 4,001
the script puts three riflemen of player 1 west of the AI's base and orders them to attack its first power plant. 5,600 ticks (before the
first attack at `max_wait`). Checksum `0x7b8f040e80571984`, AI digest `0x13d1a4466b23765e`. Measured and pinned
(`a_raid_is_answered_within_one_think_...`): first hit 4,022, posture `Defend` from the think at 4,030 (within one think), the eleven army units
ordered with one `AttackMove` at 4,030 (within 40 ticks), last hit 4,582, the raiders dead at 4,587, back to `Gather` at 4,990 (`defend_ticks`
after the last hit, at the next think), and no `AttackMove` or `Attack` after that: the only order left is the gather `Move` back.

**The fence.** `a_unit_hidden_in_the_reach_changes_nothing_the_ai_does_also_an_army_beside_the_route`: three enemy riflemen at the north-east
shore of the lake on the way to the enemy centre against the same three in a far corner: the AI's commands and digest are equal until they come
into view, with the scout (tick 2,110) and without one (tick 7,421, after the first attack went out). *(WP9.1c-3: the cells moved to (57, 28); with the scout the
riflemen come into view at 5,476, without one at 9,823 (one `AttackMove` before), see "The opening".)*

### The game seat (`m1_ai`)

*WP9.1c-3 moved every number in this subsection (first army unit 4,587, the wave of 6 at 9,717, the win at 11,664; the cost figures below were measured
with the WP9.1c-2 timing): see "The opening (WP9.1c-3)". What follows is the WP9.1c-2 measurement.*

`the_army_timing_in_the_game_seat` (`crates/sf_game/tests/ai_in_game.rs`, the AI as player 1 at (72, 14) with 4,000 credits, the human idle with
its construction centre only, driven as the sim thread drives it): the first army unit is queued at **1,147** (WP9.1c-1: 17,867), 10 army units at
the gather cell at tick 4,000, the first wave of **12** leaves at **6,037** (`max_wait`), and **the AI wins at tick 8,396** (the human's centre falls; the
finishing rule is not needed). The goal list stops at its fourth entry (the second power plant, ordered at 1,097) until then: below the floor the units
take the money. After the win nothing is sent. Pinned, with the rule-level asserts (first attack not before `first_attack_tick`, at least `min_units`,
nothing after the match is decided). Pictures: `docs/screenshots/wp9.1c-2/`. In the game a refused AI command is only an `eprintln`; none is sent
after the end. `the_ai_builds_its_base_in_the_game` re-measured: 4 builds of 3 kinds and 12 units queued by tick 4,000, 5 structures stand.

### Cost

`ai_perf.rs`, release, this container, load average 0.4 to 1.0. `ai_vs_passive_with_and_without_the_ai_and_the_tick_of_the_launch`: to the win
(14,700 ticks) **0.42 us per AI per tick, worst think 94 us** (a construction turn); the **step of the tick in which the first wave is launched (tick
6,030) took 874 us against a mean step of 20 us** (the simulation's planning of one `AttackMove` for 12 units, not the AI's think; the second launch, 5
units, 16 us); the whole run 47,454 ticks/s with the AI and 82,645 without (medians of 3 alternating runs; without the AI player 0 builds and makes
nothing, so this bounds the AI and everything it makes, not the think). `what_the_army_turn_costs_with_50_and_200_units_at_the_launch_and_on_the_march`
(20,000 turns each, the observation of `ai_vs_passive` at tick 4,000 with N riflemen added at the gather cell): 50 units 6.6 and 5.1 us on the march (two
runs); **200 units 24.3 and 19.0 us at the launch, 27.5 and 25.8 us on the march** (worst 460 to 1,467 us: spikes of a shared machine, load average
0.4 to 2.2). A second run of the scenario: 0.32 us per AI per tick, worst think 66 us, launch step 882 us against 20.8 us, 43,879 against 75,758 ticks/s. Before the attackers' list was searched by bisection instead of `contains` (first run, same machine, minutes earlier): 7.7 / 8.1 us with 50
and 45.4 / 48.0 us with 200. A turn every 40 ticks: 200 units cost about 0.7 us per tick. Budget: mean 20 us per AI per tick, worst think 1 ms; the
assert is 100 us per turn (release).

### Pins that moved (WP9.1c-2) and why

* **Every AI digest** moved: the profile digest hashes the three new numbers and the state the new parts. **No checksum of an earlier phase
  moved** (`golden.rs`, the `.expected` of every scenario but `ai_scout`). The seven frozen profiles of 9.1a and 9.1b have `unit_mix: []`, so
  production, the army and the threat rule stay off (the new state at its start values, asserted in
  `the_managers_stay_off_for_the_frozen_profiles_...`). Measured: with the new numbers left out of `Profile::digest` and the new state left out
  of `AiState::digest`, the WP9.1c-1 digests of all seven come back (`0xacb2c8e5fb876451`, `0x4dd11cc733dd4b46`, `0x94e97ccd07405cba`,
  `0x5b7d1adaf26aa90f`, `0xf94be57bd5f168e4`, `0xdf9bfa7c2721ec51`, `0x3cba4a139131b094` for `ai_fog`, `ai_base`, `ai_noore`, `ai_boxed`,
  `ai_economy`, `ai_rebuild`, `ai_deadlock`).
* **`ai_scout`** (9.1c-1's own scenario): checksum `0xf2be53002209e2f6` → `0xd6a5c9b3d0bf4aa4`, digest → `0x3c8841f5b7212ab8`. Its frozen
  profile is left as it is (no floor by default; its attack numbers 3,000 / 3,600 / 4 / 2,400 now act): the army attacks the lone centre. Measured
  (`the_ai_scout_repin_of_wp9_1c_2_is_the_attack`): with the trigger made unreachable the old checksum comes back. Consequences in its tests:
  the army at the gather cell at 20,000 is 4 (was 10), and without a scout the AI learns where the centre stands when the first attack arrives
  (15,781), not never.

## The opening (WP9.1c-3)

*Decision of the orchestrator, 12-11-2026: option A of the opening design (the vehicle factory second), data only; fix round 1 the same day: the second
refinery before the first tower and `attack_value` 2,000, both adopted on measurement.* Shipped profile (`assets/ai/normal.ron`):

* **Goal list**, both factions: power plant, refinery, **vehicle factory**, barracks, second power plant, **second refinery**, tower, radar, second tower,
  advanced defences (Veyran: the fortified bunker after the first tower). Pinned: `the_shipped_profile_is_pinned` (`sf_data`) and the resolved list
  (`the_roles_of_the_shipped_profile_become_the_kinds_of_the_shipped_definitions`, `sf_ai`).
* **`attack_value` 2,000** (was 3,000), **equal to `army_floor_value`**: the first wave leaves as soon as the floor's worth of units stands gathered, in any
  seat. Pinned with the equality (`the_attack_goes_at_attack_value_and_not_one_credit_below`).
* **`max_wait` 12,000** (was 6,000): it ends the army floor and is the fallback trigger (below). `army_floor_value` stays 2,000.

The only code change in `sf_ai` is one line unrelated to the opening (the centre recall clears `closing`, below).

**Why the vehicle factory second.** The refinery brings one free harvester; a bought one (1,100 credits, a vehicle) needs the vehicle factory (1,500, needs
the refinery). One harvester delivered about 200 credits per 750 ticks (0.27 per tick), so every 100 credits spent before the second harvester costs about
370 ticks. In WP9.1c-2 the barracks, the second power plant and the scout (1,100 credits) came first and the army floor then spent everything above the
reserve on units, so the vehicle factory waited until 11,650 and the goal list did not complete in 30,000 ticks.

### Measured (ticks)

Shipped profile; `ai_vs_passive` = player 0 on the frozen copy (Meridian), `m1_ai` = the game seat (Veyran, with the fortified bunker in its list).

| | `ai_vs_passive` | `m1_ai` |
|---|---|---|
| vehicle factory ordered | 810 | 817 |
| bought harvester queued | 1,480 | 1,487 |
| second harvester exists | **1,880** | **1,887** |
| second power plant | 2,170 | 2,177 |
| barracks ordered | 2,890 | 3,017 |
| first unit of the mix queued (exists) | 4,460 (4,621) | 4,587 |
| army floor (2,000) reached | 10,687 | 9,374 |
| first wave, on `attack_value` | 11,150, 6 units, 2,000, all 6 survive | 9,717, 6 units, 2,000 |
| the win | **13,839** | **11,664** |
| last structure of the list, attack off | 22,730 | **19,777** |
| WP9.1c-2 for comparison: first unit / first wave / win | 6,030 wave of 12 / 14,659 | 1,147 / 6,037 / 8,396 |

Pinned: `the_vehicle_factory_comes_before_the_barracks_and_the_second_harvester_before_the_first_army_unit` (first five builds, harvester queued 1,480, second
harvester 1,880 and before 4,000, first unit queued 4,460 and after it, floor reached 10,687 and a thousand ticks before `max_wait`),
`the_ai_beats_the_static_opponent_of_m1_before_t2` (win 13,839, the wave, the gather counts, the last build), `the_opening_in_the_game_seat` (the `m1_ai` column
and the credits every 500 ticks from 500 to 11,500), `the_army_timing_in_the_game_seat`, and the two attack-off tests
(`the_goal_list_with_the_floor_and_no_attack_over_thirty_thousand_ticks`, `the_goal_list_in_the_game_seat_with_the_floor_and_no_attack`: the list **completes**,
at 22,730 and 19,777 ticks, the last build being the advanced defences). The credits trace of `ai_vs_passive` under the final profile was not recorded; the
`m1_ai` one is pinned.

### The 20,000 target and the ore of the map

The brief's target, "goal list complete by 20,000 with the attack off", is **met in `m1_ai` (19,777) and not in `ai_vs_passive` (22,730)**. *14-11-2026 (WP9.2a-2): superseded, 20,297 since WP9.2a-2, see "The start-site advantage"; the target is now "by 20,500" (orchestrator decision, PLAN.md).* The cause is the
map's ore, not the AI. Read off `assets/maps/m1/terrain.png` by the sweep of this PR: **174 ore cells in six diamond fields of 29 cells**. A cell starts with
120 (`ore.ron`, inline in `ai_vs_passive.ron`), so a field holds 29 x 120 = **3,480 credits**, and it regrows 1 per cell per 200 ticks, 29 per 200 ticks for a field,
**about 145 per 1,000 ticks**. The trace of the run with the tower first (cumulative ore delivered at ticks 1,000 to 12,000: 0, 200, 800, 1,400, 1,800, 2,200,
2,600, 3,000, 3,400, 3,600, 4,000, 4,200) reads: about 600 per 1,000 ticks while the two harvesters work the field next to the refinery (ticks 2,000 to 4,000), 400
per 1,000 to tick 9,000, 3,400 delivered at 9,000 (about one field's worth), then 200 to 400 per 1,000 ticks, the regrowth plus the crumbs of the next field.
That is the near field mined out at about 9,000 to 10,000. (That the harvesters take the nearest non-empty cell is `economy.md`; the trace was measured, the field
arithmetic and the reading of the PNG are the sweep's.) A second refinery nearer the next field would help the income, which is what the next section tries;
raising the ore `start` only moves the cliff and moves every pinned economy scenario, so it is not done here.

### The second refinery before the first tower, and `attack_value` (fix round 1)

Measured, all with the vehicle factory second, one run each (`SF_` env overrides of the throw-away measurement test, not kept). Columns: floor reached, the first
wave (tick, units, value), the win, and the last structure with the attack off.

| Variant | `ai_vs_passive` floor / wave / win / list | `m1_ai` floor / wave / win / list |
|---|---|---|
| tower first, `attack_value` 3,000, `max_wait` 14,000 (round 0) | 12,567 / 14,030, 6, 2,000 / 16,726 / 22,970 | 13,494 / 14,037, 6, 2,000 / 16,023 / 24,417 |
| **second refinery first**, 3,000, 14,000 | 10,687 / 14,030, 6, 2,000 / 16,682 / 22,730 | 9,374 / 14,037, 6, 2,000 / 15,973 / 19,777 |
| tower first, `attack_value` 2,000, 14,000 | 12,567 / 12,870, 6, 2,000 / 15,576 / 22,970 | 13,494 / 13,757, 6, 2,000 / 15,756 / 24,417 |
| **second refinery first, 2,000, 14,000** | 10,687 / **11,150, 6, 2,000 / 13,839** / 22,730 | 9,374 / **9,717, 6, 2,000 / 11,664** / 19,777 |

The second refinery first completes the list earlier in both seats (22,970 -> 22,730, 24,417 -> 19,777), reaches the floor 1,900 and 4,100 ticks sooner, and the first
wave is neither later nor smaller: **adopted**. `attack_value` 2,000 makes the first wave value-driven (6 units, 2,000, in both seats, leaving as soon as they are
gathered) and the win earlier in both seats: **adopted**. The cost, as before: the tower comes later (ordered at 18,410 with the attack off in `ai_vs_passive`,
15,097 in `m1_ai`; with the attack on the win comes before it).

**`max_wait`** with the final order and `attack_value` 2,000 (`ai_vs_passive`; `m1_ai` is 9,717, 6, 2,000, win 11,664 at every row):

| `max_wait` | first wave | win |
|---|---|---|
| 10,000 | 10,030, 4, 1,200; the floor ends at 10,000, before it is reached (10,687); a second wave at 20,030 | 22,602 |
| 11,000 | 11,030, 5, 1,900 (leaves on `max_wait`, the sixth unit not yet gathered) | 14,131 |
| **12,000** | **11,150, 6, 2,000** (leaves on `attack_value`) | **13,839** |
| 14,000 | 11,150, 6, 2,000 | 13,839 |

With `attack_value` equal to the floor, `max_wait` no longer decides the first wave in these seats; it ends the floor (`production.rs`: the floor applies before the first
attack and before `max_wait`) and is the fallback if the army stalls below 2,000. A wave that leaves on `max_wait` alone carries 2,000 only if the floor is over
after it was reached and the units are gathered: at 11,000 it is 1,900, at 10,000 1,200 (the floor ended unreached). **12,000** is the smallest round value where a
wave leaving on `max_wait` alone would still carry 2,000 (the floor reached at 10,687 and the sixth unit gathered at 11,150 in the slowest seat), with 850 ticks of margin there; the old
value 14,000 gives the same runs. Under WP9.1c-2's order and `attack_value` 3,000 the first wave needed `max_wait` after the floor (one run each, `ai_vs_passive`,
first wave tick / units / value, win): 6,000: 16,510, 4, 1,200, 24,747; 8,000: 17,150, 4, 1,200, 25,519; 10,000: 10,030, 4, 1,200, 26,390; 12,000: 12,030, 4, 1,200, 25,178;
12,600: 12,630, 5, 1,900, 15,605; 13,000: 13,030, 6, 2,000, 15,963; 14,000: 14,030, 6, 2,000, 16,726 (`m1_ai` at 13,000: 4 units, 1,200, win 15,572). That fragility is what
`attack_value` 2,000 removes.

**The gap between waves.** `max_wait` is also the least gap between two waves (`army::attack_due`): a wave goes when the gathered army is worth `attack_value`,
or `max_wait` ticks after the last attack with at least `min_units` gathered. With `attack_value` equal to the floor a second wave goes at once when the army is
worth 2,000 again, and with fewer units only 12,000 ticks after the first (was 6,000 apart at 3,000 / 6,000). Pinned: `later_waves_are_max_wait_apart_unless_attack_value_is_reached`.
In both scenarios the first wave wins, so no second wave is run in a scenario.

### Risk and gaps, stated

* **The known risk, accepted by the decision.** The barracks stands at 2,890 / 3,017 and the first unit of the mix is queued at about 4,460 / 4,587: **an early infantry rush by a human
  beats this opening**; before that the base has its power plants, refinery, vehicle factory and harvesters. From 1,880 on it has two harvesters instead of one. Not tested against a
  rushing opponent: the only opponents are m1's static one and the idle human. The first wave (6 units with light tanks) does beat m1's tower, four riflemen and tank in
  `ai_vs_passive` (6 of 6 survive, the win at 13,839, pinned).
* Smaller floors complete the list sooner but lose the first wave (tower-first order, `attack_value` 3,000, attack off, `ai_vs_passive`, last build; one run each): floor 1,000 at 19,930
  (the first wave then 4 units, 1,200, the win at 27,209), 1,500 at 22,290, 2,000 at 22,970.
* Scenario coverage lost: the failing first wave and the `attack_patience` end of WP9.1c-2 are no longer run in a scenario (unit tests hold them).
* The wins in `m1_ai` and `ai_vs_passive` are later or earlier than WP9.1c-2 (8,396 -> 11,664; 14,659 -> 13,839); only a passive opponent is beaten.

### Side effects that were fixed with it

* The fence test (`a_unit_hidden_in_the_reach_...`) moved its three riflemen from (58, 34) to (57, 28): the wave with tanks takes a way along the lake's north-east shore at y 25
  to 30. With the scout the riflemen come into view at 5,476, without one at 9,823, after the wave left (one `AttackMove` before); printed, not asserted (it asserts equality of commands and digest until
  first sight).
* `the_ai_builds_its_base_in_the_game` looks at 9,000 ticks, not 4,000: the scout, the first unit the barracks makes, was queued at 7,147 and ordered out at 7,317 with the
  first profile of this PR (measured then); the test asserts that a rally point and a scout move exist within 9,000 ticks, not their ticks.
* The two defeated-AI tests (`a_defeated_ai_sends_nothing_and_its_state_stands_still`, `a_defeated_ai_stops_also_while_the_match_goes_on`) now remove the centre at 6,501, when
  the AI has a barracks, units and credits (they removed it at 3,001, before it had a unit). They assert an army unit and a command in 4,000 to 6,501 exist.
* `ai_perf.rs`: the run length stays 14,700 ticks (the win is at 13,839); the army-turn comment says what stands at tick 4,000 now.

### Folded in: the centre recall clears `closing`

In `army::watch`, a hit on the construction centre takes the attackers back without `end_attack`; `closing` (the attackers that were given the finishing `AttackMove`)
then kept their ids (the reviewer's probe of #129 left 4). The recall now clears it next to `target_hp`. Test: `a_centre_recall_leaves_no_attacker_on_the_closing_list` (with the
line removed it fails). `closing` is in the digest; no pinned digest moved because of it.

### Pins that moved (WP9.1c-3) and why

* **`ai_vs_passive`**: checksum `0x3b5a509e49e43343` -> `0xd39c2261cc0ad09a`, AI digest `0x74e38ea1d028c406` -> `0x5817c87452d633f1`; the length stays 16,000 ticks (T2: the win at
  13,839 is inside it; a first version of this PR with the tower first needed 20,000). Its frozen profile (`ai_vs_passive_defs/normal.ron`) was **refrozen** to the shipped
  profile, the only change to a frozen copy's content in this WP (brought in line with the shipped file for the rule the WP changes). **Cause, as a test:**
  `the_repin_of_ai_vs_passive_in_wp9_1c_3_is_the_profile` puts the three changed numbers back (goal list, `attack_value` 3,000, `max_wait` 6,000) on the new frozen copy and gets
  the old checksum and digest exactly (the `closing` fix moves neither).
* **`ai_raid`**: unchanged (checksum and digest, `ai_raid_defs` keeps WP9.1c-2's profile). Two comments were corrected (no content): the first lines of `ai_raid.ron` and
  `ai_raid_defs/normal.ron` said it shared the profile of `ai_vs_passive_defs`, which is no longer true; its checksum and digest pins still hold.
* **sf_ai unit pins** that hash or resolve the shipped profile moved with it: the base-state digest `0x392b1f3256aa3c19` -> `0xaa333a1fd36cb5a9` (it hashes the shipped profile) and the
  resolved goal list.
* **No earlier phase moved**: all CI scenarios pass `--expect` in release (the list is in the PR).
* Tests whose numbers moved: the wave table, `WON`, the gather counts and the last build of `ai_vs_passive`; the army timing, the credits trace and the attack-off list of `m1_ai`.

### Cost (WP9.1c-3, release, this container, load average 3.3 to 3.8: other work ran on the machine)

`ai_perf.rs` re-run: `ai_vs_passive` to the win: **0.29 us per AI per tick, worst think 53.4 us**; **the step of the launch tick (11,150, a wave of 6) 31.7 us against a mean step of 17.7 us**
(WP9.1c-2: 874 us for a wave of 12, one measurement then, not repeated here); the whole run 49,343 ticks/s with the AI and 76,731 without (medians of 3 alternating runs, 14,700 ticks).
One army turn with 200 riflemen added: 22.8 us at the launch, 25.8 us on the march (50: 5.6 and 5.3 us); worst 364 to 1,982 us (spikes of a shared machine). These sit within the
numbers of WP9.1c-2 (0.32 to 0.42 us, 24 to 28 us, 47,454 / 82,645 ticks/s); the machine was busier, so the comparison is rough.

## Groups (WP9.2a)

*13-11-2026.* The army is no longer one wave. `AiState::groups: Vec<Group>` (ascending by id, at most `GROUPS_MAX` = 4) replaces `attackers`, `attack_target`
and `target_hp`; `recalled` and `closing` are per group. **The home is every army unit that is in no group** (`army::is_army`, A1). Code: `crates/sf_ai/src/army.rs`
(`decide`, `reinforce`, `hold_back`, `staging_cell`, `threat_value`, `defenders`, `draw_jitter`, `operations`), `state.rs` (`Group`, `GroupRole`, `GroupStage`), numbers in
`sf_data::ai_profile::ArmyRules`, tests `tools/sf_headless/tests/ai_groups.rs`, `ai_duel.rs`, `ai_perf.rs`, scenario `ai_raid_guard`. Nothing changed in `crates/sf_sim`.

**A group** is `{ id, role (Strike; the raid of WP9.3b is the next number), members (ascending), goal, stage, since, launch_value, staging, merge_into, target_hp, recalled, closing }`.
Stages (the numbers are hashed, so permanent): `Staging` 0, `Advancing` 1, `Engaged` 2, `Returning` 3. `Engaged` is what the AI infers: a member within `arrive_radius` of the
target. `Returning` is a group called back by a hit on the construction centre (decision 3, unchanged: the recall stays anchor-only): its members count as home and get the defence
order once (`recalled`), and the group is dissolved when the threat is over. The strike is the group with no `merge_into`. **A unit is in at most one group**, and no group lives
past the death of its members: `turn` prunes every list to the live units. Tested in every tick of `ai_vs_passive` (`a_unit_is_in_at_most_one_group_in_every_army_turn_of_a_long_run`), in
`ai_raid_guard`, and in 40 army turns of launch, merge, death and recall on built observations.

**G1, the home guard.** At a launch the gathered units are sorted by id; the highest ids are held back until their value reaches `home_guard_permille` (250 in `ai_raid_guard`; 0, off, in the shipped profile) of the gathered value
(`hold_back`: the last unit held may take the held value over the permille, so six units of 2,000 hold back two, a third). The trigger (A4) is then asked of the **strike that remains**: its
count (`min_units`) and its value (`attack_value`, or `max_wait`). A guard that leaves a strike under the trigger launches nothing. In the scenario `ai_raid_guard` (a variant profile with the guard at 250) a hit on a
structure while the army is out meets the guard. **The shipped profile has the guard off (0)**, so in shipped play the "Known gap" of decision 3 stays open (below, and decision 1 of 13-11-2026).

**G2, staging.** With `stage_distance` above 0 the strike gets an `AttackMove` to the staging cell first: the integer point on the line from the gather cell to the target cell,
`stage_distance` cells short of the target (steps on the larger axis, `staging_cell`), moved by the ring search of `beside` (rings 1 to 3, nearest to the gather cell) when it is not open
ground; no staging when the target is no further than the distance. The strike advances (stage `Advancing`, `attack_started = tick`) when `stage_permille` (700) of its **living** members are
`Idle` within `arrive_radius` of the cell, or `stage_patience` ticks after the launch. Dead members leave the denominator. The advance orders the idle members to the target; members still
walking are not ordered again (the anti-spam rule: only an `Idle` unit is ordered).

**G3, reinforcement.** While the strike is `Staging` or `Advancing`, nothing is defended, no reinforcement is out and fewer than `GROUPS_MAX` groups exist, the gathered units minus the guard launch
as a second group (`merge_into` = the strike) once they are worth `reinforce_value` (the shipped number is `attack_value / 2`). It is no attack: `attacks_launched`, `last_attack_tick` and the jitter are
not touched. It walks to the meeting point (the strike's staging cell while it stages, else the middle cell of the strike's members) and merges when its own middle cell is within `arrive_radius` of
it; `launch_value`s add up. No launch while the strike is `Engaged` or `Returning`; one reinforcement at a time (no third group is ever made, the next batch waits for the merge); a reinforcement
whose strike has ended goes home with the survivors.

**G4, partial defence.** Threat value = the sum of `Lens::unit_cost` over the armed units of other players in `obs.visible` within `gather_radius` of the threat cell (`threat_value`; unarmed ones and
structures do not count, nothing out of sight counts). `defenders` sends the units at home, nearest to the threat cell first and the lowest id among equals, until their value reaches the threat value
times `defend_ratio_permille` (1,500) / 1000; **all of home when the ratio is 0 or nothing armed is seen**. Units already idle at the threat cell count towards the amount and are not ordered again.
Members of a `Returning` group always get the order.

**Chunking** (a WP9.2a rule: only when `groups_on()`; with every number of WP9.2a off a list goes as one command, as before, `with_the_numbers_of_wp9_2a_off_a_long_list_goes_as_one_command`). A command carries at most `gather_group` (19, the largest formation of the sim) ids: `operations` splits a longer list into several commands, each counted against the
`ARMY_COMMANDS_PER_TURN` (4) budget, and what the budget leaves goes at the next turn (those units stay `Idle` and unrecorded in `sent`). It is **a bound on the size of a command, not a speed measure**: chunking does not flatten the
launch step (one command of 45 ids: 748 to 849 us; three of at most 19: 801 to 958 us, below); the cost is the sim's path planning per unit, not the command.

**The survivors go home** (fix round 1). With the WP9.2a rules on, the survivors of an attack that ended (and a reinforcement whose strike ended) are put in a `Returning` group whose `recalled` list is who still has to be sent home; without a threat each army turn sends them a `Move` to the gather cell in chunks while the budget lasts, takes the sent ones off the list, and dissolves the group when the list is empty (under a threat the defence order takes them, as for a recall). Before, they got one `Move` in the turn the attack ended, and whoever the chunk size or the budget left out kept its `AttackMove` on the old target while in no group (`every_survivor_of_an_ended_attack_is_sent_home_over_several_turns`). With the numbers off the old single `Move` stays.

**Timing jitter** (decision of 13-11-2026). `timing_jitter_permille` is drawn once per launch from the AI's own stream (`AiState::rng`, `Rng::below(2 * p + 1) - p`) at the first army turn that checks the trigger
after the last launch (the first of the match too), kept in `AiState::jitter`, applied as `(1000 + draw) / 1000` to **both** `attack_value` and `max_wait` of that launch, and cleared by the launch. Number 0
draws nothing (the stream is untouched) and leaves `attack_value` and `max_wait` exactly as written (fix round 1: the floor of 1 on the scaled value applies only with the jitter on, so `attack_value` 0 still means "always"), which keeps every frozen profile. This is the first rule that draws from the AI stream, so **with a jitter above 0** the AI seed changes a game; the shipped profile has 0, so there it does not (`the_ai_seed_alone_does_not_change_a_game` in `tournament.rs` plays 3,000 ticks).

**Stuck rule.** Unchanged. No tournament run of this WP showed an attack ended by `stuck` while it was winning (see the measurements; none was looked for with a tick trace beyond them).

### State, digest, numbers

New numbers (`ArmyRules`, all with serde defaults that switch the rule off): `home_guard_permille` 0, `stage_distance` 0 (`stage_permille` 700 and `stage_patience` 600 are read only with a distance; 600 is a placeholder, not a tuned value; it stays because a profile with some WP9.2a number on but staging off still hashes it (`Profile::digest` writes all the WP9.2a numbers once `groups_on()`), while the frozen profiles, with all of them 0, do not: the shipped profile writes 1800, and since fix round 1 the check refuses a profile with staging on whose `stage_patience` is not below `attack_patience`, `with_staging_on_the_stage_patience_must_be_below_the_attack_patience`), `reinforce_value` 0,
`defend_ratio_permille` 0, `timing_jitter_permille` 0, checked at both ends when the file is read (`every_number_is_checked_at_both_ends_when_the_file_is_read`) and pinned for the shipped file
(`the_shipped_profile_is_pinned`; `a_profile_without_the_numbers_of_wp9_2a_reads_with_them_off` for the frozen copies). `ArmyRules::groups_on()` is true when any is set. **The digest keeps its old layout
when none is set**: `Profile::digest` adds the numbers, and `AiState::digest` adds the groups' full state and the jitter, only when `groups_on()`; otherwise the old fields are hashed from the groups
(`attackers()` is the members of the groups out, `attack_target()` the strike's goal, `recalled` and `closing` the unions), so one group with every number off hashes as the single wave did. That is why no
AI digest and no checksum of the earlier scenarios moved (`the_digest_of_the_base_state_with_the_numbers_of_wp9_2a_off_is_the_old_pin`: the old pin `0xaa33_3a1f_d36c_b5a9` comes back when the numbers are off and one group
is left). The shipped `assets/ai/normal.ron` turns them on, so `the_digest_of_the_base_state_is_pinned` moved (cause: the numbers of the shipped profile and the groups, measured by that test).

### Scenario `ai_raid_guard`

`ai_raid_guard.ron` and `ai_raid_guard_defs/` (a frozen copy of the shipped definitions, a profile `normal.ron` with the guard **on** (250), staging 14 with patience 600, reinforcement, defence 1,500, the attack at 800 value from tick 100, a goal list of the
plant and barracks the match gives the AI so it builds nothing, and a match file of its own): twelve riflemen stand at the gather cell, a power plant is the raid's target, player 1 is a lone centre. The strike (nine, the guard of three held back) launches at about
tick 160, leaves its staging cell at about 800 and is `Engaged` by 1,900; at 1,501 two riflemen of player 1 appear west of the base and attack the plant (slot 1). Pinned: the strike is out at tick 1,700, the guard kills both raiders, the
plant stands, the strike is never `Returning` (`ai_raid_guard_the_army_is_out_and_the_guard_kills_the_raiders`); the control with the guard and the partial defence off ends with a living raider or a lost plant
(`ai_raid_guard_without_the_guard_the_raiders_win`). Checksum `0xbd351313bec96799` (`ai_raid_guard.expected`, in the CI list), AI digest `0x3caf5e5be7833a76`.

### Measured (fix round 1, 13-11-2026, on main with repair rate 1)

**Where.** All numbers below were retaken in the fix round of 13-11-2026 **on the tree of this PR after merging main 48aed03** (PR #132, `hp_per_tick` 1, is on main), with the **8-cycle of start sites** (`sites_swapped`, "AI against AI"). Release, `sf_headless tournament`,
`--assets` a scratch directory with the profile under test, base seed 458,758 (the file's), cap 60,000, three runs in parallel on 4 cores; the binary was built from fa069c7 (later commits change tests and text only). The tournament reads each seat's profile from
`<assets>/ai/<name>.ron`, so a profile per seat needs no flag; `normal_old` is `tools/sf_headless/tests/fixtures/normal_old.ron` (a frozen copy of the profile of d4c06f4; tested equal to the shipped one with the numbers off). Site SW is (12, 68), site NE is (72, 14).

**Duel, normal-new against normal-old: 48 games with the new profile on seat 0 and 48 on seat 1** (96 per row; 48 is six whole 8-cycles, so each seat plays 24 games from each site). `new : old` counts decided games; the share is of the decided games; the control `off` is the new code with every number off. The last two columns split the decided games by the seat and by the site of the **new** profile (new : old):

| Profile (changes against `off`) | new : old | draws | new share of decided | new on seat 0 / seat 1 | new on SW / NE |
|---|---|---|---|---|---|
| off (control) | 13 : 13 | 70 | 50.0 % | 7 : 6 / 6 : 7 | 11 : 2 / 2 : 11 |
| guard 250 + staging 14 (600) + reinforcement 1000 + defence 1500 (the memo's numbers) | 1 : 2 | 93 | 33.3 % | 1 : 1 / 0 : 1 | 1 : 0 / 0 : 2 |
| guard 250 only | 5 : 9 | 82 | 35.7 % | 1 : 5 / 4 : 4 | 5 : 0 / 0 : 9 |
| staging 14 (600) only | 25 : 12 | 59 | 67.6 % | 12 : 4 / 13 : 8 | 25 : 0 / 0 : 12 |
| reinforcement 1000 only | 14 : 13 | 69 | 51.9 % | 8 : 7 / 6 : 6 | 11 : 3 / 3 : 10 |
| partial defence 1500 only | 18 : 16 | 62 | 52.9 % | 11 : 8 / 7 : 8 | 15 : 3 / 3 : 13 |
| staging + reinforcement + defence, guard 0 (patience 600) | 24 : 9 | 63 | 72.7 % | 11 : 4 / 13 : 5 | 24 : 0 / 0 : 9 |
| the same with guard 100 | 3 : 5 | 88 | 37.5 % | 2 : 2 / 1 : 3 | 3 : 0 / 0 : 5 |
| the memo's numbers with guard 150, `attack_value` 1700 | 3 : 7 | 86 | 30.0 % | 1 : 3 / 2 : 4 | 3 : 0 / 0 : 7 |
| the memo's numbers with `attack_value` 1500 | 1 : 3 | 92 | 25.0 % | 1 : 1 / 0 : 2 | 1 : 0 / 0 : 3 |
| **shipped: guard 0, staging 14 with patience 1800, reinforcement 1000, defence 1500, jitter 0** | **34 : 7** | 55 | **82.9 %** | **16 : 5 / 18 : 2** | **31 : 1 / 3 : 6** |

**The ≥ 55 % criterion, per site.** Over both sites the shipped profile wins 82.9 % of the decided games against the old one, and on both seats (16 : 5 as seat 0, 18 : 2 as seat 1). **Per site it does not hold on NE**: from SW it wins 31 of 32 decided games, from NE 3 of 9 (33.3 %). The control shows what the site alone does: with both profiles equal, whoever stood on SW won 22 of the 26 decided games (11 : 2 and 11 : 2), and seat 0 won 14 of 26. So the site, not the seat, is the large effect (the earlier "seat 0 advantage", 72 of 73, was the SW site: seat 0 always started there before the 8-cycle). Staging is what makes the new profile win from SW (25 : 0 and 24 : 0 there with staging, 11 : 2 without); from NE it does not help. Improving the NE side (the gather cell on its own ore, the wave not marching as one body) is WP9.2a-2, not this PR.

**What it says.** (1) **The home guard hurts**: every row with a guard (100, 150, 250 permille, with or without a lower `attack_value`) decided 3 to 14 of 96 games against 26 for the control and won 25 to 37.5 % of them. The rule is built, tested and pinned (`ai_raid_guard`, a variant profile with the guard at 250, shows it killing raiders while the army is out), but **the shipped profile has `home_guard_permille` 0**, so the "Known gap" of decision 3 stays open in shipped play; a separate home army that is not cut from every wave is a candidate for WP9.3a. (2) **`stage_patience` 600 is too short**: a rifleman needs 800 to 1,000 ticks to walk to a cell 14 short of the target, so 600 always ran out first; 1800 is shipped (82.9 % against 72.7 % with 600, the rest equal). (3) **The jitter**, `m1_duel`, new against new, 100 games each, at the shipped `stage_patience` 1800:

| `timing_jitter_permille` | decided | seat 0 : seat 1 | SW : NE | distinct coarse (plain / rounded) | median ticks of decided |
|---|---|---|---|---|---|
| **0 (shipped)** | **76** | 38 : 38 | 74 : 2 | 91 / 98 | 28,338 |
| 100 | 56 | 30 : 26 | 49 : 7 | 75 / 93 | 33,293 |
| 200 | 51 | 25 : 26 | 40 : 11 | 73 / 94 | 33,273 |

Staging already makes the games diverge (91 of 100 coarse outcomes without a jitter), and the jitter lowers both the divergence and the share of decided games, so **0 is shipped**; the rule is pinned by tests and the number can be changed by data. (The jitter does narrow the SW advantage, 74 : 2 to 40 : 11, by deciding fewer SW games, not by winning more NE ones.)

**The final shipped profile, `m1_duel` 100 games:** 76 decided, all won (seat 0 : seat 1 **38 : 38**; **SW : NE 74 : 2**; Meridian 22 and Veyran 20 wins of the 50 games between different factions), 24 capped (0 stalemate, 0 mutual defeat), decided in 15,107 / 28,338 / 58,791 ticks (min / median / max), distinct coarse outcomes 91 of 100 (98 with the last loss rounded down to 1,000 ticks), 85 s for 100 games. Command: `sf_headless tournament assets/matches/m1_duel.ron --games 100 --json m1_duel.json`. Baseline B at rate 1 (PR #132, 4-cycle) was 35 decided, seats 25 / 10.

**Launch step and think cost** (`ai_perf.rs`, release, this container, from the first round of this PR; not retaken: the fix round changes no launch or think path apart from the home `Move`s after an attack): `ai_vs_passive` (a wave of six) shows a launch step of 15 to 19 us with the numbers off and
14.5 to 19.4 us with the shipped ones. A wave of 45 units (`ai_raid_guard` with 48 more riflemen), as one command (`gather_group` 64, the old way) against commands of at most 19: launch step 748 to 849 us against 801 to 958 us (five alternating runs each), staging-exit step 87 to 95 us against 71 to 116 us:
**chunking is a size bound on a command, not a speed gain**; the cost is the per-unit path planning in the sim. Think cost with groups out: `ai_vs_passive`, shipped numbers, 16,000 ticks: mean 0.24 us per AI per tick, worst think 42 us (construction); `ai_raid_guard`: mean 0.21 us, worst 29 us; budget 20 us and 1 ms (asserted in release).

**`ai_vs_passive` with the shipped numbers wins at tick 15,908, 92 ticks before the end of the 16,000-tick scenario** (13,839 with the numbers off), the wave of six leaving at 11,150 in both (`ai_vs_passive_still_wins_with_the_shipped_numbers_and_the_tick_is_pinned`). **Why staging costs about 2,000 ticks here** (traced every 10 ticks on the merged tree): the strike stages from 11,151 to 12,711 (the 70 % threshold, not the patience), then 4 of its 6 units die one after another between 13,071 and 13,351 on the way from the staging cell, and the 2 that reach the centre need until 15,908 to destroy it. With staging off all 6 are at the target at 12,951 and the centre falls at 13,839; with reinforcement off and staging on the trace is the same as shipped. So the cost is not the waiting: the wave leaves the staging cell strung out and arrives piecemeal. That is the "wave marches as one body" fix the seat diagnosis proposes for WP9.2a-2; it is not fixed here, and the scenario was not lengthened. **The 92-tick margin is a known weakness of this pin**: a change that slows the wave by 100 ticks fails it. *14-11-2026 (WP9.2a-2): superseded, the pin is 15,249 and the margin 751 ticks; see "The start-site advantage".*

**Commands:** in the 40,000-tick duel game of `the_commands_of_a_duel_game_stay_at_a_bounded_rate` (game 0, new on seat 0; won by seat 0 at 37,931) the AIs sent 65 and 52 commands, 1.71 and 1.37 per 1,000 ticks; the ceiling is 2.3 per 1,000 (1.71 × 1.3, rounded up to one decimal; it was 3.5, which let a doubling pass).

### Tests and mutations

`ai_groups.rs` (38 tests: the guard value and its boundary, `min_units` after the guard, the staging cell, the staging threshold at 7 of 10 and not 6, the patience at 600 and not 599, dead members out of the denominator, reinforcement at its value and not one unit less, one reinforcement and no third group, the merge at 6 cells and not 7, the strike's middle cell,
no reinforcement while engaged or returning, the guard out of a reinforcement, the defence amount, the nearest-first order and the lowest id among equals, nothing seen sends all of home, the threat value reads only `obs.visible`, the recall of a group only by the centre, the chunk size, the jitter range, determinism and once per launch, its scaling of `attack_value` and `max_wait`, the invariant of one group per unit, `ai_raid_guard` and its control, two fog twins), `ai_duel.rs`,
`ai_perf.rs`, the unit tests of `state.rs` and `profile.rs`, and `sf_data`. Mutations in `army.rs` (each reverted), the tests that fail: guard 0 (`hold_back` ignores the permille): six tests, among them `the_guard_holds_back_the_highest_ids_until_a_quarter_of_the_value_and_not_one_unit_less` and `ai_raid_guard_the_army_is_out_and_the_guard_kills_the_raiders`;
staging off (`staging_cell` returns `None`): eleven, among them `the_staging_cell_is_on_the_line_to_the_target_short_of_it_and_on_open_ground`; defend ratio 0: four, among them `the_defence_sends_units_worth_one_and_a_half_times_the_armed_enemies_seen_nearest_first`; jitter ignored: `the_draw_scales_attack_value_and_max_wait_of_that_launch_and_is_cleared_by_it` and `the_draw_scales_max_wait_too`;
chunk size unbounded: `a_march_command_carries_at_most_gather_group_ids`; the merge never fires: `the_reinforcement_merges_into_the_strike_within_arrive_radius_of_its_staging_cell` and `a_strike_that_has_advanced_is_met_at_its_middle_cell`.

**Fix round 1** added (each with a mutation, reverted, that fails it): `with_the_numbers_of_wp9_2a_off_a_long_list_goes_as_one_command` (chunking without its switch), `attack_value_zero_with_the_jitter_off_launches_as_before` (the floor of 1 also with the jitter off), `every_survivor_of_an_ended_attack_is_sent_home_over_several_turns` (12 survivors, commands of 2, 4 a turn: 8 home in the first turn, 4 in the next; fails when the unsent are dropped and when the old single `Move` is put back), `a_merge_never_lists_a_unit_twice_even_if_the_two_groups_overlap` (fails without the dedup after the merge), `with_staging_on_the_stage_patience_must_be_below_the_attack_patience` (`sf_data`; fails without the check), `the_sites_swap_in_the_second_half_of_an_8_cycle` and `the_start_sites_swap_in_the_world_every_four_games` (the 8-cycle). `check_groups` is extended by `watch_groups`, which also fails on a **lost** unit: a live army unit in no group whose order is still an `AttackMove` onto the goal or staging cell of a group that ended more than 600 ticks ago. It runs in `a_raid_on_the_centre_while_the_strike_is_out_recalls_it_defends_and_loses_no_unit` (`ai_raid_guard` with the shipped WP9.2a numbers and the raiders sent at the centre: a defence call while the strike is out, the recall, no lost unit, raiders dead, centre standing) and, since fix round 2, in `a_long_duel_with_the_shipped_numbers_loses_no_unit`: `m1_duel` game 0 as the tournament plays it (base seed 458,758, factions of game 0, sites not swapped), both seats on the shipped profile, 30,000 ticks (the game is not decided by then), `watch_groups` on both seats every 10 ticks, and it asserts that each seat launched and had a group out (seat 0 launches 3 attacks, seat 1 one; a reinforcement goes out at tick 28,370). It runs in about 8 s in debug. Mutation that it catches: a reinforcement that also lists the strike's members ("unit 30 is in two groups at tick 28370"). It does **not** catch the loss path: every strike in that game ends with its members dead, so there are no survivors to send home, and dropping the unsent survivors or not sending them at all both pass. The loss path is caught by the unit test `every_survivor_of_an_ended_attack_is_sent_home_over_several_turns` (dropping the unsent survivors fails it at line 1385). The invariant test on built observations now also asserts that a second group went out and a reinforcement merged.

### Pins that moved (WP9.2a) and why

Only what the shipped `normal.ron` drives moved; every frozen scenario, every `.expected` and every AI digest of the earlier phases is unchanged (the full workspace tests and the release `--expect` runs of the CI list are green). `the_digest_of_the_base_state_is_pinned`: `0xaa33_3a1f_d36c_b5a9` to `0x8383_758e_5b5e_90e6`
(the old value returns with the numbers off, `..._with_the_numbers_of_wp9_2a_off_is_the_old_pin`). **The merge of main 48aed03 (repair rate 1) moved no pin of this PR**: the workspace tests were green on the merge commit without a pin edit.

* `sf_game`'s `ai_in_game.rs` (the game seat, shipped profile): the win 11,664 to **12,312**, the last structure ordered 3,017 to **12,097**, one more credit sample (12,000). Re-measured on the merged tree: with `stage_distance`, `reinforce_value` and `defend_ratio_permille` set to 0 in `assets/ai/normal.ron`, `xvfb-run -a cargo test -p sf_game --test ai_in_game` fails with exactly the old values, `(3017, Some(4587), [0, 4, 0], 9717, 11664)` and `Some(11664)`; the first wave still leaves at 9,717.
* `tournament_decided.rs`: not re-pinned (1,224 / 2,885, main's). Its scratch profile sets those three numbers to 0. Re-measured on the merged tree: with the three lines removed from `scratch_assets` (`cargo test -p sf_headless --test tournament_decided the_seat_with`, the ticks printed), seat 0 strong wins at **1,345** (1,224 pinned) and seat 1 strong at **1,439** (2,885 pinned). The earlier "1,455 against 1,315" was measured at repair rate 3.
* The 8-cycle changes games 4 to 7 of every eight; no test pinned such a game (`tournament_decided` and the smoke play games 0 to 3).

### Not done in WP9.2a

* The home guard is off in the shipped profile (measured harmful, above): the "Known gap" **stays open** in shipped play; the rule works in `ai_raid_guard` (a variant profile). The jitter is 0 in the shipped profile.
* `ai_vs_passive` wins 92 ticks before its end with the shipped numbers (the strung-out wave after staging, above). *(WP9.2a-2: 751 ticks before its end.)*
* The stuck rule is unchanged and no case of a winning attack ended by `stuck` was looked for beyond the runs above.
* *(14-11-2026: partly addressed in WP9.2a-2, see "The start-site advantage"; the gap shrinks, the map is not fair.)* The SW site advantage (74 of 76 decided games in `m1_duel`, and 3 of 9 for the new profile from NE in the duel) is diagnosed ("AI against AI", the 8-cycle) but not fixed: the gather cell on its own ore and a wave that marches as one body are WP9.2a-2. The 24 capped games are not explained. Staging patience and distance were tuned on a handful of runs, one seed set; the shipped profile was not tuned against the human.
* Raids (`GroupRole::Raid`), retreat and target choice are WP9.2b and 9.3.
* The chunks of one launch go in the same army turn (up to the budget of 4 commands); spreading them over turns was not tried.

## The start-site advantage (WP9.2a-2, 14-11-2026)

**The finding it answers** (seat diagnosis of 13-11-2026): in `m1_duel` the south-west start site (SW, (12, 68)) wins whichever player stands on it (72 : 1, and 1 : 75 with the two `cell:` lines swapped), so the "seat advantage" of WP9.5a was a site advantage. WP9.2a made the tournament alternate the sites; this work package fixes the two causes that sit in the AI. It does **not** make the map fair: the base layout, the distance to the ore and the staging cells stay as they are, and mirroring M1 is Tijmen's decision.

### What was found in the code and in traced games

* **The gather cell of the north-east site (NE) is an ore cell.** `find_gather` took (66, 26), a cell of the AI's own ore field, one cell from where its refinery (65, 21, 3 x 4) goes up. The army gathered among the harvesters (`docs/screenshots/wp9.2a-2/before_m1_ai_t09600_attack_wave_all_views.png`). The cell is chosen at the first army turn, when only the construction centre stands, and was never looked at again.
* **The `gather_radius` skip is real and measured, but it is the small part.** A unit within `gather_radius` (8) of the gather cell counted as arrived and was never sent. The two tanks of the SW vehicle factory come out 7 cells from the cell (24, 64) and stood there when the wave launched: 7 cells from the cell in **63 of 63** SW launches over the first 64 games (NE: 2 to 5, 35 of 64 at 4). They did join the wave (it takes every gathered unit within the radius).
* **The large part is the march.** An `AttackMove` forms no formation (`docs/rules/formations.md`: only a `Move` does), so every unit goes at its own pace. In game 0 of `m1_duel` (traced every 100 ticks) the NE tanks were about 18 to 20 cells ahead of the NE infantry at tick 10,500 and met the SW wave alone: tanks at (37, 39) and (39, 37), infantry at (55 to 58, 28 to 32), and by tick 11,400 the NE army was dead (7 to 1 units, the one left is the scout) while SW had lost one of six. This is one traced game plus the table below, not a measured speed difference between the routes: why the SW tanks stayed with their infantry in that trace was not looked into.
* **So the first clash was lopsided in SW's favour whoever stood there.** Army value left 2,500 ticks after the first launch, first 64 games of `m1_duel` (sum over the games; "more left" counts the games in which a site has more value standing): SW 80,000 : NE 14,400 (SW more in 55 games, NE in 9). The economy then follows [probable; measured as a state, not as a cause: 6,000 ticks after the first launch, first 16 games, rules off, SW has 2 to 3 harvesters and 6 to 7 structures in every game, NE has 0 or 1 harvesters in 11 of the 16 and 3 to 8 structures]; the staging cell of SW, (57, 32), lies a few cells from the NE ore field.

### The two rules (profile numbers, defaults keep the old behaviour)

* `gather_refinery_gap` (0 to 16; **shipped 2**; 0 = off): the gather cell is never an ore cell (`Lens::harvestable`, the terrain, not an entity) and keeps at least that many free cells from every own harvester dock (`BuildingFunction::HarvesterDock`; `placement::gap`). `find_gather` searches rings 0 to 10 round its first choice with the rule on (`GATHER_RINGS_WIDE`, an engine bound; 3 without), in the same order (ring, y, x) as before. `ensure_gather` checks a kept cell against the rule too, so when the first refinery goes up beside a cell chosen at tick 0 the cell is worked out again (NE: (66, 26) -> (65, 25) at tick 200 -> (68, 28) once the refinery stands; SW stays (24, 64)). 
* `gather_everyone` (bool; **shipped true**; false = off): every army unit is sent to the gather cell when it is made, whatever its distance from it. A unit within `gather_radius` that has no entry in `AiState::sent` gets the `Move` like any other; one that was sent counts as gathered (`gathered()` requires the entry; its count of tries starts again at 0 when it stands there), so the wave launches only with units that were sent and stand at the cell. Single-unit `Move`s: in `m1_duel` game 0 seat 0 sends 33 instead of 9 by tick 39,977 (about one more per unit made; the `ai_duel` command-rate bound moved for it).
* Both are in the AI digest only when one is set (tag `0x93`; `Profile::gather_rules_on`), so no frozen profile and no earlier checksum moved by this. `ai_raid_guard_defs/normal.ron` and the other frozen copies are untouched and read both as off.

### What was tried and not shipped: a march leash

A third rule was built, measured and **removed** again: while a group is staging or advancing, a member nearer the leg's target than the rearmost member by more than N cells is stopped (`Command::Stop`) until the body catches up (for at most `stage_patience` ticks of the stage). It evens the first clash (leash 3 alone: army value left 23,600 SW : 26,100 NE in the first 64 games; with the shipped gather rules and leash 3: 31,800 : 23,100) but the games stop being decided: the decided games fall to 8 to 45 of 100 (30 with the shipped gather rules and leash 3, against 48 without a leash), and the SW share of them does not fall (28 : 2 at leash 3). Measured 100-game rows (default seed; "gap/everyone" = the two rules above, leash in cells):

| gap / everyone / leash | decided | SW : NE wins | seats 0 : 1 | undecided |
|---|---|---|---|---|
| 0 / off / 2 | 16 | 14 : 2 | 11 : 5 | 84 |
| 0 / off / 3 | 8 | 8 : 0 | 3 : 5 | 92 |
| 0 / off / 4 | 18 | 17 : 1 | 6 : 12 | 82 |
| 0 / off / 6 | 37 | 37 : 0 | 15 : 22 | 63 |
| 0 / off / 8 | 45 | 45 : 0 | 19 : 26 | 55 |
| 2 / off / 3 | 16 | 15 : 1 | 4 : 12 | 84 |
| 2 / on / 3 | 30 | 28 : 2 | 13 : 17 | 70 |
| 0 / off / 5 | 28 | 27 : 1 | 9 : 19 | 72 |
| 2 / off / 5 | 41 | 41 : 0 | 20 : 21 | 59 |
| 2 / on / 5 | 18 | 15 : 3 | 7 : 11 | 82 |

The preferred route of the brief (the strike waits at the staging cell until `stage_permille` is met) already exists in WP9.2a; the gather rules alone give the even first clash, so the leash was not needed and is not in the code.

### Measured

All numbers: release `sf_headless`, `m1_duel`, sites alternating (8-cycle), the machine idle apart from my own runs (three tournaments at a time on 4 cores for the variants, the final and baseline runs alone).

**First clash** (army value left 2,500 ticks after the first launch, first 64 games, sum over the games; units alive in brackets):

| Rules | SW value (units) | NE value (units) | more value on SW : NE (games) |
|---|---|---|---|
| off (2cacd4b) | 80,000 (250) | 14,400 (52) | 55 : 9 |
| gap 2 only | 81,000 (260) | 5,900 (17) | 61 : 3 |
| everyone only | 35,500 (159) | 36,200 (150) | 27 : 34 |
| **both (shipped)** | **40,200 (184)** | **22,400 (96)** | **37 : 26** |

**Wins per start site and per seat, 100 games each** (decided = someone's construction centre fell within 60,000 ticks; undecided = nobody's did, a few of them flagged as a stalemate). Site 0 is SW. The per-seat columns are the older table form; the per-site ones are what this work package is about.

| Run | decided | SW : NE | NE share of decided | seat 0 : seat 1 | undecided (of them stalemate) | median ticks of decided |
|---|---|---|---|---|---|---|
| base seed 458,758, rules off | 76 | **74 : 2** | 2.6 % | 38 : 38 | 24 (0) | 28,338 |
| gap 2 only | 79 | 73 : 6 | 7.6 % | 37 : 42 | 21 (0) | 19,836 |
| everyone only | 34 | 31 : 3 | 8.8 % | 18 : 16 | 66 (0) | 25,628 |
| **base seed, shipped (both)** | **48** | **38 : 10** | **20.8 %** | **25 : 23** | 52 (1) | 35,055 |
| seed 777, rules off | 78 | 76 : 2 | 2.6 % | 35 : 43 | 22 (0) | 26,855 |
| seed 777, shipped | 45 | 32 : 13 | 28.9 % | 14 : 31 | 55 (0) | 31,873 |
| seed 888, shipped | 46 | 34 : 12 | 26.1 % | 24 : 22 | 54 (0) | 33,716 |

Reading: **the gap shrinks, it does not close.** NE wins 10 to 13 of 100 games instead of 2 (its share of the decided games 2.6 % to 21 to 29 %), the first clash is even, and the wins per seat are about level at two of three seeds (at seed 777 seat 1 wins 31 : 14: a seat or faction effect that was not examined). SW still wins 71 to 79 % of the decided games. **Games are decided less often**: 76 to 48 decided, 24 to 52 undecided; an undecided game holds 11.5 attacks on average (both AIs together, base seed; 10.4 with the rules off), a decided one 4.4 (3.2 off): the AIs keep launching waves that do not finish the game. The distinct coarse outcomes fall from 91 to 69 of 100. Whether that is acceptable (an AI that cannot finish a game against an equal) is not decided here. What decides the games that are decided after an even first clash was not traced; the seat diagnosis pointed at the base layout (SW's refinery and factory sit about 4 cells from its centre on the side the attack comes from, NE's refinery about 8 cells out) and at the staging cells, none of which this work package moves. **The map is not shown to be fair and this is not claimed.**

**Where the wave starts** (the largest Chebyshev distance of a member of the first strike from the gather cell, at the tick it goes out, first 64 games): SW 7 in all 63 launches with the rules off, 1 to 4 (mean 1.7, 61 launches) with them on; NE 2 to 5 (mostly 4) off, 1 to 2 (mean 1.6, 64 launches) on. Pinned for games 0 and 4 in `tools/sf_headless/tests/ai_march.rs`. At the staging exit the spread is small with or without the rules (mean 2.0 SW and 1.8 NE off, 0.9 and 1.8 on, measured on the strikes that reach the exit: 24 of 32 SW and 8 of 32 NE strikes off, 14 and 18 on: with the rules on the NE strikes live to the staging cell). **So at the staging exit the rules do not improve NE** (mean 1.8 off, 1.8 on); the gain on NE is at launch and in that its strikes now reach the staging cell at all. Pinned exactly per game in `ai_march.rs` (`EXIT_SPREAD`, rules on and a rules-off control): only one seat of each game reaches the exit before tick 20,000, so game 0 (SW: 0 on, 3 off), game 4 (NE 1 on; off, its SW seat 4) and game 3 (NE: 1 on, 4 off) are used. These single games show a smaller spread with the rules on; the 64-game means above are the fair comparison.

**What counts as "sent" for `gather_everyone`.** `send` (in `army.rs`) records every unit it orders in `sent`, whatever the order: the gather `Move`, but also a defence or a march order. So a unit that defended and was never moved to the gather cell counts as sent and is not sent again by `gather_everyone`; it is still re-sent by `gather_units` (`ensure_gather` only picks the cell) when it stands idle at home more than 8 cells from the cell (limited by `resend_ticks`). Left as it is: keying `sent` on the gather `Move` only would change the command stream of every game and all pins.

**The long duel is sensitive to profile changes.** `a_long_duel_with_the_shipped_numbers_loses_no_unit` (`ai_groups.rs`, game 6) needs a reinforcement out; it also fails under the mutation that switches the ore check of the gather cell off, because game 6 then loses its reinforcement. A profile change that fails it should move the game, not drop the check.

**`ai_vs_passive`**: wins at tick **15,249** (was 15,908): the margin to the end of the 16,000-tick scenario is **751 ticks, was 92** (13,839 with the WP9.2a numbers off). The wave of six leaves later (11,550, was 11,150: it first has to stand together) and wins sooner after it (3,699 ticks from launch to the win, was 4,758). Cause measured: with `gather_everyone` off 15,908 returns (any gap), with it on 15,249 (any gap); why the arrival is faster was not traced. The scenario was not lengthened.

### Pins that moved and why

All with `gather_refinery_gap` 0 and `gather_everyone` false the old value returns (measured one rule at a time where it matters, below).

* `the_digest_of_the_base_state_is_pinned`: `0x8383_758e_5b5e_90e6` -> `0x4d0a_8207_c799_2fad` (the shipped profile has the numbers); with both off the old value returns (`the_digest_of_the_base_state_with_the_gather_numbers_off_is_the_wp9_2a_pin`).
* `ai_vs_passive_still_wins_with_the_shipped_numbers_and_the_tick_is_pinned`: 15,908 -> 15,249 (above).
* `m1_ai` (`crates/sf_game/tests/ai_in_game.rs`; the AI is the NE seat): the barracks is ordered at 2,977 (was 3,017), the first unit queued at 4,547 (was 4,587), the floor reached at 9,654 (was 9,374), the first wave at 10,037 (was 9,717) and the match won at 12,673 (was 12,312), last structure at 11,657 (was 12,097). One rule at a time: gap only 2,977 / 4,547 / first wave 9,997 / won 12,621; everyone only 3,017 (unchanged) / 4,587 / 9,997 / 12,595; both off: every old value. The first differences come before any army exists (the second power plant at 2,137, was 2,177): the gather cell and its ring are kept free of structures by the placement search (`placement.rs`), so a gather cell that moves moves the cells the later structures take.
* `the_goal_list_in_the_game_seat_with_the_floor_and_no_attack`: the list's last build at **20,297 (was 19,777)**. **The WP9.1c-3 target of 20,000 is no longer met in this seat** (gap only 20,297, everyone only 20,537, both 20,297; gap 1 with everyone 20,017; gap 2 to 4 give the same cell). It is written down here as a known cost; the test asserts the new number and an upper bound of 21,000 instead of 20,000. Not chased by tuning the gap to land under 20,000.
* `an_ai_attack_wave_in_the_games_picture`: the third picture moved from tick 11,500 to 12,400 (the wave arrives at the human's centre later; the centre falls at 12,673).
* `the_commands_of_a_duel_game_stay_at_a_bounded_rate`: bound 2.3 -> 3.1 per 1,000 ticks (measured 2.32 and 1.45; the extra commands are the 24 single-unit `Move`s of the gathering and a game that is no longer decided in 40,000 ticks; rules off: 1.71 and 1.37 in 37,931 ticks as before).
* `the_frozen_profile_is_the_shipped_one_with_the_numbers_of_wp9_2a_off` and `tournament_decided` (`DECIDED` stays 1,224 and 2,885): the fixtures switch the two gather numbers off too (they are about the equality with the frozen profile and about a game decided in a few thousand ticks, not about the gathering; with them on the strong seat 0 wins at 1,480 instead of 1,224).
* `a_long_duel_with_the_shipped_numbers_loses_no_unit` (review nit of #133): it now asserts that a seat had two groups out together (a strike and its reinforcement). In game 0 the gathered wave no longer leaves a reinforcement out before tick 33,761, so the test plays **game 6** (seat 0 has two groups out from tick 24,801), 30,000 ticks. Mutation: `reinforce_value` 0 in the shipped profile makes it fail.
* No pin of an earlier phase moved: `golden.rs`, `wander_5000`, `command_mix`, `terrain_256`, `units_spawn` and every other `assets/scenarios/*.expected` are unchanged (those scenarios use their frozen profile copies); no frozen `*_defs/` copy was changed.

### Tests (each new rule has a mutation, run and reverted)

* `tools/sf_headless/tests/ai_march.rs` (real `m1_duel` games, both sites: game 0 and game 4): `the_gather_cell_is_never_an_ore_cell_and_keeps_its_gap_on_both_start_sites` (at tick 200 and 6,000, the kept cell and the cell worked out again), `the_wave_starts_from_one_place_on_both_start_sites` (pinned `[2, 1]` for both games, bound 4), and the two controls with the rules off: `with_the_rules_off_the_north_east_gather_cell_is_an_ore_cell` ((66, 26), next to the refinery) and `with_the_rules_off_the_south_west_wave_starts_seven_cells_from_the_gather_cell` (`[7, 4]` and `[4, 7]`), plus `the_shipped_profile_sets_both_rules`.
* `tools/sf_headless/tests/ai_groups.rs` (built observations): `with_gather_everyone_a_unit_made_within_the_radius_is_sent_to_the_gather_cell_and_without_it_is_not`, `with_gather_everyone_a_unit_counts_as_gathered_only_after_it_was_sent`, `the_wave_that_launches_with_gather_everyone_waits_for_the_unit_beside_the_cell`.
* `sf_data` (`ai_profile/tests.rs`): the shipped numbers pinned (2, true), the range of the gap in `every_number_is_checked_at_both_ends_when_the_file_is_read`, `a_profile_without_the_numbers_of_wp9_2a_2_reads_with_them_off` (all four frozen copies). `sf_ai`: both numbers in the digest test and the digest pins above.
* Mutations, each failing the named test: the ore check removed -> `the_gather_cell_is_never_an_ore_cell...`; the gap comparison `>= gap` made `>= 0` -> the same test (the gap assertion) and `the_wave_starts_from_one_place...`; `ensure_gather` not checking a kept cell against the rule -> the same two; `gather_units` ignoring the switch -> `the_wave_starts_from_one_place...` (the wave never launches) and three tests in `ai_groups.rs`; `gathered()` ignoring whether the unit was sent -> `..._counts_as_gathered_only_after_it_was_sent` and `the_wave_that_launches_...`; `gather_everyone` false in `normal.ron` -> `the_shipped_profile_sets_both_rules`, `the_wave_starts_from_one_place...` and `the_shipped_profile_is_pinned`; `gather_refinery_gap` 0 in `normal.ron` -> `the_shipped_profile_sets_both_rules`, the ore test, the wave test, `the_shipped_profile_is_pinned` and the range test; the range check of the gap removed -> `every_number_is_checked_at_both_ends...`; either number left out of the digest -> `the_digest_sees_every_number_and_list` and the base-state pin.

### Not done in WP9.2a-2

* The map: base layout, distance to the ore, the staging cells (SW's lies 5 to 8 cells from the NE ore field) and the mirror of M1 stay as they are (Tijmen).
* The games are decided less often (48 of 100, was 76) and the SW share of the decided games is still 71 to 79 %; neither was fixed. A march leash was built, measured (table above) and removed.
* The WP9.1c-3 target of 20,000 for the goal list in the game seat is missed by 297 ticks (above).
* A seat effect at seed 777 (seat 1 wins 31 : 14) was seen and not examined. 100 games per row is a small sample: differences of a few games are noise.
* The staging cells were not looked at (a strike that stages next to the enemy ore field is what starves the enemy), nor a second refinery moving the gather cell again (it can: the cell is re-checked every army turn and the army walks to the new one).
* Not run on the real game with a human: the game seat was looked at in `m1_ai` (idle human) only.


## The schedule

An AI thinks when `(tick + stagger * player) % think_interval == 0` (`schedule::due`; `tick` is the number of ticks the world has run, the
tick the next step runs). Shipped: interval 10 (half a second), stagger 3. So two players never think on the same tick: the profile
check requires `think_interval / gcd(think_interval, stagger) >= 8`, the most players of a match (player 0 thinks at tick % 10 == 0,
player 1 at 7, player 2 at 4, ...). Each think runs the observation, the memory update and **one** manager in rotation, so each of the
four managers (`Economy`, `Construction`, `Production`, `Army`, numbered 0 to 3, which are hashed) runs every 40 ticks. **Construction has
rules since WP9.1b-1** (above), **economy since WP9.1b-2** ("The economy"), **production and the first half of the army since WP9.1c-1**
("Production", "The army in WP9.1c-1"), **the army's threat, attack and finishing since WP9.1c-2** ("The army in WP9.1c-2"; the threat
rule itself runs at every think, `army::watch`).

**The driver.** `Ai::due(tick)` and `Ai::think(&World, &mut Vec<Command>)`, called by the host immediately before `World::step`; the host
schedules the commands for `world.tick()` under the AI's player id, AIs in ascending player id. In `sf_headless` that is `run::run_full`
(`run` is a wrapper; `run_watched` is the same with a look at the world after every step, for tests; `Finished::ai_commands` lists what
the AIs sent). The game's sim thread does not call it yet (WP9.1d).

## The profile (data)

`assets/ai/normal.ron`, read by `sf_data::ai_profile` (`AiProfile::from_ron`, then `check` against the unit definitions) and turned into
kinds for one faction by `sf_ai::Profile::resolve`. One file holds the numbers of the schedule, the economy and the army, and a section per
faction:

* `build_order`: a **goal list** `[(role, count)]` meaning "have at least `count` of `role`". The AI will walk it from the top and build the
  first entry it does not satisfy (WP9.1b), which gives the opening, the tech choice and rebuilding with one rule. Checked: every role
  exists and is open to the faction, is a buildable structure (not the anchor), every `requires` of an entry is earlier in the list
  (the anchor counts as always there), a role that comes again asks for more than before, counts 1 to 32.
* `scout`: (WP9.1c-1, optional, per faction) the role of the scout, checked like a unit of the mix; left out: no scout. `unit_mix` may be
  empty (the AI then makes no units, the frozen profiles of the WP9.1b scenarios).
* `unit_mix`: `[(role, weight)]`, the share of the army each role aims at. Checked: infantry or vehicle (no vessel, no harvester: the
  economy makes those), open to the faction, has a cost, listed once, weight 1 to 1000, the factory of its category and everything its
  `requires` names are in the build list.
* Numbers: `think_interval`, `stagger`; `economy` (`power_margin`, `harvesters_per_refinery`, `reserve`, `max_parallel_builds`,
  `silo_percent`, `repair_below_percent`, `blocked_ticks`, `placement_cap`, `lane`, the aprons, `max_queue`); `army` (`attack_value`, `max_wait`, `min_units`,
  `first_attack_tick`, `defend_ticks`, `gather_distance`, `gather_radius`, `gather_group`, `gather_tries`, `resend_ticks`, `scouts_max`, `scout_tick`,
  `scout_patience`, `scout_retry`; WP9.2a: the numbers of "Groups"; WP9.2a-2: `gather_refinery_gap`, `gather_everyone`, see "The start-site advantage"; WP9.2b-1: `counter_permille`, `intel_ticks`, see "Counter mix (WP9.2b-1)"; WP9.3a-1: `escalate_permille`, `attack_value_max`, `harvester_threat`, see "Escalation and the harvester threat (WP9.3a-1)"). Every one is range-checked when the file is read; the values are placeholders of the memo.
  In WP9.1a only `think_interval` and `stagger` acted; which act since WP9.1b-1 is under "The base".

The shipped Meridian list: power_plant 1, refinery 1, barracks 1, power_plant 2, vehicle_factory 1, defence_tower 1, refinery 2, radar 1,
defence_tower 2, advanced_defences 1; Veyran adds fortified_bunker 1 after the first tower. The ore silo is a rule of the economy
(`silo_percent`), not a list entry. The shipyard and vessels are out of 9.1. All pinned by `the_shipped_profile_is_pinned` (literals) and,
as kinds, by `the_roles_of_the_shipped_profile_become_the_kinds_of_the_shipped_definitions`.

## In a headless scenario

`ai: [(player: 0, profile: "normal.ron", seed: 0xA1_0001)]` (the field table in `tools/sf_headless/src/lib.rs`). The profile is a file in the
`unit_defs` directory (a **frozen copy**, so the shipped profile or definitions can change without moving a pin), the faction and start sites
come from `match_file`, so both are needed; the problems are reported when the scenario loads. A scenario with `game` takes no `ai`.

**`ai_fog`** (`assets/scenarios/ai_fog.ron`, `ai_fog_defs/`): player 0 (Meridian, the AI) has a construction centre and a scout; player 1
(Veyran) is static with a construction centre, a power plant, a barracks and a refinery, and an army in the far corner. The scout walks to
the power plant and barracks, the barracks is removed at tick 500, the scout walks home at 520. Pinned: final checksum `0x768b77651509db8c`
(`ai_fog.expected`), AI digest `0xce7a0b6fb57c9cee` (a literal in `tests/ai_fog.rs`; `0x630c0979a8ae4749` until WP9.1b-2), 800 ticks
(WP9.1a pinned `0xd369218db6025eff` and
`0x993afc7cd2c42cfd`; re-pinned by WP9.1b-1, see "The base"). The tests assert: the AI knows exactly the
two structures the scout saw (kind, anchor, footprint, tick) and not the centre, refinery or army; it forgets the removed barracks and keeps
the power plant after the scout leaves, also when it is destroyed out of sight at tick 700 (same memory and digest as without); without
the scout it knows nothing; the observation at six ticks never holds a shrouded entity;
**an invisible army changes nothing the AI sees or does** (same observation, memory and digest as a world without the army, while the world
checksums differ) and the positive control (a unit inside the scout's sight is seen); the AI thinks 80 times in 800 ticks, manager runs
`[20, 20, 20, 20]`; two AIs think at tick % 10 == 0 and == 7. Since WP9.1b-1 the AI sends exactly its two builds
(`the_ai_sends_its_two_builds_and_nothing_else`), and the invisible army also leaves its commands unchanged.

## Cost

WP9.1b-1 (`ai_perf.rs`, release, this container, load average about 0.6, three runs; the AI's commands are now scheduled in the
measurement, so the AI really builds): `ai_base` 0.12 to 0.13 us per AI per tick (worst think 39 to 54 us), with 5,000 wanderers 6.6 to
7.5 us (worst think 137 to 239 us); `ai_boxed` 0.08 to 0.09 us (worst 25 to 30 us), with the crowd 6.6 to 7.6 us (worst think 136 to
890 us, the 890 in one run of three). At most one placement question per think everywhere (11, 10, 5 and 5 in 700 thinks). One question
in the crowd: 7.8 to 8.1 us. Budget: mean 20 us, worst 1 ms.

WP9.1a:

Measured by `tools/sf_headless/tests/ai_perf.rs` (release, the container of this PR, load average about 2, three runs): in `ai_fog` the mean is
0.05 to 0.07 microseconds per AI per tick (worst think 1.4 to 14 us); with 5,000 wanderers of player 1 over the map (every one a cell
test) the mean is
6.2 to 6.6 us per AI per tick (62 to 66 us per think, worst think 101 to 113 us). The memo's budget: mean at most 20 us per AI per tick, worst at most
1 ms. A whole run of that crowd, alternating, medians of three: 633 ticks/s with the AI against 623 without (noise). The assertions are
loose (ten times the
budget, release only); the numbers are the evidence. The observation walks every entity once per think; a map with far more entities
than 5,000 costs proportionally more, and the managers of WP9.1b and WP9.1c add to this.

## Counter mix (WP9.2b-1, 14-11-2026)

**What it is.** The AI tallies the armed enemy units it sees and leans its unit mix toward the roles that do best against them, per credit. It is **built, tested and off**: the shipped `counter_permille` is 0. The measurement below shows it does not beat a static mix with the rocket trooper doubled. That static mix (and the milder 4 : 3 : 3 and 5 : 3 : 3) wins more against the profile of WP9.2a-2 but decides fewer games against itself than the old mix, so **the shipped mix stays 4 : 2 : 3** (fix round 1, 14-11-2026). The code is in `crates/sf_ai/src/counter.rs`; the one question that reads the public tables is `Lens::counter_score` in `observe.rs` (the only file that names the world).

### The rules

* **Tally** (`counter::tally`, every think, only when `counter_permille` > 0). Per kind: the number of units in `obs.visible` with `owner` not the AI, no footprint and `armed` (a non-empty loadout in the public weapon table). The entry keeps the **peak** count and the tick of it (a count equal to the peak refreshes the tick, a lower one changes nothing) and is dropped `intel_ticks` after that tick: still there at tick + `intel_ticks` - 1, gone at tick + `intel_ticks`. Never tallied: unarmed units, structures (seen or remembered), the AI's own units, and **the scout kind of the profile**. The last is a deviation from the letter of the brief ("armed"): the scout carries a pistol, so it is an armed kind, but the design memo says scouts must not count, and one scout would move the shares of a small army by about a tenth. The tally is in `AiState::intel` (`BTreeMap<Kind, (count, tick)>`). The tally counts every owner other than the AI, neutral included (fine for the 1v1 maps of today). A profile whose scout kind is also a role of the mix is refused at load, since that role would never be tallied.
* **Lens question.** `Lens::counter_score(role, enemy)` is the largest `weapons::weapon_score(weapon, percent)` over the role's loadout, with `percent` the armour table's cell for the weapon's damage type and the enemy kind's armour type (100 for a kind or table the world lacks). A raw number: it is never compared between roles of different price.
* **Share** (`counter::best_role`, `counter::shares`). Each tallied kind's value (count times `unit_cost`) goes to the mix role with the largest score **per credit**, found by cross-multiplication in `u128` (`score_a * cost_b` against `score_b * cost_a`); the earlier entry of the mix wins a tie; nobody wins when every score is 0 (the value still counts in the total, so the shares then add up to less than 1,000). A role's share is its value in thousandths of everything tallied.
* **Weight** (`counter::weight`, `counter::weights`). `base * (1000 + counter_permille * share / 1000)` in thousandths, at most `2000 * base`, **no lower clamp** (a role that is not the counter keeps its base weight). `production::pick_kind` is unchanged apart from its weight type (`u32`); with the rule off the weights are `base * 1000`, so the pick order is the old one (pinned over 729 counts, below).
* **Numbers** (`army:` in the profile). `counter_permille` 0 to 2000, default 0 (off). `intel_ticks` 100 to 60,000, default 3000, read only with the rule on. Both are in `Profile::digest` and the tally is in `AiState::digest` only with the rule on (tag `0x94`), so no frozen profile and no earlier checksum moved by the rule itself.

**The shares on M1** (raw scores from the shipped tables, pinned in `the_lens_scores_every_role_against_every_armour_type_as_pinned`; per credit = score / price, price 100 / 300 / 700):

| enemy armour | rifleman | rocket trooper | light tank | counter |
|---|---|---|---|---|
| infantry (rocket trooper) | 30,000 (300) | 36,000 (120) | 35,000 (50) | **rifleman** |
| light (light tank) | 12,000 (120) | 117,000 (390) | 87,500 (125) | **rocket trooper** |
| heavy (heavy tank) | 3,000 (30) | 90,000 (300) | 74,375 (106) | **rocket trooper** |
| structure (defence tower) | 6,000 (60) | 63,000 (210) | 52,500 (75) | **rocket trooper** |

By raw score the rocket trooper would win against everything; per credit the rifleman wins against infantry, and the light tank wins against nothing, so on M1 the rule is a dial between riflemen and rocket troopers.

### Measured: the counter mix against a static rocket row

**Setup.** Release `sf_headless tournament`, `m1_duel`, cap 60,000, 8-cycle of start sites, base seeds 458,758 (the file's), 777 and 888; one row = 96 games per seed with the profile under test on each seat (48 games with it on seat 0, 48 on seat 1, so 48 games from each site per seed, 144 per site over three seeds). The opponent is the frozen profile after #134 (`tools/sf_headless/tests/fixtures/normal_9_2a2.ron`, not shipped; `the_frozen_duel_opponent_is_the_shipped_profile_with_the_counter_mix_off` ties it to the shipped file). Rows: `ctl` = the new code with the numbers off (mix 4 : 2 : 3), `c1000` and `c2000` = `counter_permille` 1000 and 2000 with `intel_ticks` 3000, `rock` = the static mix 4 : 4 : 3 (rocket troopers doubled) with the rule off. The binary was built from the tree with the scout exclusion (an earlier set of `c1000` and `c2000` rows without it was discarded; `ctl` was played with that first build, where the rule is off and the exclusion cannot act). Rates are **wins as a share of the games played from that site** (draws and capped games count as not won); the verdict is the mean of the two site rates. One seed has about 45 decided games, a **standard error of about 7 points**; the pooled row is 144 games per site (about 4 points).

Two duels per row. **Fixture duel**: the profile under test against the frozen profile (setup above). **Mirror duel**: release `m1_duel` with the profile under test on both seats, 100 games per seed on the same three seeds, cap 60,000; "decided" counts games with a winner. Fix round 1 (14-11-2026) added the rows 4 : 3 : 3 and 5 : 3 : 3 (rule off) and the mirror duel. A row ships only if all four hold, against the control: (1) site-balanced mean at least 27.9 %; (2) SW at least 27.6 % and NE at least 8.2 %; (3) more wins than losses from each site; (4) mirror duel decided at least 124 of 300.

| Row | SW wins of 144 (w/l/d) | NE wins of 144 (w/l/d) | **site-balanced mean** | fixture decided of 288 | **mirror decided of 300** (SW : NE wins) | passes |
|---|---|---|---|---|---|---|
| `ctl` (4 : 2 : 3, off) = **shipped** | 47 (32.6 %) 47/19/78 | 19 (13.2 %) 19/47/78 | **22.9 %** | 132 | **139** (104 : 35) | control |
| `c1000` | 38 (26.4 %) | 70 (48.6 %) | **37.5 %** | 143 | not run | no (2: SW) |
| `c2000` | 37 (25.7 %) | 79 (54.9 %) | **40.3 %** | 152 | not run | no (2: SW) |
| `rock` (4 : 4 : 3, off) | 71 (49.3 %) 71/22/51 | 70 (48.6 %) 70/3/71 | **49.0 %** | 166 | **110** (81 : 29) | no (4) |
| `r433` (4 : 3 : 3, off) | 71 (49.3 %) 71/22/51 | 70 (48.6 %) 70/3/71 | **49.0 %** | 166 | **108** (79 : 29) | no (4) |
| `r533` (5 : 3 : 3, off) | 73 (50.7 %) 73/24/47 | 70 (48.6 %) 70/4/70 | **49.7 %** | 171 | **112** (82 : 30) | no (4) |

Per seed (fixture duel: wins from SW of 48 / from NE of 48 / mean; mirror duel: decided of 100, SW : NE):

| Row | seed 458,758 | seed 777 | seed 888 |
|---|---|---|---|
| `ctl` | 17 / 7 / 25.0 %; mirror 48 (38 : 10) | 15 / 7 / 22.9 %; mirror 45 (32 : 13) | 15 / 5 / 20.8 %; mirror 46 (34 : 12) |
| `c1000` | 18 / 23 / 42.7 % | 13 / 22 / 36.5 % | 7 / 25 / 33.3 % |
| `c2000` | 16 / 24 / 41.7 % | 13 / 25 / 39.6 % | 8 / 30 / 39.6 % |
| `rock` | 23 / 18 / 42.7 %; mirror 32 (22 : 10) | 22 / 26 / 50.0 %; mirror 39 (30 : 9) | 26 / 26 / 54.2 %; mirror 39 (29 : 10) |
| `r433` | 23 / 18 / 42.7 %; mirror 32 (22 : 10) | 22 / 26 / 50.0 %; mirror 37 (28 : 9) | 26 / 26 / 54.2 %; mirror 39 (29 : 10) |
| `r533` | 24 / 18 / 43.8 %; mirror 33 (23 : 10) | 23 / 26 / 51.0 %; mirror 38 (29 : 9) | 26 / 26 / 54.2 %; mirror 41 (30 : 11) |

One seed has a standard error of about 7 points; differences of a game or two between rows are noise. The fixture duel of `r433` has the same wins, losses and draws per site as `rock` on all three seeds, but the games are not the same: 82 of the 288 final checksums differ (19, 31 and 32 of 96 per seed) and two results differ, both on seed 777, where they cancel within each site. With the infantry weights 4 : 3 and 4 : 4 the barracks makes the same units until 4 riflemen and 3 rocket troopers are alive; the eighth infantry unit is the first that differs. In the mirror duels 61 of the 300 final checksums and two results differ. The mirror rows of `ctl` and `rock` are the reviewer's (`ctl` reproduces the 139 of WP9.2a-2); the `r433`/`r533` rows and a check of `ctl`'s first seat (11 / 13 / 24, as before) are fix round 1's.

Wins / losses / draws per site and seat, per seed, are in the PR body.

**Verdict.** (1) The counter mix is **better than the control** on the site-balanced mean (37.5 % and 40.3 % against 22.9 %), but it is **worse than the control from SW** (26.4 % and 25.7 % against 32.6 %), so it fails condition (2). Why it helps from NE and hurts from SW was not traced. It does not beat the static rocket rows either, so `counter_permille` ships at 0. (2) The three static mixes with more rocket troopers (4 : 4 : 3, 4 : 3 : 3, 5 : 3 : 3) pass conditions (1) to (3) by a wide margin but **fail (4)**: against themselves they decide 110, 108 and 112 of 300 games, against 139 for the old mix. More rocket troopers make both sides harder to finish. **So no new mix ships: the shipped mix stays 4 : 2 : 3** (orchestrator decision 14-11-2026, fix round 1), and 4 : 4 : 3 is recorded as measured, not shipped. (3) **The done-when of 55 % site-balanced is not met** by anything shipped (the control is 22.9 %). (4) Not examined: other static mixes, `intel_ticks` other than 3000, `counter_permille` between 0 and 1000, a mix with the tank dropped, the mirror duel of the counter rows.

### Pins

No pin moved in the end. The first build of this work package shipped 4 : 4 : 3 and moved the base-state digest (`0x4d0a_8207_c799_2fad` -> `0x5057_42d7_c905_864d`), the `m1_ai` timings (floor 10,734, first wave 10,957, won 13,413, goal list 21,097) and the third picture (13,100); with 4 : 2 : 3 back every old value returns (digest `0x4d0a_8207_c799_2fad`, goal list 20,297 with the bound 20,500, floor 9,654, first wave 10,037, won 12,673, picture 12,400), and its screenshots were removed. The shipped numbers `counter_permille` 0 and `intel_ticks` 3000 are pinned. Not moved: `golden.rs` and every `assets/scenarios/*.expected` (no `*_defs/` copy was touched).

### Cost

`ai_perf.rs` (`what_the_counter_mix_costs_with_a_visible_army_of_200_units`, release, 200 armed enemy units in sight, rifleman, rocket trooper, light and heavy tank, the fastest of five runs per think so that a container spike is not read as the code): one think mean **7.7 us**, worst **46 us** (builder's run; the reviewer measured 8.7 us and 53 us) (all four managers; the budget is 20 us and 1 ms); the tally alone 0.9 us mean, 1.4 us worst; the weights of one production turn 0.55 us mean, 0.8 us worst. Asserted in release.

### Tests

`tools/sf_headless/tests/ai_counter.rs` (20 tests), each calling the production code: the tally keeps the peak, forgets after `intel_ticks` and not one tick earlier, follows the profile's number, counts only armed units of others (not unarmed units, the scout kind, structures, own units); the raw scores pinned per role and armour; the per-credit counter per armour (rifleman against infantry, rocket trooper against light, heavy and structure; the light tank against nothing); ties go to the earlier entry; the weight formula over its corners; the weights move toward the counter and stop at twice the base; the old pick order over a sweep of 729 count triples with the rule off; the pick leans toward the counter; **composition collapse** (every combination of four kinds at 0, 1, 10 and 100 units and seven strengths: the tank weight is never under its base and its share never under 3/15, the clamp floor, and 19 % of 1,500 picks); the digest only with the rule on; the fog twin (an army out of sight changes no command and no digest with the rule on; the same army in sight is tallied and moves the digest); the production turn queues what the weights say; a tallied kind no role of the mix scores against still counts in the total (the shares then add up to less than 1,000). Mutations, each run on a scratch edit of the source and reverted (`git checkout`), with the test that failed:

| Mutation | Failing test |
|---|---|
| tally counts unarmed units | `the_tally_counts_armed_units_of_others_only` |
| tally counts the AI's own units | `the_tally_counts_armed_units_of_others_only` |
| tally counts the scout kind | `the_tally_counts_armed_units_of_others_only` |
| a kind nobody counters is left out of the total (`total += worth` inside the `if let Some(i)`, fix round 1) | `a_tallied_kind_nobody_in_the_mix_counters_still_counts_in_the_total` |
| expiry one tick late / one tick early | `the_tally_keeps_the_peak_and_forgets_it_...`, `the_tally_follows_intel_ticks_of_the_profile` |
| the last count replaces the peak | `the_tally_keeps_the_peak_and_forgets_it_...` |
| raw score instead of per credit | `per_credit_the_rifleman_counters_infantry_...`, `raw_scores_would_pick_...`, `the_weights_move_toward_the_counter_...` |
| a later entry wins a tie | `a_tie_goes_to_the_earlier_entry_...` |
| no upper clamp | `composition_collapse_...`, `the_weight_formula_is_pinned_over_its_corners`, `the_weights_move_...` |
| non-counter roles pushed below their base | `composition_collapse_...`, `the_weight_formula_...`, `the_weights_move_...`, `with_the_rule_on_the_pick_leans_toward_the_counter` |
| the tally entries are not hashed | `the_numbers_of_the_counter_mix_are_in_the_digest_only_when_it_is_on` |
| the profile digest ignores the numbers | `profile::tests::the_counter_mix_numbers_are_hashed_only_when_it_is_on` |
| production ignores the weights | `the_production_turn_queues_what_the_weights_say_...` |
| the tally runs with the rule off | `with_the_rule_off_nothing_is_tallied_...` |
| the observation shows the fog | `an_invisible_army_changes_no_command_and_no_digest_with_the_counter_mix_on` |
| a range check removed (`intel_ticks`, `counter_permille`) | `every_number_is_checked_at_both_ends_when_the_file_is_read` |

`sf_data`: the two numbers read as off and 3000 in all four frozen profiles (`a_profile_without_the_numbers_of_wp9_2b_1_reads_with_the_counter_mix_off`).

## Escalation and the harvester threat (WP9.3a-1, 14-11-2026)

**What it is.** Two rules, both **built, tested and measured** (the verdict and the shipped values are under "Measured" below). The gate (`9.3a-0`, read-only, `m1_duel`, main 748eac0, 3 x 100 games): in the 161 undecided games the waves stay at 2,000 for ever (11.6 attacks per game against 4.2 in decided ones, the ore does not run out), and the north-east player loses a harvester before its first structure hit in 178 of 300 games (59 %) and wins none of them, because `army::watch` saw hits on structures only. The code is in `crates/sf_ai/src/army.rs` (`need_value`, `attack_due`, `record_bounce`, `reset_bounces`, `harvester_hit`, `watch`) and `state.rs` (the ledger). Nothing in `crates/sf_sim`, and `observe.rs` stays the only file that names the world.

### R2: the bounce ledger and the escalated trigger

* **State** (`AiState`): `bounces` (strikes that failed since the last reset), `bounce_value` (the launch value of the strike that failed last, `Group::launch_value`) and `bounce_tick` (the tick it failed).
* **A bounce** is recorded once per strike, at the army turn the strike ends, read **before** `end_attack` takes the group away: (1) `patience_out` or `stuck` while the target still exists, (2) every member dead (a strike group with no member left) while the target still exists. A strike ends once, so no turn after it sees it again. **Not a bounce:** the target is gone (the enemy's centre and every remembered structure of it are gone and the start sites are cleared), which **resets** the ledger to zero instead (`reset_bounces`), and a recall by a hit on the AI's own construction centre, which sets the groups to Returning and never reaches the end rule. With `escalate_permille` 0 nothing is recorded.
* **The trigger** (`attack_due`, `need_value`): `need = max(jittered(attack_value), bounce_value x (1000 + escalate_permille) / 1000, bounce_value + 1)`, **at most `attack_value_max` when that is above 0**, and the old rule `value >= need` (a wave worth exactly `need` goes). Only with `escalate_permille` above 0 **and** at least one bounce on the ledger; otherwise `need` is `jittered(attack_value)` as before. The `max_wait` fallback counts from the later of `last_attack_tick` and `bounce_tick` (with the rule on), still with `min_units`, so a strike that failed at tick t is followed by a wave of any value at t + `max_wait` at the latest. No deadlock: the cap is reachable (a launch at the cap is always allowed, also when the failed strike was worth more) and the time fallback needs no value.
* **Numbers** (`army:` in the profile): `escalate_permille` 0 to 2000 (default 0, off) and `attack_value_max` 0 to 1,000,000 (default 0, no cap; read, and hashed, only with the rule on; checked: when set it is at least `attack_value`).
* **Digest:** profile tag `0x95` (`escalate_permille`, `attack_value_max`, only when `escalate_permille` > 0) and state tag `0x95` (`bounces`, `bounce_value`, `bounce_tick`, only with the rule on). Tags taken before: `0x92` (groups, WP9.2a), `0x93` (gather numbers, WP9.2a-2, profile only), `0x94` (counter mix, WP9.2b-1).

### The harvester threat

* **Rule** (`army::harvester_hit`, called by `watch` at every think): with `harvester_threat` on, an own unit of a harvester kind (`Lens::harvester_kinds`, from the AI's own `obs.own`) whose hit points fell since the last think, or that is gone (a harvester leaves the observation only by dying), is a hit like a structure hit: `state.threat` becomes the cell of the nearest seen armed enemy near it (`threat_cell`, for a 1 x 1 footprint, else open ground beside the harvester facing the gather cell), the posture becomes `Defend`, and the units at home go by the existing partial-defence rule (`defend_ratio_permille`). The hit points of every own harvester are kept in `AiState::harv_seen` (only with the switch on).
* **Rate limit:** a harvester hit raises a threat only when no structure was hit in the same think **and no defence is running** (`state.threat` is `None`). A threat lasts `defend_ticks` from its last hit, so a harvester that is shot for a thousand ticks raises one defence call per `defend_ticks` and not one per think. One simplification against the brief ("no new threat while a defence **of that area** is running"): any running defence blocks it, not only one at the same cell; a structure hit still renews the running defence as before.
* **Switch:** `harvester_threat` (default false). **Digest:** profile tag `0x96` (the switch, when on) and state tag `0x96` (`harv_seen`, when on).

### Measured: escalation and the harvester threat against the frozen profile of #135

**Setup.** Release build of the tree of this PR, the harness `tools/sf_headless/tests/ai_rows.rs` (`#[ignore]`d; its game loop is `tournament::play_seeded_sited`'s with a read of the AI's own observation added), `m1_duel`, cap 60,000, the 8-cycle of start sites, base seeds 458,758, 777 and 888. One duel row = 96 games per seed (48 with the profile under test on seat 0 and 48 on seat 1, so 48 games from each site per seed, 144 per site over three seeds) against the frozen profile after #135 (`tools/sf_headless/tests/fixtures/normal_9_2b1.ron`, mix 4 : 2 : 3, counter mix off, none of the new numbers; `the_frozen_duel_opponent_is_the_shipped_profile_after_wp9_2b_1_without_the_new_numbers` ties it to the shipped file). One mirror row = `m1_duel` with the profile under test on both seats, 3 x 100 games. Rates are **wins as a share of the games played from that site** (a draw, a capped game and a stalemate count as not won); the verdict is the mean of the two site rates. Rows: `ctl` = the numbers off (the shipped profile), `r250` = `escalate_permille` 250 with `attack_value_max` 8,000, `r500` = 500 and 8,000, `harv` = `harvester_threat` on, `both` = `r250` + `harv`, `both500` = `r500` + `harv`. The machine was shared with other agents' builds (load average 5 to 10 on 4 cores), so wall times mean nothing; the games do not depend on it.

**Check of the harness.** `ctl` reproduces the numbers of the previous work package game for game: SW 47 of 144 (47 / 19 / 78 w/l/u), NE 19 of 144 (19 / 47 / 78), mean 22.9 %, mirror 139 decided of 300 (104 : 35; 48, 45, 46 per seed), and the gate's figures: 11.65 attacks per undecided mirror game against 4.22 per decided one, and the NE player losing a harvester before its first structure hit in 86 of its 144 duel games (59.7 %), winning none of them.

| Row | SW wins of 144 (w/l/u) | NE wins of 144 (w/l/u) | **site-balanced mean** | mirror decided of 300 (SW : NE) | attacks per undecided mirror game (both AIs) | NE: harvester lost before the first structure hit (games, wins in them) |
|---|---|---|---|---|---|---|
| `ctl` (off) = **shipped** | 47 (32.6 %) 47/19/78 | 19 (13.2 %) 19/47/78 | **22.9 %** | **139** (104 : 35) | 11.65 | 86 of 144, 0 |
| `r250` | 48 (33.3 %) 48/19/77 | 18 (12.5 %) 18/45/81 | 22.9 % | 137 (102 : 35) | 8.63 | 89, 0 |
| `r500` | 48 (33.3 %) 48/19/77 | 18 (12.5 %) 18/44/82 | 22.9 % | 141 (106 : 35) | 8.59 | 88, 0 |
| `harv` | 49 (34.0 %) 49/11/84 | 19 (13.2 %) 19/39/86 | 23.6 % | **111** (84 : 27) | 11.35 | 90, 0 |
| `both` | 48 (33.3 %) 48/11/85 | 19 (13.2 %) 19/36/89 | 23.3 % | **115** (84 : 31) | 8.66 | 98, 1 |
| `both500` | 48 (33.3 %) 48/11/85 | 18 (12.5 %) 18/36/90 | 22.9 % | **117** (86 : 31) | 8.63 | 94, 0 |

Per seed (wins from SW of 48 / wins from NE of 48):

| Row | seed 458,758 | seed 777 | seed 888 |
|---|---|---|---|
| `ctl` | 17 / 7 | 15 / 7 | 15 / 5 |
| `r250` | 20 / 7 | 14 / 6 | 14 / 5 |
| `r500` | 21 / 7 | 14 / 6 | 13 / 5 |
| `harv` | 16 / 7 | 16 / 7 | 17 / 5 |
| `both` | 19 / 8 | 14 / 6 | 15 / 5 |
| `both500` | 20 / 7 | 14 / 6 | 14 / 5 |

Per seat (wins of 144 with the profile under test on seat 0 / seat 1): `ctl` 33 / 33, `r250` 34 / 32, `r500` 32 / 34, `harv` 34 / 34, `both` 33 / 34, `both500` 31 / 35. Mirror decided per seed (458,758 / 777 / 888): `ctl` 48 / 45 / 46, `r250` 48 / 47 / 42, `r500` 50 / 47 / 44, `harv` 33 / 41 / 37, `both` 38 / 40 / 37, `both500` 39 / 42 / 36. One seed has about 45 decided games, a standard error of about 7 points; the pooled site rate (144 games) about 4 points.

Attacks per undecided game as the AI under test launches them in the duel (mean per game, one AI): `ctl` 5.71 (156 games), `r250` 4.27, `r500` 4.19, `harv` 5.51, `both` 4.25, `both500` 4.18; in the mirror per AI: 5.82, 4.32, 4.30, 5.67, 4.33, 4.31.

Median launch value per wave number in the **undecided mirror games** (both seats; wave number = `attacks_launched` after the launch; waves behind each median: 313 to 378 for waves 1 to 3, and for wave 4 to 5 `ctl` 306 / 275, `r250` 295 / 143, `r500` 293 / 126, `harv` 349 / 310, `both` 338 / 171):

| Row | wave 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
|---|---|---|---|---|---|---|---|---|
| `ctl` | 2,000 | 1,200 | 2,000 | 2,000 | 2,000 | 2,000 | 2,000 | 2,000 |
| `r250` | 2,000 | 1,200 | 2,000 | 3,000 | 3,000 | 2,650 (2 waves) | | |
| `r500` | 2,000 | 1,200 | 2,000 | 3,000 | 3,000 | 2,300 (1 wave) | | |
| `harv` | 2,000 | 1,200 | 2,000 | 2,000 | 2,000 | 2,000 | 2,000 | 2,000 |
| `both` | 2,000 | 1,200 | 2,000 | 3,000 | 3,000 | 2,000 (7 waves) | | |

**Verdict (the ship rule of the orchestrator, 14-11-2026): no row ships.** A rule ships when (1) the site-balanced mean is at least the control's plus 5 points (27.9 %), (2) each site is at least its control minus 5 points and (3) wins exceed losses on each site, and the mirror keeps at least 124 decided games (control minus 15; target 169).

* **R2 (`r250`, `r500`) fails (1) and (3).** The mean does not move (22.9 % in both rows, the control's), from NE wins stay below losses (18 : 45 and 18 : 44, control 19 : 47), and the mirror decides 137 and 141 (the floor 124 holds, the target 169 is missed by 28 to 32). It does what it says to the waves: the median launch rises from 2,000 to 3,000 at waves 4 and 5 and the undecided mirror games hold 8.6 attacks instead of 11.65, the games just do not end more often. Why the larger waves do not decide more games was not traced. What the log shows: the median stops at 3,000 (the cap is 8,000, so the cap does not bind), and few waves go beyond the fifth (143 fifth waves in `r250`, 126 in `r500`, against 275 in `ctl`), as each wave now waits for a larger army.
* **The harvester threat (`harv`) fails (1) and (3) and the mirror floor.** The mean is 23.6 % (+0.7), NE wins 19 of 144 against 39 losses, and the mirror decides **111** of 300, under the floor 124 (`both` 115, `both500` 117 likewise). The threat does what it says in the scenario (a defence call at the first hit, before any structure is hit), but in the duels the north-east player still loses a harvester before its first structure hit in 90 of 144 games (control 86) and wins none of those, and fewer games are decided (SW: 19 losses become 11 and the undecided games rise from 78 to 84; mirror 111 against 139). Neither effect was traced; the threat only changes what the units at home do after a harvester is hit.
* **So `escalate_permille` ships at 0, `attack_value_max` at 0 and `harvester_threat` at false.** Both rules are built, tested and off. Every pin of the earlier phases stands.

**What the data leaves open (not measured here):** `escalate_permille` above 500 or a cap above 8,000 (the median wave stops at 3,000, so the income, not the cap, bounds it); the retreat of 9.3a-2 (the gate showed it fires with at least 30 % of a failed strike alive in 28 to 32 % of failed strikes, never in a winning strike); a defence for the harvesters that goes where the raiders are before the first hit (an escort or a guard at the ore), not after it; why the NE harvesters are lost at all (in 86 of 144 control games the first loss comes before any structure is hit; who kills them, and where, was not looked at).

### Pins and re-pins

None moved: the shipped numbers are `escalate_permille` 0, `attack_value_max` 0 and `harvester_threat` false, so the base-state digest (`0x4d0a_8207_c799_2fad`), `golden.rs`, every `assets/scenarios/*.expected` of the earlier phases, the `m1_ai` timings (goal list 20,297 with the bound 20,500) and `units_spawn` stand; `the_digest_of_the_base_state_is_pinned` and the whole `cargo test --workspace` run are the evidence. New pins: the two scenarios below, the digest literals of the rules on (`the_bounce_ledger_and_the_harvesters_seen_are_hashed_only_when_their_rules_are_on`), and the profile numbers (`the_shipped_profile_is_pinned`, `every_number_is_checked_at_both_ends_when_the_file_is_read`, `a_profile_without_the_numbers_of_wp9_3a_1_reads_with_both_rules_off`, `the_cap_may_not_sit_below_the_first_wave`).

### The scenarios

* **`ai_bounce`** (`assets/scenarios/ai_bounce.ron`, frozen copy `ai_bounce_defs`): the AI (1,200 credits, a barracks, a refinery and one harvester; riflemen only; attack value 800 from tick 100; escalation 500 permille, cap 3,000) against a static defence (six defence towers, six riflemen and a light tank round the centre). Pinned (`each_failed_strike_makes_the_next_launch_half_as_big_again_and_the_ticks_and_values_are_pinned`): launches `(tick, value)` (1,630, 800), (7,590, 1,200), (17,630, 1,800); failed strikes `(tick, value, bounces)` (3,790, 800, 1), (9,870, 1,200, 2), (20,110, 1,800, 3); each launch is exactly the escalated need of the one before. The control on the same scenario with `escalate_permille` 0 (`without_the_escalation_the_second_wave_leaves_at_once_with_less`): launches (1,630, 800), (3,790, 800), (9,710, 800), (14,270, 800), (18,750, 800): the second wave leaves 3,800 ticks earlier and 400 credits smaller.
* **`ai_harvest_raid`** (`assets/scenarios/ai_harvest_raid.ron`, frozen copy `ai_harvest_raid_defs`; the site is the south-west one, the rule is symmetric): the AI with a barracks, a refinery, two harvesters and six riflemen at its gather cell, no scout; two riflemen of the other player, standing in the far corner, are ordered at tick 1,000 to attack harvester 4. Pinned (`a_hit_on_a_harvester_raises_a_defence_call_before_any_structure_is_hit`): the first threat at tick 1,510 at the raider's cell (20, 80) with every structure still at full hit points, the first `AttackMove` of the AI the same tick to that cell, both raiders dead at 1,860, the harvester at 788 of 800 at the end. The control with the switch off (`the_control_without_the_switch_never_defends_and_the_harvester_keeps_losing_hit_points`): no threat in 6,000 ticks, no `AttackMove`, both raiders alive, the harvester down to 382.

### Tests

`tools/sf_headless/tests/ai_groups.rs` (the rules one by one on observations the test builds: `the_launch_needs_the_failed_value_plus_the_escalation_and_not_one_credit_less`, `the_cap_stops_the_growth_and_a_launch_at_the_cap_is_always_possible`, `the_max_wait_fallback_counts_from_the_failed_strike_when_that_is_later`, `a_strike_that_runs_out_of_patience_is_a_bounce_recorded_once`, `a_strike_whose_members_are_all_dead_is_a_bounce_recorded_once`, `with_the_rule_off_nothing_is_recorded_and_the_ledger_is_not_read_or_hashed`, `a_target_that_is_gone_ends_the_strike_without_a_bounce_and_clears_the_ledger`, `a_recall_by_a_hit_on_the_centre_is_no_bounce`, `the_escalated_trigger_holds_a_gathered_wave_back_that_the_old_rule_would_send`, and for the threat `a_hit_on_an_own_harvester_is_a_threat_at_the_cell_of_the_nearest_armed_enemy_seen`, `a_harvester_that_is_gone_is_a_hit_too`, `a_hit_on_a_unit_that_is_no_harvester_is_not_a_threat`, `with_the_switch_off_a_hit_on_a_harvester_is_nothing`, `a_harvester_being_shot_raises_one_threat_per_defend_ticks_and_not_one_per_think`, `a_structure_hit_in_the_same_think_comes_first`, `the_harvester_threat_calls_the_units_at_home_to_the_raider`, and the fence twin `a_raider_that_is_not_seen_changes_nothing_until_it_is`, on two real worlds that differ by a raider out of sight), `tools/sf_headless/tests/ai_escalate.rs` (the two scenarios, their checksums and digests, the duel fixture), `crates/sf_ai` (digest literals), `crates/sf_data` (the numbers, their ranges and the cap check). The measuring harness `tools/sf_headless/tests/ai_rows.rs` is `#[ignore]`d and not part of CI.

## Not done

* **WP9.3a-1 gaps.** Both rules are built and off: no row passed the ship rule (R2 leaves the site-balanced mean at 22.9 % and NE wins below losses; the harvester threat costs 22 to 28 decided mirror games, 111 of 300 against a floor of 124). Not traced: why the larger waves of R2 do not decide more games, why the NE harvesters are lost before any structure is hit (86 of 144 duel games) with or without the call, and why the call costs decided games. Not measured: `escalate_permille` above 500, a cap above 8,000, a defence of the ore field before the first hit. A harvester threat in a defence that is already running (a structure hit elsewhere) does nothing: any running defence blocks it, not only one at the same cell. `ai_harvest_raid` is on the south-west site only (the rule is symmetric; the NE site is in the duels). The retreat is WP9.3a-2.
* **WP9.2b-1 gaps.** The counter mix is off: it did not beat the static rocket row (49.0 % against 40.3 % site-balanced), and it was worse than the control from the SW site (25.7 to 26.4 % against 32.6 %); why it helps from NE and hurts from SW was not traced. Not measured: other static mixes, `intel_ticks` other than 3000, `counter_permille` between 0 and 1000, a mix without the tank, the new roles of WP9.5b (heavy tank, siege artillery and the faction units are buildable but not in the mix). Nothing remembered is tallied (structures, or units that left sight, beyond `intel_ticks`). The 55 % site-balanced aim is not met: the shipped mix is the control (22.9 %); the static mixes 4 : 4 : 3, 4 : 3 : 3 and 5 : 3 : 3 reach 49.0 to 49.7 % but decide too few mirror games (110, 108, 112 of 300 against 139) and are not shipped.
* **Choice of an economy target (the dropped WP9.2b-2, 14-11-2026).** Not built: the siege test was true at 1 of 2,461 launches on `m1_duel` (towers stand 9 to 12 cells from the centre, outside radius 8; an armed structure is remembered at only 46 % of launches; every economy structure is within 6 cells of the centre), so it would never act; its budget goes to WP9.3a, and it needs economy structures placed away from the centre, a defence memory that survives to launch and cover measured per target before it returns.
* **WP9.1c-2 gaps.** The army floor costs the economy: on M1's 4,000 credits the goal list does not complete in 30,000 ticks without an attack
  ("The army floor", the money). One group at a time (no second wave while one is out; units made meanwhile gather); no retreat, regroup, focus fire or
  target priority beyond "the construction centre first" (9.2/9.3); while the whole army is out, a hit on a structure other than the centre is
  answered only by the towers (decision 3, "Threat"); the finishing `Attack` is tested on an observation the test builds, not in a scenario (in
  `ai_vs_passive` the wave's `AttackMove` brings the centre down on its own); the defence is a cell, not a unit, so raiders that run are
  not chased beyond what an `AttackMove` meets; `ai_vs_passive` and `m1_ai` are won against a passive opponent only, no opponent that rebuilds or
  answers. The army floor stalls the goal list until the army reaches it (on `m1_ai` the list stops at its fourth entry until the AI has won at
  8,396). Defences are placed only towards the other players' start sites.
* **WP9.1c-1 gaps.** No army before tick 15,020 (`ai_scout`) or 17,867 (`m1_ai`) on M1's credits (see "Production"; the army floor of WP9.1c-2 answers it). A scout that dies on the way is replaced once
  and not told where it died; the scenario's enemy has no gun, so a scout under fire is not run. The gather cell is not checked for a route from
  the base (no `plan_route`): a cell behind rock shows only as units idle far away, which are ordered `gather_tries` times and then left. A refused unit
  order is read as accepted when any new unit of the same kind appears within the check (out of another factory, or an earlier item of the same
  factory finishing), so the refusal goes unseen.
  The AI digest hashes the AI's state, which is transient latches and memory that settle: a changed decision may show only in the world
  checksum (measured: the vehicle-factory rally point forced back on gives other checksums at ticks 4,000, 10,000 and 20,000 of `ai_scout`
  and the same AI digest at all three). (The army defends since WP9.1c-2.)
* Not checked: whether a lane of one cell is wide enough for harvester traffic (bodies are 1.5 cells); whether `Despawn` of a structure
  leaves the same state as destruction by weapons: `ai_rebuild` removes the refinery and the barracks with `Despawn` and only damages the plant
  by fire, so a structure shot to pieces and rebuilt is not run.
* **No income, no recovery after the rescue.** A refinery lost while the credits are zero is ordered unfunded and waits for money that cannot
  come (no harvester can unload). The rescue ("The rescue") pays for it once per match; a second deadlock in the same match is not rescued
  (`ai_deadlock`, the loss at tick 8601: the refinery is ordered, paid as far as the credits go and stands unfinished to the end of the run).
  Found while building `ai_rebuild`, where 4,000 credits ran out this way; that scenario starts with 6,000. The silo is built when the store
  is full, never to rescue a store that cannot fill.
* The silo rule counts the starting credits as ore (credits are the store): with more credits than 90 percent of the first refinery's 2,000
  the store is "full" at once, and the silo comes as soon as the goal list is closed.
* The ore guard is the terrain of the whole map, not the reach of a harvester: ore ground that exists but is out of every harvester's
  search radius still gets harvesters (they are ordered, they idle). The amounts of ore are not known to the AI.
* Not run: a repair of a structure that is destroyed while repairing, a silo with two refineries in a scenario (only in the unit test
  `at_most_one_silo_per_refinery_and_one_going_up_counts`), a faction other than Meridian (the economy has no faction rule except the open
  faction of the silo and the harvester).
* **A second harvester factory** (two vehicle factories) is tested only with an observation the test builds
  (`a_harvester_goes_on_the_factory_with_the_shortest_queue_...`),
  not in a scenario: the goal list has one.
* `refused` after a sent build only matters when the mask cannot see why the simulation said no; the one test of it is the unit that
  drives off (above).
* Difficulty levels (WP9.4) and the "unfair" level and its two cheats. AI-vs-AI exists since WP9.5a (below); the tuning it measures is WP9.5b.
* The AI state is plain data but is not saved anywhere (WP13.1).
* A shuffled spawn order test (the iteration-order check of the memo) is not built: the AI iterates `BTreeMap`s and ascending vectors only, and the
  determinism script forbids hash collections, but no test shuffles.

## In the game (WP9.1d)

The game runs the AI that a match file gives a player. Code:
`crates/sf_game/src/sim_thread.rs` (`SimHandle::spawn_with_ais`, `Sim::think_ais`), `matchplay.rs` (`ais`), `runner.rs` (`match_scene`), the
field `controller` of `sf_data::match_def::MatchPlayer` (`docs/rules/match.md`, "Controllers"). Data: `assets/matches/m1_ai.ron`. Tests:
`crates/sf_game/tests/ai_in_game.rs`, `ai_screenshot.rs`, the unit tests of `matchplay.rs` and `match_def.rs`.

* **One driver for game and headless.** The sim thread holds one `sf_ai::Ai` per AI player, ascending by player id. In every tick, **immediately
  before `World::step`**, each AI whose `Ai::due(world.tick())` holds calls `Ai::think(&world, ..)` and the commands are scheduled for the tick
  about to run under the AI's own player id through the thread's own `schedule`, the path a human's command takes (`sf_headless::run::run_watched`
  does the same in the same order). So they are in the command log, in `--record` and in the checksum. The AI never sees anything but the world
  it is handed, through its fenced observation (above).
* **Making the AIs.** `matchplay::ais(def, defs, assets)` reads `<assets>/ai/<profile>.ron` for every player with `controller: Ai(profile: ..)`,
  checks and resolves it for the player's faction against the unit definitions, and tells every AI the start sites of the match (`sf_ai::start_sites`)
  as `Scenario::load_ais` of `sf_headless` does; since WP9.5a the match-to-AIs loop lives in `sf_ai` (`sf_ai::match_ais`, beside `make_ai` and
  `start_sites`: the host hands it a function that reads a profile's text, because `sf_ai` reads no file) and `matchplay::ais` is a thin wrapper that reads
  `<assets>/ai/<profile>.ron` for it; `sf_headless tournament` calls the same function ("AI against AI (WP9.5a)"). `Scenario::load_ais` of
  `sf_headless` does **not** use it: it reads the `ai:` specs of a scenario (profile file in the `unit_defs` directory, its own seed per entry), not
  controllers, and keeps its own loop around `sf_ai::make_ai`. So `make_ai` is shared by all three hosts, `match_ais` by two. The AI's seed is the match's `seed` (pinned: two seeds give two digests, `the_match_seed_reaches_the_ai_...`). `spawn_with_ais` sorts the AIs by player id; `two_ais_think_in_ascending_player_order_...` gives two AIs, made due on the same ticks, in swapped order and checks the order in the log. A missing profile or one that does not fit is an
  error that names the player and the profile and stops the start. `--replay` does not run the AIs (the record holds their commands).
* **The match file.** `assets/matches/m1_ai.ron`: the map and the human as in `m1.ron`, player 1 Veyran with one construction centre on the cell of
  `m1`'s static centre (72, 14) and **4000 credits, the human's own start**: the AI is not given more than the player it is watched against; the
  refinery its goal list builds brings the free harvester and the ore the rest. Start it with `sf_game --match m1_ai` (a bare name is looked up under
  `assets/matches`; `--match assets/matches/m1_ai.ron` is the same). `m1.ron` is unchanged and still the default.
* **What the AI does there** is what its managers do (construction WP9.1b-1, economy WP9.1b-2: harvesters, silo, repair, the once-per-match rescue
  sale). Since WP9.1c-1 it also buys units (a scout from tick 600, then the unit mix; WP9.1c-1: once its goal list is done), sets rally points and sends the scout
  and the gathering army `Move` orders (since WP9.1c-2 the units come first, below the army floor, not after the goal list); **since WP9.1c-2 it attacks** (the army floor gives it an army early; the first wave goes at `max_wait`), so
  an idle human loses to it (`m1_ai`: first wave of 12 at tick 6,037, the human's construction centre falls at 8,396; "The army in WP9.1c-2", "The
  game seat"). The commands it may send are `BuildStructure`, `QueueUnit`, `SetRally`, `Repair`, `Sell` and `Order { Move | AttackMove | Attack, not
  queued }`; the test lets nothing else through and checks that builds and queued units both occur. Re-measured after WP9.1c-2 (the army floor
  takes the money first): by tick 4000 4 builds of 3 kinds, 12 units queued, 5 structures stand. Measured on the sim thread after the merge with #126 (`ai_in_game.rs`, `ai_screenshot.rs`; the human idle; printed, not asserted):
  first build at tick 17 (think 0 is the economy manager's turn); 1 structure at tick 2, 4 at tick 1000, 6 at tick 2500 and 6 at tick 4000; by tick 4000 5
  builds of 4 kinds and 1 unit queued (before the economy: 7 structures at 2500, 8 at 4000, 7 builds of 6 kinds, the harvesters now cost money). Re-measured after WP9.1c-1: the same
  (5 builds, 4 kinds, 6 structures at tick 4000; no sale). Asserted: at least 3 kinds of structure are ordered and at least 4 structures stand (the
  construction centre included) by tick 4000, which is what the rule promises (power plant, refinery and barracks, 1900 of the 4000 credits); the
  count in the picture test does not drop while no `Sell` has been sent.
* **Thinks per tick.** The profile's `think_interval` 10 and `stagger` 3 give player 1 the ticks 7, 17, 27 ...: 100 thinks in the first 1000 ticks
  (pinned: `the_ai_thinks_when_it_is_due_...`).
* **The cost of a think** is part of the sim tick (`step_once` starts its clock before the thinks) and is counted on its own:
  `SimCounters::thinks`, `think_micros`, `think_max_micros`, and in the hitch log per slow tick and as totals (`docs/rules/hitch_log.md`). Measured after the merge with
  the economy manager (2,000 thinks, relative numbers, shared container): sim thread release, mean 1-2 us, worst 59-94 us, 1.1-1.3 % of the step time (two
  runs); sim thread debug, mean 36 us, worst 984 us, 1.3 %; real game window under Xvfb (debug, 401 thinks) mean 0.097 ms, worst 1.04 ms. The profile's
  budget is 1 ms a think; nothing enforces it.
* **A record replays without the AI.** The AI's commands are in the record, so `sf_headless FILE` gives the same checksum at every tick without
  running it (`a_recorded_match_with_an_ai_replays_without_the_ai_to_the_same_checksum_at_every_tick`); and the sim thread and
  `sf_headless::run::run_full` with the same AI give the same checksum at every tick
  (`the_sim_thread_and_the_headless_host_run_the_ai_to_the_same_checksums`).
* **Not done in WP9.1d.** No AI state in the game's save (WP13.1); no difficulty choice (the profile name in the file is the only choice, WP9.4); the
  restart from the menu makes a fresh AI with the world (`a_restart_equals_a_fresh_start` runs it for `--match m1_ai`: the thinks of the restarted game equal a fresh start's); the
  F1 panel shows no AI numbers; the AI has no sight of what the human's fog hides, so it cannot find the human's base, as designed (it knows only the start
  sites).

## AI against AI (WP9.5a)

The measuring tool for the rest of phase 9: `sf_headless tournament <match.ron> [--games N] [--cap TICKS] [--seed S] [--json PATH] [--assets DIR]
[--stalemate TICKS]` (`tools/sf_headless/src/tournament.rs`; tests `src/tournament/tests.rs` and `tests/tournament.rs`). It plays a match file whose
two seats are both computer opponents, many times, and prints a table. Nothing in `crates/sf_sim` changed; the AI is the one the game runs
(`sf_ai::match_ais`, moved here from `sf_game::matchplay::ais`, which now calls it), the world is the one the game makes (the shipped assets
through `scenario::game`, speed 1.5 as in the game, pinned as `GAME_SPEED_RAW` = 98,304).

* **The match** `assets/matches/m1_duel.ron`: `m1_ai` (map `m1`, a construction centre and 4,000 credits per seat, the south-west and north-east start
  sites) with `controller: Ai(profile: "normal")` on both seats. **Headless only**: the game refuses an AI on the local seat
  (`the_game_refuses_the_duel_match_and_the_ais_make_both_seats`). A tournament takes exactly two seats, both computer-controlled.
* **What varies from game to game, on purpose.** (1) The seats: game `i` puts the file's two factions on the two start sites as written, swapped, the first
  on both, the second on both (`factions_for`, cycling with period 4; mirrors included). Since the fix round of WP9.2a (13-11-2026) games 4 to 7 of every eight play the same four seatings **with the two start sites swapped** (`sites_swapped`: the two players' start objects change places, credits, faction and profile stay with the seat), so the schedule is an 8-cycle and every seat and faction plays from both sites; the table has a `sites` column, the summary counts **wins per site** (site 0 = seat 0's site in the file, the south-west one (12, 68) in `m1_duel`; site 1 the north-east) and the JSON has `sites_swapped` per game and `site_wins`. The reason is a measured site advantage: the south-west site wins whichever player sits on it (a diagnosis of 13-11-2026: swapping the two `cell:` lines of `m1_duel` turned 72 : 1 for seat 0 into 1 : 75 for seat 1). `the_start_sites_swap_in_the_world_every_four_games` checks it in the world: the owner of the centre on (12, 68) in game k is not its owner in game k + 4. (2) The world seed and (3) the AI seed, from `--seed` (the file's
  seed when left out) and the game number through SplitMix64 (`seeds_for`; pinned, and `mix(0)` is the published first output). The AI seed feeds each AI's
  stream (`AI_STREAM` plus the player number) and so its digest, but **no rule draws from that stream yet** (checked by reading: the only draw outside the
  digest is in a test, `ai.rs`), so it does not change a game, and a test shows it: `the_ai_seed_alone_does_not_change_a_game` plays one game twice with
  only the AI seed changed (same final checksum and outcome, different AI digests, which hash the stream) and once with only the world seed changed
  (different checksum). What separates games today is the seating and the world seed. **The AI stream was not drawn when this was written**; since WP9.2a the timing jitter is the first rule that draws, and it draws only when `timing_jitter_permille` is above 0: the shipped profile has 0, so the AI seed still changes no shipped game.
* **When a game ends.** After the first step with a `match_result`, or after `--cap` ticks (default 60,000): a capped game is a draw. It is a **stalemate**
  when no unit or structure was destroyed in its last `--stalemate` ticks (default 10,000; a game shorter than the window cannot be one), otherwise
  **capped**. Both defeated in the same tick is a **mutual defeat**. `classify` is a pure function and pinned at the boundary (exactly the window is a stalemate).
* **The numbers.** Per game: world and AI seed, the factions on the seats **read back from the world that was played** (`World::faction_of`) and from each AI's profile (`Profile::faction`, new in
  this WP, not part of the digest); `prepare` refuses a game in which the two disagree, and `the_world_and_the_ais_have_the_factions_of_the_schedule_and_the_games_speed`
  asserts both against the schedule, so a seat swap that does not reach the world or an AI fails. Then the ending, the defeat tick, ticks, the last loss, the final
  checksum, each AI's digest and `attacks_launched`. **Tick numbers are all counts of ticks run**, as `World::tick()` after a step: `ticks` is the number of steps,
  `defeat_tick` is the same count at the step that decided the match (so equal to `ticks` in a decided game; `MatchResult::tick` itself is the step's own number, one
  lower, which a first version of this tool mixed up: its 100-game table showed `ticks 51156, defeat 51155`), and `last_loss` is the count after the last step in which
  something was destroyed, so `ticks - last_loss` is the number of loss-free steps at the end, which the stalemate window is compared with. Aggregates, all
  whole numbers (permille, printed with one decimal): wins per seat over all games, wins per faction over the games between **different** factions (a mirror
  says nothing about the factions), draws split into mutual defeat / stalemate / capped, min / median / max duration of the **decided** games (the median of
  an even count is the rounded-down average of the middle two), the number of **distinct final checksums**, and, since the fix round, the number of **distinct outcomes** (the keys `(ending, ticks, last loss, attacks per AI)`; and the
  same with both AI digests), since fix round 2 the number of **distinct coarse outcomes** `(ending, ticks, attacks per AI)` and the same plus the last loss
  rounded down to 1,000 ticks, overall and per seating, and min / median / max of the last loss. A final checksum differs from the first spawn on (the world seed
  draws spawn facing), so 100 of 100 checksums would hold even for 100 strategically identical games. The exact outcome key is nearly as weak: it holds the last loss
  to the tick, and capped games all have the same `ticks`, so one death one tick earlier already makes it distinct. The coarse keys ignore that tick-level noise
  and are the ones to judge divergence by. The digest key is different for every game by construction (the digest hashes the AI's random stream, which the AI seed sets), so it is printed because
  it was asked for and not to be judged by. The table and the JSON have no wall time; wall time goes
  to stderr and into the JSON's last key `host` (so `jq 'del(.host)'` leaves the part that must be equal between runs). The same arguments give the same table
  (`four_games_are_decided_or_labelled_and_the_table_is_the_same_on_a_second_run`; same machine, same build; a Linux/Windows comparison is not made and
  is not claimed).
* **A decided game in CI** (`tests/tournament_decided.rs`): the smoke games are capped long before a base falls, so a fixture decides one in a few thousand ticks. A
  match file in a scratch directory: one seat has a construction centre, 4,000 credits and twelve light tanks beside its centre, the other a construction centre and
  nothing else; the profile is the shipped `normal.ron` with `attack_value`, `max_wait` and `first_attack_tick` set low in a scratch copy (the numbers are replaced by
  key, so a later change of the shipped values does not break it). Pinned: seat 0 strong wins at 1,224 ticks, seat 1 strong at 2,885 (1,315 and 5,546 at the repair rate 3, 5,599 before WP9.1c-3: the fixture uses the shipped opening and the shipped repair rate; WP9.1e's 3 to 1 moved both, and with the rate put back to 3 on the merged tree the 1,315 and 5,546 return), `defeat_tick == ticks ==
  last_loss`, and a second game with the factions swapped is still won by the same *seat*. A change of unit or weapon numbers moves the two tick pins (a re-pin).
  Measured while building it: **four** tanks beside the centre, on this fixture, destroyed nothing in 20,000 ticks (about 100 shots per 1,000 ticks); twelve end it. That was the free
  centre repair at 3 hit points a tick; WP9.1e measured it and set the rate to 1 (below).
* **CI.** The 4-game smoke runs as a normal test in debug with a cap of 9,000 ticks (the smoke plays its 4 games three times: two command-line runs and one through the library; the whole test file
  took 37 s in this container, debug build, loaded machine); the 100 games are a manual run in release.

### Baseline A: 100 games, `m1_duel`, normal against normal, before WP9.1c-3 (release)

Main at 1586e1c plus this WP (fix round 1; the first version's table differed only in `defeat` being one lower). Baseline B, after WP9.1c-3, is below.

`sf_headless tournament assets/matches/m1_duel.ron --games 100` (cap 60,000, seed 458,758 = the file's), release build, one thread, this container with a
load average of about 4 (other agents' cargo runs): **149.1 s for 100 games (fix round; the first run 153.4 s), 1.2 to 2.0 s a game (median 1.5 s)**, i.e. roughly 40,000 ticks/s for two AIs
(the design memo guessed 10,000 to 20,000).

| | |
|---|---|
| Decided | 7 of 100: all seven won by **seat 1** (the north-east site), ticks 47,956 to 58,960, median 51,156 |
| By seating | Meridian v Veyran: 0 of 25 decided (1 stalemate, 24 capped); Veyran v Meridian: 4 of 25 (seat 1 = Meridian won all four); Meridian v Meridian: 3 of 25 (seat 1); Veyran v Veyran: 0 of 25 |
| Per faction (different factions only, 50 games) | Meridian 4 wins (8.0 %), Veyran 0 wins (0.0 %) |
| Per seat (all 100 games) | seat 0 0 wins (0.0 %), seat 1 7 wins (7.0 %) |
| Draws | 93: 0 mutual defeat, **1 stalemate** (game 8, last loss at tick count 41,239), 92 capped |
| Attacks launched | 1 to 9 per AI per game, mean 5.2 |
| Distinct final checksums | **100 of 100 games** |
| Distinct outcomes, exact (ending, ticks, last loss, attacks) | 100 of 100 games, 25 of 25 in each seating (with digests: 100, by construction) |
| Distinct coarse outcomes (ending, ticks, attacks) | **27 of 100**; per seating M v M 14, M v V 14, V v M 13, V v V 10 of 25 |
| The same plus the last loss rounded down to 1,000 ticks | **51 of 100**; per seating M v M 22, M v V 17, V v M 20, V v V 16 of 25 |
| Last loss | min 41,239 / median 59,031 / max 60,000 ticks |

### Baseline B: 100 games, `m1_duel`, normal against normal, after WP9.1c-3 (release)

Measured on the WP9.1c-3 branch with main 1d210ee (WP9.5a) merged in, same command, cap and seed as Baseline A, release build, one thread, this container
(loaded machine): **136.7 s for 100 games, about 1.3 to 1.4 s a game**.

| | |
|---|---|
| Decided | 8 of 100: seat 1 won 7, **seat 0 won 1** (game 42, Meridian v Meridian); ticks 44,712 to 57,274, median 48,063 |
| By seating | Meridian v Veyran: 1 of 25 decided (seat 1 = Veyran); Veyran v Meridian: 3 of 25 (seat 1 = Meridian); Meridian v Meridian: 2 of 25 (one per seat); Veyran v Veyran: 2 of 25 (seat 1) |
| Per faction (different factions only, 50 games) | Meridian 3 wins (6.0 %), Veyran 1 win (2.0 %) |
| Per seat (all 100 games) | seat 0 1 win (1.0 %), seat 1 7 wins (7.0 %) |
| Draws | 92: 0 mutual defeat, 0 stalemate, 92 capped |
| Attacks launched | 1 to 10 per AI per game, mean 5.75 |
| Distinct final checksums | 100 of 100 games |
| Distinct outcomes, exact (ending, ticks, last loss, attacks) | 100 of 100 games, 25 of 25 in each seating (with digests: 100, by construction) |
| Distinct coarse outcomes (ending, ticks, attacks) | **31 of 100**; per seating M v M 17, M v V 12, V v M 14, V v V 12 of 25 |
| The same plus the last loss rounded down to 1,000 ticks | **53 of 100**; per seating M v M 21, M v V 17, V v M 21, V v V 15 of 25 |
| Last loss | min 44,712 / median 58,859 / max 59,988 ticks |

Against Baseline A: one more game decided (8 against 7), decided games end earlier (median 48,063 against 51,156), the first seat-0 win, the stalemate is gone,
slightly more divergence (coarse 31 against 27, rounded 53 against 51). The seat-1 advantage stays. One run each, so these are observations, not a significant difference.

What it says: today's normal AI does not finish a game against itself. 96 of the 100 games had a unit or structure destroyed at or after tick 50,000: the armies keep trading,
the bases keep being rebuilt (game 0, looked at every 5,000 ticks with a debug print that is not in the PR: 7 to 28 entities per side), and the construction centre
falls in 7 games, always the north-east one. Whether the north-east site is easier to take or the south-west site's approach is worse is not measured here
(seven decided games are too few to say; the seat table of a larger run, and 9.5b, will). Divergence: the exact keys are all distinct (100 checksums, 100 outcome
keys), but both are moved by tick-level noise from spawn facing. On the coarse keys only **27 of 100** games differ in (ending, ticks, attacks per AI) and **51 of 100**
with the last loss rounded to 1,000 ticks, at or below the design memo's condition of "about half distinct". *13-11-2026 (orchestrator): the seeded jitter is therefore
needed and goes into WP9.2a as the profile number `timing_jitter_permille` (default 0 for frozen profiles, set in the shipped normal profile); its effect is measured
there on the coarse key.* *Superseded by WP9.2a decision 3 (13-11-2026, orchestrator): the jitter is shipped at 0; see "Groups" and PLAN.md.*
`assets/ai/normal.ron` is being changed by WP9.1c-3 at the same time, so these numbers are a **baseline of main at 1586e1c plus this WP**, not a result for
the final profile.

### After the repair rate (WP9.1e): Baseline A and Baseline B with `hp_per_tick` 1

The world reads the compiled-in `repair.ron`, so the rows below were taken by editing the shipped file and rebuilding (a `--assets` copy cannot carry the rate).

| Tree | `hp_per_tick` | decided of 100 | per seat | median ticks | per faction (50 games) | stalemate / capped | distinct coarse (plain / rounded last loss) |
|---|---|---|---|---|---|---|---|
| Baseline A (main 1d210ee, before WP9.1c-3) | 3 | 7 | 0 / 7 | 51,156 | Meridian 4, Veyran 0 | 1 / 92 | 27 / 51 |
| Baseline A tree | 2 | 14 | 1 / 13 | 50,608 | Meridian 6, Veyran 1 | 0 / 86 | 32 / 51 |
| Baseline A tree | 1 | 59 | 0 / 59 | 43,276 | Meridian 18, Veyran 12 | 0 / 41 | 69 / 83 |
| Baseline B (main d4c06f4, with WP9.1c-3) | 3 | 8 | 1 / 7 | 48,063 | Meridian 3, Veyran 1 | 0 / 92 | 31 / 53 |
| **Baseline B tree + this WP (shipped)** | **1** | **35** | **25 / 10** | **39,632** (min 15,569, max 55,332) | Meridian 9, Veyran 13 | 0 / 65 | **54 / 75** |

**The real Baseline B at rate 1: 35 decided games of 100** (4.4 times Baseline B at rate 3, which had 8), not the 59 of the first look on the tree without WP9.1c-3. Decided by seating: Meridian v Veyran 7,
Veyran v Meridian 15, Meridian v Meridian 9, Veyran v Veyran 4 of 25 each; seat 0 wins 25 and seat 1 wins 10 (the seat-1-only pattern of both earlier baselines is gone). Attacks launched 0 to 9 per AI per
game, mean 4.9. The two changes interact (59 before the opening, 35 after it, same rate), which is one observation each and not explained here. What this does not say: the remaining 65 capped games are
not explained (towers, rebuilds and the army size are untouched), and whether the 35 decided games are decided well is not looked at. One run each, so observations, not a significant difference.

The fixture behind the rate (`tools/sf_headless/tests/repair_rate.rs`) is a **best case for the attackers**: tanks in range and never shot at, no towers, a defender with 0 credits. The attack
has no "kill within `attack_patience`" rule; it ends on `stuck` (no new hit-point low of the victim for 2,400 ticks) or when no victim is in sight for that long. Measured as the longest
stretch without a new low: at rate 3 it is 39,549 to 39,829 ticks for 2 to 5 tanks (stuck fires), at rate 2 the same for 2 and 3 tanks, at rate 1 it is 31 to 40 ticks for every tank count from 2 to 12
(stuck never fires). Tables and the choice of 1 over 2: `docs/rules/repair_sell.md` "The rate and the wave".

### Baseline causes (all [Gissing], from a read-only sweep; input for 9.2a, none measured here)

* Waves are too small to kill a base: the trigger is `attack_value` 3,000 or `max_wait` 6,000 with `min_units` 4 (`army.rs`, `attack_due`), against towers and a rebuilding
  goal list; `attack_patience` 2,400 then sends survivors home (the `stuck` rule: no new hit-point low of the victim for that long), so the armies trade for ever. (Consistent with the fixture above: four tanks do not kill a
  repairing centre, twelve do.)
* A free, fast centre repair out-heals small waves: the construction centre has no `cost`, so its repair is free; `repair.ron` had
  `hp_per_tick` 3, and the AI repairs below 70 %. Four light tanks deal about 2.1 hp/tick against the centre, twelve about 6.3. **Measured and changed in WP9.1e**
  (below, and `docs/rules/repair_sell.md` "The rate and the wave"): the rate is 1 now.
* Veyran never wins as the attacker and is never beaten in a mirror: its list carries an extra `fortified_bunker`, which makes its base harder and its economy later.
  Meridian wins only from seat 1.
* Seat 1 wins only: a site asymmetry (the approach or the `beside` target cell at (12, 68) against (72, 14), the distance to ore) or the think order (player 0 always
  thinks first, `stagger` 3). A map mirrored in code would separate the two.
* The army floor is limited to before the first attack and before `max_wait` (`production.rs`), so it is unlikely to be the long-run stall.

### Not done in WP9.5a

* No tuning and no pass criterion: no test says who must win or how often (a test that required the normal AI to lose would block every later improvement); the
  smoke asserts that every game is decided or labelled, consistent with its numbers, and reproducible.
* No threads: the games are independent and could fan out, but one thread gives 100 games in 2.5 minutes.
* The 100-game run is not in CI. The numbers above were measured once; they are not asserted anywhere.
* Faction win rates exclude mirror games and so rest on half the games; a seat's win rate in a mirror game is the whole seat effect (plus noise).
* Only two seats, and only one profile name per match file is read per seat; a profile that differs per seat works (each seat's controller is read) but no
  test plays two different profiles against each other yet (that is the tool 9.2a needs: "normal-new against normal-old").

