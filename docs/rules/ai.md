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
nothing but the towers (no unit is at home, and the attackers are not called back).

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
`the_goal_list_in_the_game_seat_with_the_floor_and_no_attack`). That is the cost of the floor on a 4,000-credit start, stated as a gap. *(WP9.1c-3: closed by the opening, the list now completes at 22,970 and 24,417; see "The opening".)* The guard against the known risk (under steady losses the army stays below the
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
the second wave's `AttackMove` fights the centre on its own (the rule has its own unit test).

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
riflemen come into view at 6,356, without one at 14,674, see "The opening".)*

### The game seat (`m1_ai`)

*WP9.1c-3 moved every number in this subsection (first army unit 7,387, the wave of 6 at 14,037, the win at 16,023; the cost figures below were measured
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

The brief's target, "goal list complete by 20,000 with the attack off", is **met in `m1_ai` (19,777) and not in `ai_vs_passive` (22,730)**. The cause is the
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
  to 30. With the scout the riflemen come into view at 5,476, without one later, after the wave left; printed, not asserted (it asserts equality of commands and digest until
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
  `scout_patience`, `scout_retry`). Every one is range-checked when the file is read; the values are placeholders of the memo.
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

## Not done

* **WP9.1c-2 gaps.** The army floor costs the economy: on M1's 4,000 credits the goal list does not complete in 30,000 ticks without an attack
  ("The army floor", the money). One group at a time (no second wave while one is out; units made meanwhile gather); no retreat, regroup, focus fire or
  target priority beyond "the construction centre first" (9.2/9.3); while the whole army is out, a hit on a structure other than the centre is
  answered only by the towers (decision 3, "Threat"); the finishing `Attack` is tested on an observation the test builds, not in a scenario (in
  `ai_vs_passive` the second wave's `AttackMove` brings the centre down on its own); the defence is a cell, not a unit, so raiders that run are
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
* Difficulty levels (WP9.4), the "unfair" level and its two cheats, AI-vs-AI (WP9.5).
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
  as `Scenario::load_ais` of `sf_headless` does; the code is copied (about ten lines) and not shared, because that function is private to the
  scenario loader and `sf_ai` is not touched by this work package. The AI's seed is the match's `seed` (pinned: two seeds give two digests, `the_match_seed_reaches_the_ai_...`). `spawn_with_ais` sorts the AIs by player id; `two_ais_think_in_ascending_player_order_...` gives two AIs, made due on the same ticks, in swapped order and checks the order in the log. A missing profile or one that does not fit is an
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
