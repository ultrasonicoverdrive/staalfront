# Vision: shroud and fog of war

WP8.1. What each player can see, per cell, worked out by the simulation. **Nothing in the simulation behaves differently
yet**: targeting, defences, orders and production are as they were. The pass only keeps three facts per player and cell
(visible now, explored at some time, neither) and gives the next WPs one function to ask:

```rust
World::visibility(player, cell) -> Visibility   // Visible, Fog or Shroud
```

Code: `crates/sf_sim/src/vision.rs` (tests `vision/tests.rs`). Data: `assets/config/vision.ron` (one number),
`vision_look.ron` (the debug view only), `sight` of every role in `unit_defs.ron`. Bridge: `sf_bridge::vision_rules`.
Scenario: `assets/scenarios/vision.ron`, tests `tools/sf_headless/tests/vision.rs`.

## The three states

| State | Means | Held as |
|---|---|---|
| **visible** | inside the sight of an object of the player now | `count > 0` (and a bit) |
| **fog** | seen at some time, not now | `explored` and not visible |
| **shroud** | never seen | neither |

`explored` only ever gains cells. A visible cell is always explored (the audit checks it every tick in the tests). In a world
without vision rules `visibility` answers `Visible` for every player and cell; in a world with them, a cell outside the map
and any cell of a player that has had no object yet is `Shroud`.

## Sight: the disc

Sight is in thousandths of a cell (`sf_bridge::sight_milli`, rounded from the exact `Fx`, so a decimal with three places such
as 5.5 or 0.7 arrives as itself). A cell at offset `(dx, dy)` from the cell of an object is inside the disc when

    (1000 dx)^2 + (1000 dy)^2 <= sight^2        (integers, i64; the edge of the disc is inside)

Each distinct sight gets its disc once, as row spans (`Mask`: for each `dy` the largest `dx`, found with an integer square
root). A sight of 0 has no cells at all. Any sight above 0 has at least the centre cell. Cell counts, pinned by
`the_cell_counts_of_the_discs_are_pinned` and brute-forced from the definition: sight 1 = 5 cells, 3 = 29, 5 = 81, 10 = 317;
1.414 = 5 and 1.415 = 9 (the corner cell (1,1) is 1.4142 away).

## Who stamps what

| Object | Stamps | Notes |
|---|---|---|
| mobile unit | the disc round the cell its position is in | units have no `cell` of their own (`entities.cell` is `NO_CELL` for them); the cell comes from `pos` |
| structure that **stands** | the disc **grown by its footprint**: a cell is in when its distance to the nearest cell of the footprint passes the disc test | a 4 by 4 structure of sight 10 sees ten cells beyond each side, not ten from its anchor |
| structure under construction, being sold, or at zero hit points | nothing; its own footprint is marked explored | the same "does it count" rule as power and production (`World::is_standing`, `construction.md`) |
| any object with a sight of 0, or any object when its player already has 65,535 discs | nothing; its own cells are marked explored | a player knows where its own things are |
| dead entity | nothing | see the timing below |
| entity without a unit table entry (placeholder wanderer, legacy `Place` structure) | the `legacy_sight` of `vision.ron` (5 cells) | a legacy structure has no hit points and never stands: it only marks its cell explored |

The decision for structures (grown disc, no sight under construction) is the memo's second choice written into numbers; it
costs nothing extra: a structure's stamp is the same row spans with the footprint added to each row's span.

## The pass

Per player: `count: u16` per cell (how many of its discs cover it), a `visible` and an `explored` bitset (bit `y * width + x`),
and the sums below. Per slot: the stamp it made (owner, anchor cell, footprint, sight, whether a disc was actually added).

One pass per tick, **right after the crowd pass and before targeting** (`World::step`), walks the slots in ascending order,
works out the stamp each slot should have and **only where it differs** from the remembered one removes the old disc and
adds the new. Spawns, deaths, a slot reused by another entity (changed owner or sight in the same tick), construction
completing and a sale starting are all just "the stamp changed"; there are no hooks in spawn or despawn. A death in the
weapon stage of tick `t` is picked up by the pass of tick `t + 1` (`a_structure_shot_down_leaves_fog_for_its_owner...`: the
relay is gone in the step of tick 1021, its cells go to fog in 1022). So what targeting will read in WP8.2a is where the
bodies were at the end of this tick's movement.

Adding and removing a disc is integer addition, which commutes: the arrays do not depend on slot order
(`the_picture_does_not_depend_on_the_order_the_units_were_made`). Discs are clipped to the map per row; there is no
per-cell bounds test.

**The cap.** `u16` counts cannot overflow while a player has at most 65,535 live discs, so a stamp that would be the
65,536th is refused (that object sees nothing, only its own cells are explored) and is retried every pass. Nothing ships near
this (a player with 5,000 wanderers has 5,000); the cap is there so that the arithmetic is exact, and
`a_player_cannot_have_more_discs_than_a_count_can_hold` pins it. This is the one place slot order could show, and only
beyond 65,535 discs of one player.

## The rules and where they come from

`World::with_vision(VisionRules { legacy_sight_milli, sights: Vec<(Kind, u32)> })` (after the unit table; refused with
`ConfigError::Vision` when given twice, after the start, with a kind the table does not have, out of order, or above 64 000;
a unit table given after the rules is refused too). A world that is never given the rules has `vision: None`: no pass, no
section, every pin as before.

**Deviation from the design memo:** the sight is *not* a field of `UnitStats`. `UnitStats` is written out by hand in 65
places, most of them tests, and other branches add fields to it; the sight lives in `VisionRules` (a table by kind built by
`sf_bridge::vision_rules` from the `sight` of each role) and is hashed only inside the vision section. The units section
(`U`) never sees it, so it cannot move a pin.

Matches turn vision **on**: `matchplay::configure` gives the world the rules (nothing in the simulation reads them; the snapshot, the debug view and, since
WP8.3 and WP8.4, the picture do). The headless `match` scenario is **unchanged** and keeps its checksum: it writes its rules out in the scenario
file, and `match.expected` stays as pinned. What vision does to a match is checked by
`the_first_match_plays_the_same_with_vision_on` (see "Pins"). The sandbox gets vision only with `--debug-fog`.

## The checksum (section `v`)

After the last section (`r`, repair) and only in a world with vision rules. The byte is `v` (the upper-case letters are all
taken, and `M` is used twice, by money and by movement; that is left alone, "fixing" it would move every pin). It hashes the
rules (legacy sight, then each kind with its sight), then for each player that has had an object, ascending:

| Field | What |
|---|---|
| disc count | discs the player has stamped |
| `sum` | running `sum((index + 1) * count)` over the cells, kept up to date by arithmetic on every add and remove |
| visible cells, visible mix | the number of visible cells and a running sum of a fixed mixing of their indices, changed at each crossing of zero |
| explored cells, explored mix | the same for explored |

It does not walk the arrays (a 256 by 256 map with 8 players would be a megabyte of hashing per tick). The hashing of a tick
with vision is the rules (6 bytes per kind) and 37 bytes per player, whatever the map size. A missed unstamp, the bug this design invites, changes `sum` and
the visible mix; the full recompute that proves it cannot happen is `World::vision_audit` (the stamps against the arrays) and
`World::vision_audit_current` (the entities as they are now against the arrays, on a clone after `vision_resync`), run
**every tick** on `vision`, on the busy world of `the_incremental_counts_equal_a_recompute_every_tick` (deaths, spawns, a slot
reused, a structure going up and down) and at sample ticks on 5,000 wanderers.

## For the player's side: the snapshot, the real look and the debug view

`SimSnapshot` (sf_game) carries what the **local player** (`commands::LOCAL_PLAYER`) sees: `fog_version()` (`None` without
vision rules), the two bitsets, copied only when the version changed (the ore pattern; the version is bumped by every pass
that changed the visible or explored cells of that player), `visibility(cell)`, and `seen(id)` per view: own objects always,
others when their cell (a structure: any cell of its footprint) is visible. **Since WP8.3 and WP8.4 the picture filters by it**:
what is drawn, iconised, picked, put on the minimap or shown as an effect, the soft fog on the terrain and the ghosts of structures
last seen are all in [`fog_look.md`](fog_look.md). Worlds without vision rules are drawn as before.

The debug view (WP8.1) draws a dark film over shroud and a dim one over fog, in runs of at most 8 cells along a row, projected like
the placement ghost. Switch: the "Fog of war (debug)" checkbox of the F1 panel, or `--debug-fog` (which also gives the
sandbox vision rules). Colours: `vision_look.ron`. **It stays a debug view**: a quad over a steep slope is sheared, and it is a film over the
whole picture. It is no longer the way to see fog (units of the other side in the shroud no longer show through; they are hidden
now). Screenshots: `docs/screenshots/wp8.1/` (the debug view; tick in the name) and `docs/screenshots/wp8.3/` (the real look).

## Planned, not built

* **WP8.2a** (targeting gate, structure sight rules, allied vision): auto-acquisition (unit scan, `defence_check`) only picks
  candidates whose cell is `Visible` to the scanner's owner, behind a switch in the rules that is off by default; an explicit
  attack order on a target in fog stays valid or is dropped (to decide); retaliation (being hit) is always allowed. Allied
  vision (a team sees what its members see) is one more reader of the same arrays. This must be measured, not assumed: run
  the match with the gate on and off and compare the tick it is decided in.
* **WP8.2b** (height bonus): only after the sim has integer height levels and a map with relief exists. Both shipped maps are
  flat (`height_scale: 0`) and `sf_sim::terrain` has only type ids today.
* **WP8.3 / WP8.4**: built, see [`fog_look.md`](fog_look.md).
* **AI**: an opponent that must not cheat needs a last-seen memory on the sim side; not in this WP.

## The scenario `vision`

`assets/scenarios/vision.ron`, checksum `0x84facee4249aa869` in `vision.expected` (CI runs it on Linux and Windows), on the
frozen definitions of `tanks_vs_infantry_defs` (the sight of each kind is that copy's). Two players on 64 by 64 open
ground: a defence tower (sight 7); a scout (8) that walks east from tick 1 and back from tick 700; a light tank that shoots
the relay transmitter of player 1 (gone in the step of tick 1021) and then, from 1200, its rifleman (gone in 1407); a second
rifleman in the far corner that player 0 never sees. The transitions are asserted at their ticks in
`tools/sf_headless/tests/vision.rs`:

| Cell | Player 0 | Player 1 |
|---|---|---|
| (30, 32), the scout's corridor | shroud, visible from 175, fog from 922, visible again 1202 to 1212 while the tank passes, fog | shroud throughout |
| (41, 35), the relay | shroud, visible from 425, fog from 737, visible from 1202 | visible from tick 1, fog from 1022 |
| (44, 26), the rifleman's cell | shroud, visible from 1272 | visible from tick 1, fog from 1408 |
| (56, 52), the far corner | shroud for the whole run | visible from tick 1 |

## Tests and mutations

Tests (all call production code; `crates/sf_sim/src/vision/tests.rs`, `crates/sf_game/src/vision_view/tests.rs`,
`tools/sf_headless/tests/vision.rs`, `crates/sf_bridge/tests/vision.rs`, `crates/sf_data/src/vision*.rs`):

* **The discs**: cell counts for sight 0, 0.5, 1, 1.414, 1.415, 3, 5, 10 pinned and brute-forced from the definition; the edge
  is inside ((3,4) at sight 5, not at 4.999); symmetric in both axes and across the diagonal for seven sights.
* **What is seen**: a unit's disc and no other player's; each kind its own sight (distinct sights, so a swap shows); a unit of
  sight 0 only knows its cell; a structure from its whole footprint (counted by hand: 44 cells for a 2 by 2 of sight 3) and a 4 by 4
  of sight 4.5 not as a disc round its anchor; under construction and being sold it sees nothing but its footprint is explored;
  a legacy wanderer and a legacy structure.
* **Clipping**: eight positions at edges and corners against the brute-forced count, a quarter disc counted by hand (11 cells), a
  tower at the left edge that must not wrap to the right.
* **Transitions**: a runner crosses the map (shroud, visible, fog; explored never shrinks, every tick); a dead unit; a dying structure
  frees its cells but keeps the overlap with a neighbour's; a slot reused in the same tick by another owner and sight.
* **Bookkeeping against a recompute**: `the_incremental_counts_equal_a_recompute_every_tick` (12 wanderers, 9 table units, three
  structures, despawns, spawns in freed slots, construction on and off, 260 ticks; each tick both audits, the second on a clone);
  the same every tick of the `vision` scenario; at sample ticks on 5,000 wanderers.
* **Order**: the same stationary units made in three orders give the same bitsets, disc counts and digest.
* **Gate**: a world without rules has no digest, no bitsets, everything visible and a different checksum from the same run with
  rules; the rules are refused when they do not fit (unknown kind, twice, order, too far, after the start, a unit table afterwards).
* **No behaviour change**: the `vision` scenario with and without the rules shows the same entities in the same places and with
  the same hit points at every tick; the same for the first match (`match.ron`, 96 by 96 cells, to the end of the match, every
  tenth tick, the verdict every tick) and for 300 ticks of 5,000 wanderers.
* **Snapshot**: bitsets equal the world's answer for every cell, `seen` for own objects (also when their own cell is not in sight),
  near and far units, a structure seen only through a cell that is not its anchor, a stale id; the bitsets are copied only when the
  version changes (poisoned in between) and again when it does.

Mutations (each applied to the source, the tests run, then reverted with `git checkout`; none left in):

| Mutation | Fails |
|---|---|
| `<=` becomes `<` in the mask (`(s2 - used - 1) / 1e6`) | 13 tests, among them `the_edge_of_the_disc_is_inside` and `the_cell_counts_of_the_discs_are_pinned` |
| the last row of the disc dropped (`used >= s2`) | 12 tests, among them `the_cell_counts_of_the_discs_are_pinned` |
| no unstamp when the entity is gone | 5: `a_dead_unit_takes_its_sight_away...`, `a_dying_structure_frees_its_cells...`, `the_incremental_counts_equal_a_recompute_every_tick`, `the_digest_follows_visible_and_explored_separately`, the cap test |
| unstamp with the new owner and sight instead of the old | 6: `a_slot_taken_over_in_the_same_tick_unstamps_the_old_owner_and_sight`, `the_incremental_counts_equal_a_recompute_every_tick`, `a_structure_being_sold_stops_seeing`, ... |
| a cell that becomes visible is not marked explored | 8: `a_dead_unit_takes_its_sight_away...` (the fog is gone), `a_disc_is_clipped...` (visible and not explored), ... |
| no cap on discs (a `u16` count overflows) | `a_player_cannot_have_more_discs_than_a_count_can_hold` |
| the hash also mixes the stamps in slot order | `the_picture_does_not_depend_on_the_order_the_units_were_made` |
| the section written in a world without vision rules | `golden.rs`: `tiny_scenario_history`, `small_scenario_checkpoints` |
| the sight of each table position taken from the wrong kind | 12, among them `each_kind_has_the_sight_of_its_own_entry` |
| rows not clipped at the left edge | `a_disc_is_clipped_at_the_edges_and_corners`, `a_tower_beside_the_edge_does_not_wrap_to_the_other_side`, the recompute test |
| a structure stamps from its anchor only | `a_structure_sees_from_its_whole_footprint`, `a_big_structure_is_not_a_disc_round_its_anchor`, ... |
| the snapshot copies the bitsets on every fill | `the_bitsets_are_copied_only_when_the_version_changes` |
| own objects are not always seen | `own_objects_are_always_seen_even_when_their_own_cell_is_not_in_sight` |
| a structure is seen only by its anchor cell | `a_structure_is_seen_through_a_cell_that_is_not_its_anchor` (the first version of the tests did **not** catch this one: the seen structure's anchor was in sight; the test was added after the mutation survived) |


## Pins

**No pin moved.** The section exists only in a world given vision rules: `crates/sf_sim/tests/golden.rs`, every `.expected` of the
CI list (`git diff` of `assets/scenarios` against the WP6.6 head shows only the new `vision.ron` and `vision.expected`), the
benchmark's PLAIN and FIGHT checksums (`crates/sf_game/tests/bench_scene.rs`) and the 30 scenarios of the CI list run in release
with `--expect` all pass. The new pin is `vision.expected` (`0x84facee4249aa869`).

The first match: `match.expected` is `0x164403a92986654f` and did not move, because the headless `match` scenario does not turn
vision on. With the same scenario given vision rules (a copy with `vision: Some((legacy_sight_milli: 5000))`, measured with the
release binary) the final checksum is `0x771de0f816275bbc`; with the line taken out it is `0x164403a92986654f` again, the pin. The
two worlds are the same match in every position and hit point (`the_first_match_plays_the_same_with_vision_on`), so the section is
the whole difference. `wander_5000`: `0x38d5e3c43358a889` without the rules (the pin), `0x6231220a5eaa67d9` with sight 5 and
`0x65da61b8f36c30dc` with sight 10.

## Cost

Release `sf_headless`, same machine (4 cores, load average 2 to 5 from other agents' builds, so read the ratios; the five rounds
of each line are within 2 % of each other), alternating base and head runs. Base is the WP6.6 head (`6909b1f`), head is this branch.
Ticks per second of the whole step (the pass, the hashing and everything else):

| Scenario | base (WP6.6) | head, rules off | head, vision sight 5 | head, vision sight 10 |
|---|---|---|---|---|
| `wander_5000`, 10,000 ticks, 5,000 wanderers moving all the time | 849 to 864 (1.16 ms a tick) | 869 to 874 | 530 to 536 (1.89 ms) | 396 to 400 (2.52 ms) |
| `match`, 7,000 ticks, the first match | 27,900 to 31,800 | 32,400 to 32,800 | 30,400 to 31,800 | not run |

* **Off costs nothing**: the pass is one `Option` test (head off is the same speed as base, within noise).
* **On, in the crowd**: the pass costs 0.7 ms a tick at sight 5 and 1.4 ms at sight 10 with 5,000 units that never stop. That is
  about 61 % and 115 % on top of the 1.16 ms the whole step costs there. A tick has a budget of 50 ms, so it stays small, but it is the
  dominant cost of vision and it scales with the number of units that change cell and with the area of their disc. Where the time
  goes inside the pass (the compare of 5,000 slots against the work at each cell that becomes visible or hidden) was not measured.
* **On, in a match**: within the noise of the machine: the 7,000 ticks take 213 to 250 ms with rules off and 220 to 230 ms with them
  on, so between 0 and 0.003 ms a tick.
* **Not done, and why**: stamping only the difference of two discs one cell apart (about 2 x (2r + 1) cells instead of 2 x
  (2r + 1)^2) would cut the crowd cost several times over. It is the next step if a crowd of units with a large sight ever shows
  in a profile; nothing in a match does. The container and CI cannot measure the frame rate requirement (PLAN section 13).

## Not done, on purpose (gaps)

* No behaviour: nothing reads `visibility` (WP8.2a).
* No allied vision, no shared vision, no reveal-the-map effect, no radar or special reveal, no height or occlusion (a disc
  looks through everything).
* The debug view is sheared on slopes and is a film over the whole picture (the real look, ghosts, hiding, the minimap are in
  `fog_look.md`).
* The snapshot carries the local player only (a constant, `LOCAL_PLAYER`); a replay or observer view would pass another.
* The AI has no memory of what it has seen.
* A sold or under-construction structure sees nothing (decided, written above), which also means that a base whose only
  structure is being sold goes dark at the start of the sale.
* A player with more than 65,535 discs: the surplus sees nothing.
