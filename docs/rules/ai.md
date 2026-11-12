# The computer opponent

WP9.1a (the frame) and WP9.1b-1 (the base). Design memos of 10-11-2026 and 11-11-2026 (orchestrator, Fable); Tijmen: "fase 9 mag eerder
beginnen". WP9.1a builds what every later part stands on: the AI **observes through the fog, remembers what it saw and keeps its schedule**.
WP9.1b-1 adds the first manager with rules, **construction** (the goal list, the power rule, the money rule, placement), so the AI builds
its base. Economy (harvesters, silo, repair) is WP9.1b-2, production and army WP9.1c; those managers are still stubs. This page says what
exists and what is a stub.

Code: `crates/sf_ai/src/` (`observe.rs`, `memory.rs`, `state.rs`, `schedule.rs`, `profile.rs`, `sites.rs`, `ai.rs`, and since WP9.1b-1
`budget.rs`, `base.rs`, `placement.rs`), the profile format `crates/sf_data/src/ai_profile.rs`, the data `assets/ai/normal.ron`, the driver
`tools/sf_headless/src/run.rs` (`run_full`, `run_watched`), the scenarios `assets/scenarios/ai_fog.ron`, `ai_base.ron`, `ai_noore.ron`,
`ai_boxed.ron`. Tests: `crates/sf_ai/tests/fence.rs`, the unit tests in each module, `tools/sf_headless/tests/ai_fog.rs`, `ai_base.rs`,
`ai_perf.rs`.

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
| `building(kind)`, `harvester_kinds()`, `unit_cost(kind)` | the public rules (building table, the kinds that harvest, a unit's price) |
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

`budget::committed` = the unpaid part of every own structure going up (`cost * left / total`, rounded up) plus the unpaid price of every
queued unit that is not free (the front item minus `front_paid`). `budget::free` = credits minus that, at least zero. **Stateless**: worked
out from the observation at every turn, so there is no ledger to drift. The simulation never refuses for lack of money (`economy.md`), so
this is the AI's own count. Credits are the ore store (`World::credits`).

### The construction turn

Every 40 ticks (manager 1 of the rotation), `base::turn`:

1. **Verify.** `pending_build = (kind, cell, tick)`, the build sent at the last turn, must be an own structure of that kind anchored on that
   cell (standing or going up). If not, the simulation refused it (a unit on the cell, say), and `(kind, cell)` goes into `refused` until
   `now + blocked_ticks`. The latch is cleared either way. Expired entries of `refused` and `blocked_until` are dropped. `refused` holds at
   most 64 entries (`REFUSED_CAP`); a new one pushes out the one that expires first.
2. **Parallel cap.** With `max_parallel_builds` own structures going up, nothing starts. (The count and the goal count also include the latch, which step 1 has always cleared by then: a turn that skipped the verify would stop building rather than send a second build; the mutation of the verify step shows it.)
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
   standing, a normal build also keeps the price of a harvester (capped the same way), so a broke AI does not lock its last credits into a
   structure it cannot cancel.
6. **Place** (below). Found: `BuildStructure { kind, cell }` is sent and latched in `pending_build`. Nothing fits:
   `blocked_until[kind] = now + blocked_ticks`, and the next turn tries the next entry.

Rebuilding has no rule of its own: a lost structure makes its goal entry unsatisfied again (not tested before WP9.1b-2's `ai_rebuild`).
The AI never sells (decision 7).

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
  ground (`ore_cells`, the harvestable terrain within the reach plus 16 cells, listed once at the first search). A defence: its middle to
  the nearest start site of another player. Everything else, and a refinery without ore or a defence without other sites: its middle to the
  middle of the nearest own anchor.
* **Preview**, best first, at most `placement_cap` questions per search: `Some(Ok)` wins, `Some(Err)` goes into `refused`, `None` (not all
  visible) is skipped. **`placement_cap` is 32** (was 256; decision 4): one question costs about 8 us with 5,000 entities on the map
  (`what_one_placement_question_costs_in_a_crowd`), so 256 would be about 2 ms, over the 1 ms a think may take. The measured worst think
  stayed under 1 ms, because in every scenario the mask makes the first question the answer (at most 1 per think); the cap is the ceiling
  for a base where many questions fail.

### The numbers (profile, data)

New in the `economy` block of `assets/ai/normal.ron`, with defaults so that a profile written before WP9.1b-1 (the frozen
`ai_fog_defs/normal.ron`) still reads (decision 5): `lane` (0 to 4, default 1), `refinery_apron` and `factory_apron` (`(width, depth)`,
width odd, both 1 to 9; defaults (3, 5) and (1, 3)). `placement_cap` is 32. Pinned by `the_shipped_profile_is_pinned` (literals, of the file
and of the defaults), `every_number_is_checked_at_both_ends_when_the_file_is_read` and
`a_profile_without_the_base_numbers_of_wp9_1b_reads_with_the_defaults`. Acting since WP9.1b-1: the schedule numbers, `power_margin`,
`max_parallel_builds`, `silo_percent` (as the cap of what a build saves up), `blocked_ticks`, `placement_cap`, `lane`, the aprons.
`harvesters_per_refinery`, `reserve`, `repair_below_percent` and the army numbers are for WP9.1b-2 and WP9.1c.

### State

New in `AiState`, all in the digest in this order: `pending_build`, `blocked_until: BTreeMap<Kind, Tick>`,
`refused: BTreeMap<(Kind, Cell), Tick>`, `ore_cells: Option<Vec<Cell>>`, `previews` (questions passed to the simulation in all thinks).
Integers, ordered maps and sorted vectors; no random draw (ties by `(y, x)`). Pinned by `the_digest_of_the_base_state_is_pinned` (a
literal) and `every_new_part_of_the_state_moves_the_digest`. The profile digest hashes `lane` and the aprons.

### Scenarios

All on `ai_base_defs` (a frozen copy, WP9.1b-1, of the shipped definitions, the profile and a match file of its own): player 0 (Meridian,
the AI) with a construction centre at (12, 68) and 6000 credits on the first match's map, player 1 a lone static construction centre at
(72, 14). The numbers of ore, harvest, storage (refinery 2000, silo 1500), money, production (build speed 3/2), power and vision are the
shipped ones of that day, written out. No scripted command.

| Scenario | Map | Ticks | Checksum | AI digest |
|---|---|---|---|---|
| `ai_base` | `m1` | 8000 | `0x1b2eb3765910e496` | `0x485156104bc0ebb7` |
| `ai_noore` | `m1_noore` (no ore) | 40,000 | `0x4c0e8ed5d1f89874` | `0x17fe214bd5dcaf8a` |
| `ai_boxed` | `m1_boxed` (rock round the base, a pocket x 5..17, y 66..73) | 8000 | `0x212468ddebc97ff5` | `0xad6a07ba89ea0052` |

The maps are drawn by `tools/sf_headless/examples/gen_ai_maps.rs` from `gen_match_map.rs` (a test draws them again and compares).
What `tools/sf_headless/tests/ai_base.rs` checks:

* `ai_base`: the goal list in order (centre, plant, refinery, barracks, plant, vehicle factory, tower, refinery, **plant** by the power
  rule, radar, tower, advanced defences); everything up to the vehicle factory standing by **T1 = tick 1998** and the whole list by **T1b =
  tick 6518** (measured; the test allows 2000 and 6600); 11 builds, none refused; power made at least power used after every tick; no stall
  after every tick; the lane between every two structures and no structure on any of the four aprons (from `placement::apron`); two
  harvesters (one free per refinery) and ore delivered.
* `ai_noore`: the 40,000 ticks end; at most 25 commands per 1000 ticks; the last build before tick 3000 (8 builds, what 6000 credits pay
  for), then nothing; no stall, nothing refused.
* `ai_boxed`: the refinery fits nowhere (`blocked_until` holds it) and the later goals that do not need it are built (barracks, the second
  plant, both towers); one placement question per build.
* A build the simulation refuses (a unit of player 0 put on the plant's cell at tick 10, driving off at tick 20) is noticed at the next turn
  (tick 50) and the plant is built elsewhere, although the cell is free again by then.
* The search keeps a plant off the apron of a refinery the AI believes it has (`the_search_keeps_off_the_apron_of_an_own_refinery`).
* The whitelist: every command the AI sends in the three scenarios is `BuildStructure` (the design's whitelist is `BuildStructure`,
  `QueueUnit`, `Repair`; the last two come with WP9.1b-2).

`ai_fog` moved with this part: its AI now builds a power plant (tick 10) and a refinery (tick 330, an emergency; its 1500 credits pay for no
more). New pins `0x768b77651509db8c` and `0x54a924e86710242c`; with the construction manager off and the new state and profile numbers left
out of the digests, both old values come back (measured).

## The schedule

An AI thinks when `(tick + stagger * player) % think_interval == 0` (`schedule::due`; `tick` is the number of ticks the world has run, the
tick the next step runs). Shipped: interval 10 (half a second), stagger 3. So two players never think on the same tick: the profile
check requires `think_interval / gcd(think_interval, stagger) >= 8`, the most players of a match (player 0 thinks at tick % 10 == 0,
player 1 at 7, player 2 at 4, ...). Each think runs the observation, the memory update and **one** manager in rotation, so each of the
four managers (`Economy`, `Construction`, `Production`, `Army`, numbered 0 to 3, which are hashed) runs every 40 ticks. **Construction has
rules since WP9.1b-1** (above); the other three are still stubs whose turn only counts (`manager_runs`): economy is WP9.1b-2, production and
army WP9.1c.

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
the power plant and barracks, the barracks is removed at tick 500, the scout walks home at 520. Pinned (WP9.1a; re-pinned by WP9.1b-1, see "The base"): final checksum `0xd369218db6025eff`
(`ai_fog.expected`), AI digest `0x993afc7cd2c42cfd` (a literal in `tests/ai_fog.rs`), 800 ticks. The tests assert: the AI knows exactly the
two structures the scout saw (kind, anchor, footprint, tick) and not the centre, refinery or army; it forgets the removed barracks and keeps
the power plant after the scout leaves, also when it is destroyed out of sight at tick 700 (same memory and digest as without); without the scout it knows nothing; the observation at six ticks never holds a shrouded entity;
**an invisible army changes nothing the AI sees or does** (same observation, memory and digest as a world without the army, while the world
checksums differ) and the positive control (a unit inside the scout's sight is seen); the AI thinks 80 times in 800 ticks, manager runs
`[20, 20, 20, 20]`; two AIs think at tick % 10 == 0 and == 7. Since WP9.1b-1 the AI sends exactly its two builds (`the_ai_sends_its_two_builds_and_nothing_else`), and the invisible army also leaves its commands unchanged.

## Cost

WP9.1b-1 (`ai_perf.rs`, release, this container, load average about 0.6, three runs; the AI's commands are now scheduled in the
measurement, so the AI really builds): `ai_base` 0.12 to 0.13 us per AI per tick (worst think 39 to 54 us), with 5,000 wanderers 6.6 to
7.5 us (worst think 137 to 239 us); `ai_boxed` 0.08 to 0.09 us (worst 25 to 30 us), with the crowd 6.6 to 7.6 us (worst think 136 to
890 us, the 890 in one run of three). At most one placement question per think everywhere (11, 10, 5 and 5 in 700 thinks). One question
in the crowd: 7.8 to 8.1 us. Budget: mean 20 us, worst 1 ms.

WP9.1a:

Measured by `tools/sf_headless/tests/ai_perf.rs` (release, the container of this PR, load average about 2, three runs): in `ai_fog` the mean is
0.05 to 0.07 microseconds per AI per tick (worst think 1.4 to 14 us); with 5,000 wanderers of player 1 over the map (every one a cell test) the mean is
6.2 to 6.6 us per AI per tick (62 to 66 us per think, worst think 101 to 113 us). The memo's budget: mean at most 20 us per AI per tick, worst at most
1 ms. A whole run of that crowd, alternating, medians of three: 633 ticks/s with the AI against 623 without (noise). The assertions are loose (ten times the
budget, release only); the numbers are the evidence. The observation walks every entity once per think; a map with far more entities
than 5,000 costs proportionally more, and the managers of WP9.1b and WP9.1c add to this.

## Not done

* **WP9.1b-2**: the economy manager (harvesters, silo, repair, the silo emergency), `ai_economy`, `ai_rebuild`. Until then a destroyed
  harvester is not replaced and nothing is repaired; the AI gets harvesters only free with its refineries.
* **WP9.1c**: production, army, scouting, attacks, defence posture. Defences are placed only towards the other players' start sites.
* Not checked: whether a lane of one cell is wide enough for harvester traffic (bodies are 1.5 cells); whether `Despawn` of a structure
  leaves the same state as destruction by weapons (WP9.1b-2's `ai_rebuild`).
* `refused` after a sent build only matters when the mask cannot see why the simulation said no; the one test of it is the unit that
  drives off (above).
* The game does not run an AI (WP9.1d). `sf_game --match` is unchanged and its opponent is static.
* Difficulty levels (WP9.4), the "unfair" level and its two cheats, AI-vs-AI (WP9.5).
* The AI state is plain data but is not saved anywhere (WP13.1).
* A shuffled spawn order test (the iteration-order check of the memo) is not built: the AI iterates `BTreeMap`s and ascending vectors only, and the
  determinism script forbids hash collections, but no test shuffles.
