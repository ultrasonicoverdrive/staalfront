# The computer opponent

WP9.1a (the frame), WP9.1b-1 (the base) and WP9.1b-2 (the economy). Design memos of 10-11-2026 and 11-11-2026 (orchestrator, Fable);
Tijmen: "fase 9 mag eerder
beginnen". WP9.1a builds what every later part stands on: the AI **observes through the fog, remembers what it saw and keeps its schedule**.
WP9.1b-1 adds the first manager with rules, **construction** (the goal list, the power rule, the money rule, placement), so the AI builds
its base. WP9.1b-2 adds the **economy** manager (harvesters, the ore silo, repair; a lost structure is rebuilt by the goal list). Production
and army are WP9.1c and still stubs. This page says what exists and what is a stub.

Code: `crates/sf_ai/src/` (`observe.rs`, `memory.rs`, `state.rs`, `schedule.rs`, `profile.rs`, `sites.rs`, `ai.rs`, and since WP9.1b-1
`budget.rs`, `base.rs`, `placement.rs`, and since WP9.1b-2 `economy.rs`), the profile format `crates/sf_data/src/ai_profile.rs`, the data
`assets/ai/normal.ron`, the driver
`tools/sf_headless/src/run.rs` (`run_full`, `run_watched`), the scenarios `assets/scenarios/ai_fog.ron`, `ai_base.ron`, `ai_noore.ron`,
`ai_boxed.ron`, `ai_economy.ron`, `ai_rebuild.ron`. Tests: `crates/sf_ai/tests/fence.rs`, the unit tests in each module,
`tools/sf_headless/tests/ai_fog.rs`, `ai_base.rs`, `ai_rules.rs`, `ai_economy.rs`, `ai_perf.rs`.

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
(also the backoff of repair and of a refused harvester order). `reserve` and the army numbers are for WP9.1c.

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
`reserve` (it is for the army, WP9.1c: the harvester order ignores it) and the army numbers.

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
* The whitelist gains `Sell`: `BuildStructure`, `QueueUnit`, `Repair`, `Sell`.

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

## The schedule

An AI thinks when `(tick + stagger * player) % think_interval == 0` (`schedule::due`; `tick` is the number of ticks the world has run, the
tick the next step runs). Shipped: interval 10 (half a second), stagger 3. So two players never think on the same tick: the profile
check requires `think_interval / gcd(think_interval, stagger) >= 8`, the most players of a match (player 0 thinks at tick % 10 == 0,
player 1 at 7, player 2 at 4, ...). Each think runs the observation, the memory update and **one** manager in rotation, so each of the
four managers (`Economy`, `Construction`, `Production`, `Army`, numbered 0 to 3, which are hashed) runs every 40 ticks. **Construction has
rules since WP9.1b-1** (above) and **economy since WP9.1b-2** ("The economy"); the other two are still stubs whose turn only counts
(`manager_runs`): production and army are WP9.1c.

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
* `unit_mix`: `[(role, weight)]`, the share of the army each role aims at. Checked: infantry or vehicle (no vessel, no harvester: the
  economy makes those), open to the faction, has a cost, listed once, weight 1 to 1000, the factory of its category and everything its
  `requires` names are in the build list.
* Numbers: `think_interval`, `stagger`; `economy` (`power_margin`, `harvesters_per_refinery`, `reserve`, `max_parallel_builds`,
  `silo_percent`, `repair_below_percent`, `blocked_ticks`, `placement_cap`); `army` (`attack_value`, `max_wait`, `min_units`,
  `first_attack_tick`, `defend_ticks`, `gather_distance`). Every one is range-checked when the file is read; the values are placeholders of the memo.
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

* **WP9.1c**: production, army, scouting, attacks, defence posture. Defences are placed only towards the other players' start sites.
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
* The game does not run an AI (WP9.1d). `sf_game --match` is unchanged and its opponent is static.
* Difficulty levels (WP9.4), the "unfair" level and its two cheats, AI-vs-AI (WP9.5).
* The AI state is plain data but is not saved anywhere (WP13.1).
* A shuffled spawn order test (the iteration-order check of the memo) is not built: the AI iterates `BTreeMap`s and ascending vectors only, and the
  determinism script forbids hash collections, but no test shuffles.
