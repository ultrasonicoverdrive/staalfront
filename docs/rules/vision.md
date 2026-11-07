# Vision: shroud and fog of war

WP8.1, WP8.2a. What each player can see, per cell, worked out by the simulation, and (WP8.2a, behind a flag that is **off**
everywhere it ships) a gate on automatic target acquisition. With the flag off **nothing in the simulation behaves differently**:
targeting, defences, orders and production are as they were. The pass keeps three facts per player and cell (visible now, explored
at some time, neither) and gives the rest of the simulation one function to ask:

```rust
World::visibility(player, cell) -> Visibility   // Visible, Fog or Shroud
```

Code: `crates/sf_sim/src/vision.rs` (tests `vision/tests.rs`). Data: `assets/config/vision.ron` (two numbers: the legacy sight and the gate flag),
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

## The targeting gate and shared vision (WP8.2a)

Two fields of `VisionRules` (both in the section `v` of the checksum, and only when one is set):

| Field | Default | Meaning |
|---|---|---|
| `acquire_needs_sight: bool` | `false` (`vision.ron`, the match, every scenario but `vision_targeting`) | the gate, below |
| `shared: Vec<(PlayerId, PlayerId)>` | empty (the game has no teams) | `(sharer, viewer)`: the viewer also sees what the sharer sees |

**The gate.** With the flag on, **automatic acquisition** only picks a candidate that is `Visible` to the scanner's **owner** at the
time of the scan: the single function every automatic pick goes through (`Targeting::best_target`: a unit's scan, which covers idle,
hold, guard and attack-move units, and a defence structure's look round, `defences.md`) skips the others, after it has found that a
weapon could fire at them. A unit candidate counts by the cell its position is in; a structure candidate by **any cell of its
footprint** (a relay whose anchor is in the fog and whose lower row is in sight is seen). What the scanner's owner sees includes what
its allies share.

What the gate does **not** touch, on purpose:

* **Retaliation** (a unit that is hurt turns on its attacker, `targeting.md`) is always allowed: the attacker is picked from the fog,
  and then followed within the pursuit distance like any attacker. `return_fire` is given no gate.
* **An explicit attack order** (`Order::Attack`, `AimTurret`) is never gated and never dropped because the target is in fog. Chosen:
  the player's order wins over any rule of the units (`targeting.md`), the interface will not offer an unseen target (WP8.3, WP8.4),
  and a "grace in fog" state would be one more thing to hash and to get wrong for no case that exists yet.
* **A target already chosen** (a `Pursue` order or a defence's turret target) is kept while the existing rules keep it, also when it
  steps into fog: the gate is on the pick, not on the engagement. A unit that fired at something it saw keeps its shot.
  Gap, on purpose (see "Not done").

**Shared vision.** `World::visibility(player, cell)` is the union of the player's own picture and that of every player that shares
with it (`Visible` beats `Fog` beats `Shroud`, so also explored cells are united). It is one way (`(0, 2)` lets player 2 see what
player 0 sees, not the reverse), not transitive (`(0, 1), (1, 2)` does not give player 2 the cells of player 0), and it needs no
object of the viewer. The pairs must be strictly ascending and never a player with itself (`ConfigError::Vision` otherwise). The
per-player bitsets (`fog_words`, `fog_version`, the snapshot's bitsets and the debug view) are the player's **own** cells and do not
contain the shared ones; the snapshot's `seen` does (see "Own picture and shared vision" below): nothing in the game sets a pair yet. Sharing is stored as a list and read at query time, so it adds no state, no cost to
the pass, and a scenario can set it.

**Structure sight** needed nothing more: the data has a `sight` for every structure (`unit_defs.ron`, 3 to 12 cells), a structure
stamps its disc grown by its footprint and only while it stands (table above, WP8.1), and a structure under construction sees nothing
but knows its footprint. The gate reads that.

**One thing to know before the gate is switched on: sight against reach.** The disc is cell against cell (a cell is in when its
centre is inside), a weapon's reach is measured from the unit's position to the **edge** of the target. A unit whose sight equals its
reach therefore loses a target at the very limit of its reach: a rocket trooper (sight 5, reach 5) does not pick the defence tower
whose cell is at squared distance 26 from its own cell, though the tower is within reach. In the first match that is the whole
difference (below, "What the gate would do to the first match"). Units that outreach their sight (siege artillery: reach 10, sight 6;
the unmanned artillery: reach 12, sight 6) need a spotter by design; that is the point of the gate.

**What the gate would do to the first match** (measured, `what_the_gate_would_do_to_the_first_match`; **not shipped**: the flag is off in
`match.ron`, and `match.expected` is the match without it). The match is still won, at tick **8067** instead of 5967, with **8 units
lost** instead of 6. The two worlds part at tick 4282, when a rocket trooper would have picked the opponent's defence tower (the case
above). Whether to switch the gate on, and whether to give the units a cell more sight or to measure the disc to the edge of the
target, is for Tijmen after playing M1.

## The rules and where they come from

`World::with_vision(VisionRules::new(legacy_sight_milli, sights))` plus `.with_acquire_needs_sight(bool)` and `.with_shared(pairs)`
(after the unit table; refused with
`ConfigError::Vision` when given twice, after the start, with a kind the table does not have, out of order, or above 64 000, or with shared pairs that do not ascend or name a player with itself;
a unit table given after the rules is refused too). A world that is never given the rules has `vision: None`: no pass, no
section, every pin as before.

**Deviation from the design memo:** the sight is *not* a field of `UnitStats`. `UnitStats` is written out by hand in 65
places, most of them tests, and other branches add fields to it; the sight lives in `VisionRules` (a table by kind built by
`sf_bridge::vision_rules` from the `sight` of each role) and is hashed only inside the vision section. The units section
(`U`) never sees it, so it cannot move a pin.

Matches turn vision **on**: `matchplay::configure` gives the world the rules, and since WP8.2a the headless `match` scenario
writes the same rules out (`vision: Some((legacy_sight_milli: 5000))`), so the pinned scenario is the game's world and not a
sibling of it. In both the targeting gate (WP8.2a, below) is off. Since WP8.3 and WP8.4 the picture reads the rules too. The sandbox
gets vision only with `--debug-fog`.

## The checksum (section `v`)

After the last section (`r`, repair) and only in a world with vision rules. The gate and the shared pairs are written after the sights, as a block tagged `g`, **only when the gate is on or a pair is set**; rules with both off hash exactly as in WP8.1 (`match` and `vision` keep their pins; `the_gate_and_the_pairs_are_in_the_hash_only_when_set`). The byte is `v` (the upper-case letters are all
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
`World::vision_audit_current` (the entities as they are now against the arrays; it runs the pass on a clone itself, so no public function runs the pass outside a step), run
**every tick** on `vision`, on the busy world of `the_incremental_counts_equal_a_recompute_every_tick` (deaths, spawns, a slot
reused, a structure going up and down) and at sample ticks on 5,000 wanderers.

## For the player's side: the snapshot, the real look and the debug view

`SimSnapshot` (sf_game) carries what the **local player** (`commands::LOCAL_PLAYER`) sees: `fog_version()` (`None` without
vision rules), the two bitsets, copied only when the version changed (the ore pattern; the version is bumped by every pass
that changed the visible or explored cells of that player), `visibility(cell)`, and `seen(id)` per view: own objects always,
others when their cell (a structure: any cell of its footprint) is visible. **Since WP8.3 and WP8.4 the picture filters by it**:
what is drawn, iconised, picked, put on the minimap or shown as an effect, the soft fog on the terrain and the ghosts of structures
last seen are all in [`fog_look.md`](fog_look.md). Worlds without vision rules are drawn as before.

Own picture and shared vision, exactly: the two bitsets, `fog_version` and `visibility(cell)` of the snapshot are the local
player's **own** cells, but `seen` is computed with `World::visibility`, which answers with the union of the own picture and that of every sharer
(`sf_game/src/sim_snapshot.rs`). The two agree as long as no `(sharer, viewer)` pair is set, and nothing in the game sets one
(only scenarios do). Follow-up for when teams exist: take the bitsets and the version from the shared union too, or `seen` from the
own picture, so the fog drawn and the objects shown come from the same set.

The debug view (WP8.1) draws a dark film over shroud and a dim one over fog, in runs of at most 8 cells along a row, projected like
the placement ghost. Switch: the "Fog of war (debug)" checkbox of the F1 panel, or `--debug-fog` (which also gives the
sandbox vision rules). Colours: `vision_look.ron`. **It stays a debug view**: a quad over a steep slope is sheared, and it is a film over the
whole picture. It is no longer the way to see fog (units of the other side in the shroud no longer show through; they are hidden
now). Screenshots: `docs/screenshots/wp8.1/` (the debug view; tick in the name) and `docs/screenshots/wp8.3/` (the real look).

## Planned, not built

(WP8.2a is built, above.)

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

## The scenario `vision_targeting` (WP8.2a)

`assets/scenarios/vision_targeting.ron`, checksum `0x8ee88b023c9719cb` in `vision_targeting.expected` (CI runs it on Linux and
Windows), on the same frozen definitions (`tanks_vs_infantry_defs`), with `acquire_needs_sight: true` and `shared: [(2, 0)]`
(player 2 shares with player 0). Slots are listed in the file. Asserted in `tools/sf_headless/tests/vision_targeting.rs`:

| What | Slots | Ticks |
|---|---|---|
| A siege artillery (reach 10, sight 6) has an enemy rifleman 8 cells away in reach and does not pick it; a scout of player 2 walks up, player 0 sees the cell at tick **539** (not in its own bits: the sight is shared), the artillery engages at **541** and fires at **546** | 0, 3, 8 | 539 / 541 / 546 |
| The same file with the gate off: engaged and firing within the first ten ticks | 0, 1 | 1 / <= 10 |
| A defence (unmanned artillery, reach 12, sight 6) has a rifleman 9 cells away and ignores it; the second scout reveals it at **654**, the first shot is at **661** | 1, 4, 9 | 654 / 661 |
| A relay (2 by 2) whose anchor is outside the sight of an artillery and whose lower row is inside is picked at once | 2, 5 | 1 / first shot 18 |
| An artillery of player 1 with an explicit attack order fires at a tank it cannot see (tick **19**); the shell lands at **32** and the tank takes its attacker at once, though the attacker's cell is shroud to player 0; the attacker comes into the tank's sight at 65 | 6, 7 | 19 / 32 / 65 |
| Sharing reversed `[(0, 2)]`, or none: the artilleries never fire in 1,500 ticks, and the unrelated shots (slots 2 and 6) stay as they are | 0, 1 | never |

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

* **Gate and sharing (WP8.2a)**: `vision_targeting.rs` (table above, each with the gate-off counterpart and the reversed pair);
  `a_viewer_sees_what_its_sharer_sees_and_not_the_other_way_round`, `shared_fog_is_fog_for_the_viewer_and_a_viewer_without_objects_sees_too`,
  `sharing_both_ways_and_in_a_chain_is_not_transitive`, `the_shared_pairs_must_ascend_and_not_name_a_player_twice_in_one_pair`,
  `the_gate_and_the_pairs_are_in_the_hash_only_when_set` (five different rules, five different digests; off and empty equals plain),
  `the_rules_are_handed_back_with_the_gate_and_the_pairs`; `sf_bridge`: `the_gate_flag_reaches_the_rules_and_is_off_in_the_shipped_file`;
  `sf_data`: the line is optional and read; `match_m1.rs`: `vision_in_the_match_moves_the_pin_by_its_section_only`;
  `what_the_gate_would_do_to_the_first_match`.
* **Sets, not counts**: the tests that said "explored never shrinks" compare the sets (`prev & !now == 0`, per word) and no longer the
  number of cells (`a_unit_that_walks_away_leaves_fog_and_explored_never_shrinks`, `explored_never_shrinks_and_visible_cells_come_and_go`).

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

| WP8.2a: the gate ignores the flag (always on in a world with vision rules) | 9: `with_the_gate_off_the_same_units_fire_at_once`, `the_final_checksum_is_the_pinned_value` (match), `vision_in_the_match_moves_the_pin_by_its_section_only`, `the_human_wins_in_the_tick_the_last_of_the_opponent_falls`, `the_opponents_tower_fires_in_the_match`, ... |
| WP8.2a: the gate asks about the target's owner instead of the scanner's | 6: `a_unit_does_not_pick_what_it_cannot_see_until_a_friend_reveals_it`, `a_defence_ignores_an_unseen_target_until_it_is_revealed`, `being_hit_from_the_fog_is_answered_and_an_attack_order_is_not_gated`, ... |
| WP8.2a: retaliation goes through the gate (the attacker must be visible; the context given the gate) | `being_hit_from_the_fog_is_answered_and_an_attack_order_is_not_gated`, `the_final_checksum_is_the_pinned_value` (**the first version of this mutation changed only one of the two places and survived**: with no gate in the context the check in `return_fire` does nothing; both places are mutated together) |
| WP8.2a: a structure is seen by its anchor cell only | 5: `a_structure_target_is_seen_through_any_cell_of_its_footprint`, `the_gate_changes_only_what_it_gates`, `what_the_gate_would_do_to_the_first_match`, ... |
| WP8.2a: a unit target is "seen" whatever its cell | 5: `a_unit_does_not_pick_what_it_cannot_see_until_a_friend_reveals_it`, `a_defence_ignores_an_unseen_target_until_it_is_revealed`, ... |
| WP8.2a: shared vision not applied | 7: `a_viewer_sees_what_its_sharer_sees_and_not_the_other_way_round`, `the_viewer_sees_the_sharers_cells_in_fog_and_in_sight`, `a_unit_does_not_pick_what_it_cannot_see_until_a_friend_reveals_it`, ... |
| WP8.2a: shared vision applied the wrong way (the sharer gets the viewer's cells) | 8: the same, and `sharing_goes_from_the_sharer_to_the_viewer_only` |
| WP8.2a: the `g` block is written whatever the rules (gate off, no pairs) | `vision_in_the_match_moves_the_pin_by_its_section_only`, `the_final_checksum_is_the_pinned_value` (`match`: the pin of step 0 moves) |
| WP8.2a: the hash does not tell `(a, b)` from `(b, a)` | `the_gate_and_the_pairs_are_in_the_hash_only_when_set`, `the_final_checksum_is_the_pinned_value` |

## Pins

**No pin moved.** The section exists only in a world given vision rules: `crates/sf_sim/tests/golden.rs`, every `.expected` of the
CI list (`git diff` of `assets/scenarios` against the WP6.6 head shows only the new `vision.ron` and `vision.expected`), the
benchmark's PLAIN and FIGHT checksums (`crates/sf_game/tests/bench_scene.rs`) and the 30 scenarios of the CI list run in release
with `--expect` all pass. The new pin is `vision.expected` (`0x84facee4249aa869`).

The first match: `match.expected` moved in WP8.2a from `0x164403a92986654f` to `0x771de0f816275bbc` because the scenario now has
the vision rules like the game (review finding 1 of #104). Measured cause (release binary): the file with the `vision` line
gives `0x771de0f816275bbc`, with the line taken out `0x164403a92986654f` again (`vision_in_the_match_moves_the_pin_by_its_section_only`
in `match_m1.rs`). The two worlds are the same match in every position and hit point
(`the_first_match_plays_the_same_with_vision_on`), so the section is the whole difference. `wander_5000`: `0x38d5e3c43358a889`
without the rules (the pin), `0x6231220a5eaa67d9` with sight 5 and `0x65da61b8f36c30dc` with sight 10.

Other pins in WP8.2a: none moved (`golden.rs`, every other `.expected`, the bench PLAIN and FIGHT checksums); new: `vision_targeting.expected`
(`0x8ee88b023c9719cb`). A world with vision rules and the gate off and nothing shared hashes as before (the `g` block is absent), which is
why `vision.expected` (`0x84facee4249aa869`) stands and why the step-0 `match.expected` is the only pin that moved.

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

### The gate (WP8.2a)

Release `sf_headless`, same container (4 cores; load average 1.4 to 1.5 at the start and end of the runs, a few other agents idle),
six rounds, the five variants alternating inside each round; ticks per second of the whole step, minimum to maximum (median). The
variants of the two fights are copies of the shipped scenarios in the scratch directory with `vision: Some((legacy_sight_milli: 5000,
acquire_needs_sight: false / true))`; `tanks_vs_infantry` has 30 objects and 1,500 ticks of fighting, `match` is the first match for 7,000
ticks.

| Scenario | no vision rules | gate off | gate on |
|---|---|---|---|
| `tanks_vs_infantry` | 108,600 to 118,700 (117,000) | 81,600 to 106,100 (96,300) | 65,900 to 103,500 (98,800) |
| `match` (7,000 ticks) | 38,300 (one run, before step 0) | 27,700 to 36,500 (35,800) | 34,900 to 37,300 (36,800) |

* **The gate's own cost is below what this container can resolve.** The "on" runs are not slower than the "off" runs; the spread of
  one variant between rounds (10 to 20 %, with one slow outlier in each of three variants) is larger than any difference between the
  two columns. In `match` the gate-on world plays a different, longer fight (it is decided at 8067, not 5967), so the two columns
  are not the same work there.
* Per tick the gate adds one `Option` test in the context (`Gate::of`) and, per candidate that a weapon could fire at, one visibility
  lookup (one cell, or the cells of a footprint); the candidates are the few in reach, not the whole map. A scan runs once every
  ten ticks per unit.
* The difference between "no vision rules" and "gate off" in the table is the vision pass of WP8.1 (about 15 % of a tick in this small
  fight), not the gate.
* Not measured: a crowd of scanners with many candidates in reach and a footprint-heavy target list. The container and CI cannot
  measure the frame-rate requirement (PLAN section 13).

## Not done, on purpose (gaps)

* The gate is off in everything that ships; nothing but the scenario `vision_targeting` and the measurement test turns it on.
* A chosen target that steps into fog is kept (the gate is on the pick only). No team or alliance exists in the game: `shared` is set by
  scenarios only; the debug view and the snapshot's bitsets do not use it, while `seen` does (see "Own picture and shared vision").
  No reveal-the-map effect, no radar or special reveal, no height or occlusion (a disc looks through everything).
* The debug view is sheared on slopes and is a film over the whole picture (the real look, ghosts, hiding, the minimap are in
  `fog_look.md`).
* The snapshot carries the local player only (a constant, `LOCAL_PLAYER`); a replay or observer view would pass another.
* The AI has no memory of what it has seen.
* A sold or under-construction structure sees nothing (decided, written above), which also means that a base whose only
  structure is being sold goes dark at the start of the sale.
* A player with more than 65,535 discs: the surplus sees nothing.
